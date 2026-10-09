//! Updates on Android (`PLAN.md` Milestone 50; `docs/deploy/update-rules.md`).
//!
//! Who may update an Android application depends on where it came from:
//!
//! - **Installed from Google Play:** Play updates it, and Play's policy
//!   forbids an application updating its own code any other way.
//!   `AndroidUpdater::install` refuses (`UpdateError::Forbidden`); model
//!   and data payloads ([`PayloadKind::Model`]) are still allowed — they
//!   are data, not code.
//! - **Sideloaded** (an APK from the publisher's site, an enterprise
//!   channel): the application may offer its own update through the system
//!   package installer. The person confirms it in the system's dialog;
//!   Android checks the new APK is signed with the same key before
//!   replacing the application.
//!
//! The manifest is the desktops' (`rustnative_windows::update`): an
//! Ed25519-signed JSON description of a version, its package's SHA-256, and
//! a rollout percentage, checked against the publisher's public key pinned
//! in the application. `rustnative update sign` signs it for every host.

use std::path::PathBuf;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

/// What an update carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PayloadKind {
    /// A new version of the application (an APK).
    Application,
    /// A model or data file for the application.
    Model {
        /// Its name.
        name: String,
        /// The application versions it works with (`1.2`: every `1.2.x`).
        compatible: String,
    },
}

/// A signed description of an update — the same JSON as the desktops'.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateManifest {
    /// The application's id.
    pub app: String,
    /// The version offered.
    pub version: String,
    /// Where its package is.
    pub url: String,
    /// The package's SHA-256, hex.
    pub sha256: String,
    /// The share of installations offered it, 0–100.
    pub rollout: u8,
    /// What it carries.
    pub payload: PayloadKind,
    /// Ed25519 over the other fields, hex.
    #[serde(default)]
    pub signature: String,
}

impl UpdateManifest {
    /// Whether the signature is `key`'s, over the manifest without it.
    #[must_use]
    pub fn verify(&self, key: &VerifyingKey) -> bool {
        let unsigned = Self { signature: String::new(), ..self.clone() };
        let Ok(bytes) = serde_json::to_vec(&unsigned) else { return false };
        let Some(signature) = unhex(&self.signature) else { return false };
        let Ok(signature) = Signature::from_slice(&signature) else { return false };
        key.verify(&bytes, &signature).is_ok()
    }
}

/// Why an update was not applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    /// The manifest's signature is not the pinned key's.
    BadSignature,
    /// The package's digest does not match the manifest.
    BadDigest,
    /// The manifest could not be read.
    BadManifest(String),
    /// This installation may not update its own code (it came from Google
    /// Play, which updates it).
    Forbidden,
    /// A file could not be written, or the installer refused.
    Io(String),
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadSignature => formatter.write_str("the update is not signed by the publisher"),
            Self::BadDigest => formatter.write_str("the package does not match its manifest"),
            Self::BadManifest(why) => write!(formatter, "the manifest is unreadable: {why}"),
            Self::Forbidden => formatter.write_str(
                "this installation came from Google Play, which updates it: an application may not update its own code",
            ),
            Self::Io(why) => write!(formatter, "{why}"),
        }
    }
}

impl std::error::Error for UpdateError {}

/// What to do about a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Nothing newer.
    UpToDate,
    /// Newer, but this installation's bucket is not in the rollout yet.
    NotYetRolledOut,
    /// The installation is pinned to its version.
    Pinned,
    /// A payload for other application versions; never used.
    Incompatible,
    /// Take it.
    Update(UpdateManifest),
}

fn newer(candidate: &str, current: &str) -> bool {
    let parse = |version: &str| {
        version.split('.').map(|part| part.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>()
    };
    parse(candidate) > parse(current)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
        out
    })
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok())
        .collect()
}

#[allow(clippy::needless_pass_by_value, reason = "used as `map_err(io)`")]
fn io(error: std::io::Error) -> UpdateError {
    UpdateError::Io(error.to_string())
}

/// An installation's updater.
#[derive(Debug)]
pub struct AndroidUpdater {
    key: VerifyingKey,
    root: PathBuf,
    current: String,
}

impl AndroidUpdater {
    /// The updater keeping its records under `root` (on a device,
    /// `AndroidUpdater::for_application`), running version `current`,
    /// trusting `public_key` (hex).
    ///
    /// # Errors
    ///
    /// The key is not an Ed25519 public key.
    pub fn new(
        root: impl Into<PathBuf>,
        public_key: &str,
        current: &str,
    ) -> Result<Self, UpdateError> {
        let bytes: [u8; 32] = unhex(public_key)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(UpdateError::BadSignature)?;
        let key = VerifyingKey::from_bytes(&bytes).map_err(|_| UpdateError::BadSignature)?;
        Ok(Self { key, root: root.into(), current: current.to_owned() })
    }

