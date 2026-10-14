//! The new-backend checklist's rows, held for the Web in a real browser
//! (`docs/conformance/new-backend-checklist.md`, the Web column): a node's
//! cursor is the element's (row 19); a token or scheme change restyles the
//! elements already on the page, none re-rendered and nothing reloaded
//! (row 22); an island that panics gives back what it held of the host —
//! its pointer capture — while the page's other islands keep working
//! (row 10); and an island re-renders by patching the elements it owns,
//! none replaced and none leaked (row 8); and the page answers the
//! inspection protocol (row 26); and a node whose gestures win keeps the
//! browser's scrolling, selection, and long-press menu off it (row 6).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::needless_pass_by_value,
    clippy::must_use_candidate,
    clippy::semicolon_if_nothing_returned,
    clippy::unused_async,
    clippy::missing_panics_doc,
    missing_docs,
    reason = "tests, with client logic written as an application would"
)]

use std::time::Duration;

use rustnative_core::{Component, ComponentContext, Cursor, Event, Node, classes};
use rustnative_server::{ServerApp, get};
use rustnative_web::{Client, Head, Page};
use rustnative_web_testing::browser_or_skip;
use serde_json::{Value, json};

#[rustnative_web::client]
pub mod faulty {
    use rustnative_core::{
        Event, GesturePolicy, InputInterest, LayoutStyle, Node, NodeId, SizeMode,
    };
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Faulty {
        pub down: bool,
    }

    impl Faulty {
        pub fn update(&mut self, event: Event, fx: &mut Effects<()>) {
            match event {
                Event::PointerDown { target, pointer } if target == NodeId::from_key("grab") => {
                    self.down = true;
                    fx.capture_pointer("grab", pointer.pointer_id());
                }
                Event::PointerMove { .. } if self.down => panic!("the drag handler broke"),
                _ => {}
            }
        }

        pub fn view(&self) -> Node {
            Node::label_with_layout(
                "grab",
                "Grab",
                LayoutStyle::new().width(SizeMode::Fixed(120)).height(SizeMode::Fixed(120)),
            )
            // Its drag wins over the browser's scrolling and selection.
            .with_input(InputInterest::new().pointer().gesture_policy(GesturePolicy::Exclusive))
        }
    }
}

#[rustnative_web::client]
pub mod count {
    use rustnative_core::{Event, Node, NodeId};
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Count {
        pub count: i32,
    }

    impl Count {
        pub fn update(&mut self, event: Event, fx: &mut Effects<()>) {
            let _ = fx;
            if let Event::Click { target } = event {
                if target == NodeId::from_key("more") {
                    self.count += 1;
                }
            }
        }

        pub fn view(&self) -> Node {
            Node::row(
                "counter",
                [Node::label("value", format!("{}", self.count)), Node::button("more", "More")],
            )
        }
    }
}

/// A server-rendered part with a cursor and token-styled boxes, and two
/// islands.
struct Rows;

impl Component for Rows {
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
        Node::column("rows", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let faulty = context.child_with_props::<Client<faulty::Faulty>, _>(
            "faulty",
            faulty::Faulty::default(),
            Client::new,
        );
        let count = context.child_with_props::<Client<count::Count>, _>(
            "count",
            count::Count::default(),
            Client::new,
        );
        Node::column(
            "rows",
            [
                Node::label("pointer", "Points").with_cursor(Cursor::Pointer),
                Node::label("tinted", "Tinted").with_class(classes!("bg-blue-500")),
                Node::label("schemed", "Schemed").with_class(classes!("bg-white dark:bg-black")),
                faulty,
                count,
            ],
        )
    }
}

async fn rows() -> Page {
    Page::new::<Rows>(Head::new("Checklist", "The Web column of the checklist, held."), ())
}

