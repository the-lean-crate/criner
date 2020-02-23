use crate::error::Result;
use crate::persistence::TreeAccess;
use crate::{model, persistence};
use std::path::PathBuf;
use std::time::SystemTime;

pub struct ExtractRequest {
    pub download_task: model::TaskOwned,
    pub crate_name: String,
    pub crate_version: String,
}

pub fn default_persisted_download_task() -> model::Task<'static> {
    const TASK_NAME: &str = "extract_crate";
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
    r: async_std::sync::Receiver<ExtractRequest>,
    assets_dir: PathBuf,
) -> Result<()> {
    let mut key = Vec::with_capacity(32);
    let tasks = db.tasks();
    let mut dummy = default_persisted_download_task();

    while let Some(ExtractRequest {
        download_task,
        crate_name,
        crate_version,
    }) = r.recv().await
    {
        progress.set_name(format!("🏋️ ‍{}:{}", crate_name, crate_version));
        progress.init(None, Some("files"));

        let kt = (crate_name.as_str(), crate_version.as_str(), dummy);
        key.clear();

        persistence::TasksTree::key_to_buf(&kt, &mut key);
        dummy = kt.2;

        let task = tasks.update(&key, |t| {
            ({
                t.process = dummy.process.clone();
                t.version = dummy.version.clone()
            })
        })?;

        let downloaded_crate = {
            let crate_version_dir =
                super::iobound::crate_version_dir(&assets_dir, &crate_name, &crate_version);
            super::iobound::download_file_path(
                &download_task.process,
                &download_task.version,
                "crate",
                &crate_version_dir,
            );
        };

        progress.set_name("🏋️‍ idle");
    }

    Ok(())
}
