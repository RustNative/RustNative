//! A component the inspection tests record, replay, and generate a test
//! from.

use rustnative_core::{Component, ComponentContext, Event, HttpRequest, Node, NodeId};
use serde_json::{Value, json};

/// Types a name, then fetches a greeting for it.
pub struct Greeter {
    name: String,
    greeting: String,
    loads: u32,
    requested: bool,
}

/// What the greeter hears.
pub enum Message {
    /// A greeting arrived.
    Greeting(String),
}

impl Component for Greeter {
    type Props = ();
    type Message = Message;

    fn new((): ()) -> Self {
        Self { name: String::new(), greeting: String::new(), loads: 0, requested: false }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column(
            "greeter",
            [
                Node::text_input("name", self.name.clone()),
                Node::text_input("secret", ""),
                Node::button("load", "Load"),
                Node::label("greeting", self.greeting.clone()),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        match event {
            Event::TextChanged { target, value } if target == NodeId::from_key("name") => {
                self.name = value;
            }
            Event::Click { .. } => self.loads += 1,
            _ => {}
        }
    }
    fn message(&mut self, Message::Greeting(greeting): Message) {
        self.greeting = greeting;
    }
    fn render(&mut self, context: &mut ComponentContext<'_, Message>) -> Node {
        if self.loads > 0 && !self.requested {
            self.requested = true;
            if let Some(http) = context.services().http().cloned() {
                let url = format!("https://api.test/greet?name={}", self.name);
                context.spawn(async move {
                    let body = match http.execute(HttpRequest::get(url)).await {
                        Ok(response) => String::from_utf8_lossy(response.body_bytes()).into_owned(),
                        Err(error) => format!("error: {error}"),
                    };
                    Message::Greeting(body)
                });
            }
        }
        self.view()
    }
    fn inspect(&self) -> Option<Value> {
        Some(json!({ "name": self.name, "greeting": self.greeting, "loads": self.loads }))
    }
}
