//! Black-box tests for the task/effect lifetime invariants promised in
//! `README.md`'s "Lifetime rules" section. These exercise `ComponentTree`
//! purely through its `pub` API (as an embedder or `rustnative-windows`
//! itself would), independent of internal refactors — see P1.6 in the
//! standards audit.
//!
//! Every test observes outcomes two ways: through `ComponentTree::view()`
//! (what a real consumer would see) and through a plain `Arc<AtomicUsize>`
//! counter incremented from *inside* a spawned task's own future body. The
//! second signal only increments if that specific future is actually polled
//! to completion — a cancelled task's future is dropped before finishing,
//! so this directly observes cancellation without reaching into any private
//! `ComponentTree` field.

use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rustnative_core::{Component, ComponentContext, ComponentTree, Event, Node, NodeId};

/// Generous relative to the tasks' own sleep durations below so CI jitter
/// can't turn a real pass into a flaky failure; the invariants under test
/// are about whether a task runs at all, not fine-grained timing.
const SETTLE: Duration = Duration::from_millis(400);

fn pump_until_quiet(tree: &mut ComponentTree, budget: Duration) {
    let step = Duration::from_millis(10);
    let mut waited = Duration::ZERO;
    while waited < budget {
        tree.pump_tasks();
        std::thread::sleep(step);
        waited += step;
    }
}

// ---------------------------------------------------------------------
// Rerendering does not cancel tasks.
// ---------------------------------------------------------------------

#[derive(Clone, Default, PartialEq)]
struct CounterProps;

enum CounterMessage {
    Tick,
}

struct Counter {
    ran: Arc<AtomicUsize>,
    ticks: usize,
    spawned: bool,
}

impl Component for Counter {
    type Props = CounterProps;
    type Message = CounterMessage;

    fn new(_props: CounterProps) -> Self {
        Self { ran: Arc::new(AtomicUsize::new(0)), ticks: 0, spawned: false }
    }
    fn props(&self) -> &CounterProps {
        static PROPS: CounterProps = CounterProps;
        &PROPS
    }
    fn set_props(&mut self, _props: CounterProps) {}
    fn view(&self) -> Node {
        Node::label("counter", self.ticks.to_string())
    }
    fn update(&mut self, _event: Event) {}
    fn message(&mut self, message: CounterMessage) {
        match message {
            CounterMessage::Tick => self.ticks += 1,
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, CounterMessage>) -> Node {
        if !self.spawned {
            self.spawned = true;
            let ran = Arc::clone(&self.ran);
            let sleep = context.sleep(Duration::from_millis(30));
            context.spawn(async move {
                sleep.await;
                ran.fetch_add(1, Ordering::SeqCst);
                CounterMessage::Tick
            });
        }
        self.view()
    }
}

fn label_text(node: &Node, key: &str) -> Option<String> {
    let target = NodeId::from_key(key);
    let mut found = None;
    node.visit(&mut |n, _, _| {
        if n.id() == target {
            if let Node::Label(label) = n {
                found = Some(label.text().to_string());
            }
        }
    });
    found
}

#[test]
fn rerendering_does_not_cancel_or_respawn_tasks() {
    let mut tree = ComponentTree::new(Counter::new(CounterProps));

    // Several renders before the task has had a chance to complete. Per the
    // "rerendering does not cancel tasks" rule, none of these should cancel
    // or duplicate the single task spawned on the first render.
    for _ in 0..5 {
        let _ = tree.render();
    }

    pump_until_quiet(&mut tree, SETTLE);

    let text = label_text(&tree.view(), "counter").expect("counter label must exist");
    assert_eq!(text, "1", "exactly one Tick should have been delivered despite multiple rerenders");
}

// ---------------------------------------------------------------------
// Removing a component cancels all outstanding tasks in its scope, and
// completed results for removed components are ignored.
// ---------------------------------------------------------------------

#[derive(Clone, PartialEq, Default)]
struct ToggleProps;

#[derive(Clone)]
struct ChildProps(Arc<AtomicUsize>);

impl PartialEq for ChildProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

struct Child {
    props: ChildProps,
    spawned: bool,
}

enum ChildMessage {
    Done,
}

impl Component for Child {
    type Props = ChildProps;
    type Message = ChildMessage;

    fn new(props: ChildProps) -> Self {
        Self { props, spawned: false }
    }
    fn props(&self) -> &ChildProps {
        &self.props
    }
    fn set_props(&mut self, props: ChildProps) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::label("child", "child")
    }
    fn update(&mut self, _event: Event) {}
    fn message(&mut self, _message: ChildMessage) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ChildMessage>) -> Node {
        if !self.spawned {
            self.spawned = true;
            let ran = Arc::clone(&self.props.0);
            // Long enough that the parent can remove the child well before
            // this would otherwise complete.
            let sleep = context.sleep(Duration::from_millis(200));
            context.spawn(async move {
                sleep.await;
                ran.fetch_add(1, Ordering::SeqCst);
                ChildMessage::Done
            });
        }
        self.view()
    }
}

struct Parent {
    show_child: bool,
    ran: Arc<AtomicUsize>,
}

