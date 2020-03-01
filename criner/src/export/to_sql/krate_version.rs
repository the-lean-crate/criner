use crate::{export::to_sql::SqlConvert, model};
use rusqlite::{params, Statement};

impl<'a> SqlConvert for model::CrateVersion<'a> {
    fn replace_statement() -> &'static str {
        "REPLACE INTO crate_versions
                   (name, version, kind, checksum, features)
            VALUES (?1  , ?2     , ?3  , ?4      , ?5);
        "
    }

    fn source_table_name() -> &'static str {
        "crate_versions"
    }

    fn init_table_statement() -> &'static str {
        "CREATE TABLE crate_versions (
            name                TEXT NOT NULL,
            version             TEXT NOT NULL,
            kind                TEXT NOT NULL,
            checksum            TEXT NOT NULL,
            features            TEXT NOT NULL, -- JSON
            PRIMARY KEY (name, version)
        );
        "
    }

    fn insert(
        &self,
        _key: &str,
        _uid: i32,
        stm: &mut Statement<'_>,
        _sstm: Option<&mut Statement<'_>>,
    ) -> crate::error::Result<usize> {
        let model::CrateVersion {
            name,
            kind,
            version,
            checksum,
            features,
            dependencies: _,
        } = self;

        use crates_index_diff::ChangeKind::*;
        stm.execute(params![
            name,
            version,
            match kind {
                Added => "added",
                Yanked => "yanked",
            },
            checksum,
            serde_json::to_string_pretty(features).unwrap()
        ])
        .map_err(Into::into)
    }
}
