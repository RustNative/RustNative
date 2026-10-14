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
//! The manifest, its signature, and the decision about it are
//! `rustnative_update`'s, shared with every self-updating host; this module
//! holds only how Android installs what was decided.

use std::path::PathBuf;

use rustnative_update::write_atomically;
pub use rustnative_update::{
    Decision, Installation, PayloadKind, UpdateError, UpdateManifest, public_key,
};

#[allow(clippy::needless_pass_by_value, reason = "used as `map_err(io)`")]
fn io(error: std::io::Error) -> UpdateError {
    UpdateError::Io(error.to_string())
}

/// An installation's updater.
#[derive(Debug)]
pub struct AndroidUpdater {
    installation: Installation,
}

impl AndroidUpdater {
    /// The updater for application `app` keeping its records under `root`
    /// (on a device, `AndroidUpdater::for_application`), running version
    /// `current`, trusting `public_key` (hex).
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

    /// The updater for this application (`app`, its package name), its
    /// records in `getFilesDir()/rustnative/update`.
    ///
    /// # Errors
    ///
    /// The key is not an Ed25519 public key, or there is no files directory.
    #[cfg(target_os = "android")]
    pub fn for_application(
        app: &str,
        public_key: &str,
        current: &str,
    ) -> Result<Self, UpdateError> {
        let files =
            crate::services::files_dir().map_err(|error| UpdateError::Io(error.to_string()))?;
        Self::new(files.join("rustnative").join("update"), app, public_key, current)
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
        manifest.verify_package(payload)?;
        let models = self.installation.model_directory(name);
        std::fs::create_dir_all(&models).map_err(io)?;
        let target = models.join(&manifest.version);
        write_atomically(&target, payload)?;
        write_atomically(&models.join("current"), manifest.version.as_bytes())?;
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
        manifest.verify_package(apk)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: [u8; 32] = [7; 32];
    const APP: &str = "dev.example";

    fn signed(version: &str, payload: PayloadKind, body: &[u8], rollout: u8) -> Vec<u8> {
        UpdateManifest {
            app: APP.into(),
            version: version.into(),
            url: "https://example.com/x".into(),
            sha256: rustnative_update::sha256_hex(body),
            rollout,
            payload,
            expires: u64::MAX,
        }
        .sign(&SECRET)
    }

    fn installed(name: &str, current: &str) -> AndroidUpdater {
        let directory = std::env::temp_dir()
            .join(format!("rustnative-android-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        AndroidUpdater::new(directory, APP, &public_key(&SECRET), current).expect("a key")
    }

    #[test]
    fn a_signed_newer_manifest_is_taken_and_a_tampered_one_refused() {
        let manifest = signed("1.1.0", PayloadKind::Application, b"apk", 100);
        let updater = installed("take", "1.0.0");
        assert!(matches!(updater.check(&manifest), Ok(Decision::Update(_))));
        let tampered = String::from_utf8_lossy(&manifest).replace("1.1.0", "9.9.9");
        assert_eq!(updater.check(tampered.as_bytes()), Err(UpdateError::BadSignature));
        let zero = signed("1.1.0", PayloadKind::Application, b"apk", 0);
        assert_eq!(updater.check(&zero), Ok(Decision::NotYetRolledOut));
        assert_eq!(installed("older", "1.2.0").check(&manifest), Ok(Decision::UpToDate));
        updater.pin(true).expect("pinned");
        assert_eq!(updater.check(&manifest), Ok(Decision::Pinned));
    }

    #[test]
    fn a_model_is_verified_staged_and_made_current() {
        let model = PayloadKind::Model { name: "words".into(), compatible: "1.0".into() };
        let envelope = signed("3", model, b"weights", 100);
        let updater = installed("model", "1.0.4");
        let Ok(Decision::Update(manifest)) = updater.check(&envelope) else { panic!("an update") };
        assert_eq!(updater.stage_model(&manifest, b"other"), Err(UpdateError::BadDigest));
        let staged = updater.stage_model(&manifest, b"weights").expect("staged");
        assert_eq!(std::fs::read(staged).ok().as_deref(), Some(&b"weights"[..]));
        assert_eq!(updater.check(&envelope), Ok(Decision::UpToDate));
        assert_eq!(installed("model2", "1.1.0").check(&envelope), Ok(Decision::Incompatible));
    }
}
