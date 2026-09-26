//! The sync service: local-first collections replicated through a server.
//!
//! A [`SyncedCollection`] answers reads and writes from its local copy,
//! offline or not, and keeps what it changed until [`SyncedCollection::sync`]
//! pushes it and pulls what others changed. Conflicts are settled by the
//! collection's declared [`ConflictPolicy`]. A replica may pull only what a
//! [`Filter`] selects (partial replication). Records carry their schema
//! version: a newer server upgrades an older client's writes, and sends an
//! older client records it can read or tells it to upgrade.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::clock::{Clock, Hlc};

/// A replicated record.
pub trait Record: Serialize + DeserializeOwned + Clone + PartialEq + Send + Sync + 'static {
    /// Its collection's name.
    const COLLECTION: &'static str;
    /// Its schema version.
    const VERSION: u32 = 1;

    /// Its id.
    fn id(&self) -> String;

    /// Brings a record written at an older schema `from` to this one.
    ///
    /// # Errors
    ///
    /// When there is no path from `from`.
    fn upgrade(from: u32, value: Value) -> Result<Value, String> {
        if from == Self::VERSION {
            Ok(value)
        } else {
            Err(format!("no upgrade from version {from}"))
        }
    }
}

/// A merge of two record values: the server's, then the writer's.
pub type MergeFn = Arc<dyn Fn(&Value, &Value) -> Value + Send + Sync>;

/// How a collection settles two writes to the same record.
#[derive(Clone)]
pub enum ConflictPolicy {
    /// The server's copy wins: a write based on an outdated copy is refused
    /// and the writer gets the server's.
    ServerAuthority,
    /// The later write (by hybrid logical clock) wins.
    LastWriterWins,
    /// Both are combined by a function of the two values (server's first).
    Merge(MergeFn),
}

impl std::fmt::Debug for ConflictPolicy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ServerAuthority => "ServerAuthority",
            Self::LastWriterWins => "LastWriterWins",
            Self::Merge(_) => "Merge",
        })
    }
}

/// One write, as it travels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    /// The collection.
    pub collection: String,
    /// The record.
    pub id: String,
    /// Its new value, or `None` for a deletion.
    pub value: Option<Value>,
    /// When it was written.
    pub stamp: Hlc,
    /// The schema version it was written at.
    pub version: u32,
    /// The server sequence the writer's copy was based on (for server
    /// authority).
    pub base: u64,
    /// The server's sequence number, once accepted.
    #[serde(default)]
    pub sequence: u64,
}

/// Which records a replica wants: those whose `field` equals `value`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    /// The field.
    pub field: String,
    /// The value it must have.
    pub value: Value,
}

impl Filter {
    fn matches(&self, change: &Change) -> bool {
        // A deletion always travels: the replica may hold the record.
        change.value.as_ref().is_none_or(|value| value.get(&self.field) == Some(&self.value))
    }
}

/// A push.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Push {
    /// The writes.
    pub changes: Vec<Change>,
    /// The writer's schema version for each collection.
    pub versions: BTreeMap<String, u32>,
}

/// A pull.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pull {
    /// The collection.
    pub collection: String,
    /// The last sequence the replica has.
    pub since: u64,
    /// What it wants (all, without one).
    pub filter: Option<Filter>,
    /// Its schema version.
    pub version: u32,
}

/// The server's answer to a push.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PushReply {
    /// Writes accepted, with their sequence numbers.
    pub accepted: Vec<Change>,
    /// Writes refused (server authority), with the server's copy instead.
    pub refused: Vec<Change>,
}

/// The server's answer to a pull.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PullReply {
    /// The changes since, and the new cursor.
    Changes {
        /// The changes.
        changes: Vec<Change>,
        /// The latest sequence.
        cursor: u64,
    },
    /// The replica's schema is too old to read what the server holds.
    NeedsUpgrade {
        /// The oldest version the server can serve.
        minimum: u32,
    },
}

