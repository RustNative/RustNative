//! The write buffer between components and a [`StateStore`], and the typed
//! handle a component reads and writes through.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::store::StateStore;
use crate::services::ServiceError;

/// One window's buffered view of the state store: what has been read (so a
/// key is loaded from the medium once per run), and what has been written
/// and not yet flushed.
#[derive(Default)]
pub(crate) struct StateCache {
    store: Option<Arc<dyn StateStore>>,
    values: HashMap<String, Option<Vec<u8>>>,
    dirty: BTreeSet<String>,
}

impl fmt::Debug for StateCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StateCache")
            .field("store", &self.store.is_some())
            .field("cached", &self.values.len())
            .field("dirty", &self.dirty)
            .finish()
    }
}

impl StateCache {
    pub(crate) fn new(store: Option<Arc<dyn StateStore>>) -> Self {
        Self { store, values: HashMap::new(), dirty: BTreeSet::new() }
    }

    /// The bytes under `key`: the buffered value if there is one, otherwise
    /// the store's (read once, then remembered).
    fn read(&mut self, key: &str) -> Option<Vec<u8>> {
        if let Some(value) = self.values.get(key) {
            return value.clone();
        }
        // A store that cannot be read behaves like an empty one: the
        // component gets its default, which is what it would get on a
        // first run, rather than failing to render.
        let loaded = self.store.as_ref().and_then(|store| store.load(key).ok().flatten());
        self.values.insert(key.to_owned(), loaded.clone());
        loaded
    }

    fn write(&mut self, key: &str, value: Option<Vec<u8>>) {
        if self.values.get(key) == Some(&value) {
            return;
        }
        self.values.insert(key.to_owned(), value);
        self.dirty.insert(key.to_owned());
    }

    /// Whether there are writes not yet in the store.
    pub(crate) fn is_dirty(&self) -> bool {
        !self.dirty.is_empty()
    }

    /// Writes every buffered change to the store.
    ///
    /// A key whose write fails stays buffered, so the next flush tries it
    /// again; the first error is reported after every other key has been
    /// attempted, so one bad key does not hold back the rest.
    pub(crate) fn flush(&mut self) -> Result<(), ServiceError> {
        let Some(store) = self.store.clone() else {
            // Nowhere to write to: the buffer *is* the storage.
            self.dirty.clear();
            return Ok(());
        };
        let mut first_error = None;
        for key in std::mem::take(&mut self.dirty) {
            let outcome = match self.values.get(&key) {
                Some(Some(value)) => store.save(&key, value),
                Some(None) | None => store.remove(&key),
            };
            if let Err(error) = outcome {
                self.dirty.insert(key);
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

/// A value a component keeps across runs (see [`crate::persistence`]).
///
/// Obtained from [`crate::ComponentContext::persisted`] on each render; the
/// handle is cheap and may be stored in the component for use in `update`.
///
/// # Example
///
/// ```
/// use std::sync::Arc;
///
/// use rustnative_core::{
///     Application, Component, ComponentContext, Event, MemoryStateStore, Node, NodeId,
///     Persisted, Services, Size, Window,
/// };
///
/// struct Counter {
///     count: Option<Persisted<u32>>,
/// }
///
/// impl Component for Counter {
///     type Props = ();
///     type Message = ();
///     fn new((): ()) -> Self { Self { count: None } }
///     fn props(&self) -> &() { &() }
///     fn set_props(&mut self, (): ()) {}
///     fn view(&self) -> Node { Node::label("unused", "") }
///
///     fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
///         let count = context.persisted("count", 0u32);
///         let label = Node::button("add", format!("Clicked {} times", count.get()));
///         self.count = Some(count);
///         label
///     }
///
///     fn update(&mut self, event: Event) {
///         if let (Event::Click { .. }, Some(count)) = (event, &self.count) {
///             count.set(&(count.get() + 1));
///         }
///     }
/// }
///
/// let store = MemoryStateStore::new();
/// let run = |store: &MemoryStateStore| {
///     let services = Services::default().with_state_store(Arc::new(store.clone()));
///     Application::with_services(Counter::new(()), Window::new("c", Size::new(200, 80)), services)
/// };
///
/// let mut first = run(&store);
/// first.dispatch(Event::Click { target: NodeId::from_key("add") });
/// first.flush_state()?; // what the backend does before exiting
/// drop(first);
///
/// // A second run starts where the first left off.
/// let second = run(&store);
/// let Node::Button(button) = second.view() else { panic!("a button") };
/// assert_eq!(button.text(), "Clicked 1 times");
///
/// // The restored button, in markup:
/// assert_eq!(rustnative_core::rsx! { <Button key="add" text="Clicked 1 times" /> }, second.view());
/// # Ok::<(), rustnative_core::ServiceError>(())
/// ```
pub struct Persisted<T> {
    key: String,
    default: T,
    cache: Rc<RefCell<StateCache>>,
}

impl<T: Clone> Clone for Persisted<T> {
    fn clone(&self) -> Self {
        Self { key: self.key.clone(), default: self.default.clone(), cache: Rc::clone(&self.cache) }
    }
}

impl<T: fmt::Debug> fmt::Debug for Persisted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Persisted")
            .field("key", &self.key)
            .field("default", &self.default)
            .finish_non_exhaustive()
    }
}

impl<T> PartialEq for Persisted<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && Rc::ptr_eq(&self.cache, &other.cache)
    }
}

