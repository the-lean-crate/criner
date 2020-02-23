use crate::{
    error::{Error, Result},
    model, persistence,
    persistence::TreeAccess,
};
use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

pub struct DownloadRequest {
    pub name: String,
    pub semver: String,
    pub kind: &'static str,
    pub url: String,
}

pub fn default_persisted_download_task() -> model::Task<'static> {
    const TASK_NAME: &str = "download";
    const TASK_VERSION: &str = "1.0.0";
    model::Task {
        stored_at: SystemTime::now(),
        process: TASK_NAME.into(),
        version: TASK_VERSION.into(),
        state: Default::default(),
    }
}

pub async fn processor(
    db: persistence::Db,
    mut progress: prodash::tree::Item,
    r: async_std::sync::Receiver<DownloadRequest>,
    assets_dir: PathBuf,
) -> Result<()> {
    let mut dummy = default_persisted_download_task();

    let mut key = Vec::with_capacity(32);
    let tasks = db.tasks();
    let mut body_buf = Vec::new();

    while let Some(DownloadRequest {
        name,
        semver,
        kind,
        url,
    }) = r.recv().await
    {
        progress.set_name(format!("↓ {}:{}", name, semver));
        progress.init(None, None);
        let mut kt = (name.as_str(), semver.as_str(), dummy);
        key.clear();

        persistence::TasksTree::key_to_buf(&kt, &mut key);
        dummy = kt.2;

        let mut task = tasks.update(&key, |_| ())?;
        task.process = dummy.process.clone();
        task.version = dummy.version.clone();

        progress.blocked(None);
        let res: Result<()> = async {
            {
                let mut res = reqwest::get(&url).await?;
                let size: u32 = res
                    .content_length()
                    .ok_or(Error::InvalidHeader("expected content-length"))?
                    as u32;
                progress.init(Some(size / 1024), Some("Kb"));
                progress.blocked(None);
                progress.done(format!("HEAD:{}: content-size = {}", url, size));
                body_buf.clear();
                while let Some(chunk) = res.chunk().await? {
                    body_buf.extend(chunk);
                    progress.set((body_buf.len() / 1024) as u32);
                }
                progress.done(format!("GET:{}: body-size = {}", url, body_buf.len()));

                {
                    key.clear();
                    let insert_item = (
                        name.as_str(),
                        semver.as_str(),
                        &task,
                        model::TaskResult::Download {
                            kind: kind.into(),
                            url: url.as_str().into(),
                            content_length: size,
                            content_type: res
                                .headers()
                                .get(http::header::CONTENT_TYPE)
                                .and_then(|t| t.to_str().ok())
                                .map(Into::into),
                        },
                    );
                    persistence::TaskResultTree::key_to_buf(&insert_item, &mut key);
                    store_data(&key, &body_buf, assets_dir.as_path()).await?;
                }
                Ok(())
            }
        }
        .await;

        task.state = match res {
            Ok(_) => model::TaskState::Complete,
            Err(err) => {
                progress.fail(format!("Failed to download '{}': {}", url, err));
                model::TaskState::AttemptsWithFailure(vec![err.to_string()])
            }
        };
        kt.2 = task;
        tasks.upsert(&kt)?;
        progress.set_name("↓ idle");
        progress.init(None, None);
    }
    progress.done("Shutting down…");
    Ok(())
}

async fn store_data(key: &[u8], data: &[u8], assets_dir: &Path) -> Result<()> {
    let key_str = String::from_utf8(key.to_owned())?;

    let tokens = key_str.split(':');
    assert_eq!(tokens.count(), 5);
    let mut tokens = key_str.split(':');
    let (name, version) = (tokens.next().unwrap(), tokens.next().unwrap());
    let (process, process_version, kind) = (
        tokens.next().unwrap(),
        tokens.next().unwrap(),
        tokens.next().unwrap(),
    );

    let base_dir = assets_dir.join(name).join(version);
    tokio::fs::create_dir_all(&base_dir).await?;
    tokio::fs::write(
        base_dir.join(format!(
            "{process}{sep}{version}.{kind}",
            process = process,
            sep = crate::persistence::KEY_SEP,
            version = process_version,
            kind = kind
        )),
        data,
    )
    .await
    .map_err(Into::into)
}
