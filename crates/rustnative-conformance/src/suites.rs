//! The guarantee suites (`PLAN.md` Milestone 41, `docs/guarantees.md`):
//! each is a function over a [`ConformanceHost`], so the same assertions
//! hold every backend to the same guarantee. The headless backend runs
//! them in `tests/guarantees.rs`; Windows runs them in its own test module
//! over its native harness.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use rustnative_core::{
    ColumnStyle, Component, ComponentContext, Event, LayoutStyle, Node, NodeId, Overflow, Size,
    SizeMode, Window,
};

use crate::host::ConformanceHost;

/// What a suite's components report back: how often each rendered, and
/// what reached them.
#[derive(Debug, Default)]
pub struct ProbeState {
    renders: RefCell<Vec<(&'static str, u32)>>,
    events: RefCell<Vec<String>>,
}

/// A shared handle on a [`ProbeState`]; equal to itself only, so it can be
/// a component's props without ever looking changed.
#[derive(Debug, Clone, Default)]
pub struct Probe(Rc<ProbeState>);

impl PartialEq for Probe {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Probe {
    fn rendered(&self, who: &'static str) {
        let mut renders = self.0.renders.borrow_mut();
        match renders.iter_mut().find(|(name, _)| *name == who) {
            Some((_, count)) => *count += 1,
            None => renders.push((who, 1)),
        }
    }

    /// How many times `who` rendered.
    #[must_use]
    pub fn renders(&self, who: &str) -> u32 {
        self.0.renders.borrow().iter().find(|(name, _)| *name == who).map_or(0, |(_, count)| *count)
    }

    fn record(&self, event: impl Into<String>) {
        self.0.events.borrow_mut().push(event.into());
    }

