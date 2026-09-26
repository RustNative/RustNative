//! `rustnative.toml`: what a project says about itself.
//!
//! One file, one table, and every field either required or with an obvious
//! default. What is here is what the tooling needs and nothing else: the
//! application's identity (used for its state store, its single-instance
//! mutex, and — in Milestone 32 — its package), how it should be named to
//! a person, and which URL schemes it wants.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The file a `rustnative` project is described by.
pub const FILE_NAME: &str = "rustnative.toml";

/// A problem with `rustnative.toml`, naming the field it is about.
#[derive(Debug)]
pub enum ConfigError {
    /// The file could not be read.
    Unreadable {
        /// Where it was looked for.
        path: PathBuf,
        /// Why reading failed.
        cause: std::io::Error,
    },
    /// The file is not valid TOML, or a field has the wrong type.
    Malformed {
        /// Where the file is.
        path: PathBuf,
        /// What the TOML parser said.
        cause: toml::de::Error,
    },
    /// A field's value cannot be used.
    Invalid {
        /// The field, as it is written in the file (`app.id`).
        field: &'static str,
        /// What is wrong with it.
        problem: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path, cause } => {
                write!(f, "could not read {}: {cause}", path.display())
            }
            Self::Malformed { path, cause } => write!(f, "{}: {cause}", path.display()),
            Self::Invalid { field, problem } => write!(f, "{field}: {problem}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// A project's `rustnative.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// What the application is.
    pub app: App,
    /// Development resources (`[resources.<name>]`), provisioned by
    /// `rustnative dev` when absent and handed to the application as
    /// `RUSTNATIVE_RESOURCE_<NAME>` (`rustnative_core::dev::resource`).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub resources: std::collections::BTreeMap<String, Resource>,
    /// Localization (`[i18n]`, Milestone 46).
    #[serde(default, skip_serializing_if = "I18n::is_default")]
    pub i18n: I18n,
    /// The style file (`[style] file = "…"`, Milestone 58), which the build
    /// compiles into the application's theme.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,
    /// What the native package declares (`[package]`, Milestone 50).
    #[serde(default, skip_serializing_if = "Package::is_empty")]
    pub package: Package,
    /// The key desktop updates are signed with (`[update]`, Milestone 50).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<Update>,
}

/// The `[package]` table: what goes into the generated native project
/// files, which are build outputs and never edited by hand (`C63`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Package {
    /// MSIX capabilities (`internetClient`, `webcam`, `microphone`).
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl Package {
    fn is_empty(&self) -> bool {
        self.capabilities.is_empty()
    }
}

/// The `[update]` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Update {
    /// The publisher's Ed25519 public key, hex (`rustnative update keygen`
    /// prints it); installed copies trust only updates it signed.
    pub public_key: String,
}

/// The `[style]` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Style {
    /// The style file, relative to the project.
    pub file: String,
}

/// The `[i18n]` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct I18n {
    /// The locale messages are written in first (`locales/<source>.ftl`).
    #[serde(default = "I18n::default_source")]
    pub source: String,
    /// Literal texts `rustnative i18n lint` accepts untranslated (a brand
    /// name, a symbol).
    #[serde(default)]
    pub allow: Vec<String>,
}

impl I18n {
    fn default_source() -> String {
        "en".to_owned()
    }

    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

impl Default for I18n {
    fn default() -> Self {
        Self { source: Self::default_source(), allow: Vec::new() }
    }
}

/// One development resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Resource {
    /// What it is.
    pub kind: ResourceKind,
    /// A file or folder, relative to the project, it starts as a copy of.
    #[serde(default)]
    pub seed: Option<PathBuf>,
}

/// What a development resource is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResourceKind {
    /// A folder (a local stand-in for a bucket, a data directory).
    Directory,
    /// A file (a local database file, a settings file).
    File,
}

/// The `[app]` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct App {
    /// The Cargo package name, and the executable's name.
    pub name: String,
    /// The application's stable identity: its state store's folder, its
    /// single-instance mutex, and its package identity.
    pub id: String,
    /// The name shown to people, which may contain anything.
    pub display_name: String,
    /// The version, as `major.minor.patch`.
    pub version: String,
    /// Who publishes it — an X.500 name for signing (Milestone 32).
    #[serde(default)]
    pub publisher: Option<String>,
    /// One sentence about the application.
    #[serde(default)]
    pub description: Option<String>,
    /// An icon, relative to the project root.
    #[serde(default)]
    pub icon: Option<PathBuf>,
    /// URL schemes the application handles (`myapp`, for `myapp://…`).
    #[serde(default)]
    pub url_schemes: Vec<String>,
}

