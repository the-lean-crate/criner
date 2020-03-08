use crate::{
    error::{Error, Result},
    model, persistence,
    persistence::TableAccess,
};
use bytesize::ByteSize;
use std::{path::Path, path::PathBuf, time::SystemTime};
use tokio::io::AsyncWriteExt;

use crate::model::Task;
use async_trait::async_trait;

struct ProcessingState {
    url: String,
    kind: &'static str,
    base_dir: PathBuf,
    out_file: PathBuf,
    key: String,
}
pub struct Agent {
    asset_dir: PathBuf,
    client: reqwest::Client,
    results: persistence::TaskResultTable,
    channel: async_std::sync::Sender<super::cpubound::ExtractRequest>,
    state: Option<ProcessingState>,
    extraction_request: Option<super::cpubound::ExtractRequest>,
}

impl Agent {
    pub fn new(
        assets_dir: impl Into<PathBuf>,
        db: &persistence::Db,
        channel: async_std::sync::Sender<super::cpubound::ExtractRequest>,
    ) -> Result<Agent> {
        let client = reqwest::ClientBuilder::new()
            .connect_timeout(std::time::Duration::from_secs(120))
            .gzip(true)
            .build()?;

        let results = db.open_results()?;
        Ok(Agent {
            asset_dir: assets_dir.into(),
            client,
            results,
            channel,
            state: None,
            extraction_request: None,
        })
    }
}

#[async_trait]
impl crate::engine::work::generic::Processor for Agent {
    type Item = DownloadRequest;

    fn set(
        &mut self,
        request: Self::Item,
        out_key: &mut String,
        progress: &mut prodash::tree::Item,
    ) -> Result<(Task, String)> {
        progress.init(None, None);
        match request {
            DownloadRequest {
                crate_name,
                crate_version,
                kind,
                url,
            } => {
                let dummy_task = default_persisted_download_task();
                let progress_message = format!("↓ {}:{}", crate_name, crate_version);

                dummy_task.fq_key(&crate_name, &crate_version, out_key);
                let task_result = model::TaskResult::Download {
                    kind: kind.to_owned(),
                    url: String::new(),
                    content_length: 0,
                    content_type: None,
                };
                let mut key = String::with_capacity(out_key.len() * 2);
                task_result.fq_key(&crate_name, &crate_version, &dummy_task, &mut key);
                let base_dir = crate_version_dir(&self.asset_dir, &crate_name, &crate_version);
                let out_file =
                    download_file_path(&dummy_task.process, &dummy_task.version, kind, &base_dir);
                self.state = Some(ProcessingState {
                    url,
                    kind,
                    base_dir,
                    out_file,
                    key,
                });
                self.extraction_request = Some(super::cpubound::ExtractRequest {
                    download_task: dummy_task.clone(),
                    crate_name,
                    crate_version,
                });
                Ok((dummy_task, progress_message))
            }
        }
    }

    async fn schedule_next(
        &mut self,
        progress: &mut prodash::tree::Item,
    ) -> std::result::Result<(), Error> {
        let extract_request = self
            .extraction_request
            .take()
            .expect("this to be set when we are called");
        progress.blocked("schedule crate extraction", None);
        // Here we risk doing this work twice, but must of the time, we don't. And since it's fast,
        // we take the risk of duplicate work for keeping more precessors busy.
        // And yes, this send is blocking the source processor, but should not be an issue as CPU
        // processors are so fast - slow producer, fast consumer.
        self.channel.send(extract_request).await;
        Ok(())
    }

    fn idle_message(&self) -> String {
        "↓ IDLE".into()
    }

    async fn process(
        &mut self,
        progress: &mut prodash::tree::Item,
    ) -> std::result::Result<(), (Error, String)> {
        let ProcessingState {
            url,
            kind,
            base_dir,
            out_file,
            key,
        } = self.state.take().expect("initialized state");
        download_file_and_store_result(
            progress,
            &key,
            &self.results,
            &self.client,
            kind,
            &url,
            base_dir,
            out_file,
        )
        .await
        .map_err(|err| (err, format!("Failed to download '{}'", url)))
    }
}

pub struct DownloadRequest {
    pub crate_name: String,
    pub crate_version: String,
    pub kind: &'static str,
    pub url: String,
}

pub fn default_persisted_download_task() -> model::Task {
    const TASK_NAME: &str = "download";
    const TASK_VERSION: &str = "1.0.0";
    model::Task {
        stored_at: SystemTime::now(),
        process: TASK_NAME.into(),
        version: TASK_VERSION.into(),
        state: Default::default(),
    }
}

async fn download_file_and_store_result(
    progress: &mut prodash::tree::Item,
    key: &str,
    results: &persistence::TaskResultTable,
    client: &reqwest::Client,
    kind: &str,
    url: &str,
    base_dir: PathBuf,
    out_file: PathBuf,
) -> Result<()> {
    progress.blocked("fetch HEAD", None);
    let mut res = client.get(url).send().await?;
    let size: u32 = res
        .content_length()
        .ok_or(Error::InvalidHeader("expected content-length"))? as u32;
    progress.init(Some(size / 1024), Some("Kb"));
    progress.done(format!(
        "HEAD:{}: content-length = {}",
        url,
        ByteSize(size.into())
    ));

    let mut bytes_received = 0;
    tokio::fs::create_dir_all(&base_dir).await?;
    let mut out = tokio::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(out_file)
        .await?;

    while let Some(chunk) = res.chunk().await? {
        out.write(&chunk).await?;
        // body_buf.extend(chunk);
        bytes_received += chunk.len();
        progress.set((bytes_received / 1024) as u32);
    }
    progress.done(format!(
        "GET:{}: body-size = {}",
        url,
        ByteSize(bytes_received as u64)
    ));

    let task_result = model::TaskResult::Download {
        kind: kind.to_owned(),
        url: url.to_owned(),
        content_length: size,
        content_type: res
            .headers()
            .get(http::header::CONTENT_TYPE)
            .and_then(|t| t.to_str().ok())
            .map(Into::into),
    };
    results.insert(progress, &key, &task_result)?;
    Ok(())
}

pub fn download_file_path(process: &str, version: &str, kind: &str, base_dir: &Path) -> PathBuf {
    base_dir.join(format!(
        "{process}{sep}{version}.{kind}",
        process = process,
        sep = crate::persistence::KEY_SEP_CHAR,
        version = version,
        kind = kind
    ))
}

pub fn crate_version_dir(assets_dir: &Path, crate_name: &str, crate_version: &str) -> PathBuf {
    assets_dir.join(crate_name).join(crate_version)
}
