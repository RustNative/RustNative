//! The application's listener out of file descriptors: accepting fails with
//! `EMFILE` while the shortage lasts, and the listener serves again once it
//! passes. Its own test binary, because the descriptor limit is the
//! process's.

#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::time::{Duration, Instant};

use rustnative_server::{Security, ServerApp, get};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// The highest descriptor the process has open.
fn highest_open_descriptor() -> u64 {
    let directory =
        if std::path::Path::new("/proc/self/fd").exists() { "/proc/self/fd" } else { "/dev/fd" };
    std::fs::read_dir(directory)
        .unwrap()
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse().ok())
        .max()
        .unwrap()
}

fn set_descriptor_limit(soft: u64) -> libc::rlimit {
    let mut old = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: `old` is a valid, writable `rlimit` for the call to fill.
    assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut old) }, 0);
    let new = libc::rlimit { rlim_cur: soft.min(old.rlim_max), rlim_max: old.rlim_max };
    // SAFETY: `new` is a valid `rlimit` that only lowers the soft limit.
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &raw const new) }, 0);
    old
}

#[tokio::test]
async fn running_out_of_file_descriptors_does_not_stop_the_listener() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = ServerApp::new()
        .security(Security { rate_limit: None, ..Security::default() })
        .route("/ping", get(|| async { "pong" }).public());
    let served = tokio::spawn(app.into_service().serve(listener, std::future::pending()));
    tokio::task::yield_now().await;

    // A few descriptors of room, which the connections below use up: both
    // ends of each are this process's, so the listener's accept runs out.
    let old = set_descriptor_limit(highest_open_descriptor() + 9);
    let mut clients = Vec::new();
    for _ in 0..64 {
        match TcpStream::connect(address).await {
            Ok(stream) => clients.push(stream),
            Err(_) => break,
        }
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!served.is_finished(), "an accept that ran out of descriptors does not stop serving");

    drop(clients);
    // SAFETY: `old` is the `rlimit` `getrlimit` filled.
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &raw const old) }, 0);
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
        assert!(Instant::now() < deadline, "the listener serves again: {response:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!served.is_finished());
}
