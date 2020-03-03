use crate::model::{Context, Crate, TaskResult};
use crate::{
    model::{CrateVersion, ReportResult, Task},
    persistence::{Keyed, KEY_SEP},
    Error, Result,
};
use rusqlite::{params, OptionalExtension, NO_PARAMS};
use sled::IVec;
use std::time::SystemTime;

/// Required as we send futures to threads. The type system can't statically prove that in fact
/// these connections will only ever be created while already in the thread they should execute on.
/// Also no one can prevent futures from being resumed in after having been send to a different thread.
pub type ThreadSafeConnection = std::sync::Arc<parking_lot::Mutex<rusqlite::Connection>>;

pub trait TreeAccess {
    type StorageItem: serde::Serialize + From<IVec> + Into<IVec> + for<'a> From<&'a [u8]> + Default;
    type InsertItem;
    type InsertResult;

    fn tree(&self) -> &sled::Tree;
    fn connection(&self) -> &ThreadSafeConnection;
    fn table_name(&self) -> &'static str;

    fn key(item: &Self::InsertItem) -> Vec<u8> {
        let mut buf = Vec::with_capacity(16);
        Self::key_to_buf(item, &mut buf);
        buf
    }
    fn key_to_buf(item: &Self::InsertItem, buf: &mut Vec<u8>);
    fn map_insert_return_value(&self, v: IVec) -> Self::InsertResult;
    fn merge(
        &self,
        new_item: &Self::InsertItem,
        existing_item: Option<Self::StorageItem>,
    ) -> Option<Self::StorageItem>;

    fn get(&self, key: impl AsRef<[u8]>) -> Result<Option<Self::StorageItem>> {
        self.tree()
            .get(key)
            .map_err(Into::into)
            .map(|r| r.map(Into::into))
    }

    /// Update an existing item, or create it as default, returning the stored item
    fn update(
        &self,
        key: impl AsRef<[u8]>,
        f: impl Fn(Self::StorageItem) -> Self::StorageItem,
    ) -> Result<Self::StorageItem> {
        let res = self
            .tree()
            .update_and_fetch(key.as_ref(), |bytes: Option<&[u8]>| {
                Some(match bytes {
                    Some(bytes) => f(bytes.into()).into(),
                    None => f(Self::StorageItem::default()).into(),
                })
            })?
            .map(From::from)
            .ok_or_else(|| Error::Bug("We always set a value"));

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
        // NOTE: Copied from insert - can't use it now as it also inserts to sled.
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

        return res;
    }

