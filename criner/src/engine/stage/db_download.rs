use crate::{engine::work, persistence::Db, persistence::TableAccess, Result};
use bytesize::ByteSize;
use futures::FutureExt;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufReader, Read},
    path::PathBuf,
};

mod model {
    use std::time::SystemTime;

    type UserId = u32;

    pub struct Keyword<'a> {
        name: &'a str,
        // amount of crates using the keyword
        crates_count: u32,
    }

    pub struct Category<'a> {
        id: u32,
        name: &'a str,
        crates_count: u32,
        description: &'a str,
        path: &'a str,
        slug: &'a str,
    }

    pub struct Crate<'a> {
        name: &'a str,
        created_at: SystemTime,
        updated_at: SystemTime,
        description: Option<&'a str>,
        documentation: Option<&'a str>,
        downloads: u64,
        homepage: Option<&'a str>,
        readme: Option<&'a str>,
        repository: Option<&'a str>,
        created_by: UserId,
        keywords: Vec<Keyword<'a>>,
        categories: Vec<Category<'a>>,
        owner: UserId,
    }

    pub enum UserKind {
        Individual,
        Team,
    }

    pub struct User<'a> {
        github_avatar_url: &'a str,
        github_id: u32,
        github_login: &'a str,
        crates_io_id: UserId,
        name: Option<&'a str>,
        kind: UserKind,
    }

    pub struct Feature {
        name: String,
        dependencies: Vec<String>,
    }

    pub struct Version<'a> {
        crate_size: Option<u32>,
        created_at: SystemTime,
        updated_at: SystemTime,
        downloads: u32,
        features: Vec<Feature>,
        license: &'a str,
        crate_name: &'a str,
        // corresponds to 'num' in original data set
        semver: &'a str,
        published_by: Option<UserId>,
        is_yanked: bool,
    }
}

mod from_csv {
    pub fn records<'csv, T>(csv: &'csv [u8], cb: impl FnMut(T)) -> crate::Result<()>
    where
        T: serde::Deserialize<'csv>,
    {
        let mut rd = csv::ReaderBuilder::new()
            .delimiter(b',')
            .has_headers(true)
            .flexible(true)
            .from_reader(csv);
        for item in rd.deserialize() {
            cb(item?);
        }
        Ok(())
    }
}

fn extract_and_ingest(
    _db: Db,
    mut progress: prodash::tree::Item,
    db_file_path: PathBuf,
) -> crate::Result<()> {
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

    let mut csv_map = BTreeMap::new();
    let mut num_files_seen = 0;
    let mut num_bytes_seen = 0;
    for (eid, entry) in archive.entries()?.enumerate() {
        num_files_seen = eid + 1;
        progress.set(eid as u32);
        let mut entry = entry?;
        let entry_size = entry.header().size()?;
        num_bytes_seen += entry_size;
        if let Some(name) = entry.path().ok().and_then(|p| {
            whitelist_names
                .iter()
                .find(|n| p.ends_with(format!("{}.csv", n)))
        }) {
            let mut buf = Vec::with_capacity(entry_size as usize);
            entry.read_to_end(&mut buf)?;
            csv_map.insert(name, buf);
            progress.done(format!(
                "extracted '{}' with size {} into memory",
                entry.path()?.display(),
                ByteSize(entry_size)
            ))
        }
    }
    progress.done(format!(
        "Saw {} files and a total of {}",
        num_files_seen,
        ByteSize(num_bytes_seen)
    ));
    Ok(())
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
