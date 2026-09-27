//! The serverless shapes (`PLAN.md` Web milestone K): the application
//! answers a function runtime's invocations, whole and streamed pages
//! included, on the invocation's one thread; an invocation past its
//! deadline is refused; the host's limits reach the request; work an
//! invocation spawned does not outlive it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs, reason = "tests")]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rustnative_core::{Component, ComponentContext, Event, Node};
use rustnative_server::serverless::{HostLimits, lambda};
use rustnative_server::{ServerApp, get};
use rustnative_web::{Head, Page, Strategy, pending};
use serde_json::{Value, json};

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
            context.spawn(async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
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
        let limits = rustnative_web::request(context).map(|request| request.limits().clone());
        let deadline = limits.and_then(|limits| limits.deadline).map_or(0, |d| d.as_secs());
        Node::column(
            "page",
            [
                Node::label("title", "Notes"),
                Node::label("deadline", format!("{deadline}")),
                pending(context, "notes", list, || Node::label("loading", "Loading…")),
            ],
        )
    }
}

fn app(spawned: Arc<AtomicBool>) -> ServerApp {
    let head = || Head::new("Notes", "Your notes, on every device you use them on.");
    ServerApp::new()
        .route("/whole", get(move || async move { Page::new::<Slow>(head(), ()) }).public())
        .route(
            "/stream",
            get(move || async move { Page::new::<Slow>(head(), ()).strategy(Strategy::Streamed) })
                .public(),
        )
        .route(
            "/spawn",
            get(move || {
                let spawned = Arc::clone(&spawned);
                async move {
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        spawned.store(true, Ordering::SeqCst);
                    });
                    "started"
                }
            })
            .public(),
        )
}

fn now_ms() -> u64 {
    u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()).unwrap()
}

fn read_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).unwrap();
        head.push(byte[0]);
    }
    let head = String::from_utf8(head).unwrap();
    let length = head
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|v| v.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    let mut body = vec![0; length];
    stream.read_exact(&mut body).unwrap();
    (head.lines().next().unwrap().to_owned(), body)
}

/// A runtime API that hands out one invocation and returns what the
/// function posted back: the path it posted to, and the body.
fn invoke(
    event: &Value,
    deadline_ms: u64,
    service: &rustnative_server::AppService,
) -> (String, Value) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let api = listener.local_addr().unwrap().to_string();
    let event = event.to_string();
    let host = std::thread::spawn(move || {
        let (mut next, _) = listener.accept().unwrap();
        let (line, _) = read_request(&mut next);
        assert!(line.starts_with("GET /2018-06-01/runtime/invocation/next"));
        write!(
            next,
            "HTTP/1.1 200 OK\r\nlambda-runtime-aws-request-id: abc\r\nlambda-runtime-deadline-ms: {deadline_ms}\r\ncontent-length: {}\r\n\r\n{event}",
            event.len()
        )
        .unwrap();
        drop(next);
        let (mut posted, _) = listener.accept().unwrap();
        let (line, body) = read_request(&mut posted);
        posted.write_all(b"HTTP/1.1 202 Accepted\r\ncontent-length: 0\r\n\r\n").unwrap();
        (line, serde_json::from_slice(&body).unwrap())
    });
    lambda::next(&api, service, None).unwrap();
    host.join().unwrap()
}

fn http_event(path: &str) -> Value {
    json!({
        "version": "2.0",
        "rawPath": path,
        "rawQueryString": "",
        "headers": { "accept": "text/html" },
        "requestContext": { "http": { "method": "GET", "sourceIp": "10.0.0.1" } },
        "isBase64Encoded": false,
    })
}

#[test]
fn pages_are_answered_whole_on_the_invocations_thread() {
    let service = app(Arc::default()).into_service();
    for path in ["/whole", "/stream"] {
        let (line, reply) = invoke(&http_event(path), now_ms() + 30_000, &service);
        assert!(line.contains("/invocation/abc/response"), "{line}");
        assert_eq!(reply["statusCode"], 200, "{path}");
        let body = reply["body"].as_str().unwrap();
        assert!(body.contains("first") && body.contains("second"), "{path}: {body}");
        // The deadline the host gave reached the page as a limit.
        assert!(body.contains(">29<") || body.contains(">30<"), "{path}: {body}");
    }
}

#[test]
fn an_invocation_past_its_deadline_is_refused() {
    let service = app(Arc::default()).into_service();
    let (line, error) = invoke(&http_event("/whole"), now_ms().saturating_sub(1), &service);
    assert!(line.contains("/invocation/abc/error"), "{line}");
    assert_eq!(error["errorType"], "DeadlineExceeded");
}

#[test]
fn work_an_invocation_spawned_does_not_outlive_it() {
    let spawned = Arc::new(AtomicBool::new(false));
    let service = app(Arc::clone(&spawned)).into_service();
    let (_, reply) = invoke(&http_event("/spawn"), now_ms() + 30_000, &service);
    assert_eq!(reply["body"], "started");
    std::thread::sleep(Duration::from_millis(300));
    assert!(!spawned.load(Ordering::SeqCst), "the task outlived its invocation");
}

#[test]
fn limits_default_to_none_off_a_host() {
    assert_eq!(HostLimits::default().deadline, None);
}
