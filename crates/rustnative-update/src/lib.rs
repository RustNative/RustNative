//! The signed update manifest every self-updating host shares (`PLAN.md`
//! Milestone 50; `docs/deploy/update-rules.md`): its format, its verifier,
//! and the update decision — rollout bucket, pinning, payload
//! compatibility, version order. Each backend keeps only how its host
//! installs a staged version (`rustnative_windows::update`,
//! `rustnative_android::update`).
//!
//! A published manifest is an envelope:
//!
//! ```json
//! {"manifest": "{\"app\":\"dev.example.notes\",\"version\":\"1.1.0\",…}", "signature": "<hex>"}
//! ```
//!
//! The signature is Ed25519 over the `manifest` string's bytes exactly as
//! published, and is checked before those bytes are parsed. Nothing is
//! re-serialized to be verified, so a field added by a later publisher is
//! carried by the signature and ignored by an older reader rather than
//! breaking it. A verified manifest is then refused when it names another
//! application or has expired: a stale manifest served forever (a freeze
//! attack) stops being believed at its `expires` time.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier as _, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

/// What an update carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PayloadKind {
    /// A new version of the application.
    Application,
    /// A model or data file for the application.
    Model {
        /// Its name.
        name: String,
        /// The application versions it works with (`1.2`: every `1.2.x`).
        compatible: String,
    },
}

/// A description of an update, as signed by its publisher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateManifest {
    /// The application's id; an installation of any other refuses it.
    pub app: String,
    /// The version offered (`1.2.3`, or a pre-release such as `1.3.0-beta.1`).
    pub version: String,
    /// Where its package is.
    pub url: String,
    /// The package's SHA-256, hex.
    pub sha256: String,
    /// The share of installations offered it, 0–100.
    pub rollout: u8,
    /// What it carries.
    pub payload: PayloadKind,
    /// When it stops being believed, in seconds since the Unix epoch.
    pub expires: u64,
}

/// The published form: the manifest's exact bytes and a detached
/// signature over them.
#[derive(Debug, Serialize, Deserialize)]
struct Envelope {
    manifest: String,
    signature: String,
}

impl UpdateManifest {
    /// The published envelope for this manifest, signed with the
    /// publisher's 32-byte secret key (what `rustnative update manifest`
    /// prints).
    #[must_use]
    pub fn sign(&self, secret: &[u8; 32]) -> Vec<u8> {
        let manifest = serde_json::to_string(self).unwrap_or_default();
        let signature = hex(&SigningKey::from_bytes(secret).sign(manifest.as_bytes()).to_bytes());
        serde_json::to_vec_pretty(&Envelope { manifest, signature }).unwrap_or_default()
    }

    /// Reads a published envelope: its signature is checked against `key`
    /// over the manifest's exact bytes, and only then is it parsed.
    ///
    /// # Errors
    ///
    /// The envelope or manifest is malformed, or the signature is not the
    /// key's.
    pub fn open(envelope: &[u8], key: &VerifyingKey) -> Result<Self, UpdateError> {
        let envelope: Envelope = serde_json::from_slice(envelope)
            .map_err(|error| UpdateError::BadManifest(error.to_string()))?;
        let signature = unhex(&envelope.signature)
            .and_then(|bytes| Signature::from_slice(&bytes).ok())
            .ok_or(UpdateError::BadSignature)?;
        key.verify(envelope.manifest.as_bytes(), &signature)
            .map_err(|_| UpdateError::BadSignature)?;
        serde_json::from_str(&envelope.manifest)
            .map_err(|error| UpdateError::BadManifest(error.to_string()))
    }

    /// Whether `bytes` are the package this manifest describes.
    ///
    /// # Errors
    ///
    /// [`UpdateError::BadDigest`] when they are not.
    pub fn verify_package(&self, bytes: &[u8]) -> Result<(), UpdateError> {
        if hex(&sha2::Sha256::digest(bytes)) == self.sha256.to_ascii_lowercase() {
            Ok(())
        } else {
            Err(UpdateError::BadDigest)
        }
    }
}

/// The public key for a secret key, hex (to put in `rustnative.toml`).
#[must_use]
pub fn public_key(secret: &[u8; 32]) -> String {
    hex(SigningKey::from_bytes(secret).verifying_key().as_bytes())
}

/// A package's SHA-256, hex, as a manifest names it.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&sha2::Sha256::digest(bytes))
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

/// Why an update was not taken or applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    /// The manifest's signature is not the pinned key's (or the pinned key
    /// is not a key).
    BadSignature,
    /// The package's digest does not match the manifest.
    BadDigest,
    /// The manifest could not be read.
    BadManifest(String),
    /// The manifest is for another application.
    WrongApplication {
        /// This installation's application.
        expected: String,
        /// The one the manifest names.
        found: String,
    },
    /// The manifest is past its `expires` time.
    Expired,
    /// The package is not a readable archive.
    BadPackage(String),
    /// This installation may not update its own code (on Android, it came
    /// from Google Play, which updates it).
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
            Self::WrongApplication { expected, found } => {
                write!(formatter, "the update is for {found}, not {expected}")
            }
            Self::Expired => formatter.write_str("the update manifest has expired"),
            Self::BadPackage(why) => write!(formatter, "the package is unreadable: {why}"),
            Self::Forbidden => formatter.write_str(
                "this installation is updated by its store: an application may not update its own code",
            ),
            Self::Io(why) => write!(formatter, "{why}"),
        }
    }
}

