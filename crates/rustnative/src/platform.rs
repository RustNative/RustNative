//! The platforms `rustnative` knows about, and which of them can be built today.
//!
//! Every platform on the roadmap is *recognized*, and the ones without a
//! backend fail with the milestone that will bring them rather than with
//! "unknown platform" — or, worse, by quietly building for Windows.

use std::fmt;

use clap::ValueEnum;

/// A target platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum Platform {
    /// Win32 desktop — the backend this workspace has.
    Windows,
    /// macOS (Milestone 33).
    Macos,
    /// Linux (Milestone 34).
    Linux,
    /// Android (Milestone 35).
    Android,
    /// iOS (Milestone 36).
    Ios,
    /// Embedded targets (Milestone 37).
    Embedded,
    /// The browser: Rust on the server, HTML/CSS and compile-time generated
    /// JavaScript in the browser, with WebAssembly for subtrees that opt in
    /// (see PLAN.md's web roadmap).
    Web,
}

impl Platform {
    /// The backend crate that realizes this platform, if one exists.
    #[must_use]
    pub const fn backend(self) -> Option<&'static str> {
        match self {
            Self::Windows => Some("rustnative-windows"),
            _ => None,
        }
    }

    /// The milestone that brings this platform's backend, for the ones that
    /// do not have one yet.
    #[must_use]
    pub const fn planned_milestone(self) -> Option<u32> {
        match self {
            // The web target has a roadmap section rather than a numbered
            // milestone, so it names none either.
            Self::Windows | Self::Web => None,
            Self::Macos => Some(33),
            Self::Linux => Some(34),
            Self::Android => Some(35),
            Self::Ios => Some(36),
            Self::Embedded => Some(37),
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Windows => "windows",
            Self::Macos => "macos",
            Self::Linux => "linux",
            Self::Android => "android",
            Self::Ios => "ios",
            Self::Embedded => "embedded",
            Self::Web => "web",
        };
        f.write_str(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_windows_has_a_backend_and_every_other_names_its_milestone() {
        assert_eq!(Platform::Windows.backend(), Some("rustnative-windows"));
        for platform in
            [Platform::Macos, Platform::Linux, Platform::Android, Platform::Ios, Platform::Embedded]
        {
            assert!(platform.backend().is_none(), "{platform} has no backend yet");
            assert!(platform.planned_milestone().is_some(), "{platform} names its milestone");
        }
    }
}
