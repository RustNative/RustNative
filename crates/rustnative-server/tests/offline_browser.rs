//! Offline applications in a real browser (`PLAN.md` Web milestone I): the
//! service worker installs and controls the page; with the network gone, a
//! visited page and its island still load and work; a server call made
//! offline is delivered once the network is back, and only once; a new
//! build is announced and applied with the island's state kept; and the
//! manifest is valid.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::needless_pass_by_value,
    clippy::must_use_candidate,
    clippy::semicolon_if_nothing_returned,
    clippy::unused_async,
    clippy::assigning_clones,
    missing_docs,
    reason = "tests, with client logic written as an application would"
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use rustnative_core::{Component, ComponentContext, Event, Node};
use rustnative_server::functions::server_fn;
use rustnative_server::{AppService, ServerApp, ServerError, get};
use rustnative_web::pwa::Pwa;
use rustnative_web::{Client, Head, Page};
use rustnative_web_testing::browser_or_skip;

#[rustnative_web::client]
pub mod sender {
    use rustnative_core::server_fn::{ServerFn, ServerFnError};
    use rustnative_core::{Event, Node, NodeId};
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    pub struct Record;
    impl ServerFn for Record {
        const PATH: &'static str = "record";
        type Input = i32;
        type Output = i32;
    }

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Sender {
        pub count: i32,
        pub sent: String,
    }

    pub enum Msg {
        Sent(Result<i32, ServerFnError>),
    }

    impl Sender {
        pub fn update(&mut self, event: Event, fx: &mut Effects<Msg>) {
            match event {
                Event::Click { target } if target == NodeId::from_key("add") => self.count += 1,
                Event::Click { target } if target == NodeId::from_key("send") => {
                    fx.call::<Record>(self.count, Msg::Sent)
                }
                _ => {}
            }
        }

        pub fn message(&mut self, message: Msg, fx: &mut Effects<Msg>) {
            let _ = fx;
            match message {
                Msg::Sent(Ok(count)) => self.sent = format!("ok {count}"),
                Msg::Sent(Err(_)) => self.sent = "failed".to_owned(),
            }
        }

        pub fn view(&self) -> Node {
            Node::column(
                "sender",
                [
                    Node::label("count", format!("{}", self.count)),
                    Node::button("add", "Add"),
                    Node::button("send", "Send"),
                    Node::label("sent", self.sent.clone()),
                ],
            )
        }
    }
}

struct Home;

impl Component for Home {
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
        Node::column("home", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let island = context.child_with_props::<Client<sender::Sender>, _>(
            "sender",
            sender::Sender::default(),
            Client::new,
        );
        Node::column("home", [Node::label("title", "Offline"), island])
    }
}

async fn home() -> Page {
    Page::new::<Home>(
        Head::new("Offline", "A page served to a real browser by the offline tests."),
        (),
    )
    .lang("en")
}

fn serve(delivered: Arc<AtomicUsize>) -> (String, AppService) {
    let runtime =
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let service = ServerApp::new()
        .route("/", get(home).public())
        .function::<sender::Record>(
            server_fn::<sender::Record, _, _>(move |count: i32| {
                let delivered = Arc::clone(&delivered);
                async move {
                    delivered.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, ServerError>(count)
                }
            })
            .public(),
        )
        .pwa(Pwa::new("Offline test").short_name("Offline"))
        .into_service();
    let listener = runtime.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
    let port = listener.local_addr().unwrap().port();
    let served = service.clone();
    std::thread::spawn(move || runtime.block_on(served.serve(listener, std::future::pending())));
    (format!("http://localhost:{port}"), service)
}

const WAIT: Duration = Duration::from_secs(20);

fn rt() -> String {
    format!("import('{}')", rustnative_web::runtime::runtime_url("/_rn/"))
}

fn wait_for(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn an_offline_application_in_a_browser() {
    let Some(browser) = browser_or_skip("an_offline_application_in_a_browser") else { return };
    let delivered = Arc::new(AtomicUsize::new(0));
    let (origin, service) = serve(Arc::clone(&delivered));
    let rt = rt();

    // The worker installs and takes the page.
    let page = browser.page().unwrap();
    page.goto(&format!("{origin}/")).unwrap();
    page.wait_until("document.documentElement.hasAttribute('data-rn-ready')", WAIT).unwrap();
    page.wait_until("navigator.serviceWorker.controller !== null", WAIT).unwrap();
    // A visit it controls: the page is kept for offline use.
    page.goto(&format!("{origin}/")).unwrap();
    page.wait_until("document.documentElement.hasAttribute('data-rn-ready')", WAIT).unwrap();

    // Offline: the page and its island still load and work.
    page.set_offline_everywhere(true).unwrap();
    page.goto(&format!("{origin}/")).unwrap();
    page.wait_until("document.documentElement.hasAttribute('data-rn-ready')", WAIT).unwrap();
    page.click("#i0-add").unwrap();
    page.wait_until("document.getElementById('i0-count').textContent === '1'", WAIT).unwrap();

    // A server call made offline waits in the queue.
    page.click("#i0-send").unwrap();
    page.wait_until("document.getElementById('i0-sent').textContent === 'failed'", WAIT).unwrap();
    assert_eq!(delivered.load(Ordering::SeqCst), 0);

    // Online again: delivered, once.
    page.set_offline_everywhere(false).unwrap();
    wait_for("the queued call", || delivered.load(Ordering::SeqCst) == 1);
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(delivered.load(Ordering::SeqCst), 1, "delivered once");

    // A new build: announced, and applied with the island's state kept.
    service.set_version("build-2");
    page.eval(&format!("(async () => {{ await (await {rt}).checkUpdate(); }})()")).unwrap();
    page.wait_until("document.documentElement.hasAttribute('data-rn-update')", WAIT).unwrap();
    page.eval("window.beforeUpdate = true").unwrap();
    assert_eq!(page.eval(&format!("(async () => (await {rt}).applyUpdate())()")).unwrap(), true);
    page.wait_until("window.beforeUpdate === undefined && document.documentElement.hasAttribute('data-rn-ready')", WAIT).unwrap();
    page.wait_until("document.getElementById('i0-count')?.textContent === '1'", WAIT).unwrap();

    // The manifest.
    let manifest = page.call("Page.getAppManifest", serde_json::json!({})).unwrap();
    assert!(manifest["url"].as_str().unwrap().ends_with("/manifest.webmanifest"), "{manifest}");
    assert_eq!(manifest["errors"], serde_json::json!([]), "{manifest}");
    assert!(manifest["data"].as_str().unwrap().contains("Offline test"), "{manifest}");
    let console = page.console();
    assert!(
        console
            .iter()
            .all(|line| !line.contains("rn:mismatch") && !line.contains("Content Security Policy")),
        "{console:?}"
    );
}
