//! The MQTT broker's limits on what a client can make it do: a declared
//! packet length past the maximum is refused before anything is allocated
//! for it, a connection that never says CONNECT is closed, and one past
//! the connection cap is closed on accept.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rustnative_sync::bus::{Broker, Connect};
use rustnative_sync::mqtt::{DEFAULT_MAX_PACKET, Limits, MqttClient, Packet, serve_with};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// The system allocator, remembering the largest single allocation.
struct Largest;

static LARGEST: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call is forwarded unchanged to `System`, which upholds
// `GlobalAlloc`'s contract; the only addition is an atomic store.
unsafe impl GlobalAlloc for Largest {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LARGEST.fetch_max(layout.size(), Ordering::Relaxed);
        // SAFETY: the caller's obligations for `alloc` pass through as is.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        LARGEST.fetch_max(layout.size(), Ordering::Relaxed);
        // SAFETY: the caller's obligations for `alloc_zeroed` pass through.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `System` with this `layout`.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LARGEST.fetch_max(new_size, Ordering::Relaxed);
        // SAFETY: the caller's obligations for `realloc` pass through.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Largest = Largest;

/// Waits for the broker to close `stream`, failing after two seconds.
async fn closed(mut stream: TcpStream) {
    let mut byte = [0u8; 1];
    let read = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut byte))
        .await
        .expect("the broker closes the connection");
    assert!(matches!(read, Ok(0) | Err(_)), "nothing is sent before closing");
}

#[tokio::test]
async fn an_oversized_length_is_refused_without_allocating() {
    // PUBLISH, declaring the largest remaining length MQTT can encode
    // (268,435,455 bytes), then no body at all.
    let declared: &[u8] = &[0x30, 0xFF, 0xFF, 0xFF, 0x7F];
    LARGEST.store(0, Ordering::Relaxed);
    let error = Packet::read(&mut { declared }).await.unwrap_err();
    assert!(
        error.contains("exceeds the limit"),
        "refused for its size, not the missing body: {error}"
    );
    assert!(
        LARGEST.load(Ordering::Relaxed) < 2 * DEFAULT_MAX_PACKET,
        "no buffer of the declared size was allocated"
    );

    let small: &[u8] = &[0x30, 0x81, 0x01];
    let error = Packet::read_limited(&mut { small }, 128).await.unwrap_err();
    assert!(error.contains("129 bytes exceeds the limit of 128"), "{error}");
}

#[tokio::test]
async fn a_connection_that_never_connects_is_closed() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let limits = Limits { connect_timeout: Duration::from_millis(100), ..Limits::default() };
    tokio::spawn(serve_with(Broker::new(), listener, limits));
    closed(TcpStream::connect(address).await.unwrap()).await;
}

#[tokio::test]
async fn an_oversized_connect_closes_the_connection() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(serve_with(Broker::new(), listener, Limits::default()));
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream.write_all(&[0x10, 0xFF, 0xFF, 0xFF, 0x7F]).await.unwrap();
    closed(stream).await;
}

#[tokio::test]
async fn a_connection_past_the_cap_is_closed() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let limits = Limits { max_connections: 1, ..Limits::default() };
    tokio::spawn(serve_with(Broker::new(), listener, limits));
    let first = MqttClient::connect(
        &address.to_string(),
        Connect { client_id: "first".into(), clean_session: true, will: None },
    )
    .await
    .unwrap();
    closed(TcpStream::connect(address).await.unwrap()).await;
    first.disconnect().await;
}
