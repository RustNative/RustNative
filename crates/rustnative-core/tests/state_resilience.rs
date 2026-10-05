//! Shared state, error boundaries, supervision, and streams (`PLAN.md`
//! Milestone 47).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use rustnative_core::LocalExecutor;
use rustnative_core::{
    Component, ComponentContext, ComponentTree, Event, ManualExecutor, Node, NodeId, RenderCause,
    Scheduler, Services, Store, Supervised, SupervisionPolicy, Theme,
};

/// Silences the panics these tests cause on purpose, and only those.
fn quiet() {
    std::panic::set_hook(Box::new(|info| {
        let message = rustnative_core::scheduler::panic_message(info.payload());
        let expected = ["could not be parsed", "handler failed", "uncontained", "flaky run"];
        if !expected.iter().any(|part| message.contains(part)) {
            eprintln!("{info}");
        }
    }));
}

fn tree<C: Component>(root: C, executor: &ManualExecutor) -> ComponentTree {
    ComponentTree::with_scheduler(
        root,
        Services::default(),
        Theme::default(),
        Scheduler::with_executor(Arc::new(executor.clone())),
    )
}

fn labels(tree: &ComponentTree) -> Vec<String> {
    let mut found = Vec::new();
    tree.view().visit(&mut |node, _, _| {
        if let Node::Label(label) = node {
            found.push(label.text().to_owned());
        }
    });
    found
}

// ---------------------------------------------------------------------
// Stores
// ---------------------------------------------------------------------

#[derive(Clone, Default)]
struct Profile {
    name: String,
    visits: u32,
}

struct Page;
impl Component for Page {
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
        Node::column("page", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let store = context.provide_scoped_with(|| {
            Store::new("profile", Profile { name: "Ada".into(), visits: 0 })
        });
        STORE.with(|slot| slot.set(Some(store)));
        Node::column(
            "page",
            [context.child::<NameBadge>("name"), context.child::<Visits>("visits")],
        )
    }
}

thread_local! {
    static STORE: Cell<Option<Store<Profile>>> = const { Cell::new(None) };
}

fn store() -> Store<Profile> {
    STORE.with(|slot| {
        let store = slot.take().unwrap();
        slot.set(Some(store.clone()));
        store
    })
}

struct NameBadge;
impl Component for NameBadge {
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
        Node::label("name", "")
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let profile = context.scoped::<Store<Profile>>().unwrap();
        let name = context.select(&profile, |profile| profile.name.clone());
        Node::label("name", name)
    }
}

struct Visits;
impl Component for Visits {
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
        Node::label("visits", "")
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let profile = context.scoped::<Store<Profile>>().unwrap();
        let visits = context.select(&profile, |profile| profile.visits);
        Node::label("visits", visits.to_string())
    }
}

#[test]
fn a_store_update_re_renders_only_the_components_whose_slice_changed() {
    let executor = ManualExecutor::new();
    let mut tree = tree(Page, &executor);
    assert_eq!(labels(&tree), ["Ada", "0"]);

    store().update(|profile| profile.visits += 1);
    assert!(tree.pump_tasks(), "a store change is picked up by the next pump");
    assert_eq!(labels(&tree), ["Ada", "1"]);
    let rendered = tree
        .last_render_log()
        .iter()
        .map(|r| (r.path.clone(), r.cause.clone()))
        .collect::<Vec<_>>();
    assert_eq!(rendered.len(), 1, "{rendered:?}");
    assert!(
        rendered[0].0.ends_with("/visits") && rendered[0].1 == RenderCause::Store,
        "{rendered:?}"
    );

    // An update that leaves every slice equal renders nothing.
    store().update(|profile| profile.name = "Ada".into());
    tree.pump_tasks();
    assert_eq!(tree.last_render_log(), []);
}

