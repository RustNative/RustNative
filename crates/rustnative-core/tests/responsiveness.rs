//! Priorities, deferred values, suspension, and skipping (`PLAN.md`
//! Milestone 54), on virtual time.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use rustnative_core::{
    Callback, Component, ComponentContext, ComponentTree, Event, ManualExecutor, Node, NodeId,
    Priority, PureComponent, Scheduler, Services, SuspendRule, Theme,
};

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

fn clicked(event: &Event, key: &str) -> bool {
    matches!(event, Event::Click { target } if *target == NodeId::from_key(key))
}

// ---------------------------------------------------------------------
// Priorities
// ---------------------------------------------------------------------

struct Log {
    entries: Vec<&'static str>,
    callback: Option<Callback<&'static str>>,
}

impl Component for Log {
    type Props = ();
    type Message = &'static str;
    fn new((): ()) -> Self {
        Self { entries: Vec::new(), callback: None }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column(
            "log",
            [Node::label("entries", self.entries.join(",")), Node::button("go", "Go")],
        )
    }
    fn update(&mut self, event: Event) {
        if let (true, Some(callback)) = (clicked(&event, "go"), &self.callback) {
            callback.send_with(Priority::Deferrable, "slow-1");
            callback.send_with(Priority::Deferrable, "slow-2");
            callback.send("normal");
        }
    }
    fn message(&mut self, entry: &'static str) {
        self.entries.push(entry);
    }
    fn render(&mut self, context: &mut ComponentContext<'_, &'static str>) -> Node {
        self.callback = Some(context.callback());
        self.view()
    }
}

#[test]
fn deferrable_messages_wait_for_everything_more_urgent_and_come_in_slices() {
    let executor = ManualExecutor::new();
    let mut tree = tree(Log::new(()), &executor);
    tree.set_render_budget(Duration::ZERO); // one message per slice
    tree.dispatch(Event::Click { target: NodeId::from_key("go") });
    assert_eq!(labels(&tree), ["normal"], "the normal message first; the deferrable ones wait");
    assert!(tree.has_deferred_work());
    assert!(tree.pump_deferred());
    assert_eq!(labels(&tree), ["normal,slow-1"], "one slice, committed whole");
    assert!(tree.pump_deferred());
    assert_eq!(labels(&tree), ["normal,slow-1,slow-2"]);
    assert!(!tree.has_deferred_work() && !tree.pump_deferred());
}

// ---------------------------------------------------------------------
// Deferred values
// ---------------------------------------------------------------------

static COMPUTED: AtomicU32 = AtomicU32::new(0);

struct Search {
    query: String,
}

impl Component for Search {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { query: String::new() }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("search", [])
    }
    fn update(&mut self, event: Event) {
        if let Event::TextChanged { value, .. } = event {
            self.query = value;
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let results = context.deferred("results", self.query.clone(), |query: String| {
            COMPUTED.fetch_add(1, Ordering::SeqCst);
            ["apple", "apricot", "banana"]
                .into_iter()
                .filter(|word| word.starts_with(&query))
                .collect::<Vec<_>>()
                .join(" ")
        });
        Node::column(
            "search",
            [
                Node::text_input("query", self.query.clone()),
                Node::label(
                    "results",
                    format!(
                        "{}{}",
                        results.current.unwrap_or_default(),
                        if results.pending { " (updating)" } else { "" }
                    ),
                ),
            ],
        )
    }
}

fn type_query(tree: &mut ComponentTree, text: &str) {
    tree.dispatch(Event::TextChanged { target: NodeId::from_key("query"), value: text.to_owned() });
}

#[test]
fn a_deferred_value_shows_the_previous_result_while_the_new_one_is_computed() {
    let executor = ManualExecutor::new();
    let mut tree = tree(Search::new(()), &executor);
    for _ in 0..3 {
        executor.run_until_stalled();
        tree.pump_tasks();
    }
    assert_eq!(labels(&tree), ["apple apricot banana"]);

    type_query(&mut tree, "a");
    assert_eq!(labels(&tree), ["apple apricot banana (updating)"], "the input never waits");
    type_query(&mut tree, "ap");
    type_query(&mut tree, "apr");
    let before = COMPUTED.load(Ordering::SeqCst);
    for _ in 0..3 {
        executor.run_until_stalled();
        tree.pump_tasks();
    }
    assert_eq!(labels(&tree), ["apricot"]);
    assert_eq!(
        COMPUTED.load(Ordering::SeqCst) - before,
        1,
        "superseded computations were discarded"
    );
}

// ---------------------------------------------------------------------
// Suspension
// ---------------------------------------------------------------------

thread_local! {
    static TICKS: Cell<u32> = const { Cell::new(0) };
    static CANCELLED_RAN: Cell<bool> = const { Cell::new(false) };
}

struct Screens {
    details_open: bool,
}

impl Component for Screens {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { details_open: false }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("screens", [])
    }
    fn update(&mut self, event: Event) {
        if clicked(&event, "toggle") {
            self.details_open = !self.details_open;
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let clock = context.child::<Clock>("clock").hidden(self.details_open);
        Node::column("screens", [Node::button("toggle", "Toggle"), clock])
    }
}

struct Clock;

