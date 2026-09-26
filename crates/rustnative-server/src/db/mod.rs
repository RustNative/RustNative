//! The data layer: a pooled SQLite database (`rusqlite`, bundled),
//! transactions tied to a request's scope, compile-time-checked queries
//! ([`crate::query!`]), migrations with up and down paths and a dry run,
//! and migrations generated from model changes ([`schema`], `C38`).
//!
//! Blocking database work runs on the runtime's blocking pool
//! ([`Db::run`]), never on the thread answering requests.

pub mod migrate;
pub mod policy;
pub mod schema;

use std::sync::{Arc, Condvar, Mutex, PoisonError};

pub use rusqlite;
use rusqlite::types::{ToSqlOutput, Value, ValueRef};
use rusqlite::{Connection, OpenFlags, ToSql};

use crate::scope::RequestScope;

/// Why a database operation failed.
#[derive(Debug)]
pub enum DbError {
    /// SQLite refused.
    Sqlite(rusqlite::Error),
    /// The request ended first, so the transaction was rolled back.
    Cancelled,
    /// The blocking task failed.
    Task(String),
}

impl std::fmt::Display for DbError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "{error}"),
            Self::Cancelled => {
                formatter.write_str("the request ended; the transaction was rolled back")
            }
            Self::Task(error) => write!(formatter, "database task: {error}"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<rusqlite::Error> for DbError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<DbError> for crate::response::ServerError {
    fn from(error: DbError) -> Self {
        Self::internal(error.to_string())
    }
}

struct Pool {
    idle: Mutex<Vec<Connection>>,
    returned: Condvar,
}

/// A database with a fixed-size connection pool; cloning shares it.
#[derive(Clone)]
pub struct Db {
    pool: Arc<Pool>,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Db")
    }
}

/// A connection borrowed from the pool, returned when dropped.
pub struct Pooled {
    connection: Option<Connection>,
    pool: Arc<Pool>,
}

impl std::ops::Deref for Pooled {
    type Target = Connection;
    #[allow(clippy::expect_used, reason = "present until drop, by construction")]
    fn deref(&self) -> &Connection {
        self.connection.as_ref().expect("a pooled connection is present until dropped")
    }
}

impl std::ops::DerefMut for Pooled {
    #[allow(clippy::expect_used, reason = "present until drop, by construction")]
    fn deref_mut(&mut self) -> &mut Connection {
        self.connection.as_mut().expect("a pooled connection is present until dropped")
    }
}

impl Drop for Pooled {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            self.pool.idle.lock().unwrap_or_else(PoisonError::into_inner).push(connection);
            self.pool.returned.notify_one();
        }
    }
}

impl Db {
    /// Opens the database file at `path` with `size` connections, in WAL
    /// mode with foreign keys on.
    ///
    /// # Errors
    ///
    /// The file cannot be opened.
    pub fn open(path: &str, size: usize) -> Result<Self, DbError> {
        Self::with(size, || Connection::open(path))
    }

    /// A private in-memory database named `name`, shared by the pool's
    /// connections (for tests and examples).
    ///
    /// # Errors
    ///
    /// SQLite cannot create it.
    pub fn memory(name: &str, size: usize) -> Result<Self, DbError> {
        let uri = format!("file:{name}?mode=memory&cache=shared");
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        Self::with(size, || Connection::open_with_flags(&uri, flags))
    }

    fn with(size: usize, open: impl Fn() -> rusqlite::Result<Connection>) -> Result<Self, DbError> {
        let mut idle = Vec::with_capacity(size.max(1));
        for _ in 0..size.max(1) {
            let connection = open()?;
            connection.busy_timeout(std::time::Duration::from_secs(5))?;
            connection.pragma_update(None, "foreign_keys", true)?;
            let _ = connection.pragma_update(None, "journal_mode", "WAL");
            idle.push(connection);
        }
        Ok(Self { pool: Arc::new(Pool { idle: Mutex::new(idle), returned: Condvar::new() }) })
    }