#[test]
fn a_store_updated_by_background_work_re_renders_on_the_next_pump() {
    let executor = ManualExecutor::new();
    let mut tree = tree(Page, &executor);
    let scheduler = tree.scheduler().clone();
    let sleep = scheduler.sleep(Duration::from_millis(10));
    let profile = store();
    // A local future that updates the store after a delay, delivering no
    // message to anyone.
    let pool = rustnative_core::LocalPool::new(scheduler.host_waker());
    let _handle = pool.spawn_local(Box::pin(async move {
        sleep.await;
        profile.update(|profile| profile.name = "Grace".into());
    }));
    pool.run_until_stalled();
    executor.advance(Duration::from_millis(10));
    pool.run_until_stalled();
    assert!(tree.pump_tasks());
    assert_eq!(labels(&tree), ["Grace", "0"]);
}

// ---------------------------------------------------------------------
// Boundaries
// ---------------------------------------------------------------------

thread_local! {
    static CRASH_ON_RENDER: Cell<bool> = const { Cell::new(false) };
    static RENDERS: Cell<u32> = const { Cell::new(0) };
}

struct Feed {
    clicks: u32,
}
impl Component for Feed {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { clicks: 0 }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        RENDERS.with(|renders| renders.set(renders.get() + 1));
        assert!(!CRASH_ON_RENDER.with(Cell::get), "the feed could not be parsed");
        Node::column(
            "feed",
            [Node::label("clicks", self.clicks.to_string()), Node::button("explode", "Explode")],
        )
    }
    fn update(&mut self, event: Event) {
        if let Event::Click { target } = event {
            assert!(target != NodeId::from_key("explode"), "the handler failed");
            self.clicks += 1;
        }
    }
}

#[derive(Clone, PartialEq)]
struct ScreenProps(SupervisionPolicy);

struct Screen {
    props: ScreenProps,
}
impl Component for Screen {
    type Props = ScreenProps;
    type Message = ();
    fn new(props: ScreenProps) -> Self {
        Self { props }
    }
    fn props(&self) -> &ScreenProps {
        &self.props
    }
    fn set_props(&mut self, props: ScreenProps) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::column("screen", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let feed = context.boundary::<Feed>("feed", (), self.props.0, |failure| {
            Node::column(
                "fallback",
                [Node::label("why", failure.message.clone()), Node::button("retry", "Try again")],
            )
        });
        Node::column("screen", [Node::label("title", "News"), feed])
    }
}

fn find(_tree: &ComponentTree, key: &str) -> NodeId {
    // Dispatch resolves a key unique in the tree to its node.
    NodeId::from_key(key)
}

#[test]
fn a_render_failure_is_contained_and_restarted_with_backoff() {
    quiet();
    CRASH_ON_RENDER.with(|crash| crash.set(true));
    let executor = ManualExecutor::new();
    let policy = SupervisionPolicy::RestartWithBackoff {
        initial: Duration::from_millis(100),
        max: Duration::from_secs(1),
        attempts: 3,
    };
    let mut tree = tree(Screen::new(ScreenProps(policy)), &executor);
    assert_eq!(
        labels(&tree),
        ["News", "the feed could not be parsed"],
        "sibling kept, fallback shown"
    );
    let failures = tree.take_failures();
    assert_eq!(failures.len(), 1);
    assert!(failures[0].component.ends_with("/feed"));

    // The first restart fails too, and the next waits twice as long.
    executor.run_until_stalled();
    executor.advance(Duration::from_millis(100));
    tree.pump_tasks();
    assert_eq!(tree.take_failures()[0].attempt, 2);

    CRASH_ON_RENDER.with(|crash| crash.set(false));
    executor.run_until_stalled();
    executor.advance(Duration::from_millis(150));
    tree.pump_tasks();
    assert_eq!(labels(&tree).len(), 2, "not yet: the delay is 200 ms");
    executor.advance(Duration::from_millis(50));
    tree.pump_tasks();
    assert_eq!(labels(&tree), ["News", "0"], "rebuilt without restarting anything else");
}

