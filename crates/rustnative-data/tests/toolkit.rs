//! Forms, migrations, tables, HTTP interceptors, background work,
//! operations, state machines, and images (`PLAN.md` Milestone 47).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use rustnative_core::{
    Background, Component, ComponentContext, ComponentTree, Event, HttpRequest, HttpResponse,
    HttpService, ManualExecutor, MemoryStateStore, Node, Scheduler, ServiceError, Services,
    StateStore, Theme,
};
use rustnative_data::{
    BackgroundWork, BearerAuth, Changeset, Constraints, Endpoint, Field, FixedConditions, Form,
    HttpClient, ImageDecoder, LocalTable, Logging, MachineState, Migrations, Operation,
    OperationState, PagingSource, PortableDecoder, QueryError, ResponseCache, Row, Rule, Schema,
    StateMachine, Submission, SubmitError, VersionedStore,
};
use serde::{Deserialize, Serialize};

/// A tree whose only job is to hand out its background, on virtual time.
struct Host;
impl Component for Host {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("host", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        BACKGROUND.with(|slot| *slot.borrow_mut() = Some(context.background()));
        self.view()
    }
}

thread_local! {
    static BACKGROUND: RefCell<Option<Background>> = const { RefCell::new(None) };
}

fn host() -> (ManualExecutor, ComponentTree, Background) {
    let executor = ManualExecutor::new();
    let tree = ComponentTree::with_scheduler(
        Host,
        Services::default(),
        Theme::default(),
        Scheduler::with_executor(Arc::new(executor.clone())),
    );
    let background = BACKGROUND.with(|slot| slot.borrow().clone().unwrap());
    (executor, tree, background)
}

fn settle(executor: &ManualExecutor, tree: &mut ComponentTree) {
    for _ in 0..10 {
        executor.run_until_stalled();
        tree.pump_tasks();
    }
}

// ---------------------------------------------------------------------
// Forms
// ---------------------------------------------------------------------

const NAME: Field<String> = Field::text("name");
const EMAIL: Field<String> = Field::text("email");
const AGE: Field<i64> = Field::integer("age");

fn schema() -> Schema {
    Schema::new()
        .field(NAME, [Rule::Required, Rule::MinLength(2)])
        .field(EMAIL, [Rule::Required, Rule::Email])
        .field(AGE, [Rule::Range(13, 130)])
        .constraint("users_email_key", EMAIL, "is already registered")
}

#[test]
fn the_same_schema_checks_a_changeset_and_raw_server_input() {
    let schema = schema();
    let mut changes = Changeset::new(&schema);
    assert!(!changes.is_dirty());
    changes.set(NAME, "A");
    changes.set(EMAIL, "not-an-email");
    changes.set(AGE, "twelve");
    assert!(changes.is_dirty() && changes.field_dirty(NAME) && changes.raw(NAME) == "A");
    let errors = changes.validate().unwrap_err();
    let codes =
        |field| errors.get(field).unwrap().iter().map(|e| e.code.clone()).collect::<Vec<_>>();
    assert_eq!(codes("name"), ["form-too-short"]);
    assert_eq!(codes("email"), ["form-email"]);
    assert_eq!(codes("age"), ["form-not-a-number"]);

    // The server receives the raw values and checks them with the same
    // schema: the same errors, the same messages.
    assert_eq!(schema.check(changes.raw_values()).unwrap_err(), errors);

    changes.set(NAME, "Ada");
    changes.set(EMAIL, "ada@example.com");
    changes.set(AGE, "36");
    let valid = changes.validate().unwrap();
    assert_eq!((valid.get(NAME), valid.get(AGE)), ("Ada".to_owned(), 36));
}

#[test]
fn a_submission_maps_a_storage_violation_back_to_its_field() {
    let schema = schema();
    let mut form = Form::new(&schema);
    form.changes.set(NAME, "Ada");
    form.changes.set(EMAIL, "ada@example.com");
    assert!(form.submit().unwrap().is_ok());
    assert_eq!(form.submission(), &Submission::Submitting);
    assert!(form.submit().is_none(), "one submission at a time");
    form.finish(Err(SubmitError::Constraint("users_email_key".into())));
    assert_eq!(form.changes.errors().get("email").unwrap()[0].message, "is already registered");
    assert!(matches!(form.submission(), Submission::Failed(_)));
}

