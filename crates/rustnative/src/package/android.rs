//! `rustnative build|run|package android`: the application's Rust library
//! for each ABI, the Gradle project around it (`rustnative_build::android`),
//! and the APK or application bundle Gradle makes of them (`PLAN.md`
//! Milestone 35).
//!
//! ```text
//! cargo rustc --lib --crate-type cdylib --target <abi's triple>   (per ABI, the NDK's clang linking)
//!   → target/rustnative/android/<app>/app/src/main/jniLibs/<abi>/lib<name>.so
//! rustnative_build::android::AndroidProject::files()               (manifest, Gradle scripts, theme)
//!   + the backend's Java host library                             (crates/rustnative-android/java)
//! gradle assembleDebug | assembleRelease | bundleRelease
//!   → target/rustnative/android/<app>.apk | .aab
//! ```
//!
//! An application is built as a library (`crate-type = ["lib"]` is enough:
//! the `cdylib` is asked of Cargo here, so desktop builds never link one).

use std::path::{Path, PathBuf};
use std::process::Command;

use rustnative_build::android::{
    AndroidAssetPack, AndroidProject, AndroidSigning, AndroidTile, AndroidWidget, SURFACE_SLOTS,
};

use crate::config::Config;
use crate::error::{Error, Result};
use crate::toolchain::android::{AndroidToolchain, TARGETS, target_for_abi};

/// How to build.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Optimized libraries, and the release build type.
    pub release: bool,
    /// The ABIs to build (`[android] abis`, or all four when empty).
    pub abis: Vec<String>,
    /// Carry the device suite's instrumentation (`tools/android-device-test.sh`).
    pub instrumentation: bool,
    /// Extra Cargo features for the application's library.
    pub features: Vec<String>,
}

/// What `cargo metadata` says the build needs.
struct Metadata {
    target_directory: PathBuf,
    library: String,
    java: PathBuf,
}

fn io(what: impl Into<String>) -> impl FnOnce(std::io::Error) -> Error {
    let what = what.into();
    move |cause| Error::Io { what, cause }
}

fn usage(text: impl Into<String>) -> Error {
    Error::Usage(text.into())
}

/// The toolchain, or what is missing.
///
/// # Errors
///
/// [`Error::Usage`] naming each missing tool and where it is looked for.
pub fn toolchain() -> Result<AndroidToolchain> {
    let toolchain = AndroidToolchain::detect();
    if toolchain.is_complete() {
        return Ok(toolchain);
    }
    let mut missing = Vec::new();
    if toolchain.sdk.is_none() {
        missing.push("the Android SDK (set ANDROID_HOME)");
    }
    if toolchain.ndk.is_none() {
        missing.push("the NDK (sdkmanager \"ndk;<version>\", or set ANDROID_NDK_HOME)");
    }
    if toolchain.java_home.is_none() {
        missing.push("a JDK 17 (set JAVA_HOME, or install Android Studio)");
    }
    if toolchain.gradle.is_none() {
        missing.push("Gradle (put it on the path, or set RUSTNATIVE_GRADLE)");
    }
    if missing.is_empty() {
        Ok(toolchain)
    } else {
        Err(usage(format!(
            "cannot build for Android; missing: {}. `rustnative doctor` checks again",
            missing.join("; ")
        )))
    }
}

