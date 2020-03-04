use crate::model::{Context, Crate, TaskResult};
use crate::{
    model::{CrateVersion, Task},
    persistence::Keyed,
    Result,
};
use rusqlite::{params, OptionalExtension, NO_PARAMS};
use std::time::SystemTime;

/// Required as we send futures to threads. The type system can't statically prove that in fact
/// these connections will only ever be created while already in the thread they should execute on.
/// Also no one can prevent futures from being resumed in after having been send to a different thread.
pub type ThreadSafeConnection = std::sync::Arc<parking_lot::Mutex<rusqlite::Connection>>;

pub trait TreeAccess {
    type StorageItem: serde::Serialize + for<'a> From<&'a [u8]> + Default;
    type InsertItem;

    fn connection(&self) -> &ThreadSafeConnection;
    fn table_name(&self) -> &'static str;

    // TODO: remove this method
    fn key(item: &Self::InsertItem) -> String {
        let mut buf = String::with_capacity(16);
        Self::key_to_buf(item, &mut buf);
        buf
    }
    // TODO: remove this method
    fn key_to_buf(item: &Self::InsertItem, buf: &mut String);
    fn merge(
        &self,
        new_item: &Self::InsertItem,
        existing_item: Option<Self::StorageItem>,
    ) -> Option<Self::StorageItem>;

    fn count(&self) -> u64 {
        self.connection()
            .lock()
            .query_row(
                &format!("SELECT COUNT(*) FROM {}", self.table_name()),
                NO_PARAMS,
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0) as u64
    }
    fn get(&self, key: impl AsRef<[u8]>) -> Result<Option<Self::StorageItem>> {
        Ok(self
            .connection()
            .lock()
            .query_row(
                &format!(
                    "SELECT data FROM {} WHERE key = '{}'",
                    self.table_name(),
                    std::str::from_utf8(key.as_ref()).expect("utf8-keys")
                ),
                NO_PARAMS,
                |r| r.get::<_, Vec<u8>>(0),
            )
            .optional()?
            .map(|d| Self::StorageItem::from(d.as_slice())))
    }

    /// Update an existing item, or create it as default, returning the stored item
    fn update(
        &self,
        key: impl AsRef<[u8]>,
        f: impl Fn(Self::StorageItem) -> Self::StorageItem,
    ) -> Result<Self::StorageItem> {
        let mut guard = self.connection().lock();
        let transaction = {
            let mut t = guard.savepoint()?;
            t.set_drop_behavior(rusqlite::DropBehavior::Commit);
            t
        };
        let new_value = transaction
            .query_row(
                &format!(
                    "SELECT data FROM {} WHERE key = '{}'",
                    self.table_name(),
                    std::str::from_utf8(key.as_ref()).expect("utf8-keys")
                ),
                NO_PARAMS,
                |r| r.get::<_, Vec<u8>>(0),
            )
            .optional()?
            .map_or_else(Self::StorageItem::default, |d| f(d.as_slice().into()));
        // NOTE: Copied from insert - can't use it now as it also inserts to sled. TODO - do it
        // Here the connection upgrades to EXCLUSIVE lock, BUT…the read part before
        // may have read now outdated information, as writes are allowed to happen
        // while reading (previous) data. At least in theory.
        // This means that here we may just block as failure since if there was another writer
        // during the transaction (see https://sqlite.org/lang_transaction.html) it will return sqlite busy.
        // but on busy we wait, so we will just timeout and fail. This is good, but we can be better and
        // handle this to actually retry from the beginning.
        // This would mean we have to handle sqlite busy ourselves everywhere or deactivate the busy timer
        // for a moment.
        transaction.execute(
            &format!(
                "REPLACE INTO {} (key, data) VALUES (?1, ?2)",
                self.table_name()
            ),
            params![key.as_ref(), rmp_serde::to_vec(&new_value)?],
        )?;

        Ok(new_value)
    }

