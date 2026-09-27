//! Stateful actors: an identity, one instance at a time handling one
//! message at a time, private durable storage, and alarms.
//!
//! The contract is [`Actor`], with [`Storage`] over a [`Backend`]. It runs
//! in two places:
//!
//! - `LocalActorSystem` (feature `local`): actors in this process, their
//!   storage in SQLite, for development and single-node deployments. An
//!   actor with no work is evicted; its next message starts a new instance,
//!   which finds its storage as the last instance left it.
//! - [`crate::edge`]: an actor per request on an edge host, which routes
//!   each actor id to one instance at a time; its storage is the host's
//!   key-value store.

#[cfg(feature = "local")]
use std::collections::HashMap;
use std::sync::Arc;
#[cfg(feature = "local")]
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(feature = "local")]
use rustnative_server::db::{Db, DbError};
use serde::Serialize;
use serde::de::DeserializeOwned;
#[cfg(feature = "local")]
use tokio::sync::{mpsc, oneshot};

/// An actor's identity.
pub type ActorId = String;

/// Storage refused a write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageError(pub String);

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "storage: {}", self.0)
    }
}

impl std::error::Error for StorageError {}

#[cfg(feature = "local")]
impl From<DbError> for StorageError {
    fn from(error: DbError) -> Self {
        Self(error.to_string())
    }
}

/// Where actors keep their state: each actor's values by key (as JSON),
/// and its alarm.
pub trait Backend: Send + Sync + 'static {
    /// `actor`'s value at `key`.
    fn get(&self, actor: &str, key: &str) -> Option<String>;

    /// Stores `value` at `key` for `actor`, durably, before returning.
    ///
    /// # Errors
    ///
    /// The store refused.
    fn put(&self, actor: &str, key: &str, value: &str) -> Result<(), StorageError>;

    /// Sets `actor`'s alarm for `at` (milliseconds since the epoch).
    ///
    /// # Errors
    ///
    /// The store refused.
    fn set_alarm(&self, actor: &str, at: i64) -> Result<(), StorageError>;
}

/// An actor's private, durable key-value storage.
#[derive(Clone)]
pub struct Storage {
    backend: Arc<dyn Backend>,
    actor: String,
}

impl Storage {
    /// `actor`'s storage in `backend`.
    #[must_use]
    pub fn new(backend: Arc<dyn Backend>, actor: impl Into<String>) -> Self {
        Self { backend, actor: actor.into() }
    }

    /// A stored value.
    #[must_use]
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        serde_json::from_str(&self.backend.get(&self.actor, key)?).ok()
    }

    /// Stores a value, durably, before returning.
    ///
    /// # Errors
    ///
    /// It does not serialize, or the store refused.
    pub fn put<T: Serialize>(&self, key: &str, value: &T) -> Result<(), StorageError> {
        let text = serde_json::to_string(value).map_err(|error| StorageError(error.to_string()))?;
        self.backend.put(&self.actor, key, &text)
    }
}

/// Actors' storage in SQLite.
#[cfg(feature = "local")]
pub struct SqliteBackend(Db);

#[cfg(feature = "local")]
impl SqliteBackend {
    /// Storage in `db`, creating its tables.
    ///
    /// # Errors
    ///
    /// The tables cannot be created.
    pub fn new(db: Db) -> Result<Self, DbError> {
        db.get().execute_batch(
            "CREATE TABLE IF NOT EXISTS _actor_storage (actor TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY (actor, key));
             CREATE TABLE IF NOT EXISTS _actor_alarms (actor TEXT PRIMARY KEY, at INTEGER NOT NULL);",
        )?;
        Ok(Self(db))
    }
}

#[cfg(feature = "local")]
impl Backend for SqliteBackend {
    fn get(&self, actor: &str, key: &str) -> Option<String> {
        self.0
            .get()
            .query_row(
                "SELECT value FROM _actor_storage WHERE actor = ?1 AND key = ?2",
                rusqlite::params![actor, key],
                |row| row.get(0),
            )
            .ok()
    }

    fn put(&self, actor: &str, key: &str, value: &str) -> Result<(), StorageError> {
        self.0
            .get()
            .execute(
                "INSERT INTO _actor_storage (actor, key, value) VALUES (?1, ?2, ?3)
                 ON CONFLICT (actor, key) DO UPDATE SET value = excluded.value",
                rusqlite::params![actor, key, value],
            )
            .map_err(DbError::from)?;
        Ok(())
    }

    fn set_alarm(&self, actor: &str, at: i64) -> Result<(), StorageError> {
        self.0
            .get()
            .execute(
                "INSERT INTO _actor_alarms (actor, at) VALUES (?1, ?2) ON CONFLICT (actor) DO UPDATE SET at = excluded.at",
                rusqlite::params![actor, at],
            )
            .map_err(DbError::from)?;
        Ok(())
    }
}

/// Milliseconds since the epoch.
pub(crate) fn now_ms() -> i64 {
    i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis()))
        .unwrap_or(i64::MAX)
}

/// What an actor can reach while handling a message.
pub struct ActorContext {
    id: ActorId,
    storage: Storage,
}

impl ActorContext {
    /// The context of actor `id`, with `storage`.
    #[must_use]
    pub const fn new(id: ActorId, storage: Storage) -> Self {
        Self { id, storage }
    }

