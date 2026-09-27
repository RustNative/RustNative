//! Input, navigation, and browser services in a real browser (`PLAN.md`
//! Web milestones D, G, and E): an input method's composition ends in the
//! same `TextChanged`; Tab moves through the islands' controls in document
//! order; a captured pointer's moves keep arriving outside its node; links
//! navigate in the page and back and forward restore each page with its
//! islands' state; a deep link renders its route on the server; the page's
//! visibility reaches the islands; storage, the clipboard, and HTTP round
//! trip; and the capability answer says what the browser has.

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

use std::time::Duration;

use rustnative_core::{Component, ComponentContext, Event, Node};
use rustnative_server::{ServerApp, get};
use rustnative_web::form::link;
use rustnative_web::{Client, Head, Page, request};
use rustnative_web_testing::browser_or_skip;
use serde::Deserialize;
use serde_json::{Value, json};

#[rustnative_web::client]
pub mod pad {
    use rustnative_core::{Composition, Event, LayoutStyle, Node, NodeId, SizeMode};
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Pad {
        pub log: Vec<String>,
        pub text: String,
        pub down: bool,
    }

    impl Pad {
        pub fn update(&mut self, event: Event, fx: &mut Effects<()>) {
            match event {
                Event::PointerDown { target, pointer } if target == NodeId::from_key("pad") => {
                    self.down = true;
                    self.log.push("down".to_owned());
                    fx.capture_pointer("pad", pointer.pointer_id());
                }
                Event::PointerMove { pointer, .. } if self.down => {
                    let position = pointer.position();
                    self.log.push(format!("move {} {}", position.x, position.y));
                }
                Event::PointerUp { pointer, .. } if self.down => {
                    self.down = false;
                    self.log.push("up".to_owned());
                    fx.release_pointer("pad", pointer.pointer_id());
                }
                Event::TextChanged { target, value } if target == NodeId::from_key("ime") => {
                    self.text = value
                }
                Event::Composition { composition: Composition::Updated { text, .. }, .. } => {
                    self.log.push(format!("composing {text}"));
                }
                Event::Composition { composition: Composition::Committed { text }, .. } => {
                    self.log.push(format!("committed {text}"));
                }
                _ => {}
            }
        }

        pub fn view(&self) -> Node {
            Node::column(
                "pads",
                [
                    Node::label_with_layout(
                        "pad",
                        "Drag here",
                        LayoutStyle::new().width(SizeMode::Fixed(100)).height(SizeMode::Fixed(100)),
                    ),
                    Node::text_input("ime", self.text.clone()),
                ],
            )
        }
    }
}

#[rustnative_web::client]
pub mod services {
    use rustnative_core::{Event, Node, NodeId};
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Services {
        pub stored: String,
        pub pasted: String,
        pub fetched: String,
        pub caps: Vec<String>,
    }

    pub enum Msg {
        Loaded(Option<String>),
        Pasted(Result<String, String>),
        Fetched(Result<String, String>),
        Capabilities(Vec<String>),
    }

    impl Services {
        pub fn update(&mut self, event: Event, fx: &mut Effects<Msg>) {
            if let Event::Click { target } = event {
                if target == NodeId::from_key("save") {
                    fx.store("note", &"kept".to_owned());
                    fx.load("note", Msg::Loaded);
                } else if target == NodeId::from_key("copy") {
                    fx.copy("copied text");
                } else if target == NodeId::from_key("paste") {
                    fx.read_clipboard(Msg::Pasted);
                } else if target == NodeId::from_key("fetch") {
                    fx.http_get("/api/ping", Msg::Fetched);
                } else if target == NodeId::from_key("caps") {
                    fx.capabilities(Msg::Capabilities);
                }
            }
        }

