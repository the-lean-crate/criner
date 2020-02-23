use crate::{
    engine::work,
    error::Result,
    model,
    persistence::{Db, Keyed, TreeAccess},
};
use futures::task::{Spawn, SpawnExt};
use futures::FutureExt;
use std::path::PathBuf;

pub async fn process(
    db: Db,
    mut progress: prodash::tree::Item,
    io_bound_processors: u32,
    cpu_bound_processors: u32,
    mut download_progress: prodash::tree::Item,
    tokio: tokio::runtime::Handle,
    pool: impl Spawn,
    assets_dir: PathBuf,
) -> Result<()> {
    let (tx_io, rx) = async_std::sync::channel(1);
    for idx in 0..io_bound_processors {
        // Can only use the pool if the downloader uses a futures-compatible runtime
        // Tokio is its very own thing, and futures requiring it need to run there.
        tokio.spawn(
            work::iobound::processor(
                db.clone(),
                download_progress.add_child(format!("↓ {} - idle", idx + 1)),
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
                download_progress.add_child(format!("🏋️‍ {} - idle", idx + 1)),
                rx.clone(),
                assets_dir.clone(),
            )
            .map(|_| ()),
        )?;
    }

    let versions = db.crate_versions();
    let mut ofs = 0;
    loop {
        let chunk = {
            let tree_iter = versions.tree().iter();
            tree_iter.skip(ofs).take(1000).collect::<Vec<_>>()
        };
        progress.init(Some((ofs + chunk.len()) as u32), Some("crate version"));
        if chunk.is_empty() {
            return Ok(());
        }
        let chunk_len = chunk.len();
        for (idx, res) in chunk.into_iter().enumerate() {
            let (_key, value) = res?;
            progress.set((ofs + idx + 1) as u32);
            let version: model::CrateVersion = value.into();

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
        ofs += chunk_len;
    }
}
