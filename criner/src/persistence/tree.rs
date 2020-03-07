use crate::{
    model::{Context, Crate, TaskResult},
    model::{CrateVersion, Task},
    persistence::{merge::Merge, Keyed},
    Result,
};
use rusqlite::{params, OptionalExtension, NO_PARAMS};
use std::time::{Duration, SystemTime};

/// Required as we send futures to threads. The type system can't statically prove that in fact
/// these connections will only ever be created while already in the thread they should execute on.
/// Also no one can prevent futures from being resumed in after having been send to a different thread.
pub type ThreadSafeConnection = std::sync::Arc<parking_lot::Mutex<rusqlite::Connection>>;

pub fn new_value_query<'conn>(
    table_name: &str,
    connection: &'conn rusqlite::Connection,
) -> Result<rusqlite::Statement<'conn>> {
    Ok(connection.prepare(&format!(
        "SELECT data FROM {} ORDER BY _rowid_ DESC",
        table_name
    ))?)
}

pub fn new_key_value_query<'conn>(
    table_name: &str,
    connection: &'conn rusqlite::Connection,
) -> Result<rusqlite::Statement<'conn>> {
    Ok(connection.prepare(&format!(
        "SELECT key,data FROM {} ORDER BY _rowid_ ASC",
        table_name
    ))?)
}

pub fn new_key_value_insertion<'conn>(
    table_name: &str,
    connection: &'conn rusqlite::Connection,
) -> Result<rusqlite::Statement<'conn>> {
    Ok(connection.prepare(&format!(
        "REPLACE INTO {} (key, data) VALUES (?1, ?2)",
        table_name
    ))?)
}

pub fn value_iter<'stm, 'conn, StorageItem>(
    statement: &'stm mut rusqlite::Statement<'conn>,
) -> Result<impl Iterator<Item = Result<StorageItem>> + 'stm>
where
    StorageItem: for<'a> From<&'a [u8]>,
{
    Ok(statement
        .query_map(NO_PARAMS, |r| {
            r.get::<_, Vec<u8>>(0)
                .map(|v| StorageItem::from(v.as_slice()))
        })?
        .map(|r| r.map_err(Into::into)))
}

pub fn key_value_iter<'stm, 'conn, StorageItem>(
    statement: &'stm mut rusqlite::Statement<'conn>,
) -> Result<impl Iterator<Item = Result<(String, StorageItem)>> + 'stm>
where
    StorageItem: for<'a> From<&'a [u8]>,
{
    Ok(statement
        .query_map(NO_PARAMS, |r| {
            let key = r.get::<_, String>(0)?;
            let data = r.get::<_, Vec<u8>>(1)?;
            Ok((key, StorageItem::from(data.as_slice())))
        })?
        .map(|r| r.map_err(Into::into)))
}

pub trait TreeAccess {
    type StorageItem: serde::Serialize + for<'a> From<&'a [u8]> + Default + From<Self::InsertItem>;
    type InsertItem: Clone;

    fn connection(&self) -> &ThreadSafeConnection;
    fn table_name() -> &'static str;

    fn merge(
        new_item: &Self::InsertItem,
        _existing_item: Option<Self::StorageItem>,
    ) -> Self::StorageItem {
        Self::StorageItem::from(new_item.clone())
    }

    fn into_connection(self) -> ThreadSafeConnection;

    fn count(&self) -> u64 {
        self.connection()
            .lock()
            .query_row(
                &format!("SELECT COUNT(*) FROM {}", Self::table_name()),
                NO_PARAMS,
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0) as u64
    }

    fn get(&self, key: impl AsRef<str>) -> Result<Option<Self::StorageItem>> {
        retry_on_db_lock(|| {
            Ok(self
                .connection()
                .lock()
                .query_row(
                    &format!(
                        "SELECT data FROM {} WHERE key = '{}'",
                        Self::table_name(),
                        key.as_ref()
                    ),
                    NO_PARAMS,
                    |r| r.get::<_, Vec<u8>>(0),
                )
                .optional()?
                .map(|d| Self::StorageItem::from(d.as_slice())))
        })
    }

    /// Update an existing item, or create it as default, returning the stored item
    /// f(existing) should merge the items as desired
    fn update(
        &self,
        key: impl AsRef<str>,
        f: impl Fn(Self::StorageItem) -> Self::StorageItem,
    ) -> Result<Self::StorageItem> {
        retry_on_db_lock(|| {
            let mut guard = self.connection().lock();
            let transaction = guard.savepoint()?;
            let new_value = transaction
                .query_row(
                    &format!(
                        "SELECT data FROM {} WHERE key = '{}'",
                        Self::table_name(),
                        key.as_ref()
                    ),
                    NO_PARAMS,
                    |r| r.get::<_, Vec<u8>>(0),
                )
                .optional()?
                .map_or_else(
                    || f(Self::StorageItem::default()),
                    |d| f(d.as_slice().into()),
                );
            transaction.execute(
                &format!(
                    "REPLACE INTO {} (key, data) VALUES (?1, ?2)",
                    Self::table_name()
                ),
                params![key.as_ref(), rmp_serde::to_vec(&new_value)?],
            )?;
            transaction.commit()?;

            Ok(new_value)
        })
    }

