//! Migrations with up and down paths and a dry run.
//!
//! A migration is a pair of files in the migrations directory,
//! `0001_create_users.up.sql` and `0001_create_users.down.sql`, applied in
//! name order. Applied migrations are recorded in `_migrations`, each in
//! its own transaction, so a failed one leaves the database as it was.

use std::path::Path;

use rusqlite::Connection;

use super::DbError;

/// One migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    /// Its name (`0001_create_users`).
    pub name: String,
    /// The SQL that applies it.
    pub up: String,
    /// The SQL that reverses it.
    pub down: String,
}

/// An ordered set of migrations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Migrations(pub Vec<Migration>);

impl Migrations {
    /// Reads a migrations directory.
    ///
    /// # Errors
    ///
    /// The directory or a file cannot be read, or an `up` has no `down`.
    pub fn from_dir(directory: impl AsRef<Path>) -> std::io::Result<Self> {
        let mut names = std::fs::read_dir(directory.as_ref())?
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter_map(|name| name.strip_suffix(".up.sql").map(str::to_owned))
            .collect::<Vec<_>>();
        names.sort();
        let read = |name: &str, kind: &str| {
            std::fs::read_to_string(directory.as_ref().join(format!("{name}.{kind}.sql")))
        };
        let migrations = names
            .into_iter()
            .map(|name| Ok(Migration { up: read(&name, "up")?, down: read(&name, "down")?, name }))
            .collect::<std::io::Result<Vec<_>>>()?;
        Ok(Self(migrations))
    }

    fn ensure_table(connection: &Connection) -> Result<(), DbError> {
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (name TEXT PRIMARY KEY NOT NULL, applied_at INTEGER NOT NULL)",
        )?;
        Ok(())
    }

    /// The applied migrations' names, in order.
    ///
    /// # Errors
    ///
    /// SQLite refused.
    pub fn applied(connection: &Connection) -> Result<Vec<String>, DbError> {
        Self::ensure_table(connection)?;
        let mut statement = connection.prepare("SELECT name FROM _migrations ORDER BY name")?;
        let names = statement
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(names)
    }

    /// What [`Self::apply`] would run: the dry run.
    ///
    /// # Errors
    ///
    /// SQLite refused.
    pub fn pending(&self, connection: &Connection) -> Result<Vec<&Migration>, DbError> {
        let applied = Self::applied(connection)?;
        Ok(self.0.iter().filter(|migration| !applied.contains(&migration.name)).collect())
    }

    /// Applies every pending migration; returns their names.
    ///
    /// # Errors
    ///
    /// A migration failed (it and the ones after it are not applied).
    pub fn apply(&self, connection: &mut Connection) -> Result<Vec<String>, DbError> {
        let pending = self.pending(connection)?.into_iter().cloned().collect::<Vec<_>>();
        let mut done = Vec::new();
        for migration in pending {
            let transaction = connection.transaction()?;
            transaction.execute_batch(&migration.up)?;
            transaction.execute(
                "INSERT INTO _migrations (name, applied_at) VALUES (?1, strftime('%s','now'))",
                [&migration.name],
            )?;
            transaction.commit()?;
            done.push(migration.name);
        }
        Ok(done)
    }

    /// Reverses applied migrations, newest first, until `target` is the
    /// newest applied (`None` reverses them all); returns their names.
    ///
    /// # Errors
    ///
    /// A down migration failed, or one applied is not in this set.
    pub fn rollback(
        &self,
        connection: &mut Connection,
        target: Option<&str>,
    ) -> Result<Vec<String>, DbError> {
        let applied = Self::applied(connection)?;
        let mut undone = Vec::new();
        for name in applied.iter().rev() {
            if Some(name.as_str()) == target {
                break;
            }
            let migration =
                self.0.iter().find(|migration| &migration.name == name).ok_or_else(|| {
                    DbError::Sqlite(rusqlite::Error::InvalidParameterName(format!(
                        "unknown migration {name}"
                    )))
                })?;
            let transaction = connection.transaction()?;
            transaction.execute_batch(&migration.down)?;
            transaction.execute("DELETE FROM _migrations WHERE name = ?1", [name])?;
            transaction.commit()?;
            undone.push(name.clone());
        }
        Ok(undone)
    }
}
