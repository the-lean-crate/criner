use crate::model::db_dump;
use crate::{
    engine::work, persistence::new_key_value_insertion, persistence::Db, persistence::TableAccess,
    Error, Result,
};
use bytesize::ByteSize;
use futures::FutureExt;
use rusqlite::params;
use rusqlite::TransactionBehavior;
use std::{collections::BTreeMap, fs::File, io::BufReader, path::PathBuf};

mod convert;
mod csv_model;
mod from_csv;

fn store(db: Db, crates: Vec<db_dump::Crate>, mut progress: prodash::tree::Item) -> Result<()> {
    let now = std::time::SystemTime::now();
    let crates_len = crates.len();
    progress.init(Some(crates_len as u32), Some("crates stored"));
    let mut connection = db.open_connection_no_async_with_busy_wait()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    {
        let mut insert = new_key_value_insertion("crates.io-crate", &transaction)?;
        for (idx, mut krate) in crates.into_iter().enumerate() {
            progress.set((idx + 1) as u32);
            krate.stored_at = now;
            let data = rmp_serde::to_vec(&krate)?;
            insert.execute(params![krate.name, data])?;
        }
    }
    transaction.commit()?;
    progress.done(format!("Stored {} crates in database", crates_len));
    Ok(())
}

fn extract_and_ingest(
    db: Db,
    mut progress: prodash::tree::Item,
    db_file_path: PathBuf,
) -> Result<()> {
    progress.init(None, Some("csv files"));
    let mut archive = tar::Archive::new(libflate::gzip::Decoder::new(BufReader::new(File::open(
        db_file_path,
    )?))?);
    let whitelist_names = [
        "crates",
        "crate_owners",
        "versions",
        "version_authors",
        "crates_categories",
        "categories",
        "crates_keywords",
        "keywords",
        "users",
        "teams",
    ];

    let mut num_files_seen = 0;
    let mut num_bytes_seen = 0;
    let (
        mut teams,
        mut categories,
        mut versions,
        mut keywords,
        mut users,
        mut crates,
        mut crate_owners,
        mut version_authors,
        mut crates_categories,
        mut crates_keywords,
    ) = (
        None::<BTreeMap<csv_model::Id, csv_model::Team>>,
        None::<BTreeMap<csv_model::Id, csv_model::Category>>,
        None::<Vec<csv_model::Version>>,
        None::<BTreeMap<csv_model::Id, csv_model::Keyword>>,
        None::<BTreeMap<csv_model::Id, csv_model::User>>,
        None::<Vec<csv_model::Crate>>,
        None::<Vec<csv_model::CrateOwner>>,
        None::<Vec<csv_model::VersionAuthor>>,
        None::<Vec<csv_model::CratesCategory>>,
        None::<Vec<csv_model::CratesKeyword>>,
    );
    for (eid, entry) in archive.entries()?.enumerate() {
        num_files_seen = eid + 1;
        progress.set(eid as u32);

        let entry = entry?;
        let entry_size = entry.header().size()?;
        num_bytes_seen += entry_size;

        if let Some(name) = entry.path().ok().and_then(|p| {
            whitelist_names
                .iter()
                .find(|n| p.ends_with(format!("{}.csv", n)))
        }) {
            let done_msg = format!(
                "extracted '{}' with size {}",
                entry.path()?.display(),
                ByteSize(entry_size)
            );
            match *name {
                "teams" => teams = Some(from_csv::mapping(entry, name, &mut progress)?),
                "categories" => {
                    categories = Some(from_csv::mapping(entry, "categories", &mut progress)?);
                }
                "versions" => {
                    versions = Some(from_csv::vec(entry, "versions", &mut progress)?);
                }
                "keywords" => {
                    keywords = Some(from_csv::mapping(entry, "keywords", &mut progress)?);
                }
                "users" => {
                    users = Some(from_csv::mapping(entry, "users", &mut progress)?);
                }
                "crates" => {
                    crates = Some(from_csv::vec(entry, "crates", &mut progress)?);
                }
                "crate_owners" => {
                    crate_owners = Some(from_csv::vec(entry, "crate_owners", &mut progress)?);
                }
                "version_authors" => {
                    version_authors = Some(from_csv::vec(entry, "version_authors", &mut progress)?);
                }
                "crates_categories" => {
                    crates_categories =
                        Some(from_csv::vec(entry, "crates_categories", &mut progress)?);
                }
                "crates_keywords" => {
                    crates_keywords = Some(from_csv::vec(entry, "crates_keywords", &mut progress)?);
                }
                _ => progress.fail(format!(
                    "bug or oversight: Could not parse table of type {:?}",
                    name
                )),
            }
            progress.done(done_msg);
        }
    }
    progress.done(format!(
        "Saw {} files and a total of {}",
        num_files_seen,
        ByteSize(num_bytes_seen)
    ));

    let users = users.ok_or_else(|| Error::Bug("expected users.csv in crates-io db dump"))?;
    let teams = teams.ok_or_else(|| Error::Bug("expected teams.csv in crates-io db dump"))?;
    let versions =
        versions.ok_or_else(|| Error::Bug("expected versions.csv in crates-io db dump"))?;
    let version_authors = version_authors
        .ok_or_else(|| Error::Bug("expected version_authors.csv in crates-io db dump"))?;
    let crates = crates.ok_or_else(|| Error::Bug("expected crates.csv in crates-io db dump"))?;
    let keywords =
        keywords.ok_or_else(|| Error::Bug("expected keywords.csv in crates-io db dump"))?;
    let crates_keywords = crates_keywords
        .ok_or_else(|| Error::Bug("expected crates_keywords.csv in crates-io db dump"))?;
    let categories =
        categories.ok_or_else(|| Error::Bug("expected categories.csv in crates-io db dump"))?;
    let crates_categories = crates_categories
        .ok_or_else(|| Error::Bug("expected crates_categories.csv in crates-io db dump"))?;
    let crate_owners =
        crate_owners.ok_or_else(|| Error::Bug("expected crate_owners.csv in crates-io db dump"))?;

    progress.init(Some(4), Some("conversion steps"));
    progress.set_name("transform actors");
    progress.set(1);
    let actors_by_id = convert::into_actors_by_id(users, teams, progress.add_child("actors"));

    progress.set_name("transform versions");
    progress.set(2);
    let versions_by_crate_id = convert::into_versions_by_crate_id(
        versions,
        version_authors,
        &actors_by_id,
        progress.add_child("versions"),
    );

    progress.set_name("transform crates");
    progress.set(3);
    let crates = convert::into_crates(
        crates,
        keywords,
        crates_keywords,
        categories,
        crates_categories,
        actors_by_id,
        crate_owners,
        versions_by_crate_id,
        progress.add_child("crates"),
    );

    progress.set_name("storing crates");
    progress.set(4);
    store(db, crates, progress.add_child("persist"))
}

