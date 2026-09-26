//! Queries: typed, keyed, cached asynchronous data (`PLAN.md` Milestone 47,
//! `C29`, `C30`).
//!
//! A component asks a [`QueryClient`] for a [`Query`] and gets back a
//! [`QueryState`] — an exhaustive type it must match: loading, empty,
//! failed, succeeded, or showing a stale value while it refreshes. The
//! client does the rest:
//!
//! - **Caching by key.** A result is kept under its [`QueryKey`] and shared
//!   by every component that asks for the same key.
//! - **Deduplication.** One key has at most one request in flight, however
//!   many components ask.
//! - **Two lifetimes.** A result is *fresh* for its stale time and *kept*
//!   for its retention (garbage-collection) time after its last observer
//!   leaves. A stale result is still shown — as
//!   [`QueryState::Refreshing`] — while it is fetched again
//!   (stale-while-revalidate).
//! - **Revalidation as policy** ([`Revalidate`]): when a component first
//!   observes the key, when the window regains focus
//!   ([`QueryClient::window_focused`]), when the network returns
//!   ([`QueryClient::set_online`]), and on an interval.
//! - **Retries** with exponential backoff and jitter; the jitter comes from
//!   a seeded generator, so under a [`rustnative_core::ManualExecutor`] a
//!   test sees the same delays every run.
//! - **Hierarchical invalidation.** [`QueryClient::invalidate`] marks every
//!   key under a prefix stale, and refetches the observed ones.
//! - **Cancellation when unobserved.** A fetch nobody observes any more is
//!   cancelled, and the entry is collected after its retention time.
//! - **Structural sharing.** A result equal to the cached one keeps the
//!   cached value's identity, so no component re-renders for it.
//! - **Pagination** ([`Query::infinite`], [`QueryClient::fetch_next_page`]).
//! - **Batching** of colocated requirements ([`crate::BatchLoader`]).
//!
//! # Example
//!
//! ```
//! use std::rc::Rc;
//!
//! use rustnative_core::{Component, ComponentContext, Event, Node};
//! use rustnative_data::{Query, QueryClient, QueryError, QueryState};
//!
//! struct Todos;
//! impl Component for Todos {
//!     type Props = ();
//!     type Message = ();
//!     fn new((): ()) -> Self { Self }
//!     fn props(&self) -> &() { &() }
//!     fn set_props(&mut self, (): ()) {}
//!     fn view(&self) -> Node { Node::column("todos", []) }
//!     fn update(&mut self, _: Event) {}
//!     fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
//!         let client = context.scoped::<QueryClient>().expect("provided by the root");
//!         let query = Query::new(["todos"], || async {
//!             Ok::<_, QueryError>(vec!["write the docs".to_owned()])
//!         })
//!         .empty_when(Vec::is_empty);
//!         match client.use_query(context, query) {
//!             QueryState::Loading => Node::label("status", "Loading…"),
//!             QueryState::Empty => Node::label("status", "Nothing to do"),
//!             QueryState::Failure(error) => Node::label("status", error.message),
//!             QueryState::Success(todos) | QueryState::Refreshing(todos) => Node::column(
//!                 "todos",
//!                 todos.iter().enumerate().map(|(index, todo)| {
//!                     Node::label(format!("todo-{index}"), todo.clone())
//!                 }),
//!             ),
//!         }
//!     }
//! }
//! ```

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::time::Duration;

use rustnative_core::{Background, ComponentContext, ComponentId, Store, TaskHandle};

/// A boxed future that stays on the UI thread.
pub type LocalFuture<T> = Pin<Box<dyn Future<Output = T>>>;

pub(crate) type Erased = Rc<dyn Any>;
type Fetch = Rc<dyn Fn(&Background, Option<Erased>) -> LocalFuture<Result<Erased, QueryError>>>;
/// Loads the next page, and says whether there is one.
type More = (Fetch, fn(&dyn Any) -> bool);

