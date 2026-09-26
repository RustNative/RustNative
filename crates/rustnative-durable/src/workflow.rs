//! Durable workflows: code that survives restarts.
//!
//! A workflow is an `async fn` over a [`WorkflowContext`]. Every step's
//! result is recorded in a journal (SQLite). After a restart the workflow
//! runs again from the top. Each recorded step returns its recorded result
//! instead of running, so the code resumes exactly where it was.
//!
//! - [`WorkflowContext::step`] runs work once and records its result. Work
//!   with an outside effect gets an idempotency key to pass on.
//! - [`WorkflowContext::transactional_step`] runs work against the database
//!   in the same transaction as its journal entry. Its effect happens
//!   exactly once, even if the process dies mid-step.
//! - [`WorkflowContext::sleep`] is a durable timer.
//!   [`WorkflowContext::signal`] waits for an outside signal, such as a
//!   person's approval or a webhook.
//! - [`WorkflowContext::compensate`] registers how to undo a step. If the
//!   workflow fails, completed steps are undone newest first.
//! - [`WorkflowContext::version`] lets an in-flight execution keep the
//!   behaviour it started with when the code changes.
//!
//! **Determinism.** Replay only works if the workflow makes the same
//! decisions again. Time, randomness, and I/O are reachable only through
//! the context (`now`, `random`, `step`), and the context exposes no
//! services. If a workflow diverges anyway, it fails loudly with
//! [`WorkflowError::Divergence`] instead of continuing on a wrong history.
//! A workflow diverges when a step's name differs from the one recorded at
//! its position. `docs/durable.md` gives the `clippy.toml` that forbids the
//! standard library's clock and randomness in workflow modules.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rustnative_server::db::{Db, DbError};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Why a workflow did not finish (yet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowError {
    /// It is waiting for a timer or a signal; the engine resumes it.
    Suspended,
    /// Its replay disagreed with its journal.
    Divergence {
        /// The position in the journal.
        position: u64,
        /// What was recorded there.
        recorded: String,
        /// What the code asked for now.
        requested: String,
    },
    /// It failed (after compensation).
    Failed(String),
}

impl std::fmt::Display for WorkflowError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Suspended => formatter.write_str("waiting"),
            Self::Divergence { position, recorded, requested } => write!(
                formatter,
                "the workflow diverged from its journal at step {position}: recorded `{recorded}`, now `{requested}`"
            ),
            Self::Failed(error) => write!(formatter, "failed: {error}"),
        }
    }
}

impl std::error::Error for WorkflowError {}

impl From<DbError> for WorkflowError {
    fn from(error: DbError) -> Self {
        Self::Failed(error.to_string())
    }
}

fn failed(error: impl std::fmt::Display) -> WorkflowError {
    WorkflowError::Failed(error.to_string())
}

/// A workflow.
#[async_trait::async_trait]
pub trait Workflow: Send + Sync + 'static {
    /// Its stable name.
    const KIND: &'static str;
    /// Its input.
    type Input: Serialize + DeserializeOwned + Send + Sync;
    /// Its output.
    type Output: Serialize + DeserializeOwned + Send;

    /// The workflow.
    ///
    /// # Errors
    ///
    /// Suspension (the engine resumes it), divergence, or failure.
    async fn run(
        &self,
        context: &mut WorkflowContext,
        input: Self::Input,
    ) -> Result<Self::Output, WorkflowError>;
}

type Compensation =
    Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>> + Send>;

/// Everything a workflow may touch.
pub struct WorkflowContext {
    db: Db,
    id: String,
    position: u64,
    compensations: Vec<(String, Compensation)>,
    now: u64,
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
}

