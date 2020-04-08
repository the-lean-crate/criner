use crate::{
    export::to_sql::{to_seconds_since_epoch, SqlConvert},
    model,
};
use rusqlite::{params, Statement, NO_PARAMS};

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
        .prepare("
            REPLACE INTO 'crates.io-crate'
                     (name, stored_at, created_at, updated_at, description, documentation, downloads, homepage, readme, repository, created_by)
              VALUES (?1  , ?2       , ?3        , ?4        , ?5         , ?6           , ?7       , ?8      , ?9    , ?10       , ?11);
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

    let mut count = 0;
    for res in input_statement.query_map(NO_PARAMS, |r| {
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
            versions: _,
            keywords: _,
            categories: _,
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
            created_by.map(|actor| actor.github_id)
        ])?;
    }
    Ok(count)
}

fn insert_actor_to_db(
    insert_actor: &mut Statement,
    actor: &model::db_dump::Actor,
) -> rusqlite::Result<usize> {
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
