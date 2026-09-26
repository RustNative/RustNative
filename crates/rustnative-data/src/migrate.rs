//! Schema migration for persisted state (`PLAN.md` Milestone 47): so
//! Milestone 30's persisted values survive an application update that
//! changes their shape.
//!
//! Each [`Migrations::step`] moves a value from one version to the next
//! (`up`) and back (`down`). A [`VersionedStore`] wraps the application's
//! [`StateStore`] and applies them transparently: it writes every value in
//! an envelope carrying its version, and brings every value it reads up to
//! the latest version before handing it to the component that persisted
//! it. [`Migrations::dry_run`] reports what a migration would do to a
//! store without writing anything.
//!
//! ```
//! use rustnative_data::Migrations;
//! use serde_json::json;
//!
//! let migrations = Migrations::new()
//!     // Version 2 split `name` into `first` and `last`.
//!     .step(
//!         2,
//!         |mut v| {
//!             let name = v["name"].as_str().unwrap_or_default().to_owned();
//!             let (first, last) = name.split_once(' ').unwrap_or((&name, ""));
//!             v = json!({ "first": first, "last": last });
//!             Ok(v)
//!         },
//!         |v| Ok(json!({ "name": format!("{} {}", v["first"].as_str().unwrap_or(""), v["last"].as_str().unwrap_or("")) })),
//!     );
//!
//! let old = json!({ "name": "Ada Lovelace" });
//! let new = migrations.migrate(old.clone(), 1, 2).unwrap();
//! assert_eq!(new, json!({ "first": "Ada", "last": "Lovelace" }));
//! assert_eq!(migrations.migrate(new, 2, 1).unwrap(), old);
//! ```

use std::fmt;
use std::sync::Arc;

use rustnative_core::{ServiceError, StateStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;

type Transform = fn(Value) -> Result<Value, String>;

#[derive(Clone, Copy)]
struct Step {
    version: u32,
    up: Transform,
    down: Transform,
}

/// An ordered list of migrations; see the [module documentation](self).
#[derive(Clone, Default)]
pub struct Migrations {
    steps: Vec<Step>,
}

impl fmt::Debug for Migrations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Migrations").field("latest", &self.latest()).finish()
    }
}

/// Why a migration failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationError {
    /// The version the failing step moves to or from.
    pub version: u32,
    /// What went wrong.
    pub message: String,
}

impl fmt::Display for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "migration to or from version {} failed: {}", self.version, self.message)
    }
}

impl std::error::Error for MigrationError {}

impl Migrations {
    /// No migrations: every value is version 1.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the step to `version` from the one before it. Steps are added in
    /// order, starting at 2.
    ///
    /// # Panics
    ///
    /// If `version` is not one more than the latest step's.
    #[must_use]
    pub fn step(mut self, version: u32, up: Transform, down: Transform) -> Self {
        assert_eq!(version, self.latest() + 1, "migration steps are added in order");
        self.steps.push(Step { version, up, down });
        self
    }

    /// The latest version.
    #[must_use]
    pub fn latest(&self) -> u32 {
        self.steps.last().map_or(1, |step| step.version)
    }

    /// Moves `value` from version `from` to version `to`, up or down.
    ///
    /// # Errors
    ///
    /// A step failed, or a version is unknown.
    pub fn migrate(&self, mut value: Value, from: u32, to: u32) -> Result<Value, MigrationError> {
        let latest = self.latest();
        for version in [from, to] {
            if version == 0 || version > latest {
                return Err(MigrationError { version, message: "unknown version".into() });
            }
        }
        if from < to {
            for step in self.steps.iter().filter(|step| step.version > from && step.version <= to) {
                value = (step.up)(value)
                    .map_err(|message| MigrationError { version: step.version, message })?;
            }
        } else {
            for step in
                self.steps.iter().rev().filter(|step| step.version <= from && step.version > to)
            {
                value = (step.down)(value)
                    .map_err(|message| MigrationError { version: step.version, message })?;
            }
        }
        Ok(value)
    }

    /// What migrating each of `keys` in `store` to `to` would do, without
    /// writing anything.
    #[must_use]
    pub fn dry_run(&self, store: &dyn StateStore, keys: &[&str], to: u32) -> Vec<Planned> {
        keys.iter()
            .filter_map(|key| {
                let bytes = store.load(key).ok().flatten()?;
                let (from, value) = open(&bytes);
                let outcome = self.migrate(value, from, to);
                Some(Planned { key: (*key).to_owned(), from, to, outcome })
            })
            .collect()
    }
}

/// What migrating one stored value would do.
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    /// Its key.
    pub key: String,
    /// Its version now.
    pub from: u32,
    /// The version it would move to.
    pub to: u32,
    /// The value it would become, or why it cannot.
    pub outcome: Result<Value, MigrationError>,
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    #[serde(rename = "$version")]
    version: u32,
    value: Value,
}

/// A stored value's version and value. A value written before versioning
/// (no envelope) is version 1.
fn open(bytes: &[u8]) -> (u32, Value) {
    if let Ok(envelope) = serde_json::from_slice::<Envelope>(bytes) {
        return (envelope.version, envelope.value);
    }
    (1, serde_json::from_slice(bytes).unwrap_or(Value::Null))
}

/// A [`StateStore`] that versions what it stores and migrates what it
/// loads; see the [module documentation](self).
pub struct VersionedStore {
    inner: Arc<dyn StateStore>,
    migrations: Migrations,
}

impl fmt::Debug for VersionedStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VersionedStore")
            .field("migrations", &self.migrations)
            .finish_non_exhaustive()
    }
}

impl VersionedStore {
    /// Wraps `inner`, migrating by `migrations`.
    #[must_use]
    pub fn new(inner: Arc<dyn StateStore>, migrations: Migrations) -> Self {
        Self { inner, migrations }
    }
}

impl StateStore for VersionedStore {
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, ServiceError> {
        let Some(bytes) = self.inner.load(key)? else { return Ok(None) };
        let (version, value) = open(&bytes);
        let latest = self.migrations.latest();
        if version > latest {
            // Written by a newer version of the application: not ours to
            // read. Starting from the default is safer than guessing.
            return Ok(None);
        }
        let value = self
            .migrations
            .migrate(value, version, latest)
            .map_err(|error| ServiceError::new(error.to_string()))?;
        serde_json::to_vec(&value).map(Some).map_err(|error| ServiceError::new(error.to_string()))
    }

    fn save(&self, key: &str, value: &[u8]) -> Result<(), ServiceError> {
        let value = serde_json::from_slice(value).unwrap_or(Value::Null);
        let envelope = Envelope { version: self.migrations.latest(), value };
        let bytes =
            serde_json::to_vec(&envelope).map_err(|error| ServiceError::new(error.to_string()))?;
        self.inner.save(key, &bytes)
    }

    fn remove(&self, key: &str) -> Result<(), ServiceError> {
        self.inner.remove(key)
    }
}
