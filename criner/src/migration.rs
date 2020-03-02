use crate::persistence::TreeAccess;
use std::path::Path;

pub fn migrate(db_path: impl AsRef<Path>) -> crate::Result<()> {
    // use rayon::prelude::*;
    use rusqlite::{params, Connection};
    let db = sled::open(&db_path)?;
    let sqlite_db_path = std::path::Path::new("./criner.msgpack.sqlite");
    let tree_names: Vec<Vec<u8>> = db.tree_names().into_iter().map(|v| v.to_vec()).collect();

    // tree_names.into_par_iter().try_for_each(|tree_name| {
    for tree_name in tree_names {
        let tree_name_str = std::str::from_utf8(&tree_name).unwrap();
        if tree_name_str == "__sled__default" {
            continue;
        }
        let mut repo = Connection::open(&sqlite_db_path).unwrap();
        repo.execute(
            &format!(
                "CREATE TABLE {} (
                  key             TEXT PRIMARY KEY,
                  data            BLOB NOT NULL
                  )",
                tree_name_str
            ),
            params![],
        )
        .unwrap();

        let tree = db.open_tree(&tree_name)?;
        let mut count = 0;
        let transaction = repo.transaction().unwrap();
        for res in tree.iter() {
            let (k, v) = res?;
            count += 1;
            log::info!("{}: {}", tree_name_str, count);
            transaction
                .execute(
                    &format!("INSERT INTO {} (key, data) VALUES (?1, ?2)", tree_name_str),
                    params![std::str::from_utf8(k.as_ref()).unwrap(), v.as_ref()],
                )
                .unwrap();
        }
        log::info!("about to commit one big transaction");
        transaction.commit().unwrap();
        log::info!("done");
    }
    Ok(())
}

#[allow(dead_code)]
pub fn migrate_remove_task_by_type_from_sled(db_path: impl AsRef<Path>) -> crate::Result<()> {
    let db = crate::persistence::Db::open(&db_path)?;
    let tasks = db.open_tasks()?;
    let tasks_tree = tasks.tree();
    for (k, _v) in tasks_tree.iter().filter_map(Result::ok) {
        let ks = String::from_utf8(k.to_vec()).unwrap();
        if ks.ends_with("extract_crate") {
            tasks_tree.remove(k)?;
        }
    }
    Ok(())
}

pub fn migrate_fix_results_storage_type(db_path: impl AsRef<Path>) -> crate::Result<()> {
    use crate::model::{Task, TaskResult};
    type ResultType<'a> = (String, String, Task<'a>, TaskResult<'a>);
    let db = sled::open(db_path)?;
    let tree = db.open_tree("results")?;
    for (idx, res) in tree.iter().enumerate() {
        let (k, v) = res?;
        let v: ResultType = rmp_serde::from_read(v.as_ref()).unwrap();
        tree.insert(k, rmp_serde::to_vec(&v.3).unwrap())?;
        log::info!("{}", idx);
    }
    Ok(())
}

#[allow(dead_code)]
pub fn migrate_old_to_new_manually_and_by_pruning_trees(
    db_path: impl AsRef<Path>,
) -> crate::Result<()> {
    use sled as old_sled;
    log::info!("opening old db");
    let old_db = old_sled::open(db_path.as_ref()).unwrap();
    let new_db = sled::open("./new_new_crinerd.db").unwrap();
    log::info!("exporting data");
    for tree_name in old_db.tree_names() {
        let tree_name_str = std::str::from_utf8(tree_name.as_ref()).unwrap();
        if ["__sled__default", "reports"].contains(&tree_name_str) {
            log::warn!("skipped {}", tree_name_str);
            continue;
        }
        log::info!("processing '{}'", tree_name_str);
        let tree = old_db.open_tree(tree_name.as_ref()).unwrap();
        let new_tree = new_db.open_tree(tree_name).unwrap();
        for (k, v) in tree.iter().filter_map(|v| v.ok()) {
            new_tree.insert(k.as_ref(), v.as_ref())?;
        }
    }
    Ok(())
}

#[allow(dead_code)]
pub fn migrate_old_version_to_new_version_of_sled(db_path: impl AsRef<Path>) -> crate::Result<()> {
    use sled as old_sled;
    log::info!("opening old db");
    let old_db = old_sled::open(db_path.as_ref()).unwrap();
    let new_db = sled::open("./new_crinerd.db").unwrap();
    log::info!("exporting data");
    let data = old_db.export();
    log::info!("importing data");
    new_db.import(data);
    Ok(())
}

#[allow(dead_code)]
fn migrate_transform_tree_data_that_was_not_necessary_actually(
    db_path: impl AsRef<Path>,
) -> crate::Result<()> {
    let db = crate::persistence::Db::open(&db_path)?;
    for (k, v) in db.open_tasks()?.tree().iter().filter_map(Result::ok) {
        let ks = String::from_utf8(k.to_vec()).unwrap();
        let t: crate::model::Task = v.into();
        assert_eq!(t.version, "1.0.0");
        if ks.ends_with("download") {
            continue;
        }
        if !ks.ends_with("extract_crate") {
            panic!("got one");
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn migrate_iterate_assets_and_update_db(db_path: impl AsRef<Path>) -> crate::Result<()> {
    let assets_dir = db_path.as_ref().join("assets");
    let db = crate::persistence::Db::open(&db_path)?;
    let results = db.open_results()?;
    let task = crate::engine::work::iobound::default_persisted_download_task();

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

        let insert_item = (
            name,
            version,
            &task,
            crate::model::TaskResult::Download {
                kind: "crate".into(),
                url: format!(
                    "https://crates.io/api/v1/crates/{name}/{version}/download",
                    name = name,
                    version = version,
                )
                .into(),
                content_length: file_size as u32,
                content_type: Some("application/x-tar".into()),
            },
        );
        results.insert(&insert_item)?;
    }
    Ok(())
}
