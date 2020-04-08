use crate::{
    export::to_sql::{to_seconds_since_epoch, SqlConvert},
    model,
};
use rusqlite::{params, Statement};

impl SqlConvert for model::Task {
    fn replace_statement() -> &'static str {
        "REPLACE INTO task
                   (id, key, process, version, stored_at, state)
            VALUES (?1, ?2,  ?3,      ?4,      ?5,        ?6); "
    }
    fn secondary_replace_statement() -> Option<&'static str> {
        Some(
            "REPLACE INTO task_error
                        (parent_id, error)
                VALUES  (?1       , ?2);",
        )
    }
    fn source_table_name() -> &'static str {
        "task"
    }
    fn init_table_statement() -> &'static str {
        "BEGIN;
            CREATE TABLE task (
                 id               INTEGER UNIQUE NOT NULL,
                 key              TEXT NOT NULL,
                 process          TEXT NOT NULL,
                 version          TEXT NOT NULL,
                 stored_at        TIMESTAMP NOT NULL,
                 state            TEXT NOT NULL,
                 PRIMARY KEY      (key)
            );
            CREATE TABLE task_error (
                 parent_id        INTEGER NOT NULL,
                 error            TEXT NOT NULL,
                 FOREIGN KEY (parent_id) REFERENCES task(id)
            );
         COMMIT;"
    }

    fn insert(
        &self,
        key: &str,
        uid: i32,
        stm: &mut Statement<'_>,
        sstm: Option<&mut rusqlite::Statement<'_>>,
    ) -> crate::Result<usize> {
        use model::TaskState::*;

        let Self {
            stored_at,
            process,
            version,
            state,
        } = self;
        stm.execute(params![
            uid,
            key,
            process,
            version,
            to_seconds_since_epoch(*stored_at),
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