    /// Similar to 'update', but provides full control over the default and allows deletion
    fn upsert(&self, item: &Self::InsertItem) -> Result<Self::StorageItem> {
        let mut guard = self.connection().lock();
        let key_str = Self::key(item);

        let transaction = {
            let mut t = guard.savepoint()?;
            t.set_drop_behavior(rusqlite::DropBehavior::Commit);
            t
        };
        let new_value = {
            let maybe_vec = transaction
                .query_row(
                    &format!(
                        "SELECT data FROM {} WHERE key = '{}'",
                        self.table_name(),
                        key_str
                    ),
                    NO_PARAMS,
                    |r| r.get::<_, Vec<u8>>(0),
                )
                .optional()?;
            self.merge(item, maybe_vec.map(|v| v.as_slice().into()))
        };
        // NOTE: Copied from update, with minor changes to support deletion
        match new_value {
            Some(value) => {
                transaction.execute(
                    &format!(
                        "REPLACE INTO {} (key, data) VALUES (?1, ?2)",
                        self.table_name()
                    ),
                    params![key_str, rmp_serde::to_vec(&value)?],
                )?;
                Ok(value)
            }
            None => todo!("deletion of values - I don't think we need that"),
        }
    }

    fn insert(&self, v: &Self::InsertItem) -> Result<()> {
        self.connection().lock().execute(
            &format!(
                "REPLACE INTO {} (key, data) VALUES (?1, ?2)",
                self.table_name()
            ),
            params![
                Self::key(v),
                rmp_serde::to_vec(&self.merge(v, None).unwrap_or_else(Default::default))?
            ],
        )?;
        Ok(())
    }
}

pub struct TasksTree {
    pub inner: ThreadSafeConnection,
}

impl TreeAccess for TasksTree {
    type StorageItem = Task;
    type InsertItem = (String, String, Task);

    fn connection(&self) -> &ThreadSafeConnection {
        &self.inner
    }
    fn table_name(&self) -> &'static str {
        "task"
    }

    fn key_to_buf((name, version, t): &Self::InsertItem, buf: &mut String) {
        t.fq_key(name, version, buf);
    }

    fn merge(
        &self,
        (_n, _v, t): &Self::InsertItem,
        existing_item: Option<Self::StorageItem>,
    ) -> Option<Self::StorageItem> {
        let mut t = t.clone();
        t.stored_at = SystemTime::now();
        Some(match existing_item {
            Some(mut existing_item) => {
                existing_item.state.merge_with(&t.state);
                t.state = existing_item.state;
                t
            }
            None => t,
        })
    }
}

// FIXME: use it or drop it - it should be used once Sled can efficiently handle this kind of data
// as we currently use symlinks to mark completed HTML pages.
#[allow(dead_code)]
pub struct ReportsTree {
    inner: ThreadSafeConnection,
}

#[allow(dead_code)]
impl ReportsTree {
    pub fn key(
        crate_name: &str,
        crate_version: &str,
        report_name: &str,
        report_version: &str,
    ) -> Vec<u8> {
        format!(
            "{}:{}:{}:{}",
            crate_name, crate_version, report_name, report_version
        )
        .into()
    }
    pub fn is_done(&self, key: impl AsRef<[u8]>) -> bool {
        self.inner
            .lock()
            .query_row(
                &format!(
                    "SELECT value FROM report_done where key = {}",
                    std::str::from_utf8(key.as_ref()).expect("utf8 keys")
                ),
                NO_PARAMS,
                |_r| Ok(()),
            )
            .optional()
            .ok()
            .map(|_| true)
            .unwrap_or(false)
    }
    pub fn set_done(&self, key: impl AsRef<[u8]>) {
        self.inner
            .lock()
            .execute(
                "INSERT INTO report_done (key) VALUES (?1)",
                params![std::str::from_utf8(key.as_ref()).expect("utf8 keys")],
            )
            .ok();
    }
}

pub struct TaskResultTree {
    pub inner: ThreadSafeConnection,
}

