//! Persisted component state on disk, under the user's local app-data
//! folder.
//!
//! # Layout
//!
//! `%LOCALAPPDATA%\<app-id>\state\` holds one file per key. File names are
//! a 64-bit FNV-1a hash of the key, because keys are key paths — arbitrarily
//! long, and containing characters no file name may — and each file begins
//! with the full key it holds, which [`FileStateStore::load`] checks, so a
//! hash collision reads as "nothing stored" rather than as another key's
//! value.
//!
//! # Crash safety
//!
//! A value is written to a temporary file in the same folder, flushed to
//! the disk, and only then moved over the real file with
//! `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`, which
//! replaces it in one step. A crash or power loss at any moment leaves
//! either the old value or the new one — never a half-written file. A
//! temporary file a crash left behind is ignored by reads and deleted the
//! next time the store is opened.
//!
//! # Power loss (`C80`)
//!
//! The atomic replace is only as good as the disk's promise to honour
//! write-through. A disk that loses power with a sector half written can
//! still hand back a committed file cut short, so each file ends with a
//! checksum of what it holds: a torn file reads as nothing stored, never
//! as a truncated value. Files written before the checksum existed are
//! still read. The checksum leads the file, so a file cut short can never
//! pass for one of those.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use rustnative_core::{ServiceError, StateStore};
use windows_sys::Win32::Storage::FileSystem::{
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

/// The extension of a committed state file.
const EXTENSION: &str = "state";
/// The extension of a write in progress.
const TEMPORARY: &str = "tmp";

/// A [`StateStore`] that keeps each key in its own crash-safe file.
///
/// # Example
///
/// ```no_run
/// use std::sync::Arc;
///
/// use rustnative_core::Services;
/// use rustnative_windows::FileStateStore;
///
/// let store = FileStateStore::for_app("com.example.notes")?;
/// let services = Services::default().with_state_store(Arc::new(store));
/// # Ok::<(), rustnative_core::ServiceError>(())
/// ```
#[derive(Debug, Clone)]
pub struct FileStateStore {
    directory: PathBuf,
}

impl FileStateStore {
    /// The store for application `app_id`, in
    /// `%LOCALAPPDATA%\<app-id>\state\`.
    ///
    /// `app_id` is a stable name for the application, conventionally a
    /// reverse domain name (`com.example.notes`): it must be a valid folder
    /// name — no path separators, no characters Windows reserves.
    ///
    /// # Errors
    ///
    /// `app_id` is not a usable folder name, `LOCALAPPDATA` is not set, or
    /// the folder could not be created.
    pub fn for_app(app_id: &str) -> Result<Self, ServiceError> {
        let valid = !app_id.is_empty()
            && app_id != "."
            && app_id != ".."
            && !app_id.chars().any(|character| {
                character.is_control()
                    || matches!(character, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
            });
        if !valid {
            return Err(ServiceError::new(format!("`{app_id}` is not a usable application id")));
        }
        let base = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| ServiceError::new("LOCALAPPDATA is not set"))?;
        Self::in_directory(Path::new(&base).join(app_id).join("state"))
    }

    /// A store in `directory`, created if it does not exist.
    ///
    /// Any temporary file a crashed write left there is deleted.
    ///
    /// # Errors
    ///
    /// The folder could not be created or listed.
    pub fn in_directory(directory: impl Into<PathBuf>) -> Result<Self, ServiceError> {
        let directory = directory.into();
        fs::create_dir_all(&directory)
            .map_err(|error| io_error("create the state folder", &error))?;
        let entries =
            fs::read_dir(&directory).map_err(|error| io_error("list the state folder", &error))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == TEMPORARY) {
                // Best effort: a leftover that cannot be deleted now is
                // still ignored by every read, and tried again next open.
                let _ = fs::remove_file(&path);
            }
        }
        Ok(Self { directory })
    }

    /// The folder this store writes to.
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
        let bytes = match fs::read(self.path_for(key, EXTENSION)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error("read a state file", &error)),
        };
        Ok(decode(&bytes, key))
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
        let from = wide_path(&temporary);
        let to = wide_path(&target);
        // SAFETY: both paths are NUL-terminated wide strings that live for
        // the call.
        let moved = unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } != 0;
        if moved {
            Ok(())
        } else {
            let error = std::io::Error::last_os_error();
            let _ = fs::remove_file(&temporary);
            Err(io_error("commit a state file", &error))
        }
    }

    fn remove(&self, key: &str) -> Result<(), ServiceError> {
        match fs::remove_file(self.path_for(key, EXTENSION)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_error("remove a state file", &error)),
        }
    }
}

/// The marker starting a checksummed file. No file from before the
/// checksum starts with it: there, the first four bytes are the key's
/// length, and a key is never 827 MB long.
const MAGIC: &[u8; 4] = b"RNS1";

/// A file's contents: [`MAGIC`], the FNV-1a of the rest, then the key's
/// length (little-endian `u32`), the key, and the value.
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

