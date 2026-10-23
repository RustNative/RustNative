//! `rustnative build <platform> --release --pgo`: a profile-guided release
//! build from a scripted startup (`PLAN.md` Milestone 42, `C62-2`).
//!
//! 1. Build instrumented (`-Cprofile-generate`) into `target/pgo`.
//! 2. Run it once with `RUSTNATIVE_EXIT_AT=interactive`: the backend quits
//!    the moment the application becomes interactive, so the profile is
//!    exactly the startup path.
//! 3. Merge the raw profiles with `llvm-profdata` (the `llvm-tools` rustup
//!    component).
//! 4. Build the release with `-Cprofile-use`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::project::Project;

/// How long the scripted startup may take before it is abandoned.
const STARTUP_TIMEOUT: Duration = Duration::from_mins(2);

fn tool_output(tool: &'static str, command: &mut Command) -> Result<String> {
    let output = command.output().map_err(|cause| Error::ToolMissing {
        tool,
        hint: "install the Rust toolchain from https://rustup.rs".into(),
        cause: Some(cause.to_string()),
    })?;
    if !output.status.success() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        return Err(Error::ToolFailed { tool, code: output.status.code() });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Where `llvm-profdata` is for a toolchain whose sysroot is `sysroot` and
/// host triple is `host`.
#[must_use]
pub fn profdata_path(sysroot: &Path, host: &str) -> PathBuf {
    sysroot
        .join("lib/rustlib")
        .join(host)
        .join("bin")
        .join(format!("llvm-profdata{}", std::env::consts::EXE_SUFFIX))
}

/// The active toolchain's `llvm-profdata`.
fn llvm_profdata() -> Result<PathBuf> {
    let sysroot = tool_output("rustc", Command::new("rustc").args(["--print", "sysroot"]))?;
    let version = tool_output("rustc", Command::new("rustc").arg("-vV"))?;
    let host = version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap_or_default()
        .trim()
        .to_owned();
    let path = profdata_path(Path::new(sysroot.trim()), &host);
    if path.is_file() {
        Ok(path)
    } else {
        Err(Error::ToolMissing {
            tool: "llvm-profdata",
            hint: "a profile-guided build merges its profiles with the `llvm-tools` component: \
                   run `rustup component add llvm-tools`"
                .into(),
            cause: Some(format!("not found at {}", path.display())),
        })
    }
}

fn cargo_build(root: &Path, target_dir: &Path, rustflags: &str) -> Result<()> {
    let mut flags = std::env::var("RUSTFLAGS").unwrap_or_default();
    if !flags.is_empty() {
        flags.push(' ');
    }
    flags.push_str(rustflags);
    let status = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(root)
        .args(["build", "--release", "--target-dir"])
        .arg(target_dir)
        .env("RUSTFLAGS", flags)
        .status()
        .map_err(|cause| Error::ToolMissing {
            tool: "cargo",
            hint: "install the Rust toolchain from https://rustup.rs".into(),
            cause: Some(cause.to_string()),
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::ToolFailed { tool: "cargo", code: status.code() })
    }
}

/// Runs the instrumented executable through its startup.
fn scripted_startup(executable: &Path) -> Result<()> {
    let mut child = Command::new(executable)
        .env("RUSTNATIVE_EXIT_AT", "interactive")
        .spawn()
        .map_err(|cause| Error::Io { what: format!("run {}", executable.display()), cause })?;
    let started = Instant::now();
    loop {
        let exited = child
            .try_wait()
            .map_err(|cause| Error::Io { what: "wait for the scripted startup".into(), cause })?;
        if exited.is_some() {
            return Ok(());
        }
        if started.elapsed() > STARTUP_TIMEOUT {
            let _ = child.kill();
            return Err(Error::Usage(format!(
                "the scripted startup did not become interactive within {}s",
                STARTUP_TIMEOUT.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Builds `project` profile-guided.
pub fn build(project: &Project) -> Result<()> {
    // Checked first, so a missing tool is not found after a long build.
    let profdata = llvm_profdata()?;
    let pgo = project.root.join("target").join("pgo");
    let profiles = pgo.join("profiles");
    let _ = std::fs::remove_dir_all(&profiles);
    std::fs::create_dir_all(&profiles)
        .map_err(|cause| Error::Io { what: format!("create {}", profiles.display()), cause })?;

    println!("pgo: instrumented build");
    cargo_build(
        &project.root,
        &pgo.join("instrumented"),
        &format!("-Cprofile-generate={}", profiles.display()),
    )?;
    let name = format!("{}{}", project.config.app.name, std::env::consts::EXE_SUFFIX);
    println!("pgo: scripted startup");
    scripted_startup(&pgo.join("instrumented").join("release").join(&name))?;

    println!("pgo: merging profiles");
    let merged = pgo.join("merged.profdata");
    tool_output(
        "llvm-profdata",
        Command::new(&profdata).arg("merge").arg("-o").arg(&merged).arg(&profiles),
    )?;

    println!("pgo: optimized build");
    let target = project.root.join("target");
    cargo_build(&project.root, &target, &format!("-Cprofile-use={}", merged.display()))?;
    println!("pgo: built {}", target.join("release").join(name).display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llvm_profdata_is_looked_for_in_the_toolchain_s_own_bin_folder() {
        let path = profdata_path(Path::new("/toolchain"), "x86_64-pc-windows-msvc");
        let expected = Path::new("/toolchain/lib/rustlib/x86_64-pc-windows-msvc/bin");
        assert_eq!(path.parent(), Some(expected));
        assert!(
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("llvm-profdata"))
        );
    }
}