    /// Everything recorded, in order.
    #[must_use]
    pub fn events(&self) -> Vec<String> {
        self.0.events.borrow().clone()
    }
}

fn window() -> Window {
    Window::new("conformance", Size::new(360, 240))
}

// ---------------------------------------------------------------------------
// The transient fast path (2.10)
// ---------------------------------------------------------------------------

struct Field {
    probe: Probe,
    value: String,
}

impl Component for Field {
    type Props = Probe;
    type Message = ();
    fn new(probe: Probe) -> Self {
        Self { probe, value: String::new() }
    }
    fn props(&self) -> &Probe {
        &self.probe
    }
    fn set_props(&mut self, probe: Probe) {
        self.probe = probe;
    }
    fn view(&self) -> Node {
        Node::text_input("field", self.value.clone())
    }
    fn render(&mut self, _: &mut ComponentContext<'_, ()>) -> Node {
        self.probe.rendered("field");
        self.view()
    }
    fn update(&mut self, event: Event) {
        if let Event::TextChanged { value, .. } = event {
            self.value = value;
        }
    }
}

struct Form {
    probe: Probe,
}

impl Component for Form {
    type Props = Probe;
    type Message = ();
    fn new(probe: Probe) -> Self {
        Self { probe }
    }
    fn props(&self) -> &Probe {
        &self.probe
    }
    fn set_props(&mut self, probe: Probe) {
        self.probe = probe;
    }
    fn view(&self) -> Node {
        Node::label("unused", "")
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        self.probe.rendered("form");
        let field = context.child_with_props("field", self.probe.clone(), Field::new);
        Node::column("form", [Node::label("title", "A form"), field])
    }
    fn update(&mut self, _: Event) {}
}

/// Text entry re-renders the component that owns the field — and nothing
/// above or beside it: the host control keeps its own text, and no tree
/// pass runs past the owner (2.10).
///
/// # Panics
///
/// When the guarantee does not hold on the host.
pub fn typing_renders_only_the_owning_component(host: &mut impl ConformanceHost) {
    let probe = Probe::default();
    let root = probe.clone();
    let name = host.name();
    host.run(window(), move || Form::new(root.clone()), &mut |driver| {
        let form_before = probe.renders("form");
        let field_before = probe.renders("field");
        driver.type_text("field", "hello");
        assert_eq!(
            probe.renders("form"),
            form_before,
            "{name}: the form did not re-render for a keystroke"
        );
        assert!(probe.renders("field") > field_before, "{name}: the owning component saw the text");
    });
}

struct Scroller {
    probe: Probe,
}

impl Component for Scroller {
    type Props = Probe;
    type Message = ();
    fn new(probe: Probe) -> Self {
        Self { probe }
    }
    fn props(&self) -> &Probe {
        &self.probe
    }
    fn set_props(&mut self, probe: Probe) {
        self.probe = probe;
    }
    fn view(&self) -> Node {
        let rows = (0..60).map(|index| {
            Node::label_with_layout(
                format!("row-{index}"),
                format!("Row {index}"),
                LayoutStyle::new().height(SizeMode::Fixed(24)),
            )
        });
        Node::column_with_layout(
            "list",
            rows,
            LayoutStyle::new().height(SizeMode::Fill),
            ColumnStyle::new().overflow(Overflow::Scroll),
        )
    }
    fn render(&mut self, _: &mut ComponentContext<'_, ()>) -> Node {
        self.probe.rendered("scroller");
        self.view()
    }
    fn update(&mut self, _: Event) {}
}

/// Scrolling moves the viewport of the host's own objects and renders
/// nothing (2.10, Milestone 10).
///
/// # Panics
///
/// When the guarantee does not hold on the host.
pub fn scrolling_renders_nothing(host: &mut impl ConformanceHost) {
    let probe = Probe::default();
    let root = probe.clone();
    let name = host.name();
    host.run(window(), move || Scroller::new(root.clone()), &mut |driver| {
        let before = probe.renders("scroller");
        driver.scroll("list", 120);
        driver.scroll("list", 240);
        assert_eq!(probe.renders("scroller"), before, "{name}: scrolling rendered");
    });
}

// ---------------------------------------------------------------------------
// Batching (C09)
// ---------------------------------------------------------------------------

struct Triple {
    probe: Probe,
    a: u32,
    b: u32,
    c: u32,
}

impl Component for Triple {
    type Props = Probe;
    type Message = ();
    fn new(probe: Probe) -> Self {
        Self { probe, a: 0, b: 0, c: 0 }
    }
    fn props(&self) -> &Probe {
        &self.probe
    }
    fn set_props(&mut self, probe: Probe) {
        self.probe = probe;
    }
    fn view(&self) -> Node {
        Node::column(
            "root",
            [
                Node::label("values", format!("{} {} {}", self.a, self.b, self.c)),
                Node::button("bump", "Bump"),
            ],
        )
    }
    fn render(&mut self, _: &mut ComponentContext<'_, ()>) -> Node {
        // Every render observes all three or none of a message's changes.
        assert!(self.a == self.b && self.b == self.c, "a render observed a partial set of changes");
        self.probe.rendered("triple");
        self.view()
    }
    fn update(&mut self, event: Event) {
        self.probe.rendered("triple-message");
        if matches!(event, Event::Click { target } if target == NodeId::from_key("bump")) {
            self.a += 1;
            self.b += 1;
            self.c += 1;
        }
    }
}

/// One message, however many state changes it makes, is one render, and
/// no render observes a partial set of them (`C09`). (A click can be more
/// than one message — the button taking focus is one — so renders are
/// counted against the messages that reached the component.)
///
/// # Panics
///
/// When the guarantee does not hold on the host.
pub fn one_message_is_one_render(host: &mut impl ConformanceHost) {
    let probe = Probe::default();
    let root = probe.clone();
    let name = host.name();
    host.run(window(), move || Triple::new(root.clone()), &mut |driver| {
        let renders = probe.renders("triple");
        let messages = probe.renders("triple-message");
        driver.click("bump");
        driver.click("bump");
        let delivered = probe.renders("triple-message") - messages;
        assert!(delivered >= 2, "{name}: both clicks reached the component");
        assert_eq!(probe.renders("triple") - renders, delivered, "{name}: one render per message");
    });
}

// ---------------------------------------------------------------------------
// Scope-bound cancellation
// ---------------------------------------------------------------------------

struct Loader {
    probe: Probe,
}

impl Component for Loader {
    type Props = Probe;
    type Message = &'static str;
    fn new(probe: Probe) -> Self {
        Self { probe }
    }
    fn props(&self) -> &Probe {
        &self.probe
    }
    fn set_props(&mut self, probe: Probe) {
        self.probe = probe;
    }
    fn view(&self) -> Node {
        Node::label("loader", "Loading")
    }
    fn render(&mut self, context: &mut ComponentContext<'_, &'static str>) -> Node {
        context.effect("load", (), |effects| {
            let delay = effects.sleep(Duration::from_millis(40));
            effects.spawn(async move {
                delay.await;
                "loaded"
            });
            Box::new(|| {})
        });
        self.view()
    }
    fn update(&mut self, _: Event) {}
    fn message(&mut self, message: &'static str) {
        self.probe.record(message);
    }
    fn unmounted(&mut self) {
        self.probe.record("unmounted");
    }
}

struct Host {
    probe: Probe,
    showing: bool,
}

impl Component for Host {
    type Props = Probe;
    type Message = ();
    fn new(probe: Probe) -> Self {
        Self { probe, showing: true }
    }
    fn props(&self) -> &Probe {
        &self.probe
    }
    fn set_props(&mut self, probe: Probe) {
        self.probe = probe;
    }
    fn view(&self) -> Node {
        Node::label("unused", "")
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let mut children = vec![Node::button("unmount", "Unmount")];
        if self.showing {
            children.push(context.child_with_props("loader", self.probe.clone(), Loader::new));
        }
        Node::column("host", children)
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { target } if target == NodeId::from_key("unmount")) {
            self.showing = false;
        }
    }
}

/// A task never delivers to a component that has unmounted: its scope is
/// cancelled with its owner.
///
/// # Panics
///
/// When the guarantee does not hold on the host.
pub fn no_message_after_unmount(host: &mut impl ConformanceHost) {
    let probe = Probe::default();
    let root = probe.clone();
    let name = host.name();
    host.run(window(), move || Host::new(root.clone()), &mut |driver| {
        driver.click("unmount");
        driver.advance(Duration::from_millis(200));
        // On a real host the task may win the race and deliver before the
        // click lands; what may never happen is a delivery after it.
        let events = probe.events();
        let unmounted = events.iter().position(|event| event == "unmounted");
        assert!(unmounted.is_some(), "{name}: the loader unmounted: {events:?}");
        assert_eq!(
            unmounted.map(|index| index + 1),
            Some(events.len()),
            "{name}: nothing after unmount: {events:?}"
        );
    });
    // And with no unmount, the same task does deliver: the test is not
    // vacuous.
    let probe = Probe::default();
    let root = probe.clone();
    host.run(window(), move || Loader::new(root.clone()), &mut |driver| {
        driver.advance(Duration::from_millis(200));
        assert_eq!(
            probe.events(),
            vec!["loaded".to_owned()],
            "{name}: a mounted component's task delivers"
        );
    });
}

// ---------------------------------------------------------------------------
// Native-object lifetime
// ---------------------------------------------------------------------------

struct Toggler {
    shown: Rc<Cell<bool>>,
    showing: bool,
}

impl Component for Toggler {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { shown: Rc::default(), showing: false }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        let mut children = vec![Node::button("toggle", "Toggle")];
        if self.showing {
            children.push(Node::column(
                "panel",
                (0..10).map(|index| Node::label(format!("item-{index}"), format!("Item {index}"))),
            ));
        }
        Node::column("root", children)
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { target } if target == NodeId::from_key("toggle")) {
            self.showing = !self.showing;
            self.shown.set(self.showing);
        }
    }
}

/// Mounting and unmounting a subtree a hundred times leaves exactly the
/// native objects there were before: every realized object is released
/// with its node.
///
/// # Panics
///
/// When the guarantee does not hold on the host.
pub fn mount_unmount_returns_to_baseline(host: &mut impl ConformanceHost) {
    let name = host.name();
    host.run(window(), || Toggler::new(()), &mut |driver| {
        let baseline = driver.realized_objects();
        driver.click("toggle");
        let mounted = driver.realized_objects();
        assert!(mounted > baseline, "{name}: the panel realizes objects");
        driver.click("toggle");
        for _ in 0..99 {
            driver.click("toggle");
            driver.click("toggle");
        }
        assert_eq!(
            driver.realized_objects(),
            baseline,
            "{name}: back to baseline after 100 cycles"
        );
    });
}

/// Every suite, in order.
pub fn all(host: &mut impl ConformanceHost) {
    typing_renders_only_the_owning_component(host);
    scrolling_renders_nothing(host);
    one_message_is_one_render(host);
    no_message_after_unmount(host);
    mount_unmount_returns_to_baseline(host);
}