    /// The updater for this application, its records in
    /// `getFilesDir()/rustnative/update`.
    ///
    /// # Errors
    ///
    /// The key is not an Ed25519 public key, or there is no files directory.
    #[cfg(target_os = "android")]
    pub fn for_application(public_key: &str, current: &str) -> Result<Self, UpdateError> {
        let files =
            crate::services::files_dir().map_err(|error| UpdateError::Io(error.to_string()))?;
        Self::new(files.join("rustnative").join("update"), public_key, current)
    }

    /// This installation's stable rollout bucket, 0–99.
    ///
    /// # Errors
    ///
    /// The id file cannot be written.
    pub fn bucket(&self) -> Result<u8, UpdateError> {
        let path = self.root.join("installation-id");
        let id: u64 = if let Ok(text) = std::fs::read_to_string(&path) {
            text.trim().parse().unwrap_or(0)
        } else {
            // Only spreads installations over the rollout; not a secret.
            use std::hash::{BuildHasher, Hasher};
            let id = std::collections::hash_map::RandomState::new().build_hasher().finish();
            std::fs::create_dir_all(&self.root).map_err(io)?;
            std::fs::write(&path, id.to_string()).map_err(io)?;
            id
        };
        Ok(u8::try_from(id % 100).unwrap_or(0))
    }

    /// Pins the installation to its version (or releases the pin).
    ///
    /// # Errors
    ///
    /// The pin file cannot be written.
    pub fn pin(&self, pinned: bool) -> Result<(), UpdateError> {
        let path = self.root.join("pinned");
        if pinned {
            std::fs::create_dir_all(&self.root).map_err(io)?;
            std::fs::write(path, &self.current).map_err(io)
        } else {
            let _ = std::fs::remove_file(path);
            Ok(())
        }
    }

    /// Decides about a manifest (its JSON).
    ///
    /// # Errors
    ///
    /// A malformed or unsigned manifest.
    pub fn check(&self, manifest: &[u8]) -> Result<Decision, UpdateError> {
        let manifest: UpdateManifest = serde_json::from_slice(manifest)
            .map_err(|error| UpdateError::BadManifest(error.to_string()))?;
        if !manifest.verify(&self.key) {
            return Err(UpdateError::BadSignature);
        }
        let installed = match &manifest.payload {
            PayloadKind::Application => self.current.clone(),
            PayloadKind::Model { name, compatible } => {
                // `1.2` matches `1.2` and every `1.2.x`, never `1.20`.
                if self.current != *compatible
                    && !self.current.starts_with(&format!("{compatible}."))
                {
                    return Ok(Decision::Incompatible);
                }
                std::fs::read_to_string(self.root.join("models").join(name).join("current"))
                    .map_or_else(|_| "0".to_owned(), |text| text.trim().to_owned())
            }
        };
        if !newer(&manifest.version, &installed) {
            return Ok(Decision::UpToDate);
        }
        if self.root.join("pinned").exists() {
            return Ok(Decision::Pinned);
        }
        if self.bucket()? >= manifest.rollout {
            return Ok(Decision::NotYetRolledOut);
        }
        Ok(Decision::Update(manifest))
    }

    /// Verifies a model payload against its manifest and makes it current:
    /// `models/<name>/<version>`, returned, with `models/<name>/current`
    /// naming it.
    ///
    /// # Errors
    ///
    /// The manifest is not a model's, the digest does not match, or the
    /// files cannot be written.
    pub fn stage_model(
        &self,
        manifest: &UpdateManifest,
        payload: &[u8],
    ) -> Result<PathBuf, UpdateError> {
        let PayloadKind::Model { name, .. } = &manifest.payload else {
            return Err(UpdateError::BadManifest("not a model payload".to_owned()));
        };
        verify_digest(manifest, payload)?;
        let models = self.root.join("models").join(name);
        std::fs::create_dir_all(&models).map_err(io)?;
        let target = models.join(&manifest.version);
        let temporary = models.join(format!("{}.new", manifest.version));
        std::fs::write(&temporary, payload).map_err(io)?;
        std::fs::rename(&temporary, &target).map_err(io)?;
        let pointer = models.join("current.new");
        std::fs::write(&pointer, &manifest.version).map_err(io)?;
        std::fs::rename(&pointer, models.join("current")).map_err(io)?;
        Ok(target)
    }