fn metadata(root: &Path, config: &Config) -> Result<Metadata> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .output()
        .map_err(io("run cargo metadata"))?;
    if !output.status.success() {
        return Err(usage(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|cause| usage(format!("cargo metadata's answer: {cause}")))?;
    let target_directory = value["target_directory"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| usage("cargo metadata named no target directory"))?;
    let packages = value["packages"].as_array().cloned().unwrap_or_default();
    let own = root.join("Cargo.toml").canonicalize().map_err(io("find Cargo.toml"))?;
    let package = packages
        .iter()
        .find(|package| {
            package["manifest_path"]
                .as_str()
                .and_then(|path| Path::new(path).canonicalize().ok())
                .is_some_and(|path| path == own)
        })
        .ok_or_else(|| usage("this project's package is not in cargo metadata"))?;
    let library = package["targets"]
        .as_array()
        .and_then(|targets| targets.iter().find(|target| target["kind"].as_array().is_some_and(|kinds| kinds.iter().any(|kind| kind == "lib" || kind == "cdylib" || kind == "rlib"))))
        .and_then(|target| target["name"].as_str())
        .map(|name| name.replace('-', "_"))
        .ok_or_else(|| {
            usage(format!(
                "`{}` has no library target: an Android application is built as a library (add `src/lib.rs` exporting `main` with `rustnative_android::export_main!`)",
                config.app.name
            ))
        })?;
    let java = packages
        .iter()
        .find(|package| package["name"] == "rustnative-android")
        .and_then(|package| package["manifest_path"].as_str())
        .and_then(|path| Path::new(path).parent().map(|crate_dir| crate_dir.join("java")))
        .filter(|java| java.is_dir())
        .ok_or_else(|| usage("the application does not depend on rustnative-android, whose Java host library the package needs"))?;
    Ok(Metadata { target_directory, library, java })
}

/// The Gradle project's folder for `config`'s application.
#[must_use]
pub fn project_dir(target_directory: &Path, config: &Config) -> PathBuf {
    target_directory.join("rustnative").join("android").join(&config.app.name)
}

/// The project description `config` gives.
///
/// # Errors
///
/// [`Error::Usage`] for more widgets or tiles than the host library has
/// slots for.
pub fn describe(
    config: &Config,
    library: &str,
    instrumentation: bool,
    root: &Path,
) -> Result<AndroidProject> {
    let app = &config.app;
    let android = config.android.clone().unwrap_or_default();
    // `RUSTNATIVE_ANDROID_APPLICATION_ID` overrides it for one build: a
    // device that confirms every new package's install (HyperOS) takes an
    // update of one it already trusts without asking.
    let id = std::env::var("RUSTNATIVE_ANDROID_APPLICATION_ID")
        .ok()
        .filter(|id| !id.is_empty())
        .or_else(|| android.application_id.clone())
        .unwrap_or_else(|| app.id.clone());
    let mut project = AndroidProject::new(&id, &app.display_name, &app.version, library);
    if let Some(min) = android.min_sdk {
        project.min_sdk = min.max(crate::toolchain::android::MIN_API);
    }
    if let Some(target) = android.target_sdk {
        project.target_sdk = target;
    }
    project.url_schemes.clone_from(&app.url_schemes);
    project.share_types.clone_from(&android.share_types);
    project.permissions.clone_from(&android.permissions);
    project.launcher.clone_from(&android.launcher);
    if android.widgets.len() > SURFACE_SLOTS || android.tiles.len() > SURFACE_SLOTS {
        return Err(usage(format!(
            "an application may declare at most {SURFACE_SLOTS} widgets and {SURFACE_SLOTS} tiles"
        )));
    }
    project.widgets = android
        .widgets
        .iter()
        .map(|widget| AndroidWidget { id: widget.id.clone(), label: widget.label.clone() })
        .collect();
    project.tiles = android
        .tiles
        .iter()
        .map(|tile| AndroidTile { id: tile.id.clone(), label: tile.label.clone() })
        .collect();
    project.asset_packs = android
        .asset_packs
        .iter()
        .map(|pack| AndroidAssetPack { name: pack.name.clone(), delivery: pack.delivery.clone() })
        .collect();
    project.signing = android.keystore.as_ref().map(|keystore| AndroidSigning {
        store: if keystore.store.is_absolute() {
            keystore.store.to_string_lossy().into_owned()
        } else {
            root.join(&keystore.store).to_string_lossy().into_owned()
        },
        alias: keystore.alias.clone(),
        store_password_env: keystore.store_password_env.clone(),
        key_password_env: keystore.key_password_env.clone(),
    });
    project.instrumentation = instrumentation;
    project.icon = app.icon.is_some();
    Ok(project)
}

/// Builds the libraries and writes the Gradle project; returns its folder.
///
/// # Errors
///
/// A missing tool, a failed Cargo build, or a file that could not be
/// written.
pub fn prepare(root: &Path, config: &Config, options: &Options) -> Result<PathBuf> {
    let toolchain = toolchain()?;
    let metadata = metadata(root, config)?;
    let abis: Vec<String> = if options.abis.is_empty() {
        config
            .android
            .as_ref()
            .map(|android| android.abis.clone())
            .filter(|abis| !abis.is_empty())
            .unwrap_or_else(|| TARGETS.iter().map(|(_, abi, _)| (*abi).to_owned()).collect())
    } else {
        options.abis.clone()
    };
    let project_dir = project_dir(&metadata.target_directory, config);
    let main = project_dir.join("app").join("src").join("main");
    // Generated outputs are rebuilt from scratch, so nothing stale survives.
    for generated in [main.join("java"), main.join("jniLibs"), main.join("res")] {
        if generated.exists() {
            std::fs::remove_dir_all(&generated)
                .map_err(io(format!("clear {}", generated.display())))?;
        }
    }
    for abi in &abis {
        let target = target_for_abi(abi).ok_or_else(|| {
            usage(format!("unknown ABI `{abi}` (arm64-v8a, armeabi-v7a, x86_64, or x86)"))
        })?;
        println!("build: {} for {abi} ({target})", config.app.display_name);
        let mut cargo = Command::new("cargo");
        cargo
            .args([
                "rustc",
                "--lib",
                "--crate-type",
                "cdylib",
                "--target",
                target,
                "--manifest-path",
            ])
            .arg(root.join("Cargo.toml"));
        if options.release {
            cargo.arg("--release");
        }
        if !options.features.is_empty() {
            cargo.args(["--features", &options.features.join(",")]);
        }
        for (name, value) in toolchain.cargo_environment(target) {
            cargo.env(name, value);
        }
        let status = cargo.status().map_err(io("run cargo"))?;
        if !status.success() {
            return Err(usage(format!("the build for {abi} failed")));
        }
        let built = metadata
            .target_directory
            .join(target)
            .join(if options.release { "release" } else { "debug" })
            .join(format!("lib{}.so", metadata.library));
        let destination = main.join("jniLibs").join(abi);
        std::fs::create_dir_all(&destination)
            .map_err(io(format!("create {}", destination.display())))?;
        std::fs::copy(&built, destination.join(format!("lib{}.so", metadata.library)))
            .map_err(io(format!("copy {}", built.display())))?;
    }
    let mut project = describe(config, &metadata.library, options.instrumentation, root)?;
    // The NDK's version is its directory's name (`ndk/29.0.14206865`).
    project.ndk_version = toolchain
        .ndk
        .as_ref()
        .and_then(|ndk| ndk.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| name.chars().next().is_some_and(|first| first.is_ascii_digit()));
    if !options.release {
        // A debug build can be inspected (`run android --inspect`), whose
        // server listens on the device's loopback: a socket needs INTERNET.
        project.permissions.push("android.permission.INTERNET".to_owned());
    }
    for (path, contents) in project.files() {
        let path = project_dir.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io(format!("create {}", parent.display())))?;
        }
        // Unchanged files keep their timestamps, so Gradle's own up-to-date
        // checks hold.
        if std::fs::read_to_string(&path).ok().as_deref() != Some(contents.as_str()) {
            std::fs::write(&path, contents).map_err(io(format!("write {}", path.display())))?;
        }
    }
    copy_tree(&metadata.java, &main.join("java"))?;
    if let Some(sources) = config.android.as_ref().and_then(|android| android.java_sources.as_ref())
    {
        copy_tree(&root.join(sources), &main.join("java"))?;
    }
    if let Some(icon) = &config.app.icon {
        let destination = main.join("res").join("mipmap-xxxhdpi");
        std::fs::create_dir_all(&destination)
            .map_err(io(format!("create {}", destination.display())))?;
        std::fs::copy(root.join(icon), destination.join("ic_launcher.png"))
            .map_err(io(format!("copy {}", icon.display())))?;
    }
    if let Some(android) = &config.android {
        for pack in &android.asset_packs {
            let destination = project_dir.join(&pack.name).join("src").join("main").join("assets");
            copy_tree(&root.join(&pack.path), &destination)?;
        }
    }
    if let Some(sdk) = &toolchain.sdk {
        let escaped = sdk.to_string_lossy().replace('\\', "/").replace(':', "\\:");
        std::fs::write(project_dir.join("local.properties"), format!("sdk.dir={escaped}\n"))
            .map_err(io("write local.properties"))?;
    }
    Ok(project_dir)
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).map_err(io(format!("create {}", to.display())))?;
    for entry in std::fs::read_dir(from).map_err(io(format!("read {}", from.display())))?.flatten()
    {
        let path = entry.path();
        let destination = to.join(entry.file_name());
        if path.is_dir() {
            copy_tree(&path, &destination)?;
        } else {
            std::fs::copy(&path, &destination).map_err(io(format!("copy {}", path.display())))?;
        }
    }
    Ok(())
}

