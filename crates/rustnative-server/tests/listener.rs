//! The application's own listener under hostile peers: a connection ceiling,
//! header and body timeouts, and a shutdown that lets the requests in
//! flight finish, each followed by a request the listener still answers.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use rustnative_server::{Security, ServerApp, get};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

fn app(security: Security) -> ServerApp {
    ServerApp::new()
        .security(Security { rate_limit: None, ..security })
        .route("/ping", get(|| async { "pong" }).public())
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(400)).await;
                "done"
            })
            .public(),
        )
        .route(
            "/stuck",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(30)).await;
                "never"
            })
            .public(),
        )
}

async fn listen(security: Security) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(app(security).into_service().serve(listener, std::future::pending()));
    address
}

/// The whole response to a `GET path` on a fresh connection.
async fn fetch(address: SocketAddr, path: &str) -> String {
    let mut stream = TcpStream::connect(address).await.unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

/// Whether the server closes `stream` within `within`.
async fn closed_within(stream: &mut TcpStream, within: Duration) -> bool {
    let mut buffer = [0u8; 1024];
    let deadline = Instant::now() + within;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, stream.read(&mut buffer)).await {
            Ok(Ok(0) | Err(_)) => return true,
            Ok(Ok(_)) => {}
            Err(_) => return false,
        }
    }
}

#[tokio::test]
async fn connections_past_the_ceiling_are_closed_and_the_listener_keeps_serving() {
    let address = listen(Security { max_connections: 2, ..Security::default() }).await;
    let held =
        [TcpStream::connect(address).await.unwrap(), TcpStream::connect(address).await.unwrap()];
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut third = TcpStream::connect(address).await.unwrap();
    assert!(
        closed_within(&mut third, Duration::from_secs(2)).await,
        "a connection past the ceiling is closed as it arrives"
    );

    drop(held);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream
            .write_all(b"GET /ping HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response).await;
        if response.contains("\r\npong\r\n") {
            break;
        }
        assert!(Instant::now() < deadline, "the freed slots serve again");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn a_client_that_never_finishes_its_headers_is_cut_off() {
    let address =
        listen(Security { header_timeout: Duration::from_millis(200), ..Security::default() })
            .await;
    let mut slow = TcpStream::connect(address).await.unwrap();
    slow.write_all(b"GET /ping HTTP/1.1\r\nHost: te").await.unwrap();
    assert!(closed_within(&mut slow, Duration::from_secs(3)).await, "the header timeout applies");
    assert!(fetch(address, "/ping").await.contains("\r\npong\r\n"));
}

#[tokio::test]
async fn a_body_slower_than_its_deadline_is_refused() {
    let address =
        listen(Security { body_timeout: Duration::from_millis(200), ..Security::default() }).await;
    let mut slow = TcpStream::connect(address).await.unwrap();
    slow.write_all(
        b"POST /ping HTTP/1.1\r\nHost: test\r\nContent-Length: 100\r\nConnection: close\r\n\r\nonly ten b",
    )
    .await
    .unwrap();
    let mut response = String::new();
    tokio::time::timeout(Duration::from_secs(3), slow.read_to_string(&mut response))
        .await
        .expect("the body deadline answers")
        .unwrap();
    assert!(response.starts_with("HTTP/1.1 408"), "{response}");
    assert!(fetch(address, "/ping").await.contains("\r\npong\r\n"));
}

#[tokio::test]
async fn shutdown_lets_requests_in_flight_finish_then_stops_accepting() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(app(Security::default()).into_service().serve(listener, async {
        let _ = stopped.await;
    }));

    let in_flight = tokio::spawn(fetch(address, "/slow"));
    tokio::time::sleep(Duration::from_millis(100)).await;
    stop.send(()).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!served.is_finished(), "serve waits for the request in flight");

    let response = tokio::time::timeout(Duration::from_secs(5), in_flight)
        .await
        .expect("the request in flight finishes")
        .unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("\r\ndone\r\n"), "{response:?}");
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("serve returns once the request has finished")
        .unwrap()
        .unwrap();
    assert!(TcpStream::connect(address).await.is_err(), "nothing is accepted after shutdown");
}

#[tokio::test]
async fn shutdown_waits_for_requests_in_flight_only_as_long_as_its_grace() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let security = Security { shutdown_grace: Duration::from_millis(200), ..Security::default() };
    let served = tokio::spawn(app(security).into_service().serve(listener, async {
        let _ = stopped.await;
    }));

    let stuck = tokio::spawn(fetch(address, "/stuck"));
    tokio::time::sleep(Duration::from_millis(100)).await;
    let started = Instant::now();
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("serve returns after its grace")
        .unwrap()
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    stuck.abort();
}
