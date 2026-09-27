//! WebAssembly subtrees in a real browser (`PLAN.md` Web milestones A and
//! F): the example module builds for `wasm32-unknown-unknown`; a page loads
//! it only when it has a subtree that runs it; the subtree attaches to the
//! server's markup without a difference, handles events, runs a timer on the
//! host's clock, and runs in a Web Worker; and the same client component as
//! generated JavaScript, as WebAssembly, and as WebAssembly in a worker
//! leaves identical DOM after the same events.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::unused_async,
    missing_docs,
    reason = "tests"
)]

use std::path::PathBuf;
use std::time::Duration;

use rustnative_core::{Component, ComponentContext, Event, Node};
use rustnative_server::{ServerApp, get};
use rustnative_web::wasm::{WasmProps, WasmSubtree};
use rustnative_web::{Client, Head, Page};
use rustnative_web_testing::browser_or_skip;
use web_subtree::counter::Counter;
use web_subtree::notes::{Notes, NotesProps};

/// Builds the example for the browser; `None` when this machine cannot
/// (no `wasm32-unknown-unknown` target).
fn module() -> Option<Vec<u8>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = root.join("target/wasm");
    let status =
        std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .current_dir(&root)
            .args([
                "build",
                "-p",
                "web-subtree",
                "--target",
                "wasm32-unknown-unknown",
                "--release",
                "--target-dir",
            ])
            .arg(&target)
            .status()
            .ok()?;
    if !status.success() {
        eprintln!(
            "skipped: the example did not build for wasm32-unknown-unknown (is the target installed?)"
        );
        return None;
    }
    std::fs::read(target.join("wasm32-unknown-unknown/release/web_subtree.wasm")).ok()
}

/// A page with one subtree, as generated JavaScript, WebAssembly, or
/// WebAssembly in a worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Js,
    Wasm,
    Worker,
    Notes,
    Plain,
}

struct Host(Kind);

impl Component for Host {
    type Props = Kind;
    type Message = ();
    fn new(kind: Kind) -> Self {
        Self(kind)
    }
    fn props(&self) -> &Kind {
        &self.0
    }
    fn set_props(&mut self, kind: Kind) {
        self.0 = kind;
    }
    fn view(&self) -> Node {
        Node::column("host", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let start = Counter { count: 1, ..Counter::default() };
        let subtree = match self.0 {
            Kind::Js => {
                context.child_with_props::<Client<Counter>, _>("subtree", start, Client::new)
            }
            Kind::Wasm | Kind::Worker => {
                let props = WasmProps::new("web-subtree", "counter", start);
                let props = if self.0 == Kind::Worker { props.in_worker() } else { props };
                context.child_with_props::<WasmSubtree<Client<Counter>>, _>(
                    "subtree",
                    props,
                    WasmSubtree::new,
                )
            }
            Kind::Notes => {
                let props =
                    WasmProps::new("web-subtree", "notes", NotesProps { text: "hi".into() });
                context.child_with_props::<WasmSubtree<Notes>, _>(
                    "subtree",
                    props,
                    WasmSubtree::new,
                )
            }
            Kind::Plain => Node::label("plain", "Nothing runs here"),
        };
        Node::column("host", [Node::label("title", "Subtree"), subtree])
    }
}

fn page(kind: Kind) -> Page {
    Page::new::<Host>(
        Head::new("Subtree", "A page served to a real browser by the subtree tests."),
        kind,
    )
}

fn serve(module: Vec<u8>) -> String {
    let runtime =
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let app = ServerApp::new()
        .route("/js", get(|| async { page(Kind::Js) }).public())
        .route("/wasm", get(|| async { page(Kind::Wasm) }).public())
        .route("/worker", get(|| async { page(Kind::Worker) }).public())
        .route("/notes", get(|| async { page(Kind::Notes) }).public())
        .route("/plain", get(|| async { page(Kind::Plain) }).public())
        .wasm_module("web-subtree", module);
    let listener = runtime.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        runtime.block_on(app.into_service().serve(listener, std::future::pending()))
    });
    format!("http://localhost:{port}")
}

const WAIT: Duration = Duration::from_secs(15);

fn rt() -> String {
    format!("import('{}')", rustnative_web::runtime::runtime_url("/_rn/"))
}

