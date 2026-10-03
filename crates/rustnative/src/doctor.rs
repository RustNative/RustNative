//! `rustnative doctor`: what this machine can build, and what it is missing.
//!
//! Every check reports what it actually found — a version, a path — rather
//! than a tick, so a failing build can be compared against what `doctor`
//! said. `--json` prints the same findings for a script to read.

use serde::Serialize;

use crate::platform::Platform;
use crate::toolchain::{self, RustToolchain, windows_sdk::WindowsToolchain};

/// One thing that was checked.
#[derive(Debug, Clone, Serialize)]
pub struct Check {
    /// What was checked ("rustc", "makeappx").
    pub name: String,
    /// Whether it is usable.
    pub ok: bool,
    /// What was found: a version, a path, or why not.
    pub detail: String,
}

/// Whether one platform can be built here.
#[derive(Debug, Clone, Serialize)]
pub struct PlatformReadiness {
    /// The platform.
    pub platform: String,
    /// Whether `rustnative build <platform>` would work on this machine.
    pub ready: bool,
    /// Why, or why not.
    pub detail: String,
}

/// Everything `doctor` found.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// The toolchain checks.
    pub checks: Vec<Check>,
    /// Per-platform readiness.
    pub platforms: Vec<PlatformReadiness>,
}

impl Report {
    /// Runs every check.
    #[must_use]
    pub fn gather() -> Self {
        let rust = RustToolchain::detect();
        let windows = WindowsToolchain::detect();
        let mut checks = vec![
            Check {
                name: "rustc".to_owned(),
                ok: rust.meets_minimum,
                detail: match (&rust.version, rust.meets_minimum) {
                    (Some(version), true) => version.clone(),
                    (Some(version), false) => {
                        let (major, minor) = toolchain::MINIMUM_RUST;
                        format!("{version} is older than the required {major}.{minor}")
                    }
                    (None, _) => "not found — install Rust from https://rustup.rs".to_owned(),
                },
            },
            Check {
                name: "cargo".to_owned(),
                ok: toolchain::is_available("cargo"),
                detail: "the build `rustnative` drives".to_owned(),
            },
            Check {
                name: "git".to_owned(),
                ok: toolchain::is_available("git"),
                detail: "optional: only needed to publish or fetch git dependencies".to_owned(),
            },
            Check {
                name: "visual studio (c++ build tools)".to_owned(),
                ok: windows.visual_studio.is_some(),
                detail: windows.visual_studio.as_ref().map_or_else(
                    || {
                        "not found — install the \"Desktop development with C++\" workload"
                            .to_owned()
                    },
                    |path| path.display().to_string(),
                ),
            },
            Check {
                name: "windows sdk".to_owned(),
                ok: windows.sdk_version.is_some(),
                detail: windows
                    .sdk_version
                    .clone()
                    .unwrap_or_else(|| "not found — install the Windows 10/11 SDK".to_owned()),
            },
        ];
        checks.extend(windows.tools.iter().map(|tool| Check {
            name: tool.name.to_owned(),
            ok: tool.found(),
            detail: tool.path.as_ref().map_or_else(
                || "not found in the Windows SDK".to_owned(),
                |path| path.display().to_string(),
            ),
        }));

        let platforms = [
            Platform::Windows,
            Platform::Macos,
            Platform::Linux,
            Platform::Android,
            Platform::Ios,
            Platform::Embedded,
            Platform::Web,
        ]
        .into_iter()
        .map(|platform| {
            let (ready, detail) = match platform.planned_milestone() {
                _ if platform == Platform::Windows => {
                    if windows.can_build() {
                        let packaging = if windows.can_package() {
                            "packaging tools present"
                        } else {
                            "packaging tools missing (makeappx/signtool)"
                        };
                        (true, format!("ready — {packaging}"))
                    } else {
                        (false, "missing the MSVC build tools or the Windows SDK".to_owned())
                    }
                }
                _ if platform == Platform::Linux => linux_readiness(),
                Some(milestone) => (false, format!("no backend yet — Milestone {milestone}")),
                None => (false, "no backend yet — see PLAN.md's web roadmap".to_owned()),
            };
            PlatformReadiness { platform: platform.to_string(), ready, detail }
        })
        .collect();

        Self { checks, platforms }
    }

    /// Whether everything required is present (optional checks aside).
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.checks.iter().filter(|check| check.name != "git").all(|check| check.ok)
    }

    /// The report as a person reads it.
    #[must_use]
    pub fn to_text(&self) -> String {
        use std::fmt::Write as _;

        let mut out = String::from("Toolchains\n");
        for check in &self.checks {
            let mark = if check.ok { "ok  " } else { "MISS" };
            let _ = writeln!(out, "  [{mark}] {:<32} {}", check.name, check.detail);
        }
        out.push_str("\nPlatforms\n");
        for platform in &self.platforms {
            let mark = if platform.ready { "ok  " } else { "--  " };
            let _ = writeln!(out, "  [{mark}] {:<32} {}", platform.platform, platform.detail);
        }
        out
    }

    /// The report as a script reads it.
    ///
    /// # Errors
    ///
    /// Serialization failure, which cannot happen for this shape but is
    /// reported rather than unwrapped.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// What `rustup` can add that a `rustnative` command needs: `(what,
