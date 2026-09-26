//! Stateful actors: an identity, one instance at a time handling one
//! message at a time, private durable storage, and alarms.
//!
//! [`LocalActorSystem`] runs actors in this process, for development and
//! for single-node deployments. An actor with no work is evicted. Its next
//! message starts a new instance, which finds its storage as the last
//! instance left it. The edge adapter (an actor per object on an edge
//! platform) is owed with the Web track's edge target.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rustnative_server::db::{Db, DbError};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::sync::{mpsc, oneshot};

/// An actor's identity.
pub type ActorId = String;

/// An actor's private, durable key-value storage.
#[derive(Clone)]
pub struct Storage {
    db: Db,
    actor: String,
}

impl Storage {
    /// A stored value.
    #[must_use]
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let text: String = self
            .db
            .get()
            .query_row(
                "SELECT value FROM _actor_storage WHERE actor = ?1 AND key = ?2",
                rusqlite::params![self.actor, key],
                |row| row.get(0),
            )
            .ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Stores a value, durably, before returning.
    ///
    /// # Errors
    ///
    /// It does not serialize, or SQLite refused.
    pub fn put<T: Serialize>(&self, key: &str, value: &T) -> Result<(), DbError> {
        let text =
            serde_json::to_string(value).map_err(|error| DbError::Task(error.to_string()))?;
        self.db.get().execute(
            "INSERT INTO _actor_storage (actor, key, value) VALUES (?1, ?2, ?3)
             ON CONFLICT (actor, key) DO UPDATE SET value = excluded.value",
            rusqlite::params![self.actor, key, text],
        )?;
        Ok(())
    }
}

/// What an actor can reach while handling a message.
pub struct ActorContext {
    id: ActorId,
    storage: Storage,
}

impl ActorContext {
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
    /// SQLite refused.
    pub fn set_alarm(&self, delay: Duration) -> Result<(), DbError> {
        let at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis())
            + delay.as_millis();
        self.storage.db.get().execute(
            "INSERT INTO _actor_alarms (actor, at) VALUES (?1, ?2) ON CONFLICT (actor) DO UPDATE SET at = excluded.at",
            rusqlite::params![self.id, i64::try_from(at).unwrap_or(i64::MAX)],
        )?;
        Ok(())
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

type Envelope<A> = (<A as Actor>::Message, oneshot::Sender<<A as Actor>::Reply>);

enum Input<A: Actor> {
    Message(Envelope<A>),
    Alarm,
}

type Mailboxes<A> = Arc<Mutex<HashMap<ActorId, mpsc::UnboundedSender<Input<A>>>>>;

/// Actors of type `A`, in this process.
pub struct LocalActorSystem<A: Actor> {
    db: Db,
    running: Mailboxes<A>,
    idle: Duration,
}

impl<A: Actor> Clone for LocalActorSystem<A> {
    fn clone(&self) -> Self {
        Self { db: self.db.clone(), running: Arc::clone(&self.running), idle: self.idle }
    }
}

impl<A: Actor> LocalActorSystem<A> {
    /// A system storing in `db`. It evicts an actor after `idle` without
    /// messages.
    ///
    /// # Errors
    ///
    /// The tables cannot be created.
    pub fn new(db: Db, idle: Duration) -> Result<Self, DbError> {
        db.get().execute_batch(
            "CREATE TABLE IF NOT EXISTS _actor_storage (actor TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY (actor, key));
             CREATE TABLE IF NOT EXISTS _actor_alarms (actor TEXT PRIMARY KEY, at INTEGER NOT NULL);",
        )?;
        Ok(Self { db, running: Arc::default(), idle })
    }

    fn mailbox(&self, id: &str) -> mpsc::UnboundedSender<Input<A>> {
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(sender) = running.get(id).filter(|sender| !sender.is_closed()) {
            return sender.clone();
        }
        let (sender, mut receiver) = mpsc::unbounded_channel::<Input<A>>();
        running.insert(id.to_owned(), sender.clone());
        let context = ActorContext {
            id: id.to_owned(),
            storage: Storage { db: self.db.clone(), actor: id.to_owned() },
        };
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
        let now = i64::try_from(
            SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis()),
        )
        .unwrap_or(i64::MAX);
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
