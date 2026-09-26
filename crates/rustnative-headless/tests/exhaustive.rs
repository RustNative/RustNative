//! Exhaustive mode (`C11`): work a test did not account for fails it.

use std::time::Duration;

use rustnative_core::{
    Component, ComponentContext, Event, HttpRequest, HttpResponse, Method, Node, Size, Window,
};
use rustnative_headless::{HeadlessApp, MockHttp, Query};

/// Fetches once on mount, and optionally starts a long timer.
struct Loader {
    started: bool,
    body: String,
    slow: bool,
}

impl Component for Loader {
    type Props = bool;
    type Message = String;

    fn new(slow: bool) -> Self {
        Self { started: false, body: "loading".into(), slow }
    }
    fn props(&self) -> &bool {
        &self.slow
    }
    fn set_props(&mut self, slow: bool) {
        self.slow = slow;
    }
    fn view(&self) -> Node {
        Node::label("body", self.body.clone())
    }
    fn update(&mut self, _event: Event) {}
    fn message(&mut self, body: String) {
        self.body = body;
    }
    fn render(&mut self, context: &mut ComponentContext<'_, String>) -> Node {
        if !self.started {
            self.started = true;
            if let Some(http) = context.services().http().cloned() {
                context.spawn(async move {
                    match http.execute(HttpRequest::get("https://api.test/greeting")).await {
                        Ok(response) => String::from_utf8_lossy(response.body_bytes()).into_owned(),
                        Err(error) => format!("error: {error}"),
                    }
                });
            }
            if self.slow {
                let delay = context.sleep(Duration::from_secs(3600));
                context.spawn(async move {
                    delay.await;
                    "late".to_owned()
                });
            }
        }
        self.view()
    }
}

fn launch(slow: bool, http: MockHttp) -> HeadlessApp {
    HeadlessApp::launch(Window::new("Loader", Size::new(200, 100)), move || Loader::new(slow))
        .with_http(http)
        .exhaustive()
}

#[test]
fn a_test_that_accounts_for_everything_passes() {
    let http = MockHttp::new();
    http.expect(
        Method::Get,
        "https://api.test/greeting",
        HttpResponse::new(200, vec![], b"hi".to_vec()),
    );
    let app = launch(false, http);
    assert!(app.find(&Query::text("hi")).is_ok());
}

#[test]
#[should_panic(expected = "task(s) still pending")]
fn a_pending_task_fails_the_test() {
    let http = MockHttp::new();
    http.expect(
        Method::Get,
        "https://api.test/greeting",
        HttpResponse::new(200, vec![], b"hi".to_vec()),
    );
    let _app = launch(true, http);
}

#[test]
#[should_panic(expected = "was never requested")]
fn an_unmet_expectation_fails_the_test() {
    let http = MockHttp::new();
    http.expect(
        Method::Get,
        "https://api.test/greeting",
        HttpResponse::new(200, vec![], b"hi".to_vec()),
    );
    http.expect(Method::Post, "https://api.test/other", HttpResponse::new(204, vec![], Vec::new()));
    let _app = launch(false, http);
}

#[test]
#[should_panic(expected = "unexpected GET https://api.test/greeting")]
fn an_unexpected_request_fails_the_test() {
    let _app = launch(false, MockHttp::new());
}
