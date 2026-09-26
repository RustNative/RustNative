//! Milestone 30, black-box: navigation stacks over the managed component
//! tree, persisted state across whole `Application` lifetimes, deep links,
//! and lifecycle flushing — exercised only through the public API.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use rustnative_core::{
    Application, Component, ComponentContext, Event, Lifecycle, MemoryStateStore,
    NavigationCommand, NavigationStack, Navigator, Node, NodeId, Persisted, Router, Services, Size,
    StateStore, Window, url_path,
};

// ---------------------------------------------------------------------
// A screen with private state, and an app that stacks screens.
// ---------------------------------------------------------------------

#[derive(Clone, PartialEq)]
struct ScreenProps {
    name: String,
    navigator: Navigator<String>,
    mounts: Rc<Cell<u32>>,
}

/// A screen that counts its own clicks — state that only survives if the
/// screen is never remounted.
struct Screen {
    props: ScreenProps,
    clicks: u32,
}

impl Component for Screen {
    type Props = ScreenProps;
    type Message = ();

    fn new(props: ScreenProps) -> Self {
        Self { props, clicks: 0 }
    }
    fn mounted(&mut self) {
        self.props.mounts.set(self.props.mounts.get() + 1);
    }
    fn props(&self) -> &ScreenProps {
        &self.props
    }
    fn set_props(&mut self, props: ScreenProps) {
        self.props = props;
    }

    fn view(&self) -> Node {
        Node::column(
            "screen",
            [
                Node::label("title", format!("{}: {} clicks", self.props.name, self.clicks)),
                Node::button("click", "Click"),
                Node::button("open", "Open details"),
                Node::button("back", "Back"),
            ],
        )
    }

    fn update(&mut self, event: Event) {
        let Event::Click { target } = event else { return };
        if target == NodeId::from_key("click") {
            self.clicks += 1;
        } else if target == NodeId::from_key("open") {
            self.props.navigator.push("details".to_owned());
        } else if target == NodeId::from_key("back") {
            self.props.navigator.pop();
        }
    }
}

struct App {
    stack: NavigationStack<String>,
    mounts: Rc<Cell<u32>>,
    links: Rc<RefCell<Vec<String>>>,
    router: Router,
}

impl Component for App {
    type Props = Rc<Cell<u32>>;
    type Message = NavigationCommand<String>;

    fn new(mounts: Rc<Cell<u32>>) -> Self {
        Self {
            stack: NavigationStack::new("home".to_owned()),
            mounts,
            links: Rc::default(),
            router: Router::new()
                .route("details", "/details")
                .and_then(|router| router.route("home", "/"))
                .expect("valid routes"),
        }
    }
    fn props(&self) -> &Rc<Cell<u32>> {
        &self.mounts
    }
    fn set_props(&mut self, mounts: Rc<Cell<u32>>) {
        self.mounts = mounts;
    }
    fn view(&self) -> Node {
        Node::label("unused", "")
    }

    fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
        let navigator = Navigator::new(context.callback());
        let mounts = Rc::clone(&self.mounts);
        self.stack.view("stack", |entry| {
            context.child_with_props(
                entry.id().key(),
                ScreenProps {
                    name: entry.route().clone(),
                    navigator: navigator.clone(),
                    mounts: Rc::clone(&mounts),
                },
                Screen::new,
            )
        })
    }

    fn update(&mut self, event: Event) {
        if let Event::DeepLink { url } = event {
            self.links.borrow_mut().push(url.clone());
            if let Some((name, _)) = self.router.resolve(url_path(&url)) {
                self.stack.push(name.to_owned());
            }
        }
    }

    fn message(&mut self, command: NavigationCommand<String>) {
        self.stack.apply(command);
    }
}

/// The screen whose title is visible (the one not hidden).
fn visible_title(application: &Application) -> String {
    let Node::Column(stack) = application.view() else { panic!("the stack is a column") };
    let top = stack.children().iter().find(|screen| !screen.is_hidden()).expect("a visible screen");
    let Node::Column(screen) = top else { panic!("a screen is a column") };
    let Node::Label(title) = &screen.children()[0] else { panic!("the title is a label") };
    title.text().to_owned()
}

/// Clicks the button labelled for `key` on the visible screen.
///
/// Screens are child components, so their buttons carry scoped ids; the
/// visible screen's button is found in the rendered tree by its label.
fn click_visible(application: &mut Application, key: &str) {
    let label = match key {
        "click" => "Click",
        "open" => "Open details",
        "back" => "Back",
        other => panic!("no button {other}"),
    };
    let Node::Column(stack) = application.view() else { panic!("a column") };
    let Some(Node::Column(screen)) = stack.children().iter().find(|screen| !screen.is_hidden())
    else {
        panic!("a visible screen");
    };
    let target = screen
        .children()
        .iter()
        .find(|child| matches!(child, Node::Button(button) if button.text() == label))
        .map(Node::id)
        .expect("the button is on the visible screen");
    application.dispatch(Event::Click { target });
}

#[test]
fn pushing_a_screen_keeps_the_one_below_mounted_with_its_state() {
    let mounts = Rc::new(Cell::new(0));
    let mut application =
        Application::new(App::new(Rc::clone(&mounts)), Window::new("nav", Size::new(300, 300)));
    assert_eq!(mounts.get(), 1, "the home screen mounted");

    click_visible(&mut application, "click");
    click_visible(&mut application, "click");
    assert_eq!(visible_title(&application), "home: 2 clicks");

    click_visible(&mut application, "open");
    assert_eq!(visible_title(&application), "details: 0 clicks");
    assert_eq!(mounts.get(), 2, "only the new screen mounted");

    click_visible(&mut application, "back");
    assert_eq!(visible_title(&application), "home: 2 clicks", "home kept its state");
    assert_eq!(mounts.get(), 2, "going back remounted nothing");
}