/// A query's identity: a path of parts, so keys form a hierarchy that
/// [`QueryClient::invalidate`] can address by prefix.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct QueryKey(Vec<String>);

impl QueryKey {
    /// A key made of `parts`.
    pub fn new<S: Into<String>>(parts: impl IntoIterator<Item = S>) -> Self {
        Self(parts.into_iter().map(Into::into).collect())
    }

    /// Its parts.
    #[must_use]
    pub fn parts(&self) -> &[String] {
        &self.0
    }

    /// Whether `prefix`'s parts begin this key's.
    #[must_use]
    pub fn starts_with(&self, prefix: &Self) -> bool {
        self.0.starts_with(&prefix.0)
    }
}

impl fmt::Display for QueryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.join("/"))
    }
}

impl<const N: usize> From<[&str; N]> for QueryKey {
    fn from(parts: [&str; N]) -> Self {
        Self::new(parts)
    }
}

impl From<&str> for QueryKey {
    fn from(part: &str) -> Self {
        Self::new([part])
    }
}

impl From<Vec<String>> for QueryKey {
    fn from(parts: Vec<String>) -> Self {
        Self(parts)
    }
}

/// Why a query failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryError {
    /// What went wrong, for the person using the application.
    pub message: String,
    /// Whether trying again might succeed (a timeout, say, but not a 404).
    pub retryable: bool,
}

impl QueryError {
    /// A failure worth retrying.
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), retryable: true }
    }

    /// A failure retrying cannot fix.
    pub fn fatal(message: impl Into<String>) -> Self {
        Self { message: message.into(), retryable: false }
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for QueryError {}

impl From<rustnative_core::ServiceError> for QueryError {
    fn from(error: rustnative_core::ServiceError) -> Self {
        Self::new(error.to_string())
    }
}

/// What a component sees of a query; it must handle every case (`C29`).
#[derive(Debug, Clone, PartialEq)]
pub enum QueryState<T> {
    /// Nothing to show yet: the first fetch is under way.
    Loading,
    /// The fetch succeeded with nothing in it (see [`Query::empty_when`]).
    Empty,
    /// The fetch failed and there is no earlier value to show. (When there
    /// is one, it keeps being shown, as [`Self::Success`].)
    Failure(QueryError),
    /// The value, fresh or stale.
    Success(T),
    /// A stale value, shown while a fresh one is fetched.
    Refreshing(T),
}

impl<T> QueryState<T> {
    /// The value, if there is one to show.
    #[must_use]
    pub fn data(&self) -> Option<&T> {
        match self {
            Self::Success(data) | Self::Refreshing(data) => Some(data),
            _ => None,
        }
    }
}

/// When a query is fetched again (`C30`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Revalidate {
    /// When a component starts observing a stale result.
    pub on_mount: bool,
    /// When the window regains focus ([`QueryClient::window_focused`]).
    pub on_focus: bool,
    /// When the network returns ([`QueryClient::set_online`]).
    pub on_reconnect: bool,
    /// Every this often, while observed.
    pub interval: Option<Duration>,
}

impl Default for Revalidate {
    fn default() -> Self {
        Self { on_mount: true, on_focus: true, on_reconnect: true, interval: None }
    }
}

#[derive(Debug, Clone, Copy)]
struct Options {
    stale: Duration,
    retain: Duration,
    retries: u32,
    retry_delay: Duration,
    revalidate: Revalidate,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            stale: Duration::ZERO,
            retain: Duration::from_secs(300),
            retries: 3,
            retry_delay: Duration::from_millis(500),
            revalidate: Revalidate::default(),
        }
    }
}

/// A request for data: its key, how to fetch it, and its policies.
pub struct Query<D> {
    key: QueryKey,
    fetch: Fetch,
    more: Option<More>,
    options: Options,
    empty: Option<fn(&D) -> bool>,
}

impl<D> fmt::Debug for Query<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Query").field("key", &self.key).finish_non_exhaustive()
    }
}

