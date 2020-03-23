use crate::{
    engine::report::generic::{WriteCallback, WriteCallbackState, WriteInstruction, WriteRequest},
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

fn env_var(name: &str) -> Result<String> {
    std::env::var(name).map_err(|e| match e {
        std::env::VarError::NotPresent => {
            crate::Error::Message(format!("environment variable {:?} must be set", name))
        }
        std::env::VarError::NotUnicode(_) => crate::Error::Message(format!(
            "environment variable {:?} was set but couldn't be decoded as UTF-8",
            name
        )),
    })
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
                let res = (|| {
                    progress.init(None, Some("files stored in index"));
                    let mut index = repo.index()?;
                    let mut req_count = 0;
                    for WriteRequest { path, content } in rx.iter() {
                        req_count += 1;
                        let entry = file_index_entry(path, content.len());
                        index.add_frombuffer(&entry, &content)?;
                        progress.set(req_count as u32);
                    }
                    progress.init(Some(5), Some("steps"));
                    let tree_oid = {
                        progress.set(1);
                        progress.blocked("writing tree", None);
                        progress.info(format!(
                            "writing tree with {} new entries and a total of {} entries",
                            req_count,
                            index.len()
                        ));
                        let oid = index.write_tree()?;
                        progress.done("Tree written successfully");
                        oid
                    };

                    {
                        progress.set(2);
                        progress.blocked("writing new index", None);
                        repo.set_index(&mut index)?;
                    }

                    {
                        progress.set(3);
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
                        let parent = repo
                            .head()
                            .and_then(|h| h.resolve())
                            .and_then(|h| h.peel_to_commit())
                            .ok();
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
                        progress.done("Commit created");
                    }
                    {
                        progress.set(4);
                        progress.blocked("pushing changes", None);
                        let remote_name = repo
                            .branch_upstream_remote(
                                repo.head()
                                    .and_then(|h| h.resolve())?
                                    .name()
                                    .expect("branch name is valid utf8"),
                            )
                            .map(|b| b.as_str().expect("valid utf8").to_string())
                            .unwrap_or_else(|_| "origin".into());
                        let mut remote = repo.find_remote(&remote_name)?;
                        let mut callbacks = git2::RemoteCallbacks::new();
                        let mut subprogress = progress.add_child("git credentials");
                        let mut sideband = progress.add_child("git sideband");
                        let username = env_var("CRINER_REPORT_PUSH_HTTP_USERNAME")?;
                        let password = env_var("CRINER_REPORT_PUSH_HTTP_PASSWORD")?;
                        callbacks
                            .transfer_progress(|p| {
                                progress.set_name(format!(
                                    "Git pushing changes ({} received)",
                                    bytesize::ByteSize(p.received_bytes() as u64)
                                ));
                                progress.init(
                                    Some((p.total_deltas() + p.total_objects()) as u32),
                                    Some("objects"),
                                );
                                progress
                                    .set((p.indexed_deltas() + p.received_objects()) as u32);
                                true
                            })
                            .sideband_progress(move |line| {
                                sideband.set_name(std::str::from_utf8(line).unwrap_or(""));
                                true
                            }).credentials(move |url, username_from_url, allowed_types| {
                                subprogress.info(format!("Setting userpass plaintext credentials, allowed are {:?} for {:?} (username = {:?}", allowed_types, url, username_from_url));
                                git2::Cred::userpass_plaintext(&username, &password)
                            });

                        remote.push(
                            &["HEAD:refs/heads/master"],
                            Some(
                                git2::PushOptions::new()
                                    .packbuilder_parallelism(0)
                                    .remote_callbacks(callbacks),
                            ),
                        )?;
                        progress.done("Pushed changes");
                    }
                    Ok(())
                })();
                res.map_err(|err| {
                    progress.fail(format!("{}", err));
                    err
                })
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

pub fn not_available(req: WriteRequest, _state: &WriteCallbackState) -> Result<WriteInstruction> {
    Ok(WriteInstruction::DoWrite(req))
}
