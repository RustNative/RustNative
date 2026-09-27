//! Pages served by the application (`PLAN.md` Web milestone H): a handler
//! returns a `Page`, rendered with the request's nonce and token; a
//! streamed page's fallback reaches the socket before its data exists; the
//! runtime and client modules are served, immutable; a call from another
//! build is answered `409`.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs, reason = "tests")]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use bytes::Bytes;
use rustnative_core::{Component, ComponentContext, Event, Node};
use rustnative_server::{ServerApp, get};
use rustnative_web::{Head, Page, Strategy, pending};

struct Slow {
    notes: Option<Vec<String>>,
}

impl Component for Slow {
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
        Node::column("page", [])
    }
    fn update(&mut self, _: Event) {}
    fn message(&mut self, notes: Vec<String>) {
        self.notes = Some(notes);
    }
    fn render(&mut self, context: &mut ComponentContext<'_, Vec<String>>) -> Node {
        if self.notes.is_none() && context.task_scope().task_count() == 0 {
            // Data that takes a while: a database, another service.
            context.spawn(async move {
                tokio::time::sleep(Duration::from_millis(600)).await;
                vec!["first".to_owned(), "second".to_owned()]
            });
        }
        let list = self.notes.as_ref().map(|notes| {
            Node::column(
                "list",
                notes
                    .iter()
                    .enumerate()
                    .map(|(i, note)| Node::label(format!("n{i}"), note.clone())),
            )
        });
        Node::column(
            "page",
            [
                Node::label("title", "Notes"),
                pending(context, "notes", list, || Node::label("loading", "Loading…")),
            ],
        )
    }
}

fn head() -> Head {
    Head::new("Notes", "Your notes, on every device you use them on.")
}

async fn streamed() -> Page {
    Page::new::<Slow>(head(), ()).strategy(Strategy::Streamed)
}

async fn whole() -> Page {
    Page::new::<Slow>(head(), ())
}

fn app() -> ServerApp {
    ServerApp::new()
        .route("/stream", get(streamed).public())
        .route("/whole", get(whole).public())
        .version("build-2")
        .explain_pages()
}

/// Serves `app` on a runtime of its own; the address.
fn serve(app: ServerApp) -> std::net::SocketAddr {
    let runtime =
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let listener = runtime.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        runtime.block_on(app.into_service().serve(listener, std::future::pending()))
    });
    address
}

#[test]
fn a_streamed_pages_fallback_reaches_the_socket_before_its_data_exists() {
    let address = serve(app());
    let mut stream = TcpStream::connect(address).unwrap();
    let started = Instant::now();
    stream
        .write_all(b"GET /stream HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut received = Vec::new();
    let mut buffer = [0u8; 8192];
    let mut fallback_at = None;
    let mut data_at = None;
    loop {
        let read = stream.read(&mut buffer).unwrap();
        if read == 0 {
            break;
        }
        received.extend_from_slice(&buffer[..read]);
        let text = String::from_utf8_lossy(&received);
        if fallback_at.is_none() && text.contains("Loading…") {
            fallback_at = Some(started.elapsed());
        }
        if data_at.is_none() && text.contains(">second<") {
            data_at = Some(started.elapsed());
        }
    }
    let text = String::from_utf8_lossy(&received);
    assert!(text.starts_with("HTTP/1.1 200"), "{text}");
    assert!(text.contains("transfer-encoding: chunked"), "{text}");
    let fallback_at = fallback_at.expect("the fallback");
    let data_at = data_at.expect("the data");
    assert!(fallback_at < Duration::from_millis(500), "the shell came at once: {fallback_at:?}");
    assert!(data_at >= Duration::from_millis(550), "the data came when it existed: {data_at:?}");
    // The policy's nonce is the one the page's scripts carry.
    let nonce = text.split("'nonce-").nth(1).unwrap().split('\'').next().unwrap().to_owned();
    assert!(
        text.contains(&format!("<script nonce=\"{nonce}\">rnFill(\"notes\")</script>")),
        "{text}"
    );
}

#[tokio::test]
async fn a_whole_page_is_rendered_with_its_data_and_is_never_cached() {
    let service = app().into_service();
    let response =
        service.handle(http::Request::get("/whole").body(Bytes::new()).unwrap(), None).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["content-type"], "text/html; charset=utf-8");
    let html = String::from_utf8(response.body().to_vec()).unwrap();
    assert!(html.contains(">second</span>") && !html.contains("Loading"), "{html}");
    let policy = response.headers()["content-security-policy"].to_str().unwrap();
    assert!(!policy.contains("wasm-unsafe-eval"), "no WebAssembly subtree, no WebAssembly");
}