    /// Similar to 'update', but provides full control over the default and allows deletion
    fn upsert(&self, item: &Self::InsertItem) -> Result<Self::InsertResult> {
        let res = self
            .tree()
            .update_and_fetch(Self::key(item), |existing: Option<&[u8]>| {
                self.merge(item, existing.map(From::from)).map(Into::into)
            })?
            .ok_or_else(|| Error::Bug("We always put a value or update the existing one"))
            .map(|v| self.map_insert_return_value(v));

        let mut guard = self.connection().lock();
        let key = Self::key(item);
        let key_str = std::str::from_utf8(key.as_slice()).expect("utf8-keys");

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
            }
            None => todo!("deletion of values - I don't think we need that"),
        };

        res
    }

    fn insert(&self, v: &Self::InsertItem) -> Result<()> {
        self.tree()
            .insert(
                Self::key(v),
                rmp_serde::to_vec(&self.merge(v, None).unwrap_or_else(Default::default))?,
            )
            .map_err(Error::from)
            .map(|_| ())?;

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

pub struct TasksTree<'a> {
    pub inner: (&'a sled::Tree, ThreadSafeConnection),
}

impl<'a> TreeAccess for TasksTree<'a> {
    type StorageItem = Task<'a>;
    type InsertItem = (&'a str, &'a str, Task<'a>);
    type InsertResult = Task<'a>;

    fn tree(&self) -> &sled::Tree {
        self.inner.0
    }

    fn connection(&self) -> &ThreadSafeConnection {
        &self.inner.1
    }
    fn table_name(&self) -> &'static str {
        "task"
    }

    fn key_to_buf((name, version, t): &Self::InsertItem, buf: &mut Vec<u8>) {
        CrateVersion::key_from(name, version, buf);
        buf.push(KEY_SEP);
        t.key_bytes_buf(buf);
    }

    fn map_insert_return_value(&self, v: IVec) -> Self::InsertResult {
        v.into()
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
pub struct ReportsTree<'a> {
    inner: (&'a sled::Tree, ThreadSafeConnection),
}

#[allow(dead_code)]
impl<'a> ReportsTree<'a> {
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
        self.inner.0.contains_key(key).unwrap_or(false)
    }
    pub fn set_done(&self, key: impl AsRef<[u8]>) {
        self.inner.0.insert(key, ReportResult::Done).ok();
    }
}

pub struct TaskResultTree<'a> {
    pub inner: (&'a sled::Tree, ThreadSafeConnection),
}

impl<'a> TreeAccess for TaskResultTree<'a> {
    type StorageItem = TaskResult<'a>;
    type InsertItem = (&'a str, &'a str, &'a Task<'a>, TaskResult<'a>);
    type InsertResult = ();

    fn tree(&self) -> &sled::Tree {
        self.inner.0
    }
    fn connection(&self) -> &ThreadSafeConnection {
        &self.inner.1
    }
    fn table_name(&self) -> &'static str {
        "result"
    }

    fn key_to_buf(v: &(&str, &str, &Task, TaskResult<'a>), buf: &mut Vec<u8>) {
        TasksTree::key_to_buf(&(v.0, v.1, v.2.clone()), buf);
        buf.push(KEY_SEP);
        buf.extend_from_slice(v.2.version.as_bytes());
        buf.push(KEY_SEP);
        v.3.key_bytes_buf(buf);
    }

    fn map_insert_return_value(&self, _v: IVec) -> Self::InsertResult {
        ()
    }

    fn merge(
        &self,
        new_item: &Self::InsertItem,
        _existing_item: Option<TaskResult>,
    ) -> Option<Self::StorageItem> {
        Some(new_item.3.clone().into())
    }
}

pub struct ContextTree<'a> {
    pub inner: (&'a sled::Tree, ThreadSafeConnection),
}

impl<'a> TreeAccess for ContextTree<'a> {
    type StorageItem = Context;
    type InsertItem = Context;
    type InsertResult = ();

    fn tree(&self) -> &sled::Tree {
        self.inner.0
    }
    fn connection(&self) -> &ThreadSafeConnection {
        &self.inner.1
    }
    fn table_name(&self) -> &'static str {
        "meta"
    }

    fn key_to_buf(_item: &Self::InsertItem, buf: &mut Vec<u8>) {
        buf.extend_from_slice(
            format!(
                "context/{}",
                humantime::format_rfc3339(SystemTime::now())
                    .to_string()
                    .get(..10)
                    .expect("YYYY-MM-DD - 10 bytes")
            )
            .as_bytes(),
        );
    }

    fn map_insert_return_value(&self, _v: IVec) -> Self::InsertResult {
        ()
    }

    fn merge(&self, new: &Context, existing_item: Option<Context>) -> Option<Self::StorageItem> {
        existing_item
            .map(|existing| existing + new)
            .or_else(|| Some(new.clone()))
    }
}

impl<'a> ContextTree<'a> {
    pub fn update_today(&self, f: impl Fn(&mut Context)) -> Result<Context> {
        self.update(Self::key(&Context::default()), |mut c| {
            f(&mut c);
            c
        })
    }

    // NOTE: impl iterator is not allowed in traits unfortunately, but one could implement one manually
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = Result<(String, Context)>> {
        self.inner.0.iter().map(|r| {
            r.map(|(k, v)| {
                (
                    String::from_utf8(k.as_ref().to_vec()).expect("utf8"),
                    Context::from(v),
                )
            })
            .map_err(Error::from)
        })
    }
}

#[derive(Clone)]
pub struct CratesTree<'a> {
    pub inner: (&'a sled::Tree, ThreadSafeConnection),
}

impl<'a> TreeAccess for CratesTree<'a> {
    type StorageItem = Crate<'a>;
    type InsertItem = crates_index_diff::CrateVersion;
    type InsertResult = bool;

    fn tree(&self) -> &sled::Tree {
        self.inner.0
    }
    fn connection(&self) -> &ThreadSafeConnection {
        &self.inner.1
    }
    fn table_name(&self) -> &'static str {
        "crate"
    }

    fn key_to_buf(item: &crates_index_diff::CrateVersion, buf: &mut Vec<u8>) {
        buf.extend_from_slice(item.name.as_bytes());
    }

    fn map_insert_return_value(&self, v: IVec) -> Self::InsertResult {
        let c = Crate::from(v);
        c.versions.len() == 1
    }

    fn merge(
        &self,
        new_item: &crates_index_diff::CrateVersion,
        existing_item: Option<Crate<'a>>,
    ) -> Option<Crate<'a>> {
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
pub struct CrateVersionsTree<'a> {
    pub inner: (&'a sled::Tree, ThreadSafeConnection),
}

impl<'a> TreeAccess for CrateVersionsTree<'a> {
    type StorageItem = CrateVersion<'a>;
    type InsertItem = crates_index_diff::CrateVersion;
    type InsertResult = ();

    fn tree(&self) -> &sled::Tree {
        self.inner.0
    }
    fn connection(&self) -> &ThreadSafeConnection {
        &self.inner.1
    }
    fn table_name(&self) -> &'static str {
        "crate_version"
    }

    fn key_to_buf(v: &crates_index_diff::CrateVersion, buf: &mut Vec<u8>) {
        v.key_bytes_buf(buf);
    }

    fn map_insert_return_value(&self, _v: IVec) -> Self::InsertResult {
        ()
    }

    fn merge(
        &self,
        new_item: &Self::InsertItem,
        _existing_item: Option<CrateVersion>,
    ) -> Option<Self::StorageItem> {
        Some(new_item.into())
    }
}
