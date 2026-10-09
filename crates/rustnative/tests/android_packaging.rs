//! Milestone 35's packaging acceptance tests: what `rustnative package
//! android` produces, read back from the archives themselves — the APK's
//! and the App Bundle's entries, and the APK's signature through the SDK's
//! own `apksigner` — rather than trusting the Gradle project that asked for
//! them.
//!
//! They need the Android SDK, the NDK, a JDK, and the `aarch64-linux-android`
//! Rust target, and they build a whole application, so they are ignored by
//! default: `cargo test -p rustnative-cli --test android_packaging --
//! --ignored` runs them (CI's `android` job does).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn rustnative() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustnative"))
}

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A generated project depending on this workspace, built for one ABI.
fn project(name: &str) -> PathBuf {
    let parent =
        std::env::temp_dir().join(format!("rustnative-android-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let output = rustnative()
        .args(["new", name, "--syntax", "builder", "--path"])
        .arg(&parent)
        .arg("--framework-path")
        .arg(workspace())
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let root = parent.join(name);
    let toml = root.join("rustnative.toml");
    let mut text = std::fs::read_to_string(&toml).unwrap();
    text.push_str("\n[android]\nabis = [\"arm64-v8a\"]\n");
    std::fs::write(&toml, text).unwrap();
    root
}

/// `rustnative package android` in `root`, sharing the workspace's target
/// folder so the framework is not rebuilt.
fn package(root: &Path, format: &str, env: &[(&str, &str)]) -> Output {
    rustnative()
        .current_dir(root)
        .args(["package", "android", "--format", format])
        .env("CARGO_TARGET_DIR", workspace().join("target"))
        .envs(env.iter().copied())
        .output()
        .unwrap()
}

/// The paths it printed as packaged.
fn packaged(output: &Output) -> Vec<PathBuf> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix("Packaged "))
        .map(PathBuf::from)
        .collect()
}

/// A zip archive's entry names, from its central directory.
fn entries(path: &Path) -> Vec<String> {
    let bytes = std::fs::read(path).unwrap();
    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let u32_at =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let end = (0..bytes.len() - 21).rev().find(|at| u32_at(*at) == 0x0605_4b50).unwrap();
    let mut at = u32_at(end + 16) as usize;
    let mut names = Vec::new();
    for _ in 0..u16_at(end + 10) {
        assert_eq!(u32_at(at), 0x0201_4b50, "a central directory entry");
        let name = usize::from(u16_at(at + 28));
        names.push(String::from_utf8_lossy(&bytes[at + 46..at + 46 + name]).into_owned());
        at += 46 + name + usize::from(u16_at(at + 30)) + usize::from(u16_at(at + 32));
    }
    names
}

fn android_home() -> PathBuf {
    std::env::var_os("ANDROID_HOME")
        .or_else(|| std::env::var_os("ANDROID_SDK_ROOT"))
        .map(PathBuf::from)
        .expect("ANDROID_HOME is set (tools/android-env.sh)")
}

/// The newest build-tools' `apksigner`.
fn apksigner() -> PathBuf {
    let tools = android_home().join("build-tools");
    let newest =
        std::fs::read_dir(&tools).unwrap().flatten().map(|entry| entry.path()).max().unwrap();
    newest.join(if cfg!(windows) { "apksigner.bat" } else { "apksigner" })
}

#[test]
#[ignore = "needs the Android SDK, NDK, and JDK, and builds an application"]
fn an_unsigned_release_is_an_apk_and_a_bundle_carrying_the_library() {
    let root = project("pkg-unsigned");
    let output = package(&root, "all", &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    let paths = packaged(&output);
    let apk = paths.iter().find(|path| path.extension().is_some_and(|ext| ext == "apk")).unwrap();
    let aab = paths.iter().find(|path| path.extension().is_some_and(|ext| ext == "aab")).unwrap();
    assert!(apk.to_string_lossy().contains("unsigned"), "no keystore: an unsigned APK ({apk:?})");
    assert!(String::from_utf8_lossy(&output.stdout).contains("is unsigned"), "and it says so");

    let apk_entries = entries(apk);
    for expected in ["AndroidManifest.xml", "classes.dex", "lib/arm64-v8a/libpkg_unsigned.so"] {
        assert!(apk_entries.iter().any(|entry| entry == expected), "{expected} in {apk_entries:?}");
    }
    assert!(!apk_entries.iter().any(|entry| entry.starts_with("lib/x86")), "only the declared ABI");
    let bundle = entries(aab);
    assert!(bundle.iter().any(|entry| entry == "base/lib/arm64-v8a/libpkg_unsigned.so"));
    assert!(bundle.iter().any(|entry| entry == "base/manifest/AndroidManifest.xml"));
}

#[test]
#[ignore = "needs the Android SDK, NDK, and JDK, and builds an application"]
fn a_release_with_a_keystore_is_signed_with_its_key() {
    let root = project("pkg-signed");
    let java_home = PathBuf::from(std::env::var_os("JAVA_HOME").expect("JAVA_HOME is set"));
    let keytool = java_home.join("bin").join(if cfg!(windows) { "keytool.exe" } else { "keytool" });
    let store = root.join("release.jks");
    let made = Command::new(keytool)
        .args(["-genkeypair", "-keystore"])
        .arg(&store)
        .args([
            "-alias",
            "release",
            "-keyalg",
            "RSA",
            "-keysize",
            "2048",
            "-validity",
            "365",
            "-storepass",
            "store-secret",
            "-keypass",
            "store-secret",
            "-dname",
            "CN=Rust Native packaging test",
        ])
        .output()
        .unwrap();
    assert!(made.status.success(), "{}", stderr(&made));
    let toml = root.join("rustnative.toml");
    let mut text = std::fs::read_to_string(&toml).unwrap();
    text.push_str(
        "\n[android.keystore]\nstore = \"release.jks\"\nalias = \"release\"\n\
         store-password-env = \"PKG_STORE_PASSWORD\"\nkey-password-env = \"PKG_KEY_PASSWORD\"\n",
    );
    std::fs::write(&toml, text).unwrap();

    let output = package(
        &root,
        "apk",
        &[("PKG_STORE_PASSWORD", "store-secret"), ("PKG_KEY_PASSWORD", "store-secret")],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let apk = packaged(&output).into_iter().next().unwrap();
    assert!(!apk.to_string_lossy().contains("unsigned"), "{apk:?}");
    let verified =
        Command::new(apksigner()).args(["verify", "--print-certs"]).arg(&apk).output().unwrap();
    let printed = String::from_utf8_lossy(&verified.stdout);
    assert!(verified.status.success(), "apksigner verifies it: {}", stderr(&verified));
    assert!(
        printed.contains("CN=Rust Native packaging test"),
        "signed with the keystore's key: {printed}"
    );
    // The passwords are read from the environment, never written into the
    // generated project.
    let generated = std::fs::read_to_string(
        workspace().join("target/rustnative/android/pkg-signed/app/build.gradle"),
    )
    .unwrap();
    assert!(!generated.contains("store-secret"), "no password in the Gradle file");
}
