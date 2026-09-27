//! Static export and source maps (`PLAN.md` Web milestone J): a site
//! exported to files works from a static host under one policy with no
//! nonce; each route ships only what it uses; and a client module's source
//! map leads each generated line back to the Rust it came from.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::needless_pass_by_value,
    clippy::must_use_candidate,
    missing_docs,
    reason = "tests, with client logic written as an application would"
)]

use std::path::Path;

use rustnative_core::{Component, ComponentContext, Event, Node};
use rustnative_web::export::Site;
use rustnative_web::{Client, Head, Page};
use rustnative_web_testing::{Response, StaticServer, browser_or_skip};

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

struct Home(bool);

impl Component for Home {
    type Props = bool;
    type Message = ();
    fn new(island: bool) -> Self {
        Self(island)
    }
    fn props(&self) -> &bool {
        &self.0
    }
    fn set_props(&mut self, island: bool) {
        self.0 = island;
    }
    fn view(&self) -> Node {
        Node::column("home", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let mut children = vec![Node::label("title", if self.0 { "Counter" } else { "About" })];
        if self.0 {
            children.push(context.child_with_props::<Client<tally::Tally>, _>(
                "tally",
                tally::Tally { count: 2 },
                Client::new,
            ));
        }
        Node::column("home", children)
    }
}

fn site() -> Site {
    let head = |title: &str| Head::new(title, "A page of the exported site in the export tests.");
    Site::new()
        .page("/", move || Page::new::<Home>(head("Counter"), true).lang("en"))
        .page("/about", move || Page::new::<Home>(head("About"), false).lang("en"))
        .origin("https://example.com")
}

fn content_type(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css",
        Some("js") => "text/javascript",
        Some("json") => "application/json",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[test]
fn an_exported_site_works_from_a_static_host() {
    let folder = std::env::temp_dir().join(format!("rn-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let report = site().export(&folder).unwrap();

    // Each route ships only what it uses.
    let route = |path: &str| report.routes.iter().find(|route| route.path == path).unwrap().clone();
    assert_eq!(route("/about").script_bytes, 0, "no island, no JavaScript");
    assert!(route("/").script_bytes > rustnative_web::runtime::RUNTIME_JS.len());
    assert!(route("/").css_bytes > 0);
    for file in ["index.html", "about/index.html", "_headers", "sitemap.xml", "_rn/report.json"] {
        assert!(folder.join(file).exists(), "{file}");
    }
    let about = std::fs::read_to_string(folder.join("about/index.html")).unwrap();
    assert!(
        !about.contains("<script")
            && !about.contains("<style")
            && about.contains("<link rel=\"stylesheet\""),
        "{about}"
    );
    let headers = std::fs::read_to_string(folder.join("_headers")).unwrap();
    let policy = headers
        .lines()
        .find_map(|line| line.trim().strip_prefix("Content-Security-Policy: "))
        .unwrap()
        .to_owned();
    assert!(
        policy.contains("script-src 'self';")
            && policy.contains("style-src 'self';")
            && !policy.contains("nonce"),
        "{policy}"
    );
    assert!(
        std::fs::read_to_string(folder.join("sitemap.xml"))
            .unwrap()
            .contains("<loc>https://example.com/about</loc>")
    );

    // Served as a static host would, under that one policy.
    let Some(browser) = browser_or_skip("an_exported_site_works_from_a_static_host") else {
        return;
    };
    let server = StaticServer::start().unwrap();
    for file in &report.files {
        let path = folder.join(file.trim_start_matches('/'));
        let body = std::fs::read(&path).unwrap();
        let address = file.strip_suffix("index.html").unwrap_or(file);
        server.set(
            address,
            Response::ok(content_type(file), body).header("content-security-policy", &policy),
        );
    }
    let page = browser.page().unwrap();
    page.goto(&server.url("/")).unwrap();
    page.wait_until(
        "document.documentElement.hasAttribute('data-rn-ready')",
        std::time::Duration::from_secs(10),
    )
    .unwrap();
    page.click("#i0-bump").unwrap();
    page.wait_until(
        "document.getElementById('i0-total').textContent === 'Tally: 3'",
        std::time::Duration::from_secs(10),
    )
    .unwrap();
    // The stylesheet applied: the row is a flex row.
    assert_eq!(
        page.eval("getComputedStyle(document.getElementById('i0-tally')).display").unwrap(),
        "flex"
    );
    let console = page.console();
    assert!(
        console
            .iter()
            .all(|line| !line.contains("Content Security Policy") && !line.contains("rn:mismatch")),
        "{console:?}"
    );
    let _ = std::fs::remove_dir_all(&folder);
}

/// Decodes a source map's `mappings`: for each generated line, the source
/// line of its first segment.
fn decode(mappings: &str) -> Vec<Option<i64>> {
    const DIGITS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut line = 0;
    mappings
        .split(';')
        .map(|segment| {
            if segment.is_empty() {
                return None;
            }
            let mut values = Vec::new();
            let (mut value, mut shift) = (0i64, 0);
            for character in segment.split(',').next().unwrap().chars() {
                let digit = i64::try_from(DIGITS.find(character).unwrap()).unwrap();
                value += (digit & 31) << shift;
                if digit & 32 == 0 {
                    values.push(if value & 1 == 1 { -(value >> 1) } else { value >> 1 });
                    (value, shift) = (0, 0);
                } else {
                    shift += 5;
                }
            }
            line += values[2];
            Some(line)
        })
        .collect()
}

#[test]
fn a_source_map_leads_generated_lines_back_to_the_rust() {
    let module = &tally::__RUSTNATIVE_CLIENT_MODULE;
    let map: serde_json::Value = serde_json::from_str(&module.source_map()).unwrap();
    assert_eq!(map["version"], 3);
    assert!(map["sources"][0].as_str().unwrap().ends_with("tests/export.rs"), "{map}");
    let lines = decode(map["mappings"].as_str().unwrap());
    // The generated line that adds to the count comes from `self.count += 1`.
    let generated =
        module.js.lines().position(|line| line.contains("rn.add(")).expect("the addition");
    let source = include_str!("export.rs")
        .lines()
        .position(|line| line.trim() == "self.count += 1;")
        .unwrap();
    assert_eq!(lines[generated], Some(i64::try_from(source).unwrap()), "{}", module.js);
}
