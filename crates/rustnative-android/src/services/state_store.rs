//! Persisted component state on disk, in the application's private files.
//!
//! `Context.getFilesDir()/rustnative/state/` holds one file per key, named by a hash of the key and holding the key itself,
//! a checksum, and the value — the same format as the Windows and
//! Linux backends' stores, so the guarantees are the same:
//!
//! - a value is written to a temporary file, `fsync`ed, renamed over the
//!   real file (`rename(2)` replaces it atomically), and the directory is
//!   `fsync`ed so the rename itself survives a power loss;
//! - a crash leaves the old value or the new one, never a mixture;
//! - a torn or bit-flipped file reads as nothing stored, never as a
//!   shorter value; a hash collision reads as nothing stored.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use rustnative_core::{ServiceError, StateStore};

const EXTENSION: &str = "state";
const TEMPORARY: &str = "tmp";
const MAGIC: &[u8; 4] = b"RNS1";

/// A [`StateStore`] that keeps each key in its own crash-safe file.
///
/// ```no_run
/// # #[cfg(target_os = "android")]
/// # fn main() -> Result<(), rustnative_core::ServiceError> {
/// use std::sync::Arc;
///
/// use rustnative_core::Services;
/// use rustnative_android::FileStateStore;
///
/// let store = FileStateStore::for_application()?;
/// let services = Services::default().with_state_store(Arc::new(store));
/// # Ok(())
/// # }
/// # #[cfg(not(target_os = "android"))]
/// # fn main() {}
/// ```
#[derive(Debug, Clone)]
pub struct FileStateStore {
    directory: PathBuf,
}

impl FileStateStore {
    /// The application's store, in `getFilesDir()/rustnative/state` —
    /// private to the application, kept across updates, removed with it.
    ///
    /// # Errors
    ///
    /// The process has no Android context yet (call it from `main`), or the
    /// directory could not be created.
    #[cfg(target_os = "android")]
    pub fn for_application() -> Result<Self, ServiceError> {
        let files = super::files_dir()?;
        Self::in_directory(files.join("rustnative").join("state"))
    }

    /// A store in `directory`, created if it does not exist; any temporary
    /// file a crashed write left there is deleted.
    ///
    /// # Errors
    ///
    /// The directory could not be created or listed.
    pub fn in_directory(directory: impl Into<PathBuf>) -> Result<Self, ServiceError> {
        let directory = directory.into();
        fs::create_dir_all(&directory)
            .map_err(|error| io_error("create the state directory", &error))?;
        let entries = fs::read_dir(&directory)
            .map_err(|error| io_error("list the state directory", &error))?;
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|extension| extension == TEMPORARY) {
                // Best effort: a leftover is ignored by every read anyway.
                let _ = fs::remove_file(entry.path());
            }
        }
        Ok(Self { directory })
    }

    /// The directory this store writes to.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    fn path_for(&self, key: &str, extension: &str) -> PathBuf {
        self.directory.join(format!("{:016x}.{extension}", fnv1a(key.as_bytes())))
    }
}

impl StateStore for FileStateStore {
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, ServiceError> {
        match fs::read(self.path_for(key, EXTENSION)) {
            Ok(bytes) => Ok(decode(&bytes, key)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(io_error("read a state file", &error)),
        }
    }

    fn save(&self, key: &str, value: &[u8]) -> Result<(), ServiceError> {
        let temporary = self.path_for(key, TEMPORARY);
        let target = self.path_for(key, EXTENSION);
        {
            let mut file = fs::File::create(&temporary)
                .map_err(|error| io_error("create a state file", &error))?;
            file.write_all(&encode(key, value))
                .and_then(|()| file.sync_all())
                .map_err(|error| io_error("write a state file", &error))?;
        }
        if let Err(error) = fs::rename(&temporary, &target) {
            let _ = fs::remove_file(&temporary);
            return Err(io_error("commit a state file", &error));
        }
        // The rename is durable only once the directory entry is.
        fs::File::open(&self.directory)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| io_error("flush the state directory", &error))
    }

    fn remove(&self, key: &str) -> Result<(), ServiceError> {
        match fs::remove_file(self.path_for(key, EXTENSION)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_error("remove a state file", &error)),
        }
    }
}

fn encode(key: &str, value: &[u8]) -> Vec<u8> {
    let length = u32::try_from(key.len()).unwrap_or(u32::MAX);
    let mut body = Vec::with_capacity(4 + key.len() + value.len());
    body.extend_from_slice(&length.to_le_bytes());
    body.extend_from_slice(key.as_bytes());
    body.extend_from_slice(value);
    let mut bytes = Vec::with_capacity(12 + body.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&fnv1a(&body).to_le_bytes());
    bytes.extend_from_slice(&body);
    bytes
}

fn decode(bytes: &[u8], key: &str) -> Option<Vec<u8>> {
    let rest = bytes.strip_prefix(MAGIC)?;
    let checksum = u64::from_le_bytes(rest.get(..8)?.try_into().ok()?);
    let body = &rest[8..];
    if fnv1a(body) != checksum {
        return None;
    }
    let length = usize::try_from(u32::from_le_bytes(body.get(..4)?.try_into().ok()?)).ok()?;
    let stored = body.get(4..4 + length)?;
    (stored == key.as_bytes()).then(|| body[4 + length..].to_vec())
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn io_error(what: &str, error: &std::io::Error) -> ServiceError {
    ServiceError::new(format!("could not {what}: {error}"))
}

// Windows refuses to open a directory to flush it, so the tests run where
// the store does: on a Unix (Android, or a Linux host).
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir()
            .join(format!("rustnative-android-state-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        directory
    }

    #[test]
    fn a_saved_value_loads_back_and_removing_it_forgets_it() {
        let store = FileStateStore::in_directory(scratch("round-trip")).unwrap();
        assert_eq!(store.load("app::Root/list#scroll").unwrap(), None);
        store.save("app::Root/list#scroll", b"120").unwrap();
        assert_eq!(store.load("app::Root/list#scroll").unwrap().as_deref(), Some(&b"120"[..]));
        store.save("app::Root/list#scroll", b"240").unwrap();
        assert_eq!(store.load("app::Root/list#scroll").unwrap().as_deref(), Some(&b"240"[..]));
        store.remove("app::Root/list#scroll").unwrap();
        assert_eq!(store.load("app::Root/list#scroll").unwrap(), None);
    }

    #[test]
    fn a_torn_file_or_a_crash_leftover_never_reads_as_a_value() {
        let directory = scratch("torn");
        let store = FileStateStore::in_directory(&directory).unwrap();
        store.save("key", b"the whole value").unwrap();
        let path = store.path_for("key", EXTENSION);
        let whole = fs::read(&path).unwrap();
        for cut in 0..whole.len() {
            fs::write(&path, &whole[..cut]).unwrap();
            assert_eq!(store.load("key").unwrap(), None, "cut at {cut}");
        }
        fs::write(&path, &whole).unwrap();
        let debris = store.path_for("key", TEMPORARY);
        fs::write(&debris, b"half").unwrap();
        let reopened = FileStateStore::in_directory(&directory).unwrap();
        assert!(!debris.exists(), "the leftover was cleaned up");
        assert_eq!(reopened.load("key").unwrap().as_deref(), Some(&b"the whole value"[..]));
        fs::write(&path, encode("other", b"not yours")).unwrap();
        assert_eq!(store.load("key").unwrap(), None, "a collision reads as nothing");
    }
}