/// rustup arguments, why)`.
const INSTALLABLE: [(&str, [&str; 3], &str); 1] = [(
    "llvm-tools",
    ["component", "add", "llvm-tools"],
    "`rustnative build --release --pgo` merges its profiles with llvm-profdata",
)];

/// The installable pieces this toolchain lacks.
#[must_use]
pub fn missing_installable() -> Vec<(&'static str, [&'static str; 3], &'static str)> {
    let installed = std::process::Command::new("rustup")
        .args(["component", "list", "--installed"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    INSTALLABLE
        .into_iter()
        .filter(|(name, _, _)| !installed.lines().any(|line| line.starts_with(name)))
        .collect()
}

/// `rustnative doctor --install`: adds what `rustup` can add and a command
/// needs. The Windows SDK and the MSVC build tools are the person's to
/// install (`doctor` says where); device toolchains are owed with their
/// backends.
///
/// # Errors
///
/// `rustup` cannot be run, or fails.
pub fn install(dry_run: bool) -> crate::error::Result<()> {
    let missing = missing_installable();
    if missing.is_empty() {
        println!("nothing to install: every piece rustup can add is present");
        return Ok(());
    }
    for (name, arguments, why) in missing {
        println!("{name}: {why}");
        println!("  rustup {}", arguments.join(" "));
        if dry_run {
            continue;
        }
        let status =
            std::process::Command::new("rustup").args(arguments).status().map_err(|cause| {
                crate::error::Error::ToolMissing {
                    tool: "rustup",
                    hint: "install Rust through https://rustup.rs".into(),
                    cause: Some(cause.to_string()),
                }
            })?;
        if !status.success() {
            return Err(crate::error::Error::ToolFailed { tool: "rustup", code: status.code() });
        }
    }
    Ok(())
}

/// Whether this machine builds the Linux backend: a Linux host with GTK
/// 4.14 or newer and libsoup 3's development files, as `pkg-config` finds
/// them, and whether it can make an AppImage.
fn linux_readiness() -> (bool, String) {
    if !cfg!(target_os = "linux") {
        return (false, "build on Linux (from Windows: inside WSL)".to_owned());
    }
    let version = |library: &str| {
        std::process::Command::new("pkg-config")
            .args(["--modversion", library])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    let Some(gtk) = version("gtk4") else {
        return (false, "GTK 4 development files missing (apt install libgtk-4-dev)".to_owned());
    };
    let recent =
        gtk.split('.').take(2).map(|part| part.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>()
            >= vec![4, 14];
    if !recent {
        return (false, format!("GTK {gtk} is older than 4.14"));
    }
    if version("libsoup-3.0").is_none() {
        return (
            false,
            "libsoup 3 development files missing (apt install libsoup-3.0-dev)".to_owned(),
        );
    }
    let appimage = if crate::package::linux::which("appimagetool").is_some() {
        "AppImage tool present"
    } else {
        "deb and tar (appimagetool missing for AppImages)"
    };
    (true, format!("ready — GTK {gtk}; packages: {appimage}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_covers_every_platform_and_the_toolchains_that_matter() {
        let report = Report::gather();
        let names = report.checks.iter().map(|check| check.name.as_str()).collect::<Vec<_>>();
        for expected in ["rustc", "cargo", "windows sdk", "makeappx", "signtool"] {
            assert!(names.contains(&expected), "{expected} is checked: {names:?}");
        }
        assert_eq!(report.platforms.len(), 7);
        let windows = &report.platforms[0];
        assert_eq!(windows.platform, "windows");
        let linux = &report.platforms[2];
        assert_eq!(linux.platform, "linux");
        if !cfg!(target_os = "linux") {
            assert!(!linux.ready && linux.detail.contains("build on Linux"), "{}", linux.detail);
        }
        for platform in
            report.platforms.iter().filter(|p| !["windows", "linux"].contains(&p.platform.as_str()))
        {
            assert!(!platform.ready, "{} has no backend yet", platform.platform);
            assert!(platform.detail.contains("no backend yet"));
        }
        // rustc and cargo are obviously present: they built this test.
        assert!(report.checks[0].ok && report.checks[1].ok);
    }

    #[test]
    fn the_json_report_is_json() {
        let json = Report::gather().to_json().expect("serializable");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert!(parsed["checks"].is_array());
        assert!(parsed["platforms"].is_array());
    }
}
