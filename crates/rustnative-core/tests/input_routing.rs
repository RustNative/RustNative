//! Advanced input (Milestone 25) through the component runtime: routing of
//! node-targeted pointer/drag events to the owning component under its own
//! local keys, and deferred input requests carrying framework-wide ids.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use rustnative_core::{
    Application, Component, ComponentContext, DragData, DropEffect, Event, InputInterest,
    InputRequest, InputRequests, Node, NodeId, Point, PointerButton, PointerEvent, PointerKind,
    Size, TreeSnapshot, Window, WindowId,
};

type Log = Rc<RefCell<Vec<String>>>;

/// A child owning a pointer-interested pad and a drop zone. It captures the
/// pointer on down and accepts file drops.
struct Pad {
    log: Log,
    input: Option<InputRequests>,
}

impl Component for Pad {
    type Props = Log;
    type Message = ();

    fn new(log: Log) -> Self {
        Self { log, input: None }
    }
    fn props(&self) -> &Log {
        &self.log
    }
    fn set_props(&mut self, log: Log) {
        self.log = log;
    }
    fn view(&self) -> Node {
        Node::column(
            "pad-root",
            [
                Node::column("pad", []).with_input(InputInterest::new().pointer()),
                Node::column("drop", []).with_input(InputInterest::new().drop_target()),
            ],
        )
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        self.input = Some(context.input());
        self.view()
    }
    fn update(&mut self, event: Event) {
        let input = self.input.as_ref().expect("render ran before any event");
        match &event {
            Event::PointerDown { target, pointer } => {
                assert_eq!(*target, NodeId::from_key("pad"), "the component sees its local key");
                input.capture_pointer("pad", pointer.pointer_id());
                self.log.borrow_mut().push(format!("down@{:?}", pointer.position()));
            }
            Event::PointerUp { pointer, .. } => {
                input.release_pointer("pad", pointer.pointer_id());
                self.log.borrow_mut().push("up".into());
            }
            Event::DragEnter { data, .. } => {
                let accept = !data.files().is_empty();
                input.set_drop_effect(if accept { DropEffect::Copy } else { DropEffect::None });
                self.log.borrow_mut().push("enter".into());
            }
            Event::Drop { data, .. } => {
                self.log.borrow_mut().push(format!("drop {}", data.files().len()));
            }
            _ => {}
        }
    }
}

struct Root {
    log: Log,
}

impl Component for Root {
    type Props = Log;
    type Message = ();

    fn new(log: Log) -> Self {
        Self { log }
    }
    fn props(&self) -> &Log {
        &self.log
    }
    fn set_props(&mut self, log: Log) {
        self.log = log;
    }
    fn view(&self) -> Node {
        Node::column("root", [])
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let child = context.child_with_props("pad", self.log.clone(), Pad::new);
        Node::column("root", [child])
    }
    fn update(&mut self, event: Event) {
        self.log.borrow_mut().push(format!("root saw {event:?}"));
    }
}

/// The framework-wide id a backend would see for the first node that
/// declares `wanted` interest — exactly how a backend finds it.
fn interested(application: &Application, wanted: fn(InputInterest) -> bool) -> NodeId {
    let snapshot = TreeSnapshot::from_node(&application.view()).unwrap();
    snapshot
        .ordered_nodes()
        .into_iter()
        .find(|node| wanted(node.input))
        .map(|node| node.id)
        .expect("an interested node is realized")
}

fn sample(x: i32) -> PointerEvent {
    PointerEvent::new(0, PointerKind::Mouse, Point::new(x, 4), Duration::from_millis(1))
        .with_button(PointerButton::Primary)
}

#[test]
fn pointer_events_reach_the_owning_child_and_capture_requests_carry_the_global_id() {
    let log = Log::default();
    let mut application =
        Application::new(Root::new(log.clone()), Window::new("t", Size::new(100, 100)));
    let pad = interested(&application, InputInterest::wants_pointer);
    assert_ne!(pad, NodeId::from_key("pad"), "a child's node is scoped, not its raw key");

    assert!(application.dispatch(Event::PointerDown { target: pad, pointer: sample(7) }));
    assert_eq!(log.borrow().as_slice(), ["down@Point { x: 7, y: 4 }"]);
    assert_eq!(
        application.take_input_requests(WindowId::PRIMARY),
        vec![InputRequest::CapturePointer { node: pad, pointer_id: 0 }],
        "the backend receives the id it realized the node under, not the local key"
    );
    assert!(application.take_input_requests(WindowId::PRIMARY).is_empty(), "drained once");

    application.dispatch(Event::PointerUp { target: pad, pointer: sample(9) });
    assert_eq!(
        application.take_input_requests(WindowId::PRIMARY),
        vec![InputRequest::ReleasePointer { node: pad, pointer_id: 0 }]
    );
}

#[test]
fn drag_feedback_and_drop_route_to_the_drop_target_owner() {
    let log = Log::default();
    let mut application =
        Application::new(Root::new(log.clone()), Window::new("t", Size::new(100, 100)));
    let zone = interested(&application, InputInterest::wants_drop);
    let files = DragData::new().with_files([PathBuf::from("a.txt"), PathBuf::from("b.txt")]);

    application.dispatch(Event::DragEnter {
        target: zone,
        data: files.clone(),
        position: Point::new(1, 1),
    });
    assert_eq!(
        application.take_input_requests(WindowId::PRIMARY),
        vec![InputRequest::SetDropEffect(DropEffect::Copy)]
    );
    application.dispatch(Event::Drop { target: zone, data: files, position: Point::new(1, 1) });
    assert_eq!(log.borrow().as_slice(), ["enter", "drop 2"]);
}

#[test]
fn a_pointer_event_for_a_node_that_no_longer_exists_is_not_delivered_anywhere() {
    let log = Log::default();
    let mut application =
        Application::new(Root::new(log.clone()), Window::new("t", Size::new(100, 100)));
    let stale = NodeId::from_key("never-rendered");
    assert!(!application.dispatch(Event::PointerMove { target: stale, pointer: sample(1) }));
    assert!(log.borrow().is_empty(), "stale targeted input must not fall back to the root");
}

#[test]
fn clipboard_changed_routes_to_the_named_window_root() {
    let log = Log::default();
    let mut application =
        Application::new(Root::new(log.clone()), Window::new("t", Size::new(100, 100)));
    let other = WindowId::PRIMARY;
    assert!(application.dispatch(Event::ClipboardChanged { window: other }));
    assert!(log.borrow()[0].starts_with("root saw ClipboardChanged"));
}
