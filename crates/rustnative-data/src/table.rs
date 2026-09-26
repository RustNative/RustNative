//! Live queries over local storage (`PLAN.md` Milestone 47, `C31`).
//!
//! # The repository pattern
//!
//! The network writes to local storage; the UI reads from it. A screen
//! showing a list reads a [`LocalTable::live`] query, which re-renders it
//! whenever the rows it selects change — whoever changed them. Fetching is
//! then only a matter of keeping the table filled: a [`PagingSource`]
//! loads the pages a virtualized list needs and has not got (it fills
//! gaps), and writes them into the table. Offline, the list shows what the
//! table holds; online, it fills in.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::future::Future;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use rustnative_core::{Background, ComponentContext, StateStore, Store};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::query::{LocalFuture, QueryError};

/// A row of a [`LocalTable`].
pub trait Row: Clone + PartialEq + Serialize + DeserializeOwned + 'static {
    /// Its primary key.
    fn id(&self) -> String;
}

/// A table of rows, in memory and optionally in the state store, whose
/// queries are live; see the [module documentation](self).
pub struct LocalTable<T: Row> {
    rows: Store<BTreeMap<String, T>>,
    persist: Option<(Arc<dyn StateStore>, String)>,
}

impl<T: Row> Clone for LocalTable<T> {
    fn clone(&self) -> Self {
        Self { rows: self.rows.clone(), persist: self.persist.clone() }
    }
}

impl<T: Row> PartialEq for LocalTable<T> {
    fn eq(&self, other: &Self) -> bool {
        self.rows == other.rows
    }
}

impl<T: Row> fmt::Debug for LocalTable<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalTable").field("rows", &self.rows).finish_non_exhaustive()
    }
}

impl<T: Row> LocalTable<T> {
    /// An in-memory table.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self { rows: Store::new(name, BTreeMap::new()), persist: None }
    }

    /// A table kept in `store` under `key`, loaded from it now.
    #[must_use]
    pub fn persisted(name: &str, store: Arc<dyn StateStore>, key: impl Into<String>) -> Self {
        let key = key.into();
        let rows = store
            .load(&key)
            .ok()
            .flatten()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self { rows: Store::new(name, rows), persist: Some((store, key)) }
    }

    fn save(&self) {
        if let Some((store, key)) = &self.persist {
            let bytes = self.rows.read(|rows| serde_json::to_vec(rows).unwrap_or_default());
            if let Err(error) = store.save(key, &bytes) {
                eprintln!("rustnative-data: could not save table {key}: {error}");
            }
        }
    }

    /// Inserts `row`, or replaces the row with its id.
    pub fn upsert(&self, row: T) {
        self.rows.update(move |rows| {
            rows.insert(row.id(), row);
        });
        self.save();
    }

    /// Inserts or replaces every row of `new`, in one update.
    pub fn upsert_all(&self, new: impl IntoIterator<Item = T>) {
        let new = new.into_iter().collect::<Vec<_>>();
        self.rows.update(move |rows| {
            for row in new {
                rows.insert(row.id(), row);
            }
        });
        self.save();
    }

    /// Removes the row with `id`.
    pub fn remove(&self, id: &str) {
        let id = id.to_owned();
        self.rows.update(move |rows| {
            rows.remove(&id);
        });
        self.save();
    }

    /// The row with `id`.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<T> {
        self.rows.read(|rows| rows.get(id).cloned())
    }

    /// How many rows it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.read(BTreeMap::len)
    }

    /// Whether it holds no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The rows `filter` keeps, in `order` — re-rendering the component
    /// rendering with `context` when, and only when, that result changes.
    /// Feed it to a virtual list.
    pub fn live<M: Send + 'static>(
        &self,
        context: &mut ComponentContext<'_, M>,
        filter: impl Fn(&T) -> bool + 'static,
        order: fn(&T, &T) -> Ordering,
    ) -> Vec<T> {
        context.select(&self.rows, move |rows| {
            let mut kept = rows.values().filter(|row| filter(row)).cloned().collect::<Vec<_>>();
            kept.sort_by(order);
            kept
        })
    }
}

type FetchPage<T> = Rc<dyn Fn(usize) -> LocalFuture<Result<Vec<T>, QueryError>>>;

/// Loads the pages of a remote list that a window of it needs, into a
/// [`LocalTable`], skipping the pages already loaded; see the [module
/// documentation](self).
pub struct PagingSource<T: Row> {
    table: LocalTable<T>,
    page_size: usize,
    fetch: FetchPage<T>,
    background: Background,
    loaded: Rc<RefCell<BTreeSet<usize>>>,
    loading: Rc<RefCell<BTreeSet<usize>>>,
}

impl<T: Row> fmt::Debug for PagingSource<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PagingSource")
            .field("loaded", &self.loaded.borrow())
            .finish_non_exhaustive()
    }
}

impl<T: Row + Send> PagingSource<T> {
    /// A source filling `table` a page of `page_size` rows at a time, page
    /// `n` fetched by `fetch(n)` off the UI thread.
    pub fn new<F, Fut>(
        table: LocalTable<T>,
        page_size: usize,
        background: &Background,
        fetch: F,
    ) -> Self
    where
        F: Fn(usize) -> Fut + 'static,
        Fut: Future<Output = Result<Vec<T>, QueryError>> + Send + 'static,
    {
        let runner = background.clone();
        let fetch: FetchPage<T> = Rc::new(move |page| Box::pin(runner.offload(fetch(page))));
        Self {
            table,
            page_size: page_size.max(1),
            fetch,
            background: background.clone(),
            loaded: Rc::new(RefCell::new(BTreeSet::new())),
            loading: Rc::new(RefCell::new(BTreeSet::new())),
        }
    }

    /// Loads every page overlapping `rows` (a virtual list's visible range)
    /// that is neither loaded nor loading.
    pub fn ensure(&self, rows: Range<usize>) {
        if rows.is_empty() {
            return;
        }
        let first = rows.start / self.page_size;
        let last = (rows.end - 1) / self.page_size;
        for page in first..=last {
            if self.loaded.borrow().contains(&page) || !self.loading.borrow_mut().insert(page) {
                continue;
            }
            let running = (self.fetch)(page);
            let table = self.table.clone();
            let loaded = Rc::clone(&self.loaded);
            let loading = Rc::clone(&self.loading);
            self.background.spawn_local(async move {
                let outcome = running.await;
                loading.borrow_mut().remove(&page);
                match outcome {
                    Ok(rows) => {
                        table.upsert_all(rows);
                        loaded.borrow_mut().insert(page);
                    }
                    Err(error) => eprintln!("rustnative-data: page {page} failed: {error}"),
                }
            });
        }
    }

    /// The pages loaded so far.
    #[must_use]
    pub fn loaded_pages(&self) -> Vec<usize> {
        self.loaded.borrow().iter().copied().collect()
    }

    /// Forgets which pages are loaded, so the next [`Self::ensure`] fetches
    /// them again (the rows stay in the table until replaced).
    pub fn refresh(&self) {
        self.loaded.borrow_mut().clear();
    }
}