fn erase<D: 'static>(data: D) -> Erased {
    Rc::new(data)
}

impl<D: PartialEq + 'static> Query<D> {
    /// A query for `key`, fetched by `fetch` on the executor — off the UI
    /// thread.
    pub fn new<F, Fut>(key: impl Into<QueryKey>, fetch: F) -> Self
    where
        F: Fn() -> Fut + 'static,
        Fut: Future<Output = Result<D, QueryError>> + Send + 'static,
        D: Send,
    {
        let fetch: Fetch = Rc::new(move |background, _| {
            let running = background.offload(fetch());
            Box::pin(async move { running.await.map(erase) })
        });
        Self { key: key.into(), fetch, more: None, options: Options::default(), empty: None }
    }

    /// A query whose fetch runs on the UI thread — for a fetch that awaits
    /// UI-thread work itself, such as a [`crate::BatchLoader`].
    pub fn local<F, Fut>(key: impl Into<QueryKey>, fetch: F) -> Self
    where
        F: Fn() -> Fut + 'static,
        Fut: Future<Output = Result<D, QueryError>> + 'static,
    {
        let fetch: Fetch = Rc::new(move |_, _| {
            let running = fetch();
            Box::pin(async move { running.await.map(erase) })
        });
        Self { key: key.into(), fetch, more: None, options: Options::default(), empty: None }
    }

    /// How long a result is fresh (default: zero — stale at once, so it is
    /// revalidated whenever the policy says).
    #[must_use]
    pub fn stale_time(mut self, stale: Duration) -> Self {
        self.options.stale = stale;
        self
    }

    /// How long a result is kept once nothing observes it (default: five
    /// minutes).
    #[must_use]
    pub fn retain_time(mut self, retain: Duration) -> Self {
        self.options.retain = retain;
        self
    }

    /// How many times a retryable failure is retried (default: 3), the
    /// first after `delay`, each later one after twice the one before,
    /// each lengthened by up to half again at random.
    #[must_use]
    pub fn retries(mut self, retries: u32, delay: Duration) -> Self {
        self.options.retries = retries;
        self.options.retry_delay = delay;
        self
    }

    /// When the result is fetched again.
    #[must_use]
    pub fn revalidate(mut self, revalidate: Revalidate) -> Self {
        self.options.revalidate = revalidate;
        self
    }

    /// Which results are [`QueryState::Empty`] rather than
    /// [`QueryState::Success`].
    #[must_use]
    pub fn empty_when(mut self, empty: fn(&D) -> bool) -> Self {
        self.empty = Some(empty);
        self
    }

    /// Its key.
    #[must_use]
    pub fn key(&self) -> &QueryKey {
        &self.key
    }
}

/// One page of an [`Query::infinite`] query, as its fetch returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<I, C> {
    /// The page's items.
    pub items: Vec<I>,
    /// Where the next page starts, or `None` after the last.
    pub next: Option<C>,
}

/// Every page an [`Query::infinite`] query has loaded so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pages<I, C> {
    /// The items of every loaded page, in order.
    pub items: Vec<I>,
    /// Where the next page starts, or `None` when all are loaded.
    pub next: Option<C>,
    /// How many pages are loaded.
    pub pages: usize,
}