/// Why a sync did not complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncError {
    /// The transport failed (offline); nothing was lost.
    Unreachable(String),
    /// The server needs a newer application.
    NeedsUpgrade(u32),
    /// A record could not be read.
    Decode(String),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(error) => write!(formatter, "the server is unreachable: {error}"),
            Self::NeedsUpgrade(minimum) => {
                write!(formatter, "update the application (schema {minimum} or later)")
            }
            Self::Decode(error) => write!(formatter, "a record could not be read: {error}"),
        }
    }
}

impl std::error::Error for SyncError {}

/// How a replica talks to the server.
#[async_trait::async_trait]
pub trait SyncTransport: Send + Sync {
    /// Sends writes.
    async fn push(&self, push: Push) -> Result<PushReply, SyncError>;
    /// Asks for changes.
    async fn pull(&self, pull: Pull) -> Result<PullReply, SyncError>;
}

struct CollectionState {
    policy: ConflictPolicy,
    version: u32,
    minimum_readable: u32,
    upgrade: Arc<dyn Fn(u32, Value) -> Result<Value, String> + Send + Sync>,
    records: BTreeMap<String, Change>,
}

/// The server side of sync: every collection's current records, in
/// sequence order.
pub struct SyncServer {
    collections: BTreeMap<String, CollectionState>,
    sequence: u64,
    notify: tokio::sync::broadcast::Sender<u64>,
}

impl Default for SyncServer {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncServer {
    /// A server with no collections.
    #[must_use]
    pub fn new() -> Self {
        Self {
            collections: BTreeMap::new(),
            sequence: 0,
            notify: tokio::sync::broadcast::channel(64).0,
        }
    }

    /// Serves collection `T` with `policy`; replicas older than
    /// `minimum_readable` are told to upgrade.
    #[must_use]
    pub fn collection<T: Record>(mut self, policy: ConflictPolicy, minimum_readable: u32) -> Self {
        self.collections.insert(
            T::COLLECTION.to_owned(),
            CollectionState {
                policy,
                version: T::VERSION,
                minimum_readable,
                upgrade: Arc::new(T::upgrade),
                records: BTreeMap::new(),
            },
        );
        self
    }

    /// The latest sequence.
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// A receiver told the sequence after every accepted write (server
    /// push: a replica waiting on it syncs when something changed).
    #[must_use]
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<u64> {
        self.notify.subscribe()
    }

    /// Applies a push.
    pub fn push(&mut self, push: Push) -> PushReply {
        let mut reply = PushReply { accepted: Vec::new(), refused: Vec::new() };
        for mut change in push.changes {
            let Some(state) = self.collections.get_mut(&change.collection) else { continue };
            // A write from an older schema is brought up to date first.
            if change.version < state.version {
                if let Some(value) = change.value.take() {
                    match (state.upgrade)(change.version, value) {
                        Ok(upgraded) => change.value = Some(upgraded),
                        Err(_) => continue,
                    }
                }
                change.version = state.version;
            }
            let existing = state.records.get(&change.id).cloned();
            let winner = match (&state.policy, &existing) {
                (_, None) => Some(change),
                (ConflictPolicy::ServerAuthority, Some(current)) => {
                    if change.base >= current.sequence {
                        Some(change)
                    } else {
                        reply.refused.push(current.clone());
                        None
                    }
                }
                (ConflictPolicy::LastWriterWins, Some(current)) => {
                    if change.stamp > current.stamp {
                        Some(change)
                    } else {
                        reply.refused.push(current.clone());
                        None
                    }
                }
                (ConflictPolicy::Merge(merge), Some(current)) => {
                    let value = match (&current.value, &change.value) {
                        (Some(server), Some(client)) => Some(merge(server, client)),
                        (_, client) => client.clone(),
                    };
                    Some(Change { value, stamp: change.stamp.max(current.stamp), ..change })
                }
            };
            if let Some(mut winner) = winner {
                self.sequence += 1;
                winner.sequence = self.sequence;
                state.records.insert(winner.id.clone(), winner.clone());
                reply.accepted.push(winner);
            }
        }
        if !reply.accepted.is_empty() {
            let _ = self.notify.send(self.sequence);
        }
        reply
    }

