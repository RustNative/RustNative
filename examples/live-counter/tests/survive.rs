//! The client's screen survives a reconnect and a deploy without losing
//! state (`PLAN.md` Milestone 55's second "done when"), driven through the
//! client's own UI on the headless backend.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use live_counter::CounterApp;
use rustnative_core::{Component, Services, Size, Theme, Window};
use rustnative_headless::{HeadlessApp, Query};
use rustnative_sync::live::{LiveClient, LiveServer, RemoteView};

fn text(app: &HeadlessApp, key: &str) -> String {
    app.find(&Query::key(key)).map(|node| node.text.clone().unwrap_or_default()).unwrap_or_default()
}

fn settle_until(app: &mut HeadlessApp, what: &str, done: impl Fn(&HeadlessApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(app) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}:\n{}", app.golden());
        app.settle();
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_count_survives_a_reconnect_and_a_deploy() {
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let _inside = runtime.enter();
    let start = || {
        let server = LiveServer::new(CounterApp, Duration::from_secs(30));
        let listener = runtime.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        runtime.spawn(Arc::clone(&server).serve(listener));
        (server, url)
    };
    let (old, old_url) = start();
    let client = LiveClient::connect(&old_url);
    let mut app = HeadlessApp::launch_with(
        Window::new("Live", Size::new(360, 240)),
        Services::default(),
        Theme::default(),
        move || RemoteView::new(client.clone()),
    );
    settle_until(&mut app, "the server's screen", |app| text(app, "count") == "Count 0");
    app.click(&Query::key("increment")).unwrap();
    app.click(&Query::key("increment")).unwrap();
    settle_until(&mut app, "two clicks", |app| text(app, "count") == "Count 2");

    old.drop_connections();
    settle_until(&mut app, "the reconnect", |app| text(app, "count") == "Count 2");
    app.click(&Query::key("increment")).unwrap();
    settle_until(&mut app, "a click after reconnecting", |app| text(app, "count") == "Count 3");

    let (new, new_url) = start();
    old.drain(&new_url, Duration::from_millis(20));
    settle_until(&mut app, "the deploy", |_| new.sessions() == 1);
    settle_until(&mut app, "the state on the new instance", |app| text(app, "count") == "Count 3");
    app.click(&Query::key("decrement")).unwrap();
    settle_until(&mut app, "a click on the new instance", |app| text(app, "count") == "Count 2");
}
