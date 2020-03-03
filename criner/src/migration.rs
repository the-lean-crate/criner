use crate::persistence::TreeAccess;
use std::path::Path;

pub fn migrate(_db_path: impl AsRef<Path>) -> crate::Result<()> {
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
            name.to_owned(),
            version.to_owned(),
            task.clone(),
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