#[test]
fn webassembly_subtrees_in_a_browser() {
    let Some(browser) = browser_or_skip("webassembly_subtrees_in_a_browser") else { return };
    let Some(module) = module() else { return };
    let origin = serve(module);
    let rt = rt();
    let fetched_wasm =
        "performance.getEntriesByType('resource').some((entry) => entry.name.endsWith('.wasm'))";

    // Pages with no subtree never fetch a module.
    let plain = browser.page().unwrap();
    plain.goto(&format!("{origin}/plain")).unwrap();
    assert_eq!(plain.eval(fetched_wasm).unwrap(), false);

    let pages: Vec<_> = ["js", "wasm", "worker"]
        .iter()
        .map(|path| {
            let page = browser.page().unwrap();
            page.goto(&format!("{origin}/{path}")).unwrap();
            page.wait_until("document.documentElement.hasAttribute('data-rn-ready')", WAIT)
                .unwrap();
            page.wait_until(
                "document.getElementById('i0-count')?.textContent === 'Count: 1'",
                WAIT,
            )
            .unwrap();
            page
        })
        .collect();
    let [js, wasm, worker] = [&pages[0], &pages[1], &pages[2]];
    assert_eq!(js.eval(fetched_wasm).unwrap(), false, "the JavaScript page loads no module");
    assert_eq!(wasm.eval(fetched_wasm).unwrap(), true);
    assert_eq!(
        wasm.eval(&format!("(async () => (await {rt}).islands[0].constructor.name)()")).unwrap(),
        "WasmIsland"
    );
    assert_eq!(
        worker.eval(&format!("(async () => (await {rt}).islands[0].constructor.name)()")).unwrap(),
        "WorkerIsland"
    );

    // Compiling WebAssembly is allowed on the page that runs it, and only there.
    let policy =
        "fetch(location.href).then((response) => response.headers.get('content-security-policy'))";
    assert!(wasm.eval(policy).unwrap().as_str().unwrap().contains("'wasm-unsafe-eval'"));
    assert!(!js.eval(policy).unwrap().as_str().unwrap().contains("wasm-unsafe-eval"));

    // The same events, the same DOM.
    let island = "document.querySelector('[data-rn-i=\"0\"]').outerHTML";
    let same = |step: &str| {
        let expected = js.eval(island).unwrap();
        for (name, page) in [("wasm", wasm), ("worker", worker)] {
            assert_eq!(page.eval(island).unwrap(), expected, "{name} after {step}");
        }
    };
    same("attaching");
    for (name, page) in [("js", js), ("wasm", wasm), ("worker", worker)] {
        page.click("#i0-add").unwrap();
        page.click("#i0-add").unwrap();
        if page
            .wait_until("document.getElementById('i0-count').textContent === 'Count: 3'", WAIT)
            .is_err()
        {
            panic!(
                "{name}: {} {:?}",
                page.eval("document.getElementById('i0-count').textContent").unwrap(),
                page.console()
            );
        }
    }
    same("two clicks");
    for (name, page) in [("js", js), ("wasm", wasm), ("worker", worker)] {
        page.focus("#i0-name").unwrap();
        page.type_text("Ada").unwrap();
        if page
            .wait_until("document.getElementById('i0-greeting').textContent === 'Hello, Ada'", WAIT)
            .is_err()
        {
            panic!(
                "{name}: {} {:?}",
                page.eval("document.getElementById('i0-greeting').textContent").unwrap(),
                page.console()
            );
        }
    }
    same("typing");
    for page in [js, wasm, worker] {
        page.click("#i0-on").unwrap();
        page.click("#i0-reset").unwrap();
        page.wait_until("document.getElementById('i0-count').textContent === 'Count: 0'", WAIT)
            .unwrap();
    }
    same("a toggle and a reset");
    for page in [js, wasm, worker] {
        let console = page.console();
        assert!(
            console
                .iter()
                .all(|line| !line.contains("rn:mismatch")
                    && !line.contains("Content Security Policy")),
            "{console:?}"
        );
    }

    // Full Rust in the browser: replicated text merged, and a timer on the
    // host's clock.
    let notes = browser.page().unwrap();
    notes.goto(&format!("{origin}/notes")).unwrap();
    notes.wait_until("document.documentElement.hasAttribute('data-rn-ready')", WAIT).unwrap();
    notes
        .wait_until("document.getElementById('i0-ticks')?.textContent === 'Ticks: 3'", WAIT)
        .unwrap();
    notes.click("#i0-type-left").unwrap();
    notes.click("#i0-type-left").unwrap();
    notes.click("#i0-type-right").unwrap();
    notes.wait_until("document.getElementById('i0-right').textContent === 'hib'", WAIT).unwrap();
    assert_eq!(notes.eval("document.getElementById('i0-left').textContent").unwrap(), "hiaa");
    notes.click("#i0-merge").unwrap();
    notes.wait_until(
        "document.getElementById('i0-left').textContent.length === 5 && \
         document.getElementById('i0-left').textContent === document.getElementById('i0-right').textContent",
        WAIT,
    )
    .unwrap();
    let merged = notes.eval("document.getElementById('i0-left').textContent").unwrap();
    let merged = merged.as_str().unwrap();
    assert!(merged.starts_with("hi") && merged.contains("aa") && merged.contains('b'), "{merged}");
    let console = notes.console();
    assert!(console.iter().all(|line| !line.contains("rn:mismatch")), "{console:?}");
}
