use crate::persistence::{key_value_iter, new_key_value_query_old_to_new, CrateTable};
use crate::{
    error::{Error, Result},
    model,
    persistence::{self, new_key_value_insertion, CrateVersionTable, Keyed, TableAccess},
    utils::enforce_threaded,
};
use crates_index_diff::Index;
use rusqlite::params;
use std::{
    collections::BTreeMap,
    ops::Add,
    path::Path,
    time::{Duration, SystemTime},
};

pub async fn fetch(
    crates_io_path: impl AsRef<Path>,
    db: persistence::Db,
    mut progress: prodash::tree::Item,
    deadline: Option<SystemTime>,
) -> Result<()> {
    let start = SystemTime::now();
    let mut subprogress = progress.add_child("Fetching changes from crates.io index");
    subprogress.blocked("potentially cloning", None);
    let index = enforce_threaded(
        deadline.unwrap_or_else(|| SystemTime::now().add(Duration::from_secs(60 * 60))),
        {
            let path = crates_io_path.as_ref().to_path_buf();
            if !path.is_dir() {
                std::fs::create_dir(&path)?;
            }
            || Index::from_path_or_cloned(path)
        },
    )
    .await??;
    let (crate_versions, last_seen_git_object) = enforce_threaded(
        deadline.unwrap_or_else(|| SystemTime::now().add(Duration::from_secs(10 * 60))),
        move || {
            let mut cbs = crates_index_diff::git2::RemoteCallbacks::new();
            let mut opts = {
                cbs.transfer_progress(|p| {
                    subprogress.set_name(format!(
                        "Fetching crates index ({} received)",
                        bytesize::ByteSize(p.received_bytes() as u64)
                    ));
                    subprogress.init(
                        Some((p.total_deltas() + p.total_objects()) as u32),
                        Some("objects"),
                    );
                    subprogress.set((p.indexed_deltas() + p.received_objects()) as u32);
                    true
                });
                let mut opts = crates_index_diff::git2::FetchOptions::new();
                opts.remote_callbacks(cbs);
                opts
            };

            index.peek_changes_with_options(Some(&mut opts))
        },
    )
    .await??;

    progress.done(format!("Fetched {} changed crates", crate_versions.len()));

    let mut store_progress = progress.add_child("processing new crates");
    store_progress.init(Some(crate_versions.len() as u32), Some("crate versions"));

    let without_time_limit_unless_one_is_set =
        deadline.unwrap_or_else(|| SystemTime::now().add(Duration::from_secs(24 * 60 * 60)));
    enforce_threaded(without_time_limit_unless_one_is_set, {
        let db = db.clone();
        let index_path = crates_io_path.as_ref().to_path_buf();
        move || {
            use std::iter::FromIterator;
            let mut connection = db.open_connection_no_async_with_busy_wait()?;
            let mut crates_lut = {
                let transaction = connection.transaction()?;
                store_progress.blocked("caching crates", None);
                let mut statement =
                    new_key_value_query_old_to_new(CrateTable::table_name(), &transaction)?;
                let iter = key_value_iter::<model::Crate>(&mut statement)?.flat_map(Result::ok);
                BTreeMap::from_iter(iter)
            };

            let mut key_buf = String::new();
            let crate_versions_len = crate_versions.len();
            let mut new_crate_versions = 0;
            let mut new_crates = 0;
            store_progress.blocked("write lock for crate versions", None);
            let transaction =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            {
                let mut statement =
                    new_key_value_insertion(CrateVersionTable::table_name(), &transaction)?;
                for (versions_stored, version) in crate_versions
                    .into_iter()
                    .map(model::CrateVersion::from)
                    .enumerate()
                {
                    key_buf.clear();
                    version.key_buf(&mut key_buf);
                    statement.execute(params![&key_buf, rmp_serde::to_vec(&version)?])?;
                    new_crate_versions += 1;

                    key_buf.clear();
                    model::Crate::key_from_version_buf(&version, &mut key_buf);
                    if crates_lut
                        .entry(key_buf.to_owned())
                        .or_default()
                        .merge_mut(&version)
                        .versions
                        .len()
                        == 1
                    {
                        new_crates += 1;
                    }

                    store_progress.set((versions_stored + 1) as u32);
                }
            }

            store_progress.blocked("commit crate versions", None);
            transaction.commit()?;

            let transaction = {
                store_progress.blocked("write lock for crates", None);
                let mut t = connection
                    .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                t.set_drop_behavior(rusqlite::DropBehavior::Commit);
                t
            };
            {
                let mut statement =
                    new_key_value_insertion(CrateTable::table_name(), &transaction)?;
                store_progress.init(Some(crates_lut.len() as u32), Some("crates"));
                for (cid, (key, value)) in crates_lut.into_iter().enumerate() {
                    statement.execute(params![key, rmp_serde::to_vec(&value)?])?;
                    store_progress.set((cid + 1) as u32);
                }
            }
            store_progress.blocked("commit crates", None);
            transaction.commit()?;

            Index::from_path_or_cloned(index_path)?
                .set_last_seen_reference(last_seen_git_object)?;
            db.open_context()?.update_today(|c| {
                c.counts.crate_versions += new_crate_versions;
                c.counts.crates += new_crates;
                c.durations.fetch_crate_versions += SystemTime::now()
                    .duration_since(start)
                    .unwrap_or_else(|_| Duration::default())
            })?;
            store_progress.done(format!(
                "Stored {} crate versions to database",
                crate_versions_len
            ));
            Ok::<_, Error>(())
        }
    })
    .await??;
    Ok(())
}
