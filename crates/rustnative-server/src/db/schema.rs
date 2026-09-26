//! Migrations generated from model changes (`C38`).
//!
//! The model is declared in `schema.toml`; the migrations directory is the
//! history. [`diff`] compares the model with the schema the migrations
//! produce and writes the next migration — up and down — asking about
//! anything that might be a rename rather than a drop and an add. The CLI
//! runs it as `rustnative db diff`; [`squash`] folds a history into one
//! migration.
//!
//! ```toml
//! [tables.users]
//! columns = [
//!   { name = "id", type = "INTEGER", primary_key = true },
//!   { name = "email", type = "TEXT", not_null = true },
//! ]
//! ```

use std::collections::BTreeMap;
use std::fmt::Write as _;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::DbError;
use super::migrate::{Migration, Migrations};

/// A column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    /// Its name.
    pub name: String,
    /// Its SQL type.
    #[serde(rename = "type")]
    pub kind: String,
    /// `NOT NULL`.
    #[serde(default)]
    pub not_null: bool,
    /// Part of the primary key.
    #[serde(default)]
    pub primary_key: bool,
    /// Its default, as SQL.
    #[serde(default)]
    pub default: Option<String>,
}

impl Column {
    fn definition(&self) -> String {
        let mut sql = format!("{} {}", self.name, self.kind);
        if self.primary_key {
            sql.push_str(" PRIMARY KEY");
        }
        if self.not_null {
            sql.push_str(" NOT NULL");
        }
        if let Some(default) = &self.default {
            let _ = write!(sql, " DEFAULT {default}");
        }
        sql
    }
}

/// A table.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Table {
    /// Its columns, in order.
    pub columns: Vec<Column>,
}

/// A schema: tables by name.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Schema {
    /// The tables.
    pub tables: BTreeMap<String, Table>,
}

impl Schema {
    /// Parses `schema.toml`.
    ///
    /// # Errors
    ///
    /// It is not a valid schema.
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|error| error.to_string())
    }

    /// The schema of a database.
    ///
    /// # Errors
    ///
    /// SQLite refused.
    pub fn of(connection: &Connection) -> Result<Self, DbError> {
        let mut statement = connection.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name <> '_migrations' ORDER BY name",
        )?;
        let names = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut tables = BTreeMap::new();
        for name in names {
            let mut info = connection.prepare(&format!("PRAGMA table_info(\"{name}\")"))?;
            let columns = info
                .query_map([], |row| {
                    Ok(Column {
                        name: row.get(1)?,
                        kind: row.get(2)?,
                        not_null: row.get::<_, i64>(3)? != 0,
                        default: row.get(4)?,
                        primary_key: row.get::<_, i64>(5)? != 0,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            tables.insert(name, Table { columns });
        }
        Ok(Self { tables })
    }

    /// The schema `migrations` produce.
    ///
    /// # Errors
    ///
    /// A migration fails.
    pub fn from_migrations(migrations: &Migrations) -> Result<Self, DbError> {
        let mut connection = Connection::open_in_memory()?;
        migrations.apply(&mut connection)?;
        Self::of(&connection)
    }
}

/// A column that disappeared while another of the same type appeared in
/// the same table: perhaps a rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PossibleRename {
    /// The table.
    pub table: String,
    /// The column that went.
    pub from: String,
    /// The column that came.
    pub to: String,
}

/// The next migration, and the renames it could not decide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Applies the change.
    pub up: String,
    /// Reverses it.
    pub down: String,
    /// Drop-and-add pairs that might be renames; confirm them and diff
    /// again with them in `renames`.
    pub possible_renames: Vec<PossibleRename>,
}

impl Plan {
    /// Whether there is nothing to do.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.up.is_empty()
    }
}

fn create(name: &str, table: &Table) -> String {
    let columns = table.columns.iter().map(Column::definition).collect::<Vec<_>>().join(", ");
    format!("CREATE TABLE {name} ({columns});\n")
}

/// The migration from `current` to `desired`. `renames` are confirmed
/// renames (`PossibleRename`s answered yes).
#[must_use]
pub fn diff(current: &Schema, desired: &Schema, renames: &[PossibleRename]) -> Plan {
    let (mut up, mut down) = (String::new(), String::new());
    let mut possible = Vec::new();
    for (name, table) in &desired.tables {
        let Some(existing) = current.tables.get(name) else {
            up.push_str(&create(name, table));
            down.insert_str(0, &format!("DROP TABLE {name};\n"));
            continue;
        };
        let has = |columns: &[Column], column: &str| {
            columns.iter().any(|candidate| candidate.name == column)
        };
        let confirmed: Vec<&PossibleRename> =
            renames.iter().filter(|rename| &rename.table == name).collect();
        for rename in &confirmed {
            let _ =
                writeln!(up, "ALTER TABLE {name} RENAME COLUMN {} TO {};", rename.from, rename.to);
            down.insert_str(
                0,
                &format!("ALTER TABLE {name} RENAME COLUMN {} TO {};\n", rename.to, rename.from),
            );
        }
        let added: Vec<&Column> = table
            .columns
            .iter()
            .filter(|column| {
                !has(&existing.columns, &column.name)
                    && !confirmed.iter().any(|rename| rename.to == column.name)
            })
            .collect();
        let removed: Vec<&Column> = existing
            .columns
            .iter()
            .filter(|column| {
                !has(&table.columns, &column.name)
                    && !confirmed.iter().any(|rename| rename.from == column.name)
            })
            .collect();
        for gone in &removed {
            if let Some(came) = added.iter().find(|came| came.kind.eq_ignore_ascii_case(&gone.kind))
            {
                possible.push(PossibleRename {
                    table: name.clone(),
                    from: gone.name.clone(),
                    to: came.name.clone(),
                });
            }
        }
        for column in &added {
            let _ = writeln!(up, "ALTER TABLE {name} ADD COLUMN {};", column.definition());
            down.insert_str(0, &format!("ALTER TABLE {name} DROP COLUMN {};\n", column.name));
        }
        for column in &removed {
            let _ = writeln!(up, "ALTER TABLE {name} DROP COLUMN {};", column.name);
            down.insert_str(
                0,
                &format!("ALTER TABLE {name} ADD COLUMN {};\n", column.definition()),
            );
        }
    }
    for (name, table) in &current.tables {
        if !desired.tables.contains_key(name) {
            let _ = writeln!(up, "DROP TABLE {name};");
            down.insert_str(0, &create(name, table));
        }
    }
    Plan { up, down, possible_renames: possible }
}

/// Folds `migrations` into one, named `name`, that creates their result.
///
/// # Errors
///
/// A migration fails.
pub fn squash(migrations: &Migrations, name: &str) -> Result<Migration, DbError> {
    let schema = Schema::from_migrations(migrations)?;
    let plan = diff(&Schema::default(), &schema, &[]);
    Ok(Migration { name: name.to_owned(), up: plan.up, down: plan.down })
}
