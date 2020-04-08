use crate::{
    export::to_sql::{to_seconds_since_epoch, SqlConvert},
    model,
};
use rusqlite::{params, Statement, NO_PARAMS};
use std::collections::BTreeMap;

impl<'a> SqlConvert for model::db_dump::Crate {
    fn replace_statement() -> &'static str {
        "will not be called"
    }
    fn source_table_name() -> &'static str {
        "crates.io-crate"
    }
    fn init_table_statement() -> &'static str {
        "
        BEGIN;
        CREATE TABLE 'crates.io-actor' (
             id                                INTEGER NOT NULL,
             crates_io_id                      INTEGER NOT NULL,
             kind                              TEXT NOT NULL,
             github_id                         INTEGER NOT NULL,
             github_avatar_url                 TEXT NOT NULL,
             github_login                      TEXT NOT NULL,
             name                              TEXT,
             PRIMARY KEY (id)
        );
        CREATE TABLE 'crates.io-crate' (
             name                TEXT NOT NULL,
             stored_at           TIMESTAMP NOT NULL,
             created_at          TIMESTAMP NOT NULL,
             updated_at          TIMESTAMP NOT NULL,
             description         TEXT,
             documentation       TEXT,
             downloads           INTEGER NOT NULL,
             homepage            TEXT,
             readme              TEXT,
             repository          TEXT,
             created_by          INTEGER,
             PRIMARY KEY (name),
             FOREIGN KEY (created_by) REFERENCES actor(id)
        );
        COMMIT;
        "
    }

    fn convert_to_sql(
        input_statement: &mut rusqlite::Statement,
        transaction: &rusqlite::Transaction,
    ) -> Option<crate::Result<usize>> {
        Some(do_it(input_statement, transaction))
    }

    fn insert(
        &self,
        _key: &str,
        _uid: i32,
        _stm: &mut Statement<'_>,
        _sstm: Option<&mut rusqlite::Statement<'_>>,
    ) -> crate::Result<usize> {
        unimplemented!("we implement convert_to_sql instead (having our own loop and unlimited prepared statements")
    }
}

fn do_it(
    input_statement: &mut rusqlite::Statement,
    transaction: &rusqlite::Transaction,
) -> crate::Result<usize> {
    let mut insert_crate = transaction
        .prepare(&format!(
            "
            REPLACE INTO '{}'
                     (name, stored_at, created_at, updated_at, description, documentation, downloads, homepage, readme, repository, created_by)
              VALUES (?1  , ?2       , ?3        , ?4        , ?5         , ?6           , ?7       , ?8      , ?9    , ?10       , ?11);
        ",
            model::db_dump::Crate::source_table_name()
        ))
        .unwrap();

    let mut actors = BTreeMap::new();
    {
        let mut actor_id = 0;
        for res in input_statement.query_map(NO_PARAMS, |r| {
            let key: String = r.get(0)?;
            let value: Vec<u8> = r.get(1)?;
            Ok((key, value))
        })? {
            let (_crate_name, bytes) = res?;
            let krate: model::db_dump::Crate = bytes.as_slice().into();
            let mut incremented_id = || {
                let id = actor_id;
                actor_id += 1;
                id
            };
            if let Some(actor) = krate.created_by {
                actors.entry(actor).or_insert_with(&mut incremented_id);
            }
            for actor in krate.owners.into_iter() {
                actors.entry(actor).or_insert_with(&mut incremented_id);
            }
        }
    }
    let mut count = 0;
    for res in input_statement.query_map(NO_PARAMS, |r| {
        let key: String = r.get(0)?;
        let value: Vec<u8> = r.get(1)?;
        Ok((key, value))
    })? {
        count += 1;
        let (_crate_name, bytes) = res?;
        let model::db_dump::Crate {
            name,
            stored_at,
            created_at,
            updated_at,
            description,
            documentation,
            downloads,
            homepage,
            readme,
            repository,
            versions: _,
            keywords: _,
            categories: _,
            created_by,
            owners: _,
        } = bytes.as_slice().into();

        insert_crate.execute(params![
            name,
            to_seconds_since_epoch(stored_at),
            to_seconds_since_epoch(created_at),
            to_seconds_since_epoch(updated_at),
            description,
            documentation,
            downloads as i64,
            homepage,
            readme,
            repository,
            created_by.map(|actor| actors.get(&actor))
        ])?;
    }
    Ok(count)
}
