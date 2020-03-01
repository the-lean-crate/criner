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

    // Turn off keychecks during insertion - we assume we can't get it wrong
    // However, we do embed foreign key relations as form of documentation.
    output.execute_batch("
        PRAGMA foreign_keys = FALSE; -- assume we don't mess up relations, save validation time
        PRAGMA journal_mode = 'OFF' -- no journal, direct writes
    ")?;
    transfer::<model::Crate>(&mut input, &mut output)?;
    transfer::<model::Task>(&mut input, &mut output)?;

    Ok(())
}

fn transfer<T>(input: &mut Connection, output: &mut Connection) -> crate::error::Result<()>
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
        let mut secondary_ostm = match T::secondary_replace_statement() {
            Some(s) => Some(transaction.prepare(s)?),
            None => None,
        };
        for (uid, res) in istm
            .query_map(NO_PARAMS, |r| {
                let key: String = r.get(0)?;
                let value: Vec<u8> = r.get(1)?;
                Ok((key, value))
            })?
            .enumerate()
        {
            count += 1;
            let (key, value) = res?;
            let value = T::from(value.as_slice());
            value.insert(&key, uid as i32, &mut ostm, secondary_ostm.as_mut())?;
        }
    }
    transaction.commit()?;
    log::info!(
        "Inserted {} {} in {:?}",
        count,
        T::source_table_name(),
        std::time::SystemTime::now().duration_since(start).unwrap()
    );

    Ok(())
}

trait SqlConvert {
    fn replace_statement() -> &'static str;
    fn secondary_replace_statement() -> Option<&'static str> {
        None
    }
    fn source_table_name() -> &'static str;
    fn init_table_statement() -> &'static str;
    fn insert(
        &self,
        key: &str,
        uid: i32,
        stm: &mut rusqlite::Statement,
        sstm: Option<&mut rusqlite::Statement>,
    ) -> crate::error::Result<usize>;
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
             PRIMARY KEY (name, version)
        )"
    }

    fn insert(
        &self,
        key: &str,
        _uid: i32,
        stm: &mut Statement<'_>,
        _sstm: Option<&mut rusqlite::Statement<'_>>,
    ) -> crate::error::Result<usize> {
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
                   (id, crate_name, crate_version, process, version, stored_at, state)
            VALUES (?1, ?2,         ?3,            ?4,      ?5,      ?6,        ?7); "
    }
    fn secondary_replace_statement() -> Option<&'static str> {
        Some(
            "replace into task_errors
            (parent_task, error)
        VALUES  (?1,          ?2);",
        )
    }
    fn source_table_name() -> &'static str {
        "tasks"
    }
    fn init_table_statement() -> &'static str {
        "BEGIN;
        CREATE TABLE tasks (
             id               INTEGER UNIQUE NOT NULL,
             crate_name       TEXT NOT NULL,
             crate_version    TEXT NOT NULL,
             process          TEXT NOT NULL,
             version          TEXT NOT NULL,
             stored_at        TIMESTAMP NOT NULL,
             state            TEXT NOT NULL,
             PRIMARY KEY      (crate_name, crate_version, process, version)
        );
        CREATE TABLE task_errors (
             parent_task      INTEGER NOT NULL,
             error            TEXT NOT NULL,
             FOREIGN KEY (parent_task) REFERENCES tasks(id)
        );
        COMMIT;"
    }

    fn insert(
        &self,
        key: &str,
        uid: i32,
        stm: &mut Statement<'_>,
        sstm: Option<&mut rusqlite::Statement<'_>>,
    ) -> crate::error::Result<usize> {
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
        stm.execute(params![
            uid,
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
            },
        ])?;
        match state {
            InProgress(Some(errors)) | AttemptsWithFailure(errors) => {
                let sstm = sstm.ok_or_else(|| crate::Error::Bug("need secondary statement"))?;
                for error in errors.iter() {
                    sstm.execute(params![uid, error])?;
                }
            }
            _ => {}
        }
        Ok(1)
    }
}
