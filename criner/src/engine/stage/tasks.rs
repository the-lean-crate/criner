use crate::{
    engine::work,
    error::Result,
    model::CrateVersion,
    persistence::{Db, Keyed, TreeAccess},
};
use futures::{
    task::{Spawn, SpawnExt},
    FutureExt,
};
use std::path::PathBuf;

pub async fn process(
    db: Db,
    mut progress: prodash::tree::Item,
    io_bound_processors: u32,
    cpu_bound_processors: u32,
    mut download_progress: prodash::tree::Item,
    tokio: tokio::runtime::Handle,
    pool: impl Spawn + Clone + Send + 'static + Sync,
    assets_dir: PathBuf,
) -> Result<()> {
    let (tx_io, rx) = async_std::sync::channel(1);
    for idx in 0..io_bound_processors {
        // Can only use the pool if the downloader uses a futures-compatible runtime
        // Tokio is its very own thing, and futures requiring it need to run there.
        tokio.spawn(
            work::iobound::processor(
                db.clone(),
                download_progress.add_child(format!("{}: ↓ 🔆", idx + 1)),
                rx.clone(),
                assets_dir.clone(),
            )
            .map(|_| ()),
        );
    }
    let (tx_cpu, rx) = async_std::sync::channel(1);
    for idx in 0..cpu_bound_processors {
        pool.spawn(
            work::cpubound::processor(
                db.clone(),
                download_progress.add_child(format!("{}:🏋 🔆", idx + 1)),
                rx.clone(),
                assets_dir.clone(),
            )
            .map(|_| ()),
        )?;
    }

    let versions = db.crate_versions();
    let num_versions = versions.tree().len();
    progress.init(Some(num_versions as u32), Some("crate versions"));
    for (vid, version) in versions
        .tree()
        .iter()
        .filter_map(|r| r.ok())
        .map(|(_k, v)| CrateVersion::from(v))
        .enumerate()
    {
        progress.set((vid + 1) as u32);
        progress.blocked(None);
        work::schedule::tasks(
            db.tasks(),
            &version,
            progress.add_child(format!("schedule {}", version.key_string()?)),
            work::schedule::Scheduling::AtLeastOne,
            &tx_io,
            &tx_cpu,
        )
        .await?;
    }
    Ok(())
}
