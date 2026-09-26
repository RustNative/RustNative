//! Where persisted bytes are kept.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use crate::services::ServiceError;

/// Durable key-value storage for persisted state.
///
/// Synchronous, unlike [`crate::StorageService`]: state is read while a
/// component renders, which cannot wait. It is also read rarely (once per
/// key per run) and written in batches (see the module documentation), so
/// a synchronous store costs nothing an asynchronous one would save.
///
/// Keys are arbitrary UTF-8; a store maps them to its medium however it
/// likes (the Windows file store hex-encodes them into file names).
pub trait StateStore: Send + Sync {
    /// The bytes stored under `key`, or `None` if nothing is.
    ///
    /// # Errors
    ///
    /// The medium could not be read.
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, ServiceError>;

    /// Stores `value` under `key`, replacing what was there.
    ///
    /// # Errors
    ///
    /// The medium could not be written.
    fn save(&self, key: &str, value: &[u8]) -> Result<(), ServiceError>;

    /// Removes `key`, if it is stored.
    ///
    /// # Errors
    ///
    /// The medium could not be written.
    fn remove(&self, key: &str) -> Result<(), ServiceError>;
}

/// A [`StateStore`] in memory, shared by its clones.
///
/// Useful in tests — two `Application`s built one after the other with
/// clones of the same store see each other's state, exactly as two runs of
/// a program see the same files — and for applications that want the
/// persistence API without persistence.
///
/// # Example
///
/// ```
/// use rustnative_core::{MemoryStateStore, StateStore};
///
/// let store = MemoryStateStore::new();
/// let same = store.clone();
/// store.save("greeting", b"hello")?;
/// assert_eq!(same.load("greeting")?.as_deref(), Some(&b"hello"[..]));
/// # Ok::<(), rustnative_core::ServiceError>(())
/// ```
#[derive(Debug, Clone, Default)]
pub struct MemoryStateStore {
    values: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MemoryStateStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every stored key, sorted — for tests asserting what was written.
    #[must_use]
    pub fn keys(&self) -> Vec<String> {
        let mut keys = self
            .values
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        keys.sort();
        keys
    }
}

impl StateStore for MemoryStateStore {
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, ServiceError> {
        Ok(self.values.lock().unwrap_or_else(PoisonError::into_inner).get(key).cloned())
    }

    fn save(&self, key: &str, value: &[u8]) -> Result<(), ServiceError> {
        self.values
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.to_owned(), value.to_vec());
        Ok(())
    }

    fn remove(&self, key: &str) -> Result<(), ServiceError> {
        self.values.lock().unwrap_or_else(PoisonError::into_inner).remove(key);
        Ok(())
    }
}
