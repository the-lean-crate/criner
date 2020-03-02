use crate::Result;
use std::path::{Path, PathBuf};

mod keyed;
pub use keyed::*;

mod serde;
mod sled_tree;
use crate::persistence::ThreadSafeConnection;
pub use sled_tree::*;

#[derive(Clone)]
pub struct Db {
    pub inner: sled::Db,
    sqlite_path: PathBuf,
    meta: sled::Tree,
    tasks: sled::Tree,
    versions: sled::Tree,
    crates: sled::Tree,
    results: sled::Tree,
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
            ")?;

            let transaction = connection.transaction()?;
            for name in &["meta", "crate_version", "crate", "task", "result"] {
                transaction.execute_batch(&format!(
                    "CREATE TABLE IF NOT EXISTS {} (
                          key             TEXT PRIMARY KEY,
                          data            BLOB NOT NULL
                    )",
                    name
                ))?;
            }
            transaction.commit()?;
        }

        // NOTE: Default compression achieves cutting disk space in half, but the processing speed is cut in half
        // for our binary data as well.
        // TODO: re-evaluate that for textual data - it might enable us to store all files, and when we
        // have more read-based workloads. Maybe it's worth it to turn on.
        // NOTE: Databases with and without compression need migration.
        let inner = sled::Config::new()
            .cache_capacity(128 * 1024 * 1024)
            .path(&path)
            .open()?;

        let meta = inner.open_tree("meta")?;
        let versions = inner.open_tree("crate_versions")?;
        let crates = inner.open_tree("crates")?;
        let tasks = inner.open_tree("tasks")?;
        let results = inner.open_tree("results")?;
        Ok(Db {
            sqlite_path,
            inner,
            meta,
            versions,
            crates,
            tasks,
            results,
        })
    }

    pub fn open_crate_versions(&self) -> Result<CrateVersionsTree> {
        Ok(CrateVersionsTree {
            inner: (&self.versions, open_connection(&self.sqlite_path)?),
        })
    }
    pub fn open_crates(&self) -> Result<CratesTree> {
        Ok(CratesTree {
            inner: (&self.crates, open_connection(&self.sqlite_path)?),
        })
    }
    pub fn open_tasks(&self) -> Result<TasksTree> {
        Ok(TasksTree {
            inner: (&self.tasks, open_connection(&self.sqlite_path)?),
        })
    }
    pub fn open_results(&self) -> Result<TaskResultTree> {
        Ok(TaskResultTree {
            inner: (&self.results, open_connection(&self.sqlite_path)?),
        })
    }
    pub fn open_context(&self) -> Result<ContextTree> {
        Ok(ContextTree {
            inner: (&self.meta, open_connection(&self.sqlite_path)?),
        })
    }
}

fn open_connection(db_path: &Path) -> Result<ThreadSafeConnection> {
    let connection = rusqlite::Connection::open(db_path)?;
    // TODO: this one day could be rewritten to using async sleeps. However, that is some extra work in the face of
    // traits not supporting async fn natively (there is a crate though). So figure out if this is an issue, possibly
    // by busy-logging ourselves.
    connection.busy_timeout(std::time::Duration::from_millis(500))?;
    Ok(std::sync::Arc::new(parking_lot::Mutex::new(connection)))
}