// ---------------------------------------------------------------------
// Migrations
// ---------------------------------------------------------------------

fn migrations() -> Migrations {
    Migrations::new().step(
        2,
        |value| Ok(serde_json::json!({ "count": value, "unit": "items" })),
        |value| Ok(value["count"].clone()),
    )
}

#[test]
fn a_versioned_store_migrates_what_an_earlier_version_persisted() {
    let raw = Arc::new(MemoryStateStore::new());
    raw.save("counter", b"41").unwrap(); // written before versioning: version 1
    let dry = migrations().dry_run(&*raw, &["counter", "missing"], 2);
    assert_eq!(dry.len(), 1);
    assert_eq!(dry[0].outcome.as_ref().unwrap()["count"], 41);
    assert_eq!(raw.load("counter").unwrap().unwrap(), b"41", "a dry run writes nothing");

    let store = VersionedStore::new(raw.clone(), migrations());
    let loaded: serde_json::Value =
        serde_json::from_slice(&store.load("counter").unwrap().unwrap()).unwrap();
    assert_eq!(loaded, serde_json::json!({ "count": 41, "unit": "items" }));
    store.save("counter", br#"{"count":42,"unit":"items"}"#).unwrap();
    let raw_text = String::from_utf8(raw.load("counter").unwrap().unwrap()).unwrap();
    assert!(raw_text.contains("\"$version\":2"), "{raw_text}");

    // Downgrading moves it back down.
    assert_eq!(migrations().migrate(loaded, 2, 1).unwrap(), 41);
}

// ---------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Message {
    id: u32,
    text: String,
    read: bool,
}

impl Row for Message {
    fn id(&self) -> String {
        format!("{:06}", self.id)
    }
}

#[test]
fn a_paging_source_fills_only_the_gaps_and_the_table_persists() {
    let (executor, mut tree, background) = host();
    let store = Arc::new(MemoryStateStore::new());
    let table = LocalTable::<Message>::persisted("inbox", store.clone(), "inbox");
    let fetched = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&fetched);
    let source = PagingSource::new(table.clone(), 10, &background, move |page| {
        log.lock().push(page);
        async move {
            let start = u32::try_from(page * 10).unwrap();
            Ok::<_, QueryError>(
                (start..start + 10)
                    .map(|id| Message { id, text: format!("#{id}"), read: false })
                    .collect(),
            )
        }
    });
    source.ensure(0..15);
    source.ensure(5..12); // already loading: nothing new
    settle(&executor, &mut tree);
    source.ensure(8..25);
    settle(&executor, &mut tree);
    assert_eq!(*fetched.lock(), [0, 1, 2], "each page once");
    assert_eq!(table.len(), 30);

    let reopened = LocalTable::<Message>::persisted("inbox", store, "inbox");
    assert_eq!(reopened.get("000029").unwrap().text, "#29", "read back from the store");
}

struct Unread {
    table: LocalTable<Message>,
}
thread_local! {
    static RENDERS: Cell<u32> = const { Cell::new(0) };
}
impl Component for Unread {
    type Props = LocalTable<Message>;
    type Message = ();
    fn new(table: LocalTable<Message>) -> Self {
        Self { table }
    }
    fn props(&self) -> &LocalTable<Message> {
        &self.table
    }
    fn set_props(&mut self, table: LocalTable<Message>) {
        self.table = table;
    }
    fn view(&self) -> Node {
        Node::label("unread", "")
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        RENDERS.with(|renders| renders.set(renders.get() + 1));
        let unread = self.table.live(context, |m| !m.read, |a, b| b.id.cmp(&a.id));
        Node::label("unread", unread.iter().map(|m| m.text.clone()).collect::<Vec<_>>().join(","))
    }
}

