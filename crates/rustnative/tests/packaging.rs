//! Milestone 32 acceptance tests: what `rustnative package` produces, and what an
//! executable built through `rustnative-build` actually carries.
//!
//! These run the real binary against a real generated project, then read
//! the results back the way Windows does — the version resource through
//! `GetFileVersionInfoW`, the manifest through `FindResourceW` — rather
//! than trusting the scripts that asked for them.
#![cfg(windows)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use windows_sys::Win32::Foundation::FreeLibrary;
use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows_sys::Win32::System::LibraryLoader::{
    FindResourceW, LOAD_LIBRARY_AS_DATAFILE, LoadLibraryExW, SizeofResource,
};

fn rustnative() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustnative"))
}

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A project whose packages these tests build, shared by them all: one
/// release build is enough, and it is the slowest thing here.
fn packaged_project() -> &'static PathBuf {
    use std::sync::OnceLock;

    static PROJECT: OnceLock<PathBuf> = OnceLock::new();
    PROJECT.get_or_init(|| {
        let parent =
            std::env::temp_dir().join(format!("rustnative-package-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&parent);
        std::fs::create_dir_all(&parent).expect("a scratch folder");
        let created = rustnative()
            .args(["new", "packaged", "--syntax", "builder", "--path"])
            .arg(&parent)
            .arg("--framework-path")
            .arg(workspace())
            .output()
            .expect("rustnative runs");
        assert!(created.status.success(), "{}", stderr(&created));

        let project = parent.join("packaged");
        let output = rustnative()
            .current_dir(&project)
            .args(["package", "windows", "--format", "all"])
            .env("CARGO_TARGET_DIR", workspace().join("target"))
            .output()
            .expect("rustnative runs");
        assert!(output.status.success(), "packaging must succeed:\n{}", stderr(&output));
        project
    })
}

/// Where packages are written.
fn package_dir() -> PathBuf {
    packaged_project().join("target").join("package")
}

/// The release executable the packages were built from.
fn executable() -> PathBuf {
    let _ = packaged_project();
    workspace().join("target").join("release").join("packaged.exe")
}

#[test]
fn the_executable_carries_the_version_information_rf_toml_declares() {
    let path = wide(&executable().display().to_string());
    // SAFETY: `path` is a NUL-terminated wide string that lives for the call.
    let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), std::ptr::null_mut()) };
    assert!(size > 0, "the executable has a version resource");

    let mut buffer = vec![0u8; size as usize];
    // SAFETY: `buffer` is exactly `size` bytes, as just reported.
    let read =
        unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, buffer.as_mut_ptr().cast()) } != 0;
    assert!(read, "the version resource is readable");

    let mut value: *mut std::ffi::c_void = std::ptr::null_mut();
    let mut length = 0u32;
    // The language/code page the resource script declares: US English,
    // Unicode.
    let query = wide(r"\StringFileInfo\040904B0\ProductVersion");
    // SAFETY: the buffer holds a version resource; `query` is a
    // NUL-terminated wide string; both out-parameters are exclusively
    // borrowed.
    let found = unsafe {
        VerQueryValueW(buffer.as_ptr().cast(), query.as_ptr(), &raw mut value, &raw mut length)
    } != 0;
    assert!(found, "ProductVersion is in the resource");
    // SAFETY: `value`/`length` describe a wide string inside `buffer`.
    let text = unsafe { std::slice::from_raw_parts(value.cast::<u16>(), length as usize) };
    let product_version = String::from_utf16_lossy(text).trim_end_matches('\0').to_owned();
    assert_eq!(product_version, "0.1.0.0", "the version rustnative.toml declares");
}

#[test]
fn the_executable_carries_the_application_manifest() {
    let path = wide(&executable().display().to_string());
    // SAFETY: `path` is NUL-terminated; `LOAD_LIBRARY_AS_DATAFILE` maps the
    // file without running any of its code.
    let module =
        unsafe { LoadLibraryExW(path.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_AS_DATAFILE) };
    assert!(!module.is_null(), "the executable can be mapped as a data file");

    // `RT_MANIFEST` is resource type 24, and id 1 is the manifest a process
    // is activated with.
    // `MAKEINTRESOURCE`: a small integer *is* the "pointer" these take.
    let manifest_type = std::ptr::without_provenance::<u16>(24);
    let first = std::ptr::without_provenance::<u16>(1);
    // SAFETY: `module` is a live mapped module; the type and id are the
    // documented integer-resource forms.
    let resource = unsafe { FindResourceW(module, first, manifest_type) };
    assert!(!resource.is_null(), "the executable has an RT_MANIFEST resource");
    // SAFETY: as above; `resource` was just found in `module`.
    let size = unsafe { SizeofResource(module, resource) };
    assert!(size > 100, "the manifest is a real document, {size} bytes");
    // SAFETY: `module` came from `LoadLibraryExW` above and is not used
    // after this call.
    unsafe { FreeLibrary(module) };
}