    /// Verifies an application update's APK and hands it to the system
    /// package installer, which asks the person to confirm. The
    /// application declares `android.permission.REQUEST_INSTALL_PACKAGES`,
    /// and the person allows "install unknown apps" for it the first time.
    ///
    /// # Errors
    ///
    /// The installation came from Google Play ([`UpdateError::Forbidden`]),
    /// the digest does not match, or the installer refused the session.
    #[cfg(target_os = "android")]
    pub fn install(&self, manifest: &UpdateManifest, apk: &[u8]) -> Result<(), UpdateError> {
        if !matches!(manifest.payload, PayloadKind::Application) {
            return Err(UpdateError::BadManifest("not an application update".to_owned()));
        }
        if from_play() {
            return Err(UpdateError::Forbidden);
        }
        verify_digest(manifest, apk)?;
        let message = crate::jni_host::call_static(
            crate::jni_host::Class::Services,
            "installPackage",
            "([B)Ljava/lang/String;",
            &[crate::jni_host::Arg::Bytes(apk)],
        )
        .map_err(|error| UpdateError::Io(error.to_string()))?
        .string();
        message.map_or(Ok(()), |message| Err(UpdateError::Io(message)))
    }
}

/// Whether this installation came from Google Play.
#[cfg(target_os = "android")]
#[must_use]
pub fn from_play() -> bool {
    crate::jni_host::call_static(
        crate::jni_host::Class::Services,
        "installer",
        "()Ljava/lang/String;",
        &[],
    )
    .ok()
    .and_then(crate::jni_host::Ret::string)
    .is_some_and(|installer| installer == "com.android.vending")
}

fn verify_digest(manifest: &UpdateManifest, bytes: &[u8]) -> Result<(), UpdateError> {
    if hex(&sha2::Sha256::digest(bytes)) == manifest.sha256.to_ascii_lowercase() {
        Ok(())
    } else {
        Err(UpdateError::BadDigest)
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer as _, SigningKey};

    use super::*;

    fn signed(version: &str, payload: PayloadKind, body: &[u8], rollout: u8) -> (Vec<u8>, String) {
        let secret = SigningKey::from_bytes(&[7; 32]);
        let mut manifest = UpdateManifest {
            app: "dev.example".into(),
            version: version.into(),
            url: "https://example.com/x".into(),
            sha256: hex(&sha2::Sha256::digest(body)),
            rollout,
            payload,
            signature: String::new(),
        };
        let bytes = serde_json::to_vec(&manifest).unwrap_or_default();
        manifest.signature = hex(&secret.sign(&bytes).to_bytes());
        (serde_json::to_vec(&manifest).unwrap_or_default(), hex(secret.verifying_key().as_bytes()))
    }

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir()
            .join(format!("rustnative-android-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        directory
    }

    #[test]
    fn a_signed_newer_manifest_is_taken_and_a_tampered_one_refused() {
        let (manifest, key) = signed("1.1.0", PayloadKind::Application, b"apk", 100);
        let updater = AndroidUpdater::new(scratch("take"), &key, "1.0.0").expect("a key");
        assert!(matches!(updater.check(&manifest), Ok(Decision::Update(_))));
        let tampered = String::from_utf8_lossy(&manifest).replace("1.1.0", "9.9.9");
        assert_eq!(updater.check(tampered.as_bytes()), Err(UpdateError::BadSignature));
        let (zero, _) = signed("1.1.0", PayloadKind::Application, b"apk", 0);
        assert_eq!(updater.check(&zero), Ok(Decision::NotYetRolledOut));
        let older = AndroidUpdater::new(scratch("older"), &key, "1.2.0").expect("a key");
        assert_eq!(older.check(&manifest), Ok(Decision::UpToDate));
        updater.pin(true).expect("pinned");
        assert_eq!(updater.check(&manifest), Ok(Decision::Pinned));
    }

    #[test]
    fn a_model_is_verified_staged_and_made_current() {
        let model = PayloadKind::Model { name: "words".into(), compatible: "1.0".into() };
        let (manifest, key) = signed("3", model, b"weights", 100);
        let root = scratch("model");
        let updater = AndroidUpdater::new(&root, &key, "1.0.4").expect("a key");
        let Ok(Decision::Update(manifest)) = updater.check(&manifest) else { panic!("an update") };
        assert_eq!(updater.stage_model(&manifest, b"other"), Err(UpdateError::BadDigest));
        let staged = updater.stage_model(&manifest, b"weights").expect("staged");
        assert_eq!(std::fs::read(staged).ok().as_deref(), Some(&b"weights"[..]));
        assert_eq!(
            updater.check(&serde_json::to_vec(&manifest).unwrap_or_default()),
            Ok(Decision::UpToDate)
        );
        let elsewhere = AndroidUpdater::new(scratch("model2"), &key, "1.1.0").expect("a key");
        assert_eq!(
            elsewhere.check(&serde_json::to_vec(&manifest).unwrap_or_default()),
            Ok(Decision::Incompatible)
        );
    }
}