/// The value in `bytes` if the file holds `key`; `None` for a file that is
/// another key's (a hash collision), torn, or not a state file at all.
fn decode(bytes: &[u8], key: &str) -> Option<Vec<u8>> {
    let body = if let Some(rest) = bytes.strip_prefix(MAGIC) {
        // The checksum covers everything after it, so a file cut short or
        // a flipped bit anywhere reads as nothing stored.
        let checksum = u64::from_le_bytes(rest.get(..8)?.try_into().ok()?);
        let body = &rest[8..];
        if fnv1a(body) != checksum {
            return None;
        }
        body
    } else {
        // A file from before the checksum.
        bytes
    };
    let length = usize::try_from(u32::from_le_bytes(body.get(..4)?.try_into().ok()?)).ok()?;
    let stored = body.get(4..4 + length)?;
    (stored == key.as_bytes()).then(|| body[4 + length..].to_vec())
}

/// 64-bit FNV-1a: small, stable across Rust versions (unlike `std`'s
/// hasher), and plenty for a handful of file names checked on read.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt as _;
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

fn io_error(what: &str, error: &std::io::Error) -> ServiceError {
    ServiceError::new(format!("could not {what}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, empty folder under the system temp directory.
    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("rust-native-state-{name}-{}", std::process::id()));
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
        store.remove("app::Root/list#scroll").unwrap();
    }

    #[test]
    fn a_crash_mid_write_leaves_the_previous_value_and_its_debris_is_cleaned_up() {
        let directory = scratch("crash");
        let store = FileStateStore::in_directory(&directory).unwrap();
        store.save("key", b"committed").unwrap();

        // What a crash between writing the temporary file and moving it
        // into place leaves behind: a half-written temporary file.
        let debris = store.path_for("key", TEMPORARY);
        fs::write(&debris, b"\x03\x00\x00\x00keyhalf-writ").unwrap();
        assert_eq!(
            store.load("key").unwrap().as_deref(),
            Some(&b"committed"[..]),
            "reads never see a write that was not committed"
        );

        // The next run's store cleans up and still reads the old value.
        let reopened = FileStateStore::in_directory(&directory).unwrap();
        assert!(!debris.exists(), "the leftover temporary file was deleted");
        assert_eq!(reopened.load("key").unwrap().as_deref(), Some(&b"committed"[..]));
    }

    // req: C80-1
    #[test]
    fn a_torn_file_reads_as_nothing_never_as_a_shorter_value() {
        let store = FileStateStore::in_directory(scratch("torn")).unwrap();
        store.save("key", b"the whole value").unwrap();
        let path = store.path_for("key", EXTENSION);
        let whole = fs::read(&path).unwrap();
        for cut in 0..whole.len() {
            fs::write(&path, &whole[..cut]).unwrap();
            assert_eq!(store.load("key").unwrap(), None, "cut at {cut} of {}", whole.len());
        }
        let mut flipped = whole.clone();
        flipped[6] ^= 1;
        fs::write(&path, flipped).unwrap();
        assert_eq!(store.load("key").unwrap(), None, "a flipped bit is caught");
        fs::write(&path, b"\x03\x00\x00\x00keylegacy").unwrap();
        assert_eq!(
            store.load("key").unwrap().as_deref(),
            Some(&b"legacy"[..]),
            "old files still read"
        );
    }

    /// The child side of the power-cut test: saves ever-longer values
    /// until it is killed.
    #[test]
    fn power_cut_child() {
        let Some(directory) = std::env::var_os("RUSTNATIVE_POWER_CUT_DIR") else { return };
        let store = FileStateStore::in_directory(PathBuf::from(directory)).unwrap();
        for round in 0..u32::MAX {
            let value =
                format!("{round}|{}", "x".repeat(usize::try_from(round % 4096).unwrap_or(0)));
            store.save("key", value.as_bytes()).unwrap();
        }
    }

    // req: C80-1
    #[test]
    fn a_process_killed_mid_save_leaves_a_whole_value() {
        let directory = scratch("power-cut");
        for attempt in 0..12u64 {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "services::state_store::tests::power_cut_child", "--nocapture"])
                .env("RUSTNATIVE_POWER_CUT_DIR", &directory)
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(150 + attempt * 37));
            child.kill().unwrap();
            let _ = child.wait();
            let store = FileStateStore::in_directory(&directory).unwrap();
            if let Some(value) = store.load("key").unwrap() {
                let text = String::from_utf8(value).unwrap();
                let (round, filler) = text.split_once('|').unwrap();
                let round: u32 = round.parse().unwrap();
                assert_eq!(
                    filler.len(),
                    usize::try_from(round % 4096).unwrap(),
                    "attempt {attempt}: a whole value"
                );
            }
        }
    }

    #[test]
    fn a_file_holding_another_key_reads_as_nothing_stored() {
        let store = FileStateStore::in_directory(scratch("collision")).unwrap();
        // Forge a collision: write "other"'s contents under "key"'s name.
        fs::write(store.path_for("key", EXTENSION), encode("other", b"not yours")).unwrap();
        assert_eq!(store.load("key").unwrap(), None);
    }

    #[test]
    fn long_keys_with_any_characters_are_stored() {
        let store = FileStateStore::in_directory(scratch("long")).unwrap();
        let key = format!("{}/<:\"|?*>\u{2713}", "segment/".repeat(80));
        store.save(&key, b"ok").unwrap();
        assert_eq!(store.load(&key).unwrap().as_deref(), Some(&b"ok"[..]));
    }

    #[test]
    fn unusable_application_ids_are_refused() {
        for id in ["", "..", "a/b", "a\\b", "c:", "what?"] {
            assert!(FileStateStore::for_app(id).is_err(), "{id:?} must be refused");
        }
    }
}
