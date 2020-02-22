use crate::model::TaskResult;
use crate::persistence::TreeAccess;
use std::path::Path;

pub fn migrate(db_path: impl AsRef<Path>) -> crate::error::Result<()> {
    let assets_dir = db_path.as_ref().join("assets");
    log::info!("opening old db");
    let old_db = sled::open(db_path.as_ref())?;
    let new_db = new_sled::open("./new_crinerd.db").unwrap();
    log::info!("exporting data");
    let data = old_db.export();
    log::info!("importing data");
    new_db.import(data);
    Ok(())
}
