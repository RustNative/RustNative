//! A client component as a custom element (`C43-1`), on a plain HTML page
//! with no other Rust Native code, under a strict policy: it renders from
//! its attributes, follows attribute and property changes, is announced by
//! the browser as what it is, and its published values reach the page as
//! DOM events.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::needless_pass_by_value,
    clippy::must_use_candidate,
    missing_docs,
    reason = "tests, with client logic written as an application would"
)]

use std::time::Duration;

use rustnative_web::element::CustomElement;
use rustnative_web_testing::{Response, StaticServer, browser_or_skip};

#[rustnative_web::client]
pub mod stepper {
    use rustnative_core::{Event, Node, NodeId};
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Stepper {
        pub count: i32,
        pub label: String,
    }

    impl Stepper {
        pub fn update(&mut self, event: Event, fx: &mut Effects<()>) {
            if let Event::Click { target } = event {
                if target == NodeId::from_key("up") {
                    self.count += 1;
                    fx.publish("changed", &self.count);
                }
            }
        }

        pub fn view(&self) -> Node {
            Node::row(
                "stepper",
                [
                    Node::label("value", format!("{}", self.count)),
                    Node::button("up", self.label.clone()),
                ],
            )
        }
    }
}

const POLICY: &str =
    "default-src 'self'; script-src 'self'; style-src 'self'; object-src 'none'; base-uri 'none'";

#[test]
fn a_custom_element_on_a_plain_page() {
    let element = CustomElement::new::<stepper::Stepper>("rn-stepper");
    assert_eq!(element.fields(), ["count", "label"]);
    let Some(browser) = browser_or_skip("a_custom_element_on_a_plain_page") else { return };
    let server = StaticServer::start().unwrap();
    for (path, body) in element.files() {
        server.set(
            &path,
            Response::ok("text/javascript", body).header("content-security-policy", POLICY),
        );
    }
    server.set(
        "/listen.js",
        Response::ok(
            "text/javascript",
            "document.addEventListener('changed', (event) => { document.getElementById('heard').textContent = String(event.detail); });",
        ),
    );
    let page_html = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Plain</title>\
        <script type=\"module\" src=\"/listen.js\"></script><script type=\"module\" src=\"/_rn/e/rn-stepper.js\"></script></head>\
        <body><h1>A page that is not a Rust Native page</h1><rn-stepper count=\"3\" label=\"Apples\"></rn-stepper><p id=\"heard\"></p></body></html>";
    server.set(
        "/",
        Response::ok("text/html; charset=utf-8", page_html)
            .header("content-security-policy", POLICY),
    );

    let page = browser.page().unwrap();
    page.goto(&server.url("/")).unwrap();
    let wait = Duration::from_secs(10);
    let value = "document.querySelector('rn-stepper [id$=\"-value\"]')?.textContent";
    page.wait_until(&format!("{value} === '3'"), wait).unwrap();
    // Announced as what it is: a button named by its label.
    page.find_accessible("button", "Apples").unwrap();

    page.click("rn-stepper button").unwrap();
    page.wait_until(&format!("{value} === '4'"), wait).unwrap();
    page.wait_until("document.getElementById('heard').textContent === '4'", wait).unwrap();

    page.eval("document.querySelector('rn-stepper').setAttribute('count', '10')").unwrap();
    page.wait_until(&format!("{value} === '10'"), wait).unwrap();
    page.eval("document.querySelector('rn-stepper').count = 20").unwrap();
    page.wait_until(&format!("{value} === '20'"), wait).unwrap();
    assert_eq!(page.eval("document.querySelector('rn-stepper').label").unwrap(), "Apples");
    // Its rules applied, through the object model the policy allows.
    assert_eq!(
        page.eval(
            "getComputedStyle(document.querySelector('rn-stepper [id$=\"-stepper\"]')).display"
        )
        .unwrap(),
        "flex"
    );
    let console = page.console();
    assert!(
        console
            .iter()
            .all(|line| !line.contains("Content Security Policy") && !line.contains("rn:")),
        "{console:?}"
    );
}
