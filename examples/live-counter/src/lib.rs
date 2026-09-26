//! A server-interactive counter (`PLAN.md` Milestone 55's second "done
//! when"): the screen lives on the server; the Windows client shows it
//! with [`rustnative_sync::live::RemoteView`] and survives a reconnect and a
//! deploy without losing its state.

use rustnative_core::{Component, Event, Node, NodeId};
use rustnative_sync::live::LiveApp;
use serde_json::{Value, json};

/// The counter, on the server.
pub struct Counter {
    count: i64,
    note: String,
    snapshot: Option<Value>,
}

impl Component for Counter {
    type Props = Option<Value>;
    type Message = ();

    fn new(snapshot: Option<Value>) -> Self {
        let state = snapshot.clone().unwrap_or_default();
        Self {
            count: state["count"].as_i64().unwrap_or(0),
            note: state["note"].as_str().unwrap_or_default().to_owned(),
            snapshot,
        }
    }
    fn props(&self) -> &Option<Value> {
        &self.snapshot
    }
    fn set_props(&mut self, snapshot: Option<Value>) {
        self.snapshot = snapshot;
    }
    fn view(&self) -> Node {
        Node::column(
            "counter",
            [
                Node::label("count", format!("Count {}", self.count)),
                Node::row(
                    "buttons",
                    [Node::button("decrement", "−"), Node::button("increment", "+")],
                ),
                Node::text_input("note", self.note.clone()),
                Node::label("served-by", format!("Served by process {}", std::process::id())),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        match event {
            Event::Click { target } if target == NodeId::from_key("increment") => self.count += 1,
            Event::Click { target } if target == NodeId::from_key("decrement") => self.count -= 1,
            Event::TextChanged { target, value } if target == NodeId::from_key("note") => {
                self.note = value;
            }
            _ => {}
        }
    }
    /// The state a reconnecting or moved client carries.
    fn inspect(&self) -> Option<Value> {
        Some(json!({ "count": self.count, "note": self.note }))
    }
}

/// The live application.
pub struct CounterApp;

impl LiveApp for CounterApp {
    type Root = Counter;
    fn root(&self, snapshot: Option<Value>) -> Counter {
        Counter::new(snapshot)
    }
}