impl std::error::Error for UpdateError {}

#[allow(clippy::needless_pass_by_value, reason = "used as `map_err(io)`")]
fn io(error: std::io::Error) -> UpdateError {
    UpdateError::Io(error.to_string())
}

/// What to do about a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Nothing newer.
    UpToDate,
    /// Newer, but this installation's bucket is not in the rollout yet.
    NotYetRolledOut,
    /// The installation is pinned to its version.
    Pinned,
    /// A model or data payload for application versions other than this
    /// one; it is never activated.
    Incompatible,
    /// Take it.
    Update(UpdateManifest),
}

/// Whether `candidate` is a later version than `current`: dotted numbers
/// compared numerically (`1.2` is `1.2.0`), a release after its
/// pre-releases (`1.3.0-beta.2` < `1.3.0`), pre-release identifiers in
/// Semantic Versioning order, and build metadata (`+…`) ignored.
#[must_use]
pub fn newer(candidate: &str, current: &str) -> bool {
    compare_versions(candidate, current) == std::cmp::Ordering::Greater
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let split = |version: &str| {
        let version = version.split('+').next().unwrap_or_default();
        let (core, pre) = version.split_once('-').map_or((version, None), |(c, p)| (c, Some(p)));
        let numbers: Vec<u64> = core.split('.').map(|part| part.parse().unwrap_or(0)).collect();
        (numbers, pre.map(str::to_owned))
    };
    let ((mut a, a_pre), (mut b, b_pre)) = (split(left), split(right));
    let length = a.len().max(b.len());
    a.resize(length, 0);
    b.resize(length, 0);
    match a.cmp(&b) {
        Ordering::Equal => {}
        unequal => return unequal,
    }
    match (a_pre, b_pre) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => {
            let identifier = |part: &str| part.parse::<u64>().map_err(|_| part.to_owned());
            let a: Vec<_> = a.split('.').map(identifier).collect();
            let b: Vec<_> = b.split('.').map(identifier).collect();
            // Numeric identifiers sort before alphanumeric ones (`Ok` before
            // `Err`), and a shorter list before a longer one it prefixes.
            a.cmp(&b)
        }
    }
}

/// One installation's update records and decisions: its pinned key, its
/// application id and running version, and a directory for its rollout
/// bucket, pin, and installed models.
#[derive(Debug)]
pub struct Installation {
    key: VerifyingKey,
    root: PathBuf,
    app: String,
    current: String,
}

