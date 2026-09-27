//! Pages in a real browser (`PLAN.md` Web milestones A and H), served by
//! the application on a socket: an island attaches to the server's markup
//! and updates on click, input, and toggle, its DOM after each step being
//! the server's rendering of the state the runtime reports; one patch per
//! microtask; a server function called from the island, and refused
//! without the token; a streamed boundary filled in place; a form that
//! works with scripts off; and a subtree held on the server that survives a
//! dropped socket and, in `Auto` mode, hands over to the client module with
//! its state.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::needless_pass_by_value,
    clippy::must_use_candidate,
    clippy::semicolon_if_nothing_returned,
    clippy::unused_async,
    missing_docs,
    reason = "tests, with client logic written as an application would"
)]

use std::sync::Arc;
use std::time::Duration;

use rustnative_core::{Component, ComponentContext, Event, Node};
use rustnative_server::functions::server_fn;
use rustnative_server::{Form, Redirect, ServerApp, ServerError, get, post};
use rustnative_sync::live::{LiveApp, LiveServer};
use rustnative_web::form::form;
use rustnative_web::live::{Live, LiveProps};
use rustnative_web::{Client, Head, Page, Strategy, pending, request};
use rustnative_web_testing::browser_or_skip;
use serde::Deserialize;

#[rustnative_web::client]
pub mod counter {
    use rustnative_core::server_fn::{ServerFn, ServerFnError};
    use rustnative_core::{Event, Node, NodeId};
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    pub struct Double;
    impl ServerFn for Double {
        const PATH: &'static str = "double";
        type Input = i32;
        type Output = i32;
    }

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Counter {
        pub count: i32,
        pub name: String,
        pub on: bool,
        pub doubled: i32,
    }

    pub enum Msg {
        Doubled(Result<i32, ServerFnError>),
    }

    impl Counter {
        pub fn update(&mut self, event: Event, fx: &mut Effects<Msg>) {
            match event {
                Event::Click { target } if target == NodeId::from_key("add") => self.count += 1,
                Event::Click { target } if target == NodeId::from_key("ask") => {
                    fx.call::<Double>(self.count, Msg::Doubled);
                }
                Event::TextChanged { target, value } if target == NodeId::from_key("name") => {
                    self.name = value;
                }
                Event::Toggled { target, on } if target == NodeId::from_key("on") => self.on = on,
                _ => {}
            }
        }

        pub fn message(&mut self, message: Msg, fx: &mut Effects<Msg>) {
            let _ = fx;
            match message {
                Msg::Doubled(Ok(value)) => self.doubled = value,
                Msg::Doubled(Err(_)) => self.doubled = -1,
            }
        }

        pub fn view(&self) -> Node {
            Node::column(
                "counter",
                [
                    Node::label("count", format!("Count: {}", self.count)),
                    Node::button("add", "Add"),
                    Node::text_input("name", self.name.clone()),
                    Node::label("greeting", format!("Hello, {}", self.name)),
                    Node::toggle("on", "On", self.on),
                    Node::button("ask", "Double"),
                    Node::label("doubled", format!("Doubled: {}", self.doubled)),
                ],
            )
        }
    }
}

#[rustnative_web::client]
pub mod tally {
    use rustnative_core::{Event, Node, NodeId};
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Tally {
        pub count: i32,
    }

    impl Tally {
        pub fn update(&mut self, event: Event, fx: &mut Effects<()>) {
            let _ = fx;
            if let Event::Click { target } = event {
                if target == NodeId::from_key("bump") {
                    self.count += 1;
                }
            }
        }

        pub fn view(&self) -> Node {
            Node::row(
                "tally",
                [
                    Node::label("total", format!("Tally: {}", self.count)),
                    Node::button("bump", "Bump"),
                ],
            )
        }
    }
}

/// The home page: an island, and a list that takes a moment to load.
struct Home {
    notes: Option<Vec<String>>,
}

impl Component for Home {
    type Props = ();
    type Message = Vec<String>;
    fn new((): ()) -> Self {
        Self { notes: None }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("home", [])
    }
    fn update(&mut self, _: Event) {}
    fn message(&mut self, notes: Vec<String>) {
        self.notes = Some(notes);
    }
    fn render(&mut self, context: &mut ComponentContext<'_, Vec<String>>) -> Node {
        if self.notes.is_none() && context.task_scope().task_count() == 0 {
            context.spawn(async {
                tokio::time::sleep(Duration::from_millis(300)).await;
                vec!["Milk".to_owned(), "Bread".to_owned()]
            });
        }
        let island = context.child_with_props::<Client<counter::Counter>, _>(
            "counter",
            counter::Counter { count: 1, ..counter::Counter::default() },
            Client::new,
        );
        let list = self.notes.as_ref().map(|notes| {
            Node::column(
                "list",
                notes.iter().map(|note| Node::label(note.to_lowercase(), note.clone())),
            )
        });
        Node::column(
            "home",
            [
                Node::label("title", "Home"),
                island,
                pending(context, "notes", list, || Node::label("loading", "Loading…")),
            ],
        )
    }
}