    /// Similar to 'update', but provides full control over the default and allows deletion
    fn upsert(&self, key: impl AsRef<str>, item: &Self::InsertItem) -> Result<Self::StorageItem> {
        retry_on_db_lock(|| {
            let mut guard = self.connection().lock();
            let transaction = guard.savepoint()?;

            let new_value = {
                let maybe_vec = transaction
                    .query_row(
                        &format!(
                            "SELECT data FROM {} WHERE key = '{}'",
                            Self::table_name(),
                            key.as_ref()
                        ),
                        NO_PARAMS,
                        |r| r.get::<_, Vec<u8>>(0),
                    )
                    .optional()?;
                Self::merge(item, maybe_vec.map(|v| v.as_slice().into()))
            };
            transaction.execute(
                &format!(
                    "REPLACE INTO {} (key, data) VALUES (?1, ?2)",
                    Self::table_name()
                ),
                params![key.as_ref(), rmp_serde::to_vec(&new_value)?],
            )?;
            transaction.commit()?;
            Ok(new_value)
        })
    }

    fn insert(&self, key: impl AsRef<str>, v: &Self::InsertItem) -> Result<()> {
        retry_on_db_lock(|| {
            self.connection().lock().execute(
                &format!(
                    "REPLACE INTO {} (key, data) VALUES (?1, ?2)",
                    Self::table_name()
                ),
                params![key.as_ref(), rmp_serde::to_vec(&Self::merge(v, None))?],
            )?;
            Ok(())
        })
    }
}

fn retry_on_db_lock<T>(mut f: impl FnMut() -> Result<T>) -> Result<T> {
    use crate::Error;
    use rusqlite::ffi::Error as SqliteFFIError;
    use rusqlite::ffi::ErrorCode as SqliteFFIErrorCode;
    use rusqlite::Error as SqliteError;

    let max_wait_ms = Duration::from_secs(100);
    let mut total_wait_time = Duration::default();
    let mut wait_for = Duration::from_millis(1);
    let max_wait_time = Duration::from_millis(250);
    loop {
        total_wait_time += wait_for;
        match f() {
            Ok(v) => return Ok(v),
            Err(
                err
                @
                Error::Rusqlite(SqliteError::SqliteFailure(
                    SqliteFFIError {
                        code: SqliteFFIErrorCode::DatabaseBusy,
                        extended_code: _,
                    },
                    _,
                )),
            )
            | Err(
                err
                @
                Error::Rusqlite(SqliteError::SqliteFailure(
                    SqliteFFIError {
                        code: SqliteFFIErrorCode::DatabaseLocked,
                        extended_code: _,
                    },
                    _,
                )),
            ) => {
                if total_wait_time >= max_wait_ms {
                    log::warn!(
                        "Giving up to wait for {:?} after {:?})",
                        err,
                        total_wait_time
                    );
                    return Err(err);
                }
                log::warn!(
                    "Waiting 1ms for {:?} (total wait time {:?})",
                    err,
                    total_wait_time
                );
                std::thread::sleep(wait_for);
                wait_for = (wait_for * 2).min(max_wait_time);
            }
            Err(err) => return Err(err),
        }
    }
}

pub struct TasksTree {
    pub inner: ThreadSafeConnection,
}

impl TreeAccess for TasksTree {
    type StorageItem = Task;
    type InsertItem = Task;

    fn connection(&self) -> &ThreadSafeConnection {
        &self.inner
    }
    fn table_name() -> &'static str {
        "task"
    }
    fn merge(
        new_task: &Self::InsertItem,
        existing_task: Option<Self::StorageItem>,
    ) -> Self::StorageItem {
        Task {
            stored_at: SystemTime::now(),
            ..existing_task.map_or_else(
                || new_task.clone(),
                |existing_task| existing_task.merge(new_task),
            )
        }
    }

    fn into_connection(self) -> ThreadSafeConnection {
        self.inner
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
    type InsertItem = TaskResult;

    fn connection(&self) -> &ThreadSafeConnection {
        &self.inner
    }
    fn table_name() -> &'static str {
        "result"
    }
    fn into_connection(self) -> ThreadSafeConnection {
        self.inner
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
    fn table_name() -> &'static str {
        "meta"
    }

    fn merge(new: &Context, existing_item: Option<Context>) -> Self::StorageItem {
        existing_item.map_or_else(|| new.to_owned(), |existing| existing.merge(new))
    }
    fn into_connection(self) -> ThreadSafeConnection {
        self.inner
    }
}

impl ContextTree {
    pub fn update_today(&self, f: impl Fn(&mut Context)) -> Result<Context> {
        self.update(Context::default().key(), |mut c| {
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
    type InsertItem = CrateVersion;

    fn connection(&self) -> &ThreadSafeConnection {
        &self.inner
    }
    fn table_name() -> &'static str {
        "crate"
    }

    fn merge(new_item: &CrateVersion, existing_item: Option<Crate>) -> Crate {
        existing_item.map_or_else(|| Crate::from(new_item.to_owned()), |c| c.merge(new_item))
    }
    fn into_connection(self) -> ThreadSafeConnection {
        self.inner
    }
}

#[derive(Clone)]
pub struct CrateVersionsTree {
    pub inner: ThreadSafeConnection,
}

impl TreeAccess for CrateVersionsTree {
    type StorageItem = CrateVersion;
    type InsertItem = CrateVersion;

    fn connection(&self) -> &ThreadSafeConnection {
        &self.inner
    }
    fn table_name() -> &'static str {
        "crate_version"
    }
    fn into_connection(self) -> ThreadSafeConnection {
        self.inner
    }
}
