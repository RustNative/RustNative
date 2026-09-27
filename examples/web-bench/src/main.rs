//! The web budget scenarios (`PLAN.md` Web milestone J, `C42-4`): what
//! `rustnative bench --target web` runs and measures against
//! `budgets/web.toml`. `--scenario web` prints one JSON object:
//!
//! - `route_script_kb`: the JavaScript the home route loads (the runtime and
//!   its island's module);
//! - `unrelated_routes_script_growth_kb`: how much that grows when ten
//!   unrelated routes, each with a heavy module, join the site — nothing,
//!   because each route loads only its own modules;
//! - on a throttled profile (CPU ×4, 150 ms and 1.6 Mb/s), in headless
//!   Edge: `startup_ms` (to the islands being ready), `lcp_ms` (largest
//!   contentful paint), `cls` (cumulative layout shift), and `inp_ms` (a
//!   click's interaction to next paint).

#![allow(
    clippy::expect_used,
    clippy::needless_pass_by_value,
    clippy::semicolon_if_nothing_returned,
    reason = "a measurement harness, with client logic written as an application would"
)]

use std::process::ExitCode;
use std::time::Duration;

use rustnative_core::{Component, ComponentContext, Event, Node};
use rustnative_server::{ServerApp, get};
use rustnative_web::export::Site;
use rustnative_web::{Client, Head, Page};
use serde_json::json;

#[rustnative_web::client]
pub mod counter {
    use rustnative_core::{Event, Node, NodeId};
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    /// A counter.
    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Counter {
        /// The count.
        pub count: i32,
    }

    impl Counter {
        /// Counts clicks.
        pub fn update(&mut self, event: Event, fx: &mut Effects<()>) {
            let _ = fx;
            if let Event::Click { target } = event {
                if target == NodeId::from_key("add") {
                    self.count += 1;
                }
            }
        }

        /// The count and a button.
        #[must_use]
        pub fn view(&self) -> Node {
            Node::row(
                "counter",
                [Node::label("count", format!("{}", self.count)), Node::button("add", "Add")],
            )
        }
    }
}

#[rustnative_web::client]
pub mod heavy {
    use rustnative_core::{Event, Node, NodeId};
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    /// A report with a lot of logic: what an unrelated heavy route runs.
    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Report {
        /// Its rows.
        pub rows: Vec<String>,
        /// Its filter.
        pub filter: String,
    }

    impl Report {
        /// Filters, sorts, and edits.
        pub fn update(&mut self, event: Event, fx: &mut Effects<()>) {
            let _ = fx;
            match event {
                Event::TextChanged { target, value } if target == NodeId::from_key("filter") => {
                    self.filter = value;
                }
                Event::Click { target } if target == NodeId::from_key("sort") => self.rows.sort(),
                Event::Click { target } if target == NodeId::from_key("reverse") => {
                    self.rows.reverse()
                }
                Event::Click { target } if target == NodeId::from_key("add") => {
                    let next = format!(
                        "row {} of {}",
                        self.rows.len() + 1,
                        self.filter.trim().to_uppercase()
                    );
                    self.rows.push(next);
                }
                Event::Click { target } if target == NodeId::from_key("dedup") => self.rows.dedup(),
                Event::Click { target } if target == NodeId::from_key("clear") => self.rows.clear(),
                _ => {}
            }
        }

        /// The filtered rows and a summary.
        #[must_use]
        pub fn view(&self) -> Node {
            let shown: Vec<&String> =
                self.rows.iter().filter(|row| row.contains(self.filter.as_str())).collect();
            let longest = shown.iter().map(|row| row.chars().count()).max().unwrap_or(0);
            let total: usize = shown.iter().map(|row| row.len()).sum();
            Node::column(
                "report",
                [
                    Node::text_input("filter", self.filter.clone()),
                    Node::row(
                        "actions",
                        ["sort", "reverse", "add", "dedup", "clear"]
                            .map(|key| Node::button(key, key)),
                    ),
                    Node::label(
                        "summary",
                        format!("{} shown, longest {longest}, {total} bytes", shown.len()),
                    ),
                    Node::column(
                        "rows",
                        shown
                            .iter()
                            .enumerate()
                            .map(|(index, row)| Node::label(format!("r{index}"), (*row).clone())),
                    ),
                ],
            )
        }
    }
}