fn as_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn recorded(db: &Db, id: &str, position: u64) -> Result<Option<(String, String)>, DbError> {
    let connection = db.get();
    let row = connection.query_row(
        "SELECT name, result FROM _workflow_steps WHERE workflow = ?1 AND position = ?2",
        rusqlite::params![id, as_i64(position)],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    );
    match row {
        Ok(entry) => Ok(Some(entry)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn record(
    connection: &rusqlite::Connection,
    id: &str,
    position: u64,
    name: &str,
    result: &str,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO _workflow_steps (workflow, position, name, result) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![id, as_i64(position), name, result],
    )?;
    Ok(())
}

impl WorkflowContext {
    /// The execution's id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    fn next(&mut self, name: &str) -> Result<(u64, Option<String>), WorkflowError> {
        self.position += 1;
        match recorded(&self.db, &self.id, self.position)? {
            Some((recorded_name, result)) if recorded_name == name => {
                Ok((self.position, Some(result)))
            }
            Some((recorded_name, _)) => Err(WorkflowError::Divergence {
                position: self.position,
                recorded: recorded_name,
                requested: name.to_owned(),
            }),
            None => Ok((self.position, None)),
        }
    }

    fn decode<T: DeserializeOwned>(result: &str) -> Result<T, WorkflowError> {
        serde_json::from_str(result).map_err(|error| failed(format!("a recorded result: {error}")))
    }

    fn encode<T: Serialize>(value: &T) -> Result<String, WorkflowError> {
        serde_json::to_string(value).map_err(failed)
    }

    /// Runs `work` once and records its result. On replay, returns the
    /// recorded result instead. `work` gets an idempotency key unique to
    /// this step of this execution, to hand to whatever it calls. If the
    /// process dies after the call but before the record, the retry repeats
    /// the key.
    ///
    /// # Errors
    ///
    /// `work` failed, or the journal disagrees.
    pub async fn step<T, F, Fut>(&mut self, name: &str, work: F) -> Result<T, WorkflowError>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce(String) -> Fut,
        Fut: Future<Output = Result<T, String>>,
    {
        let (position, result) = self.next(name)?;
        if let Some(result) = result {
            return Self::decode(&result);
        }
        let value = work(format!("{}:{position}", self.id)).await.map_err(WorkflowError::Failed)?;
        let text = Self::encode(&value)?;
        record(&self.db.get(), &self.id, position, name, &text).map_err(failed)?;
        Ok(value)
    }

    /// Runs `work` in a database transaction together with its journal
    /// entry. The effect and the record commit together or not at all, so
    /// the step happens exactly once, whatever the process does.
    ///
    /// # Errors
    ///
    /// `work` failed (nothing is committed), or the journal disagrees.
    pub fn transactional_step<T, F>(&mut self, name: &str, work: F) -> Result<T, WorkflowError>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<T, String>,
    {
        let (position, result) = self.next(name)?;
        if let Some(result) = result {
            return Self::decode(&result);
        }
        let mut connection = self.db.get();
        let transaction = connection.transaction().map_err(failed)?;
        let value = work(&transaction).map_err(WorkflowError::Failed)?;
        let text = Self::encode(&value)?;
        record(&transaction, &self.id, position, name, &text).map_err(failed)?;
        transaction.commit().map_err(failed)?;
        Ok(value)
    }

    /// Registers how to undo the step just completed. If the workflow fails,
    /// compensations run newest first, each once.
    pub fn compensate<F, Fut>(&mut self, name: &str, undo: F)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        self.compensations.push((name.to_owned(), Box::new(move || Box::pin(undo()))));
    }

    /// The time in milliseconds, recorded so it is the same on every replay.
    ///
    /// # Errors
    ///
    /// The journal disagrees.
    pub async fn now(&mut self) -> Result<u64, WorkflowError> {
        let now = self.now;
        self.step("now", |_| async move { Ok(now) }).await
    }

    /// A random number, recorded so it is the same on every replay.
    ///
    /// # Errors
    ///
    /// The journal disagrees.
    pub async fn random(&mut self) -> Result<u64, WorkflowError> {
        use std::hash::{BuildHasher, Hasher};
        let value = std::collections::hash_map::RandomState::new().build_hasher().finish();
        self.step("random", |_| async move { Ok(value) }).await
    }

    /// Waits `duration`, durably. The wake time is recorded, so after a
    /// restart the workflow waits only for the time that is left.
    ///
    /// # Errors
    ///
    /// [`WorkflowError::Suspended`] until the time has come. The engine
    /// resumes the workflow then.
    pub async fn sleep(&mut self, duration: Duration) -> Result<(), WorkflowError> {
        let now = self.now;
        let length = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
        let wake: u64 =
            self.step("sleep", |_| async move { Ok(now.saturating_add(length)) }).await?;
        if self.now < wake {
            self.db
                .get()
                .execute(
                    "UPDATE _workflows SET wake_at = ?2 WHERE id = ?1",
                    rusqlite::params![self.id, as_i64(wake)],
                )
                .map_err(failed)?;
            return Err(WorkflowError::Suspended);
        }
        Ok(())
    }

    /// Waits for the signal `name`, sent with [`LocalEngine::signal`], and
    /// returns its payload.
    ///
    /// # Errors
    ///
    /// [`WorkflowError::Suspended`] until it arrives.
    #[allow(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "waiting is asynchronous in the engine contract; a hosted engine awaits here"
    )]
    pub async fn signal<T: Serialize + DeserializeOwned>(
        &mut self,
        name: &str,
    ) -> Result<T, WorkflowError> {
        let step = format!("signal:{name}");
        let (position, result) = self.next(&step)?;
        if let Some(result) = result {
            return Self::decode(&result);
        }
        let mut connection = self.db.get();
        let transaction = connection.transaction().map_err(failed)?;
        let payload: Option<(i64, String)> = transaction
            .query_row(
                "SELECT rowid, payload FROM _workflow_signals WHERE workflow = ?1 AND name = ?2 ORDER BY rowid LIMIT 1",
                rusqlite::params![self.id, name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();
        let Some((rowid, payload)) = payload else {
            self.position -= 1;
            return Err(WorkflowError::Suspended);
        };
        transaction
            .execute("DELETE FROM _workflow_signals WHERE rowid = ?1", [rowid])
            .map_err(failed)?;
        record(&transaction, &self.id, position, &step, &payload).map_err(failed)?;
        transaction.commit().map_err(failed)?;
        Self::decode(&payload)
    }

    /// Which version of a changed behaviour this execution uses. A new
    /// execution gets `newest`. An execution that was already past this
    /// point when the change shipped keeps `oldest`. The answer is
    /// recorded, so it never changes mid-execution.
    ///
    /// # Errors
    ///
    /// The journal is unreadable.
    pub fn version(
        &mut self,
        change: &str,
        oldest: u32,
        newest: u32,
    ) -> Result<u32, WorkflowError> {
        let connection = self.db.get();
        let marker: Option<u32> = connection
            .query_row(
                "SELECT version FROM _workflow_versions WHERE workflow = ?1 AND change = ?2",
                rusqlite::params![self.id, change],
                |row| row.get(0),
            )
            .ok();
        if let Some(version) = marker {
            return Ok(version);
        }
        // Steps recorded beyond this point mean the execution ran this code
        // before the change existed.
        let later: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM _workflow_steps WHERE workflow = ?1 AND position > ?2",
                rusqlite::params![self.id, as_i64(self.position)],
                |row| row.get(0),
            )
            .map_err(failed)?;
        let version = if later > 0 { oldest } else { newest };
        connection
            .execute(
                "INSERT INTO _workflow_versions (workflow, change, version) VALUES (?1, ?2, ?3)",
                rusqlite::params![self.id, change, version],
            )
            .map_err(failed)?;
        Ok(version)
    }
}

