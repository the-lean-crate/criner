use crate::engine::report::generic::WriteCallback;
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

mod git {
    use crate::{
        engine::report::generic::{
            WriteCallback, WriteCallbackState, WriteInstruction, WriteRequest,
        },
        Result,
    };
    use crates_index_diff::git2;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn file_index_entry(path: PathBuf, file_size: usize) -> git2::IndexEntry {
        use std::os::unix::ffi::OsStringExt;
        git2::IndexEntry {
            ctime: git2::IndexTime::new(0, 0),
            mtime: git2::IndexTime::new(0, 0),
            dev: 0,
            ino: 0,
            mode: 0o100644,
            uid: 0,
            gid: 0,
            file_size: file_size as u32,
            id: git2::Oid::zero(),
            flags: 0,
            flags_extended: 0,
            path: path.into_os_string().into_vec(),
        }
    }

    pub fn select_callback(
        processors: u32,
        report_dir: &Path,
        mut progress: prodash::tree::Item,
    ) -> (
        WriteCallback,
        WriteCallbackState,
        Option<std::thread::JoinHandle<Result<()>>>,
    ) {
        match git2::Repository::open(report_dir) {
            Ok(repo) => {
                let (tx, rx) = flume::bounded(processors as usize);
                let is_bare_repo = repo.is_bare();
                let handle = std::thread::spawn(move || -> Result<()> {
                    progress.init(None, Some("file write requests"));
                    let mut index = repo.index()?;
                    let mut req_count = 0;
                    for WriteRequest { path, content } in rx.iter() {
                        req_count += 1;
                        let entry = file_index_entry(path, content.len());
                        index.add_frombuffer(&entry, &content)?;
                        progress.set(req_count as u32);
                    }
                    progress.init(Some(3), Some("steps"));
                    progress.set(0);
                    progress.blocked("writing tree", None);
                    progress.info(format!(
                        "writing tree with {} new entries and a total of {} entries",
                        req_count,
                        index.len()
                    ));
                    let tree_oid = index.write_tree()?;

                    progress.set(1);
                    progress.blocked("writing commit", None);
                    let current_time = git2::Time::new(
                        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64,
                        0,
                    );
                    let signature = git2::Signature::new(
                        "Criner",
                        "https://github.com/the-lean-crate/criner",
                        &current_time,
                    )?;
                    let parent = repo.head().and_then(|h| h.peel_to_commit()).ok();
                    let mut parent_store = Vec::with_capacity(1);

                    repo.commit(
                        Some("HEAD"),
                        &signature,
                        &signature,
                        &format!("update {} reports", req_count),
                        &repo
                            .find_tree(tree_oid)
                            .expect("tree just written to be found"),
                        match parent.as_ref() {
                            Some(parent) => {
                                parent_store.push(parent);
                                &parent_store
                            }
                            None => &[],
                        },
                    )?;

                    progress.set(2);
                    progress.blocked("pushing changes", None);

                    Ok(())
                });
                (
                    if is_bare_repo {
                        log::info!("Writing into bare git repo only, local writes disabled");
                        repo_bare
                    } else {
                        log::info!("Writing into git repo and working dir");
                        repo_with_working_dir
                    },
                    Some(tx),
                    Some(handle),
                )
            }
            Err(err) => {
                log::info!(
                    "no git available in '{}', will write files only (error is '{}')",
                    report_dir.display(),
                    err,
                );
                (not_available, None, None)
            }
        }
    }

    pub fn repo_with_working_dir(
        req: WriteRequest,
        send: &WriteCallbackState,
    ) -> Result<WriteInstruction> {
        send.as_ref()
            .expect("send to be available if a repo is available")
            .send(req.clone())
            .map_err(|_| crate::Error::Message("Could not send git request".into()))?;
        Ok(WriteInstruction::DoWrite(req))
    }
    pub fn repo_bare(req: WriteRequest, send: &WriteCallbackState) -> Result<WriteInstruction> {
        send.as_ref()
            .expect("send to be available if a repo is available")
            .send(req)
            .map_err(|_| crate::Error::Message("Could not send git request".into()))?;
        Ok(WriteInstruction::Skip)
    }

    pub fn not_available(
        req: WriteRequest,
        _state: &WriteCallbackState,
    ) -> Result<WriteInstruction> {
        Ok(WriteInstruction::DoWrite(req))
    }
}

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
    let (cache_dir, (git_handle, git_state, maybe_join_handle)) = match glob.as_ref() {
        Some(_) => (None, (git::not_available as WriteCallback, None, None)),
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
        progress.blocked("write crate report", None);
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
