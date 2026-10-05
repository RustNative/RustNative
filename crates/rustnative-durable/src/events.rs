//! Event handlers: the entry point for work that arrives as events rather
//! than requests, one invocation per batch.
//!
//! - A standard [`EventEnvelope`] (id, source, kind, time, data, attempt),
//!   close to `CloudEvents`.
//! - Batches: a handler takes several events and reports which ones failed
//!   ([`BatchResult`]). The rest are done. A failed event is retried with
//!   backoff. After its last attempt it goes to the dead-letter table with
//!   its error.
//! - Deduplication: an event id is processed once, however often it is
//!   published.
//! - Each invocation runs in a task scope bounded by the invocation, like a
//!   request's. Work it spawns is cancelled when the invocation ends.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rustnative_server::RequestScope;
use rustnative_server::db::{Db, DbError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// Its unique id (the deduplication key).
    pub id: String,
    /// Who produced it.
    pub source: String,
    /// What happened (`order.placed`).
    pub kind: String,
    /// When, in milliseconds since the epoch.
    pub time: u64,
    /// The payload.
    pub data: Value,
    /// Which delivery attempt this is (1 for the first).
    #[serde(default)]
    pub attempt: u32,
}

impl EventEnvelope {
    /// An event of `kind` from `source`, now.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        source: impl Into<String>,
        kind: impl Into<String>,
        data: Value,
    ) -> Self {
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(0));
        Self { id: id.into(), source: source.into(), kind: kind.into(), time, data, attempt: 0 }
    }
}

/// Which events of a batch failed.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BatchResult {
    /// The ids of the events that failed, with why.
    pub failed: Vec<(String, String)>,
}

/// Handles batches of events.
#[async_trait::async_trait]
pub trait EventHandler: Send + Sync + 'static {
    /// Handles `batch`. Events not listed as failed are done. `scope` ends
    /// with the invocation.
    async fn handle(&self, batch: &[EventEnvelope], scope: &RequestScope) -> BatchResult;
}

/// The event queue and its runner.
#[derive(Clone)]
pub struct EventRunner {
    db: Db,
    handler: Arc<dyn EventHandler>,
    batch: usize,
    attempts: u32,
    base_delay: Duration,
}

fn now_ms() -> i64 {
    i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis()))
        .unwrap_or(i64::MAX)
}

impl EventRunner {
    /// A runner for `handler`, taking batches of `batch` and trying each
    /// event `attempts` times.
    ///
    /// # Errors
    ///
    /// The tables cannot be created.
    pub fn new(
        db: Db,
        handler: Arc<dyn EventHandler>,
        batch: usize,
        attempts: u32,
    ) -> Result<Self, DbError> {
        db.get().execute_batch(
            "CREATE TABLE IF NOT EXISTS _events (
                id TEXT PRIMARY KEY, envelope TEXT NOT NULL, attempts INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'ready', next_at INTEGER NOT NULL DEFAULT 0, last_error TEXT
            );
            CREATE TABLE IF NOT EXISTS _dead_letters (id TEXT PRIMARY KEY, envelope TEXT NOT NULL, error TEXT NOT NULL);",
        )?;
        Ok(Self {
            db,
            handler,
            batch: batch.max(1),
            attempts: attempts.max(1),
            base_delay: Duration::from_secs(1),
        })
    }

    /// Retries after `delay`, doubling with each attempt (default one second).
    #[must_use]
    pub const fn backoff(mut self, delay: Duration) -> Self {
        self.base_delay = delay;
        self
    }

    /// Accepts an event. An id seen before is ignored. Returns whether the
    /// event was new.
    ///
    /// # Errors
    ///
    /// SQLite refused.
    pub fn publish(&self, event: &EventEnvelope) -> Result<bool, DbError> {
        let envelope =
            serde_json::to_string(event).map_err(|error| DbError::Task(error.to_string()))?;
        let added = self.db.get().execute(
            "INSERT OR IGNORE INTO _events (id, envelope) VALUES (?1, ?2)",
            rusqlite::params![event.id, envelope],
        )?;
        Ok(added == 1)
    }

    /// Runs one batch of ready events. Returns how many it took.
    ///
    /// # Errors
    ///
    /// SQLite refused.
    pub async fn run_batch(&self) -> Result<usize, DbError> {
        let now = now_ms();
        let batch: Vec<(EventEnvelope, u32)> = {
            let connection = self.db.get();
            let mut statement = connection.prepare(
                "SELECT envelope, attempts FROM _events WHERE status = 'ready' AND next_at <= ?1 ORDER BY rowid LIMIT ?2",
            )?;
            let rows = statement.query_map(
                rusqlite::params![now, i64::try_from(self.batch).unwrap_or(i64::MAX)],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?)),
            )?;
            rows.filter_map(Result::ok)
                .filter_map(|(envelope, attempts)| {
                    let mut event: EventEnvelope = serde_json::from_str(&envelope).ok()?;
                    event.attempt = attempts + 1;
                    Some((event, attempts + 1))
                })
                .collect()
        };
        if batch.is_empty() {
            return Ok(0);
        }
        let events: Vec<EventEnvelope> = batch.iter().map(|(event, _)| event.clone()).collect();
        let scope = RequestScope::default();
        let result = self.handler.handle(&events, &scope).await;
        scope.cancel();
        let connection = self.db.get();
        for (event, attempt) in &batch {
            let failure = result
                .failed
                .iter()
                .find(|(id, _)| id == &event.id)
                .map(|(_, error)| error.clone());
            match failure {
                None => {
                    connection.execute(
                        "UPDATE _events SET status = 'done', attempts = ?2 WHERE id = ?1",
                        rusqlite::params![event.id, attempt],
                    )?;
                }
                Some(error) if *attempt >= self.attempts => {
                    let envelope = serde_json::to_string(event).unwrap_or_default();
                    connection.execute(
                        "INSERT OR REPLACE INTO _dead_letters (id, envelope, error) VALUES (?1, ?2, ?3)",
                        rusqlite::params![event.id, envelope, error],
                    )?;
                    connection.execute(
                        "UPDATE _events SET status = 'dead', attempts = ?2, last_error = ?3 WHERE id = ?1",
                        rusqlite::params![event.id, attempt, error],
                    )?;
                }
                Some(error) => {
                    let factor = 2u32.saturating_pow(attempt.saturating_sub(1));
                    let delay = i64::try_from(self.base_delay.saturating_mul(factor).as_millis())
                        .unwrap_or(i64::MAX);
                    connection.execute(
                        "UPDATE _events SET attempts = ?2, last_error = ?3, next_at = ?4 WHERE id = ?1",
                        rusqlite::params![event.id, attempt, error, now.saturating_add(delay)],
                    )?;
                }
            }
        }
        Ok(batch.len())
    }

    /// Runs batches until none is ready.
    ///
    /// # Errors
    ///
    /// SQLite refused.
    pub async fn run_until_idle(&self) -> Result<(), DbError> {
        while self.run_batch().await? > 0 {}
        Ok(())
    }

    /// The dead letters: events that failed every attempt, each with its
    /// last error.
    ///
    /// # Errors
    ///
    /// SQLite refused.
    pub fn dead_letters(&self) -> Result<Vec<(EventEnvelope, String)>, DbError> {
        let connection = self.db.get();
        let mut statement =
            connection.prepare("SELECT envelope, error FROM _dead_letters ORDER BY rowid")?;
        let rows = statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        Ok(rows
            .filter_map(Result::ok)
            .filter_map(|(envelope, error)| Some((serde_json::from_str(&envelope).ok()?, error)))
            .collect())
    }

    /// Makes every waiting retry due now (a way for tests to skip the
    /// backoff).
    ///
    /// # Errors
    ///
    /// SQLite refused.
    pub fn expedite(&self) -> Result<(), DbError> {
        self.db.get().execute("UPDATE _events SET next_at = 0 WHERE status = 'ready'", [])?;
        Ok(())
    }
}