#[test]
fn a_live_query_re_renders_only_when_its_result_changes() {
    let table = LocalTable::<Message>::new("inbox");
    table.upsert(Message { id: 1, text: "hi".into(), read: false });
    let mut tree = ComponentTree::new(Unread::new(table.clone()));
    let text = |tree: &ComponentTree| {
        let mut found = String::new();
        tree.view().visit(&mut |node, _, _| {
            if let Node::Label(label) = node {
                found = label.text().to_owned();
            }
        });
        found
    };
    table.upsert(Message { id: 2, text: "yo".into(), read: false });
    tree.pump_tasks();
    assert_eq!(text(&tree), "yo,hi");
    let before = RENDERS.with(Cell::get);
    table.upsert(Message { id: 3, text: "old".into(), read: true });
    tree.pump_tasks();
    assert_eq!(RENDERS.with(Cell::get), before, "a read message is not in the result");
}

// ---------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------

/// A server that wants token "fresh", tags `/todo/1` with an `ETag`, and
/// fails `/flaky` once.
#[derive(Default)]
struct FakeServer {
    seen: Mutex<Vec<String>>,
    flaky: Mutex<u32>,
}

#[async_trait::async_trait]
impl HttpService for FakeServer {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ServiceError> {
        let header = |name: &str| {
            request
                .headers()
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.clone())
        };
        self.seen.lock().push(format!(
            "{} {}",
            request.url(),
            header("if-none-match").unwrap_or_default()
        ));
        if header("authorization").as_deref() != Some("Bearer fresh") {
            return Ok(HttpResponse::new(401, vec![], vec![]));
        }
        if request.url().ends_with("/flaky") {
            let mut flaky = self.flaky.lock();
            *flaky += 1;
            if *flaky == 1 {
                return Ok(HttpResponse::new(503, vec![], vec![]));
            }
        }
        if header("if-none-match").as_deref() == Some("v1") {
            return Ok(HttpResponse::new(304, vec![], vec![]));
        }
        Ok(HttpResponse::new(
            200,
            vec![("ETag".into(), "v1".into())],
            br#"{"title":"Write docs"}"#.to_vec(),
        ))
    }
}

#[derive(Debug, Deserialize, PartialEq)]
struct Todo {
    title: String,
}

const TODO: Endpoint<(), Todo> = Endpoint::get("todo/{id}");

#[test]
fn the_interceptor_chain_authenticates_retries_logs_and_caches() {
    let server = Arc::new(FakeServer::default());
    let refreshes = Arc::new(Mutex::new(0));
    let counted = Arc::clone(&refreshes);
    let logging = Logging::new();
    let client = HttpClient::new(server.clone(), "https://api.example.com/")
        .with(logging.clone())
        .with(BearerAuth::new("stale", move || {
            *counted.lock() += 1;
            Box::pin(async { Ok("fresh".to_owned()) })
        }))
        .with(rustnative_data::Retry { attempts: 2 })
        .with(ResponseCache::new());
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();

    let todo = runtime.block_on(TODO.call(&client, &[("id", "1")], None)).unwrap();
    assert_eq!(todo, Todo { title: "Write docs".into() });
    assert_eq!(*refreshes.lock(), 1, "refreshed once after the 401");

    // Revalidated with the ETag; the 304 is answered from the cache.
    let again = runtime.block_on(TODO.call(&client, &[("id", "1")], None)).unwrap();
    assert_eq!(again.title, "Write docs");
    assert!(server.seen.lock().last().unwrap().ends_with(" v1"));

    let flaky = runtime.block_on(client.execute(HttpRequest::get(client.url("flaky")))).unwrap();
    assert_eq!(flaky.status(), 200, "the 503 was retried");
    let log = logging.entries();
    assert!(log.iter().any(|line| line == "GET https://api.example.com/todo/1 -> 200"), "{log:?}");
}

// ---------------------------------------------------------------------
// Background work, operations, state machines
// ---------------------------------------------------------------------

