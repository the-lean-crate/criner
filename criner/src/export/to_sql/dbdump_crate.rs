use crate::{export::to_sql::SqlConvert, model};
use rusqlite::{params, Statement};

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
        CREATE TABLE crates.io-actor (
             id                                INTEGER NOT NULL,
             crates_io_id                      INTEGER NOT NULL,
             kind                              TEXT NOT NULL,
             github_id                         INTEGER NOT NULL,
             github_avatar_url                 TEXT NOT NULL,
             github_login                      TEXT NOT NULL,
             name                              TEXT,
             PRIMARY KEY (id),
        );
        CREATE TABLE crates.io-crate (
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
    _input_statement: &mut rusqlite::Statement,
    _transaction: &rusqlite::Transaction,
) -> crate::Result<usize> {
    Ok(0)
}