impl Installation {
    /// The installation of application `app` at `root`, running
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
        let bytes: [u8; 32] = unhex(public_key)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(UpdateError::BadSignature)?;
        let key = VerifyingKey::from_bytes(&bytes).map_err(|_| UpdateError::BadSignature)?;
        Ok(Self { key, root: root.into(), app: app.to_owned(), current: current.to_owned() })
    }

    /// Where the records live.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The running version.
    #[must_use]
    pub fn current(&self) -> &str {
        &self.current
    }

    /// This installation's stable rollout bucket, 0–99 (chosen at random
    /// the first time and kept).
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

    /// The directory a model's versions are kept in.
    #[must_use]
    pub fn model_directory(&self, name: &str) -> PathBuf {
        self.root.join("models").join(name)
    }

    /// Decides about a published manifest, now.
    ///
    /// # Errors
    ///
    /// A malformed, unsigned, expired, or other application's manifest.
    pub fn check(&self, envelope: &[u8]) -> Result<Decision, UpdateError> {
        self.check_at(envelope, SystemTime::now())
    }

    /// Decides about a published manifest at time `now`.
    ///
    /// # Errors
    ///
    /// As [`Self::check`].
    pub fn check_at(&self, envelope: &[u8], now: SystemTime) -> Result<Decision, UpdateError> {
        let manifest = UpdateManifest::open(envelope, &self.key)?;
        if manifest.app != self.app {
            return Err(UpdateError::WrongApplication {
                expected: self.app.clone(),
                found: manifest.app,
            });
        }
        let now = now.duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
        if now >= manifest.expires {
            return Err(UpdateError::Expired);
        }
        let installed = match &manifest.payload {
            PayloadKind::Application => self.current.clone(),
            PayloadKind::Model { name, compatible } => {
                // `1.2` matches `1.2` and every `1.2.x`, never `1.20`.
                let fits = self.current == *compatible
                    || self.current.starts_with(&format!("{compatible}."));
                if !fits {
                    return Ok(Decision::Incompatible);
                }
                std::fs::read_to_string(self.model_directory(name).join("current"))
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
}

/// Writes `contents` to `path` through a rename, so a reader sees the old
/// contents or the new, never half.
///
/// # Errors
///
/// The file cannot be written or renamed.
pub fn write_atomically(path: &Path, contents: &[u8]) -> Result<(), UpdateError> {
    let temporary = path.with_extension("new");
    std::fs::write(&temporary, contents).map_err(io)?;
    std::fs::rename(&temporary, path).map_err(io)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const SECRET: [u8; 32] = [7; 32];
    const NOW: u64 = 1_800_000_000;

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn manifest(version: &str, rollout: u8) -> UpdateManifest {
        UpdateManifest {
            app: "dev.example.notes".into(),
            version: version.into(),
            url: "https://example.com/notes.zip".into(),
            sha256: sha256_hex(b"package"),
            rollout,
            payload: PayloadKind::Application,
            expires: NOW + 3600,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("rustnative-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        directory
    }

    fn installation(name: &str, current: &str) -> Installation {
        Installation::new(scratch(name), "dev.example.notes", &public_key(&SECRET), current)
            .unwrap()
    }

    #[test]
    fn the_signature_covers_the_published_bytes_not_a_reserialization() {
        let signed = manifest("1.1.0", 100).sign(&SECRET);
        let here = installation("bytes", "1.0.0");
        assert!(matches!(here.check_at(&signed, at(NOW)), Ok(Decision::Update(_))));

        // A field a later publisher adds is signed with the rest and
        // ignored by this reader.
        let mut envelope: serde_json::Value = serde_json::from_slice(&signed).unwrap();
        let later =
            envelope["manifest"].as_str().unwrap().replacen('{', r#"{"channel":"beta","#, 1);
        let signature = hex(&SigningKey::from_bytes(&SECRET).sign(later.as_bytes()).to_bytes());
        envelope = serde_json::json!({ "manifest": later, "signature": signature });
        assert!(matches!(
            here.check_at(&serde_json::to_vec(&envelope).unwrap(), at(NOW)),
            Ok(Decision::Update(_))
        ));

        // Any change to the bytes — even one that parses the same — breaks it.
        envelope["manifest"] = serde_json::Value::String(format!("{later} "));
        assert_eq!(
            here.check_at(&serde_json::to_vec(&envelope).unwrap(), at(NOW)),
            Err(UpdateError::BadSignature)
        );
    }

    #[test]
    fn another_applications_or_an_expired_manifest_is_refused() {
        let here = installation("refused", "1.0.0");
        let mut other = manifest("1.1.0", 100);
        other.app = "dev.example.other".into();
        assert!(matches!(
            here.check_at(&other.sign(&SECRET), at(NOW)),
            Err(UpdateError::WrongApplication { .. })
        ));
        let signed = manifest("1.1.0", 100).sign(&SECRET);
        assert_eq!(here.check_at(&signed, at(NOW + 3600)), Err(UpdateError::Expired));
    }

    #[test]
    fn rollout_pinning_and_models_decide() {
        let here = installation("decide", "1.0.0");
        assert_eq!(
            here.check_at(&manifest("1.1.0", 0).sign(&SECRET), at(NOW)),
            Ok(Decision::NotYetRolledOut)
        );
        assert_eq!(
            here.check_at(&manifest("1.0.0", 100).sign(&SECRET), at(NOW)),
            Ok(Decision::UpToDate)
        );
        here.pin(true).unwrap();
        assert_eq!(
            here.check_at(&manifest("1.1.0", 100).sign(&SECRET), at(NOW)),
            Ok(Decision::Pinned)
        );
        here.pin(false).unwrap();
        let mut model = manifest("2", 100);
        model.payload = PayloadKind::Model { name: "ranker".into(), compatible: "1.0".into() };
        assert!(matches!(here.check_at(&model.sign(&SECRET), at(NOW)), Ok(Decision::Update(_))));
        let elsewhere = installation("decide-elsewhere", "1.10.0");
        model.payload = PayloadKind::Model { name: "ranker".into(), compatible: "1.1".into() };
        assert_eq!(elsewhere.check_at(&model.sign(&SECRET), at(NOW)), Ok(Decision::Incompatible));
        assert_eq!(model.verify_package(b"other"), Err(UpdateError::BadDigest));
        assert_eq!(model.verify_package(b"package"), Ok(()));
    }

    #[test]
    fn versions_order_releases_after_their_pre_releases() {
        assert!(newer("1.2.3", "1.2.3-beta"), "a release follows its pre-release");
        assert!(newer("1.2.3-beta", "1.2.0"), "a pre-release of 1.2.3 is after 1.2.0");
        assert!(newer("1.2.3-beta.2", "1.2.3-beta.1"));
        assert!(newer("1.2.3-beta.11", "1.2.3-beta.2"), "numeric identifiers compare as numbers");
        assert!(newer("1.2.3-beta", "1.2.3-alpha.9"));
        assert!(newer("1.2.3-alpha.1", "1.2.3-alpha"), "a longer list follows its prefix");
        assert!(newer("1.2.3-rc", "1.2.3-1"), "alphanumeric follows numeric");
        assert!(!newer("1.2", "1.2.0"), "1.2 is 1.2.0");
        assert!(!newer("1.2.0+build.9", "1.2.0"), "build metadata is ignored");
        assert!(newer("1.10.0", "1.9.9"));
    }
}
