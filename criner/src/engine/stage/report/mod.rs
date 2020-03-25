use crate::persistence::new_key_value_query_old_to_new_filtered;
use crate::{
    engine::{report, work},
    error::Result,
    persistence::{self, TableAccess},
    utils::check,
};
use futures::{task::Spawn, task::SpawnExt, FutureExt};
use rusqlite::NO_PARAMS;
use std::{path::PathBuf, time::SystemTime};

mod git;

pub async fn generate(
    db: persistence::Db,
    mut progress: prodash::tree::Item,
    assets_dir: PathBuf,
    glob: Option<String>,
    deadline: Option<SystemTime>,
    cpu_o_bound_processors: u32,
    pool: impl Spawn + Clone + Send + 'static + Sync,
) -> Result<()> {
    use report::generic::Generator;
    let krates = db.open_crates()?;
    let output_dir = assets_dir
        .parent()
        .expect("assets directory to be in criner.db")
        .join("reports");
    let glob_str = glob.as_ref().map(|s| s.as_str());
    let num_crates = krates.count_filtered(glob_str.clone()) as u32;
    let chunk_size = 500.min(num_crates);
    progress.init(Some(num_crates), Some("crates"));

    let (rx_result, tx) = {
        let (tx, rx) = async_std::sync::channel(1);
        let (tx_result, rx_result) =
            async_std::sync::channel((cpu_o_bound_processors * 2) as usize);
        for _ in 0..cpu_o_bound_processors {
            pool.spawn(work::simple::processor(rx.clone(), tx_result.clone()).map(|_| ()))?;
        }
        (rx_result, tx)
    };

    let waste_report_dir = output_dir.join(report::waste::Generator::name());
    async_std::fs::create_dir_all(&waste_report_dir).await?;
    // DEBUG: allow using git while using globs
    // use crate::engine::report::generic::WriteCallback;
    let (cache_dir, (git_handle, git_state, maybe_join_handle)) = match glob.as_ref() {
        // Some(_) => (None, (git::not_available as WriteCallback, None, None)),
        Some(_) => (
            None,
            git::select_callback(
                cpu_o_bound_processors,
                &waste_report_dir,
                progress.add_child("git"),
            ),
        ),
        None => {
            let cd = waste_report_dir.join("__incremental_cache__");
            async_std::fs::create_dir_all(&cd).await?;
            (
                Some(cd),
                git::select_callback(
                    cpu_o_bound_processors,
                    &waste_report_dir,
                    progress.add_child("git"),
                ),
            )
        }
    };
    let merge_reports = pool.spawn_with_handle({
        let mut merge_progress = progress.add_child("report aggregator");
        merge_progress.init(Some(num_crates / chunk_size), Some("Reports"));
        report::waste::Generator::merge_reports(
            waste_report_dir.clone(),
            cache_dir.clone(),
            merge_progress,
            rx_result,
            git_handle,
            git_state.clone(),
        )
        .map(|_| ())
        .boxed()
    })?;

    let mut fetched_crates = 0;
    let mut chunk = Vec::<(String, Vec<u8>)>::with_capacity(chunk_size as usize);
    let mut cid = 0;
    loop {
        let abort_loop = {
            progress.blocked("fetching chunk of crates to schedule", None);
            let mut connection = db.open_connection_no_async_with_busy_wait()?;
            let mut statement = new_key_value_query_old_to_new_filtered(
                persistence::CrateTable::table_name(),
                glob_str,
                &mut connection,
                Some((fetched_crates, chunk_size as usize)),
            )?;

            chunk.clear();
            chunk.extend(
                statement
                    .query_map(NO_PARAMS, |r| Ok((r.get(0)?, r.get(1)?)))?
                    .filter_map(|r| r.ok()),
            );
            fetched_crates += chunk.len();

            chunk.len() != chunk_size as usize
        };

        cid += 1;
        check(deadline.clone())?;

        progress.set((cid * chunk_size) as u32);
        progress.halted("write crate report", None);
        tx.send(
            report::waste::Generator::write_files(
                db.clone(),
                waste_report_dir.clone(),
                cache_dir.clone(),
                chunk,
                progress.add_child(""),
                git_handle,
                git_state.clone(),
            )
            .boxed(),
        )
        .await;
        chunk = Vec::with_capacity(chunk_size as usize);
        if abort_loop {
            break;
        }
    }
    drop(git_state);
    drop(tx);
    progress.set(num_crates);
    merge_reports.await;
    progress.done("Generating and merging waste report done");

    if let Some(handle) = maybe_join_handle {
        progress.blocked("waiting for git to finish", None);
        if handle.join().is_err() {
            progress.fail("git failed with unknown error");
        }
    };
    Ok(())
}
