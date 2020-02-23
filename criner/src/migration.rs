use crate::persistence::TreeAccess;
use std::path::Path;

pub fn migrate(_db_path: impl AsRef<Path>) -> crate::error::Result<()> {
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

// migrate old to new sled db
// log::info!("opening old db");
// let old_db = sled::open(db_path.as_ref())?;
// let new_db = new_sled::open("./new_crinerd.db").unwrap();
// log::info!("exporting data");
// let data = old_db.export();
// log::info!("importing data");
// new_db.import(data);
