//! The library rung, end to end (`PLAN.md` Milestone 40): the shared
//! library this crate builds is driven by a C program compiled against the
//! generated header (MSVC on Windows, the system C compiler on Linux), and
//! on Windows by a C# program compiled with the .NET Framework compiler
//! against the generated bindings. Each host exits 0 only when every
//! expectation in it holds.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a failure is the test failing"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

/// Builds the DLL (a no-op when the workspace build already has) and
/// returns the directory holding it and its import library.
fn build_library() -> PathBuf {
    let status = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(workspace())
        .args(["build", "-p", "adoption-library", "--lib"])
        .status()
        .expect("cargo runs");
    assert!(status.success(), "the library builds");
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| workspace().join("target"), PathBuf::from);
    let directory = target.join("debug");
    let library = if cfg!(windows) { "counter.dll" } else { "libcounter.so" };
    assert!(directory.join(library).is_file(), "the library is at {}", directory.display());
    directory
}

fn scratch(name: &str) -> PathBuf {
    let directory = workspace().join("target").join("adoption-library").join(name);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("scratch directory");
    directory
}

fn generate(directory: &Path) {
    let source =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("counter.ril")).unwrap();
    let idl = rustnative_interop::parse_idl(&source).unwrap();
    std::fs::write(directory.join("counter.h"), rustnative_interop::generate::c(&idl)).unwrap();
    std::fs::write(directory.join("Counter.cs"), rustnative_interop::generate::csharp(&idl))
        .unwrap();
}

fn run(program: &Path) {
    let output = Command::new(program).output().expect("the host runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{} failed:\n{stdout}{stderr}", program.display());
    assert_eq!(stdout.trim(), "ok");
}

#[cfg(windows)]
#[test]
fn a_c_program_drives_the_model_through_the_generated_header() {
    let library = build_library();
    let directory = scratch("c");
    generate(&directory);
    std::fs::copy(library.join("counter.dll"), directory.join("counter.dll")).unwrap();
    let compiler = cc::windows_registry::find_tool("x86_64-pc-windows-msvc", "cl.exe")
        .expect("MSVC (cl.exe) is installed — `rustnative doctor` reports it");
    let host = Path::new(env!("CARGO_MANIFEST_DIR")).join("hosts").join("host.c");
    let output = compiler
        .to_command()
        .current_dir(&directory)
        .args(["/nologo", "/W4", "/WX", "/I."])
        .arg(&host)
        .arg("/Fe:host.exe")
        .arg(library.join("counter.dll.lib"))
        .output()
        .expect("cl runs");
    assert!(
        output.status.success(),
        "the generated header compiles warning-free:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    run(&directory.join("host.exe"));
}

/// The same host on Linux, compiled warning-free by the system's C
/// compiler and linked against the shared library.
#[cfg(target_os = "linux")]
#[test]
fn a_c_program_drives_the_model_through_the_generated_header() {
    let library = build_library();
    let directory = scratch("c");
    generate(&directory);
    let host = Path::new(env!("CARGO_MANIFEST_DIR")).join("hosts").join("host.c");
    let output = Command::new(std::env::var_os("CC").unwrap_or_else(|| "cc".into()))
        .current_dir(&directory)
        .args(["-Wall", "-Wextra", "-Werror", "-I."])
        .arg(&host)
        .args(["-o", "host"])
        .arg(format!("-L{}", library.display()))
        .arg("-lcounter")
        .arg(format!("-Wl,-rpath,{}", library.display()))
        .output()
        .expect("the C compiler runs (build-essential)");
    assert!(
        output.status.success(),
        "the generated header compiles warning-free:
{}",
        String::from_utf8_lossy(&output.stderr)
    );
    run(&directory.join("host"));
}

#[cfg(windows)]
#[test]
fn a_csharp_program_drives_the_model_through_the_generated_bindings() {
    let csc = Path::new(r"C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe");
    assert!(csc.is_file(), "the .NET Framework compiler ships with Windows");
    let library = build_library();
    let directory = scratch("csharp");
    generate(&directory);
    std::fs::copy(library.join("counter.dll"), directory.join("counter.dll")).unwrap();
    let host = Path::new(env!("CARGO_MANIFEST_DIR")).join("hosts").join("Host.cs");
    let output = Command::new(csc)
        .current_dir(&directory)
        .args(["/nologo", "/platform:x64", "/warnaserror", "/out:host.exe", "Counter.cs"])
        .arg(&host)
        .output()
        .expect("csc runs");
    assert!(
        output.status.success(),
        "the generated bindings compile warning-free:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    run(&directory.join("host.exe"));
}
