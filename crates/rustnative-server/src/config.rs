//! Layered typed configuration: defaults, then a file, then the
//! environment, then a secrets directory — each layer overriding the one
//! before — validated into the application's own type at startup. There is
//! no global: the result is a value the application passes on, and a
//! [`Secret`] never prints.
//!
//! ```
//! use rustnative_server::config::{Config, Secret};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, Serialize, Deserialize)]
//! struct Settings {
//!     port: u16,
//!     database: Database,
//! }
//! #[derive(Debug, Serialize, Deserialize)]
//! struct Database {
//!     url: String,
//!     password: Secret<String>,
//! }
//!
//! let settings: Settings = Config::new()
//!     .defaults(&serde_json::json!({ "port": 8080, "database": { "url": "app.db", "password": "" } }))
//!     .toml("port = 9000")
//!     .unwrap()
//!     .pairs([("APP_DATABASE__PASSWORD", "hunter2")], "APP_")
//!     .build()
//!     .unwrap();
//! assert_eq!(settings.port, 9000);
//! assert_eq!(settings.database.password.expose(), "hunter2");
//! assert!(!format!("{settings:?}").contains("hunter2"), "a secret never prints");
//! ```

use std::fmt;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// A value that must not be logged: `Debug` and `Display` print
/// `[redacted]`, and reading it is an explicit [`Secret::expose`].
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Secret<T>(T);

impl<T> Secret<T> {
    /// Wraps `value`.
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    /// The value, for the one place that needs it.
    pub const fn expose(&self) -> &T {
        &self.0
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[redacted]")
    }
}

impl<T> fmt::Display for Secret<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[redacted]")
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Secret<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Self)
    }
}

/// Serializes as `"[redacted]"`, so a configuration dump cannot leak it.
impl<T> Serialize for Secret<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("[redacted]")
    }
}

/// Why configuration could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "configuration: {}", self.0)
    }
}

impl std::error::Error for ConfigError {}

/// A configuration being layered.
#[derive(Debug, Clone, Default)]
pub struct Config {
    value: Map<String, Value>,
    sources: Vec<String>,
}

impl Config {
    /// No layers yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The defaults (typically the settings type's own default).
    #[must_use]
    pub fn defaults(mut self, defaults: &impl Serialize) -> Self {
        if let Ok(Value::Object(object)) = serde_json::to_value(defaults) {
            merge(&mut self.value, object);
            self.sources.push("defaults".into());
        }
        self
    }

    /// A TOML document.
    ///
    /// # Errors
    ///
    /// It is not valid TOML.
    pub fn toml(mut self, text: &str) -> Result<Self, ConfigError> {
        let parsed: Value = toml::from_str(text).map_err(|error| ConfigError(error.to_string()))?;
        if let Value::Object(object) = parsed {
            merge(&mut self.value, object);
        }
        self.sources.push("toml".into());
        Ok(self)
    }

    /// A TOML file; a missing file is skipped (it is an optional layer).
    ///
    /// # Errors
    ///
    /// It exists but cannot be read or parsed.
    pub fn file(self, path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let mut config = self.toml(&text)?;
                config.sources.pop();
                config.sources.push(path.display().to_string());
                Ok(config)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(self),
            Err(error) => Err(ConfigError(format!("{}: {error}", path.display()))),
        }
    }

    /// The process environment's variables starting with `prefix`
    /// (`APP_DATABASE__URL` sets `database.url`).
    #[must_use]
    pub fn env(self, prefix: &str) -> Self {
        self.pairs(std::env::vars(), prefix)
    }

    /// `pairs` as environment variables (see [`Self::env`]).
    #[must_use]
    pub fn pairs<K: AsRef<str>, V: AsRef<str>>(
        mut self,
        pairs: impl IntoIterator<Item = (K, V)>,
        prefix: &str,
    ) -> Self {
        for (key, value) in pairs {
            let Some(name) = key.as_ref().strip_prefix(prefix) else { continue };
            let path = name.split("__").map(str::to_lowercase).collect::<Vec<_>>();
            set_path(&mut self.value, &path, parse_scalar(value.as_ref()));
        }
        self.sources.push(format!("environment {prefix}*"));
        self
    }

    /// A directory of secret files (one per value, as a container runtime
    /// mounts them): `database__password` sets `database.password` to the
    /// file's trimmed contents. A missing directory is skipped.
    ///
    /// # Errors
    ///
    /// A file in it cannot be read.
    pub fn secrets_dir(mut self, directory: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let directory = directory.as_ref();
        let Ok(entries) = std::fs::read_dir(directory) else { return Ok(self) };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(entry.path())
                .map_err(|error| ConfigError(format!("{name}: {error}")))?;
            let path = name.split("__").map(str::to_lowercase).collect::<Vec<_>>();
            set_path(&mut self.value, &path, Value::String(text.trim().to_owned()));
        }
        self.sources.push(format!("secrets {}", directory.display()));
        Ok(self)
    }

    /// The layers applied, in order, for the startup report.
    #[must_use]
    pub fn sources(&self) -> &[String] {
        &self.sources
    }

    /// The configuration as `T`.
    ///
    /// # Errors
    ///
    /// A value is missing or of the wrong type; the message names it.
    pub fn build<T: DeserializeOwned>(self) -> Result<T, ConfigError> {
        serde_json::from_value(Value::Object(self.value))
            .map_err(|error| ConfigError(error.to_string()))
    }
}

fn merge(into: &mut Map<String, Value>, from: Map<String, Value>) {
    for (key, value) in from {
        match (into.get_mut(&key), value) {
            (Some(Value::Object(existing)), Value::Object(incoming)) => merge(existing, incoming),
            (_, value) => {
                into.insert(key, value);
            }
        }
    }
}

fn set_path(into: &mut Map<String, Value>, path: &[String], value: Value) {
    let Some((last, parents)) = path.split_last() else { return };
    let mut at = into;
    for parent in parents {
        let entry = at.entry(parent.clone()).or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        let Value::Object(next) = entry else { return };
        at = next;
    }
    at.insert(last.clone(), value);
}

fn parse_scalar(text: &str) -> Value {
    serde_json::from_str::<Value>(text)
        .ok()
        .filter(|value| value.is_number() || value.is_boolean())
        .unwrap_or_else(|| Value::String(text.to_owned()))
}