/// A form, and what the last submission saved.
struct Notes;

impl Component for Notes {
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
        Node::column("notes", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let saved =
            request(context).and_then(|request| request.query_param("saved")).unwrap_or_default();
        Node::column(
            "notes",
            [
                Node::label("saved", format!("Saved: {saved}")),
                form(
                    context,
                    "new",
                    "/notes",
                    [Node::text_input("title", ""), Node::button("save", "Save")],
                ),
            ],
        )
    }
}

/// A subtree held on the server until its client module takes over.
struct Held;

impl Component for Held {
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
        Node::column("held", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let live = context.child_with_props::<Live<Client<tally::Tally>>, _>(
            "tally",
            LiveProps::auto("/_rn/live/tally", tally::Tally { count: 0 }),
            Live::new,
        );
        Node::column("held", [Node::label("title", "Held"), live])
    }
}

struct TallyApp;

impl LiveApp for TallyApp {
    type Root = Client<tally::Tally>;
    fn root(&self, snapshot: Option<serde_json::Value>) -> Client<tally::Tally> {
        Client::new(
            snapshot.and_then(|value| serde_json::from_value(value).ok()).unwrap_or_default(),
        )
    }
}

#[derive(Deserialize)]
struct NewNote {
    title: String,
}

fn head(title: &str) -> Head {
    Head::new(title, "A page served to a real browser by the page tests.")
}

async fn home() -> Page {
    Page::new::<Home>(head("Home"), ()).strategy(Strategy::Streamed).lang("en")
}

async fn notes() -> Page {
    Page::new::<Notes>(head("Notes"), ()).lang("en")
}

async fn save(Form(note): Form<NewNote>) -> Redirect {
    let title: String = note.title.chars().filter(char::is_ascii_alphanumeric).collect();
    Redirect::see_other(format!("/notes?saved={title}"))
}

async fn held() -> Page {
    Page::new::<Held>(head("Held"), ()).lang("en")
}

/// The server's rendering of a counter state, as the browser's elements.
async fn rendering(body: bytes::Bytes) -> rustnative_server::Json<serde_json::Value> {
    let input: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let state: counter::Counter = serde_json::from_value(input["state"].clone()).unwrap();
    let options = serde_json::json!({ "scope": "i0-", "flow": input["flow"] });
    let encoded = rustnative_web::live::encode(&state.view(), &options);
    rustnative_server::Json(encoded["element"].clone())
}

#[derive(Debug)]
struct RawBody(bytes::Bytes);

impl rustnative_server::FromRequest for RawBody {
    fn from_request(request: &rustnative_server::RequestContext) -> Result<Self, ServerError> {
        Ok(Self(request.body().clone()))
    }
}

fn serve() -> (String, Arc<LiveServer<TallyApp>>) {
    let runtime =
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let live = LiveServer::new(TallyApp, Duration::from_secs(30));
    live.set_browser_encoding(Arc::new(rustnative_web::live::encode));
    let app = ServerApp::new()
        .route("/", get(home).public())
        .route("/notes", get(notes).post(save).public())
        .route("/held", get(held).public())
        .route("/render", post(|RawBody(body): RawBody| rendering(body)).csrf_exempt().public())
        .function::<counter::Double>(
            server_fn::<counter::Double, _, _>(|value: i32| async move {
                Ok::<_, ServerError>(value * 2)
            })
            .public(),
        );
    let app = live.mount(app, "/_rn/live/tally");
    let listener = runtime.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        runtime.block_on(app.into_service().serve(listener, std::future::pending()))
    });
    (format!("http://localhost:{port}"), live)
}

const WAIT: Duration = Duration::from_secs(10);

fn runtime_import() -> String {
    format!("import('{}')", rustnative_web::runtime::runtime_url("/_rn/"))
}

