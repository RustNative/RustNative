//! The task board on the headless backend (`PLAN.md` Milestone 47's "done
//! when"): cached, deduplicated, paginated data; optimistic updates;
//! offline queueing that survives a restart; and a failing widget
//! contained and retried without restarting anything else.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::sync::Arc;
use std::time::Duration;

use data_demo::{Board, DemoServer};
use rustnative_core::{Component, Services, Size, Theme, Window};
use rustnative_headless::{HeadlessApp, Query};

fn launch(server: &DemoServer) -> HeadlessApp {
    launch_with(server, Services::default())
}

fn launch_with(server: &DemoServer, services: Services) -> HeadlessApp {
    let services = services.with_http(Arc::new(server.clone()));
    let root = server.clone();
    HeadlessApp::launch_with(
        Window::new("Tasks", Size::new(560, 520)),
        services,
        Theme::default(),
        move || Board::new(root.clone()),
    )
}

fn text(app: &HeadlessApp, key: &str) -> String {
    app.find(&Query::key(key)).map(|node| node.text.clone().unwrap_or_default()).unwrap_or_default()
}

fn tasks(app: &HeadlessApp) -> Vec<String> {
    (0..)
        .map_while(|index| app.find(&Query::key(format!("task-{index}"))).ok())
        .map(|node| node.text.clone().unwrap_or_default())
        .collect()
}

fn quiet() {
    std::panic::set_hook(Box::new(|info| {
        let message = rustnative_core::scheduler::panic_message(info.payload());
        if !message.contains("garbage") && !message.contains("on purpose") {
            eprintln!("{info}");
        }
    }));
}

#[test]
fn the_list_is_fetched_once_for_two_observers_and_paginated() {
    quiet();
    let server = DemoServer::new(12, Duration::ZERO);
    let mut app = launch(&server);
    assert_eq!(text(&app, "summary"), "5 tasks");
    assert_eq!(tasks(&app).len(), 5);
    assert_eq!(
        server.requests(),
        ["GET tasks?page=0"],
        "the summary and the list share one request"
    );

    app.click(&Query::key("load-more")).unwrap();
    app.click(&Query::key("load-more")).unwrap();
    assert_eq!(text(&app, "summary"), "12 tasks");
    assert_eq!(tasks(&app).last().unwrap(), "Task 12");
    app.click(&Query::key("load-more")).unwrap();
    assert_eq!(server.requests().len(), 3, "no request past the last page");
}

#[test]
fn an_added_task_shows_at_once_and_is_confirmed_by_the_server() {
    quiet();
    let server = DemoServer::new(3, Duration::ZERO);
    let mut app = launch(&server);
    app.click(&Query::key("add")).unwrap();
    // The server has answered (virtual time), and the list was refetched.
    assert_eq!(tasks(&app).last().unwrap(), "New task 1", "no longer \"saving\"");
    assert!(server.titles().contains(&"New task 1".to_owned()));
    assert!(server.requests().contains(&"POST tasks".to_owned()));
    assert!(text(&app, "status").contains("0 sending"));
}

#[test]
fn offline_additions_queue_durably_and_are_sent_on_reconnect() {
    quiet();
    let server = DemoServer::new(3, Duration::ZERO);
    let mut app = launch(&server);
    app.click(&Query::key("network")).unwrap();
    assert!(text(&app, "status").starts_with("Offline"));
    app.click(&Query::key("add")).unwrap();
    app.click(&Query::key("add")).unwrap();
    assert!(text(&app, "status").contains("2 queued"), "{}", text(&app, "status"));
    assert_eq!(
        tasks(&app)[3..],
        ["New task 1 (saving)".to_owned(), "New task 2 (saving)".to_owned()]
    );

    // The application is closed and started again while offline: the queue
    // is in the state store.
    let mut app = app.kill_and_restore();
    // Started, it tried to send the queue, found the server unreachable,
    // and went offline again.
    let status = text(&app, "status");
    assert!(status.starts_with("Offline") && status.contains("2 queued"), "{status}");

    app.click(&Query::key("network")).unwrap();
    assert!(text(&app, "status").contains("0 queued"), "{}", text(&app, "status"));
    let titles = server.titles();
    assert_eq!(titles[3..], ["New task 1".to_owned(), "New task 2".to_owned()], "sent in order");
}

#[test]
fn a_failing_widget_is_contained_and_restarted_without_disturbing_the_board() {
    quiet();
    let server = DemoServer::new(3, Duration::ZERO);
    let mut app = launch(&server);
    // It failed when first rendered; the board around it is intact.
    assert!(text(&app, "weather-error").contains("garbage"));
    assert_eq!(tasks(&app).len(), 3);

    // Restarted after 200 ms: fails again; after 400 ms more, it works.
    app.advance(Duration::from_millis(200));
    assert!(text(&app, "weather-error").contains("garbage"));
    app.advance(Duration::from_millis(400));
    assert_eq!(text(&app, "forecast"), "Sunny, 21°C");

    // Broken on purpose: contained, and retried by hand.
    app.click(&Query::key("break")).unwrap();
    assert!(text(&app, "weather-error").contains("on purpose"));
    assert_eq!(tasks(&app).len(), 3, "the rest of the board is untouched");
    app.click(&Query::key("retry")).unwrap();
    assert_eq!(text(&app, "forecast"), "Sunny, 21°C");
    let failures = app.application_mut().take_failures(rustnative_core::WindowId::PRIMARY);
    assert!(failures.is_empty() || failures.iter().all(|f| f.component.ends_with("/weather")));
}

#[test]
fn the_task_list_is_unaffected_by_services_it_does_not_use() {
    quiet();
    let server = DemoServer::new(1, Duration::ZERO);
    let app = launch_with(&server, Services::default());
    assert_eq!(tasks(&app), ["Task 1"]);
}
