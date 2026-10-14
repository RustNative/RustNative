//! A component that records what it is told, for the widget tests.

#![allow(dead_code, reason = "each test file uses part of it")]

use std::cell::RefCell;
use std::rc::Rc;

use rustnative_core::{Component, Event, Node};

/// Every event the component received, shared with the test.
pub type Log = Rc<RefCell<Vec<Event>>>;

/// A component whose view is a function of how many events it has seen,
/// and which records each one.
pub struct Recorder {
    pub log: Log,
    pub view: Rc<dyn Fn(&[Event]) -> Node>,
}

impl Component for Recorder {
    type Props = ();
    type Message = ();

    fn new((): ()) -> Self {
        Self { log: Rc::default(), view: Rc::new(|_| Node::label("empty", "")) }
    }

    fn props(&self) -> &() {
        &()
    }

    fn set_props(&mut self, (): ()) {}

    fn view(&self) -> Node {
        (self.view)(&self.log.borrow())
    }

    fn update(&mut self, event: Event) {
        self.log.borrow_mut().push(event);
    }
}

/// A recorder showing `view`, and its log.
pub fn recorder(view: impl Fn(&[Event]) -> Node + 'static) -> (Log, impl Fn() -> Recorder) {
    let log: Log = Rc::default();
    let shared = Rc::clone(&log);
    let view: Rc<dyn Fn(&[Event]) -> Node> = Rc::new(view);
    (log, move || Recorder { log: Rc::clone(&shared), view: Rc::clone(&view) })
}