/// Runs Gradle `tasks` in `project_dir`.
///
/// # Errors
///
/// Gradle could not run, or a task failed.
pub fn gradle(toolchain: &AndroidToolchain, project_dir: &Path, tasks: &[&str]) -> Result<()> {
    let gradle = toolchain.gradle.as_ref().ok_or_else(|| usage("Gradle was not found"))?;
    let mut command = Command::new(gradle);
    command.current_dir(project_dir).args(["--console=plain", "--warning-mode=none"]).args(tasks);
    if let Some(java) = &toolchain.java_home {
        command.env("JAVA_HOME", java);
    }
    println!("gradle: {}", tasks.join(" "));
    let status = command.status().map_err(io(format!("run {}", gradle.display())))?;
    if status.success() { Ok(()) } else { Err(usage(format!("gradle {} failed", tasks.join(" ")))) }
}

/// Builds the APK; returns it.
///
/// # Errors
///
/// As [`prepare`] and [`gradle`].
pub fn build(root: &Path, config: &Config, options: &Options) -> Result<PathBuf> {
    let toolchain = toolchain()?;
    let project_dir = prepare(root, config, options)?;
    let (task, folder) =
        if options.release { ("assembleRelease", "release") } else { ("assembleDebug", "debug") };
    gradle(&toolchain, &project_dir, &[task])?;
    let outputs = project_dir.join("app").join("build").join("outputs").join("apk").join(folder);
    std::fs::read_dir(&outputs)
        .map_err(io(format!("read {}", outputs.display())))?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|extension| extension == "apk"))
        .ok_or_else(|| usage(format!("Gradle made no APK in {}", outputs.display())))
}

