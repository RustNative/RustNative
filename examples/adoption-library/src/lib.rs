//! The library rung of the adoption ladder (`PLAN.md` Milestone 40): an
//! application model written as an ordinary Rust Native component — state,
//! events, rendering — compiled into a DLL with no UI dependency and driven
//! by a C or C# host through bindings generated from `counter.ril`.
//!
//! The component is the same one a Windows or headless backend would
//! realize; here nothing realizes it, and the host reads the view instead.

use rustnative_core::{Component, ComponentTree, Event, Node, NodeId};

include!(concat!(env!("OUT_DIR"), "/counter.rs"));

/// The model: the component a UI would render.
pub struct CounterModel {
    count: u32,
    name: String,
}

impl Component for CounterModel {
    type Props = u32;
    type Message = ();

    fn new(start: u32) -> Self {
        Self { count: start, name: "Counter".to_owned() }
    }
    fn props(&self) -> &u32 {
        &self.count
    }
    fn set_props(&mut self, _: u32) {}
    fn view(&self) -> Node {
        Node::column(
            "root",
            [
                Node::label("label", format!("{}: {}", self.name, self.count)),
                Node::text_input("name", self.name.clone()),
                Node::button("increment", "Increment"),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        match event {
            Event::Click { target } if target == NodeId::from_key("increment") => self.count += 1,
            Event::TextChanged { target, value } if target == NodeId::from_key("name") => {
                self.name = value;
            }
            _ => {}
        }
    }
}

/// The exported service: the component tree, driven through its events.
pub struct Library {
    tree: ComponentTree,
}

impl Library {
    fn label(&self) -> String {
        let Node::Column(root) = self.tree.view() else { return String::new() };
        root.children()
            .iter()
            .find_map(|child| match child {
                Node::Label(label) => Some(label.text().to_owned()),
                _ => None,
            })
            .unwrap_or_default()
    }

    fn count(&self) -> u32 {
        self.label().rsplit(": ").next().and_then(|count| count.parse().ok()).unwrap_or_default()
    }
}

impl Counter for Library {
    fn new(start: u32) -> Self {
        Self { tree: ComponentTree::new(CounterModel::new(start)) }
    }

    fn increment(&mut self, events: &CounterEvents) -> u32 {
        self.tree.dispatch(Event::Click { target: NodeId::from_key("increment") });
        let count = self.count();
        events.changed(count);
        count
    }

    fn label(&self) -> String {
        Self::label(self)
    }

    fn rename(&mut self, name: &str, _events: &CounterEvents) {
        self.tree.dispatch(Event::TextChanged {
            target: NodeId::from_key("name"),
            value: name.to_owned(),
        });
    }
}

export_counter!(Library);