pub async fn trigger(
    db: Db,
    assets_dir: PathBuf,
    mut progress: prodash::tree::Item,
    tokio: tokio::runtime::Handle,
    startup_time: std::time::SystemTime,
) -> Result<()> {
    let (tx_result, rx_result) = async_std::sync::channel(1);
    let tx_io = {
        let (tx_io, rx) = async_std::sync::channel(1);
        let max_retries_on_timeout = 80;
        tokio.spawn(
            work::generic::processor(
                db.clone(),
                progress.add_child("↓ IDLE"),
                rx,
                work::iobound::Agent::new(&db, tx_result, {
                    move |_, _, output_file_path| Some(output_file_path.to_path_buf())
                })?,
                max_retries_on_timeout,
            )
            .map(|r| {
                if let Err(e) = r {
                    log::warn!("db download: iobound processor failed: {}", e);
                }
            }),
        );
        tx_io
    };

    let today_yyyy_mm_dd = time::OffsetDateTime::now_local().format("%F");
    let task_key = format!(
        "{}{}{}",
        "crates-io-db-dump",
        crate::persistence::KEY_SEP_CHAR,
        today_yyyy_mm_dd
    );

    let tasks = db.open_tasks()?;
    if tasks
        .get(&task_key)?
        .map(|t| t.can_be_started(startup_time) || t.state.is_complete()) // always allow the extractor to run - must be idempotent
        .unwrap_or(true)
    {
        let db_file_path = assets_dir
            .join("crates-io-db")
            .join(format!("{}-crates-io-db-dump.tar.gz", today_yyyy_mm_dd));
        tx_io
            .send(work::iobound::DownloadRequest {
                output_file_path: db_file_path,
                progress_name: "db dump".to_string(),
                task_key,
                crate_name_and_version: None,
                kind: "tar.gz",
                url: "https://static.crates.io/db-dump.tar.gz".to_string(),
            })
            .await;
        drop(tx_io);
        if let Some(db_file_path) = rx_result.recv().await {
            extract_and_ingest(db, progress.add_child("ingest"), db_file_path).map_err(|err| {
                progress.fail(format!("ingestion failed: {}", err));
                err
            })?;
        }
    }

    // TODO: cleanup old db dumps

    Ok(())
}
