use crate::{
    error::{Error, Result},
    model::{Crate, CrateVersion},
    persistence::{self, Keyed, TreeAccess},
    utils::*,
};
use crates_index_diff::Index;
use futures::task::Spawn;
use std::{
    path::Path,
    time::{Duration, SystemTime},
};

pub async fn fetch(
    crates_io_path: impl AsRef<Path>,
    pool: impl Spawn,
    db: persistence::Db,
    mut progress: prodash::tree::Item,
    deadline: Option<SystemTime>,
) -> Result<()> {
    let start = SystemTime::now();
    let mut subprogress =
        progress.add_child("Potentially cloning crates index - this can take a while…");
    let index = enforce_blocking(
        deadline,
        {
            let path = crates_io_path.as_ref().to_path_buf();
            || Index::from_path_or_cloned(path)
        },
        &pool,
    )
    .await??;
    let crate_versions = enforce_blocking(
        deadline,
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

            index.fetch_changes_with_options(Some(&mut opts))
        },
        &pool,
    )
    .await??;

    progress.done(format!("Fetched {} changed crates", crate_versions.len()));

    let mut store_progress = progress.add_child("processing new crates");
    store_progress.init(Some(crate_versions.len() as u32), Some("crate versions"));

    enforce_blocking(
        deadline,
        {
            let db = db.clone();
            move || {
                let versions = db.open_crate_versions()?;
                let krate = db.open_crates()?;
                let context = db.open_context()?;
                // NOTE: this loop can also be a stream, but that makes computation slower due to overhead
                // Thus we just do this 'quickly' on the main thread, knowing that criner really needs its
                // own executor or resources.
                // We could chunk things, but that would only make the code harder to read. No gains here…
                // NOTE: Even chunks of 1000 were not faster, didn't even saturate a single core...
                let mut key_buf = String::new();
                let crate_versions_len = crate_versions.len();
                for (versions_stored, version) in crate_versions
                    .into_iter()
                    .map(CrateVersion::from)
                    .enumerate()
                {
                    // NOTE: For now, not transactional, but we *could*!
                    {
                        key_buf.clear();
                        version.key_buf(&mut key_buf);
                        versions.insert(&key_buf, &version)?;
                        context.update_today(|c| c.counts.crate_versions += 1)?;
                    }
                    key_buf.clear();
                    Crate::key_from_version_buf(&version, &mut key_buf);
                    if krate.upsert(&key_buf, &version)?.versions.len() == 1 {
                        context.update_today(|c| c.counts.crates += 1)?;
                    }

                    store_progress.set((versions_stored + 1) as u32);
                }
                context.update_today(|c| {
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
        },
        &pool,
    )
    .await??;
    Ok(())
}