impl Component for Clock {
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
        Node::label("clock", "")
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        context.effect("tick", (), |effects| {
            let scope = effects.task_scope();
            let sleeper = scope.scheduler().clone();
            scope.spawn_with(SuspendRule::Defer, async move {
                loop {
                    sleeper.sleep(Duration::from_secs(1)).await;
                    TICKS.with(|ticks| ticks.set(ticks.get() + 1));
                }
            });
            let slow = scope.scheduler().clone();
            scope.spawn_with(SuspendRule::Cancel, async move {
                slow.sleep(Duration::from_secs(100)).await;
                CANCELLED_RAN.with(|ran| ran.set(true));
            });
            Box::new(|| {})
        });
        self.view()
    }
}

#[test]
fn a_hidden_screen_does_no_periodic_work_and_resumes_when_shown() {
    let executor = ManualExecutor::new();
    let mut tree = tree(Screens::new(()), &executor);
    let tick = |tree: &mut ComponentTree, seconds: u64| {
        for _ in 0..seconds {
            executor.run_until_stalled();
            executor.advance(Duration::from_secs(1));
            executor.run_until_stalled();
            tree.pump_tasks();
        }
    };
    tick(&mut tree, 3);
    let shown = TICKS.with(Cell::get);
    assert!(shown >= 2, "{shown}");

    tree.dispatch(Event::Click { target: NodeId::from_key("toggle") });
    assert_eq!(tree.suspended_components().len(), 1, "the clock is suspended");
    let at_hide = TICKS.with(Cell::get);
    tick(&mut tree, 200);
    assert!(TICKS.with(Cell::get) <= at_hide + 1, "at most the tick already in flight");
    assert!(!CANCELLED_RAN.with(Cell::get), "the cancel-rule task was cancelled");

    tree.dispatch(Event::Click { target: NodeId::from_key("toggle") });
    assert!(tree.suspended_components().is_empty());
    let at_show = TICKS.with(Cell::get);
    tick(&mut tree, 3);
    assert!(TICKS.with(Cell::get) >= at_show + 2, "resumed");
}

#[test]
fn a_backgrounded_window_suspends_every_component() {
    let executor = ManualExecutor::new();
    let mut tree = tree(Screens::new(()), &executor);
    tree.set_backgrounded(true);
    assert_eq!(tree.suspended_components().len(), 2, "the root and the clock");
    tree.set_backgrounded(false);
    assert!(tree.suspended_components().is_empty());
}

// ---------------------------------------------------------------------
// Skipping
// ---------------------------------------------------------------------

thread_local! {
    static ROW_RENDERS: Cell<u32> = const { Cell::new(0) };
}

struct Row;
impl PureComponent for Row {
    type Props = String;
    fn render(name: &String) -> Node {
        ROW_RENDERS.with(|renders| renders.set(renders.get() + 1));
        Node::label("row", name.clone())
    }
}

/// Props that are never equal, even to themselves.
#[derive(Clone)]
struct Always;
impl PartialEq for Always {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}

struct Stubborn {
    props: Always,
}
impl Component for Stubborn {
    type Props = Always;
    type Message = ();
    fn new(props: Always) -> Self {
        Self { props }
    }
    fn props(&self) -> &Always {
        &self.props
    }
    fn set_props(&mut self, props: Always) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::label("stubborn", "")
    }
    fn update(&mut self, _: Event) {}
}

struct Parent {
    clicks: u32,
}
impl Component for Parent {
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
        Node::column("parent", [])
    }
    fn update(&mut self, event: Event) {
        if clicked(&event, "bump") {
            self.clicks += 1;
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let row = context.pure::<Row>("row", "Ada".to_owned());
        let stubborn = context.child_with_props::<Stubborn, _>("stubborn", Always, Stubborn::new);
        Node::column("parent", [Node::button("bump", format!("{}", self.clicks)), row, stubborn])
    }
}

#[test]
fn equal_props_skip_a_pure_component_and_unequal_ones_are_reported() {
    let mut tree = ComponentTree::new(Parent::new(()));
    let first = ROW_RENDERS.with(Cell::get);
    tree.dispatch(Event::Click { target: NodeId::from_key("bump") });
    assert_eq!(ROW_RENDERS.with(Cell::get), first, "equal props: skipped");
    let unskippable = tree.unskippable_components();
    assert!(unskippable.iter().any(|name| name.ends_with("Stubborn")), "{unskippable:?}");
}

// ---------------------------------------------------------------------
// A hidden child that re-renders alone stays hidden
// ---------------------------------------------------------------------

struct Busy {
    count: u32,
}

impl Component for Busy {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { count: 0 }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::label("busy", self.count.to_string())
    }
    fn update(&mut self, _: Event) {}
    fn message(&mut self, (): ()) {
        self.count += 1;
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        if self.count == 0 {
            context.spawn(async {});
        }
        self.view()
    }
}

struct Holder;

impl Component for Holder {
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
        Node::column("holder", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        Node::column("holder", [context.child::<Busy>("busy").hidden(true)])
    }
}

#[test]
fn a_hidden_child_that_re_renders_alone_stays_hidden() {
    let executor = ManualExecutor::new();
    let mut tree = tree(Holder, &executor);
    executor.run_until_stalled();
    assert!(tree.pump_tasks(), "the child re-rendered from its own message");
    assert_eq!(labels(&tree), ["1"]);
    let mut hidden = false;
    tree.view().visit(&mut |node, _, _| {
        if matches!(node, Node::Label(_)) {
            hidden = node.is_hidden();
        }
    });
    assert!(hidden, "the parent's hidden flag survives the child's re-render");
    assert_eq!(tree.suspended_components().len(), 1);
}