impl TreeAccess for TaskResultTree {
    type StorageItem = TaskResult;
    type InsertItem = (String, String, Task, TaskResult);

    fn connection(&self) -> &ThreadSafeConnection {
        &self.inner
    }
    fn table_name(&self) -> &'static str {
        "result"
    }

    fn key_to_buf(v: &(String, String, Task, TaskResult), buf: &mut String) {
        v.3.fq_key(&v.0, &v.1, &v.2, buf);
    }

    fn merge(
        &self,
        new_item: &Self::InsertItem,
        _existing_item: Option<TaskResult>,
    ) -> Option<Self::StorageItem> {
        Some(new_item.3.clone().into())
    }
}

pub struct ContextTree {
    pub inner: ThreadSafeConnection,
}

impl TreeAccess for ContextTree {
    type StorageItem = Context;
    type InsertItem = Context;

    fn connection(&self) -> &ThreadSafeConnection {
        &self.inner
    }
    fn table_name(&self) -> &'static str {
        "meta"
    }

    fn key_to_buf(item: &Self::InsertItem, buf: &mut String) {
        item.key_buf(buf);
    }

    fn merge(&self, new: &Context, existing_item: Option<Context>) -> Option<Self::StorageItem> {
        existing_item
            .map(|existing| existing + new)
            .or_else(|| Some(new.clone()))
    }
}

impl ContextTree {
    pub fn update_today(&self, f: impl Fn(&mut Context)) -> Result<Context> {
        self.update(Self::key(&Context::default()), |mut c| {
            f(&mut c);
            c
        })
    }

    // NOTE: impl iterator is not allowed in traits unfortunately, but one could implement one manually
    pub fn most_recent(&self) -> Result<Option<(String, Context)>> {
        Ok(self
            .connection()
            .lock()
            .query_row(
                "SELECT key, data FROM meta ORDER BY key DESC limit 1",
                NO_PARAMS,
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)),
            )
            .optional()?
            .map(|(k, v)| (k, Context::from(v.as_slice()))))
    }
}

#[derive(Clone)]
pub struct CratesTree {
    pub inner: ThreadSafeConnection,
}

impl TreeAccess for CratesTree {
    type StorageItem = Crate;
    type InsertItem = crates_index_diff::CrateVersion;

    fn connection(&self) -> &ThreadSafeConnection {
        &self.inner
    }
    fn table_name(&self) -> &'static str {
        "crate"
    }

    fn key_to_buf(item: &crates_index_diff::CrateVersion, buf: &mut String) {
        item.key_buf(buf);
    }

    fn merge(
        &self,
        new_item: &crates_index_diff::CrateVersion,
        existing_item: Option<Crate>,
    ) -> Option<Crate> {
        Some(match existing_item {
            Some(mut c) => {
                if let Some(existing_version) = c
                    .versions
                    .iter_mut()
                    .find(|other| *other == &std::borrow::Cow::from(&new_item.version))
                {
                    *existing_version = new_item.version.to_owned().into();
                } else {
                    c.versions.push(new_item.version.to_owned().into());
                }
                c.versions.sort();
                c
            }
            None => Crate::from(new_item),
        })
    }
}

#[derive(Clone)]
pub struct CrateVersionsTree {
    pub inner: ThreadSafeConnection,
}

impl TreeAccess for CrateVersionsTree {
    type StorageItem = CrateVersion;
    type InsertItem = crates_index_diff::CrateVersion;

    fn connection(&self) -> &ThreadSafeConnection {
        &self.inner
    }
    fn table_name(&self) -> &'static str {
        "crate_version"
    }

    fn key_to_buf(v: &crates_index_diff::CrateVersion, buf: &mut String) {
        v.key_buf(buf);
    }

    fn merge(
        &self,
        new_item: &Self::InsertItem,
        _existing_item: Option<CrateVersion>,
    ) -> Option<Self::StorageItem> {
        Some(new_item.into())
    }
}
