use crate::Result;
use std::path::{Path, PathBuf};

mod keyed;
mod merge;
pub use keyed::*;

mod serde;
mod tree;
use crate::persistence::ThreadSafeConnection;
pub use tree::*;

#[derive(Clone)]
pub struct Db {
    sqlite_path: PathBuf,
}

impl Db {
    pub fn open(path: impl AsRef<Path>) -> Result<Db> {
        std::fs::create_dir_all(&path)?;
        let sqlite_path = path.as_ref().join("db.msgpack.sqlite");
        {
            let mut connection = rusqlite::Connection::open(&sqlite_path)?;
            connection.execute_batch("
                PRAGMA journal_mode = WAL;          -- better write-concurrency
                PRAGMA synchronous = NORMAL;        -- fsync only in critical moments
                PRAGMA wal_autocheckpoint = 1000;   -- write WAL changes back every 1000 pages, for an in average 1MB WAL file. May affect readers if number is increased
                PRAGMA wal_checkpoint(TRUNCATE);    -- free some space by truncating possibly massive WAL files from the last run.
            ")?;

            let transaction = connection.transaction()?;
            for name in &["meta", "crate_version", "crate", "task", "result"] {
                transaction.execute_batch(&format!(
                    "CREATE TABLE IF NOT EXISTS {} (
                          key             TEXT PRIMARY KEY NOT NULL,
                          data            BLOB NOT NULL
                    )",
                    name
                ))?;
            }
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS report_done (
                        key             TEXT PRIMARY KEY NOT NULL
                )",
            )?;
            transaction.commit()?;
        }

        Ok(Db { sqlite_path })
    }

    pub fn open_connection(&self) -> Result<ThreadSafeConnection> {
        Ok(std::sync::Arc::new(parking_lot::Mutex::new(
            rusqlite::Connection::open(&self.sqlite_path)?,
        )))
    }

    pub fn open_connection_no_async(&self) -> Result<rusqlite::Connection> {
        let connection = rusqlite::Connection::open(&self.sqlite_path)?;
        connection.busy_handler(Some(sleeper))?;
        Ok(connection)
    }

    pub fn open_crate_versions(&self) -> Result<CrateVersionsTree> {
        Ok(CrateVersionsTree {
            inner: self.open_connection()?,
        })
    }
    pub fn open_crates(&self) -> Result<CratesTree> {
        Ok(CratesTree {
            inner: self.open_connection()?,
        })
    }
    pub fn open_tasks(&self) -> Result<TasksTree> {
        Ok(TasksTree {
            inner: self.open_connection()?,
        })
    }
    pub fn open_results(&self) -> Result<TaskResultTree> {
        Ok(TaskResultTree {
            inner: self.open_connection()?,
        })
    }
    pub fn open_context(&self) -> Result<ContextTree> {
        Ok(ContextTree {
            inner: self.open_connection()?,
        })
    }
    pub fn open_reports(&self) -> Result<ReportsTree> {
        Ok(ReportsTree {
            inner: self.open_connection()?,
        })
    }
}

fn sleeper(attempts: i32) -> bool {
    log::warn!("SQLITE_BUSY, retrying after 250ms (attempt {})", attempts);
    std::thread::sleep(std::time::Duration::from_millis(250));
    true
}
