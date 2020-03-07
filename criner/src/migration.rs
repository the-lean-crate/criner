use crate::persistence::{TaskResultTree, TreeAccess};
use rusqlite::{params, NO_PARAMS};
use std::path::Path;

pub fn migrate(db_path: impl AsRef<Path>) -> crate::Result<()> {
    log::info!("open db");
    let db = crate::persistence::Db::open(&db_path)?;
    let mut connection = db.open_connection_no_async()?;
    let mut keys = Vec::<String>::new();
    let table_name = TaskResultTree::table_name();
    {
        log::info!("begin iteration");
        let mut statement = connection.prepare(&format!("SELECT key FROM {}", table_name))?;
        let mut rows = statement.query(NO_PARAMS)?;
        while let Some(r) = rows.next()? {
            keys.push(r.get(0)?);
        }
        log::info!("got {} keys", keys.len());
    }
    {
        log::info!("begin change");
        let transaction = connection.transaction()?;
        let mut statement =
            transaction.prepare(&format!("UPDATE {} SET key=?1 WHERE key=?2;", table_name))?;
        for key in keys.into_iter() {
            statement.execute(params![
                format!(
                    "{}",
                    if key.ends_with(':') {
                        &key[..key.len() - 1]
                    } else {
                        &key[..]
                    }
                ),
                key
            ])?;
        }
        drop(statement);
        transaction.commit()?;
    }
    Ok(())
}

#[allow(dead_code)]
fn migrate_iterate_assets_and_update_db(db_path: impl AsRef<Path>) -> crate::Result<()> {
    let assets_dir = db_path.as_ref().join("assets");
    let db = crate::persistence::Db::open(&db_path)?;
    let results = db.open_results()?;
    let task = crate::engine::work::iobound::default_persisted_download_task();
    let mut key = String::new();
    let root = prodash::Tree::new();
    let mut progress = root.add_child("does not matter");

    for entry in jwalk::WalkDir::new(assets_dir)
        .preload_metadata(true)
        .into_iter()
        .filter_map(Result::ok)
    {
        let entry: jwalk::DirEntry = entry;
        if entry.file_type.as_ref().ok().map_or(true, |d| d.is_dir()) {
            continue;
        }

        if entry.file_name != std::ffi::OsString::from("download:1.0.0.crate") {
            let new_name = entry.path().parent().unwrap().join("download:1.0.0.crate");
            std::fs::rename(entry.path(), &new_name)?;
            log::warn!(
                "Renamed '{}' to '{}'",
                entry.path().display(),
                new_name.display()
            );
        }
        let file_size = entry.metadata.as_ref().unwrap().as_ref().unwrap().len();
        let mut iter = entry.parent_path().iter().skip(3);
        let name = iter.next().and_then(|p| p.to_str()).unwrap();
        let version = iter.next().and_then(|p| p.to_str()).unwrap();
        log::info!("{} {}", name, version);

        key.clear();
        let task_result = crate::model::TaskResult::Download {
            kind: "crate".into(),
            url: format!(
                "https://crates.io/api/v1/crates/{name}/{version}/download",
                name = name,
                version = version,
            )
            .into(),
            content_length: file_size as u32,
            content_type: Some("application/x-tar".into()),
        };
        task_result.fq_key(name, version, &task, &mut key);
        results.insert(&mut progress, &key, &task_result)?;
    }
    Ok(())
}
