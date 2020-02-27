use crate::persistence::TreeAccess;
use std::io::Write;
use std::path::Path;

pub fn migrate(db_path: impl AsRef<Path>) -> crate::error::Result<()> {
    use acid_store::store::Open;
    acid_store::init();
    let db = sled::open(&db_path)?;
    for tree_name in db.tree_names() {
        let tree_name_str = std::str::from_utf8(&tree_name).unwrap();
        if ["crates", "meta", "results"].contains(&tree_name_str) {
            log::info!("Skipped {} - already done", tree_name_str);
            continue;
        }

        log::info!("Creating repository '{}'", tree_name_str);
        let db_path: std::path::PathBuf = format!("{}.sqlite", tree_name_str).into();
        let mut repo = if !db_path.exists() {
            acid_store::repo::ObjectRepository::create_repo(
                acid_store::store::SqliteStore::open(
                    db_path.into(),
                    acid_store::store::OpenOption::CREATE,
                )
                .unwrap(),
                acid_store::repo::RepositoryConfig::default(),
                None,
            )
        } else {
            acid_store::repo::ObjectRepository::open_repo(
                acid_store::store::SqliteStore::open(
                    db_path.into(),
                    acid_store::store::OpenOption::CREATE,
                )
                .unwrap(),
                None,
                acid_store::repo::LockStrategy::Abort,
            )
        }
        .unwrap();

        let tree = db.open_tree(&tree_name)?;
        let mut count = 0;
        for res in tree.iter() {
            let (k, v) = res?;
            count += 1;
            log::info!("{}: {}", tree_name_str, count);
            let mut object = repo.insert(std::str::from_utf8(&k).unwrap().to_string());
            object.write_all(v.as_ref())?;
            object.flush().unwrap();
            if count % 100 == 0 {
                log::info!("Committing 100 objects…");
                repo.commit().unwrap();
                log::info!("Commit done");
            }
        }
        log::info!("About to commit remaining objects (totalling {})", count);
        repo.commit().unwrap();
        log::info!("Commit done");
    }
    Ok(())
}

#[allow(dead_code)]
pub fn migrate_old_to_new_manually_and_by_pruning_trees(
    db_path: impl AsRef<Path>,
) -> crate::error::Result<()> {
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
pub fn migrate_old_version_to_new_version_of_sled(
    db_path: impl AsRef<Path>,
) -> crate::error::Result<()> {
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
) -> crate::error::Result<()> {
    let db = crate::persistence::Db::open(&db_path)?;
    for (k, v) in db.tasks().tree().iter().filter_map(Result::ok) {
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
fn migrate_iterate_assets_and_update_db(db_path: impl AsRef<Path>) -> crate::error::Result<()> {
    let assets_dir = db_path.as_ref().join("assets");
    let db = crate::persistence::Db::open(&db_path)?;
    let results = db.results();
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