    /// Borrows a connection, waiting for one if all are in use.
    #[must_use]
    pub fn get(&self) -> Pooled {
        let mut idle = self.pool.idle.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(connection) = idle.pop() {
                return Pooled { connection: Some(connection), pool: Arc::clone(&self.pool) };
            }
            idle = self.pool.returned.wait(idle).unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Runs `work` with a connection on the blocking pool.
    ///
    /// # Errors
    ///
    /// `work` failed, or the task did.
    pub async fn run<T, F>(&self, work: F) -> Result<T, DbError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, DbError> + Send + 'static,
    {
        let db = self.clone();
        tokio::task::spawn_blocking(move || work(&mut db.get()))
            .await
            .map_err(|error| DbError::Task(error.to_string()))?
    }

    /// Runs `work` in a transaction tied to `scope`: committed if `work`
    /// succeeds while the request is still live, rolled back if it fails
    /// or the request has ended — as a component's tasks end with it.
    ///
    /// # Errors
    ///
    /// `work` failed, or [`DbError::Cancelled`].
    pub async fn transaction<T, F>(&self, scope: &RequestScope, work: F) -> Result<T, DbError>
    where
        T: Send + 'static,
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<T, DbError> + Send + 'static,
    {
        let scope = scope.clone();
        self.run(move |connection| {
            let transaction = connection.transaction()?;
            let value = work(&transaction)?;
            if scope.is_cancelled() {
                transaction.rollback()?;
                return Err(DbError::Cancelled);
            }
            transaction.commit()?;
            Ok(value)
        })
        .await
    }
}

/// A parameter, owned, so a query can cross to the blocking pool.
#[must_use]
pub fn param<T: ToSql + ?Sized>(value: &T) -> Value {
    match value.to_sql() {
        Ok(ToSqlOutput::Owned(owned)) => owned,
        Ok(ToSqlOutput::Borrowed(borrowed)) => match borrowed {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(integer) => Value::Integer(integer),
            ValueRef::Real(real) => Value::Real(real),
            ValueRef::Text(text) => Value::Text(String::from_utf8_lossy(text).into_owned()),
            ValueRef::Blob(blob) => Value::Blob(blob.to_vec()),
        },
        _ => Value::Null,
    }
}

/// A checked query (from [`crate::query!`]): its SQL, parameters, and how
/// to read a row.
pub struct Query<R> {
    sql: &'static str,
    params: Vec<Value>,
    read: fn(&rusqlite::Row<'_>) -> rusqlite::Result<R>,
}

impl<R> Query<R> {
    /// Used by [`crate::query!`].
    #[doc(hidden)]
    #[must_use]
    pub fn new(
        sql: &'static str,
        params: Vec<Value>,
        read: fn(&rusqlite::Row<'_>) -> rusqlite::Result<R>,
    ) -> Self {
        Self { sql, params, read }
    }

    /// Every row.
    ///
    /// # Errors
    ///
    /// SQLite refused.
    pub fn fetch_all(&self, connection: &Connection) -> Result<Vec<R>, DbError> {
        let mut statement = connection.prepare(self.sql)?;
        let rows =
            statement.query_map(rusqlite::params_from_iter(self.params.iter()), self.read)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(DbError::from)
    }

    /// The first row, if any.
    ///
    /// # Errors
    ///
    /// SQLite refused.
    pub fn fetch_optional(&self, connection: &Connection) -> Result<Option<R>, DbError> {
        Ok(self.fetch_all(connection)?.into_iter().next())
    }

    /// Runs a statement that returns no rows; the number of rows changed.
    ///
    /// # Errors
    ///
    /// SQLite refused.
    pub fn execute(&self, connection: &Connection) -> Result<usize, DbError> {
        let mut statement = connection.prepare(self.sql)?;
        statement.execute(rusqlite::params_from_iter(self.params.iter())).map_err(DbError::from)
    }
}
