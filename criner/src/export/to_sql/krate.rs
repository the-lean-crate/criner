use crate::{export::to_sql::SqlConvert, model};
use rusqlite::{params, Statement};

impl<'a> SqlConvert for model::Crate {
    fn replace_statement() -> &'static str {
        "REPLACE INTO crate
                   (name, version)
            VALUES (?1,   ?2)"
    }
    fn source_table_name() -> &'static str {
        "crates"
    }
    fn init_table_statement() -> &'static str {
        "CREATE TABLE crate (
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
    ) -> crate::Result<usize> {
        let mut tokens = key.split(crate::persistence::KEY_SEP_CHAR);
        let name = tokens.next().unwrap();
        assert!(tokens.next().is_none());

        let Self { versions } = self;
        for version in versions.iter() {
            stm.execute(params![name, version])?;
        }
        Ok(versions.len())
    }
}
