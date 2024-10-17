use crate::{
    engine::report,
    persistence::{self, new_key_value_query_old_to_new_filtered, TableAccess},
    utils::check,
    {Error, Result},
};
use futures_util::FutureExt;
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
    let num_crates = krates.count_filtered(glob_str) as usize;
    let chunk_size = 500.min(num_crates);
    if chunk_size == 0 {
        return Ok(());
    }
    progress.init(Some(num_crates), Some("crates".into()));

    let (processors, rx_result) = {
        let (tx_task, rx_task) = async_channel::bounded(1);
        let (tx_result, rx_result) = async_channel::bounded(cpu_o_bound_processors as usize * 2);

        for _ in 0..cpu_o_bound_processors {
            let task = rx_task.clone();
            let result = tx_result.clone();
            crate::spawn(blocking::unblock(move || {
                futures_lite::future::block_on(async move {
                    while let Ok(f) = task.recv().await {
                        result.send(f.await).await.map_err(Error::send_msg("send CPU result"))?;
                    }
                    Ok::<_, Error>(())
                })
            }))
            .detach();
        }
        (tx_task, rx_result)
    };

    let waste_report_dir = output_dir.join(report::waste::Generator::name());
    blocking::unblock({
        let dir = waste_report_dir.clone();
        move || std::fs::create_dir_all(dir)
    })
    .await?;
    use crate::engine::report::generic::WriteCallback;
    let (cache_dir, (git_handle, git_state, maybe_join_handle)) = match glob.as_ref() {
        Some(_) => (None, (git::not_available as WriteCallback, None, None)),
        None => {
            let cd = waste_report_dir.join("__incremental_cache__");
            blocking::unblock({
                let cd = cd.clone();
                move || std::fs::create_dir_all(cd)
            })
            .await?;
            (
                Some(cd),
                git::select_callback(cpu_o_bound_processors, &waste_report_dir, progress.add_child("git")),
            )
        }
    };
    let merge_reports = crate::spawn({
        let merge_progress = progress.add_child("report aggregator");
        merge_progress.init(Some(num_crates / chunk_size), Some("Reports".into()));
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
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .filter_map(|r| r.ok()),
            );
            fetched_crates += chunk.len();

            chunk.len() != chunk_size as usize
        };

        cid += 1;
        check(deadline)?;

        progress.set(cid * chunk_size);
        progress.halted("write crate report", None);
        processors
            .send(report::waste::Generator::write_files(
                db.clone(),
                waste_report_dir.clone(),
                cache_dir.clone(),
                chunk,
                progress.add_child(""),
                git_handle,
                git_state.clone(),
            ))
            .await
            .map_err(Error::send_msg("Chunk of files to write"))?;
        chunk = Vec::with_capacity(chunk_size as usize);
        if abort_loop {
            break;
        }
    }
    drop(git_state);
    drop(processors);
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
