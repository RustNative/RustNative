//! Durable and event-driven execution (`PLAN.md` Milestone 56). The tests
//! cover workflow timers, signals, approvals, compensation, versioning, and
//! divergence; event batches with partial failures; actors; supervision;
//! and operations across the client/server boundary.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustnative_core::SupervisionPolicy;
use rustnative_durable::operations::{RemoteState, cancel, follow};
use rustnative_durable::{
    Actor, ActorContext, BatchResult, EventEnvelope, EventHandler, EventRunner, LocalActorSystem,
    LocalEngine, Operations, Status, Storage, WorkerEvent, Workflow, WorkflowContext,
    WorkflowEngine, WorkflowError, supervise,
};
use rustnative_server::RequestScope;
use rustnative_server::db::Db;
use rustnative_server::local::InProcess;
use serde_json::json;

// --- Workflows -------------------------------------------------------------

static CHARGED: AtomicU32 = AtomicU32::new(0);
static REFUNDED: AtomicU32 = AtomicU32::new(0);

/// Waits a day, asks a person, charges, and fails at shipping when told to.
struct Approval;

#[async_trait::async_trait]
impl Workflow for Approval {
    const KIND: &'static str = "approval";
    type Input = bool;
    type Output = String;

    async fn run(
        &self,
        context: &mut WorkflowContext,
        fail_shipping: bool,
    ) -> Result<String, WorkflowError> {
        context.sleep(Duration::from_secs(86_400)).await?;
        let approved: bool = context.signal("approve").await?;
        if !approved {
            return Ok("declined".into());
        }
        context
            .step("charge", |key| async move {
                CHARGED.fetch_add(1, Ordering::SeqCst);
                Ok(key)
            })
            .await?;
        context.compensate("refund", || async {
            REFUNDED.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        let fee = if context.version("fees", 1, 2)? == 2 { 5 } else { 0 };
        context
            .step("ship", |_| async move {
                if fail_shipping { Err("the courier refused".to_owned()) } else { Ok(fee) }
            })
            .await?;
        Ok(format!("shipped with fee {fee}"))
    }
}

#[tokio::test]
async fn timers_signals_and_compensation_are_durable() {
    let clock = Arc::new(AtomicU64::new(1_000_000));
    let db = Db::memory("workflow-approval", 2).unwrap();
    let engine = LocalEngine::new(db).unwrap().with_clock(Arc::clone(&clock)).register(Approval);
    engine.start::<Approval>("a", &false).unwrap();
    engine.start::<Approval>("b", &true).unwrap();
    engine.run_until_idle().await;
    assert_eq!(engine.status("a"), Some(Status::Running), "sleeping");

    clock.fetch_add(86_400_000, Ordering::SeqCst);
    engine.run_until_idle().await;
    assert_eq!(engine.status("a"), Some(Status::Running), "waiting for approval");
    engine.signal("a", "approve", &true).unwrap();
    engine.signal("b", "approve", &true).unwrap();
    engine.run_until_idle().await;
    assert_eq!(engine.status("a"), Some(Status::Completed("\"shipped with fee 5\"".into())));
    let Some(Status::Failed(why)) = engine.status("b") else { panic!("b should have failed") };
    assert!(why.contains("courier"), "{why}");
    assert_eq!(CHARGED.load(Ordering::SeqCst), 2);
    assert_eq!(REFUNDED.load(Ordering::SeqCst), 1, "the failed order's charge was undone");

    // Replaying a finished execution changes nothing.
    engine.run_until_idle().await;
    assert_eq!(CHARGED.load(Ordering::SeqCst), 2);
}

/// The same kind of workflow with a changed step order. A replay must
/// notice.
struct Changed;

#[async_trait::async_trait]
impl Workflow for Changed {
    const KIND: &'static str = "approval";
    type Input = bool;
    type Output = String;
    async fn run(&self, context: &mut WorkflowContext, _: bool) -> Result<String, WorkflowError> {
        context.step("audit", |_| async { Ok(()) }).await?;
        Ok("never".into())
    }
}

#[tokio::test]
async fn a_diverging_replay_fails_loudly() {
    let clock = Arc::new(AtomicU64::new(0));
    let db = Db::memory("workflow-divergence", 1).unwrap();
    let first =
        LocalEngine::new(db.clone()).unwrap().with_clock(Arc::clone(&clock)).register(Approval);
    first.start::<Approval>("c", &false).unwrap();
    first.run_until_idle().await;
    // Its timer is due, so the changed code replays it.
    clock.fetch_add(86_400_000, Ordering::SeqCst);
    let changed = LocalEngine::new(db).unwrap().with_clock(clock).register(Changed);
    changed.run_until_idle().await;
    let Some(Status::Failed(why)) = changed.status("c") else {
        panic!("the divergence should fail it")
    };
    assert!(why.contains("diverged") && why.contains("sleep") && why.contains("audit"), "{why}");
}

// --- Events ----------------------------------------------------------------

struct Orders {
    seen: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl EventHandler for Orders {
    async fn handle(&self, batch: &[EventEnvelope], _: &RequestScope) -> BatchResult {
        let mut result = BatchResult::default();
        for event in batch {
            self.seen.lock().unwrap().push(format!("{}#{}", event.id, event.attempt));
            if event.data["poison"] == true {
                result.failed.push((event.id.clone(), "cannot parse the address".into()));
            }
        }
        result
    }
}

#[tokio::test]
async fn a_batch_with_partial_failures_is_settled_per_event() {
    let handler = Arc::new(Orders { seen: Mutex::new(Vec::new()) });
    let runner =
        EventRunner::new(Db::memory("events", 1).unwrap(), handler.clone(), 10, 3).unwrap();
    for (id, poison) in [("e1", false), ("e2", true), ("e3", false)] {
        let event = EventEnvelope::new(id, "shop", "order.placed", json!({ "poison": poison }));
        assert!(runner.publish(&event).unwrap());
    }
    let duplicate = EventEnvelope::new("e1", "shop", "order.placed", json!({}));
    assert!(!runner.publish(&duplicate).unwrap(), "a duplicate");
    runner.run_until_idle().await.unwrap();
    for _ in 0..2 {
        runner.expedite().unwrap();
        runner.run_until_idle().await.unwrap();
    }
    let seen = handler.seen.lock().unwrap().clone();
    assert_eq!(seen, ["e1#1", "e2#1", "e3#1", "e2#2", "e2#3"], "only the failed event was retried");
    let dead = runner.dead_letters().unwrap();
    assert_eq!(dead.len(), 1);
    assert_eq!((dead[0].0.id.as_str(), dead[0].1.as_str()), ("e2", "cannot parse the address"));
}

// --- Actors: a collaborative session ----------------------------------------

struct Document {
    text: String,
    editors: Vec<String>,
    reminders: u32,
}

enum Edit {
    Join(String),
    Append(String, String),
    Read,
}

#[async_trait::async_trait]
impl Actor for Document {
    type Message = Edit;
    type Reply = (String, Vec<String>, u32);

    fn start(_: &str, storage: &Storage) -> Self {
        Self {
            text: storage.get("text").unwrap_or_default(),
            editors: storage.get("editors").unwrap_or_default(),
            reminders: storage.get("reminders").unwrap_or_default(),
        }
    }

    async fn handle(&mut self, edit: Edit, context: &ActorContext) -> Self::Reply {
        match edit {
            Edit::Join(who) => {
                if !self.editors.contains(&who) {
                    self.editors.push(who);
                }
                context.storage().put("editors", &self.editors).unwrap();
                context.set_alarm(Duration::ZERO).unwrap();
            }
            Edit::Append(who, words) => {
                // Serialized: a read-modify-write that cannot lose an update.
                let current = self.text.clone();
                tokio::task::yield_now().await;
                self.text = format!("{current}{who}: {words}\n");
                context.storage().put("text", &self.text).unwrap();
            }
            Edit::Read => {}
        }
        (self.text.clone(), self.editors.clone(), self.reminders)
    }

    async fn alarm(&mut self, context: &ActorContext) {
        self.reminders += 1;
        context.storage().put("reminders", &self.reminders).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_actor_backed_session_serializes_and_persists() {
    let db = Db::memory("actors", 2).unwrap();
    let system = LocalActorSystem::<Document>::new(db, Duration::from_millis(100)).unwrap();
    system.ask("doc-1", Edit::Join("ada".into())).await.unwrap();
    system.ask("doc-1", Edit::Join("grace".into())).await.unwrap();
    let writers = (0..20).map(|index| {
        let system = system.clone();
        let who = if index % 2 == 0 { "ada" } else { "grace" };
        tokio::spawn(async move {
            system.ask("doc-1", Edit::Append(who.into(), format!("line {index}"))).await
        })
    });
    for writer in writers {
        writer.await.unwrap().unwrap();
    }
    let (text, editors, _) = system.ask("doc-1", Edit::Read).await.unwrap();
    assert_eq!(text.lines().count(), 20, "no lost update");
    assert_eq!(editors, ["ada", "grace"]);

    assert_eq!(system.fire_alarms().unwrap(), 1);
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(system.running(), 0, "evicted when idle");
    let (again, _, reminders) = system.ask("doc-1", Edit::Read).await.unwrap();
    assert_eq!(again, text, "a new instance finds its storage");
    assert_eq!(reminders, 1, "the alarm fired once");
}

// --- Supervision and operations ---------------------------------------------

#[tokio::test]
async fn a_failing_worker_is_restarted_by_its_policy() {
    let tries = Arc::new(AtomicU32::new(0));
    let policy = SupervisionPolicy::RestartWithBackoff {
        initial: Duration::from_millis(5),
        max: Duration::from_millis(20),
        attempts: 3,
    };
    let counter = Arc::clone(&tries);
    let history = supervise(policy, move || {
        let tries = Arc::clone(&counter);
        async move {
            match tries.fetch_add(1, Ordering::SeqCst) {
                0 => Err("the disk was busy".to_owned()),
                1 => panic!("a bug"),
                _ => Ok(()),
            }
        }
    })
    .await;
    assert_eq!(
        history,
        [
            WorkerEvent::Started(1),
            WorkerEvent::Failed("the disk was busy".into()),
            WorkerEvent::Started(2),
            WorkerEvent::Failed("a bug".into()),
            WorkerEvent::Started(3),
            WorkerEvent::Finished,
        ]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_follows_and_cancels_a_server_operation() {
    let operations = Operations::new();
    let service = operations
        .mount(rustnative_server::ServerApp::new(), |router| router.csrf_exempt().public())
        .into_service();
    let http = InProcess(service);
    let base = "https://app.example.com";
    let finishing = operations.start("Export", |reporter| async move {
        for percent in [25, 50, 75] {
            reporter.report(percent);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok(json!("export.zip"))
    });
    let mut seen = Vec::new();
    let last = follow(&http, base, finishing, &[], Duration::from_millis(5), |state| {
        seen.push(state.clone());
    })
    .await
    .unwrap();
    assert_eq!(last, RemoteState::Succeeded { result: json!("export.zip") });
    assert!(
        seen.iter().any(|state| matches!(state, RemoteState::Running { progress: Some(_), .. })),
        "progress crossed over"
    );

    let endless = operations.start("Reindex", |reporter| async move {
        loop {
            reporter.report("working");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
    cancel(&http, base, endless, &[]).await.unwrap();
    let last = follow(&http, base, endless, &[], Duration::from_millis(5), |_| {}).await.unwrap();
    assert_eq!(last, RemoteState::Cancelled);
}
