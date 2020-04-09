use crate::{
    model,
    persistence::{self, TableAccess},
    Error, Result,
};
use bytesize::ByteSize;
use tokio::io::AsyncWriteExt;

use crate::utils::timeout_after;
use async_trait::async_trait;
use futures::FutureExt;
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

const CONNECT_AND_FETCH_HEAD_TIMEOUT: Duration = Duration::from_secs(15);
const FETCH_CHUNK_TIMEOUT_SECONDS: Duration = Duration::from_secs(10);

struct ProcessingState {
    url: String,
    kind: &'static str,
    output_file_path: PathBuf,
    result_key: Option<String>,
}
pub struct Agent<Fn, FnResult> {
    client: reqwest::Client,
    results: persistence::TaskResultTable,
    channel: async_std::sync::Sender<FnResult>,
    state: Option<ProcessingState>,
    make_state: Fn,
    next_action_state: Option<FnResult>,
}

impl<Fn, FnResult> Agent<Fn, FnResult>
where
    Fn: FnMut(Option<(String, String)>, &model::Task, &Path) -> Option<FnResult>,
{
    pub fn new(
        db: &persistence::Db,
        channel: async_std::sync::Sender<FnResult>,
        make_state: Fn,
    ) -> Result<Agent<Fn, FnResult>> {
        let client = reqwest::ClientBuilder::new().gzip(true).build()?;

        let results = db.open_results()?;
        Ok(Agent {
            client,
            results,
            channel,
            state: None,
            next_action_state: None,
            make_state,
        })
    }
}

#[async_trait]
impl<Fn, FnResult> crate::engine::work::generic::Processor for Agent<Fn, FnResult>
where
    Fn: FnMut(Option<(String, String)>, &model::Task, &Path) -> Option<FnResult> + Send,
    FnResult: Send,
{
    type Item = DownloadRequest;

    fn set(
        &mut self,
        request: Self::Item,
        progress: &mut prodash::tree::Item,
    ) -> Result<(model::Task, String, String)> {
        progress.init(None, None);
        match request {
            DownloadRequest {
                output_file_path,
                progress_name,
                task_key,
                crate_name_and_version,
                kind,
                url,
            } => {
                let dummy_task = default_persisted_download_task();
                let progress_name = format!("↓ {}", progress_name);

                let task_result = model::TaskResult::Download {
                    kind: kind.to_owned(),
                    url: String::new(),
                    content_length: 0,
                    content_type: None,
                };

                self.next_action_state = (self.make_state)(
                    crate_name_and_version.clone(),
                    &dummy_task,
                    &output_file_path,
                );
                self.state = Some(ProcessingState {
                    url,
                    kind,
                    output_file_path,
                    result_key: crate_name_and_version.as_ref().map(
                        |(crate_name, crate_version)| {
                            let mut result_key = String::with_capacity(task_key.len() * 2);
                            task_result.fq_key(
                                &crate_name,
                                &crate_version,
                                &dummy_task,
                                &mut result_key,
                            );
                            result_key
                        },
                    ),
                });
                Ok((dummy_task, task_key, progress_name))
            }
        }
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
            output_file_path,
            result_key,
        } = self.state.take().expect("initialized state");
        download_file_and_store_result(
            progress,
            result_key,
            &self.results,
            &self.client,
            kind,
            &url,
            output_file_path,
        )
        .await
        .map_err(|err| (err, format!("Failed to download '{}'", url)))
    }

    async fn schedule_next(&mut self, progress: &mut prodash::tree::Item) -> Result<()> {
        if let Some(request) = self.next_action_state.take() {
            progress.blocked("schedule crate extraction", None);
            // Here we risk doing this work twice, but most of the time, we don't. And since it's fast,
            // we take the risk of duplicate work for keeping more processors busy.
            // NOTE: We assume there is no risk of double-scheduling, also we assume the consumer is faster
            // then the producer (us), so we are ok with blocking until the task is scheduled.
            self.channel.send(request).await;
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct DownloadRequest {
    pub output_file_path: PathBuf,
    pub progress_name: String,
    pub task_key: String,
    pub crate_name_and_version: Option<(String, String)>,
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
    result_key: Option<String>,
    results: &persistence::TaskResultTable,
    client: &reqwest::Client,
    kind: &str,
    url: &str,
    out_file: PathBuf,
) -> Result<()> {
    tokio::fs::create_dir_all(&out_file.parent().expect("parent directory")).await?;

    // NOTE: We assume that the files we download never change, and we assume the server supports resumption!
    let (start_byte, truncate) = if let Ok(existing_meta) = tokio::fs::metadata(&out_file).await {
        (existing_meta.len(), false)
    } else {
        (0, true)
    };

    progress.blocked("fetch HEAD", None);
    let mut response = timeout_after(
        CONNECT_AND_FETCH_HEAD_TIMEOUT,
        "fetching HEAD",
        client
            .get(url)
            .header(http::header::RANGE, format!("bytes={}-", start_byte))
            .send(),
    )
    .await??;

    match response.status().as_u16() {
        200..=299 => {}
        416 => {
            // we assume that this means we have fully downloaded the item previously, and that the DB result was written already
            // but not checked
            progress.done(format!(
                "GET{}:{}: body-size = {}",
                if start_byte != 0 {
                    "(resumed, already completed)"
                } else {
                    ""
                },
                url,
                ByteSize(start_byte as u64)
            ));
            return Ok(());
        }
        _ => return Err(Error::HttpStatus(response.status())),
    };

    let remaining_content_length = response
        .content_length()
        .ok_or(Error::InvalidHeader("expected content-length"))?;

    let content_length = (start_byte + remaining_content_length) as u32;
    progress.init(Some(content_length / 1024), Some("Kb"));
    progress.done(format!(
        "HEAD{}:{}: content-length = {}",
        if start_byte != 0 { "(resumable)" } else { "" },
        url,
        ByteSize(content_length.into())
    ));

    if remaining_content_length != 0 {
        let mut out = tokio::fs::OpenOptions::new()
            .create(truncate)
            .truncate(truncate)
            .write(truncate)
            .append(!truncate)
            .open(&out_file)
            .await
            .map_err(|err| {
                crate::Error::Message(format!("Failed to open '{}': {}", out_file.display(), err))
            })?;

        let mut bytes_received = start_byte as usize;
        while let Some(chunk) = timeout_after(
            FETCH_CHUNK_TIMEOUT_SECONDS,
            format!(
                "fetched {} of {}",
                ByteSize(bytes_received as u64),
                ByteSize(content_length.into())
            ),
            response.chunk().boxed(),
        )
        .await??
        {
            out.write(&chunk).await?;
            bytes_received += chunk.len();
            progress.set((bytes_received / 1024) as u32);
        }
        progress.done(format!(
            "GET{}:{}: body-size = {}",
            if start_byte != 0 { "(resumed)" } else { "" },
            url,
            ByteSize(bytes_received as u64)
        ));
        out.flush().await?;
    } else {
        progress.done(format!("{} already on disk - skipping", url))
    }

    if let Some(result_key) = result_key {
        let task_result = model::TaskResult::Download {
            kind: kind.to_owned(),
            url: url.to_owned(),
            content_length,
            content_type: response
                .headers()
                .get(http::header::CONTENT_TYPE)
                .and_then(|t| t.to_str().ok())
                .map(Into::into),
        };
        results.insert(progress, &result_key, &task_result)?;
    }
    Ok(())
}
