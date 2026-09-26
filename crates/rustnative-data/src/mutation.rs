//! Mutations: changes sent to a server, shown optimistically, rolled back
//! when rejected, and queued while offline (`PLAN.md` Milestone 47).
//!
//! A [`Mutation`] is data — a kind and a JSON payload — so a mutation made
//! while offline can be written to durable storage and sent after the
//! application restarts. The code that sends each kind is registered once
//! with [`QueryClient::register_mutation`].
//!
//! # Lifecycle
//!
//! 1. The mutation's optimistic updates apply to the cached queries at once.
//! 2. **Online**, it is sent. Accepted, the queries it names are
//!    invalidated. Rejected, the optimistic updates are rolled back and the
//!    queries invalidated. In conflict with the server's version, the
//!    client's [`ConflictPolicy`] decides.
//! 3. **Offline** (or when sending finds the server unreachable), it joins
//!    the offline queue, persisted in the [`StateStore`] given to
//!    [`QueryClient::with_offline_queue`]. Coming back online
//!    ([`QueryClient::set_online`]) sends the queue in order.
//!
//! [`StateStore`]: rustnative_core::StateStore

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;

use rustnative_core::StateStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::query::{Erased, LocalFuture, QueryClient, QueryKey};

/// Where the offline queue is kept in the state store.
pub const OFFLINE_QUEUE_KEY: &str = "rustnative.data.offline-mutations";

/// The mutations' progress, as components observe it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MutationStatus {
    /// Mutations sent and not yet answered.
    pub pending: usize,
    /// Mutations waiting in the offline queue.
    pub queued: usize,
    /// Mutations the server rejected, since the client was made.
    pub rejected: usize,
    /// Whether the client believes the network is reachable.
    pub online: bool,
}

impl Default for MutationStatus {
    fn default() -> Self {
        Self { pending: 0, queued: 0, rejected: 0, online: true }
    }
}

/// Why the server did not accept a mutation.
#[derive(Debug, Clone, PartialEq)]
pub enum MutationError {
    /// It will never be accepted as it is.
    Rejected(String),
    /// It conflicts with the server's current version, which is attached.
    Conflict {
        /// The server's version.
        server: Value,
    },
    /// The server could not be reached: the mutation is queued.
    Unreachable(String),
}

/// How a conflict with the server's version is resolved.
#[derive(Clone, Copy)]
pub enum ConflictPolicy {
    /// The server's version stands: the mutation is dropped and rolled back.
    ServerWins,
    /// The mutation is sent again with [`MutationRequest::force`] set.
    ClientWins,
    /// The payload is merged with the server's version by the function, and
    /// the result sent with [`MutationRequest::force`] set.
    Merge(fn(client: &Value, server: &Value) -> Value),
}

impl fmt::Debug for ConflictPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ServerWins => "ServerWins",
            Self::ClientWins => "ClientWins",
            Self::Merge(_) => "Merge",
        })
    }
}

/// What a mutation handler is asked to send.
#[derive(Debug, Clone, PartialEq)]
pub struct MutationRequest {
    /// The mutation's kind.
    pub kind: String,
    /// Its payload.
    pub payload: Value,
    /// Whether to overwrite the server's version (after a conflict).
    pub force: bool,
}

/// A mutation waiting in the offline queue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueuedMutation {
    /// The mutation's kind.
    pub kind: String,
    /// Its payload.
    pub payload: Value,
    /// The queries to invalidate once it is sent.
    pub invalidates: Vec<QueryKey>,
}

type Optimistic = Box<dyn FnOnce(Option<Erased>) -> Option<Erased>>;
type Handler = Rc<dyn Fn(MutationRequest) -> LocalFuture<Result<(), MutationError>>>;

/// A change to send; see the [module documentation](self).
pub struct Mutation {
    kind: String,
    payload: Value,
    optimistic: Vec<(QueryKey, Optimistic)>,
    invalidates: Vec<QueryKey>,
}

impl fmt::Debug for Mutation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mutation")
            .field("kind", &self.kind)
            .field("payload", &self.payload)
            .finish_non_exhaustive()
    }
}

impl Mutation {
    /// A mutation of `kind` carrying `payload`.
    pub fn new(kind: impl Into<String>, payload: impl Serialize) -> Self {
        Self {
            kind: kind.into(),
            payload: serde_json::to_value(payload).unwrap_or(Value::Null),
            optimistic: Vec::new(),
            invalidates: Vec::new(),
        }
    }

