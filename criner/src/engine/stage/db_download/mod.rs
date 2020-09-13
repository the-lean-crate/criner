use crate::model::db_dump;
use crate::{
    engine::work, persistence::new_key_value_insertion, persistence::Db, persistence::TableAccess, Error, Result,
};
use bytesize::ByteSize;
use futures_util::FutureExt;
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

fn extract_and_ingest(db: Db, mut progress: prodash::tree::Item, db_file_path: PathBuf) -> Result<()> {
    progress.init(None, Some("csv files"));
    let mut archive = tar::Archive::new(libflate::gzip::Decoder::new(BufReader::new(File::open(db_file_path)?))?);
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
    let mut teams = None::<BTreeMap<csv_model::Id, csv_model::Team>>;
    let mut categories = None::<BTreeMap<csv_model::Id, csv_model::Category>>;
    let mut versions = None::<Vec<csv_model::Version>>;
    let mut keywords = None::<BTreeMap<csv_model::Id, csv_model::Keyword>>;
    let mut users = None::<BTreeMap<csv_model::Id, csv_model::User>>;
    let mut crates = None::<Vec<csv_model::Crate>>;
    let mut crate_owners = None::<Vec<csv_model::CrateOwner>>;
    let mut version_authors = None::<Vec<csv_model::VersionAuthor>>;
    let mut crates_categories = None::<Vec<csv_model::CratesCategory>>;
    let mut crates_keywords = None::<Vec<csv_model::CratesKeyword>>;

    for (eid, entry) in archive.entries()?.enumerate() {
        num_files_seen = eid + 1;
        progress.set(eid as u32);

        let entry = entry?;
        let entry_size = entry.header().size()?;
        num_bytes_seen += entry_size;

        if let Some(name) = entry
            .path()
            .ok()
            .and_then(|p| whitelist_names.iter().find(|n| p.ends_with(format!("{}.csv", n))))
        {
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
                    crates_categories = Some(from_csv::vec(entry, "crates_categories", &mut progress)?);
                }
                "crates_keywords" => {
                    crates_keywords = Some(from_csv::vec(entry, "crates_keywords", &mut progress)?);
                }
                _ => progress.fail(format!("bug or oversight: Could not parse table of type {:?}", name)),
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
    let versions = versions.ok_or_else(|| Error::Bug("expected versions.csv in crates-io db dump"))?;
    let version_authors =
        version_authors.ok_or_else(|| Error::Bug("expected version_authors.csv in crates-io db dump"))?;
    let crates = crates.ok_or_else(|| Error::Bug("expected crates.csv in crates-io db dump"))?;
    let keywords = keywords.ok_or_else(|| Error::Bug("expected keywords.csv in crates-io db dump"))?;
    let crates_keywords =
        crates_keywords.ok_or_else(|| Error::Bug("expected crates_keywords.csv in crates-io db dump"))?;
    let categories = categories.ok_or_else(|| Error::Bug("expected categories.csv in crates-io db dump"))?;
    let crates_categories =
        crates_categories.ok_or_else(|| Error::Bug("expected crates_categories.csv in crates-io db dump"))?;
    let crate_owners = crate_owners.ok_or_else(|| Error::Bug("expected crate_owners.csv in crates-io db dump"))?;

    progress.init(Some(4), Some("conversion steps"));
    progress.set_name("transform actors");
    progress.set(1);
    let actors_by_id = convert::into_actors_by_id(users, teams, progress.add_child("actors"));

    progress.set_name("transform versions");
    progress.set(2);
    let versions_by_crate_id =
        convert::into_versions_by_crate_id(versions, version_authors, &actors_by_id, progress.add_child("versions"));

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

fn cleanup(db_file_path: PathBuf, mut progress: prodash::tree::Item) -> Result<()> {
    let glob_pattern = db_file_path
        .parent()
        .expect("parent directory for db dump")
        .join("[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]-*")
        .with_extension(db_file_path.extension().expect("file extension"));
    let pattern = glob::Pattern::new(&glob_pattern.to_str().expect("db dump path is valid utf8 string"))?;
    if !pattern.matches_path(&db_file_path) {
        return Err(crate::Error::Message(format!(
            "BUG: Pattern {} did not match the original database path '{}'",
            pattern,
            db_file_path.display()
        )));
    }

    for file in glob::glob(pattern.as_str())? {
        let file = file?;
        if file != db_file_path {
            std::fs::remove_file(&file)?;
            progress.done(format!("Deleted old db-dump at '{}'", file.display()));
        }
    }
    Ok(())
}

pub async fn schedule(
    db: Db,
    assets_dir: PathBuf,
    mut progress: prodash::tree::Item,
    startup_time: std::time::SystemTime,
) -> Result<()> {
    let (tx_result, rx_result) = async_channel::bounded(1);
    let tx_io = {
        let (tx_io, rx) = async_channel::bounded(1);
        let max_retries_on_timeout = 80;
        crate::smol::Task::spawn(
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
        )
        .detach();
        tx_io
    };

    let today_yyyy_mm_dd = time::OffsetDateTime::now_local().format("%F");
    let file_suffix = "db-dump.tar.gz";
    let task_key = format!(
        "{}{}{}",
        "crates-io-db-dump",
        crate::persistence::KEY_SEP_CHAR,
        today_yyyy_mm_dd
    );

    let db_file_path = assets_dir
        .join("crates-io-db")
        .join(format!("{}-{}", today_yyyy_mm_dd, file_suffix));
    let tasks = db.open_tasks()?;
    if tasks
        .get(&task_key)?
        .map(|t| t.can_be_started(startup_time) || t.state.is_complete()) // always allow the extractor to run - must be idempotent
        .unwrap_or(true)
    {
        tx_io
            .send(work::iobound::DownloadRequest {
                output_file_path: db_file_path.clone(),
                progress_name: "db dump".to_string(),
                task_key,
                crate_name_and_version: None,
                kind: "tar.gz",
                url: "https://static.crates.io/db-dump.tar.gz".to_string(),
            })
            .await
            .map_err(Error::send_msg("Download Request"))?;
        drop(tx_io);
        if let Ok(db_file_path) = rx_result.recv().await {
            {
                let progress = progress.add_child("ingest");
                blocking::unblock(move || extract_and_ingest(db, progress, db_file_path))
            }
            .await
            .map_err(|err| {
                progress.fail(format!("ingestion failed: {}", err));
                err
            })?;
        }
    }

    blocking::unblock(move || cleanup(db_file_path, progress.add_child("removing old db-dumps"))).await?;
    Ok(())
}