fn serve() -> String {
    let runtime =
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let app = ServerApp::new().route("/", get(rows).public());
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

/// A computed style property of the element whose id ends in `key`.
fn computed(page: &rustnative_web_testing::Page, key: &str, property: &str) -> String {
    page.eval(&format!(
        "getComputedStyle(document.querySelector('[id$=\"{key}\"]')).getPropertyValue('{property}')"
    ))
    .unwrap()
    .as_str()
    .unwrap_or_default()
    .to_owned()
}

#[test]
fn the_web_column_of_the_checklist_in_a_browser() {
    let Some(browser) = browser_or_skip("the_web_column_of_the_checklist_in_a_browser") else {
        return;
    };
    let origin = serve();
    let page = browser.page().unwrap();
    page.goto(&format!("{origin}/")).unwrap();
    page.wait_until("document.documentElement.hasAttribute('data-rn-ready')", WAIT).unwrap();

    // Row 19: the node's cursor is the element's.
    assert_eq!(computed(&page, "pointer", "cursor"), "pointer");

    // Row 22: a token changes in place. The element is the one already on
    // the page (marked before the change), restyled, and the page did not
    // reload (the mark on `window` survives).
    page.eval("window.__tinted = document.querySelector('[id$=\"tinted\"]'); true").unwrap();
    let before = computed(&page, "tinted", "background-color");
    page.eval(&format!(
        "(async () => {{ (await {}).applyTokens({{ 'color-blue-500': 'rgb(1, 2, 3)' }}); return true; }})()",
        rt()
    ))
    .unwrap();
    let after = computed(&page, "tinted", "background-color");
    assert_ne!(before, after, "the token's new value is used");
    assert_eq!(after, "rgb(1, 2, 3)");
    assert_eq!(
        page.eval("window.__tinted === document.querySelector('[id$=\"tinted\"]')").unwrap(),
        Value::Bool(true),
        "the same element, restyled, not re-rendered"
    );

    // The scheme is a host trait the browser feeds: switching it restyles
    // the same element. (Both are set, so the machine's own setting does
    // not decide the first.)
    let scheme = |value: &str| {
        page.call(
            "Emulation.setEmulatedMedia",
            json!({ "features": [{ "name": "prefers-color-scheme", "value": value }] }),
        )
        .unwrap();
        computed(&page, "schemed", "background-color")
    };
    let light = scheme("light");
    let dark = scheme("dark");
    assert_ne!(light, dark, "the dark scheme's colour applies without a reload");
    assert_eq!(page.eval("window.__tinted !== undefined").unwrap(), Value::Bool(true));

    // Row 26: the page answers the inspection protocol: the backend, its
    // realized elements with their rectangles, the element census, and its
    // capability and style tables.
    let ask = |request: &str| {
        page.eval(&format!("(async () => (await {}).inspect('{request}'))()", rt())).unwrap()
    };
    assert_eq!(ask("hello")["backend"], "web");
    let realized = ask("realized");
    let grab = realized
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["key"] == "grab")
        .expect("the island's element is realized");
    assert_eq!(grab["host_type"], "<span>");
    assert_eq!(grab["rect"][2], 120, "its width, as laid out");
    let lifetimes = ask("lifetimes");
    assert!(lifetimes["live"].as_u64().unwrap() > 0);
    assert!(lifetimes["created"].as_u64() >= lifetimes["live"].as_u64());
    let capabilities = ask("capabilities");
    let names = |field: &str| -> Vec<String> {
        capabilities[field]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_str().map_or_else(|| row["what"].to_string(), str::to_owned))
            .collect()
    };
    assert!(names("advertised").contains(&"Http".to_owned()));
    assert!(names("refused").iter().any(|name| name.contains("Surface(TrayExtra)")));
    assert!(!capabilities["style"].as_array().unwrap().is_empty(), "the style table");
    assert!(capabilities["units"]["host_unit"].is_string());

    // Row 6: the arbitration table, as the browser applies it. A node whose
    // gestures win takes touch moves from the browser's scrolling and its
    // presses from text selection.
    assert_eq!(computed(&page, "grab", "touch-action"), "none");
    assert_eq!(computed(&page, "grab", "user-select"), "none");
    assert_eq!(computed(&page, "more", "touch-action"), "auto", "a node without gestures");

    // Row 10: the drag handler panics while it holds the pointer. The
    // island fails, the capture is given back, and the other island works.
    let (x, y) = page.center("#i0-grab").unwrap();
    page.mouse("mouseMoved", x, y, 0).unwrap();
    page.mouse("mousePressed", x, y, 1).unwrap();
    page.wait_until("document.getElementById('i0-grab').hasPointerCapture(1)", WAIT).unwrap();
    page.mouse("mouseMoved", x + 40.0, y + 40.0, 0).unwrap();
    page.wait_until(
        "document.querySelector('[data-rn-failed]') !== null \
         && !document.getElementById('i0-grab').hasPointerCapture(1)",
        WAIT,
    )
    .unwrap();
    page.mouse("mouseReleased", x + 40.0, y + 40.0, 1).unwrap();
    page.click("#i1-more").unwrap();
    page.wait_until("document.getElementById('i1-value').textContent === '1'", WAIT).unwrap();

    // Row 8: an island owns its subtree and re-renders it in place. The
    // button is the same element after three renders, and the island's
    // census of elements is what it was.
    page.eval(
        "window.__more = document.getElementById('i1-more'); \
         window.__census = document.querySelectorAll('[id^=\"i1-\"]').length; true",
    )
    .unwrap();
    for count in 2..=4 {
        page.click("#i1-more").unwrap();
        page.wait_until(
            &format!("document.getElementById('i1-value').textContent === '{count}'"),
            WAIT,
        )
        .unwrap();
    }
    assert_eq!(
        page.eval(
            "window.__more === document.getElementById('i1-more') \
             && window.__census === document.querySelectorAll('[id^=\"i1-\"]').length"
        )
        .unwrap(),
        Value::Bool(true),
        "the same elements, patched in place, none leaked"
    );
}
