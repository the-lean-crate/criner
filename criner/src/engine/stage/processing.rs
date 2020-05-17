use crate::persistence::{new_value_query_recent_first, value_iter, CrateVersionTable};
use crate::{
    engine::work,
    error::Result,
    model::CrateVersion,
    persistence::{Db, Keyed, TableAccess},
};
use futures_util::FutureExt;
use std::{path::PathBuf, time::SystemTime};

pub async fn process(
    db: Db,
    mut progress: prodash::tree::Item,
    io_bound_processors: u32,
    cpu_bound_processors: u32,
    mut processing_progress: prodash::tree::Item,
    assets_dir: PathBuf,
    startup_time: SystemTime,
) -> Result<()> {
    processing_progress.set_name("Downloads and Extractors");
    let tx_cpu = {
        let (tx_cpu, rx) = piper::chan(1);
        for idx in 0..cpu_bound_processors {
            let max_retries_on_timeout = 0;
            smol::Task::blocking(
                work::generic::processor(
                    db.clone(),
                    processing_progress.add_child(format!("{}:CPU IDLE", idx + 1)),
                    rx.clone(),
                    work::cpubound::Agent::new(assets_dir.clone(), &db)?,
                    max_retries_on_timeout,
                )
                .map(|r| {
                    if let Err(e) = r {
                        log::warn!("CPU bound processor failed: {}", e);
                    }
                }),
            )
            .detach();
        }
        tx_cpu
    };

    let tx_io = {
        let (tx_io, rx) = piper::chan(1);
        for idx in 0..io_bound_processors {
            let max_retries_on_timeout = 40;
            smol::Task::spawn(
                work::generic::processor(
                    db.clone(),
                    processing_progress.add_child(format!("{}: ↓ IDLE", idx + 1)),
                    rx.clone(),
                    work::iobound::Agent::new(
                        &db,
                        tx_cpu.clone(),
                        |crate_name_and_version, task, _| {
                            crate_name_and_version.map(|(crate_name, crate_version)| {
                                work::cpubound::ExtractRequest {
                                    download_task: task.clone(),
                                    crate_name,
                                    crate_version,
                                }
                            })
                        },
                    )?,
                    max_retries_on_timeout,
                )
                .map(|r| {
                    if let Err(e) = r {
                        log::warn!("iobound processor failed: {}", e);
                    }
                }),
            )
            .detach();
        }
        tx_io
    };

    let versions = db.open_crate_versions()?;
    let num_versions = versions.count();
    progress.init(Some(num_versions as u32), Some("crate versions"));

    let auto_checkpoint_every = 10000;
    let checkpoint_connection = db.open_connection_with_busy_wait()?;
    let mut fetched_versions = 0;
    let mut versions = Vec::with_capacity(auto_checkpoint_every);
    let mut last_elapsed_for_checkpointing = None;

    loop {
        let abort_loop = {
            progress.blocked("fetching chunk of version to schedule", None);
            let connection = db.open_connection_no_async_with_busy_wait()?;
            let mut statement = new_value_query_recent_first(
                CrateVersionTable::table_name(),
                &connection,
                fetched_versions,
                auto_checkpoint_every,
            )?;
            let iter = value_iter::<CrateVersion>(&mut statement)?;
            versions.clear();
            versions.extend(iter);
            fetched_versions += versions.len();

            versions.len() != auto_checkpoint_every
        };

        let tasks = db.open_tasks()?;
        for (vid, version) in versions.drain(..).enumerate() {
            let version = version?;

            progress.set((vid + fetched_versions + 1) as u32);
            progress.halted("wait for task consumers", None);
            work::schedule::tasks(
                &assets_dir,
                &tasks,
                &version,
                progress.add_child(format!("schedule {}", version.key())),
                work::schedule::Scheduling::AtLeastOne,
                &tx_io,
                &tx_cpu,
                startup_time,
            )
            .await?;
        }

        // We have too many writers which cause the WAL to get so large that all reads are slowing to a crawl
        // Standard SQLITE autocheckpoints are passive, which are not effective in our case as they never
        // kick in with too many writers. There is no way to change the autocheckpoint mode to something more suitable… :/
        progress.blocked(
            "checkpointing database",
            last_elapsed_for_checkpointing
                .clone()
                .map(|d| SystemTime::now() + d),
        );
        let start = SystemTime::now();
        checkpoint_connection
            .lock()
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        last_elapsed_for_checkpointing = Some(SystemTime::now().duration_since(start)?);

        if abort_loop {
            break;
        }
    }
    Ok(())
}