impl Config {
    /// Reads and validates the `rustnative.toml` in `directory`.
    ///
    /// # Errors
    ///
    /// [`ConfigError`] naming the file or the field that is wrong.
    pub fn load(directory: &Path) -> Result<Self, ConfigError> {
        let path = directory.join(FILE_NAME);
        let text = std::fs::read_to_string(&path)
            .map_err(|cause| ConfigError::Unreadable { path: path.clone(), cause })?;
        let config: Self =
            toml::from_str(&text).map_err(|cause| ConfigError::Malformed { path, cause })?;
        config.validate()?;
        Ok(config)
    }

    /// Checks every field's value.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Invalid`] for the first field that cannot be used.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let app = &self.app;
        invalid_if(
            app.name.is_empty()
                || !app.name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')),
            "app.name",
            "must be a Cargo package name: ASCII letters, digits, `-`, and `_`",
        )?;
        invalid_if(
            app.id.is_empty()
                || !app.id.contains('.')
                || !app
                    .id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
                || app.id.split('.').any(str::is_empty),
            "app.id",
            "must be a dotted identifier like `com.example.app`",
        )?;
        invalid_if(app.display_name.trim().is_empty(), "app.display-name", "must not be empty")?;
        let parts = app.version.split('.').collect::<Vec<_>>();
        invalid_if(
            parts.len() != 3
                || parts
                    .iter()
                    .any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit())),
            "app.version",
            "must be `major.minor.patch`, all numbers",
        )?;
        for scheme in &app.url_schemes {
            invalid_if(
                scheme.is_empty()
                    || !scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                    || !scheme.chars().all(|c| {
                        c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '+' | '-' | '.')
                    }),
                "app.url-schemes",
                format!("`{scheme}` is not a URL scheme: lower-case, starting with a letter"),
            )?;
        }
        Ok(())
    }

    /// A starting `rustnative.toml` for a new project called `name`.
    #[must_use]
    pub fn template(name: &str) -> Self {
        Self {
            app: App {
                name: name.to_owned(),
                id: format!("com.example.{}", name.replace(['-', '_'], "")),
                display_name: name.to_owned(),
                version: "0.1.0".to_owned(),
                publisher: Some("CN=Example".to_owned()),
                description: Some(format!("{name}, a Rust Native application")),
                icon: None,
                url_schemes: Vec::new(),
            },
            resources: std::collections::BTreeMap::new(),
            i18n: I18n::default(),
            style: None,
            package: Package::default(),
            update: None,
        }
    }
}

fn invalid_if(
    wrong: bool,
    field: &'static str,
    problem: impl Into<String>,
) -> Result<(), ConfigError> {
    if wrong { Err(ConfigError::Invalid { field, problem: problem.into() }) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(body: &str) -> Result<Config, ConfigError> {
        let parsed: Config = toml::from_str(body)
            .map_err(|cause| ConfigError::Malformed { path: PathBuf::from(FILE_NAME), cause })?;
        parsed.validate().map(|()| parsed)
    }

    const GOOD: &str = r#"
[app]
name = "demo"
id = "com.example.demo"
display-name = "Demo"
version = "1.2.3"
url-schemes = ["demo"]
"#;

    #[test]
    fn a_minimal_file_parses_and_validates() {
        let config = config(GOOD).expect("valid");
        assert_eq!(config.app.name, "demo");
        assert_eq!(config.app.url_schemes, vec!["demo".to_owned()]);
        assert_eq!(config.app.publisher, None, "optional fields stay unset");
    }

    #[test]
    fn every_invalid_field_is_named() {
        let cases = [
            ("name = \"demo\"", "name = \"not a name\"", "app.name"),
            ("id = \"com.example.demo\"", "id = \"nodots\"", "app.id"),
            ("display-name = \"Demo\"", "display-name = \"  \"", "app.display-name"),
            ("version = \"1.2.3\"", "version = \"1.2\"", "app.version"),
            ("url-schemes = [\"demo\"]", "url-schemes = [\"Demo!\"]", "app.url-schemes"),
        ];
        for (from, to, field) in cases {
            let broken = GOOD.replace(from, to);
            match config(&broken) {
                Err(ConfigError::Invalid { field: named, .. }) => assert_eq!(named, field),
                other => panic!("{to} should have been refused as {field}, got {other:?}"),
            }
        }
    }

    #[test]
    fn an_unknown_field_is_refused_rather_than_ignored() {
        let extra = format!("{GOOD}oops = true\n");
        assert!(matches!(config(&extra), Err(ConfigError::Malformed { .. })));
    }

    #[test]
    fn a_template_validates_and_round_trips() {
        let template = Config::template("my-app");
        template.validate().expect("the template must be valid");
        let text = toml::to_string_pretty(&template).expect("serializable");
        assert_eq!(config(&text).expect("re-parses"), template);
    }
}
