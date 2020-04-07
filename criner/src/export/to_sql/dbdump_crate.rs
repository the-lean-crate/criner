use crate::{export::to_sql::SqlConvert, model};
use rusqlite::{params, Statement};

impl<'a> SqlConvert for model::db_dump::Crate {
    fn replace_statement() -> &'static str {
        "will not be called"
    }
    fn source_table_name() -> &'static str {
        "crate"
    }
    fn init_table_statement() -> &'static str {
        "CREATE TABLE crate (
             name           TEXT NOT NULL,
             version        TEXT NOT NULL,
             PRIMARY KEY (name, version)
        )"
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
