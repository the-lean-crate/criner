use std::path::PathBuf;
use crate::persistence::TreeAccess;
use crate::model::TaskResult;

pub fn migrate(db_path: impl AsRef<PathBuf>) -> crate::error::Result<()> {
    let assets_dir = db.as_ref().join("assets");
    log::info!("opening sled db");
    let db = sled::open(&db_path)?;
    log::info!("DONE opening sled db");
    log::info!("dropping tree");
    db.drop_tree("results".into());
    log::info!("DONE dropping tree");
    log::info!("Dropping DB");
    drop(db);
    log::info!("DONE Dropping DB");
    log::info!("Opening DB again");
    let db = crate::persistence::Db::open(db_path)?;
    log::info!("DONE open db");
    let tree = db.results();

    log::info!("Reading assets directory");
    let list = std::fs::read_dir(assets_dir)?;
    log::info!("DONE Reading assets directory");
    let mut task = crate::model::Task::default();
    task.process = "download".into();
    task.version = "1.0.0".into();

    for item in list {
        let item = item?;
        // Zen:0.0.0:download:1.0.0:crate
        let tokens: Vec<_> = item
            .file_name()
            .to_str()
            .ok_or(crate::error::Error::Bug("need ascii only asset name"))?
            .split(':')
            .collect();
        assert_eq!(tokens.len(), 5);
        let [name, version, _, _, _] = tokens;
        let url = format!(
            "https://crates.io/api/v1/crates/{name}/{version}/download",
            name = name,
            version = version
        );
        let insert_item = (name, version, &task, TaskResult::Download {
            kind: "crate".into(),
            url: url.into(),
            content_length: item.metadata()?.len() as u32,
            content_type: Some("application/x-tar".into())
        });
        tree.insert(&insert_item)?;
        log::info!("{} DONE", item.path().display())
    }
    Ok(())
}