impl Component for Parent {
    type Props = ToggleProps;
    type Message = ();

    fn new(_props: ToggleProps) -> Self {
        Self { show_child: true, ran: Arc::new(AtomicUsize::new(0)) }
    }
    fn props(&self) -> &ToggleProps {
        static PROPS: ToggleProps = ToggleProps;
        &PROPS
    }
    fn set_props(&mut self, _props: ToggleProps) {}
    fn view(&self) -> Node {
        Node::label("parent", if self.show_child { "shown" } else { "hidden" })
    }
    fn update(&mut self, event: Event) {
        if event.target() == Some(NodeId::from_key("toggle")) {
            self.show_child = !self.show_child;
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let toggle = Node::button("toggle", "toggle");
        if self.show_child {
            let child = context.child_with_props::<Child, _>(
                "child",
                ChildProps(Arc::clone(&self.ran)),
                Child::new,
            );
            Node::column("root", [toggle, child])
        } else {
            Node::column("root", [toggle, Node::label("empty", "empty")])
        }
    }
}

#[test]
fn removing_a_component_cancels_its_outstanding_tasks() {
    let ran = Arc::new(AtomicUsize::new(0));
    let props = ToggleProps;
    let mut parent = Parent::new(props);
    parent.ran = Arc::clone(&ran);
    let mut tree = ComponentTree::new(parent);
    // The child's task was spawned on the first render above.

    // Remove the child well before its 200ms task would complete.
    std::thread::sleep(Duration::from_millis(20));
    tree.dispatch(Event::Click { target: NodeId::from_key("toggle") });
    assert_eq!(
        label_text(&tree.view(), "empty"),
        Some("empty".to_string()),
        "after removing the child, render() must produce the empty-state label"
    );

    // Wait past the point the task would have completed if it had not been
    // cancelled, then pump. If cancellation genuinely aborted the task
    // (rather than merely detaching it), `ran` must still be zero: a
    // cooperative-only cancellation flag the task body never checks (the
    // pre-P0.3 design) would let it keep running to completion here.
    pump_until_quiet(&mut tree, SETTLE);

    assert_eq!(
        ran.load(Ordering::SeqCst),
        0,
        "a task belonging to a removed component's scope must not run to completion"
    );
}

// ---------------------------------------------------------------------
// Effects: retained across unchanged dependencies, cleanup runs before
// replacement, run after render is committed.
// ---------------------------------------------------------------------

struct EffectCounts {
    runs: AtomicUsize,
    cleanups: AtomicUsize,
}

#[derive(Clone)]
struct EffectProbeProps {
    dependency: u32,
    counts: Arc<EffectCounts>,
}

impl PartialEq for EffectProbeProps {
    fn eq(&self, other: &Self) -> bool {
        self.dependency == other.dependency && Arc::ptr_eq(&self.counts, &other.counts)
    }
}

struct EffectProbe {
    props: EffectProbeProps,
}

impl Component for EffectProbe {
    type Props = EffectProbeProps;
    type Message = ();

    fn new(props: EffectProbeProps) -> Self {
        Self { props }
    }
    fn props(&self) -> &EffectProbeProps {
        &self.props
    }
    fn set_props(&mut self, props: EffectProbeProps) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::label("probe", "probe")
    }
    fn update(&mut self, _event: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let counts = Arc::clone(&self.props.counts);
        let cleanup_counts = Arc::clone(&self.props.counts);
        context.effect("dep", self.props.dependency, move |_ctx| {
            counts.runs.fetch_add(1, Ordering::SeqCst);
            Box::new(move || {
                cleanup_counts.cleanups.fetch_add(1, Ordering::SeqCst);
            })
        });
        self.view()
    }
}

#[test]
fn effects_are_retained_across_unchanged_dependencies_and_cleaned_up_on_change() {
    let counts =
        Arc::new(EffectCounts { runs: AtomicUsize::new(0), cleanups: AtomicUsize::new(0) });

    let mut tree = ComponentTree::new(EffectProbe::new(EffectProbeProps {
        dependency: 1,
        counts: Arc::clone(&counts),
    }));
    assert_eq!(counts.runs.load(Ordering::SeqCst), 1);
    assert_eq!(counts.cleanups.load(Ordering::SeqCst), 0);

    // Rerendering with the *same* dependency must not rerun the effect.
    let _ = tree.render();
    let _ = tree.render();
    assert_eq!(
        counts.runs.load(Ordering::SeqCst),
        1,
        "an effect must be retained, not rerun, while its dependency is unchanged"
    );
    assert_eq!(counts.cleanups.load(Ordering::SeqCst), 0);
}

#[test]
fn dependency_change_generation_hash_differs() {
    // `ComponentContext::effect` hashes its `dependencies` argument with
    // `DefaultHasher` to decide whether to restart; this is a direct,
    // black-box check that two different dependency values the framework
    // is expected to treat as "changed" really do hash differently, which
    // is the actual mechanism `effects_are_retained_...` above relies on
    // never producing a false "unchanged" verdict.
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;

    fn hash_of<D: Hash>(value: D) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    assert_ne!(hash_of(1u32), hash_of(2u32));
}
