//! Persisted component state on disk, in the application's private files:
//! `Context.getFilesDir()/rustnative/state/`, in the crash-safe format of
//! `rustnative_core::services::FileStateStore`.

use std::path::{Path, PathBuf};

use rustnative_core::{ServiceError, StateStore};

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
pub struct FileStateStore(rustnative_core::services::FileStateStore);

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

    /// A store in `directory`, created if it does not exist.
    ///
    /// # Errors
    ///
    /// The directory could not be created or listed.
    pub fn in_directory(directory: impl Into<PathBuf>) -> Result<Self, ServiceError> {
        rustnative_core::services::FileStateStore::in_directory(directory).map(Self)
    }

    /// The directory this store writes to.
    #[must_use]
    pub fn directory(&self) -> &Path {
        self.0.directory()
    }
}

impl StateStore for FileStateStore {
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, ServiceError> {
        self.0.load(key)
    }

    fn save(&self, key: &str, value: &[u8]) -> Result<(), ServiceError> {
        self.0.save(key, value)
    }

    fn remove(&self, key: &str) -> Result<(), ServiceError> {
        self.0.remove(key)
    }
}