impl<I, C> Query<Pages<I, C>>
where
    I: Clone + PartialEq + Send + 'static,
    C: Clone + PartialEq + Send + 'static,
{
    /// A paginated query: `fetch_page(None)` loads the first page, and
    /// [`QueryClient::fetch_next_page`] loads each next one from the cursor
    /// the page before returned. Invalidating it reloads the first page.
    pub fn infinite<F, Fut>(key: impl Into<QueryKey>, fetch_page: F) -> Self
    where
        F: Fn(Option<C>) -> Fut + 'static,
        Fut: Future<Output = Result<Page<I, C>, QueryError>> + Send + 'static,
    {
        let fetch_page = Rc::new(fetch_page);
        let first = Rc::clone(&fetch_page);
        let fetch: Fetch = Rc::new(move |background, _| {
            let running = background.offload(first(None));
            Box::pin(async move {
                running
                    .await
                    .map(|page| erase(Pages { items: page.items, next: page.next, pages: 1 }))
            })
        });
        // ponytail: each page copies the items loaded so far; a persistent
        // list would make appending O(page) if lists grow to many thousands.
        let more: Fetch = Rc::new(move |background, current| {
            let loaded = current
                .and_then(|current| current.downcast_ref::<Pages<I, C>>().cloned())
                .unwrap_or(Pages { items: Vec::new(), next: None, pages: 0 });
            let running = background.offload(fetch_page(loaded.next.clone()));
            Box::pin(async move {
                let page = running.await?;
                let mut all = loaded;
                all.items.extend(page.items);
                all.next = page.next;
                all.pages += 1;
                Ok(erase(all))
            })
        });
        Self {
            key: key.into(),
            fetch,
            more: Some((more, probe_pages::<I, C>)),
            options: Options::default(),
            empty: None,
        }
    }
}

/// The observable part of one cached query.
#[derive(Clone, Default)]
pub(crate) struct Slot {
    pub(crate) data: Option<Erased>,
    pub(crate) error: Option<QueryError>,
    pub(crate) fetching: bool,
    pub(crate) loading_more: bool,
}

/// Everything a [`QueryClient`] shows: its cached results and its
/// mutations' progress. Read slices of it with
/// [`ComponentContext::select`] on [`QueryClient::store`].
#[derive(Default)]
pub struct Cache {
    pub(crate) slots: HashMap<QueryKey, Slot>,
    pub(crate) mutations: crate::mutation::MutationStatus,
}

impl Cache {
    /// The mutations' progress.
    #[must_use]
    pub fn mutations(&self) -> crate::mutation::MutationStatus {
        self.mutations
    }

    /// Whether `key` is loading its next page.
    #[must_use]
    pub fn loading_more(&self, key: &QueryKey) -> bool {
        self.slots.get(key).is_some_and(|slot| slot.loading_more)
    }
}

/// The bookkeeping behind one cached query.
struct Meta {
    fetch: Fetch,
    more: Option<More>,
    same: fn(&dyn Any, &dyn Any) -> bool,
    options: Options,
    fetched_at: Option<Duration>,
    invalidated: bool,
    observers: HashMap<ComponentId, u32>,
    known: HashSet<ComponentId>,
    task: Option<TaskHandle>,
    interval: Option<TaskHandle>,
    retention: Option<TaskHandle>,
}

impl Meta {
    fn observed(&self) -> bool {
        !self.known.is_empty()
    }
}

fn same<D: PartialEq + 'static>(old: &dyn Any, new: &dyn Any) -> bool {
    matches!((old.downcast_ref::<D>(), new.downcast_ref::<D>()), (Some(old), Some(new)) if old == new)
}

pub(crate) struct Inner {
    pub(crate) cache: Store<Cache>,
    meta: RefCell<HashMap<QueryKey, Meta>>,
    background: RefCell<Option<Background>>,
    random: Cell<u64>,
    fetches: Cell<u64>,
    pub(crate) mutations: RefCell<crate::mutation::Mutations>,
}

/// The cache of queries and the runner of mutations; see the [module
/// documentation](self).
///
/// Clones share one cache. A client is provided to the components that use
/// it with [`ComponentContext::provide_scoped`] — usually by a window's
/// root, for a window-wide cache.
#[derive(Clone)]
pub struct QueryClient {
    pub(crate) inner: Rc<Inner>,
}

impl PartialEq for QueryClient {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl fmt::Debug for QueryClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueryClient")
            .field("queries", &self.inner.meta.borrow().len())
            .finish_non_exhaustive()
    }
}

