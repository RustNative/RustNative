//! The Android toolchain: the SDK, the NDK, a JDK, Gradle, and `adb` — found,
//! never installed behind the person's back (`rustnative doctor` names what
//! is missing and how to get it).
//!
//! | Tool | Where it is looked for |
//! |---|---|
//! | SDK | `ANDROID_HOME`, `ANDROID_SDK_ROOT`, then the per-user default (`%LOCALAPPDATA%\Android\Sdk`, `~/Android/Sdk`, `~/Library/Android/sdk`) |
//! | NDK | `ANDROID_NDK_HOME`, `ANDROID_NDK_ROOT`, then the newest under `<sdk>/ndk` |
//! | JDK | `JAVA_HOME`, then Android Studio's bundled runtime (`jbr`) |
//! | Gradle | `RUSTNATIVE_GRADLE`, `gradle` on the path, `GRADLE_HOME`, then the newest distribution a Gradle wrapper downloaded under `GRADLE_USER_HOME` (`~/.gradle`) |
//! | `adb` | `<sdk>/platform-tools` |

use std::path::{Path, PathBuf};

/// The API level the backend supports from (`minSdk`).
pub const MIN_API: u32 = 26;

/// The Rust targets of the four Android ABIs, with each ABI's name and the
/// NDK's clang prefix for it.
pub const TARGETS: [(&str, &str, &str); 4] = [
    ("aarch64-linux-android", "arm64-v8a", "aarch64-linux-android"),
    ("armv7-linux-androideabi", "armeabi-v7a", "armv7a-linux-androideabi"),
    ("x86_64-linux-android", "x86_64", "x86_64-linux-android"),
    ("i686-linux-android", "x86", "i686-linux-android"),
];

/// The Rust target for an ABI name (`arm64-v8a`).
#[must_use]
pub fn target_for_abi(abi: &str) -> Option<&'static str> {
    TARGETS.iter().find(|(_, name, _)| *name == abi).map(|(target, _, _)| *target)
}

/// What was found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AndroidToolchain {
    /// The SDK.
    pub sdk: Option<PathBuf>,
    /// The NDK.
    pub ndk: Option<PathBuf>,
    /// The JDK (`JAVA_HOME`).
    pub java_home: Option<PathBuf>,
    /// The Gradle executable.
    pub gradle: Option<PathBuf>,
    /// `adb`.
    pub adb: Option<PathBuf>,
    /// The highest platform installed under `<sdk>/platforms`.
    pub platform: Option<u32>,
}

impl AndroidToolchain {
    /// Looks for every tool.
    #[must_use]
    pub fn detect() -> Self {
        let sdk = sdk();
        let ndk = sdk.as_deref().and_then(ndk);
        let adb = sdk
            .as_ref()
            .map(|sdk| sdk.join("platform-tools").join(exe("adb")))
            .filter(|adb| adb.is_file());
        let platform = sdk.as_deref().and_then(highest_platform);
        Self { sdk, ndk, java_home: java_home(), gradle: gradle(), adb, platform }
    }

    /// Whether everything a build needs was found.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.sdk.is_some()
            && self.ndk.is_some()
            && self.java_home.is_some()
            && self.gradle.is_some()
    }

    /// The NDK's clang for `target` at the minimum API level, the linker
    /// Cargo links an Android library with.
    #[must_use]
    pub fn linker(&self, target: &str) -> Option<PathBuf> {
        let (_, _, prefix) = TARGETS.iter().find(|(name, _, _)| *name == target)?;
        let suffix = if cfg!(windows) { ".cmd" } else { "" };
        let path = self.llvm_bin()?.join(format!("{prefix}{MIN_API}-clang{suffix}"));
        path.is_file().then_some(path)
    }

    /// The NDK's prebuilt LLVM `bin` folder for this host.
    #[must_use]
    pub fn llvm_bin(&self) -> Option<PathBuf> {
        let host = if cfg!(windows) {
            "windows-x86_64"
        } else if cfg!(target_os = "macos") {
            "darwin-x86_64"
        } else {
            "linux-x86_64"
        };
        let bin = self
            .ndk
            .as_ref()?
            .join("toolchains")
            .join("llvm")
            .join("prebuilt")
            .join(host)
            .join("bin");
        bin.is_dir().then_some(bin)
    }

    /// The environment Cargo needs to build for `target`: its linker, and
    /// the C compiler and archiver for crates with C parts.
    #[must_use]
    pub fn cargo_environment(&self, target: &str) -> Vec<(String, String)> {
        let mut environment = Vec::new();
        let Some(linker) = self.linker(target) else { return environment };
        let upper = target.to_uppercase().replace('-', "_");
        let lower = target.replace('-', "_");
        let linker = linker.to_string_lossy().into_owned();
        environment.push((format!("CARGO_TARGET_{upper}_LINKER"), linker.clone()));
        environment.push((format!("CC_{lower}"), linker));
        if let Some(bin) = self.llvm_bin() {
            environment.push((
                format!("AR_{lower}"),
                bin.join(exe("llvm-ar")).to_string_lossy().into_owned(),
            ));
        }
        if let Some(ndk) = &self.ndk {
            environment.push(("ANDROID_NDK_HOME".to_owned(), ndk.to_string_lossy().into_owned()));
        }
        environment
    }
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() }
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|path| path.is_dir())
}

