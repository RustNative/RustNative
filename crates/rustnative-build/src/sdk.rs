//! Finding `rc.exe`.
//!
//! A build script cannot assume the SDK is on the path — it usually is not,
//! outside a Developer Command Prompt — so the newest installed Windows SDK
//! is located the same way `rustnative doctor` locates it, by looking under
//! `Windows Kits\10\bin`.

use std::path::{Path, PathBuf};

/// The newest installed `rc.exe`, if there is one.
#[must_use]
pub fn resource_compiler() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("RUSTNATIVE_RC_EXE").map(PathBuf::from) {
        // An escape hatch for a machine whose SDK is somewhere unusual,
        // and how this crate's own tests point at a stand-in.
        return path.is_file().then_some(path);
    }
    latest_sdk_bin().and_then(|bin| {
        let rc = bin.join("rc.exe");
        rc.is_file().then_some(rc)
    })
}

/// The `bin\<version>\<arch>` folder of the newest installed Windows SDK.
fn latest_sdk_bin() -> Option<PathBuf> {
    let architecture = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" };
    let mut best: Option<(Vec<u64>, PathBuf)> = None;
    for root in ["ProgramFiles(x86)", "ProgramFiles"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(|root| PathBuf::from(root).join("Windows Kits").join("10").join("bin"))
    {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("10.") {
                continue;
            }
            let bin = entry.path().join(architecture);
            if !bin.is_dir() {
                continue;
            }
            let version = numbers(&name);
            if best.as_ref().is_none_or(|(newest, _)| version > *newest) {
                best = Some((version, bin));
            }
        }
    }
    best.map(|(_, bin)| bin)
}

fn numbers(version: &str) -> Vec<u64> {
    version.split('.').map(|part| part.parse().unwrap_or(0)).collect()
}

/// Whether `path` looks like a usable tool.
#[must_use]
pub fn exists(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_folders_are_compared_by_number() {
        assert!(numbers("10.0.26100.0") > numbers("10.0.9999.0"));
    }

    #[test]
    fn the_override_is_used_when_it_points_at_a_real_file() {
        // Safe: this test's process is the only reader of the variable, and
        // the value is removed before the test returns.
        let temporary =
            std::env::temp_dir().join(format!("rustnative-rc-{}.exe", std::process::id()));
        std::fs::write(&temporary, b"not really a compiler").expect("a scratch file");
        // SAFETY: `set_var` is unsafe because another thread could be
        // reading the environment; this test reads it back itself on the
        // same thread and clears it immediately.
        unsafe { std::env::set_var("RUSTNATIVE_RC_EXE", &temporary) };
        assert_eq!(resource_compiler(), Some(temporary.clone()));
        // SAFETY: as above.
        unsafe { std::env::remove_var("RUSTNATIVE_RC_EXE") };
        std::fs::remove_file(&temporary).ok();
    }
}