    /// Changes the query `key`'s cached `D` at once, before the server
    /// answers; rolled back if the server rejects the mutation.
    #[must_use]
    pub fn optimistic<D: Clone + 'static>(
        mut self,
        key: impl Into<QueryKey>,
        change: impl FnOnce(&mut D) + 'static,
    ) -> Self {
        self.optimistic.push((
            key.into(),
            Box::new(move |current: Option<Erased>| {
                let mut data = current?.downcast_ref::<D>()?.clone();
                change(&mut data);
                Some(Rc::new(data) as Erased)
            }),
        ));
        self
    }

    /// Invalidates the queries under `prefix` once the mutation is settled.
    #[must_use]
    pub fn invalidates(mut self, prefix: impl Into<QueryKey>) -> Self {
        self.invalidates.push(prefix.into());
        self
    }
}

pub(crate) struct Mutations {
    handlers: HashMap<String, Handler>,
    conflict: ConflictPolicy,
    queue: VecDeque<QueuedMutation>,
    store: Option<Arc<dyn StateStore>>,
    replaying: bool,
}

impl Mutations {
    pub(crate) fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

impl Default for Mutations {
    fn default() -> Self {
        Self {
            handlers: HashMap::new(),
            conflict: ConflictPolicy::ServerWins,
            queue: VecDeque::new(),
            store: None,
            replaying: false,
        }
    }
}

/// The optimistic values a mutation replaced, to restore on rejection.
type Snapshots = Vec<(QueryKey, Option<Erased>)>;

enum Outcome {
    Settled,
    Queue,
}

impl QueryClient {
    /// Registers the code that sends mutations of `kind`. It runs on the
    /// executor, off the UI thread.
    pub fn register_mutation<F, Fut>(&self, kind: impl Into<String>, send: F)
    where
        F: Fn(MutationRequest) -> Fut + 'static,
        Fut: Future<Output = Result<(), MutationError>> + Send + 'static,
    {
        // Weak: the handler lives in the client, and must not keep it alive.
        let client = Rc::downgrade(&self.inner);
        let handler: Handler = Rc::new(move |request| {
            let running = client
                .upgrade()
                .and_then(|inner| QueryClient { inner }.background())
                .map(|background| background.offload(send(request)));
            Box::pin(async move {
                match running {
                    Some(running) => running.await,
                    None => Err(MutationError::Unreachable("the client is not attached".into())),
                }
            })
        });
        self.inner.mutations.borrow_mut().handlers.insert(kind.into(), handler);
    }

    /// Queues mutations made while offline in `store`, which keeps them
    /// across restarts; mutations a previous run left queued are loaded now.
    /// Conflicts are resolved by `policy`.
    #[must_use]
    pub fn with_offline_queue(self, store: Arc<dyn StateStore>, policy: ConflictPolicy) -> Self {
        let queue = store
            .load(OFFLINE_QUEUE_KEY)
            .ok()
            .flatten()
            .and_then(|bytes| serde_json::from_slice::<VecDeque<QueuedMutation>>(&bytes).ok())
            .unwrap_or_default();
        let queued = queue.len();
        {
            let mut mutations = self.inner.mutations.borrow_mut();
            mutations.queue = queue;
            mutations.store = Some(store);
            mutations.conflict = policy;
        }
        self.inner.cache.update(move |cache| cache.mutations.queued = queued);
        self
    }

    /// Binds the client to the tree's background work. A client used by a
    /// component through [`Self::use_query`] is bound already; one that only
    /// mutates is bound with this.
    pub fn bind(&self, background: rustnative_core::Background) {
        self.attach(background);
    }

    /// The mutations waiting in the offline queue, oldest first.
    #[must_use]
    pub fn queued_mutations(&self) -> Vec<QueuedMutation> {
        self.inner.mutations.borrow().queue.iter().cloned().collect()
    }

    /// Applies `mutation`'s optimistic updates and sends it, or queues it
    /// while offline; see the [module documentation](self).
    pub fn mutate(&self, mutation: Mutation) {
        let Mutation { kind, payload, optimistic, invalidates } = mutation;
        let mut snapshots = Snapshots::new();
        for (key, change) in optimistic {
            let before = self.erased(&key);
            let after = change(before.clone());
            if after.is_some() {
                self.set_erased(&key, after);
                snapshots.push((key, before));
            }
        }
        let queued = QueuedMutation { kind, payload, invalidates };
        if self.is_online() {
            self.send(queued, snapshots, false);
        } else {
            self.enqueue(queued);
        }
    }