#[test]
fn a_deep_link_is_routed_by_the_root_component() {
    let mounts = Rc::new(Cell::new(0));
    let mut application =
        Application::new(App::new(Rc::clone(&mounts)), Window::new("nav", Size::new(300, 300)));
    assert!(application.open_url("myapp://open/details?from=mail"));
    assert_eq!(visible_title(&application), "details: 0 clicks");
}

// ---------------------------------------------------------------------
// Persisted state across application lifetimes.
// ---------------------------------------------------------------------

#[derive(Clone, PartialEq, Default)]
struct Unit;

/// A leaf that persists one number, keyed by where it sits.
struct Remembering {
    props: Unit,
    value: Option<Persisted<u32>>,
}

impl Component for Remembering {
    type Props = Unit;
    type Message = ();
    fn new(props: Unit) -> Self {
        Self { props, value: None }
    }
    fn props(&self) -> &Unit {
        &self.props
    }
    fn set_props(&mut self, props: Unit) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::label("unused", "")
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let value = context.persisted("value", 0u32);
        let node = Node::button("bump", value.get().to_string());
        self.value = Some(value);
        node
    }
    fn update(&mut self, event: Event) {
        if let (Event::Click { .. }, Some(value)) = (event, &self.value) {
            value.update(|value| *value += 10);
        }
    }
}

/// Two `Remembering` children, in an order that can be flipped.
struct Pair {
    flipped: bool,
    seen: Rc<RefCell<Vec<Lifecycle>>>,
}

impl Component for Pair {
    type Props = bool;
    type Message = ();
    fn new(flipped: bool) -> Self {
        Self { flipped, seen: Rc::default() }
    }
    fn props(&self) -> &bool {
        &self.flipped
    }
    fn set_props(&mut self, flipped: bool) {
        self.flipped = flipped;
    }
    fn view(&self) -> Node {
        Node::label("unused", "")
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let a = context.child::<Remembering>("a");
        let b = context.child::<Remembering>("b");
        Node::column("pair", if self.flipped { [b, a] } else { [a, b] })
    }
    fn update(&mut self, event: Event) {
        if let Event::Lifecycle(lifecycle) = event {
            self.seen.borrow_mut().push(lifecycle);
        }
    }
}

fn pair(store: &MemoryStateStore, flipped: bool) -> Application {
    let services = Services::default().with_state_store(Arc::new(store.clone()));
    Application::with_services(
        Pair::new(flipped),
        Window::new("pair", Size::new(200, 200)),
        services,
    )
}

/// The two buttons' labels, in on-screen order, with their ids.
fn buttons(application: &Application) -> Vec<(NodeId, String)> {
    let Node::Column(column) = application.view() else { panic!("a column") };
    column
        .children()
        .iter()
        .map(|child| {
            let Node::Button(button) = child else { panic!("a button") };
            (child.id(), button.text().to_owned())
        })
        .collect()
}

#[test]
fn persisted_state_survives_the_application_and_follows_keys_not_positions() {
    let store = MemoryStateStore::new();
    {
        let mut first = pair(&store, false);
        let (a, _) = buttons(&first)[0].clone();
        first.dispatch(Event::Click { target: a });
        first.dispatch(Event::Click { target: a });
        assert!(first.has_unsaved_state());
        first.flush_state().expect("the memory store never fails");
        assert!(!first.has_unsaved_state());
    }
    assert_eq!(store.keys().len(), 1, "one value written: {:?}", store.keys());

    // A new run, with the children in the other order: `a` is second on
    // screen but still has `a`'s value, because keys — not positions — are
    // what a key path is made of.
    let second = pair(&store, true);
    let labels = buttons(&second).into_iter().map(|(_, label)| label).collect::<Vec<_>>();
    assert_eq!(labels, vec!["0", "20"]);
}

#[test]
fn suspending_flushes_before_the_component_hears_about_it() {
    let store = MemoryStateStore::new();
    let mut application = pair(&store, false);
    let (a, _) = buttons(&application)[0].clone();
    application.dispatch(Event::Click { target: a });
    assert!(store.keys().is_empty(), "buffered, not written");

    application.lifecycle(Lifecycle::Suspending).expect("flushed");
    assert_eq!(store.keys().len(), 1, "written by the time the event was delivered");
    let key = &store.keys()[0];
    assert_eq!(store.load(key).unwrap().as_deref(), Some(&b"10"[..]));
    assert!(key.ends_with("/a#value"), "keyed by the component's path: {key}");
}

#[test]
fn a_navigation_stack_can_itself_be_persisted() {
    let store = MemoryStateStore::new();
    let key = "stack";
    let saved = {
        let mut stack = NavigationStack::new("home".to_owned());
        stack.push("settings".to_owned());
        store.save(key, &serde_json::to_vec(&stack).unwrap()).unwrap();
        stack
    };
    let restored: NavigationStack<String> =
        serde_json::from_slice(&store.load(key).unwrap().unwrap()).unwrap();
    assert_eq!(restored, saved, "entries and their ids come back as they were");
}
