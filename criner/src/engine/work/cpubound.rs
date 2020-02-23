use crate::error::Result;
use crate::persistence;
use std::path::PathBuf;

pub struct Request;

pub async fn processor(
    db: persistence::Db,
    mut progress: prodash::tree::Item,
    r: async_std::sync::Receiver<Request>,
    assets_dir: PathBuf,
) -> Result<()> {
    Ok(())
}
