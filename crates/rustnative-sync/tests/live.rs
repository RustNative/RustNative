//! Server-interactive mode (`PLAN.md` Milestone 55's "done when"): a
//! screen held on the server survives a reconnect and a deploy without
//! losing state; typing is echoed at once; a subtree switches from the
//! server to the client carrying its state.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::time::{Duration, Instant};

use rustnative_core::{Component, ComponentTree, Event, Node, NodeId};
use rustnative_sync::live::{
    ClientModules, LiveApp, LiveClient, LiveServer, RenderMode, Subtree, SubtreeProps,
};
use serde_json::{Value, json};

/// A counter with a name field, whose state is its snapshot.
struct Counter {
    count: i64,
    name: String,
    snapshot: Option<Value>,
}

impl Component for Counter {
    type Props = Option<Value>;
    type Message = ();
    fn new(snapshot: Option<Value>) -> Self {
        let snapshot = snapshot.unwrap_or_default();
        let snapshot: Value = snapshot;
        Self {
            count: snapshot["count"].as_i64().unwrap_or(0),
            name: snapshot["name"].as_str().unwrap_or_default().to_owned(),
            snapshot: Some(snapshot),
        }
    }
    fn props(&self) -> &Option<Value> {
        &self.snapshot
    }
    fn set_props(&mut self, _: Option<Value>) {}
    fn view(&self) -> Node {
        Node::column(
            "counter",
            [
                Node::label("count", format!("Count {}", self.count)),
                Node::button("increment", "Add one"),
                Node::text_input("name", self.name.clone()),
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
    fn inspect(&self) -> Option<Value> {
        Some(json!({ "count": self.count, "name": self.name }))
    }
}

struct CounterApp;

impl LiveApp for CounterApp {
    type Root = Counter;
    fn root(&self, snapshot: Option<Value>) -> Counter {
        Counter::new(snapshot)
    }
}

fn label(client: &LiveClient, key: &str) -> Option<String> {
    let view = client.view()?;
    let wire = rustnative_core::wire::WireNode::from_node(&view);
    find(&wire, key)
}

fn find(node: &rustnative_core::wire::WireNode, key: &str) -> Option<String> {
    use rustnative_core::wire::WireKind;
    // A component's node carries its owner in its wire key (`owner~key`).
    if node.key == key || node.key.ends_with(&format!("~{key}")) {
        return match &node.content {
            WireKind::Label { text } => Some(text.clone()),
            WireKind::TextInput { value } => Some(value.clone()),
            _ => None,
        };
    }
    match &node.content {
        WireKind::Column { children, .. } | WireKind::Row { children, .. } => {
            children.iter().find_map(|child| find(child, key))
        }
        _ => None,
    }
}

async fn until(what: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn server() -> (std::sync::Arc<LiveServer<CounterApp>>, String) {
    let server = LiveServer::new(CounterApp, Duration::from_secs(30));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    tokio::spawn(std::sync::Arc::clone(&server).serve(listener));
    (server, url)
}

fn click() -> Event {
    Event::Click { target: NodeId::from_key("increment") }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_screen_survives_a_reconnect_and_a_deploy() {
    let (old, old_url) = server().await;
    let client = LiveClient::connect(&old_url);
    until("the first tree", || label(&client, "count").as_deref() == Some("Count 0")).await;
    client.send(&click());
    client.send(&click());
    until("two clicks", || label(&client, "count").as_deref() == Some("Count 2")).await;
    let session = client.session().unwrap();

    // The network drops; the client reconnects to the same session.
    old.drop_connections();
    until("the reconnect", || client.is_connected() && label(&client, "count").is_some()).await;
    client.send(&click());
    until("a third click", || label(&client, "count").as_deref() == Some("Count 3")).await;
    assert_eq!(client.session().unwrap(), session, "the same session: nothing was lost");
    assert_eq!(old.sessions(), 1);

    // A deploy: the old instance drains to the new one.
    let (new, new_url) = server().await;
    old.drain(&new_url, Duration::from_millis(20));
    until("the move", || client.session().is_some_and(|id| id != session) && client.is_connected())
        .await;
    until("the state on the new instance", || {
        label(&client, "count").as_deref() == Some("Count 3")
    })
    .await;
    client.send(&click());
    until("a click on the new instance", || label(&client, "count").as_deref() == Some("Count 4"))
        .await;
    assert_eq!(new.sessions(), 1);
    assert_eq!(old.sessions(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typing_is_echoed_before_the_server_answers() {
    let (_server, url) = server().await;
    let client = LiveClient::connect(&url);
    until("the first tree", || label(&client, "name").is_some()).await;
    client.send(&Event::TextChanged { target: NodeId::from_key("name"), value: "Ada".into() });
    assert_eq!(label(&client, "name").as_deref(), Some("Ada"), "at once, locally");
    until("the server's copy", || client.snapshot().is_some_and(|state| state["name"] == "Ada"))
        .await;
}

/// Hosts a subtree in a component tree, as a client screen would.
struct Screen {
    props: SubtreeProps,
}

impl Component for Screen {
    type Props = SubtreeProps;
    type Message = ();
    fn new(props: SubtreeProps) -> Self {
        Self { props }
    }
    fn props(&self) -> &SubtreeProps {
        &self.props
    }
    fn set_props(&mut self, props: SubtreeProps) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::column("screen", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut rustnative_core::ComponentContext<'_, ()>) -> Node {
        let subtree =
            context.child_with_props::<Subtree, _>("counter", self.props.clone(), Subtree::new);
        Node::column("screen", [subtree])
    }
}

fn text_of(tree: &ComponentTree, key: &str) -> Option<String> {
    find(&rustnative_core::wire::WireNode::from_node(&tree.view()), key)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_mode_moves_to_the_client_with_its_state() {
    let (_server, url) = server().await;
    let client = LiveClient::connect(&url);
    until("the first tree", || label(&client, "count").is_some()).await;
    let modules = ClientModules::new();
    let mut tree = ComponentTree::new(Screen::new(SubtreeProps {
        name: "counter".into(),
        mode: RenderMode::Auto,
        client: client.clone(),
        modules: modules.clone(),
    }));
    let _ = tree.render();
    assert_eq!(text_of(&tree, "count").as_deref(), Some("Count 0"), "server-interactive at first");

    client.send(&click());
    client.send(&click());
    until("the server's two clicks", || client.snapshot().is_some_and(|state| state["count"] == 2))
        .await;

    // The client module arrives; the next render switches, carrying the state.
    modules.register::<Counter>("counter", |snapshot| snapshot);
    let _ = tree.render();
    let view = rustnative_core::wire::WireNode::from_node(&tree.view());
    assert_eq!(
        find(&view, "count").as_deref(),
        Some("Count 2"),
        "the client component took over the state"
    );
}