/// Packages the application: a release APK, an application bundle, or both.
///
/// # Errors
///
/// As [`build`].
pub fn package(root: &Path, config: &Config, apk: bool, aab: bool) -> Result<Vec<PathBuf>> {
    let toolchain = toolchain()?;
    let options = Options { release: true, ..Options::default() };
    let project_dir = prepare(root, config, &options)?;
    let mut tasks = Vec::new();
    if apk {
        tasks.push("assembleRelease");
    }
    if aab {
        tasks.push("bundleRelease");
    }
    gradle(&toolchain, &project_dir, &tasks)?;
    let out_dir = root.join("target").join("rustnative").join("android");
    std::fs::create_dir_all(&out_dir).map_err(io(format!("create {}", out_dir.display())))?;
    let mut produced = Vec::new();
    let outputs = project_dir.join("app").join("build").join("outputs");
    for (wanted, folder, extension) in [
        (apk, outputs.join("apk").join("release"), "apk"),
        (aab, outputs.join("bundle").join("release"), "aab"),
    ] {
        if !wanted {
            continue;
        }
        let Some(made) = std::fs::read_dir(&folder)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .find(|path| path.extension().is_some_and(|found| found == extension))
        else {
            return Err(usage(format!("Gradle made no .{extension} in {}", folder.display())));
        };
        let unsigned =
            made.file_name().is_some_and(|name| name.to_string_lossy().contains("unsigned"));
        let name = format!(
            "{}-{}{}.{extension}",
            config.app.name,
            config.app.version,
            if unsigned { "-unsigned" } else { "" }
        );
        let destination = out_dir.join(name);
        std::fs::copy(&made, &destination).map_err(io(format!("copy {}", made.display())))?;
        if unsigned {
            println!(
                "package: {} is unsigned: declare [android.keystore] in rustnative.toml to sign releases",
                destination.display()
            );
        }
        produced.push(destination);
    }
    Ok(produced)
}

