//! Durable background jobs and scheduled work.
//!
//! Jobs live in the database (`_jobs`), so a queue survives a restart:
//! a job that was running when the process died is picked up again. Each
//! enqueue may carry an idempotency key — enqueueing the same key twice is
//! one job. A failed job is retried with exponential backoff, and after
//! its last attempt it is kept as `dead` with its error. Recurring work is
//! a [`Schedule`] (a cron subset). [`Jobs::inspect`] answers the
//! inspection protocol's `Jobs` request (Milestone 44).
//!
//! ```no_run
//! # async fn example(db: rustnative_server::db::Db) {
//! use rustnative_server::jobs::{Job, JobContext, Jobs};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct SendWelcome { email: String }
//!
//! #[async_trait::async_trait]
//! impl Job for SendWelcome {
//!     const KIND: &'static str = "send-welcome";
//!     async fn run(&self, _: &JobContext) -> Result<(), String> { Ok(()) }
//! }
//!
//! let jobs = Jobs::new(db).unwrap().register::<SendWelcome>();
//! jobs.enqueue(&SendWelcome { email: "ada@example.com".into() }, Some("welcome:ada")).unwrap();
//! jobs.work_until_idle().await;
//! # }
//! ```

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::db::{Db, DbError};

/// What a job's run can see.
#[derive(Debug, Clone)]
pub struct JobContext {
    /// The database.
    pub db: Db,
    /// Which attempt this is (1 for the first).
    pub attempt: u32,
}

/// A kind of background work.
#[async_trait::async_trait]
pub trait Job: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// Its stable name in the queue.
    const KIND: &'static str;
    /// How many attempts before it is dead.
    const MAX_ATTEMPTS: u32 = 5;

    /// Does the work.
    ///
    /// # Errors
    ///
    /// Why it failed; it is retried.
    async fn run(&self, context: &JobContext) -> Result<(), String>;
}

type Runner = Arc<
    dyn Fn(String, JobContext) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>>
        + Send
        + Sync,
>;

/// A job's state, as inspection reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobRecord {
    /// Its id.
    pub id: i64,
    /// Its kind.
    pub kind: String,
    /// `queued`, `running`, `done`, or `dead`.
    pub status: String,
    /// Attempts made.
    pub attempts: u32,
    /// The last error, if any.
    pub last_error: Option<String>,
}

/// A recurring schedule: five cron fields (minute, hour, day of month,
/// month, day of week), each `*`, `*/n`, a number, or a comma list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    fields: [Vec<u32>; 5],
}

const RANGES: [(u32, u32); 5] = [(0, 59), (0, 23), (1, 31), (1, 12), (0, 6)];

impl Schedule {
    /// Parses a cron expression.
    ///
    /// # Errors
    ///
    /// It is not five valid fields.
    pub fn parse(expression: &str) -> Result<Self, String> {
        let parts = expression.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 5 {
            return Err(format!("{expression:?}: five fields expected"));
        }
        let mut fields: [Vec<u32>; 5] = Default::default();
        for (index, part) in parts.iter().enumerate() {
            let (low, high) = RANGES[index];
            let mut values = Vec::new();
            for item in part.split(',') {
                if item == "*" {
                    values.extend(low..=high);
                } else if let Some(step) = item.strip_prefix("*/") {
                    let step: u32 = step.parse().map_err(|_| format!("{item:?}: a step"))?;
                    values.extend((low..=high).step_by(usize::try_from(step.max(1)).unwrap_or(1)));
                } else {
                    let value: u32 = item.parse().map_err(|_| format!("{item:?}: a number"))?;
                    if !(low..=high).contains(&value) {
                        return Err(format!("{item:?}: out of range"));
                    }
                    values.push(value);
                }
            }
            fields[index] = values;
        }
        Ok(Self { fields })
    }

    /// The first minute after `after` (seconds since the epoch, UTC) the
    /// schedule fires.
    #[must_use]
    pub fn next_after(&self, after: u64) -> u64 {
        // At most four years of minutes; a valid schedule fires long before.
        for minute in (after / 60 + 1..).take(4 * 366 * 24 * 60) {
            let (month, day, hour, minute_of_hour, weekday) = civil(minute * 60);
            let matches = [minute_of_hour, hour, day, month, weekday]
                .iter()
                .zip(&self.fields)
                .all(|(value, allowed)| allowed.contains(value));
            if matches {
                return minute * 60;
            }
        }
        u64::MAX
    }
}

