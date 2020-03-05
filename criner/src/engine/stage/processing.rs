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
use rusqlite::NO_PARAMS;
use std::{path::PathBuf, time::SystemTime};

pub async fn process(
    db: Db,
    mut progress: prodash::tree::Item,
    io_bound_processors: u32,
    cpu_bound_processors: u32,
    mut download_progress: prodash::tree::Item,
    tokio: tokio::runtime::Handle,
    pool: impl Spawn + Clone + Send + 'static + Sync,
    assets_dir: PathBuf,
    startup_time: SystemTime,
) -> Result<()> {
    let (tx_io, rx) = async_std::sync::channel(1);
    for idx in 0..io_bound_processors {
        // Can only use the pool if the downloader uses a futures-compatible runtime
        // Tokio is its very own thing, and futures requiring it need to run there.
        tokio.spawn(
            work::generic::processor(
                db.clone(),
                download_progress.add_child(format!("{}: ↓ IDLE", idx + 1)),
                rx.clone(),
                work::iobound::Agent::new(assets_dir.clone(), &db)?,
            )
            .map(|r| {
                if let Err(e) = r {
                    log::error!("iobound processor failed: {}", e);
                }
            }),
        );
    }
    let (tx_cpu, rx) = async_std::sync::channel(1);
    for idx in 0..cpu_bound_processors {
        pool.spawn(
            work::cpubound::processor(
                db.clone(),
                download_progress.add_child(format!("{}:CPU IDLE", idx + 1)),
                rx.clone(),
                assets_dir.clone(),
            )
            .map(|r| {
                if let Err(e) = r {
                    log::error!("CPU bound processor failed: {}", e);
                }
                ()
            }),
        )?;
    }

    let versions = db.open_crate_versions()?;
    let num_versions = versions.count();
    let guard = versions.connection().lock();
    let mut statement = guard.prepare(&format!(
        "SELECT data FROM {} ORDER BY _rowid_ DESC",
        versions.table_name()
    ))?;

    let mut rows = statement.query(NO_PARAMS)?;

    progress.init(Some(num_versions as u32), Some("crate versions"));
    let mut vid = 0;
    while let Some(r) = rows.next()? {
        let version: Vec<u8> = r.get(0)?;
        let version = CrateVersion::from(version.as_slice());

        progress.set((vid + 1) as u32);
        progress.blocked(None);
        work::schedule::tasks(
            db.open_tasks()?,
            &version,
            progress.add_child(format!("schedule {}", version.key())),
            work::schedule::Scheduling::AtLeastOne,
            &tx_io,
            &tx_cpu,
            startup_time,
        )
        .await?;
        vid += 1;
    }
    Ok(())
}