    fn persist_queue(&self) {
        let mutations = self.inner.mutations.borrow();
        let queued = mutations.queue.len();
        if let Some(store) = &mutations.store {
            let bytes = serde_json::to_vec(&mutations.queue).unwrap_or_default();
            if let Err(error) = store.save(OFFLINE_QUEUE_KEY, &bytes) {
                eprintln!("rustnative-data: could not save the offline queue: {error}");
            }
        }
        drop(mutations);
        self.inner.cache.update(move |cache| cache.mutations.queued = queued);
    }

    fn enqueue(&self, mutation: QueuedMutation) {
        self.inner.mutations.borrow_mut().queue.push_back(mutation);
        self.persist_queue();
    }

    fn send(&self, mutation: QueuedMutation, snapshots: Snapshots, replay: bool) {
        let Some(background) = self.background() else {
            if !replay {
                self.enqueue(mutation);
            }
            self.inner.mutations.borrow_mut().replaying = false;
            return;
        };
        self.inner.cache.update(|cache| cache.mutations.pending += 1);
        let client = self.clone();
        background.spawn_local(async move {
            let outcome = client.deliver(&mutation, &snapshots).await;
            client.inner.cache.update(|cache| cache.mutations.pending -= 1);
            match outcome {
                Outcome::Settled => {
                    if replay {
                        // Removed from the queue only now that it is sent,
                        // so a run that ends mid-send sends it next time.
                        client.inner.mutations.borrow_mut().queue.pop_front();
                        client.persist_queue();
                        client.replay_next();
                    }
                }
                Outcome::Queue => {
                    if !replay {
                        client.enqueue(mutation);
                    }
                    client.inner.mutations.borrow_mut().replaying = false;
                    client.inner.cache.update(|cache| cache.mutations.online = false);
                }
            }
        });
    }

    async fn deliver(&self, mutation: &QueuedMutation, snapshots: &Snapshots) -> Outcome {
        let (handler, policy) = {
            let mutations = self.inner.mutations.borrow();
            (mutations.handlers.get(&mutation.kind).cloned(), mutations.conflict)
        };
        let Some(handler) = handler else {
            eprintln!("rustnative-data: no handler registered for mutation {:?}", mutation.kind);
            self.reject(mutation, snapshots);
            return Outcome::Settled;
        };
        let mut request = MutationRequest {
            kind: mutation.kind.clone(),
            payload: mutation.payload.clone(),
            force: false,
        };
        loop {
            match handler(request.clone()).await {
                Ok(()) => {
                    self.invalidate_all(&mutation.invalidates);
                    return Outcome::Settled;
                }
                Err(MutationError::Unreachable(_)) => return Outcome::Queue,
                Err(MutationError::Conflict { server }) if !request.force => match policy {
                    ConflictPolicy::ServerWins => {
                        self.reject(mutation, snapshots);
                        return Outcome::Settled;
                    }
                    ConflictPolicy::ClientWins => request.force = true,
                    ConflictPolicy::Merge(merge) => {
                        request.payload = merge(&request.payload, &server);
                        request.force = true;
                    }
                },
                Err(MutationError::Rejected(_) | MutationError::Conflict { .. }) => {
                    self.reject(mutation, snapshots);
                    return Outcome::Settled;
                }
            }
        }
    }

    fn reject(&self, mutation: &QueuedMutation, snapshots: &Snapshots) {
        for (key, before) in snapshots.iter().rev() {
            self.set_erased(key, before.clone());
        }
        self.inner.cache.update(|cache| cache.mutations.rejected += 1);
        self.invalidate_all(&mutation.invalidates);
    }

    fn invalidate_all(&self, prefixes: &[QueryKey]) {
        for prefix in prefixes {
            self.invalidate(prefix.clone());
        }
    }

    /// Sends the offline queue, one mutation at a time, in order.
    pub(crate) fn replay_offline(&self) {
        {
            let mut mutations = self.inner.mutations.borrow_mut();
            if mutations.replaying {
                return;
            }
            mutations.replaying = true;
        }
        self.replay_next();
    }

    fn replay_next(&self) {
        let next = self.inner.mutations.borrow().queue.front().cloned();
        match next {
            Some(mutation) => {
                // Its optimistic update was applied when it was made (or
                // lost with the previous run); a rejection now can only
                // invalidate.
                self.send(mutation, Snapshots::new(), true);
            }
            None => self.inner.mutations.borrow_mut().replaying = false,
        }
    }
}