fn sdk() -> Option<PathBuf> {
    env_dir("ANDROID_HOME").or_else(|| env_dir("ANDROID_SDK_ROOT")).or_else(|| {
        let candidates = [
            std::env::var_os("LOCALAPPDATA")
                .map(|base| PathBuf::from(base).join("Android").join("Sdk")),
            home().map(|home| home.join("Android").join("Sdk")),
            home().map(|home| home.join("Library").join("Android").join("sdk")),
        ];
        candidates.into_iter().flatten().find(|path| path.is_dir())
    })
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

/// Version-ordered: `29.0.14206865` after `28.2.13676358`.
fn version_key(name: &str) -> Vec<u64> {
    name.split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

fn newest_child(directory: &Path, keep: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut children: Vec<PathBuf> = std::fs::read_dir(directory)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| keep(path))
        .collect();
    children.sort_by_key(|path| {
        version_key(
            &path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
        )
    });
    children.pop()
}

fn ndk(sdk: &Path) -> Option<PathBuf> {
    env_dir("ANDROID_NDK_HOME")
        .or_else(|| env_dir("ANDROID_NDK_ROOT"))
        .or_else(|| newest_child(&sdk.join("ndk"), |path| path.join("toolchains").is_dir()))
}

fn highest_platform(sdk: &Path) -> Option<u32> {
    std::fs::read_dir(sdk.join("platforms"))
        .ok()?
        .flatten()
        .filter_map(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .strip_prefix("android-")
                .and_then(|level| level.split('.').next()?.parse().ok())
        })
        .max()
}

fn java_home() -> Option<PathBuf> {
    let has_javac = |path: &Path| path.join("bin").join(exe("javac")).is_file();
    if let Some(home) = env_dir("JAVA_HOME").filter(|path| has_javac(path)) {
        return Some(home);
    }
    let mut candidates = Vec::new();
    for root in ["C:\\Program Files", "D:\\Program Files", "E:\\Program Files"] {
        candidates.push(PathBuf::from(root).join("Android").join("Android Studio").join("jbr"));
    }
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(base).join("Programs").join("Android Studio").join("jbr"));
    }
    candidates.push(PathBuf::from("/opt/android-studio/jbr"));
    candidates.push(PathBuf::from("/Applications/Android Studio.app/Contents/jbr/Contents/Home"));
    candidates.into_iter().find(|path| has_javac(path))
}

fn gradle() -> Option<PathBuf> {
    let script = if cfg!(windows) { "gradle.bat" } else { "gradle" };
    if let Some(explicit) =
        std::env::var_os("RUSTNATIVE_GRADLE").map(PathBuf::from).filter(|path| path.is_file())
    {
        return Some(explicit);
    }
    if let Some(on_path) = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(script))
            .find(|candidate| candidate.is_file())
    }) {
        return Some(on_path);
    }
    if let Some(home) = env_dir("GRADLE_HOME")
        .map(|home| home.join("bin").join(script))
        .filter(|path| path.is_file())
    {
        return Some(home);
    }
    let user_home =
        env_dir("GRADLE_USER_HOME").or_else(|| home().map(|home| home.join(".gradle")))?;
    // `wrapper/dists/gradle-8.10.2-all/<hash>/gradle-8.10.2/bin/gradle`.
    let dists = user_home.join("wrapper").join("dists");
    let distribution = newest_child(&dists, Path::is_dir)?;
    for hashed in std::fs::read_dir(&distribution).ok()?.flatten() {
        if let Some(unpacked) =
            newest_child(&hashed.path(), |path| path.join("bin").join(script).is_file())
        {
            return Some(unpacked.join("bin").join(script));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abis_name_their_targets() {
        assert_eq!(target_for_abi("arm64-v8a"), Some("aarch64-linux-android"));
        assert_eq!(target_for_abi("x86"), Some("i686-linux-android"));
        assert_eq!(target_for_abi("mips"), None);
    }

    #[test]
    fn versions_order_numerically() {
        let mut names = vec!["29.0.14206865", "28.2.13676358", "29.0.9"];
        names.sort_by_key(|name| version_key(name));
        assert_eq!(names, ["28.2.13676358", "29.0.9", "29.0.14206865"]);
    }
}