type Outcome = (WorkflowContext, Result<String, WorkflowError>);
type Erased = Arc<
    dyn Fn(WorkflowContext, String) -> Pin<Box<dyn Future<Output = Outcome> + Send>> + Send + Sync,
>;

/// Where an execution is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Running or waiting.
    Running,
    /// Finished, with its output (JSON).
    Completed(String),
    /// Failed (and compensated), with why.
    Failed(String),
}

/// What any workflow engine offers.
#[async_trait::async_trait]
pub trait WorkflowEngine: Send + Sync {
    /// Starts execution `id` of `kind` with `input` (JSON). Starting an id
    /// again does nothing.
    ///
    /// # Errors
    ///
    /// The engine refused.
    fn start_raw(&self, kind: &str, id: &str, input: &str) -> Result<(), WorkflowError>;

    /// Delivers signal `name` to execution `id`.
    ///
    /// # Errors
    ///
    /// The engine refused.
    fn signal_raw(&self, id: &str, name: &str, payload: &str) -> Result<(), WorkflowError>;

    /// Where execution `id` is.
    fn status(&self, id: &str) -> Option<Status>;

    /// Runs everything that can run now.
    async fn run_until_idle(&self);
}

/// The local engine: workflows journaled in SQLite and run in this process.
/// [`WorkflowEngine`] is the contract a hosted engine would implement too.
#[derive(Clone)]
pub struct LocalEngine {
    db: Db,
    workflows: HashMap<&'static str, Erased>,
    clock: Option<Arc<AtomicU64>>,
}

impl LocalEngine {
    /// An engine journaling in `db`.
    ///
    /// # Errors
    ///
    /// The journal tables cannot be created.
    pub fn new(db: Db) -> Result<Self, DbError> {
        db.get().execute_batch(
            "CREATE TABLE IF NOT EXISTS _workflows (
                id TEXT PRIMARY KEY, kind TEXT NOT NULL, input TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'running', output TEXT, wake_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS _workflow_steps (
                workflow TEXT NOT NULL, position INTEGER NOT NULL, name TEXT NOT NULL, result TEXT NOT NULL,
                PRIMARY KEY (workflow, position)
            );
            CREATE TABLE IF NOT EXISTS _workflow_signals (workflow TEXT NOT NULL, name TEXT NOT NULL, payload TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS _workflow_versions (
                workflow TEXT NOT NULL, change TEXT NOT NULL, version INTEGER NOT NULL, PRIMARY KEY (workflow, change)
            );",
        )?;
        Ok(Self { db, workflows: HashMap::new(), clock: None })
    }

    /// Uses `clock` (milliseconds since the epoch) instead of the system's,
    /// for tests of timers.
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<AtomicU64>) -> Self {
        self.clock = Some(clock);
        self
    }