    /// Answers a pull.
    #[must_use]
    pub fn pull(&self, pull: &Pull) -> PullReply {
        let Some(state) = self.collections.get(&pull.collection) else {
            return PullReply::Changes { changes: Vec::new(), cursor: self.sequence };
        };
        if pull.version < state.minimum_readable {
            return PullReply::NeedsUpgrade { minimum: state.minimum_readable };
        }
        let mut changes: Vec<Change> = state
            .records
            .values()
            .filter(|change| change.sequence > pull.since)
            .filter(|change| pull.filter.as_ref().is_none_or(|filter| filter.matches(change)))
            .cloned()
            .collect();
        changes.sort_by_key(|change| change.sequence);
        PullReply::Changes { changes, cursor: self.sequence }
    }
}

/// A transport to a server in the same process, which can be taken
/// offline (for tests and examples).
#[derive(Clone)]
pub struct InMemory {
    server: Arc<Mutex<SyncServer>>,
    online: Arc<std::sync::atomic::AtomicBool>,
}

impl InMemory {
    /// A transport to `server`.
    #[must_use]
    pub fn new(server: Arc<Mutex<SyncServer>>) -> Self {
        Self { server, online: Arc::new(std::sync::atomic::AtomicBool::new(true)) }
    }

    /// Goes offline or back online.
    pub fn set_online(&self, online: bool) {
        self.online.store(online, std::sync::atomic::Ordering::SeqCst);
    }

    fn check(&self) -> Result<(), SyncError> {
        if self.online.load(std::sync::atomic::Ordering::SeqCst) {
            Ok(())
        } else {
            Err(SyncError::Unreachable("offline".into()))
        }
    }
}

#[async_trait::async_trait]
impl SyncTransport for InMemory {
    async fn push(&self, push: Push) -> Result<PushReply, SyncError> {
        self.check()?;
        Ok(self.server.lock().unwrap_or_else(PoisonError::into_inner).push(push))
    }

    async fn pull(&self, pull: Pull) -> Result<PullReply, SyncError> {
        self.check()?;
        Ok(self.server.lock().unwrap_or_else(PoisonError::into_inner).pull(&pull))
    }
}

/// The records a replica holds, with what it has not sent yet.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Local {
    records: BTreeMap<String, Change>,
    pending: BTreeMap<String, Change>,
    cursor: u64,
}

/// A local-first replicated collection of `T`.
pub struct SyncedCollection<T: Record> {
    local: Local,
    clock: Arc<Clock>,
    filter: Option<Filter>,
    _record: std::marker::PhantomData<fn() -> T>,
}

/// What one sync did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SyncReport {
    /// Local writes the server accepted.
    pub sent: usize,
    /// Local writes the server refused (its copy is now local).
    pub refused: usize,
    /// Remote changes applied.
    pub received: usize,
}

impl<T: Record> SyncedCollection<T> {
    /// An empty collection on the replica `clock` belongs to. Its
    /// conflict policy is the server's declaration for the collection
    /// ([`SyncServer::collection`]), so every replica settles alike.
    #[must_use]
    pub fn new(clock: Arc<Clock>) -> Self {
        Self { local: Local::default(), clock, filter: None, _record: std::marker::PhantomData }
    }

    /// Replicates only what `filter` selects.
    #[must_use]
    pub fn filtered(mut self, filter: Filter) -> Self {
        self.filter = Some(filter);
        self
    }

    /// The local copy, serialized, to persist (offline survives a restart).
    #[must_use]
    pub fn snapshot(&self) -> Value {
        serde_json::to_value(&self.local).unwrap_or(Value::Null)
    }

