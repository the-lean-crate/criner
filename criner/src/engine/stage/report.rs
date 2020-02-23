use crate::engine::work;
use crate::{
    engine::report,
    error::Result,
    model,
    persistence::{self, TreeAccess},
    utils::check,
};
use futures::{
    task::{Spawn, SpawnExt},
    FutureExt,
};
use itertools::Itertools;
use std::{path::PathBuf, time::SystemTime};

pub async fn generate(
    db: persistence::Db,
    mut progress: prodash::tree::Item,
    assets_dir: PathBuf,
    deadline: Option<SystemTime>,
    cpu_o_bound_processors: u32,
    pool: impl Spawn + Clone + Send + 'static + Sync,
) -> Result<()> {
    let krates = db.crates();
    let chunk_size = 500;
    let output_dir = assets_dir
        .parent()
        .expect("assets directory to be in criner.db")
        .join("reports");
    let mut waste_aggregator = report::waste::Generator { db: db.clone() };
    let waste_report_dir = output_dir.join("waste");

    std::fs::create_dir_all(&waste_report_dir)?;
    let num_crates = krates.tree().len() as u32;
    progress.init(Some(num_crates), Some("crates"));

    let (_tx, rx) = async_std::sync::channel(1);
    for idx in 0..cpu_o_bound_processors {
        pool.spawn(
            work::cpubound::processor(
                db.clone(),
                progress.add_child(format!("🏋 - {} idle", idx + 1)),
                rx.clone(),
                assets_dir.clone(),
            )
            .map(|_| ()),
        )?;
    }

    for (cid, chunk) in krates
        .tree()
        .iter()
        .filter_map(|res| res.ok())
        .map(|(k, v)| (k, model::Crate::from(v)))
        .chunks(chunk_size)
        .into_iter()
        .enumerate()
    {
        check(deadline.clone())?;
        progress.set(((cid + 1) * chunk_size) as u32);
        progress.blocked(None);
        waste_aggregator.write_files(
            &waste_report_dir,
            chunk,
            progress.add_child("waste report"),
        )?;
    }
    progress.set(num_crates);
    Ok(())
}