    /// Registers workflow `W`.
    #[must_use]
    pub fn register<W: Workflow>(mut self, workflow: W) -> Self {
        let workflow = Arc::new(workflow);
        let erased: Erased = Arc::new(move |mut context, input| {
            let workflow = Arc::clone(&workflow);
            Box::pin(async move {
                let result = match serde_json::from_str::<W::Input>(&input) {
                    Ok(input) => workflow
                        .run(&mut context, input)
                        .await
                        .and_then(|output| WorkflowContext::encode(&output)),
                    Err(error) => Err(failed(format!("input: {error}"))),
                };
                (context, result)
            })
        });
        self.workflows.insert(W::KIND, erased);
        self
    }

    /// Starts execution `id` of `W`.
    ///
    /// # Errors
    ///
    /// The engine refused.
    pub fn start<W: Workflow>(&self, id: &str, input: &W::Input) -> Result<(), WorkflowError> {
        let input = WorkflowContext::encode(input)?;
        self.start_raw(W::KIND, id, &input)
    }

    /// Sends signal `name` with `payload` to execution `id`.
    ///
    /// # Errors
    ///
    /// The engine refused.
    pub fn signal<T: Serialize>(
        &self,
        id: &str,
        name: &str,
        payload: &T,
    ) -> Result<(), WorkflowError> {
        self.signal_raw(id, name, &WorkflowContext::encode(payload)?)
    }

    fn now(&self) -> u64 {
        self.clock.as_ref().map_or_else(epoch_ms, |clock| clock.load(Ordering::SeqCst))
    }

    async fn run_one(&self, id: String, kind: String, input: String) {
        let Some(workflow) = self.workflows.get(kind.as_str()) else { return };
        let context = WorkflowContext {
            db: self.db.clone(),
            id: id.clone(),
            position: 0,
            compensations: Vec::new(),
            now: self.now(),
        };
        let (mut context, result) = workflow(context, input).await;
        match result {
            Ok(output) => {
                let _ = self.db.get().execute(
                    "UPDATE _workflows SET status = 'completed', output = ?2 WHERE id = ?1",
                    rusqlite::params![id, output],
                );
            }
            Err(WorkflowError::Suspended) => {}
            Err(error) => {
                // Undo what was done, newest first, each once (journaled).
                let mut compensations = std::mem::take(&mut context.compensations);
                compensations.reverse();
                for (name, undo) in compensations {
                    let step = format!("compensate:{name}");
                    let _ = context.step(&step, |_| async move { undo().await }).await;
                }
                let _ = self.db.get().execute(
                    "UPDATE _workflows SET status = 'failed', output = ?2 WHERE id = ?1",
                    rusqlite::params![id, error.to_string()],
                );
            }
        }
    }
}

#[async_trait::async_trait]
impl WorkflowEngine for LocalEngine {
    fn start_raw(&self, kind: &str, id: &str, input: &str) -> Result<(), WorkflowError> {
        self.db
            .get()
            .execute(
                "INSERT OR IGNORE INTO _workflows (id, kind, input) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, kind, input],
            )
            .map_err(failed)?;
        Ok(())
    }

    fn signal_raw(&self, id: &str, name: &str, payload: &str) -> Result<(), WorkflowError> {
        let connection = self.db.get();
        connection
            .execute(
                "INSERT INTO _workflow_signals (workflow, name, payload) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, name, payload],
            )
            .map_err(failed)?;
        connection
            .execute("UPDATE _workflows SET wake_at = 0 WHERE id = ?1", [id])
            .map_err(failed)?;
        Ok(())
    }

    fn status(&self, id: &str) -> Option<Status> {
        self.db
            .get()
            .query_row("SELECT status, output FROM _workflows WHERE id = ?1", [id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .ok()
            .map(|(status, output)| match status.as_str() {
                "completed" => Status::Completed(output.unwrap_or_default()),
                "failed" => Status::Failed(output.unwrap_or_default()),
                _ => Status::Running,
            })
    }

    async fn run_until_idle(&self) {
        let now = as_i64(self.now());
        let runnable: Vec<(String, String, String)> = {
            let connection = self.db.get();
            let Ok(mut statement) = connection.prepare(
                "SELECT id, kind, input FROM _workflows WHERE status = 'running' AND wake_at <= ?1",
            ) else {
                return;
            };
            statement
                .query_map([now], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .map(|rows| rows.filter_map(Result::ok).collect())
                .unwrap_or_default()
        };
        for (id, kind, input) in runnable {
            self.run_one(id, kind, input).await;
        }
    }
}