#[test]
fn pages_in_a_browser() {
    let Some(browser) = browser_or_skip("pages_in_a_browser") else { return };
    let (origin, _live) = serve();
    let rt = runtime_import();

    // An island attaches to the server's markup, and a streamed boundary
    // fills in place.
    let page = browser.page().unwrap();
    page.goto(&format!("{origin}/")).unwrap();
    page.wait_until("document.documentElement.hasAttribute('data-rn-ready')", WAIT).unwrap();
    page.wait_until("document.getElementById('bread') !== null", WAIT).unwrap();
    assert_eq!(page.eval("document.getElementById('loading')").unwrap(), serde_json::Value::Null);
    let console = page.console();
    assert!(
        console
            .iter()
            .all(|line| !line.contains("rn:mismatch") && !line.contains("Content Security Policy")),
        "{console:?}"
    );

    // After each step, the island's DOM is the server's rendering of the
    // state the runtime reports.
    let agrees = format!(
        "(async () => {{ const rt = await {rt}; const island = rt.islands[0]; \
         const response = await fetch('/render', {{ method: 'POST', body: JSON.stringify({{ state: island.state, flow: island.flow }}) }}); \
         return rt.mismatch(island.root, await response.json()); }})()"
    );
    assert_eq!(page.eval(&agrees).unwrap(), serde_json::Value::Null);

    page.click("#i0-add").unwrap();
    page.wait_until("document.getElementById('i0-count').textContent === 'Count: 2'", WAIT)
        .unwrap();
    assert_eq!(page.eval(&agrees).unwrap(), serde_json::Value::Null);

    page.focus("#i0-name").unwrap();
    page.type_text("Ada").unwrap();
    page.wait_until("document.getElementById('i0-greeting').textContent === 'Hello, Ada'", WAIT)
        .unwrap();
    assert_eq!(page.eval(&agrees).unwrap(), serde_json::Value::Null);

    page.click("#i0-on").unwrap();
    page.wait_until(&format!("(async () => (await {rt}).islands[0].state.on)()"), WAIT).unwrap();
    assert_eq!(page.eval(&agrees).unwrap(), serde_json::Value::Null);

    // Three events in one task: one patch.
    let renders = page
        .eval(&format!(
            "(async () => {{ const island = (await {rt}).islands[0]; const before = island.renders; \
             for (let i = 0; i < 3; i++) island.dispatch({{ type: 'Click', target: 'add' }}); \
             await new Promise((resolve) => setTimeout(resolve, 50)); \
             return [island.renders - before, island.state.count]; }})()"
        ))
        .unwrap();
    assert_eq!(renders, serde_json::json!([1, 5]));
    assert_eq!(page.eval(&agrees).unwrap(), serde_json::Value::Null);

    // A server function, with the page's token; without it, refused.
    page.click("#i0-ask").unwrap();
    page.wait_until("document.getElementById('i0-doubled').textContent === 'Doubled: 10'", WAIT)
        .unwrap();
    let refused = page
        .eval("fetch('/_fn/double', { method: 'POST', headers: { 'content-type': 'application/json' }, body: '3' }).then((r) => r.status)")
        .unwrap();
    assert_eq!(refused, 403);

    // A form, with scripts off.
    let form = browser.page().unwrap();
    form.set_script_enabled(false).unwrap();
    form.goto(&format!("{origin}/notes")).unwrap();
    form.focus("#title").unwrap();
    form.type_text("Milk").unwrap();
    let _ = form.take_events("Page.loadEventFired");
    form.click("#save").unwrap();
    form.wait_event("Page.loadEventFired", WAIT).unwrap();
    assert_eq!(form.eval("document.getElementById('saved').textContent").unwrap(), "Saved: Milk");

    // Held on the server: the module is blocked, so it stays live.
    let held = browser.page().unwrap();
    held.call("Network.setBlockedURLs", serde_json::json!({ "urls": ["*/_rn/m/*"] })).unwrap();
    held.goto(&format!("{origin}/held")).unwrap();
    held.wait_until(&format!("(async () => (await {rt}).islands[0]?.frames > 0)()"), WAIT).unwrap();
    assert_eq!(
        held.eval(&format!("(async () => (await {rt}).islands[0].constructor.name)()")).unwrap(),
        "LiveIsland"
    );
    held.click("#i0-bump").unwrap();
    held.click("#i0-bump").unwrap();
    held.wait_until("document.getElementById('i0-total').textContent === 'Tally: 2'", WAIT)
        .unwrap();

    // The socket drops; the session survives it.
    held.eval(&format!("(async () => {{ (await {rt}).islands[0].socket.close(); }})()")).unwrap();
    held.wait_until(
        &format!("(async () => (await {rt}).islands[0].socket?.readyState === 1)()"),
        WAIT,
    )
    .unwrap();
    held.click("#i0-bump").unwrap();
    held.wait_until("document.getElementById('i0-total').textContent === 'Tally: 3'", WAIT)
        .unwrap();

    // `Auto`: once the client module loads, it takes over with the state.
    held.call("Network.setBlockedURLs", serde_json::json!({ "urls": [] })).unwrap();
    held.eval(&format!("(async () => {{ await (await {rt}).islands[0].handoff(); }})()")).unwrap();
    let handed = held
        .eval(&format!("(async () => {{ const island = (await {rt}).islands[0]; return [island.constructor.name, island.state.count]; }})()"))
        .unwrap();
    assert_eq!(handed, serde_json::json!(["Island", 3]));
    held.click("#i0-bump").unwrap();
    held.wait_until("document.getElementById('i0-total').textContent === 'Tally: 4'", WAIT)
        .unwrap();
    let console = held.console();
    assert!(console.iter().all(|line| !line.contains("rn:mismatch")), "{console:?}");
}