impl<T: Serialize + DeserializeOwned + Clone> Persisted<T> {
    pub(crate) fn new(key: String, default: T, cache: Rc<RefCell<StateCache>>) -> Self {
        Self { key, default, cache }
    }

    /// The full key this value is stored under: the owning component's key
    /// path, then the key it chose.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The current value, or the default if none was ever saved (or what
    /// was saved no longer deserializes as `T`).
    #[must_use]
    pub fn get(&self) -> T {
        self.cache
            .borrow_mut()
            .read(&self.key)
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| self.default.clone())
    }

    /// Replaces the value. Buffered: written to the store at the next
    /// flush, not now.
    ///
    /// A value that cannot be serialized (a map with non-string keys, say)
    /// is not stored; the previous value stays.
    pub fn set(&self, value: &T) {
        if let Ok(bytes) = serde_json::to_vec(value) {
            self.cache.borrow_mut().write(&self.key, Some(bytes));
        }
    }

    /// Changes the value in place.
    pub fn update(&self, change: impl FnOnce(&mut T)) {
        let mut value = self.get();
        change(&mut value);
        self.set(&value);
    }

    /// Forgets the saved value, so the default applies again.
    pub fn clear(&self) {
        self.cache.borrow_mut().write(&self.key, None);
    }
}

#[cfg(test)]
mod tests {
    use super::super::store::MemoryStateStore;
    use super::*;

    fn cache(store: &MemoryStateStore) -> Rc<RefCell<StateCache>> {
        Rc::new(RefCell::new(StateCache::new(Some(Arc::new(store.clone())))))
    }

    #[test]
    fn writes_are_buffered_until_flushed() {
        let store = MemoryStateStore::new();
        let value = Persisted::new("a/count".to_owned(), 0u32, cache(&store));
        value.set(&5);
        assert_eq!(value.get(), 5, "the buffer answers reads");
        assert!(store.keys().is_empty(), "nothing reached the store yet");
        value.cache.borrow_mut().flush().unwrap();
        assert_eq!(store.load("a/count").unwrap().as_deref(), Some(&b"5"[..]));
    }

    #[test]
    fn a_value_that_no_longer_deserializes_reads_as_the_default() {
        let store = MemoryStateStore::new();
        store.save("a/name", b"\"a string, not a number\"").unwrap();
        let value = Persisted::new("a/name".to_owned(), 7u32, cache(&store));
        assert_eq!(value.get(), 7);
    }

    #[test]
    fn clearing_removes_the_key_from_the_store() {
        let store = MemoryStateStore::new();
        store.save("a/x", b"1").unwrap();
        let value = Persisted::new("a/x".to_owned(), 0u8, cache(&store));
        value.clear();
        assert_eq!(value.get(), 0);
        value.cache.borrow_mut().flush().unwrap();
        assert_eq!(store.keys(), [] as [&str; 0]);
    }

    #[test]
    fn writing_what_is_already_there_is_not_a_change() {
        let store = MemoryStateStore::new();
        store.save("a/x", b"3").unwrap();
        let value = Persisted::new("a/x".to_owned(), 0u8, cache(&store));
        let _ = value.get();
        value.set(&3);
        assert!(!value.cache.borrow().is_dirty(), "nothing to write back");
    }

    struct FailingOnce {
        inner: MemoryStateStore,
        failed: std::sync::atomic::AtomicBool,
    }

    impl StateStore for FailingOnce {
        fn load(&self, key: &str) -> Result<Option<Vec<u8>>, ServiceError> {
            self.inner.load(key)
        }
        fn save(&self, key: &str, value: &[u8]) -> Result<(), ServiceError> {
            if !self.failed.swap(true, std::sync::atomic::Ordering::SeqCst) {
                return Err(ServiceError::new("disk full"));
            }
            self.inner.save(key, value)
        }
        fn remove(&self, key: &str) -> Result<(), ServiceError> {
            self.inner.remove(key)
        }
    }

    #[test]
    fn a_failed_write_stays_buffered_for_the_next_flush() {
        let inner = MemoryStateStore::new();
        let store = Arc::new(FailingOnce { inner: inner.clone(), failed: false.into() });
        let cache = Rc::new(RefCell::new(StateCache::new(Some(store))));
        Persisted::new("k".to_owned(), 0u8, Rc::clone(&cache)).set(&9);
        assert!(cache.borrow_mut().flush().is_err());
        assert!(cache.borrow().is_dirty(), "not lost");
        cache.borrow_mut().flush().unwrap();
        assert_eq!(inner.load("k").unwrap().as_deref(), Some(&b"9"[..]));
    }
}