impl Default for QueryClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Keeps a component counted as an observer of a key for as long as the
/// selection that holds it is alive — that is, until the component renders
/// again or unmounts.
struct Observer {
    client: Weak<Inner>,
    key: QueryKey,
    id: ComponentId,
}

impl Drop for Observer {
    fn drop(&mut self) {
        let Some(inner) = self.client.upgrade() else { return };
        let client = QueryClient { inner };
        let released = {
            let Ok(mut meta) = client.inner.meta.try_borrow_mut() else { return };
            let Some(entry) = meta.get_mut(&self.key) else { return };
            let Some(count) = entry.observers.get_mut(&self.id) else { return };
            *count = count.saturating_sub(1);
            *count == 0
        };
        if released {
            // Decided after the render that dropped this: the component
            // may be observing again already.
            let key = self.key.clone();
            let id = self.id;
            let settle = client.clone();
            if let Some(background) = client.background() {
                background.spawn_local(async move { settle.settle_observer(&key, id) });
            }
        }
    }
}

impl QueryClient {
    /// An empty client.
    #[must_use]
    pub fn new() -> Self {
        Self::with_seed(0x9E37_79B9_7F4A_7C15)
    }

    /// An empty client whose retry jitter comes from `seed`.
    #[must_use]
    pub fn with_seed(seed: u64) -> Self {
        Self {
            inner: Rc::new(Inner {
                cache: Store::new("queries", Cache::default()),
                meta: RefCell::new(HashMap::new()),
                background: RefCell::new(None),
                random: Cell::new(seed.max(1)),
                fetches: Cell::new(0),
                mutations: RefCell::new(crate::mutation::Mutations::default()),
            }),
        }
    }

    /// The store behind the client: select slices of it to observe
    /// mutations' progress.
    #[must_use]
    pub fn store(&self) -> &Store<Cache> {
        &self.inner.cache
    }

    /// How many fetches the client has started — what a test counts to see
    /// deduplication and caching at work.
    #[must_use]
    pub fn fetch_count(&self) -> u64 {
        self.inner.fetches.get()
    }

    pub(crate) fn attach(&self, background: Background) {
        {
            let mut slot = self.inner.background.borrow_mut();
            if slot.is_some() {
                return;
            }
            *slot = Some(background);
        }
        // Mutations a previous run left queued are sent as soon as there is
        // somewhere to send them from.
        if self.is_online() && !self.inner.mutations.borrow().is_empty() {
            self.replay_offline();
        }
    }

    pub(crate) fn background(&self) -> Option<Background> {
        self.inner.background.borrow().clone()
    }

    pub(crate) fn update_slot(&self, key: &QueryKey, change: impl FnOnce(&mut Slot) + 'static) {
        let key = key.clone();
        self.inner.cache.update(move |cache| change(cache.slots.entry(key).or_default()));
    }

