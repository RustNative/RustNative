//! Desktop updates (`PLAN.md` Milestone 50): the Windows counterpart of an
//! over-the-air update, for applications installed from the portable
//! package (an MSIX install updates through App Installer instead —
//! `rustnative package windows --format msix --appinstaller <url>`).
//!
//! - **Signed manifests.** The manifest, its signature, and the decision
//!   about it are `rustnative_update`'s, shared with every self-updating
//!   host: an Ed25519 signature over the manifest's published bytes by the
//!   publisher's key, pinned at build (`rustnative.toml` `[update]
//!   public-key`); an unsigned, tampered, expired, or other application's
//!   manifest is refused.
//! - **Staged rollout.** Each installation has a stable random id; it takes
//!   an update only when its bucket (0–99) is under the rollout percentage.
//! - **Side-by-side versions.** A version is unpacked into
//!   `versions/<version>/` beside the running one and verified before
//!   anything switches; activating it is an atomic rename of the `current`
//!   pointer, and the previous version is kept.
//! - **Automatic rollback.** The [`Launcher`] starts the current version.
//!   A version that fails twice before reaching `interactive` (it calls
//!   [`mark_interactive`] then — Milestone 42's startup phase) is rolled
//!   back to the previous one.
//! - **Pinning.** An installation can be pinned to its version.
//! - **Payloads.** A manifest may carry a model rather than the
//!   application ([`PayloadKind::Model`]), with the application versions it
//!   is compatible with (`C89`).

use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use rustnative_update::write_atomically;
pub use rustnative_update::{
    Decision, Installation, PayloadKind, UpdateError, UpdateManifest, public_key,
};

#[allow(clippy::needless_pass_by_value, reason = "used as `map_err(io)`")]
fn io(error: std::io::Error) -> UpdateError {
    UpdateError::Io(error.to_string())
}

/// An installation's updater: the shared decision, and how Windows stages
/// and switches versions.
#[derive(Debug)]
pub struct Updater {
    installation: Installation,
}

impl Updater {
    /// The updater for application `app` installed at `root` (its
    /// `versions/` and pointers live there), running `current`, trusting
    /// `public_key` (hex).
    ///
    /// # Errors
    ///
    /// The key is not an Ed25519 public key.
    pub fn new(
        root: impl Into<PathBuf>,
        app: &str,
        public_key: &str,
        current: &str,
    ) -> Result<Self, UpdateError> {
        Ok(Self { installation: Installation::new(root, app, public_key, current)? })
    }

    /// This installation's stable rollout bucket, 0–99.
    ///
    /// # Errors
    ///
    /// The id file cannot be written.
    pub fn bucket(&self) -> Result<u8, UpdateError> {
        self.installation.bucket()
    }

    /// Pins the installation to its version (or releases the pin).
    ///
    /// # Errors
    ///
    /// The pin file cannot be written.
    pub fn pin(&self, pinned: bool) -> Result<(), UpdateError> {
        self.installation.pin(pinned)
    }

    /// Decides about a published manifest.
    ///
    /// # Errors
    ///
    /// A malformed, unsigned, expired, or other application's manifest.
    pub fn check(&self, manifest: &[u8]) -> Result<Decision, UpdateError> {
        self.installation.check(manifest)
    }

    /// Verifies `package` against `manifest` and unpacks it beside the
    /// running version; nothing switches yet. A model payload has no start
    /// to fail, so it is made current as soon as it is verified and
    /// unpacked (`models/<name>/current`).
    ///
    /// # Errors
    ///
    /// The digest does not match, or the package cannot be unpacked.
    pub fn stage(&self, manifest: &UpdateManifest, package: &[u8]) -> Result<PathBuf, UpdateError> {
        manifest.verify_package(package)?;
        let versions = match &manifest.payload {
            PayloadKind::Application => self.installation.root().join("versions"),
            PayloadKind::Model { name, .. } => self.installation.model_directory(name),
        };
        let staging = versions.join(format!("{}.staging", manifest.version));
        let target = versions.join(&manifest.version);
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(io)?;
        for (name, contents) in stored_entries(package)? {
            // A package names files below itself only.
            if name.contains("..") || name.starts_with(['/', '\\']) || name.contains(':') {
                return Err(UpdateError::BadPackage(format!("unsafe path {name:?}")));
            }
            let path = staging.join(&name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(io)?;
            }
            std::fs::write(path, contents).map_err(io)?;
        }
        let _ = std::fs::remove_dir_all(&target);
        std::fs::rename(&staging, &target).map_err(io)?;
        if matches!(manifest.payload, PayloadKind::Model { .. }) {
            write_atomically(&versions.join("current"), manifest.version.as_bytes())?;
        }
        Ok(target)
    }

    /// Makes `version` the one the launcher starts, keeping the current one
    /// as the rollback target.
    ///
    /// # Errors
    ///
    /// The pointers cannot be written.
    pub fn activate(&self, version: &str) -> Result<(), UpdateError> {
        activate(self.installation.root(), version)
    }
}