#[test]
fn an_event_handler_failure_is_contained_and_retried_by_hand() {
    quiet();
    CRASH_ON_RENDER.with(|crash| crash.set(false));
    let executor = ManualExecutor::new();
    let mut tree = tree(Screen::new(ScreenProps(SupervisionPolicy::Isolate)), &executor);
    let explode = find(&tree, "explode");
    assert!(tree.dispatch(Event::Click { target: explode }));
    assert_eq!(labels(&tree), ["News", "the handler failed"]);

    // Isolated: no restart however long we wait.
    executor.run_until_stalled();
    executor.advance(Duration::from_secs(60));
    tree.pump_tasks();
    assert_eq!(labels(&tree)[1], "the handler failed");

    let retry = find(&tree, "retry");
    tree.dispatch(Event::Click { target: retry });
    assert_eq!(labels(&tree), ["News", "0"], "fresh state after the retry");
}

#[test]
fn a_failure_with_no_boundary_reaches_the_application() {
    struct Bare;
    impl Component for Bare {
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
            Node::button("explode", "Explode")
        }
        fn update(&mut self, _: Event) {
            panic!("uncontained");
        }
    }
    quiet();
    let mut tree = ComponentTree::new(Bare);
    let explode = find(&tree, "explode");
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tree.dispatch(Event::Click { target: explode })
    }));
    assert!(outcome.is_err());
}

// ---------------------------------------------------------------------
// Supervised tasks and streams
// ---------------------------------------------------------------------

struct Worker {
    result: Option<Supervised<u32>>,
    items: Vec<u32>,
}

#[derive(Debug)]
enum WorkerMessage {
    Done(Supervised<u32>),
    Item(u32),
}

static RUNS: AtomicU32 = AtomicU32::new(0);

impl Component for Worker {
    type Props = ();
    type Message = WorkerMessage;
    fn new((): ()) -> Self {
        Self { result: None, items: Vec::new() }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::label("result", format!("{:?} {:?}", self.result, self.items))
    }
    fn update(&mut self, _: Event) {}
    fn message(&mut self, message: WorkerMessage) {
        match message {
            WorkerMessage::Done(result) => self.result = Some(result),
            WorkerMessage::Item(item) => self.items.push(item),
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, WorkerMessage>) -> Node {
        if self.result.is_none() && RUNS.load(Ordering::SeqCst) == 0 {
            let policy = SupervisionPolicy::RestartWithBackoff {
                initial: Duration::from_millis(10),
                max: Duration::from_millis(10),
                attempts: 5,
            };
            let scope = context.task_scope();
            scope.spawn_supervised(
                policy,
                || async {
                    let run = RUNS.fetch_add(1, Ordering::SeqCst) + 1;
                    assert!(run >= 3, "flaky run {run}");
                    run
                },
                WorkerMessage::Done,
            );
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            for item in [1, 2, 3] {
                sender.send(item).unwrap();
            }
            drop(sender);
            context.collect(ReceiverStream(receiver), WorkerMessage::Item);
        }
        self.view()
    }
}

struct ReceiverStream(tokio::sync::mpsc::UnboundedReceiver<u32>);

impl futures_core::Stream for ReceiverStream {
    type Item = u32;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<u32>> {
        self.0.poll_recv(cx)
    }
}

#[test]
fn supervised_work_restarts_and_streams_deliver_every_item() {
    quiet();
    let executor = ManualExecutor::new();
    let mut tree = tree(Worker::new(()), &executor);
    executor.run_until_stalled();
    for _ in 0..4 {
        executor.advance(Duration::from_millis(10));
    }
    tree.pump_tasks();
    let text = labels(&tree).join("");
    assert!(text.contains("[1, 2, 3]") && text.contains("Some(Ok(3))"), "{text}");
    assert_eq!(RUNS.load(Ordering::SeqCst), 3, "two failures, restarted twice");
}