#[test]
fn a_job_waits_for_its_constraints_or_its_deadline() {
    let (executor, mut tree, background) = host();
    let conditions = Arc::new(FixedConditions::new(false, false));
    let work = BackgroundWork::new(&background, conditions.clone(), Duration::from_secs(1));
    let done = Rc::new(Cell::new(0));
    let (a, b) = (Rc::clone(&done), Rc::clone(&done));
    work.schedule(
        "sync",
        Constraints { network: true, ..Constraints::default() },
        move || async move {
            a.set(a.get() + 1);
        },
    );
    work.schedule(
        "backup",
        Constraints {
            charging: true,
            deadline: Some(Duration::from_secs(30)),
            ..Constraints::default()
        },
        move || async move { b.set(b.get() + 10) },
    );
    settle(&executor, &mut tree);
    assert_eq!(work.waiting(), 2);

    conditions.set_network(true);
    executor.advance(Duration::from_secs(1));
    settle(&executor, &mut tree);
    assert_eq!(work.started(), ["sync"]);

    for _ in 0..30 {
        executor.advance(Duration::from_secs(1));
        settle(&executor, &mut tree);
    }
    assert_eq!(work.started(), ["sync", "backup"], "the deadline ran it without power");
    assert_eq!(done.get(), 11);
}

#[test]
fn a_newer_goal_pre_empts_a_running_operation() {
    let (executor, mut tree, background) = host();
    let operation = Operation::<u32, String>::new("export", background.clone());
    let sleeper = background.clone();
    operation.start("first", move |progress| async move {
        progress.report(10);
        sleeper.sleep(Duration::from_secs(5)).await;
        Ok("first done".to_owned())
    });
    settle(&executor, &mut tree);
    assert_eq!(
        operation.store().get(),
        OperationState::Running { goal: "first".into(), progress: Some(10) }
    );
    operation.start("second", |_| async { Ok("second done".to_owned()) });
    executor.advance(Duration::from_secs(10));
    settle(&executor, &mut tree);
    assert_eq!(operation.store().get(), OperationState::Succeeded("second done".into()));

    let sleeper = background.clone();
    operation.start("third", move |_| async move {
        sleeper.sleep(Duration::from_secs(5)).await;
        Ok(String::new())
    });
    operation.cancel();
    assert_eq!(operation.store().get(), OperationState::Cancelled);
}

#[derive(Debug, Clone, PartialEq)]
enum Sync {
    Idle,
    Polling,
}

thread_local! {
    static POLLS: Cell<u32> = const { Cell::new(0) };
}

impl MachineState for Sync {
    type Event = bool;
    fn next(&self, start: &bool) -> Option<Self> {
        Some(if *start { Self::Polling } else { Self::Idle })
    }
    fn name(&self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Polling => "Polling",
        }
    }
    fn transitions() -> &'static [(&'static str, &'static str, &'static str)] {
        &[("Idle", "start", "Polling"), ("Polling", "stop", "Idle")]
    }
    fn enter(&self, scope: &rustnative_data::StateScope) {
        if *self == Self::Polling {
            let sleeper = scope.background().clone();
            scope.spawn(async move {
                loop {
                    sleeper.sleep(Duration::from_secs(1)).await;
                    POLLS.with(|polls| polls.set(polls.get() + 1));
                }
            });
        }
    }
}

#[test]
fn leaving_a_state_cancels_the_work_it_started() {
    let (executor, mut tree, background) = host();
    let mut machine = StateMachine::new(Sync::Idle, Some(background));
    assert!(machine.send(&true));
    for _ in 0..3 {
        settle(&executor, &mut tree);
        executor.advance(Duration::from_secs(1));
    }
    settle(&executor, &mut tree);
    let polled = POLLS.with(Cell::get);
    assert!(polled >= 2, "{polled}");
    assert!(machine.send(&false));
    for _ in 0..3 {
        executor.advance(Duration::from_secs(1));
        settle(&executor, &mut tree);
    }
    assert_eq!(POLLS.with(Cell::get), polled, "no polling once idle");
    assert!(StateMachine::<Sync>::to_mermaid().contains("Polling --> Idle : stop"));
}

#[test]
fn the_portable_decoder_downscales_keeping_the_aspect_ratio() {
    let mut ppm = b"P6\n4 2\n255\n".to_vec();
    for pixel in 0..8_u8 {
        ppm.extend_from_slice(&[pixel * 10, 0, 0]);
    }
    let image = PortableDecoder.decode(&ppm, Some((2, 2))).unwrap();
    assert_eq!((image.width(), image.height()), (2, 1));
    assert_eq!(&image.pixels()[..4], &[0, 0, 0, 255]);
    assert!(PortableDecoder.decode(b"GIF89a", None).is_err());
}