        pub fn message(&mut self, message: Msg, fx: &mut Effects<Msg>) {
            let _ = fx;
            match message {
                Msg::Loaded(value) => self.stored = value.unwrap_or_default(),
                Msg::Pasted(Ok(text)) => self.pasted = text,
                Msg::Pasted(Err(error)) => self.pasted = format!("error: {error}"),
                Msg::Fetched(Ok(text)) => self.fetched = text,
                Msg::Fetched(Err(error)) => self.fetched = format!("error: {error}"),
                Msg::Capabilities(names) => self.caps = names,
            }
        }

        pub fn view(&self) -> Node {
            Node::column(
                "services",
                [
                    Node::button("save", "Save"),
                    Node::button("copy", "Copy"),
                    Node::button("paste", "Paste"),
                    Node::button("fetch", "Fetch"),
                    Node::button("caps", "Capabilities"),
                    Node::label("stored", self.stored.clone()),
                    Node::label("pasted", self.pasted.clone()),
                    Node::label("fetched", self.fetched.clone()),
                ],
            )
        }
    }
}

#[rustnative_web::client]
pub mod count {
    use rustnative_core::{Event, Lifecycle, Node, NodeId};
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Count {
        pub count: i32,
        pub hidden: i32,
        pub shown: i32,
    }

    impl Count {
        pub fn update(&mut self, event: Event, fx: &mut Effects<()>) {
            let _ = fx;
            match event {
                Event::Click { target } if target == NodeId::from_key("inc") => self.count += 1,
                Event::Lifecycle(Lifecycle::Suspending) => self.hidden += 1,
                Event::Lifecycle(Lifecycle::Resuming) => self.shown += 1,
                _ => {}
            }
        }

        pub fn view(&self) -> Node {
            Node::row(
                "counter",
                [
                    Node::label("value", format!("{}", self.count)),
                    Node::button("inc", "More"),
                    Node::label("life", format!("{} {}", self.hidden, self.shown)),
                ],
            )
        }
    }
}

/// Two islands side by side.
struct Input;

impl Component for Input {
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
        Node::column("input", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let pad = context.child_with_props::<Client<pad::Pad>, _>(
            "pad",
            pad::Pad::default(),
            Client::new,
        );
        let services = context.child_with_props::<Client<services::Services>, _>(
            "services",
            services::Services::default(),
            Client::new,
        );
        Node::column("input", [pad, services])
    }
}

/// `/a` and `/b`: a counter each, and a link to the other.
struct Nav {
    here: &'static str,
}

#[derive(Deserialize)]
struct Start {
    #[serde(default)]
    start: i32,
}

impl Component for Nav {
    type Props = &'static str;
    type Message = ();
    fn new(here: &'static str) -> Self {
        Self { here }
    }
    fn props(&self) -> &&'static str {
        &self.here
    }
    fn set_props(&mut self, here: &'static str) {
        self.here = here;
    }
    fn view(&self) -> Node {
        Node::column("nav", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        // A typed query parameter: `/b?start=5` starts the counter at 5.
        let start = request(context)
            .and_then(|request| request.query_as::<Start>().ok())
            .map_or(0, |query| query.start);
        let counter = context.child_with_props::<Client<count::Count>, _>(
            "count",
            count::Count { count: start, ..Default::default() },
            Client::new,
        );
        let (other, text) = if self.here == "a" { ("/b", "Go to B") } else { ("/a", "Go to A") };
        Node::column(
            "nav",
            [
                Node::label("here", self.here.to_uppercase()),
                counter,
                link(context, "go", text, other),
            ],
        )
    }
}

fn head(title: &str) -> Head {
    Head::new(title, "A page served to a real browser by the input and navigation tests.")
}

async fn input() -> Page {
    Page::new::<Input>(head("Input"), ())
}

async fn a() -> Page {
    Page::new::<Nav>(head("A"), "a")
}

async fn b() -> Page {
    Page::new::<Nav>(head("B"), "b")
}

async fn ping() -> &'static str {
    "pong"
}

