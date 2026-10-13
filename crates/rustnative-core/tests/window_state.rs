//! What a window takes with it when it closes, what every window hears, and
//! the host traits every window reads.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rustnative_core::services::ServiceError;
use rustnative_core::{
    Application, Capability, Component, ComponentContext, Event, Lifecycle, MemoryStateStore,
    MotionPreference, Node, Persisted, PlatformCapabilities, Services, Size, StateStore, Window,
    WindowId, keys,
};

/// Writes a persisted value on every click, and records lifecycle events.
struct Saver {
    value: Option<Persisted<u32>>,
    seen: Rc<RefCell<Vec<Lifecycle>>>,
}

impl Component for Saver {
    type Props = Rc<RefCell<Vec<Lifecycle>>>;
    type Message = ();
    fn new(seen: Self::Props) -> Self {
        Self { value: None, seen }
    }
    fn props(&self) -> &Self::Props {
        &self.seen
    }
    fn set_props(&mut self, seen: Self::Props) {
        self.seen = seen;
    }
    fn view(&self) -> Node {
        Node::label("unused", "")
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let value = context.persisted("value", 0u32);
        let node = Node::button("save", value.get().to_string());
        self.value = Some(value);
        node
    }
    fn update(&mut self, event: Event) {
        match event {
            Event::Click { .. } => {
                if let Some(value) = &self.value {
                    value.update(|value| *value += 1);
                }
            }
            Event::Lifecycle(lifecycle) => self.seen.borrow_mut().push(lifecycle),
            _ => {}
        }
    }
}

fn save_button(application: &Application, window: WindowId) -> rustnative_core::NodeId {
    application.view_for(window).expect("open").id()
}

/// A store that refuses writes until told otherwise.
#[derive(Default)]
struct Flaky {
    refuse: AtomicBool,
    saved: Mutex<Vec<String>>,
}

impl StateStore for Flaky {
    fn load(&self, _key: &str) -> Result<Option<Vec<u8>>, ServiceError> {
        Ok(None)
    }
    fn save(&self, key: &str, _value: &[u8]) -> Result<(), ServiceError> {
        if self.refuse.load(Ordering::SeqCst) {
            return Err(ServiceError::new("the disk is full"));
        }
        self.saved.lock().expect("lock").push(key.to_owned());
        Ok(())
    }
    fn remove(&self, _key: &str) -> Result<(), ServiceError> {
        Ok(())
    }
}

#[test]
fn closing_a_window_flushes_what_it_saved() {
    let store = MemoryStateStore::new();
    let services = Services::default().with_state_store(Arc::new(store.clone()));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut application = Application::with_services(
        Saver::new(seen.clone()),
        Window::new("primary", Size::new(100, 100)),
        services,
    );
    let second =
        application.open_window(Saver::new(seen), Window::new("second", Size::new(100, 100)), None);
    let button = save_button(&application, second);
    application.dispatch_to_window(second, Event::Click { target: button });
    assert!(application.has_unsaved_state());

    application.close_window(second);
    assert!(!application.has_unsaved_state());
    assert_eq!(store.keys().len(), 1, "the closed window's write reached the store");
}

#[test]
fn a_refused_write_from_a_closed_window_is_retried() {
    let store = Arc::new(Flaky::default());
    store.refuse.store(true, Ordering::SeqCst);
    let services = Services::default().with_state_store(store.clone());
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut application = Application::with_services(
        Saver::new(seen.clone()),
        Window::new("primary", Size::new(100, 100)),
        services,
    );
    let second =
        application.open_window(Saver::new(seen), Window::new("second", Size::new(100, 100)), None);
    let button = save_button(&application, second);
    application.dispatch_to_window(second, Event::Click { target: button });
    application.close_window(second);
    assert!(application.has_unsaved_state(), "kept, not dropped with the tree");

    store.refuse.store(false, Ordering::SeqCst);
    application.flush_state().expect("the store accepts writes again");
    assert!(!application.has_unsaved_state());
    assert_eq!(store.saved.lock().expect("lock").len(), 1);
}

#[test]
fn every_window_hears_the_lifecycle() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut application =
        Application::new(Saver::new(seen.clone()), Window::new("primary", Size::new(100, 100)));
    application.open_window(
        Saver::new(seen.clone()),
        Window::new("second", Size::new(100, 100)),
        None,
    );
    application.lifecycle(Lifecycle::LowMemory).expect("no store, nothing to fail");
    assert_eq!(*seen.borrow(), vec![Lifecycle::LowMemory, Lifecycle::LowMemory]);
}

/// Reads reduced motion and the capabilities from the environment.
struct Reader;

impl Component for Reader {
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
        Node::label("unused", "")
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let motion = context.env(&keys::REDUCED_MOTION);
        let clipboard = context.capabilities().supports(Capability::Clipboard);
        Node::label("read", format!("{motion:?} {clipboard}"))
    }
    fn update(&mut self, _event: Event) {}
}

fn read(application: &Application) -> String {
    let Node::Label(label) = application.view() else { panic!("a label") };
    label.text().to_owned()
}

#[test]
fn reduced_motion_is_one_channel() {
    let mut application = Application::new(Reader, Window::new("reader", Size::new(100, 100)));
    application.set_motion_preference(MotionPreference::Reduced);
    assert!(read(&application).starts_with("Reduced"), "the environment follows the preference");
    application.set_environment(&keys::REDUCED_MOTION, MotionPreference::Full);
    assert_eq!(
        application.motion_preference(),
        MotionPreference::Full,
        "the timelines follow the environment"
    );
}

#[test]
fn capabilities_reach_components_through_the_environment() {
    let mut application = Application::new(Reader, Window::new("reader", Size::new(100, 100)));
    assert!(read(&application).ends_with("false"));
    application
        .set_environment(&keys::CAPABILITIES, PlatformCapabilities::new([Capability::Clipboard]));
    assert!(read(&application).ends_with("true"));
}
