//! Deterministic in-memory service implementations for tests and previews.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use super::{ClipboardService, ServiceError, StorageService};

/// An in-process, non-persistent [`StorageService`].
#[derive(Debug, Default)]
pub struct MemoryStorage {
    values: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

#[async_trait::async_trait]
impl StorageService for MemoryStorage {
    async fn get(&self, key: String) -> Result<Option<Vec<u8>>, ServiceError> {
        Ok(self.values.lock().get(&key).cloned())
    }
    async fn set(&self, key: String, value: Vec<u8>) -> Result<(), ServiceError> {
        self.values.lock().insert(key, value);
        Ok(())
    }
    async fn remove(&self, key: String) -> Result<(), ServiceError> {
        self.values.lock().remove(&key);
        Ok(())
    }
}

/// An in-process, non-persistent [`ClipboardService`].
#[derive(Debug, Default)]
pub struct MemoryClipboard {
    value: Arc<Mutex<Option<String>>>,
}

#[async_trait::async_trait]
impl ClipboardService for MemoryClipboard {
    async fn read_text(&self) -> Result<Option<String>, ServiceError> {
        Ok(self.value.lock().clone())
    }
    async fn write_text(&self, text: String) -> Result<(), ServiceError> {
        *self.value.lock() = Some(text);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::block_on_for_test;

    #[test]
    fn memory_services_are_async_compatible_and_deterministic() {
        let storage = MemoryStorage::default();
        block_on_for_test(async {
            storage.set("k".into(), b"v".to_vec()).await.unwrap();
            assert_eq!(storage.get("k".into()).await.unwrap(), Some(b"v".to_vec()));
        });

        let clipboard = MemoryClipboard::default();
        block_on_for_test(async {
            clipboard.write_text("hello".into()).await.unwrap();
            assert_eq!(clipboard.read_text().await.unwrap(), Some("hello".to_owned()));
        });
    }
}