/// Month, day, hour, minute, and weekday (0 = Sunday) of a Unix time, UTC.
fn civil(seconds: u64) -> (u32, u32, u32, u32, u32) {
    let days = seconds / 86_400;
    let rem = seconds % 86_400;
    let (hour, minute) =
        (u32::try_from(rem / 3600).unwrap_or(0), u32::try_from(rem % 3600 / 60).unwrap_or(0));
    let weekday = u32::try_from((days + 4) % 7).unwrap_or(0);
    // Howard Hinnant's days-to-civil.
    let z = i64::try_from(days).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    (month, day, hour, minute, weekday)
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

/// The queue.
#[derive(Clone)]
pub struct Jobs {
    db: Db,
    runners: HashMap<&'static str, (Runner, u32)>,
    schedules: Vec<(&'static str, Schedule, String)>,
}

impl Jobs {
    /// The queue in `db`; jobs left running by a previous process are
    /// queued again.
    ///
    /// # Errors
    ///
    /// The table cannot be created.
    pub fn new(db: Db) -> Result<Self, DbError> {
        let connection = db.get();
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS _jobs (
                id INTEGER PRIMARY KEY,
                kind TEXT NOT NULL,
                payload TEXT NOT NULL,
                idempotency_key TEXT UNIQUE,
                attempts INTEGER NOT NULL DEFAULT 0,
                max_attempts INTEGER NOT NULL,
                run_at INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'queued',
                last_error TEXT
            );
            UPDATE _jobs SET status = 'queued' WHERE status = 'running';",
        )?;
        drop(connection);
        Ok(Self { db, runners: HashMap::new(), schedules: Vec::new() })
    }

    /// Registers the job kind `J`.
    #[must_use]
    pub fn register<J: Job>(mut self) -> Self {
        let runner: Runner = Arc::new(|payload, context| {
            Box::pin(async move {
                let job: J = serde_json::from_str(&payload).map_err(|error| error.to_string())?;
                job.run(&context).await
            })
        });
        self.runners.insert(J::KIND, (runner, J::MAX_ATTEMPTS));
        self
    }

    /// Runs `job` on `schedule`: each time the worker passes a firing
    /// time, one instance is enqueued (keyed by the time, so two workers
    /// enqueue it once).
    #[must_use]
    pub fn every<J: Job>(mut self, schedule: Schedule, job: &J) -> Self {
        let payload = serde_json::to_string(job).unwrap_or_default();
        self.schedules.push((J::KIND, schedule, payload));
        self
    }

    /// Enqueues `job`; with an `idempotency_key` already used, does
    /// nothing. Returns whether a job was added.
    ///
    /// # Errors
    ///
    /// SQLite refused, or the kind is not registered.
    pub fn enqueue<J: Job>(&self, job: &J, idempotency_key: Option<&str>) -> Result<bool, DbError> {
        self.enqueue_at(
            J::KIND,
            &serde_json::to_string(job).unwrap_or_default(),
            idempotency_key,
            now(),
        )
    }

    fn enqueue_at(
        &self,
        kind: &str,
        payload: &str,
        key: Option<&str>,
        at: u64,
    ) -> Result<bool, DbError> {
        let max = self.runners.get(kind).map_or(5, |(_, max)| *max);
        let added = self.db.get().execute(
            "INSERT OR IGNORE INTO _jobs (kind, payload, idempotency_key, max_attempts, run_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![kind, payload, key, max, i64::try_from(at).unwrap_or(i64::MAX)],
        )?;
        Ok(added == 1)
    }

    /// Enqueues any scheduled work due by `at`.
    fn enqueue_scheduled(&self, at: u64) -> Result<(), DbError> {
        for (kind, schedule, payload) in &self.schedules {
            let due = schedule.next_after(at.saturating_sub(60));
            if due <= at {
                self.enqueue_at(kind, payload, Some(&format!("schedule:{kind}:{due}")), due)?;
            }
        }
        Ok(())
    }

    /// Claims and runs one due job; `false` when none was due.
    ///
    /// # Errors
    ///
    /// SQLite refused.
    pub async fn work_one(&self) -> Result<bool, DbError> {
        self.enqueue_scheduled(now())?;
        let claimed = {
            let connection = self.db.get();
            let row = connection
                .query_row(
                    "SELECT id, kind, payload, attempts FROM _jobs WHERE status = 'queued' AND run_at <= ?1 ORDER BY run_at, id LIMIT 1",
                    [i64::try_from(now()).unwrap_or(i64::MAX)],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, u32>(3)?)),
                )
                .ok();
            match row {
                Some(row) => {
                    let updated = connection.execute(
                        "UPDATE _jobs SET status = 'running', attempts = attempts + 1 WHERE id = ?1 AND status = 'queued'",
                        [row.0],
                    )?;
                    (updated == 1).then_some(row)
                }
                None => None,
            }
        };
        let Some((id, kind, payload, attempts)) = claimed else { return Ok(false) };
        let attempt = attempts + 1;
        let result = match self.runners.get(kind.as_str()) {
            Some((runner, _)) => runner(payload, JobContext { db: self.db.clone(), attempt }).await,
            None => Err(format!("no job kind {kind:?} is registered")),
        };
        let connection = self.db.get();
        match result {
            Ok(()) => {
                connection.execute(
                    "UPDATE _jobs SET status = 'done', last_error = NULL WHERE id = ?1",
                    [id],
                )?;
            }
            Err(error) => {
                let max: u32 = connection.query_row(
                    "SELECT max_attempts FROM _jobs WHERE id = ?1",
                    [id],
                    |row| row.get(0),
                )?;
                if attempt >= max {
                    connection.execute(
                        "UPDATE _jobs SET status = 'dead', last_error = ?2 WHERE id = ?1",
                        rusqlite::params![id, error],
                    )?;
                } else {
                    let backoff = Self::backoff(attempt).as_secs();
                    connection.execute(
                        "UPDATE _jobs SET status = 'queued', last_error = ?2, run_at = ?3 WHERE id = ?1",
                        rusqlite::params![id, error, i64::try_from(now() + backoff).unwrap_or(i64::MAX)],
                    )?;
                }
            }
        }
        Ok(true)
    }

    /// The wait before attempt `attempt + 1`: 2, 4, 8, … seconds, capped at
    /// an hour.
    #[must_use]
    pub fn backoff(attempt: u32) -> Duration {
        Duration::from_secs(2u64.saturating_pow(attempt).min(3600))
    }

    /// Runs due jobs until none is due.
    pub async fn work_until_idle(&self) {
        while matches!(self.work_one().await, Ok(true)) {}
    }

    /// Works until `shutdown`, polling every `interval` when idle.
    pub async fn run(&self, interval: Duration, shutdown: impl Future<Output = ()>) {
        let mut shutdown = std::pin::pin!(shutdown);
        loop {
            self.work_until_idle().await;
            tokio::select! {
                () = tokio::time::sleep(interval) => {}
                () = &mut shutdown => return,
            }
        }
    }

    /// Every job, newest first: what the inspection protocol's `Jobs`
    /// request returns.
    ///
    /// # Errors
    ///
    /// SQLite refused.
    pub fn inspect(&self) -> Result<Vec<JobRecord>, DbError> {
        let connection = self.db.get();
        let mut statement = connection.prepare(
            "SELECT id, kind, status, attempts, last_error FROM _jobs ORDER BY id DESC LIMIT 500",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(JobRecord {
                id: row.get(0)?,
                kind: row.get(1)?,
                status: row.get(2)?,
                attempts: row.get(3)?,
                last_error: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

/// Answers one inspection request (Milestone 44's protocol) for a server:
/// `Jobs` with the queue, the rest with why a server does not answer them.
#[must_use]
pub fn answer(
    jobs: &Jobs,
    request: &rustnative_core::inspect::Request,
) -> rustnative_core::inspect::Reply {
    use rustnative_core::inspect::{Reply, Request};
    match request {
        Request::Jobs => match jobs.inspect() {
            Ok(records) => serde_json::to_value(records)
                .map_or_else(|error| Reply::Error(error.to_string()), Reply::Ok),
            Err(error) => Reply::Error(error.to_string()),
        },
        _ => Reply::Error("a server answers `jobs`; ask the application for the rest".into()),
    }
}

impl crate::ServerApp {
    /// Serves the inspection protocol at `POST /__inspect`, to principals
    /// `Pol` allows: a request as JSON in, the reply as JSON out.
    #[must_use]
    pub fn inspection<P, Pol>(self, jobs: Jobs) -> Self
    where
        P: Clone + Send + Sync + 'static,
        Pol: crate::auth::Policy<P>,
    {
        let handler =
            move |crate::Json(request): crate::Json<rustnative_core::inspect::Request>| {
                let jobs = jobs.clone();
                async move { crate::Json(answer(&jobs, &request)) }
            };
        self.route("/__inspect", crate::post(handler).authorized::<P, Pol>())
    }
}
