use crate::utils::check;
use crate::{
    engine::report,
    error::Result,
    model,
    persistence::{Db, TreeAccess},
};
use itertools::Itertools;
use std::{path::PathBuf, time::SystemTime};

pub async fn generate(
    db: Db,
    mut progress: prodash::tree::Item,
    assets_dir: PathBuf,
    deadline: Option<SystemTime>,
) -> Result<()> {
    let krates = db.crates();
    let chunk_size = 500;
    let output_dir = assets_dir.clone();
    let mut waste_aggregator = report::waste::Generator;
    progress.init(Some(krates.tree().len() as u32), Some("crates"));

    for (cid, chunk) in krates
        .tree()
        .iter()
        .filter_map(|res| res.ok())
        .map(|(_k, v)| model::Crate::from(v))
        .chunks(chunk_size)
        .into_iter()
        .enumerate()
    {
        check(deadline.clone())?;
        progress.set(((cid + 1) * chunk_size) as u32);
        progress.blocked(None);
        waste_aggregator.write_files(output_dir.join("waste"), chunk)?;
    }
    Ok(())
}
