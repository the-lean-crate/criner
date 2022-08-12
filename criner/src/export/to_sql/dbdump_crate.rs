use crate::{
    export::to_sql::{to_seconds_since_epoch, SqlConvert},
    model,
};
use rusqlite::{params, Statement};

impl SqlConvert for model::db_dump::Crate {
    fn replace_statement() -> &'static str {
        "will not be called"
    }
    fn source_table_name() -> &'static str {
        "crates.io-crate"
    }
    fn init_table_statement() -> &'static str {
        "
        BEGIN;
        CREATE TABLE 'crates.io-crate_version' (
             parent_id              INTEGER NOT NULL,
             crate_name             TEXT NOT NULL,
             semver                 TEXT NOT NULL,
             created_at             TIMESTAMP NOT NULL,
             updated_at             TIMESTAMP NOT NULL,
             downloads              INTEGER NOT NULL,
             features               JSON NOT NULL, -- Array of Feature objects
             license                TEXT NOT NULL,
             crate_size             INTEGER,
             published_by           INTEGER,  -- Github user id as index into crates.io-actor table
             is_yanked              INTEGER NOT NULL,  -- is 1 if this version is yanked
             FOREIGN KEY (parent_id) REFERENCES 'crates.io-crate'(_row_id_)
        );
        CREATE TABLE 'crates.io-actor' (
             crates_io_id                      INTEGER NOT NULL, -- these IDs are not unique, so we can't use it as unique id
             kind                              TEXT NOT NULL,
             github_id                         INTEGER NOT NULL, -- This is a unique id across teams and users
             github_avatar_url                 TEXT NOT NULL,
             github_login                      TEXT NOT NULL,
             name                              TEXT,
             PRIMARY KEY (github_id)
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
             created_by          INTEGER,  -- Github user id as index into crates.io-actor table
             owners              JSON NOT NULL, -- Array of github user ids for indexing into the crates.io-actor table
             keywords            JSON NOT NULL, -- Array of strings, each string being a keyword
             categories          JSON NOT NULL, -- Array of category objects, providing a wealth of information for each
             PRIMARY KEY (name),
             FOREIGN KEY (created_by) REFERENCES actor(github_id)
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

fn do_it(input_statement: &mut rusqlite::Statement, transaction: &rusqlite::Transaction) -> crate::Result<usize> {
    let mut insert_crate = transaction
        .prepare("
            REPLACE INTO 'crates.io-crate'
                     (name, stored_at, created_at, updated_at, description, documentation, downloads, homepage, readme, repository, created_by, owners, keywords, categories)
              VALUES (?1  , ?2       , ?3        , ?4        , ?5         , ?6           , ?7       , ?8      , ?9    , ?10       , ?11       , ?12   , ?13     , ?14);
        ",)
        .unwrap();
    let mut insert_actor = transaction
        .prepare(
            "
            INSERT OR IGNORE INTO 'crates.io-actor'
                     (crates_io_id, kind, github_id, github_avatar_url, github_login, name)
              VALUES (?1          , ?2  , ?3       , ?4               , ?5          , ?6  );
        ",
        )
        .unwrap();

    let mut insert_crate_version = transaction
        .prepare(
            "
            INSERT OR IGNORE INTO 'crates.io-crate_version'
                     (parent_id, crate_name, semver, created_at, updated_at, downloads, features, license, crate_size, published_by, is_yanked)
              VALUES (?1       , ?2        , ?3        , ?4        , ?5        , ?6       , ?7      , ?8 , ?9        , ?10         , , ?11);
        ",
        )
        .unwrap();

    let mut count = 0;
    for res in input_statement.query_map([], |r| {
        let key: String = r.get(0)?;
        let value: Vec<u8> = r.get(1)?;
        Ok((key, value))
    })? {
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
            versions,
            keywords,
            categories,
            created_by,
            owners,
        } = bytes.as_slice().into();

        if let Some(actor) = created_by.as_ref() {
            insert_actor_to_db(&mut insert_actor, actor)?;
        }

        for owner in owners.iter() {
            insert_actor_to_db(&mut insert_actor, owner)?;
        }

        count += insert_crate.execute(params![
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
            created_by.map(|actor| actor.github_id),
            serde_json::to_string_pretty(&owners.iter().map(|actor| actor.github_id).collect::<Vec<_>>()).unwrap(),
            serde_json::to_string_pretty(&keywords).unwrap(),
            serde_json::to_string_pretty(&categories).unwrap(),
        ])?;

        for version in versions {
            let model::db_dump::CrateVersion {
                crate_size,
                created_at,
                updated_at,
                downloads,
                features,
                license,
                semver,
                published_by,
                is_yanked,
            } = version;
            insert_crate_version.execute(params![
                count as i32,
                name,
                semver,
                to_seconds_since_epoch(created_at),
                to_seconds_since_epoch(updated_at),
                downloads as i64,
                serde_json::to_string_pretty(&features).unwrap(),
                license,
                crate_size,
                published_by.map(|a| a.github_id),
                is_yanked
            ])?;
        }
    }
    Ok(count)
}

fn insert_actor_to_db(insert_actor: &mut Statement, actor: &model::db_dump::Actor) -> rusqlite::Result<usize> {
    insert_actor.execute(params![
        actor.crates_io_id,
        match actor.kind {
            model::db_dump::ActorKind::User => "user",
            model::db_dump::ActorKind::Team => "team",
        },
        actor.github_id,
        actor.github_avatar_url,
        actor.github_login,
        actor.name
    ])
}
