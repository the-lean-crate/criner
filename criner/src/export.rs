use rusqlite::{Connection, NO_PARAMS};
use std::path::Path;

pub fn run_blocking(
    source_db: impl AsRef<Path>,
    destination_db: impl AsRef<Path>,
) -> crate::error::Result<()> {
    let input = Connection::open(source_db)?;
    if destination_db.as_ref().is_file() {
        return Err(crate::Error::Message(format!(
            "Destination database at '{}' does already exist - this is currently unsupported",
            destination_db.as_ref().display()
        )));
    }
    let output = Connection::open(destination_db)?;
    let mut stm = input.prepare("select key, data from crates");
    let start = std::time::SystemTime::now();
    let mut count = 0;
    for res in stm.query_map(NO_PARAMS, |r| {
        let key: String = r.get(0)?;
        let value: Vec<u8> = r.get(1)?;
        Ok((key, value))
    })? {
        count += 1;
        let (key, value) = res?;
        drop(key);
        drop(value);
    }
    log::info!(
        "Traversed {} crates in {:?}s",
        count,
        std::time::SystemTime::now().duration_since(start).unwrap()
    );

    Ok(())
}
