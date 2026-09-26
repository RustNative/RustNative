//! `rustnative inspect` against a live application (`PLAN.md` Milestone
//! 44): the client is the shipped binary; the application is served from
//! this test's thread, answering between the client's requests as a
//! backend's loop would.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::process::{Command, Output};
use std::time::{Duration, Instant};

use rustnative_core::inspect::NoBackend;
use rustnative_core::{Application, Component, Event, Node, Size, Window, classes};
use serde_json::{Value, json};

struct Counter {
    count: u32,
}

impl Component for Counter {
    type Props = ();
    type Message = u32;
    fn new((): ()) -> Self {
        Self { count: 0 }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column(
            "root",
            [
                Node::label("count", format!("{}", self.count)).with_class(classes!("font-bold")),
                Node::button("increment", "More"),
            ],
        )
    }
    fn update(&mut self, _: Event) {}
    fn message(&mut self, count: u32) {
        self.count = count;
    }
    fn inspect(&self) -> Option<Value> {
        Some(json!({ "count": self.count }))
    }
    fn edit(&self, field: &str, value: &Value) -> Option<u32> {
        (field == "count").then(|| value.as_u64().and_then(|v| u32::try_from(v).ok())).flatten()
    }
}

/// Runs `rustnative inspect <args>` against `app`, answering its request.
fn inspect(app: &mut Application, args: &[&str]) -> Output {
    let endpoint = app.enable_inspection(None).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_rustnative"));
    command
        .args(["inspect", "--addr", &endpoint.addr.to_string(), "--token", &endpoint.token])
        .args(args);
    let child = std::thread::spawn(move || command.output().unwrap());
    let deadline = Instant::now() + Duration::from_secs(60);
    while !child.is_finished() {
        app.poll_inspection(&NoBackend);
        assert!(Instant::now() < deadline, "the client did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
    child.join().unwrap()
}

fn stdout(output: &Output) -> String {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn app() -> Application {
    Application::new(Counter::new(()), Window::new("Counter", Size::new(200, 100)))
}

#[test]
fn the_client_prints_the_tree_and_edits_state() {
    let mut app = app();
    let tree = stdout(&inspect(&mut app, &["tree"]));
    assert!(tree.contains("Column root"), "{tree}");
    assert!(tree.contains("  Label count \"0\""), "{tree}");
    assert!(tree.contains(".font-bold"), "{tree}");

    let path = app.components().root_path().to_owned();
    stdout(&inspect(&mut app, &["set", &path, "count", "9"]));
    let json = stdout(&inspect(&mut app, &["--json", "state", &path]));
    assert_eq!(serde_json::from_str::<Value>(&json).unwrap(), json!({ "count": 9 }));

    let style = stdout(&inspect(&mut app, &["style", "count"]));
    assert!(
        style.contains("font-weight = var(--font-weight-bold) (700)  <- class `font-bold`"),
        "{style}"
    );
    // The size the class did not set is the theme's, not an override.
    assert!(style.contains("font-size = 14  <- Label default"), "{style}");
    let explain = stdout(&inspect(&mut app, &["explain", "increment"]));
    assert!(explain.contains("width Fill"), "{explain}");
}

#[test]
fn the_client_reports_a_refusal_and_a_missing_application() {
    let mut app = app();
    let refused = inspect(&mut app, &["set", "nowhere", "count", "1"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("no component at `nowhere`"));

    let missing = Command::new(env!("CARGO_BIN_EXE_rustnative"))
        .args(["inspect", "--pid", "1", "hello"])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("RUSTNATIVE_INSPECT=1"));
}

#[test]
fn a_recording_is_written_and_turned_into_a_test() {
    let mut app = app();
    stdout(&inspect(&mut app, &["record"]));
    let directory =
        std::env::temp_dir().join(format!("rustnative-inspect-test-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let recording = directory.join("session.json");
    stdout(&inspect(&mut app, &["stop", "--out", recording.to_str().unwrap()]));
    let test = stdout(
        &Command::new(env!("CARGO_BIN_EXE_rustnative"))
            .args(["inspect", "to-test", recording.to_str().unwrap()])
            .args(["--launch", "my_app::Counter::new(())", "--name", "counter_session"])
            .output()
            .unwrap(),
    );
    assert!(test.contains("fn counter_session()"), "{test}");
    assert!(test.contains("|| my_app::Counter::new(())"), "{test}");
    let _ = std::fs::remove_dir_all(directory);
}
