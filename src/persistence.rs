use crate::model::{CrateVersion, Task, TaskResult};
use crate::{
    error::{Error, Result},
    model::{Context, Crate},
};
use sled::{IVec, Tree};
use std::{path::Path, time::SystemTime};

#[derive(Clone)]
pub struct Db {
    pub inner: sled::Db,
    meta: sled::Tree,
    versions: sled::Tree,
    crates: sled::Tree,
    tasks: sled::Tree,
    results: sled::Tree,
}

impl Db {
    pub fn open(path: impl AsRef<Path>) -> Result<Db> {
        // NOTE: Default compression achieves cutting disk space in half, but the processing speed is cut in half
        // for our binary data as well.
        // TODO: re-evaluate that for textual data - it might enable us to store all files, and when we
        // have more read-based workloads. Maybe it's worth it to turn on.
        // NOTE: Databases with and without compression need migration.
        let inner = sled::Config::new().path(path).open()?;
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

const KEY_SEP: u8 = b':';

pub trait Keyed {
    fn key_bytes_buf(&self, buf: &mut Vec<u8>);
    fn key_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(16);
        self.key_bytes_buf(&mut buf);
        buf
    }
    fn key_string(&self) -> Result<String> {
        String::from_utf8(self.key_bytes()).map_err(Into::into)
    }
}

impl<'a> Task<'a> {
    pub fn key_from(process: &str, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&process.as_bytes());
    }
}

impl<'a> Keyed for Task<'a> {
    fn key_bytes_buf(&self, buf: &mut Vec<u8>) {
        Task::key_from(&self.process, buf)
    }
}

impl Keyed for crates_index_diff::CrateVersion {
    fn key_bytes_buf(&self, buf: &mut Vec<u8>) {
        CrateVersion::key_from(&self.name, &self.version, buf)
    }
}

impl<'a> Keyed for CrateVersion<'a> {
    fn key_bytes_buf(&self, buf: &mut Vec<u8>) {
        CrateVersion::key_from(&self.name, &self.version, buf)
    }
}

impl<'a> CrateVersion<'a> {
    pub fn key_from(name: &str, version: &str, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&name.as_bytes());
        buf.push(KEY_SEP);
        buf.extend_from_slice(&version.as_bytes());
    }
}

pub trait TreeAccess {
    type StorageItem: From<IVec> + Into<IVec> + for<'a> From<&'a [u8]> + Default;
    type InsertItem: serde::Serialize;
    type InsertResult;

    fn tree(&self) -> &sled::Tree;
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
        f: impl Fn(&mut Self::StorageItem),
    ) -> Result<Self::StorageItem> {
        self.tree()
            .update_and_fetch(key, |bytes: Option<&[u8]>| {
                Some(match bytes {
                    Some(bytes) => {
                        let mut v = bytes.into();
                        f(&mut v);
                        v.into()
                    }
                    None => {
                        let mut v = Self::StorageItem::default();
                        f(&mut v);
                        v.into()
                    }
                })
            })?
            .map(From::from)
            .ok_or_else(|| Error::Bug("We always set a value"))
    }

    /// Similar to 'update', but provides full control over the default
    fn upsert(&self, item: &Self::InsertItem) -> Result<Self::InsertResult> {
        self.tree()
            .update_and_fetch(Self::key(item), |existing: Option<&[u8]>| {
                self.merge(item, existing.map(From::from)).map(Into::into)
            })?
            .ok_or_else(|| Error::Bug("We always put a value or update the existing one"))
            .map(|v| self.map_insert_return_value(v))
    }

    fn insert(&self, v: &Self::InsertItem) -> Result<()> {
        self.tree()
            .insert(Self::key(v), rmp_serde::to_vec(v)?)
            .map_err(Error::from)
            .map(|_| ())
    }
}

pub struct TasksTree<'a> {
    inner: &'a sled::Tree,
}

impl<'a> TreeAccess for TasksTree<'a> {
    type StorageItem = Task<'a>;
    type InsertItem = (&'a str, &'a str, Task<'a>);
    type InsertResult = Task<'a>;

    fn tree(&self) -> &Tree {
        self.inner
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
            Some(existing_item) => {
                let new_state = existing_item.state.merged(&t.state);
                t.state = new_state;
                t
            }
            None => t,
        })
    }
}

pub struct TaskResultTree<'a> {
    inner: &'a sled::Tree,
}

impl<'a> Keyed for TaskResult<'a> {
    fn key_bytes_buf(&self, buf: &mut Vec<u8>) {
        match self {
            TaskResult::Download { kind, .. } => buf.extend_from_slice(kind.as_bytes()),
            TaskResult::None => {}
        }
    }
}

impl<'a> TreeAccess for TaskResultTree<'a> {
    type StorageItem = TaskResult<'a>;
    type InsertItem = (&'a str, &'a str, &'a Task<'a>, TaskResult<'a>);
    type InsertResult = ();

    fn tree(&self) -> &Tree {
        self.inner
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
    inner: &'a sled::Tree,
}

impl<'a> TreeAccess for ContextTree<'a> {
    type StorageItem = Context;
    type InsertItem = Context;
    type InsertResult = ();

    fn tree(&self) -> &Tree {
        self.inner
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
        self.update(Self::key(&Context::default()), f)
    }

    // NOTE: impl iterator is not allowed in traits unfortunately, but one could implement one manually
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = Result<(String, Context)>> {
        self.inner.iter().map(|r| {
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
    inner: &'a sled::Tree,
}

impl<'a> TreeAccess for CratesTree<'a> {
    type StorageItem = Crate<'a>;
    type InsertItem = crates_index_diff::CrateVersion;
    type InsertResult = bool;

    fn tree(&self) -> &Tree {
        self.inner
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
                // NOTE: We assume that a version can only be added once! They are immutable.
                // However, idempotence is more important
                if !c
                    .versions
                    .contains(&std::borrow::Cow::from(&new_item.version))
                {
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
    inner: &'a sled::Tree,
}

impl<'a> TreeAccess for CrateVersionsTree<'a> {
    type StorageItem = CrateVersion<'a>;
    type InsertItem = crates_index_diff::CrateVersion;
    type InsertResult = ();

    fn tree(&self) -> &Tree {
        self.inner
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

macro_rules! impl_ivec_transform {
    ($ty:ty) => {
        impl From<&[u8]> for $ty {
            fn from(b: &[u8]) -> Self {
                rmp_serde::from_read(b).expect("always valid decoding: TODO: migrations")
            }
        }
        impl From<IVec> for $ty {
            fn from(b: IVec) -> Self {
                rmp_serde::from_read(b.as_ref()).expect("always valid decoding: TODO: migrations")
            }
        }
        impl From<$ty> for IVec {
            fn from(c: $ty) -> Self {
                rmp_serde::to_vec(&c)
                    .expect("serialization to always succeed")
                    .into()
            }
        }
    };
}

impl_ivec_transform!(Crate<'_>);
impl_ivec_transform!(Task<'_>);
impl_ivec_transform!(TaskResult<'_>);
impl_ivec_transform!(CrateVersion<'_>);
impl_ivec_transform!(Context);