/// Event handlers as a function's entry point (`C44-1`, Web milestone K):
/// an SQS-shaped batch (`{"Records": [...]}`) handed to `handler` as
/// envelopes, answered with the batch's partial failures
/// (`{"batchItemFailures": [{"itemIdentifier": ...}]}`), so the queue
/// redelivers only those — and, after its own retry limit, dead-letters
/// them. A record whose body is an [`EventEnvelope`] is taken as it is;
/// any other body is the data of a `sqs.message` event whose id is the
/// message id. The invocation's scope ends with the batch.
///
/// Pass it to [`rustnative_server::serverless::lambda::run`].
pub fn sqs_batch(
    handler: Arc<dyn EventHandler>,
) -> rustnative_server::serverless::lambda::EventHook {
    Box::new(move |event: Value, _remaining: Duration| {
        let records = event["Records"].as_array().cloned().unwrap_or_default();
        let mut message_ids = Vec::new();
        let batch: Vec<EventEnvelope> = records
            .iter()
            .map(|record| {
                let message_id = record["messageId"].as_str().unwrap_or_default().to_owned();
                let body = record["body"].as_str().unwrap_or_default();
                let attempt = record["attributes"]["ApproximateReceiveCount"]
                    .as_str()
                    .and_then(|count| count.parse().ok())
                    .unwrap_or(1);
                let mut envelope =
                    serde_json::from_str::<EventEnvelope>(body).unwrap_or_else(|_| {
                        let data = serde_json::from_str(body)
                            .unwrap_or_else(|_| Value::String(body.to_owned()));
                        let source = record["eventSourceARN"].as_str().unwrap_or("sqs");
                        EventEnvelope::new(message_id.clone(), source, "sqs.message", data)
                    });
                envelope.attempt = attempt;
                message_ids.push((envelope.id.clone(), message_id));
                envelope
            })
            .collect();
        let scope = RequestScope::default();
        let result = rustnative_server::serverless::block_on(handler.handle(&batch, &scope));
        drop(scope);
        let failures: Vec<Value> = result
            .failed
            .iter()
            .filter_map(|(id, _)| message_ids.iter().find(|(event, _)| event == id))
            .map(|(_, message)| serde_json::json!({ "itemIdentifier": message }))
            .collect();
        serde_json::json!({ "batchItemFailures": failures })
    })
}
