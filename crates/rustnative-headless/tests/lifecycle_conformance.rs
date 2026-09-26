//! The lifecycle conformance suite (Milestone 45): not the individual
//! events, which each backend already reports, but their *collisions* —
//! the sequences that break in production.
//!
//! 1. process death with unflushed state: only what was flushed survives;
//! 2. a deep link arriving during restoration: the restored stack and the
//!    link's destination both hold;
//! 3. a configuration change while work is in flight: the work lands;
//! 4. low memory while suspended, then resume: state intact, caches gone;
//! 5. kill-and-restore per navigation destination (`C14-1`).

use std::time::Duration;

use rustnative_core::{
    Component, ComponentContext, Event, Lifecycle, NavigationStack, Node, NodeId, Persisted, Route,
    Size, Window,
};
use rustnative_headless::{HeadlessApp, Query, STATE_FLUSH_DELAY};

/// Serializable, so it can live in a persisted navigation stack.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
enum Screen {
    Home,
    Item(u32),
}

/// An app with a persisted counter, a persisted navigation stack, an
/// in-memory cache it drops on low memory, and a slow load.
struct Notes {
    count: Option<Persisted<u32>>,
    stack: Option<Persisted<NavigationStack<Screen>>>,
    cache: Vec<u8>,
    loaded: bool,
    started: bool,
}

impl Component for Notes {
    type Props = ();
    type Message = ();

    fn new((): ()) -> Self {
        Self { count: None, stack: None, cache: vec![0; 1024], loaded: false, started: false }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}

    fn view(&self) -> Node {
        let count = self.count.as_ref().map_or(0, Persisted::get);
        let top =
            self.stack.as_ref().map_or(Screen::Home, |stack| stack.get().top().route().clone());
        Node::column(
            "root",
            [
                Node::label("count", format!("Count {count}")),
                Node::button("increment", "Increment"),
                Node::label("screen", format!("{top:?}")),
                Node::label(
                    "depth",
                    format!("Depth {}", self.stack.as_ref().map_or(0, |stack| stack.get().len())),
                ),
                Node::label("cache", format!("Cache {}", self.cache.len())),
                Node::label("loaded", if self.loaded { "Loaded" } else { "Loading" }),
            ],
        )
    }

    fn update(&mut self, event: Event) {
        match event {
            Event::Click { target } if target == NodeId::from_key("increment") => {
                if let Some(count) = &self.count {
                    count.update(|value| *value += 1);
                }
            }
            Event::DeepLink { url } => {
                if let Some(id) = Route::parse("/items/:id")
                    .ok()
                    .and_then(|route| route.matches(rustnative_core::url_path(&url)))
                    .and_then(|params| params.param::<u32>("id"))
                {
                    if let Some(stack) = &self.stack {
                        stack.update(|stack| {
                            stack.push(Screen::Item(id));
                        });
                    }
                }
            }
            Event::Lifecycle(Lifecycle::LowMemory) => self.cache = Vec::new(),
            _ => {}
        }
    }

    fn message(&mut self, (): ()) {
        self.loaded = true;
    }

    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        if self.count.is_none() {
            self.count = Some(context.persisted("count", 0));
            self.stack = Some(context.persisted("stack", NavigationStack::new(Screen::Home)));
        }
        if !self.started {
            self.started = true;
            let delay = context.sleep(Duration::from_secs(5));
            context.spawn(delay);
        }
        self.view()
    }
}

fn launch() -> HeadlessApp {
    HeadlessApp::launch(Window::new("Notes", Size::new(300, 400)), || Notes::new(()))
}

fn increment(app: &mut HeadlessApp) {
    let clicked = app.click(&Query::text("Increment"));
    assert!(clicked.is_ok(), "the increment button: {clicked:?}");
}

#[test]
fn process_death_keeps_only_flushed_state() {
    let mut app = launch();
    increment(&mut app);
    app.advance(STATE_FLUSH_DELAY); // flushed: count 1
    increment(&mut app); // count 2, still buffered

    let app = app.kill_and_restore();
    assert!(app.find(&Query::text("Count 1")).is_ok(), "{}", app.golden());
}

#[test]
fn a_polite_termination_flushes_everything() {
    let mut app = launch();
    increment(&mut app);
    increment(&mut app);
    let app = app.terminate_and_relaunch();
    assert!(app.find(&Query::text("Count 2")).is_ok());
}

#[test]
fn a_deep_link_arriving_during_restoration_lands_on_the_restored_stack() {
    let mut app = launch();
    app.open_url("notes://app/items/7");
    app.lifecycle(Lifecycle::Suspending); // flushed: [Home, Item(7)]

    let app = app.relaunch_with_deep_link("notes://app/items/9");
    assert!(app.find(&Query::text("Item(9)")).is_ok(), "{}", app.golden());
    assert!(app.find(&Query::text("Depth 3")).is_ok(), "restored entries stay beneath the link");
}

#[test]
fn a_configuration_change_during_a_load_does_not_lose_the_load() {
    let mut app = launch();
    app.advance(Duration::from_secs(2));
    app.resize(Size::new(600, 300));
    app.set_theme(rustnative_core::Theme::default());
    app.advance(Duration::from_secs(3));
    assert!(app.find(&Query::text("Loaded")).is_ok());
    assert_eq!(app.realized().size(), Size::new(600, 300));
}

#[test]
fn low_memory_while_suspended_drops_caches_but_not_state() {
    let mut app = launch();
    increment(&mut app);
    app.lifecycle(Lifecycle::Suspending);
    app.lifecycle(Lifecycle::LowMemory);
    app.lifecycle(Lifecycle::Resuming);
    assert!(app.find(&Query::text("Cache 0")).is_ok());
    assert!(app.find(&Query::text("Count 1")).is_ok());
    // And the low-memory flush made the state durable.
    let app = app.kill_and_restore();
    assert!(app.find(&Query::text("Count 1")).is_ok());
}

#[test]
fn each_destination_survives_kill_and_restore() {
    for id in [1_u32, 2, 3] {
        let mut app = launch();
        app.open_url(&format!("notes://app/items/{id}"));
        app.advance(STATE_FLUSH_DELAY);
        let app = app.kill_and_restore();
        assert!(app.find(&Query::text(format!("Item({id})"))).is_ok(), "destination {id}");
    }
}
