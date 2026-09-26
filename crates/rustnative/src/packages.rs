//! `rustnative add` and `rustnative search` (`PLAN.md` Milestone 52,
//! `C71`): capability packages, checked against the project before they
//! are added.

use std::path::{Path, PathBuf};

use rustnative_core::package::{FRAMEWORK_VERSION, version_matches};
use serde::Deserialize;

use crate::error::{Error, Result};
use crate::project::Project;

/// The index shipped with this version of the tool; `--index` names
/// another (a file an organization keeps, say).
const BUILT_IN_INDEX: &str = include_str!("../../../docs/packages/index.json");

/// The backends a project built with this tool has today.
const PROJECT_BACKENDS: [&str; 1] = ["windows"];

/// One package as an index lists it.
#[derive(Debug, Clone, Deserialize)]
pub struct IndexEntry {
    /// Its crate name.
    pub name: String,
    /// Its latest version.
    pub version: String,
    /// What it does.
    pub description: String,
    /// The contract it implements.
    pub contract: String,
    /// The backends it has code for.
    pub backends: Vec<String>,
    /// The framework versions it works with.
    pub framework: String,
    /// The grants it needs.
    #[serde(default)]
    pub grants: Vec<String>,
}

/// A package's `[package.metadata.rustnative]`.
#[derive(Debug, Clone, Deserialize)]
struct Metadata {
    contract: String,
    backends: Vec<String>,
    framework: String,
    #[serde(default)]
    grants: Vec<String>,
}

fn index(path: Option<&Path>) -> Result<Vec<IndexEntry>> {
    let text = match path {
        Some(path) => std::fs::read_to_string(path)
            .map_err(|cause| Error::Io { what: format!("read {}", path.display()), cause })?,
        None => BUILT_IN_INDEX.to_owned(),
    };
    serde_json::from_str(&text)
        .map_err(|error| Error::Usage(format!("the package index is malformed: {error}")))
}

/// Why a package cannot be used by this project, if it cannot.
fn incompatible(backends: &[String], framework: &str) -> Option<String> {
    let missing: Vec<&str> = PROJECT_BACKENDS
        .iter()
        .copied()
        .filter(|backend| !backends.iter().any(|supported| supported == backend))
        .collect();
    if !missing.is_empty() {
        return Some(format!("it has no code for the {} backend", missing.join(", ")));
    }
    (!version_matches(framework, FRAMEWORK_VERSION))
        .then(|| format!("it works with framework {framework}; this is {FRAMEWORK_VERSION}"))
}

/// `rustnative search <term>`.
///
/// # Errors
///
/// The index cannot be read.
pub fn search(term: &str, index_path: Option<&Path>) -> Result<()> {
    let term = term.to_lowercase();
    let found: Vec<IndexEntry> = index(index_path)?
        .into_iter()
        .filter(|entry| {
            [&entry.name, &entry.description, &entry.contract]
                .iter()
                .any(|field| field.to_lowercase().contains(&term))
        })
        .collect();
    if found.is_empty() {
        println!("search: no package matches {term:?}");
    }
    for entry in found {
        let note = incompatible(&entry.backends, &entry.framework)
            .map_or_else(String::new, |why| format!("  (not usable: {why})"));
        println!(
            "{} {} — {} [{}]{note}",
            entry.name,
            entry.version,
            entry.description,
            entry.backends.join(", ")
        );
    }
    Ok(())
}

/// `rustnative add <package>`: a path to the package's crate, or a name
/// from the index.
///
/// # Errors
///
/// The package is unknown or incompatible, or the project's `Cargo.toml`
/// cannot be updated.
pub fn add(here: &Path, package: &str, index_path: Option<&Path>) -> Result<()> {
    let project = Project::find(here)?;
    let as_path = PathBuf::from(package);
    let (name, dependency, backends, framework, grants, contract) =
        if as_path.join("Cargo.toml").is_file() {
            let path = std::fs::canonicalize(&as_path)
                .map_err(|cause| Error::Io { what: format!("find {package}"), cause })?;
            let text = std::fs::read_to_string(path.join("Cargo.toml")).map_err(|cause| {
                Error::Io { what: format!("read {}", path.join("Cargo.toml").display()), cause }
            })?;
            let cargo: toml::Value =
                toml::from_str(&text).map_err(|error| Error::Usage(error.to_string()))?;
            let name = cargo["package"]["name"].as_str().unwrap_or_default().to_owned();
            let metadata: Metadata = cargo
            .get("package")
            .and_then(|table| table.get("metadata"))
            .and_then(|table| table.get("rustnative"))
            .cloned()
            .ok_or_else(|| {
                Error::Usage(format!(
                    "{name} has no [package.metadata.rustnative]: it is not a capability package"
                ))
            })?
            .try_into()
            .map_err(|error: toml::de::Error| {
                Error::Usage(format!("{name}'s [package.metadata.rustnative]: {error}"))
            })?;
            let dependency =
                format!("{{ path = {:?} }}", path.display().to_string().replace('\\', "/"));
            (
                name,
                dependency,
                metadata.backends,
                metadata.framework,
                metadata.grants,
                metadata.contract,
            )
        } else {
            let entry = index(index_path)?
                .into_iter()
                .find(|entry| entry.name == package)
                .ok_or_else(|| {
                    Error::Usage(format!(
                        "no package {package} in the index (`rustnative search` lists them)"
                    ))
                })?;
            let dependency = format!("{:?}", entry.version);
            (entry.name, dependency, entry.backends, entry.framework, entry.grants, entry.contract)
        };
    if let Some(why) = incompatible(&backends, &framework) {
        return Err(Error::Usage(format!("{name} cannot be added: {why}")));
    }

    let manifest = project.root.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .map_err(|cause| Error::Io { what: format!("read {}", manifest.display()), cause })?;
    if text.lines().any(|line| {
        line.trim_start().starts_with(&format!("{name} "))
            || line.trim_start().starts_with(&format!("{name}="))
    }) {
        println!("add: {name} is already a dependency");
        return Ok(());
    }
    let line = format!("{name} = {dependency}\n");
    let updated = match text.find("[dependencies]\n") {
        Some(at) => {
            let insert = at + "[dependencies]\n".len();
            format!("{}{line}{}", &text[..insert], &text[insert..])
        }
        None => format!("{}\n[dependencies]\n{line}", text.trim_end()),
    };
    std::fs::write(&manifest, updated)
        .map_err(|cause| Error::Io { what: format!("write {}", manifest.display()), cause })?;
    println!("add: {name} ({contract}) added; install it with `Services::install(&…, backend)`");
    if !grants.is_empty() {
        println!("add: it asks for these grants: {}", grants.join(", "));
    }
    Ok(())
}