fn serve() -> String {
    let runtime =
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let app = ServerApp::new()
        .route("/input", get(input).public())
        .route("/a", get(a).public())
        .route("/b", get(b).public())
        .route("/api/ping", get(ping).public())
        // Reading the clipboard is a declared capability; nothing else is.
        .capabilities(&[rustnative_core::Capability::Clipboard]);
    let listener = runtime.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        runtime.block_on(app.into_service().serve(listener, std::future::pending()))
    });
    format!("http://localhost:{port}")
}

const WAIT: Duration = Duration::from_secs(10);

fn rt() -> String {
    format!("import('{}')", rustnative_web::runtime::runtime_url("/_rn/"))
}

fn state(page: &rustnative_web_testing::Page, island: usize) -> Value {
    page.eval(&format!("(async () => (await {}).islands[{island}].state)()", rt())).unwrap()
}

fn ready(page: &rustnative_web_testing::Page) {
    page.wait_until("document.documentElement.hasAttribute('data-rn-ready')", WAIT).unwrap();
}

#[test]
fn input_navigation_and_services_in_a_browser() {
    let Some(browser) = browser_or_skip("input_navigation_and_services_in_a_browser") else {
        return;
    };
    let origin = serve();
    let rt = rt();

    let page = browser.page().unwrap();
    page.call("Emulation.setFocusEmulationEnabled", json!({ "enabled": true })).unwrap();
    page.goto(&format!("{origin}/input")).unwrap();
    ready(&page);

    // An input method: composition events, and the committed text as the
    // control's value.
    page.focus("#i0-ime").unwrap();
    page.call(
        "Input.imeSetComposition",
        json!({ "text": "にほ", "selectionStart": 2, "selectionEnd": 2 }),
    )
    .unwrap();
    page.call("Input.insertText", json!({ "text": "日本" })).unwrap();
    page.wait_until(
        &format!("(async () => (await {rt}).islands[0].state.text === '日本')()"),
        WAIT,
    )
    .unwrap();
    let log = state(&page, 0)["log"].clone();
    assert!(log.as_array().unwrap().iter().any(|line| line == "composing にほ"), "{log}");

    // Tab: the islands' controls in document order.
    page.press("Tab").unwrap();
    assert_eq!(page.eval("document.activeElement.id").unwrap(), "i1-save");
    page.press("Tab").unwrap();
    assert_eq!(page.eval("document.activeElement.id").unwrap(), "i1-copy");

    // A captured pointer: its moves arrive outside the pad, and so does
    // its release.
    let (x, y) = page.center("#i0-pad").unwrap();
    let mouse = |kind: &str, x: f64, y: f64| {
        page.call(
            "Input.dispatchMouseEvent",
            json!({ "type": kind, "x": x, "y": y, "button": "left", "buttons": u8::from(kind != "mouseReleased"), "clickCount": 1 }),
        )
        .unwrap();
    };
    mouse("mouseMoved", x, y);
    mouse("mousePressed", x, y);
    page.wait_until(&format!("(async () => (await {rt}).islands[0].state.down)()"), WAIT).unwrap();
    mouse("mouseMoved", x + 300.0, y + 200.0);
    mouse("mouseReleased", x + 300.0, y + 200.0);
    page.wait_until(&format!("(async () => !(await {rt}).islands[0].state.down)()"), WAIT).unwrap();
    let log: Vec<String> = serde_json::from_value(state(&page, 0)["log"].clone()).unwrap();
    let outside = log.iter().filter_map(|line| line.strip_prefix("move ")).any(|position| {
        position.split(' ').next().and_then(|x| x.parse::<i32>().ok()).is_some_and(|x| x > 100)
    });
    assert!(outside, "a move outside the pad reached it: {log:?}");
    assert_eq!(log.last().map(String::as_str), Some("up"), "{log:?}");

    // Storage, the clipboard, and HTTP.
    page.grant(&origin, &["clipboardReadWrite", "clipboardSanitizedWrite"]).unwrap();
    page.click("#i1-save").unwrap();
    page.wait_until("document.getElementById('i1-stored').textContent === 'kept'", WAIT).unwrap();
    page.click("#i1-copy").unwrap();
    page.click("#i1-paste").unwrap();
    page.wait_until("document.getElementById('i1-pasted').textContent !== ''", WAIT).unwrap();
    assert_eq!(
        page.eval("document.getElementById('i1-pasted').textContent").unwrap(),
        "copied text"
    );
    page.click("#i1-fetch").unwrap();
    page.wait_until("document.getElementById('i1-fetched').textContent === 'pong'", WAIT).unwrap();

    // What the browser has: the headless browser has storage and HTTP, and
    // no Bluetooth or sensors.
    page.click("#i1-caps").unwrap();
    page.wait_until(
        &format!("(async () => (await {rt}).islands[1].state.caps.length > 0)()"),
        WAIT,
    )
    .unwrap();
    let caps: Vec<String> = serde_json::from_value(state(&page, 1)["caps"].clone()).unwrap();
    for present in ["fetch", "store", "load", "copy", "db_put"] {
        assert!(caps.iter().any(|name| name == present), "{present} in {caps:?}");
    }
    for absent in ["bluetooth", "sensor"] {
        assert!(!caps.iter().any(|name| name == absent), "no {absent} in {caps:?}");
    }
    // The permissions policy closes what the application did not declare.
    let policy = page.eval("document.permissionsPolicy ? document.permissionsPolicy.allowsFeature('geolocation') : false").unwrap();
    assert_eq!(policy, false);

    // Navigation in the page: the runtime stays loaded, the head follows,
    // and back and forward restore each page with its island's state.
    let nav = browser.page().unwrap();
    nav.goto(&format!("{origin}/a")).unwrap();
    ready(&nav);
    nav.eval("window.stayed = true").unwrap();
    for _ in 0..3 {
        nav.click("#i0-inc").unwrap();
    }
    nav.wait_until("document.getElementById('i0-value').textContent === '3'", WAIT).unwrap();
    nav.click("#go").unwrap();
    nav.wait_until(
        "location.pathname === '/b' && document.getElementById('here')?.textContent === 'B'",
        WAIT,
    )
    .unwrap();
    ready(&nav);
    assert_eq!(nav.eval("document.title").unwrap(), "B");
    assert_eq!(nav.eval("window.stayed === true").unwrap(), true, "no full load");
    nav.wait_until("document.getElementById('i0-value')?.textContent === '0'", WAIT).unwrap();
    nav.click("#i0-inc").unwrap();
    nav.wait_until("document.getElementById('i0-value').textContent === '1'", WAIT).unwrap();
    nav.eval("history.back()").unwrap();
    nav.wait_until(
        "location.pathname === '/a' && document.getElementById('i0-value')?.textContent === '3'",
        WAIT,
    )
    .unwrap();
    nav.eval("history.forward()").unwrap();
    nav.wait_until(
        "location.pathname === '/b' && document.getElementById('i0-value')?.textContent === '1'",
        WAIT,
    )
    .unwrap();
    assert_eq!(nav.eval("window.stayed === true").unwrap(), true);

    // The page's visibility, as lifecycle events.
    nav.eval(
        "Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'hidden' }); \
         document.dispatchEvent(new Event('visibilitychange')); \
         Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'visible' }); \
         document.dispatchEvent(new Event('visibilitychange'));",
    )
    .unwrap();
    nav.wait_until("document.getElementById('i0-life').textContent === '1 1'", WAIT).unwrap();

    // A deep link renders its route on the server, typed query included.
    let deep = browser.page().unwrap();
    deep.set_script_enabled(false).unwrap();
    deep.goto(&format!("{origin}/b?start=5")).unwrap();
    assert_eq!(deep.eval("document.getElementById('i0-value').textContent").unwrap(), "5");
    assert_eq!(deep.eval("document.getElementById('go').getAttribute('href')").unwrap(), "/a");
    let console = nav.console();
    assert!(
        console
            .iter()
            .all(|line| !line.contains("rn:mismatch") && !line.contains("Content Security Policy")),
        "{console:?}"
    );
}