    /// Its identity.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Its storage.
    #[must_use]
    pub const fn storage(&self) -> &Storage {
        &self.storage
    }

    /// Sets the alarm, so that [`Actor::alarm`] runs `delay` from now. The
    /// alarm is durable: one set before a restart still fires.
    ///
    /// # Errors
    ///
    /// The store refused.
    pub fn set_alarm(&self, delay: Duration) -> Result<(), StorageError> {
        let delay = i64::try_from(delay.as_millis()).unwrap_or(i64::MAX);
        self.storage.backend.set_alarm(&self.id, now_ms().saturating_add(delay))
    }
}

/// An actor.
#[async_trait::async_trait]
pub trait Actor: Send + 'static {
    /// What it receives.
    type Message: Send + 'static;
    /// What it answers.
    type Reply: Send + 'static;

    /// A fresh instance for `id`, loading what it needs from `storage`.
    fn start(id: &str, storage: &Storage) -> Self;

    /// Handles one message. No other message is handled meanwhile.
    async fn handle(&mut self, message: Self::Message, context: &ActorContext) -> Self::Reply;

    /// The alarm fired.
    async fn alarm(&mut self, _context: &ActorContext) {}
}

#[cfg(feature = "local")]
type Envelope<A> = (<A as Actor>::Message, oneshot::Sender<<A as Actor>::Reply>);

#[cfg(feature = "local")]
enum Input<A: Actor> {
    Message(Envelope<A>),
    Alarm,
}

#[cfg(feature = "local")]
type Mailboxes<A> = Arc<Mutex<HashMap<ActorId, mpsc::UnboundedSender<Input<A>>>>>;

/// Actors of type `A`, in this process.
#[cfg(feature = "local")]
pub struct LocalActorSystem<A: Actor> {
    db: Db,
    backend: Arc<SqliteBackend>,
    running: Mailboxes<A>,
    idle: Duration,
}

#[cfg(feature = "local")]
impl<A: Actor> Clone for LocalActorSystem<A> {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            backend: Arc::clone(&self.backend),
            running: Arc::clone(&self.running),
            idle: self.idle,
        }
    }
}

#[cfg(feature = "local")]
impl<A: Actor> LocalActorSystem<A> {
    /// A system storing in `db`. It evicts an actor after `idle` without
    /// messages.
    ///
    /// # Errors
    ///
    /// The tables cannot be created.
    pub fn new(db: Db, idle: Duration) -> Result<Self, DbError> {
        let backend = Arc::new(SqliteBackend::new(db.clone())?);
        Ok(Self { db, backend, running: Arc::default(), idle })
    }

    fn mailbox(&self, id: &str) -> mpsc::UnboundedSender<Input<A>> {
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(sender) = running.get(id).filter(|sender| !sender.is_closed()) {
            return sender.clone();
        }
        let (sender, mut receiver) = mpsc::unbounded_channel::<Input<A>>();
        running.insert(id.to_owned(), sender.clone());
        let backend: Arc<dyn Backend> = self.backend.clone();
        let context = ActorContext::new(id.to_owned(), Storage::new(backend, id));
        let idle = self.idle;
        tokio::spawn(async move {
            let mut actor = A::start(&context.id, &context.storage);
            // One message at a time: this loop is the actor's only thread of
            // execution.
            while let Ok(Some(input)) = tokio::time::timeout(idle, receiver.recv()).await {
                match input {
                    Input::Message((message, reply)) => {
                        let _ = reply.send(actor.handle(message, &context).await);
                    }
                    Input::Alarm => actor.alarm(&context).await,
                }
            }
        });
        sender
    }

    /// Sends `message` to actor `id`, starting it if needed, and waits for
    /// its reply.
    ///
    /// # Errors
    ///
    /// The actor stopped before replying.
    pub async fn ask(&self, id: &str, message: A::Message) -> Result<A::Reply, String> {
        let (reply, answer) = oneshot::channel();
        let mut input = Input::Message((message, reply));
        // An actor evicted between lookup and send is started again.
        for _ in 0..2 {
            match self.mailbox(id).send(input) {
                Ok(()) => return answer.await.map_err(|_| "the actor stopped".to_owned()),
                Err(returned) => {
                    self.running.lock().unwrap_or_else(PoisonError::into_inner).remove(id);
                    input = returned.0;
                }
            }
        }
        Err("the actor could not be started".into())
    }

    /// How many actors are running.
    #[must_use]
    pub fn running(&self) -> usize {
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .filter(|sender| !sender.is_closed())
            .count()
    }

    /// Fires the alarms that are due. Call it on a timer.
    ///
    /// # Errors
    ///
    /// SQLite refused.
    pub fn fire_alarms(&self) -> Result<usize, DbError> {
        let now = now_ms();
        let due: Vec<String> = {
            let connection = self.db.get();
            let mut statement =
                connection.prepare("SELECT actor FROM _actor_alarms WHERE at <= ?1")?;
            let rows = statement.query_map([now], |row| row.get(0))?;
            rows.filter_map(Result::ok).collect()
        };
        for actor in &due {
            self.db.get().execute("DELETE FROM _actor_alarms WHERE actor = ?1", [actor])?;
            let _ = self.mailbox(actor).send(Input::Alarm);
        }
        Ok(due.len())
    }
}