#[tokio::test]
async fn the_runtime_is_served_immutable() {
    let service = app().into_service();
    let url = rustnative_web::runtime::runtime_url("/_rn/");
    let response =
        service.handle(http::Request::get(url.as_str()).body(Bytes::new()).unwrap(), None).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["content-type"], "text/javascript; charset=utf-8");
    assert_eq!(response.headers()["cache-control"], "public, max-age=31536000, immutable");
    assert_eq!(response.body(), rustnative_web::runtime::RUNTIME_JS);
    let missing = service
        .handle(http::Request::get("/_rn/m/nothing.x.js").body(Bytes::new()).unwrap(), None)
        .await;
    assert_eq!(missing.status(), 404);
}

#[tokio::test]
async fn a_call_from_another_build_is_told_to_reload() {
    let service = app().into_service();
    let request = http::Request::post("/_fn/anything")
        .header("x-rn-fn-version", "build-1")
        .body(Bytes::new())
        .unwrap();
    let response = service.handle(request, None).await;
    assert_eq!(response.status(), 409);
    let body: serde_json::Value = serde_json::from_slice(response.body()).unwrap();
    assert_eq!(body, serde_json::json!({ "error": "version", "version": "build-2" }));
}

#[tokio::test]
async fn explain_says_which_parts_are_static() {
    let service = app().into_service();
    let response = service
        .handle(http::Request::get("/whole?_rn_explain").body(Bytes::new()).unwrap(), None)
        .await;
    let body: serde_json::Value = serde_json::from_slice(response.body()).unwrap();
    assert_eq!(body["boundaries"][0], serde_json::json!({ "boundary": "notes", "static": false }));
}

#[tokio::test]
async fn the_permissions_policy_opens_only_declared_capabilities() {
    let closed = app().into_service();
    let response =
        closed.handle(http::Request::get("/whole").body(Bytes::new()).unwrap(), None).await;
    let policy = response.headers()["permissions-policy"].to_str().unwrap().to_owned();
    for feature in [
        "camera=()",
        "geolocation=()",
        "bluetooth=()",
        "accelerometer=()",
        "clipboard-read=()",
        "usb=()",
    ] {
        assert!(policy.contains(feature), "{feature} in {policy}");
    }
    let open = app()
        .capabilities(&[
            rustnative_core::Capability::Location,
            rustnative_core::Capability::Sensors,
        ])
        .into_service();
    let response =
        open.handle(http::Request::get("/whole").body(Bytes::new()).unwrap(), None).await;
    let policy = response.headers()["permissions-policy"].to_str().unwrap().to_owned();
    for feature in
        ["geolocation=(self)", "accelerometer=(self)", "gyroscope=(self)", "camera=()", "usb=()"]
    {
        assert!(policy.contains(feature), "{feature} in {policy}");
    }
}

#[tokio::test]
async fn an_offline_application_is_configured_from_rustnative_toml() {
    let text = "[web.pwa]\nname = \"Notes\"\nshort_name = \"N\"\ntheme_color = \"#112233\"\n";
    let pwa = rustnative_server::web::pwa_config(text).unwrap().expect("the table");
    assert_eq!(
        (pwa.name.as_str(), pwa.theme_color.as_str(), pwa.start_url.as_str()),
        ("Notes", "#112233", "/")
    );
    assert_eq!(rustnative_server::web::pwa_config("port = 1").unwrap(), None);

    let service = app().pwa(pwa).into_service();
    let get = |path: &'static str| {
        let service = service.clone();
        async move { service.handle(http::Request::get(path).body(Bytes::new()).unwrap(), None).await }
    };
    let manifest = get("/manifest.webmanifest").await;
    assert_eq!(manifest.headers()["content-type"], "application/manifest+json");
    let worker = get("/_rn/sw.js").await;
    assert_eq!(worker.headers()["service-worker-allowed"], "/");
    assert!(std::str::from_utf8(worker.body()).unwrap().contains("const VERSION = \"build-2\";"));
    let page = String::from_utf8(get("/whole").await.body().to_vec()).unwrap();
    assert!(page.contains("<link rel=\"manifest\" href=\"/manifest.webmanifest\">"), "{page}");
    assert!(page.contains("\"sw\":\"/_rn/sw.js\""), "every page registers the worker: {page}");
    assert_eq!(get("/_rn/icon-192.png").await.headers()["content-type"], "image/png");
}