    /// The data cached under `key`, if it is a `D`.
    #[must_use]
    pub fn data<D: 'static>(&self, key: &QueryKey) -> Option<Rc<D>> {
        self.inner.cache.read(|cache| {
            cache.slots.get(key).and_then(|slot| slot.data.clone()).and_then(|d| d.downcast().ok())
        })
    }

    /// Replaces the data cached under `key` with `data` — as a mutation's
    /// optimistic update does.
    pub fn set_data<D: 'static>(&self, key: &QueryKey, data: D) {
        let data = erase(data);
        self.update_slot(key, move |slot| slot.data = Some(data));
    }

    pub(crate) fn set_erased(&self, key: &QueryKey, data: Option<Erased>) {
        self.update_slot(key, move |slot| slot.data = data);
    }

    pub(crate) fn erased(&self, key: &QueryKey) -> Option<Erased> {
        self.inner.cache.read(|cache| cache.slots.get(key).and_then(|slot| slot.data.clone()))
    }

    /// Asks for `query` on behalf of the component rendering with
    /// `context`, which re-renders when what it sees changes.
    pub fn use_query<D, M>(
        &self,
        context: &mut ComponentContext<'_, M>,
        query: Query<D>,
    ) -> QueryState<Rc<D>>
    where
        D: PartialEq + 'static,
        M: Send + 'static,
    {
        self.attach(context.background());
        let id = context.component_id();
        let Query { key, fetch, more, options, empty } = query;
        let mounted = {
            let mut meta = self.inner.meta.borrow_mut();
            let entry = meta.entry(key.clone()).or_insert_with(|| Meta {
                fetch: Rc::clone(&fetch),
                more: more.clone(),
                same: same::<D>,
                options,
                fetched_at: None,
                invalidated: false,
                observers: HashMap::new(),
                known: HashSet::new(),
                task: None,
                interval: None,
                retention: None,
            });
            // The latest render's fetch is the one used from now on.
            entry.fetch = fetch;
            entry.more = more;
            entry.options = options;
            if let Some(retention) = entry.retention.take() {
                retention.cancel();
            }
            *entry.observers.entry(id).or_insert(0) += 1;
            entry.known.insert(id)
        };
        if mounted {
            self.mounted(&key);
        }
        let observer = Observer { client: Rc::downgrade(&self.inner), key: key.clone(), id };
        context.select(&self.inner.cache, move |cache| {
            let _keep = &observer;
            state_of::<D>(cache.slots.get(&key), empty)
        })
    }

    /// Fetches `query` ahead of need, so a component that asks for it later
    /// finds it cached.
    pub fn prefetch<D: PartialEq + 'static>(&self, background: &Background, query: &Query<D>) {
        self.attach(background.clone());
        let key = query.key.clone();
        self.inner.meta.borrow_mut().entry(key.clone()).or_insert_with(|| Meta {
            fetch: Rc::clone(&query.fetch),
            more: query.more.clone(),
            same: same::<D>,
            options: query.options,
            fetched_at: None,
            invalidated: false,
            observers: HashMap::new(),
            known: HashSet::new(),
            task: None,
            interval: None,
            retention: None,
        });
        if self.is_stale(&key) {
            self.start_fetch(&key, false);
        }
        self.schedule_retention(&key);
    }

    /// Marks every query whose key starts with `prefix` stale, and fetches
    /// again the ones a component is observing.
    pub fn invalidate(&self, prefix: impl Into<QueryKey>) {
        let prefix = prefix.into();
        let observed = {
            let mut meta = self.inner.meta.borrow_mut();
            meta.iter_mut()
                .filter(|(key, _)| key.starts_with(&prefix))
                .filter_map(|(key, entry)| {
                    entry.invalidated = true;
                    entry.observed().then(|| key.clone())
                })
                .collect::<Vec<_>>()
        };
        for key in observed {
            self.start_fetch(&key, false);
        }
    }

    /// Loads the next page of the [`Query::infinite`] query `key`, unless it
    /// is loading already or has no next page.
    pub fn fetch_next_page(&self, key: &QueryKey) {
        self.start_fetch(key, true);
    }

    /// Tells the client its window regained focus: observed stale queries
    /// whose policy says so are fetched again.
    pub fn window_focused(&self) {
        self.revalidate_where(|revalidate| revalidate.on_focus);
    }

    /// Tells the client whether the network is reachable. Coming back
    /// online fetches again the observed queries whose policy says so, and
    /// sends the mutations queued while offline.
    pub fn set_online(&self, online: bool) {
        let was = self.is_online();
        self.inner.cache.update(move |cache| cache.mutations.online = online);
        if online && !was {
            self.revalidate_where(|revalidate| revalidate.on_reconnect);
            self.replay_offline();
        }
    }

    /// Whether the client believes the network is reachable.
    #[must_use]
    pub fn is_online(&self) -> bool {
        self.inner.cache.read(|cache| cache.mutations.online)
    }

    fn revalidate_where(&self, applies: impl Fn(&Revalidate) -> bool) {
        let keys = self
            .inner
            .meta
            .borrow()
            .iter()
            .filter(|(_, entry)| entry.observed() && applies(&entry.options.revalidate))
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in keys {
            if self.is_stale(&key) {
                self.start_fetch(&key, false);
            }
        }
    }

    fn is_stale(&self, key: &QueryKey) -> bool {
        let now = self.background().map_or(Duration::ZERO, |background| background.now());
        let has_data = self.erased(key).is_some();
        self.inner.meta.borrow().get(key).is_some_and(|entry| {
            !has_data
                || entry.invalidated
                || entry.fetched_at.is_none_or(|at| now.saturating_sub(at) >= entry.options.stale)
        })
    }

    /// A component started observing `key`.
    fn mounted(&self, key: &QueryKey) {
        let (on_mount, interval, has_interval) = {
            let meta = self.inner.meta.borrow();
            let Some(entry) = meta.get(key) else { return };
            (
                entry.options.revalidate.on_mount,
                entry.options.revalidate.interval,
                entry.interval.is_some(),
            )
        };
        let has_data = self.erased(key).is_some();
        if (!has_data || on_mount) && self.is_stale(key) {
            self.start_fetch(key, false);
        }
        if let (Some(every), false, Some(background)) = (interval, has_interval, self.background())
        {
            let client = self.clone();
            let polled = key.clone();
            let sleeper = background.clone();
            let handle = background.spawn_local(async move {
                loop {
                    sleeper.sleep(every).await;
                    client.start_fetch(&polled, false);
                }
            });
            if let Some(entry) = self.inner.meta.borrow_mut().get_mut(key) {
                entry.interval = Some(handle);
            }
        }
    }

    fn settle_observer(&self, key: &QueryKey, id: ComponentId) {
        let unobserved = {
            let mut meta = self.inner.meta.borrow_mut();
            let Some(entry) = meta.get_mut(key) else { return };
            if entry.observers.get(&id).copied().unwrap_or(0) > 0 {
                return;
            }
            entry.observers.remove(&id);
            entry.known.remove(&id);
            if entry.observed() {
                false
            } else {
                // Nobody wants it: stop fetching it.
                if let Some(task) = entry.task.take() {
                    task.cancel();
                }
                if let Some(interval) = entry.interval.take() {
                    interval.cancel();
                }
                true
            }
        };
        if unobserved {
            self.update_slot(key, |slot| {
                slot.fetching = false;
                slot.loading_more = false;
            });
            self.schedule_retention(key);
        }
    }

    fn schedule_retention(&self, key: &QueryKey) {
        let Some(background) = self.background() else { return };
        let Some(retain) = self.inner.meta.borrow().get(key).map(|entry| entry.options.retain)
        else {
            return;
        };
        let client = self.clone();
        let collected = key.clone();
        let sleeper = background.clone();
        let handle = background.spawn_local(async move {
            sleeper.sleep(retain).await;
            let remove = client
                .inner
                .meta
                .borrow()
                .get(&collected)
                .is_some_and(|entry| !entry.observed() && entry.task.is_none());
            if remove {
                client.inner.meta.borrow_mut().remove(&collected);
                client.inner.cache.update(move |cache| {
                    cache.slots.remove(&collected);
                });
            }
        });
        if let Some(entry) = self.inner.meta.borrow_mut().get_mut(key) {
            if let Some(previous) = entry.retention.replace(handle) {
                previous.cancel();
            }
        }
    }

    /// Whether a cached query `key` exists.
    #[must_use]
    pub fn contains(&self, key: &QueryKey) -> bool {
        self.inner.meta.borrow().contains_key(key)
    }

    fn next_random(&self) -> u64 {
        // xorshift64: deterministic for a seed, which is all jitter needs.
        let mut x = self.inner.random.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.inner.random.set(x);
        x
    }

    /// The delay before retry `attempt` (from 1): `base` doubled per
    /// attempt, lengthened by up to half again at random.
    pub(crate) fn backoff(&self, base: Duration, attempt: u32) -> Duration {
        let doubled =
            base.saturating_mul(1_u32.checked_shl(attempt.saturating_sub(1)).unwrap_or(u32::MAX));
        let jitter = self.next_random() % 500;
        doubled.saturating_add(doubled.saturating_mul(u32::try_from(jitter).unwrap_or(0)) / 1000)
    }

    fn start_fetch(&self, key: &QueryKey, more: bool) {
        let Some(background) = self.background() else { return };
        let current = self.erased(key);
        let (fetch, options) = {
            let meta = self.inner.meta.borrow();
            let Some(entry) = meta.get(key) else { return };
            if entry.task.is_some() {
                return; // deduplicated: one request per key in flight
            }
            let fetch = if more {
                match (&entry.more, &current) {
                    (Some((more, has_next)), Some(data)) if has_next(data.as_ref()) => {
                        Rc::clone(more)
                    }
                    _ => return,
                }
            } else {
                Rc::clone(&entry.fetch)
            };
            (fetch, entry.options)
        };
        self.inner.fetches.set(self.inner.fetches.get() + 1);
        self.update_slot(key, move |slot| {
            if more {
                slot.loading_more = true;
            } else {
                slot.fetching = true;
            }
        });
        let client = self.clone();
        let fetched = key.clone();
        let runner = background.clone();
        let handle = background.spawn_local(async move {
            let mut attempt = 0;
            let outcome = loop {
                match fetch(&runner, current.clone()).await {
                    Ok(data) => break Ok(data),
                    Err(error) if error.retryable && attempt < options.retries => {
                        attempt += 1;
                        runner.sleep(client.backoff(options.retry_delay, attempt)).await;
                    }
                    Err(error) => break Err(error),
                }
            };
            client.settle_fetch(&fetched, outcome);
        });
        if let Some(entry) = self.inner.meta.borrow_mut().get_mut(key) {
            entry.task = Some(handle);
        }
    }

    fn settle_fetch(&self, key: &QueryKey, outcome: Result<Erased, QueryError>) {
        let now = self.background().map_or(Duration::ZERO, |background| background.now());
        let same = {
            let mut meta = self.inner.meta.borrow_mut();
            let Some(entry) = meta.get_mut(key) else { return };
            entry.task = None;
            if outcome.is_ok() {
                entry.fetched_at = Some(now);
                entry.invalidated = false;
            }
            entry.same
        };
        self.update_slot(key, move |slot| {
            slot.fetching = false;
            slot.loading_more = false;
            match outcome {
                Ok(data) => {
                    // Structural sharing: an equal result keeps the cached
                    // value's identity, and so re-renders nobody.
                    let unchanged =
                        slot.data.as_ref().is_some_and(|old| same(old.as_ref(), data.as_ref()));
                    if !unchanged {
                        slot.data = Some(data);
                    }
                    slot.error = None;
                }
                Err(error) => slot.error = Some(error),
            }
        });
    }
}

fn probe_pages<I: 'static, C: 'static>(data: &dyn Any) -> bool {
    data.downcast_ref::<Pages<I, C>>().is_some_and(|pages| pages.next.is_some())
}

fn state_of<D: 'static>(slot: Option<&Slot>, empty: Option<fn(&D) -> bool>) -> QueryState<Rc<D>> {
    let Some(slot) = slot else { return QueryState::Loading };
    match (&slot.data, &slot.error) {
        (Some(data), _) => match Rc::clone(data).downcast::<D>() {
            Ok(data) if slot.fetching => QueryState::Refreshing(data),
            Ok(data) if empty.is_some_and(|empty| empty(&data)) => QueryState::Empty,
            Ok(data) => QueryState::Success(data),
            Err(_) => QueryState::Failure(QueryError::fatal(
                "two queries used the same key for different types",
            )),
        },
        (None, Some(error)) if !slot.fetching => QueryState::Failure(error.clone()),
        _ => QueryState::Loading,
    }
}