/// The application id `config` gives.
#[must_use]
pub fn application_id(config: &Config) -> String {
    let id = std::env::var("RUSTNATIVE_ANDROID_APPLICATION_ID")
        .ok()
        .filter(|id| !id.is_empty())
        .or_else(|| config.android.as_ref().and_then(|android| android.application_id.clone()))
        .unwrap_or_else(|| config.app.id.clone());
    rustnative_build::android::application_id_from(&id)
}

/// Runs `adb` with `arguments`, returning its output.
///
/// # Errors
///
/// `adb` was not found or failed.
pub fn adb(toolchain: &AndroidToolchain, arguments: &[&str]) -> Result<String> {
    let adb = toolchain
        .adb
        .as_ref()
        .ok_or_else(|| usage("adb was not found (the SDK's platform-tools)"))?;
    let output = Command::new(adb).args(arguments).output().map_err(io("run adb"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() {
        Ok(text)
    } else {
        Err(usage(format!("adb {} failed: {}", arguments.join(" "), text.trim())))
    }
}

/// Builds, installs, and launches the application on the attached device
/// (`ANDROID_SERIAL` picks one when several are attached).
///
/// # Errors
///
/// As [`build`], and a failed install or launch.
pub fn run(root: &Path, config: &Config, release: bool, inspect: bool) -> Result<()> {
    let toolchain = toolchain()?;
    // Only the device's own ABI: a debug library per ABI is large, and the
    // device runs one.
    let abis = adb(&toolchain, &["shell", "getprop", "ro.product.cpu.abi"])
        .map(|abi| abi.trim().to_owned())
        .ok()
        .filter(|abi| crate::toolchain::android::target_for_abi(abi).is_some())
        .into_iter()
        .collect();
    let apk = build(root, config, &Options { release, abis, ..Options::default() })?;
    install(&toolchain, &apk)?;
    let component = format!("{}/dev.rustnative.android.RnActivity", application_id(config));
    let mut start = vec!["shell", "am", "start", "-W", "-n", component.as_str()];
    if inspect {
        // `RnIntents.inspect`: the inspection server starts with the
        // application (`rustnative inspect --android` reaches it).
        start.extend(["--es", "dev.rustnative.inspect", "1"]);
    }
    let output = adb(&toolchain, &start)?;
    print!("{output}");
    Ok(())
}

/// Installs `apk` on the attached device.
///
/// # Errors
///
/// `adb install` failed (on some devices, the person must confirm an
/// install over USB on the device itself).
pub fn install(toolchain: &AndroidToolchain, apk: &Path) -> Result<()> {
    println!("install: {}", apk.display());
    let apk = apk.to_string_lossy().into_owned();
    adb(toolchain, &["install", "-r", "-t", &apk]).map(|_| ())
}
