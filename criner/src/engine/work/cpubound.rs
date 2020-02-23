use crate::error::Result;
use crate::{model, persistence};
use std::path::PathBuf;
use std::time::SystemTime;

pub struct Request;

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
    _db: persistence::Db,
    _progress: prodash::tree::Item,
    _r: async_std::sync::Receiver<Request>,
    _assets_dir: PathBuf,
) -> Result<()> {
    Ok(())
}