/// Makes `version` current under `root`, keeping the old current as
/// previous.
///
/// # Errors
///
/// The pointers cannot be written.
pub fn activate(root: &Path, version: &str) -> Result<(), UpdateError> {
    std::fs::create_dir_all(root).map_err(io)?;
    if let Ok(current) = std::fs::read_to_string(root.join("current")) {
        if current.trim() != version {
            write_atomically(&root.join("previous"), current.trim().as_bytes())?;
        }
    }
    let _ = std::fs::remove_file(root.join("failures"));
    write_atomically(&root.join("current"), version.as_bytes())
}

/// Called by the application when it reaches `interactive`: this version
/// works, and failures are forgotten.
///
/// # Errors
///
/// The marker cannot be written.
pub fn mark_interactive(root: &Path, version: &str) -> Result<(), UpdateError> {
    std::fs::write(root.join("versions").join(version).join(".interactive"), "").map_err(io)?;
    let _ = std::fs::remove_file(root.join("failures"));
    Ok(())
}

/// Reads the entries of a stored (uncompressed) ZIP archive — the format
/// `rustnative package` writes.
fn stored_entries(archive: &[u8]) -> Result<Vec<(String, Vec<u8>)>, UpdateError> {
    let bad = |why: &str| UpdateError::BadPackage(why.to_owned());
    let u16_at =
        |at: usize| archive.get(at..at + 2).map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]));
    let u32_at = |at: usize| {
        archive
            .get(at..at + 4)
            .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    };
    let mut entries = Vec::new();
    let mut at = 0usize;
    while u32_at(at) == Some(0x0403_4b50) {
        let method = u16_at(at + 8).ok_or_else(|| bad("header"))?;
        if method != 0 {
            return Err(bad("only stored entries are read"));
        }
        let size = usize::try_from(u32_at(at + 18).ok_or_else(|| bad("size"))?)
            .map_err(|_| bad("size"))?;
        let name_length = usize::from(u16_at(at + 26).ok_or_else(|| bad("name"))?);
        let extra_length = usize::from(u16_at(at + 28).ok_or_else(|| bad("extra"))?);
        let name_start = at + 30;
        let data_start = name_start + name_length + extra_length;
        let name = String::from_utf8(
            archive.get(name_start..name_start + name_length).ok_or_else(|| bad("name"))?.to_vec(),
        )
        .map_err(|_| bad("name"))?;
        let data =
            archive.get(data_start..data_start + size).ok_or_else(|| bad("truncated"))?.to_vec();
        if !name.ends_with('/') {
            entries.push((name, data));
        }
        at = data_start + size;
    }
    Ok(entries)
}

/// Starts the current version's `exe` under `root` and watches it: a
/// version that fails twice before reaching `interactive` is rolled back.
pub struct Launcher {
    root: PathBuf,
    exe: String,
}

/// What a launch did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    /// It ran; its exit status's success.
    Ran {
        /// Which version.
        version: String,
        /// Whether it exited successfully or reached `interactive`.
        healthy: bool,
    },
    /// It failed twice and was rolled back to `to`.
    RolledBack {
        /// The failed version.
        from: String,
        /// The one restored.
        to: String,
    },
}

