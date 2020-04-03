use crate::{engine::work, persistence::Db, persistence::TableAccess, Result};
use bytesize::ByteSize;
use futures::FutureExt;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;

async fn extract_and_ingest(
    _db: Db,
    mut progress: prodash::tree::Item,
    db_file_path: PathBuf,
) -> crate::Result<()> {
    progress.init(None, Some("csv files"));
    let mut archive = tar::Archive::new(libflate::gzip::Decoder::new(BufReader::new(File::open(
        db_file_path,
    )?))?);
    let whitelist_names = [
        "crate_owners",
        "versions",
        "version_authors",
        "users",
        "crates",
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
            extract_and_ingest(db, progress.add_child("ingest"), db_file_path)
                .await
                .map_err(|err| {
                    progress.fail(format!("ingestion failed: {}", err));
                    err
                })?;
        }
    }

    // TODO: cleanup old db dumps

    Ok(())
}
