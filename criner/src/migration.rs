use crate::model::TaskResult;
use crate::persistence::TreeAccess;
use std::path::Path;

pub fn migrate(db_path: impl AsRef<Path>) -> crate::error::Result<()> {
    let assets_dir = db_path.as_ref().join("assets");
    log::info!("opening sled db");
    let db = sled::open(&db_path)?;
    log::info!("DONE opening sled db");
    log::info!("dropping tree");
    db.drop_tree("results".as_bytes())?;
    log::info!("DONE dropping tree");
    log::info!("Dropping DB");
    drop(db);
    log::info!("DONE Dropping DB");
    log::info!("Opening DB again");
    let db = crate::persistence::Db::open(db_path)?;
    log::info!("DONE open db");
    let tree = db.results();

    log::info!("Reading assets directory");
    let list = std::fs::read_dir(&assets_dir)?;
    log::info!("DONE Reading assets directory");
    let mut task = crate::model::Task::default();
    task.process = "download".into();
    task.version = "1.0.0".into();

    for item in list {
        let item = item?;
        // Zen:0.0.0:download:1.0.0:crate
        let tokens: Vec<String> = item
            .file_name()
            .to_str()
            .map(ToOwned::to_owned)
            .ok_or(crate::error::Error::Bug("need ascii only asset name"))?
            .as_str()
            .split(':')
            .map(ToOwned::to_owned)
            .collect();
        if tokens.len() != 5 {
            continue;
        }
        let name = &tokens[0];
        let version = &tokens[1];
        let url = format!(
            "https://crates.io/api/v1/crates/{name}/{version}/download",
            name = name,
            version = version
        );
        let insert_item = (
            name.as_str(),
            version.as_str(),
            &task,
            TaskResult::Download {
                kind: "crate".into(),
                url: url.into(),
                content_length: item.metadata()?.len() as u32,
                content_type: Some("application/x-tar".into()),
            },
        );
        tree.insert(&insert_item)?;
        let new_dir = assets_dir.join(name).join(version);
        std::fs::create_dir_all(&new_dir)?;
        std::fs::rename(item.path(), new_dir.join("download:1.0.0.crate"))?;
        log::info!("{} DONE", item.path().display())
    }
    Ok(())
}