    /// Restores a [`Self::snapshot`].
    pub fn restore(&mut self, snapshot: Value) {
        if let Ok(local) = serde_json::from_value(snapshot) {
            self.local = local;
        }
    }

    fn decode(change: &Change) -> Option<T> {
        let value = change.value.clone()?;
        let value = if change.version < T::VERSION {
            T::upgrade(change.version, value).ok()?
        } else {
            value
        };
        serde_json::from_value(value).ok()
    }

    /// A record.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<T> {
        self.local.records.get(id).and_then(Self::decode)
    }

    /// Every record, by id.
    #[must_use]
    pub fn list(&self) -> Vec<T> {
        self.local.records.values().filter_map(Self::decode).collect()
    }

    /// Writes `record` locally; it is sent at the next sync.
    ///
    /// # Errors
    ///
    /// The record does not serialize (nothing is written: a value that
    /// cannot be stored must never read as a deletion).
    pub fn put(&mut self, record: &T) -> Result<(), SyncError> {
        let value =
            serde_json::to_value(record).map_err(|error| SyncError::Decode(error.to_string()))?;
        let change = Change {
            collection: T::COLLECTION.into(),
            id: record.id(),
            value: Some(value),
            stamp: self.clock.now(),
            version: T::VERSION,
            base: self.local.records.get(&record.id()).map_or(0, |current| current.sequence),
            sequence: 0,
        };
        self.local.records.insert(change.id.clone(), change.clone());
        self.local.pending.insert(change.id.clone(), change);
        Ok(())
    }

    /// Deletes `id` locally; the deletion is sent at the next sync.
    pub fn delete(&mut self, id: &str) {
        let change = Change {
            collection: T::COLLECTION.into(),
            id: id.to_owned(),
            value: None,
            stamp: self.clock.now(),
            version: T::VERSION,
            base: self.local.records.get(id).map_or(0, |current| current.sequence),
            sequence: 0,
        };
        self.local.records.insert(id.to_owned(), change.clone());
        self.local.pending.insert(id.to_owned(), change);
    }

    /// Writes not yet accepted.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.local.pending.len()
    }

    /// Pushes local writes and pulls remote ones.
    ///
    /// # Errors
    ///
    /// The transport failed (the writes stay pending), or the server needs
    /// a newer application.
    pub async fn sync(&mut self, transport: &dyn SyncTransport) -> Result<SyncReport, SyncError> {
        let mut report = SyncReport::default();
        if !self.local.pending.is_empty() {
            let changes: Vec<Change> = self.local.pending.values().cloned().collect();
            let versions = BTreeMap::from([(T::COLLECTION.to_owned(), T::VERSION)]);
            let reply = transport.push(Push { changes, versions }).await?;
            for accepted in reply.accepted {
                self.clock.observe(accepted.stamp);
                self.local.pending.remove(&accepted.id);
                self.local.records.insert(accepted.id.clone(), accepted);
                report.sent += 1;
            }
            for refused in reply.refused {
                self.clock.observe(refused.stamp);
                self.local.pending.remove(&refused.id);
                self.local.records.insert(refused.id.clone(), refused);
                report.refused += 1;
            }
        }
        let pull = Pull {
            collection: T::COLLECTION.into(),
            since: self.local.cursor,
            filter: self.filter.clone(),
            version: T::VERSION,
        };
        match transport.pull(pull).await? {
            PullReply::NeedsUpgrade { minimum } => Err(SyncError::NeedsUpgrade(minimum)),
            PullReply::Changes { changes, cursor } => {
                for change in changes {
                    self.clock.observe(change.stamp);
                    // A local write not yet sent is settled at its push.
                    if !self.local.pending.contains_key(&change.id) {
                        self.local.records.insert(change.id.clone(), change);
                        report.received += 1;
                    }
                }
                self.local.cursor = cursor;
                Ok(report)
            }
        }
    }
}
