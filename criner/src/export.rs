use crate::model;
use rusqlite::{params, Connection, Statement, NO_PARAMS};
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
    let mut istm = input.prepare(&format!(
        "SELECT key, data FROM {}",
        model::Crate::source_table_name()
    ))?;

    let mut output = Connection::open(destination_db)?;
    output.execute(model::Crate::init_table_statement(), NO_PARAMS)?;
    let transaction = output.transaction()?;
    let mut count = 0;
    let start = std::time::SystemTime::now();
    {
        let mut ostm = transaction.prepare(model::Crate::insert_statement())?;
        for res in istm.query_map(NO_PARAMS, |r| {
            let key: String = r.get(0)?;
            let value: Vec<u8> = r.get(1)?;
            Ok((key, value))
        })? {
            count += 1;
            let (key, value) = res?;
            let value: model::Crate = value.as_slice().into();
            value.insert(&key, &mut ostm)?;
        }
    }
    transaction.commit()?;
    log::info!(
        "Inserted {} crates in {:?}s",
        count,
        std::time::SystemTime::now().duration_since(start).unwrap()
    );

    Ok(())
}

trait SqlConvert {
    fn insert_statement() -> &'static str;
    fn source_table_name() -> &'static str;
    fn init_table_statement() -> &'static str;
    fn insert(&self, key: &str, stm: &mut rusqlite::Statement) -> rusqlite::Result<usize>;
}

impl<'a> SqlConvert for model::Crate<'a> {
    fn insert_statement() -> &'static str {
        "INSERT INTO crates
                   (name, version)
            VALUES (?1,   ?2)"
    }
    fn source_table_name() -> &'static str {
        "crates"
    }
    fn init_table_statement() -> &'static str {
        "CREATE TABLE crates (
             name           TEXT NOT NULL,
             version        TEXT NOT NULL,
          CONSTRAINT con_primary_name PRIMARY KEY (name, version)
        )"
    }

    fn insert(&self, key: &str, stm: &mut Statement<'_>) -> rusqlite::Result<usize> {
        let name = key.split(crate::persistence::KEY_SEP_CHAR).next().unwrap();
        for version in self.versions.iter() {
            stm.execute(params![name, version.as_ref()])?;
        }
        Ok(self.versions.len())
    }
}
