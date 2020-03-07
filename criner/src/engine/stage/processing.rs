use crate::persistence::{new_value_query, value_iter, CrateVersionsTree};
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
use std::{path::PathBuf, time::SystemTime};

pub async fn process(
    db: Db,
    mut progress: prodash::tree::Item,
    io_bound_processors: u32,
    cpu_bound_processors: u32,
    mut processing_progress: prodash::tree::Item,
    tokio: tokio::runtime::Handle,
    pool: impl Spawn + Clone + Send + 'static + Sync,
    assets_dir: PathBuf,
    startup_time: SystemTime,
) -> Result<()> {
    processing_progress.set_name("Downloads and Extractors");
    let tx_cpu = {
        let (tx_cpu, rx) = async_std::sync::channel(1);
        for idx in 0..cpu_bound_processors {
            pool.spawn(
                work::generic::processor(
                    db.clone(),
                    processing_progress.add_child(format!("{}:CPU IDLE", idx + 1)),
                    rx.clone(),
                    work::cpubound::Agent::new(assets_dir.clone(), &db)?,
                )
                .map(|r| {
                    if let Err(e) = r {
                        log::warn!("CPU bound processor failed: {}", e);
                    }
                    ()
                }),
            )?;
        }
        tx_cpu
    };

    let tx_io = {
        let (tx_io, rx) = async_std::sync::channel(1);
        for idx in 0..io_bound_processors {
            // Can only use the pool if the downloader uses a futures-compatible runtime
            // Tokio is its very own thing, and futures requiring it need to run there.
            tokio.spawn(
                work::generic::processor(
                    db.clone(),
                    processing_progress.add_child(format!("{}: ↓ IDLE", idx + 1)),
                    rx.clone(),
                    work::iobound::Agent::new(assets_dir.clone(), &db, tx_cpu.clone())?,
                )
                .map(|r| {
                    if let Err(e) = r {
                        log::warn!("iobound processor failed: {}", e);
                    }
                }),
            );
        }
        tx_io
    };

    let versions = db.open_crate_versions()?;
    let num_versions = versions.count();

    let connection = versions.into_connection();
    let mut guard = connection.lock();
    let mut statement = new_value_query(CrateVersionsTree::table_name(), &mut *guard)?;
    let iter = value_iter::<CrateVersion>(&mut statement)?;

    progress.init(Some(num_versions as u32), Some("crate versions"));
    let tasks = db.open_tasks()?;
    let mut vid = 0;
    for version in iter {
        let version = version?;

        progress.set((vid + 1) as u32);
        progress.blocked("wait for task consumers", None);
        work::schedule::tasks(
            &tasks,
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
