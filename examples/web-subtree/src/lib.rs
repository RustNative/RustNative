//! WebAssembly subtrees (Web milestone A, `C32-2` in the browser).
//!
//! - [`counter`]: a client component, the same definition as generated
//!   JavaScript and as WebAssembly, for the equivalence of the two.
//! - [`notes`]: two replicas of a text edited apart and merged with a
//!   replicated sequence (RGA), with a timer on the host's clock — full Rust
//!   in the browser, where the client subset would not reach.
//!
//! Built for the browser with
//! `cargo build -p web-subtree --target wasm32-unknown-unknown --release`.

rustnative_web::wasm_subtree! {
    "counter" => rustnative_web::Client<counter::Counter>,
    "notes" => notes::Notes,
}

#[rustnative_web::client]
pub mod counter {
    use rustnative_core::{Event, Node, NodeId};
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    /// A counter, a name, and a switch.
    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Counter {
        /// The count.
        pub count: i32,
        /// The name typed.
        pub name: String,
        /// The switch.
        pub on: bool,
    }

    impl Counter {
        /// Handles an event.
        pub fn update(&mut self, event: Event, fx: &mut Effects<()>) {
            let _ = fx;
            match event {
                Event::Click { target } if target == NodeId::from_key("add") => self.count += 1,
                Event::Click { target } if target == NodeId::from_key("reset") => self.count = 0,
                Event::TextChanged { target, value } if target == NodeId::from_key("name") => {
                    self.name = value;
                }
                Event::Toggled { target, on } if target == NodeId::from_key("on") => self.on = on,
                _ => {}
            }
        }

        /// The view.
        #[must_use]
        pub fn view(&self) -> Node {
            Node::column(
                "counter",
                [
                    Node::label("count", format!("Count: {}", self.count)),
                    Node::button("add", "Add"),
                    Node::button("reset", "Reset"),
                    Node::text_input("name", self.name.clone()),
                    Node::label("greeting", format!("Hello, {}", self.name)),
                    Node::toggle("on", "On", self.on),
                ],
            )
        }
    }
}

pub mod notes {
    //! Two replicas of one text, edited apart and merged.

    use std::time::Duration;

    use rustnative_core::{Component, ComponentContext, Event, Node, NodeId};
    use rustnative_sync::clock::Hlc;
    use rustnative_sync::crdt::{Crdt, Rga};
    use serde::{Deserialize, Serialize};

    /// Where the text starts.
    #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
    pub struct NotesProps {
        /// The initial text.
        pub text: String,
    }

    /// What the component's own task sends: a tick of its timer.
    #[derive(Debug, Clone, Copy)]
    pub struct Tick;

    /// See the [module documentation](self).
    pub struct Notes {
        props: NotesProps,
        left: Rga<char>,
        right: Rga<char>,
        counter: u32,
        ticks: u32,
        ticking: bool,
    }

    impl Notes {
        /// An element id: this component's edits are ordered by their
        /// counter, and two replicas' by their replica.
        fn stamp(&mut self, replica: u64) -> Hlc {
            self.counter += 1;
            Hlc { wall: 0, counter: self.counter, replica }
        }

        fn type_into(&mut self, left: bool, character: char) {
            let id = self.stamp(if left { 1 } else { 2 });
            let replica = if left { &mut self.left } else { &mut self.right };
            let end = replica.text().chars().count();
            replica.insert(end, character, id);
        }
    }

    impl Component for Notes {
        type Props = NotesProps;
        type Message = Tick;

        fn new(props: NotesProps) -> Self {
            let mut base = Rga::default();
            for (index, character) in props.text.chars().enumerate() {
                let index_u32 = u32::try_from(index).unwrap_or(u32::MAX);
                base.insert(index, character, Hlc { wall: 0, counter: index_u32, replica: 0 });
            }
            let counter = u32::try_from(props.text.chars().count()).unwrap_or(u32::MAX);
            Self { left: base.clone(), right: base, props, counter, ticks: 0, ticking: false }
        }

        fn props(&self) -> &NotesProps {
            &self.props
        }

        fn set_props(&mut self, props: NotesProps) {
            self.props = props;
        }

        fn view(&self) -> Node {
            Node::column(
                "notes",
                [
                    Node::label("left", self.left.text()),
                    Node::label("right", self.right.text()),
                    Node::button("type-left", "Type on the left"),
                    Node::button("type-right", "Type on the right"),
                    Node::button("merge", "Merge"),
                    Node::label("ticks", format!("Ticks: {}", self.ticks)),
                ],
            )
        }

        fn update(&mut self, event: Event) {
            let Event::Click { target } = event else { return };
            if target == NodeId::from_key("type-left") {
                self.type_into(true, 'a');
            } else if target == NodeId::from_key("type-right") {
                self.type_into(false, 'b');
            } else if target == NodeId::from_key("merge") {
                let (left, right) = (self.left.clone(), self.right.clone());
                self.left.merge(&right);
                self.right.merge(&left);
            }
        }

        fn message(&mut self, _: Tick) {
            self.ticks += 1;
            self.ticking = false;
        }

        fn render(&mut self, context: &mut ComponentContext<'_, Tick>) -> Node {
            // A timer on the host's clock: three ticks, a tenth of a second
            // apart — where the component runs, not while a server renders
            // its first markup.
            let server =
                context.services().extension::<rustnative_web::client::ServerRender>().is_some();
            if self.ticks < 3 && !self.ticking && !server {
                self.ticking = true;
                let tick = context.sleep(Duration::from_millis(100));
                context.spawn(async move {
                    tick.await;
                    Tick
                });
            }
            self.view()
        }

        fn inspect(&self) -> Option<serde_json::Value> {
            Some(
                serde_json::json!({ "left": self.left.text(), "right": self.right.text(), "ticks": self.ticks }),
            )
        }
    }
}
