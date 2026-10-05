//! The data layer and jobs (`PLAN.md` Milestone 49): pooled connections,
//! transactions tied to request scopes, migrations up, down, and dry,
//! migrations generated from the model, row policies, and durable jobs.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::sync::atomic::{AtomicU32, Ordering};

use rustnative_server::RequestScope;
use rustnative_server::db::migrate::{Migration, Migrations};
use rustnative_server::db::policy::RowPolicies;
use rustnative_server::db::schema::{PossibleRename, Schema, diff, squash};
use rustnative_server::db::{Db, DbError};
use rustnative_server::jobs::{Job, JobContext, Jobs, Schedule};
use serde::{Deserialize, Serialize};

fn migrations() -> Migrations {
    Migrations(vec![
        Migration {
            name: "0001_users".into(),
            up: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);".into(),
            down: "DROP TABLE users;".into(),
        },
        Migration {
            name: "0002_notes".into(),
            up: "CREATE TABLE notes (id INTEGER PRIMARY KEY, owner INTEGER NOT NULL, title TEXT NOT NULL);".into(),
            down: "DROP TABLE notes;".into(),
        },
    ])
}

fn database(name: &str) -> Db {
    let db = Db::memory(name, 2).unwrap();
    migrations().apply(&mut db.get()).unwrap();
    db
}

#[test]
fn migrations_apply_roll_back_and_dry_run() {
    let db = Db::memory("migrate", 1).unwrap();
    let mut connection = db.get();
    let all = migrations();
    let pending: Vec<_> =
        all.pending(&connection).unwrap().into_iter().map(|m| m.name.clone()).collect();
    assert_eq!(pending, ["0001_users", "0002_notes"], "the dry run lists, and applies nothing");
    assert_eq!(Migrations::applied(&connection).unwrap(), [] as [&str; 0]);
    assert_eq!(all.apply(&mut connection).unwrap().len(), 2);
    assert!(all.apply(&mut connection).unwrap().is_empty(), "applying twice is nothing");
    assert_eq!(all.rollback(&mut connection, Some("0001_users")).unwrap(), ["0002_notes"]);
    assert!(connection.prepare("SELECT * FROM notes").is_err());
    assert_eq!(all.rollback(&mut connection, None).unwrap(), ["0001_users"]);
}

#[tokio::test]
async fn a_transaction_ends_with_its_request() {
    let db = database("transactions");
    let scope = RequestScope::default();
    db.transaction(&scope, |transaction| {
        transaction.execute("INSERT INTO users (name) VALUES ('Ada')", [])?;
        Ok(())
    })
    .await
    .unwrap();

    // The request finished (the server cancels its scope) before commit.
    let ended = RequestScope::default();
    ended.cancel();
    let result = db
        .transaction(&ended, |transaction| {
            transaction.execute("INSERT INTO users (name) VALUES ('Grace')", [])?;
            Ok(())
        })
        .await;
    assert!(matches!(result, Err(DbError::Cancelled)));
    let count: i64 = db
        .run(|connection| {
            Ok(connection.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?)
        })
        .await
        .unwrap();
    assert_eq!(count, 1, "only the live request's write committed");
}

#[test]
fn the_next_migration_comes_from_the_model() {
    let current = Schema::from_migrations(&migrations()).unwrap();
    let desired = Schema::parse(
        r#"
        [tables.users]
        columns = [
          { name = "id", type = "INTEGER", primary_key = true },
          { name = "display_name", type = "TEXT", not_null = true, default = "''" },
          { name = "email", type = "TEXT" },
        ]
        [tables.tags]
        columns = [{ name = "id", type = "INTEGER", primary_key = true }, { name = "label", type = "TEXT" }]
        "#,
    )
    .unwrap();
    let plan = diff(&current, &desired, &[]);
    assert_eq!(
        plan.possible_renames,
        [PossibleRename { table: "users".into(), from: "name".into(), to: "display_name".into() }],
        "a dropped and an added column of one type: perhaps a rename"
    );
    assert!(plan.up.contains("CREATE TABLE tags") && plan.up.contains("DROP TABLE notes"));

    let plan = diff(&current, &desired, &plan.possible_renames);
    assert!(plan.up.contains("RENAME COLUMN name TO display_name"), "{}", plan.up);
    assert!(!plan.up.contains("DROP COLUMN name"));

    // The generated migration applies and reverses cleanly.
    let mut with_next = migrations();
    with_next.0.push(Migration {
        name: "0003_next".into(),
        up: plan.up.clone(),
        down: plan.down.clone(),
    });
    let db = Db::memory("generated", 1).unwrap();
    let mut connection = db.get();
    with_next.apply(&mut connection).unwrap();
    assert_eq!(
        Schema::of(&connection).unwrap().tables.keys().collect::<Vec<_>>(),
        ["tags", "users"]
    );
    with_next.rollback(&mut connection, Some("0002_notes")).unwrap();
    assert_eq!(Schema::of(&connection).unwrap(), current);

    let squashed = squash(&with_next, "0001_squashed").unwrap();
    assert!(
        squashed.up.contains("CREATE TABLE tags") && squashed.up.contains("CREATE TABLE users")
    );
}

