use crate::persistence::new_key_value_query_old_to_new_filtered;
use crate::{
    engine::{report, work},
    error::Result,
    persistence::{self, TableAccess},
    utils::check,
};
use futures_util::future::FutureExt;
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
) -> Result<()> {
    use report::generic::Generator;
    let krates = db.open_crates()?;
    let output_dir = assets_dir
        .parent()
        .expect("assets directory to be in criner.db")
        .join("reports");
    let glob_str = glob.as_deref();
    let num_crates = krates.count_filtered(glob_str.clone()) as u32;
    let chunk_size = 500.min(num_crates);
    if chunk_size == 0 {
        return Ok(());
    }
    progress.init(Some(num_crates), Some("crates"));

    let (rx_result, tx) = {
        let (tx, rx) = piper::chan(1);
        let (tx_result, rx_result) = piper::chan((cpu_o_bound_processors * 2) as usize);
        // TODO: use task span with a bounded channel instead - no need for this kind of 'simple' agent
        for _ in 0..cpu_o_bound_processors {
            smol::Task::spawn(work::simple::processor(rx.clone(), tx_result.clone()).map(|_| ()))
                .detach();
        }
        (rx_result, tx)
    };

    let waste_report_dir = output_dir.join(report::waste::Generator::name());
    {
        let dir = waste_report_dir.clone();
        smol::blocking!(std::fs::create_dir_all(dir))?;
    }
    use crate::engine::report::generic::WriteCallback;
    let (cache_dir, (git_handle, git_state, maybe_join_handle)) = match glob.as_ref() {
        Some(_) => (None, (git::not_available as WriteCallback, None, None)),
        None => {
            let cd = waste_report_dir.join("__incremental_cache__");
            {
                let cd = cd.clone();
                smol::blocking!(std::fs::create_dir_all(cd))?;
            }
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
    let merge_reports = smol::Task::spawn({
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
    });

    let mut fetched_crates = 0;
    let mut chunk = Vec::<(String, Vec<u8>)>::with_capacity(chunk_size as usize);
    let mut cid = 0;
    loop {
        let abort_loop = {
            progress.blocked("fetching chunk of crates to schedule", None);
            let connection = db.open_connection_no_async_with_busy_wait()?;
            let mut statement = new_key_value_query_old_to_new_filtered(
                persistence::CrateTable::table_name(),
                glob_str,
                &connection,
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
        check(deadline)?;

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