impl Launcher {
    /// A launcher for `exe` in each version directory under `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>, exe: &str) -> Self {
        Self { root: root.into(), exe: exe.to_owned() }
    }

    /// Runs the current version once.
    ///
    /// # Errors
    ///
    /// There is no current version, or it cannot be started.
    pub fn launch(&self) -> Result<Launch, UpdateError> {
        let version =
            std::fs::read_to_string(self.root.join("current")).map_err(io)?.trim().to_owned();
        let directory = self.root.join("versions").join(&version);
        let status: ExitStatus = std::process::Command::new(directory.join(&self.exe))
            .current_dir(&directory)
            .env("RUSTNATIVE_UPDATE_ROOT", &self.root)
            .env("RUSTNATIVE_VERSION", &version)
            .status()
            .map_err(io)?;
        let healthy = status.success() || directory.join(".interactive").exists();
        if healthy {
            let _ = std::fs::remove_file(self.root.join("failures"));
            return Ok(Launch::Ran { version, healthy });
        }
        let failures: u32 = std::fs::read_to_string(self.root.join("failures"))
            .ok()
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(0)
            + 1;
        std::fs::write(self.root.join("failures"), failures.to_string()).map_err(io)?;
        if failures >= 2 {
            if let Ok(previous) = std::fs::read_to_string(self.root.join("previous")) {
                let previous = previous.trim().to_owned();
                write_atomically(&self.root.join("current"), previous.as_bytes())?;
                let _ = std::fs::remove_file(self.root.join("failures"));
                return Ok(Launch::RolledBack { from: version, to: previous });
            }
        }
        Ok(Launch::Ran { version, healthy })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: [u8; 32] = [42; 32];

    /// A stored ZIP of `files`, as `rustnative package` writes one.
    fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, bytes) in files {
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            out.extend_from_slice(&0u32.to_le_bytes()); // crc (unchecked here)
            let size = u32::try_from(bytes.len()).unwrap();
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&u16::try_from(name.len()).unwrap().to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(bytes);
        }
        out
    }

    const APP: &str = "dev.rustnative.demo";

    fn manifest(version: &str, package: &[u8], rollout: u8) -> UpdateManifest {
        UpdateManifest {
            app: APP.into(),
            version: version.into(),
            url: format!("https://updates.example.com/demo-{version}.zip"),
            sha256: rustnative_update::sha256_hex(package),
            rollout,
            payload: PayloadKind::Application,
            expires: u64::MAX,
        }
    }

    fn system(program: &str) -> Vec<u8> {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        std::fs::read(Path::new(&root).join("System32").join(program)).unwrap()
    }

    #[test]
    fn an_update_is_verified_staged_switched_and_rolled_back_when_it_fails() {
        let root = std::env::temp_dir().join(format!("rustnative-update-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let updater = Updater::new(&root, APP, &public_key(&SECRET), "1.0.0").unwrap();

        // Version 1.0.0 is installed and runs (a program that succeeds).
        let good = zip(&[("app.exe", &system("whoami.exe"))]);
        updater.stage(&manifest("1.0.0", &good, 100), &good).unwrap();
        activate(&root, "1.0.0").unwrap();
        let launcher = Launcher::new(&root, "app.exe");
        assert_eq!(
            launcher.launch().unwrap(),
            Launch::Ran { version: "1.0.0".into(), healthy: true }
        );

        // A tampered manifest, a wrong package, and a pinned installation
        // are all refused.
        let broken = zip(&[("app.exe", &system("findstr.exe"))]);
        let offered = manifest("1.1.0", &broken, 100);
        let tampered = String::from_utf8(offered.sign(&SECRET))
            .unwrap()
            .replace("updates.example.com", "evil.example.com");
        assert_eq!(updater.check(tampered.as_bytes()), Err(UpdateError::BadSignature));
        assert_eq!(updater.stage(&offered, &good), Err(UpdateError::BadDigest));
        updater.pin(true).unwrap();
        assert_eq!(updater.check(&offered.sign(&SECRET)), Ok(Decision::Pinned));
        updater.pin(false).unwrap();
        let bucket = updater.bucket().unwrap();
        let mut staged_rollout = manifest("1.1.0", &broken, bucket);
        assert_eq!(
            updater.check(&staged_rollout.sign(&SECRET)),
            Ok(Decision::NotYetRolledOut),
            "a rollout below this installation's bucket skips it"
        );
        staged_rollout.rollout = bucket + 1;
        assert!(matches!(updater.check(&staged_rollout.sign(&SECRET)), Ok(Decision::Update(_))));

        // 1.1.0 is staged and activated — but it fails at start.
        let Ok(Decision::Update(taken)) = updater.check(&offered.sign(&SECRET)) else {
            panic!("offered")
        };
        updater.stage(&taken, &broken).unwrap();
        updater.activate("1.1.0").unwrap();
        assert!(
            matches!(launcher.launch().unwrap(), Launch::Ran { healthy: false, .. }),
            "first failure"
        );
        assert_eq!(
            launcher.launch().unwrap(),
            Launch::RolledBack { from: "1.1.0".into(), to: "1.0.0".into() }
        );
        assert_eq!(
            launcher.launch().unwrap(),
            Launch::Ran { version: "1.0.0".into(), healthy: true }
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_model_is_taken_only_by_the_application_versions_it_fits() {
        let root =
            std::env::temp_dir().join(format!("rustnative-update-model-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let weights = zip(&[("weights.bin", b"model v2")]);
        let offer = |compatible: &str| {
            let mut manifest = manifest("2", &weights, 100);
            manifest.payload =
                PayloadKind::Model { name: "ranker".into(), compatible: compatible.into() };
            manifest.sign(&SECRET)
        };
        let updater = Updater::new(&root, APP, &public_key(&SECRET), "1.20.0").unwrap();
        assert_eq!(updater.check(&offer("1.2")), Ok(Decision::Incompatible), "1.2 is not 1.20");
        let Ok(Decision::Update(taken)) = updater.check(&offer("1.20")) else {
            panic!("fits 1.20.x")
        };
        updater.stage(&taken, &weights).unwrap();
        assert_eq!(std::fs::read(root.join("models/ranker/2/weights.bin")).unwrap(), b"model v2");
        assert_eq!(
            updater.check(&offer("1.20")),
            Ok(Decision::UpToDate),
            "the model is now current"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_package_cannot_write_outside_its_version() {
        let root =
            std::env::temp_dir().join(format!("rustnative-update-escape-{}", std::process::id()));
        let updater = Updater::new(&root, APP, &public_key(&SECRET), "1.0.0").unwrap();
        for name in ["../../escaped.txt", r"\escaped.txt", "C:escaped.txt"] {
            let evil = zip(&[(name, b"x")]);
            assert!(
                matches!(
                    updater.stage(&manifest("2.0.0", &evil, 100), &evil),
                    Err(UpdateError::BadPackage(_))
                ),
                "{name}"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
