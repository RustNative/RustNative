//! The inspection protocol on the headless backend (`PLAN.md` Milestone
//! 44): realized objects and rectangles, capabilities, lifetimes, the
//! overlay, and a recorded session — input and HTTP — replayed
//! deterministically and turned into a regression test.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

mod support;

use std::sync::Arc;
use std::time::Duration;

use rustnative_core::inspect::{
    CapabilityReport, HttpTape, LayoutExplanation, Lifetimes, OverlayMode, RealizedObject,
    Recording, RecordingHttp, Reply, Request,
};
use rustnative_core::{Component, HttpResponse, Method, Services, Size, Theme, Window};
use rustnative_headless::{HeadlessApp, MockHttp, Query};
use serde_json::{Value, json};
use support::Greeter;

fn ask<T: serde::de::DeserializeOwned>(app: &mut HeadlessApp, request: &Request) -> T {
    match app.inspect(request) {
        Reply::Ok(value) => serde_json::from_value(value).unwrap(),
        Reply::Error(error) => panic!("the inspector refused: {error}"),
    }
}

fn window() -> Window {
    Window::new("Greeter", Size::new(320, 240))
}

#[test]
fn the_headless_backend_answers_what_only_a_backend_knows() {
    let mut app = HeadlessApp::launch(window(), || Greeter::new(()));
    let hello: Value = ask(&mut app, &Request::Hello);
    assert_eq!(hello["backend"], json!("headless"));

    let realized: Vec<RealizedObject> = ask(&mut app, &Request::Realized { window: None });
    let load = realized.iter().find(|object| object.key.as_deref() == Some("load")).unwrap();
    assert_eq!(load.host_type, "headless Button");
    let rect = load.rect.unwrap();

    let explained: LayoutExplanation =
        ask(&mut app, &Request::ExplainLayout { node: "load".into(), window: None });
    assert!(explained.realized, "the backend's own rectangle");
    assert_eq!(explained.rect, rect);

    let capabilities: CapabilityReport = ask(&mut app, &Request::Capabilities);
    assert_eq!(capabilities.backend, "headless");
    assert!(capabilities.style.iter().all(|row| row.why == "realized"));
    assert!(capabilities.refused.iter().any(|refusal| refusal.what == "service `http`"));
    assert!(capabilities.units.is_some());

    let lifetimes: Lifetimes = ask(&mut app, &Request::Lifetimes);
    assert_eq!(lifetimes.live, 5, "the column, two fields, the button, the label");

    let _: Value = ask(&mut app, &Request::Overlay { mode: Some(OverlayMode::Layout) });
    let overlay = app.overlay().unwrap();
    assert_eq!(overlay.commands().len(), 10, "an outline and a label per node");
}

/// Records a session against a scripted server, through a recording HTTP
/// service.
fn record() -> Recording {
    let server = MockHttp::new();
    server.expect(
        Method::Get,
        "https://api.test/greet?name=Ada",
        HttpResponse::new(200, vec![("Set-Cookie".into(), "id=1".into())], b"Hello, Ada".to_vec()),
    );
    let tape = HttpTape::new();
    let services =
        Services::default().with_http(Arc::new(RecordingHttp::new(Arc::new(server), tape.clone())));
    let mut app =
        HeadlessApp::launch_with(window(), services, Theme::default(), || Greeter::new(()));
    app.application_mut().record_http(tape);
    let _: Value = ask(&mut app, &Request::StartRecording { redact: vec!["secret".into()] });
    app.set_text(&Query::key("name"), "Ada").unwrap();
    app.advance(Duration::from_millis(250));
    app.set_text(&Query::key("secret"), "hunter2").unwrap();
    app.click(&Query::key("load")).unwrap();
    app.advance(Duration::from_millis(40));
    assert!(app.find(&Query::text("Hello, Ada")).is_ok());
    ask(&mut app, &Request::StopRecording)
}

#[test]
fn a_recorded_session_replays_deterministically_without_the_server() {
    let recording = record();
    let text = serde_json::to_string(&recording).unwrap();
    assert!(!text.contains("hunter2") && !text.contains("id=1"), "secrets are not recorded");
    assert_eq!(recording.http.len(), 1);
    assert_eq!(recording.inputs[0].at_ms, 0);
    assert_eq!(recording.inputs.last().unwrap().at_ms, 250, "virtual time is recorded");
    assert_eq!(recording.final_state.values().next().unwrap()["greeting"], json!("Hello, Ada"));

    // The redacted input is dropped; the rest replays against the recorded
    // response, with no server.
    let mut replayable = recording.clone();
    replayable.inputs.retain(|input| !serde_json::to_string(input).unwrap().contains("[redacted]"));
    let mut app = HeadlessApp::launch(window(), || Greeter::new(()));
    app.replay(&replayable).unwrap();
    assert_eq!(app.inspected_state(), recording.final_state);
    assert!(app.find(&Query::text("Hello, Ada")).is_ok());
}

/// `tests/replayed_session.rs` is this recording's generated test, checked
/// in and run like any other; regenerate with `RUSTNATIVE_BLESS=1`.
#[test]
fn a_recording_becomes_the_checked_in_regression_test() {
    let mut recording = record();
    recording.inputs.retain(|input| !serde_json::to_string(input).unwrap().contains("[redacted]"));
    let generated = format!(
        "mod support;\n\n{}",
        recording.to_test("replays_the_recorded_greeting", "support::Greeter::new(())")
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/replayed_session.rs");
    if std::env::var("RUSTNATIVE_BLESS").is_ok_and(|value| value == "1") {
        std::fs::write(&path, &generated).unwrap();
    }
    // Compared without whitespace or trailing commas, so formatting the
    // checked-in file is not a difference.
    let bare = |text: &str| {
        let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        text.replace(",)", ")")
    };
    let checked_in = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(bare(&checked_in), bare(&generated), "run with RUSTNATIVE_BLESS=1 to regenerate");
}
