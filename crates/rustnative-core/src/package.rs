//! Third-party capability packages (`PLAN.md` Milestone 52, `C71`): a
//! community crate that implements a portable contract with per-backend
//! code, installed into an application without forking the framework.
//!
//! A package states what it is in a [`PackageManifest`]: the backends it
//! supports, the grants it needs, and the framework versions it works
//! with. Installing it ([`crate::Services::install`]) checks all three,
//! then lets the package build its service from a scope holding only the
//! grants it declared (Milestone 51's enforcement), and keeps what it
//! built under the package's name — never in a global registry.
//!
//! ```
//! use std::sync::Arc;
//!
//! use rustnative_core::grant::GrantSet;
//! use rustnative_core::grant::ScopedServices;
//! use rustnative_core::package::{CapabilityPackage, PackageManifest};
//! use rustnative_core::Services;
//!
//! struct Greeter;
//! struct Greeting(&'static str);
//!
//! impl CapabilityPackage for Greeter {
//!     type Service = Greeting;
//!     fn manifest(&self) -> PackageManifest {
//!         PackageManifest::new("greeter", "1.0.0", &["windows", "headless"], ">=0.1, <0.2")
//!     }
//!     fn build(&self, _scope: ScopedServices, _backend: &str) -> Option<Greeting> {
//!         Some(Greeting("hello"))
//!     }
//! }
//!
//! let services = Services::default().install(&Greeter, "headless").unwrap();
//! assert_eq!(services.package::<Greeting>("greeter").unwrap().0, "hello");
//! assert!(Services::default().install(&Greeter, "android").is_err(), "not a backend it supports");
//! ```

use serde::{Deserialize, Serialize};

use crate::grant::{GrantSet, ScopedServices};

/// The framework version packages are checked against.
pub const FRAMEWORK_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What a package declares about itself — also what `rustnative add`
/// reads from `[package.metadata.rustnative]` and what an index lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageManifest {
    /// Its name (the crate's).
    pub name: String,
    /// Its version.
    pub version: String,
    /// The backends it has code for (`windows`, `headless`, `android`, …).
    pub backends: Vec<String>,
    /// The framework versions it works with (`>=0.1, <0.2`).
    pub framework: String,
    /// The grants it needs, as `GrantSet` holds them — declared, so an
    /// application sees what it hands over.
    #[serde(skip)]
    pub grants: GrantSet,
}

impl PackageManifest {
    /// A manifest with no grants.
    #[must_use]
    pub fn new(name: &str, version: &str, backends: &[&str], framework: &str) -> Self {
        Self {
            name: name.to_owned(),
            version: version.to_owned(),
            backends: backends.iter().map(|backend| (*backend).to_owned()).collect(),
            framework: framework.to_owned(),
            grants: GrantSet::none(),
        }
    }

    /// Adds a grant it needs.
    #[must_use]
    pub fn needs(mut self, grant: crate::grant::Grant) -> Self {
        self.grants = self.grants.with(grant);
        self
    }

    /// Why this package cannot be used on `backend` with this framework,
    /// if it cannot.
    ///
    /// # Errors
    ///
    /// The backend is not one it supports, or the framework version is
    /// outside its range.
    pub fn check(&self, backend: &str) -> Result<(), PackageError> {
        if !self.backends.iter().any(|supported| supported == backend) {
            return Err(PackageError::Backend {
                package: self.name.clone(),
                backend: backend.to_owned(),
            });
        }
        if !version_matches(&self.framework, FRAMEWORK_VERSION) {
            return Err(PackageError::Framework {
                package: self.name.clone(),
                wants: self.framework.clone(),
                have: FRAMEWORK_VERSION.to_owned(),
            });
        }
        Ok(())
    }
}

/// Why a package was not installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageError {
    /// It has no code for this backend.
    Backend {
        /// The package.
        package: String,
        /// The backend.
        backend: String,
    },
    /// It does not work with this framework version.
    Framework {
        /// The package.
        package: String,
        /// The range it declares.
        wants: String,
        /// The framework's version.
        have: String,
    },
    /// It declined to build its service here (a device it needs is absent).
    Declined {
        /// The package.
        package: String,
    },
}

impl std::fmt::Display for PackageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Backend { package, backend } => {
                write!(formatter, "{package} has no code for the {backend} backend")
            }
            Self::Framework { package, wants, have } => {
                write!(formatter, "{package} works with framework {wants}; this is {have}")
            }
            Self::Declined { package } => {
                write!(formatter, "{package} is not available on this machine")
            }
        }
    }
}

impl std::error::Error for PackageError {}

/// A capability package.
pub trait CapabilityPackage {
    /// The service it provides.
    type Service: Send + Sync + 'static;

    /// What it declares about itself.
    fn manifest(&self) -> PackageManifest;

    /// Builds its service for `backend` from a scope holding only its
    /// declared grants; `None` when it cannot here.
    fn build(&self, scope: ScopedServices, backend: &str) -> Option<Self::Service>;
}

/// Whether `version` satisfies `requirement`: comma-separated comparisons
/// (`>=0.1`, `<0.2`, `=0.1.3`), or a bare version meaning the same
/// compatible series (`0.1` accepts `0.1.x`; `1.2` accepts `1.x` from
/// `1.2`), as Cargo reads it.
#[must_use]
pub fn version_matches(requirement: &str, version: &str) -> bool {
    let parse = |text: &str| -> Vec<u64> {
        text.trim().split('.').map(|part| part.trim().parse().unwrap_or(0)).collect()
    };
    let pad = |mut parts: Vec<u64>| {
        parts.resize(3, 0);
        parts
    };
    let have = pad(parse(version.split(['-', '+']).next().unwrap_or(version)));
    requirement.split(',').map(str::trim).filter(|part| !part.is_empty()).all(|part| {
        let (operator, rest) = [">=", "<=", ">", "<", "=", "^"]
            .iter()
            .find_map(|operator| part.strip_prefix(operator).map(|rest| (*operator, rest)))
            .unwrap_or(("^", part));
        let given = parse(rest);
        let wanted = pad(given.clone());
        match operator {
            ">=" => have >= wanted,
            "<=" => have <= wanted,
            ">" => have > wanted,
            "<" => have < wanted,
            "=" => have == wanted,
            _ => {
                // Caret: the leftmost non-zero part may not change.
                let significant = given
                    .iter()
                    .position(|part| *part != 0)
                    .unwrap_or(given.len().saturating_sub(1));
                have >= wanted && have[..=significant.min(2)] == wanted[..=significant.min(2)]
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::version_matches;

    #[test]
    fn requirements_read_as_cargo_reads_them() {
        assert!(version_matches(">=0.1, <0.2", "0.1.7"));
        assert!(!version_matches(">=0.1, <0.2", "0.2.0"));
        assert!(version_matches("0.1", "0.1.9") && !version_matches("0.1", "0.2.0"));
        assert!(
            version_matches("1.2", "1.9.0")
                && !version_matches("1.2", "1.1.0")
                && !version_matches("1.2", "2.0.0")
        );
        assert!(version_matches("=0.1.3", "0.1.3") && !version_matches("=0.1.3", "0.1.4"));
        assert!(version_matches("0.0.3", "0.0.3") && !version_matches("0.0.3", "0.0.4"));
    }
}