struct Reader {
    id: i64,
    admin: bool,
}

#[test]
fn row_policies_hold_for_queries_and_single_rows() {
    let db = database("policies");
    let connection = db.get();
    connection
        .execute_batch(
            "INSERT INTO notes (owner, title) VALUES (1, 'mine'), (2, 'theirs'), (1, 'also mine');",
        )
        .unwrap();
    let policies = RowPolicies::<Reader>::new().table("notes", |reader| {
        if reader.admin {
            ("1 = 1".into(), vec![])
        } else {
            ("owner = ?".into(), vec![reader.id.into()])
        }
    });
    let titles = |reader: &Reader| {
        policies
            .visible(&connection, reader, "notes", "title")
            .unwrap()
            .into_iter()
            .map(|value| match value {
                rusqlite::types::Value::Text(text) => text,
                other => format!("{other:?}"),
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(titles(&Reader { id: 1, admin: false }), ["mine", "also mine"]);
    assert_eq!(titles(&Reader { id: 3, admin: true }).len(), 3);
    assert!(!policies.may_read(&connection, &Reader { id: 1, admin: false }, "notes", 2).unwrap());
    assert!(policies.may_read(&connection, &Reader { id: 2, admin: false }, "notes", 2).unwrap());
    assert_eq!(
        policies.visible(&connection, &Reader { id: 1, admin: true }, "users", "name").unwrap(),
        []
    );
}

static RUNS: AtomicU32 = AtomicU32::new(0);
static FLAKY: AtomicU32 = AtomicU32::new(0);

#[derive(Serialize, Deserialize)]
struct Welcome {
    email: String,
}

#[async_trait::async_trait]
impl Job for Welcome {
    const KIND: &'static str = "welcome";
    async fn run(&self, _: &JobContext) -> Result<(), String> {
        RUNS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct Flaky;

#[async_trait::async_trait]
impl Job for Flaky {
    const KIND: &'static str = "flaky";
    const MAX_ATTEMPTS: u32 = 2;
    async fn run(&self, context: &JobContext) -> Result<(), String> {
        FLAKY.fetch_add(1, Ordering::SeqCst);
        Err(format!("attempt {} failed", context.attempt))
    }
}

#[tokio::test]
async fn jobs_are_durable_idempotent_and_retried() {
    let path = std::env::temp_dir().join(format!("rustnative-jobs-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    {
        let db = Db::open(path.to_str().unwrap(), 1).unwrap();
        let jobs = Jobs::new(db).unwrap().register::<Welcome>();
        assert!(
            jobs.enqueue(&Welcome { email: "ada@example.com".into() }, Some("welcome:ada"))
                .unwrap()
        );
        assert!(
            !jobs
                .enqueue(&Welcome { email: "ada@example.com".into() }, Some("welcome:ada"))
                .unwrap()
        );
        // The process stops before working.
    }
    let db = Db::open(path.to_str().unwrap(), 1).unwrap();
    let jobs = Jobs::new(db.clone()).unwrap().register::<Welcome>().register::<Flaky>();
    jobs.work_until_idle().await;
    assert_eq!(RUNS.load(Ordering::SeqCst), 1, "survived the restart, ran once");

    jobs.enqueue(&Flaky, None).unwrap();
    jobs.work_until_idle().await;
    let flaky = jobs.inspect().unwrap().into_iter().find(|job| job.kind == "flaky").unwrap();
    assert_eq!((flaky.status.as_str(), flaky.attempts), ("queued", 1), "backing off");
    db.get().execute("UPDATE _jobs SET run_at = 0 WHERE kind = 'flaky'", []).unwrap();
    jobs.work_until_idle().await;
    let flaky = jobs.inspect().unwrap().into_iter().find(|job| job.kind == "flaky").unwrap();
    assert_eq!(flaky.status, "dead");
    assert_eq!(flaky.last_error.as_deref(), Some("attempt 2 failed"));
    assert_eq!(Jobs::backoff(3).as_secs(), 8);
    drop(jobs);
    drop(db);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn schedules_fire_on_their_minutes() {
    let every_quarter = Schedule::parse("*/15 * * * *").unwrap();
    // 2026-09-25 10:07:00 UTC.
    let at = 1_790_330_820;
    assert_eq!(every_quarter.next_after(at) - at, 8 * 60);
    let nightly = Schedule::parse("30 2 * * *").unwrap();
    assert_eq!(nightly.next_after(at) % 86_400, 2 * 3600 + 30 * 60);
    assert!(Schedule::parse("61 * * * *").is_err());
}
