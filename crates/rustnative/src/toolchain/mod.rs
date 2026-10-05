//! The native toolchains `rustnative` drives.
//!
//! | Platform | Toolchain | Status |
//! |---|---|---|
//! | Windows | Cargo + MSVC build tools + Windows SDK | driven here |
//! | macOS, iOS | Xcode and the Apple SDKs | Milestones 33, 36 |
//! | Linux | the system compiler, GTK 4, and libsoup 3, named per distribution | Milestone 34 |
//! | Android | Gradle, the Android SDK and NDK | Milestone 35 |
//! | Embedded | per-target toolchains through Cargo | Milestone 37 |
//!
//! `rustnative` never reimplements a toolchain: it finds one, runs it, and reports
//! honestly when it is not there.

pub mod cargo;
pub mod linux_distro;
pub mod windows_sdk;

/// The minimum Rust version this workspace supports, which `rustnative doctor`
/// checks the installed toolchain against.
pub const MINIMUM_RUST: (u64, u64) = (1, 85);

/// The `rustc` on the path, and whether it is new enough.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustToolchain {
    /// The version `rustc --version` reported, if it ran at all.
    pub version: Option<String>,
    /// Whether that version is at least [`MINIMUM_RUST`].
    pub meets_minimum: bool,
}

impl RustToolchain {
    /// Asks `rustc` what it is.
    #[must_use]
    pub fn detect() -> Self {
        let Some(output) = std::process::Command::new("rustc").arg("--version").output().ok()
        else {
            return Self { version: None, meets_minimum: false };
        };
        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let version = text.split_whitespace().nth(1).unwrap_or_default().to_owned();
        let numbers = version
            .split(['.', '-'])
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>();
        let meets_minimum = match numbers.as_slice() {
            [major, minor, ..] => (*major, *minor) >= MINIMUM_RUST,
            _ => false,
        };
        Self { version: (!text.is_empty()).then_some(text), meets_minimum }
    }
}

/// Whether `tool` can be run at all (`git --version`, say).
#[must_use]
pub fn is_available(tool: &str) -> bool {
    std::process::Command::new(tool)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rust_that_is_building_this_test_meets_the_minimum() {
        let rust = RustToolchain::detect();
        assert!(rust.version.is_some(), "rustc is obviously installed: it built this");
        assert!(rust.meets_minimum, "{:?} is below {MINIMUM_RUST:?}", rust.version);
    }

    #[test]
    fn a_tool_that_does_not_exist_is_reported_missing() {
        assert!(!is_available("rustnative-no-such-tool-exists"));
    }
}
