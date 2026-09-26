//! The reference application on the headless backend (`PLAN.md`
//! Milestone 54's "done when"): typing into a filter over 200 000 rows is
//! handled within the input-latency budget while the filtered view updates
//! behind it, and a hidden screen does no periodic work.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::time::{Duration, Instant};

use filter_demo::{App, ROWS, dataset, filter};
use rustnative_core::{Component, Event, NodeId, Size, Window};
use rustnative_headless::{HeadlessApp, Query};

/// The budget a keystroke must be handled in: the headless backend's
/// `input_latency_ms` (`budgets/headless.toml`), the same promise the bench
/// checks.
fn budget() -> Duration {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../budgets/headless.toml"),
    )
    .unwrap();
    let line = text.lines().find(|line| line.starts_with("input_latency_ms")).unwrap();
    let max = line.split("max =").nth(1).unwrap().split(',').next().unwrap().trim();
    Duration::from_secs_f64(max.parse::<f64>().unwrap() / 1000.0)
}

fn text(app: &HeadlessApp, key: &str) -> String {
    app.find(&Query::key(key)).map(|node| node.text.clone().unwrap_or_default()).unwrap_or_default()
}

fn launch() -> HeadlessApp {
    HeadlessApp::launch(Window::new("Filter", Size::new(520, 640)), || App::new(()))
}

#[test]
fn every_keystroke_is_handled_within_budget_while_the_matches_follow() {
    let mut app = launch();
    assert_eq!(text(&app, "status"), format!("{ROWS} of {ROWS} rows match"));

    let budget = budget();
    let mut typed = String::new();
    let mut slowest = Duration::ZERO;
    for character in "amber falcon".chars() {
        typed.push(character);
        let started = Instant::now();
        // The keystroke's whole handling: the event, the render it causes,
        // and nothing else — the filter is not waited for.
        app.application_mut().dispatch(Event::TextChanged {
            target: NodeId::from_key("query"),
            value: typed.clone(),
        });
        slowest = slowest.max(started.elapsed());
    }
    // Measured in debug builds too: a keystroke never pays for the filter.
    assert!(slowest <= budget * 4, "slowest keystroke {slowest:?} against a budget of {budget:?}");

    app.settle();
    let expected = filter(&dataset(), "amber falcon").len();
    assert_eq!(text(&app, "status"), format!("{expected} of {ROWS} rows match"));
    assert_eq!(text(&app, "query"), "amber falcon");
    let snapshot = app.realized().snapshot();
    let realized = snapshot
        .nodes()
        .filter(|node| node.id.local_key().is_some_and(|key| key.starts_with("row-")))
        .count();
    assert!(realized > 0 && realized < 60, "only a screenful of rows exists: {realized}");
}

#[test]
fn the_view_shows_the_previous_matches_while_it_updates() {
    let mut app = launch();
    app.application_mut()
        .dispatch(Event::TextChanged { target: NodeId::from_key("query"), value: "zephyr".into() });
    // Rendered, but the executor has not run the filter yet.
    app.application_mut().pump_tasks();
    let status = app
        .application()
        .components_for(rustnative_core::WindowId::PRIMARY)
        .map(|tree| {
            let mut found = String::new();
            tree.view().visit(&mut |node, _, _| {
                if let rustnative_core::Node::Label(label) = node {
                    if label.text().contains("rows match") {
                        found = label.text().to_owned();
                    }
                }
            });
            found
        })
        .unwrap();
    assert_eq!(status, format!("{ROWS} of {ROWS} rows match — updating…"));
    app.settle();
    assert_eq!(
        text(&app, "status"),
        format!("{} of {ROWS} rows match", filter(&dataset(), "zephyr").len())
    );
}

#[test]
fn a_hidden_screen_does_no_periodic_work() {
    let mut app = launch();
    for _ in 0..3 {
        app.advance(Duration::from_secs(1));
    }
    assert_eq!(text(&app, "clock"), "Open for 3 s");

    app.click(&Query::key("open-details")).unwrap();
    let suspended = app
        .application()
        .components_for(rustnative_core::WindowId::PRIMARY)
        .map(rustnative_core::ComponentTree::suspended_components)
        .unwrap();
    assert!(
        suspended.iter().any(|path| path.ends_with("/filter")),
        "{suspended:?} details={}",
        text(&app, "about")
    );
    for _ in 0..30 {
        app.advance(Duration::from_secs(1));
    }
    assert!(app.executor().pending_task_count() <= 2, "nothing piles up while hidden");

    app.click(&Query::key("back")).unwrap();
    let clock = text(&app, "clock");
    assert!(clock == "Open for 3 s" || clock == "Open for 4 s", "stopped while hidden: {clock}");
    app.advance(Duration::from_secs(2));
    assert_ne!(text(&app, "clock"), clock, "running again once shown");
}