/// A route: the counter (home), or a heavy report.
struct Route(bool);

impl Component for Route {
    type Props = bool;
    type Message = ();
    fn new(home: bool) -> Self {
        Self(home)
    }
    fn props(&self) -> &bool {
        &self.0
    }
    fn set_props(&mut self, home: bool) {
        self.0 = home;
    }
    fn view(&self) -> Node {
        Node::column("page", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let island = if self.0 {
            context.child_with_props::<Client<counter::Counter>, _>(
                "counter",
                counter::Counter::default(),
                Client::new,
            )
        } else {
            context.child_with_props::<Client<heavy::Report>, _>(
                "report",
                heavy::Report::default(),
                Client::new,
            )
        };
        Node::column("page", [Node::label("title", if self.0 { "Home" } else { "Report" }), island])
    }
}

fn page(home: bool) -> Page {
    Page::new::<Route>(Head::new("Bench", "A page the web budget scenarios measure."), home)
        .lang("en")
}

fn home_script_bytes(unrelated: usize) -> usize {
    let mut site = Site::new().page("/", || page(true));
    for index in 0..unrelated {
        site = site.page(&format!("/report-{index}"), || page(false));
    }
    let folder =
        std::env::temp_dir().join(format!("rn-web-bench-{}-{unrelated}", std::process::id()));
    let report = site.export(&folder).expect("the export");
    let _ = std::fs::remove_dir_all(&folder);
    report.routes.iter().find(|route| route.path == "/").map_or(0, |route| route.script_bytes)
}

#[allow(clippy::cast_precision_loss, reason = "kilobytes")]
fn kilobytes(bytes: usize) -> f64 {
    bytes as f64 / 1024.0
}

fn browser_metrics() -> serde_json::Value {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("a runtime");
    let app = ServerApp::new().route("/", get(|| async { page(true) }).public());
    let listener = runtime.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).expect("a port");
    let port = listener.local_addr().expect("an address").port();
    std::thread::spawn(move || {
        runtime.block_on(app.into_service().serve(listener, std::future::pending()))
    });
    let browser = rustnative_web_testing::Browser::launch()
        .expect("the browser started")
        .expect("a browser (Edge or Chrome)");
    let page = browser.page().expect("a page");
    page.throttle(4.0, Some((150, 1600))).expect("throttling");
    page.goto(&format!("http://localhost:{port}/")).expect("the page");
    page.wait_until(
        "document.documentElement.hasAttribute('data-rn-ready')",
        Duration::from_secs(30),
    )
    .expect("ready");
    page.click("#i0-add").expect("a click");
    page.wait_until(
        "document.getElementById('i0-count').textContent === '1'",
        Duration::from_secs(10),
    )
    .expect("the click");
    std::thread::sleep(Duration::from_millis(300));
    let metrics = page
        .eval(&format!(
            "(async () => (await import('{}')).metrics())()",
            rustnative_web::runtime::runtime_url("/_rn/")
        ))
        .expect("the metrics");
    json!({
        "startup_ms": metrics["ready"].as_f64().unwrap_or(f64::MAX),
        "lcp_ms": metrics["lcp"].as_f64().unwrap_or(f64::MAX),
        "cls": metrics["cls"].as_f64().unwrap_or(f64::MAX),
        // Under the event-timing threshold (16 ms) there is no entry: the
        // interaction was that fast.
        "inp_ms": metrics["inp"].as_f64().unwrap_or(0.0),
    })
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().collect();
    let scenario = arguments
        .iter()
        .position(|argument| argument == "--scenario")
        .and_then(|at| arguments.get(at + 1));
    if scenario.map(String::as_str) != Some("web") {
        eprintln!("usage: web-bench --scenario web");
        return ExitCode::FAILURE;
    }
    let alone = home_script_bytes(0);
    let beside = home_script_bytes(10);
    let mut measured = browser_metrics();
    measured["route_script_kb"] = json!(kilobytes(alone));
    measured["unrelated_routes_script_growth_kb"] = json!(kilobytes(beside.saturating_sub(alone)));
    println!("{measured}");
    ExitCode::SUCCESS
}
