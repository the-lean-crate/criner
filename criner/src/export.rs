use crate::model;
use rusqlite::{params, Connection, Statement, NO_PARAMS};
use std::path::Path;

pub fn run_blocking(
    source_db: impl AsRef<Path>,
    destination_db: impl AsRef<Path>,
) -> crate::error::Result<()> {
    if destination_db.as_ref().is_file() {
        return Err(crate::Error::Message(format!(
            "Destination database at '{}' does already exist - this is currently unsupported",
            destination_db.as_ref().display()
        )));
    }
    let mut input = Connection::open(source_db)?;
    let mut output = Connection::open(destination_db)?;

    transfer::<model::Crate>(&mut input, &mut output)?;
    transfer::<model::Task>(&mut input, &mut output)?;

    Ok(())
}

fn transfer<T>(input: &mut Connection, output: &mut Connection) -> rusqlite::Result<()>
where
    for<'a> T: SqlConvert + From<&'a [u8]>,
{
    output.execute_batch(T::init_table_statement())?;
    let mut istm = input.prepare(&format!("SELECT key, data FROM {}", T::source_table_name()))?;
    let transaction = output.transaction()?;
    let mut count = 0;
    let start = std::time::SystemTime::now();
    {
        let mut ostm = transaction.prepare(T::replace_statement())?;
        for res in istm.query_map(NO_PARAMS, |r| {
            let key: String = r.get(0)?;
            let value: Vec<u8> = r.get(1)?;
            Ok((key, value))
        })? {
            count += 1;
            let (key, value) = res?;
            let value = T::from(value.as_slice());
            value.insert(&key, &mut ostm)?;
        }
    }
    transaction.commit()?;
    log::info!(
        "Inserted {} {} in {:?}s",
        count,
        T::source_table_name(),
        std::time::SystemTime::now().duration_since(start).unwrap()
    );

    Ok(())
}

trait SqlConvert {
    fn replace_statement() -> &'static str;
    fn source_table_name() -> &'static str;
    fn init_table_statement() -> &'static str;
    fn insert(&self, key: &str, stm: &mut rusqlite::Statement) -> rusqlite::Result<usize>;
}

impl<'a> SqlConvert for model::Crate<'a> {
    fn replace_statement() -> &'static str {
        "REPLACE INTO crates
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
        let mut tokens = key.split(crate::persistence::KEY_SEP_CHAR);
        let name = tokens.next().unwrap();
        assert!(tokens.next().is_none());

        let Self { versions } = self;
        for version in versions.iter() {
            stm.execute(params![name, version.as_ref()])?;
        }
        Ok(versions.len())
    }
}

impl<'a> SqlConvert for model::Task<'a> {
    fn replace_statement() -> &'static str {
        "REPLACE INTO tasks
                   (crate_name, crate_version, process, version, stored_at, state)
            VALUES (?1,         ?2,            ?3,      ?4,      ?5,        ?6)"
    }
    fn source_table_name() -> &'static str {
        "tasks"
    }
    fn init_table_statement() -> &'static str {
        "BEGIN;
        CREATE TABLE tasks (
             crate_name       TEXT NOT NULL,
             crate_version    TEXT NOT NULL,
             process          TEXT NOT NULL,
             version          TEXT NOT NULL,
             stored_at        TIMESTAMP PRIMARY_KEY NOT NULL,
             state            TEXT NOT NULL,
          CONSTRAINT con_primary_name PRIMARY KEY (crate_name, crate_version, process, version)
        );
        COMMIT;"
    }

    fn insert(&self, key: &str, stm: &mut Statement<'_>) -> rusqlite::Result<usize> {
        use model::TaskState::*;
        let mut tokens = key.split(crate::persistence::KEY_SEP_CHAR);
        let crate_name = tokens.next().unwrap();
        let crate_version = tokens.next().unwrap();
        let _process_name = tokens.next().unwrap();
        assert!(tokens.next().is_none());

        let Self {
            stored_at,
            process,
            version,
            state,
        } = self;
        let row_id = stm.insert(params![
            crate_name,
            crate_version,
            process.as_ref(),
            version.as_ref(),
            stored_at
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as u32,
            match state {
                NotStarted => "NotStarted",
                Complete => "Complete",
                InProgress(_) => "InProgress",
                AttemptsWithFailure(_) => "AttemptsWithFailure",
            }
        ])?;
        match state {
            InProgress(Some(errors)) | AttemptsWithFailure(errors) => {},
            _ => {}
        }
        Ok(1)
    }
}
