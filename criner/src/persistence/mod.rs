use crate::Result;
use std::path::Path;

mod keyed;
pub use keyed::*;

mod serde;
mod sled_tree;
pub use sled_tree::*;

#[derive(Clone)]
pub struct Db {
    pub inner: sled::Db,
    meta: sled::Tree,
    tasks: sled::Tree,
    versions: sled::Tree,
    crates: sled::Tree,
    results: sled::Tree,
}

impl Db {
    pub fn open(path: impl AsRef<Path>) -> Result<Db> {
        // NOTE: Default compression achieves cutting disk space in half, but the processing speed is cut in half
        // for our binary data as well.
        // TODO: re-evaluate that for textual data - it might enable us to store all files, and when we
        // have more read-based workloads. Maybe it's worth it to turn on.
        // NOTE: Databases with and without compression need migration.
        let inner = sled::Config::new()
            .cache_capacity(128 * 1024 * 1024)
            .path(path)
            .open()?;
        let meta = inner.open_tree("meta")?;
        let versions = inner.open_tree("crate_versions")?;
        let crates = inner.open_tree("crates")?;
        let tasks = inner.open_tree("tasks")?;
        let results = inner.open_tree("results")?;
        Ok(Db {
            inner,
            meta,
            versions,
            crates,
            tasks,
            results,
        })
    }

    pub fn crate_versions(&self) -> CrateVersionsTree {
        CrateVersionsTree {
            inner: &self.versions,
        }
    }
    pub fn crates(&self) -> CratesTree {
        CratesTree {
            inner: &self.crates,
        }
    }
    pub fn tasks(&self) -> TasksTree {
        TasksTree { inner: &self.tasks }
    }
    pub fn results(&self) -> TaskResultTree {
        TaskResultTree {
            inner: &self.results,
        }
    }
    pub fn context(&self) -> ContextTree {
        ContextTree { inner: &self.meta }
    }
}