#[test]
fn the_portable_zip_is_reproducible_and_its_checksums_verify() {
    let project = packaged_project();
    let zip = package_dir().join("packaged-0.1.0.zip");
    let first = std::fs::read(&zip).expect("the archive was written");

    // Packaging again — a separate process, a separate build — produces the
    // same bytes.
    let again = rustnative()
        .current_dir(project)
        .args(["package", "windows", "--format", "zip"])
        .env("CARGO_TARGET_DIR", workspace().join("target"))
        .output()
        .expect("rustnative runs");
    assert!(again.status.success(), "{}", stderr(&again));
    let second = std::fs::read(&zip).expect("the archive was rewritten");
    assert_eq!(first, second, "two builds of the same files are byte-identical");

    // The `SHA256SUMS` entry describes the executable that is in the
    // archive beside it.
    let text = String::from_utf8_lossy(&first).into_owned();
    let sums_at = text.find("SHA256SUMS").expect("the archive names its checksum file");
    let sums = &text[sums_at + "SHA256SUMS".len()..];
    let line = sums.lines().next().unwrap_or_default();
    let (hash, name) = line.split_once("  ").expect("`<hash>  <name>`");
    assert_eq!(name.trim_end_matches(char::from(0)).trim(), "packaged.exe", "{line}");
    assert_eq!(hash.len(), 64, "a SHA-256 in hexadecimal: {hash}");
    let expected = sha256_of(&std::fs::read(executable()).unwrap());
    assert_eq!(hash, expected, "the checksum is of the executable that was packaged");
}

/// The SHA-256 of `data`, computed independently of the code under test.
fn sha256_of(data: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};

    use std::fmt::Write as _;

    Sha256::digest(data).iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

#[test]
fn the_msix_packs_and_unpacks_with_the_manifest_rf_toml_describes() {
    let msix = package_dir().join("packaged-0.1.0.msix");
    assert!(msix.is_file(), "an MSIX was produced at {}", msix.display());

    let makeappx = makeappx().expect("this machine has the Windows SDK: it packed the MSIX");
    let unpacked = package_dir().join("unpacked");
    let _ = std::fs::remove_dir_all(&unpacked);
    let output = Command::new(makeappx)
        .arg("unpack")
        .arg("/p")
        .arg(&msix)
        .arg("/d")
        .arg(&unpacked)
        .arg("/o")
        .output()
        .expect("makeappx runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));

    let manifest = std::fs::read_to_string(unpacked.join("AppxManifest.xml")).expect("a manifest");
    assert!(manifest.contains(r#"Name="com.example.packaged""#), "{manifest}");
    assert!(manifest.contains(r#"Version="0.1.0.0""#), "{manifest}");
    assert!(manifest.contains(r#"Executable="packaged.exe""#), "{manifest}");
    assert!(manifest.contains("runFullTrust"), "{manifest}");
    assert!(unpacked.join("packaged.exe").is_file(), "the executable is in the package");
    assert!(unpacked.join("assets/Square44x44Logo.png").is_file(), "and so are its logos");
}

#[test]
fn signing_without_a_certificate_is_refused_before_anything_is_built() {
    let project = packaged_project();
    let output = rustnative()
        .current_dir(project)
        .args(["package", "windows", "--format", "msix", "--sign", "no-such.pfx"])
        .output()
        .expect("rustnative runs");
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(stderr(&output).contains("no-such.pfx"), "{}", stderr(&output));
}

/// The SDK's `makeappx`, found the way `rustnative doctor` finds it.
fn makeappx() -> Option<PathBuf> {
    let output = rustnative().args(["doctor", "--json"]).output().ok()?;
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let checks = report["checks"].as_array()?;
    let entry = checks.iter().find(|check| check["name"] == "makeappx")?;
    let path = entry["detail"].as_str()?;
    let path = PathBuf::from(path);
    path.is_file().then_some(path)
}

/// Sanity: the helpers above point at files that exist.
#[test]
fn the_packaged_project_builds_what_these_tests_read() {
    assert!(executable().is_file(), "{}", executable().display());
    assert!(Path::new(&package_dir()).is_dir());
}
