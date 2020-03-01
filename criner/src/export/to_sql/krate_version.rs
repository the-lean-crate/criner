use crate::{export::to_sql::SqlConvert, model};
use rusqlite::{params, Statement};

impl<'a> SqlConvert for model::CrateVersion<'a> {
    fn replace_statement() -> &'static str {
        "REPLACE INTO crate_versions
                   (id, name, version, kind, checksum, features)
            VALUES (?1, ?2  , ?3     , ?4  , ?5      , ?6);
        "
    }

    fn secondary_replace_statement() -> Option<&'static str> {
        Some(
            "REPLACE INTO crate_version_dependencies
                        (parent_id, name, required_version, features, optional, default_features, target, kind, package)
                VALUES  (?1       , ?2  , ?3              , ?4      , ?5      , ?6              , ?7    , ?8  , ?9);",
        )
    }

    fn source_table_name() -> &'static str {
        "crate_versions"
    }

    fn init_table_statement() -> &'static str {
        "CREATE TABLE crate_versions (
            id                  INTEGER UNIQUE NOT NULL,
            name                TEXT NOT NULL,
            version             TEXT NOT NULL,
            kind                TEXT NOT NULL,
            checksum            TEXT NOT NULL,
            features            TEXT NOT NULL, -- JSON
            PRIMARY KEY (name, version)
        );
        CREATE TABLE crate_version_dependencies (
             parent_id              INTEGER NOT NULL,
             name                   TEXT NOT NULL,
             required_version       TEXT NOT NULL,
             features               TEXT NOT NULL,    -- JSON
             optional               INTEGER NOT NULL, -- BOOL
             default_features       INTEGER NOT NULL, -- BOOL
             target                 TEXT,
             kind                   TEXT,
             package                TEXT,
             FOREIGN KEY (parent_id) REFERENCES crate_versions(id)
        );
        "
    }

    fn insert(
        &self,
        _key: &str,
        uid: i32,
        stm: &mut Statement<'_>,
        sstm: Option<&mut Statement<'_>>,
    ) -> crate::error::Result<usize> {
        let model::CrateVersion {
            name,
            kind,
            version,
            checksum,
            features,
            dependencies,
        } = self;

        use crates_index_diff::ChangeKind::*;
        stm.execute(params![
            uid,
            name,
            version,
            match kind {
                Added => "added",
                Yanked => "yanked",
            },
            checksum,
            serde_json::to_string_pretty(features).unwrap()
        ])?;

        let sstm = sstm.expect("secondary statement to be set");
        for dep in dependencies {
            let model::Dependency {
                name,
                required_version,
                features,
                optional,
                default_features,
                target,
                kind,
                package,
            } = dep;
            sstm.execute(params![
                uid,
                name,
                required_version,
                serde_json::to_string_pretty(features).unwrap(),
                optional,
                default_features,
                target,
                kind,
                package
            ])?;
        }
        Ok(1)
    }
}
