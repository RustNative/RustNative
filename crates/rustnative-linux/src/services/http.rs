//! `SoupHttp`: the framework's [`HttpService`] on Linux, over libsoup 3 —
//! GNOME's HTTP stack, with the system's trust store and TLS through
//! glib-networking and the desktop's proxy settings through GIO — with
//! certificate pinning as declared policy (`PLAN.md` Milestone 47, `C34`).
//!
//! Every request runs on one HTTP thread that owns the libsoup session and
//! its main context, so connections are reused across requests. For a host
//! with pins ([`CertificatePins`]) the request goes out on a connection of
//! its own: when its TLS handshake completes, the server's certificate is
//! checked against the pins, and a mismatch cancels the request before a
//! byte of it is written. Redirects are not followed for a pinned host, so a
//! redirect cannot lead the request somewhere the pins do not cover.

use std::sync::{Arc, OnceLock};

use gio::prelude::*;
use glib::translate::IntoGlib as _;
use rustnative_core::{CertificatePins, HttpRequest, HttpResponse, HttpService, ServiceError};
use soup::prelude::*;

/// The SHA-256 digest of `bytes`, by GLib.
fn sha256(bytes: &[u8]) -> Option<[u8; 32]> {
    let mut checksum = glib::Checksum::new(glib::ChecksumType::Sha256)?;
    checksum.update(bytes);
    let mut digest = [0_u8; 32];
    digest.copy_from_slice(checksum.digest().get(..32)?);
    Some(digest)
}

/// The framework's HTTP service over libsoup.
#[derive(Debug, Clone)]
pub struct SoupHttp {
    pins: Arc<CertificatePins>,
    agent: String,
}

impl SoupHttp {
    /// A service with no pins.
    #[must_use]
    pub fn new() -> Self {
        Self { pins: Arc::new(CertificatePins::new()), agent: "RustNative".to_owned() }
    }

    /// The service, refusing a pinned host whose certificate matches none
    /// of its pins.
    #[must_use]
    pub fn with_pins(mut self, pins: CertificatePins) -> Self {
        self.pins = Arc::new(pins);
        self
    }
}

impl Default for SoupHttp {
    fn default() -> Self {
        Self::new()
    }
}

/// The HTTP thread's main context, started on first use (`None` if the
/// thread could not be started).
fn worker() -> Option<&'static glib::MainContext> {
    static WORKER: OnceLock<Option<glib::MainContext>> = OnceLock::new();
    WORKER
        .get_or_init(|| {
            let (ready, started) = std::sync::mpsc::channel();
            std::thread::Builder::new()
                .name("rustnative-http".into())
                .spawn(move || {
                    let context = glib::MainContext::new();
                    let _ = context.with_thread_default(|| {
                        let main = glib::MainLoop::new(Some(&context), false);
                        let _ = ready.send(context.clone());
                        main.run();
                    });
                })
                .ok()?;
            started.recv().ok()
        })
        .as_ref()
}

thread_local! {
    /// The HTTP thread's session.
    static SESSION: soup::Session = soup::Session::new();
}

#[async_trait::async_trait]
impl HttpService for SoupHttp {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ServiceError> {
        let pins = Arc::clone(&self.pins);
        let agent = self.agent.clone();
        let (reply, answer) = tokio::sync::oneshot::channel();
        let worker =
            worker().ok_or_else(|| ServiceError::new("the HTTP thread could not start"))?;
        worker.invoke(move || {
            glib::MainContext::ref_thread_default().spawn_local(async move {
                let _ = reply.send(send(&agent, &pins, &request).await);
            });
        });
        answer.await.map_err(|_| ServiceError::new("the HTTP thread stopped"))?
    }
}

async fn send(
    agent: &str,
    pins: &CertificatePins,
    request: &HttpRequest,
) -> Result<HttpResponse, ServiceError> {
    let uri = glib::Uri::parse(request.url(), glib::UriFlags::ENCODED)
        .map_err(|error| ServiceError::new(format!("`{}` is not a URL: {error}", request.url())))?;
    let host = uri.host().map(|host| host.to_string()).unwrap_or_default();
    let pinned = pins.is_pinned(&host);
    if pinned && uri.scheme() != "https" {
        return Err(ServiceError::new(format!(
            "{host} is pinned, and cannot be reached over plain HTTP"
        )));
    }
    let message = soup::Message::from_uri(request.method().as_str(), &uri);
    let headers = message
        .request_headers()
        .ok_or_else(|| ServiceError::new("libsoup made a message without headers"))?;
    headers.replace("User-Agent", agent);
    for (name, value) in request.headers() {
        headers.append(name, value);
    }
    if !request.body_bytes().is_empty()
        || matches!(request.method().as_str(), "POST" | "PUT" | "PATCH")
    {
        message.set_request_body_from_bytes(None, Some(&glib::Bytes::from(request.body_bytes())));
    }
    let cancellable = gio::Cancellable::new();
    let refused = Arc::new(std::sync::Mutex::new(None::<String>));
    if pinned {
        message.add_flags(soup::MessageFlags::NEW_CONNECTION | soup::MessageFlags::NO_REDIRECT);
        let (pins, host, cancel, verdict) =
            (pins.clone(), host.clone(), cancellable.clone(), Arc::clone(&refused));
        // Runs on the HTTP thread as the handshake completes, before the
        // request is written.
        // Connected by name: the binding's typed handler assumes a stream
        // with every event, and libsoup passes none while resolving.
        message.connect_local("network-event", false, move |values| {
            let event = values.get(1).and_then(|value| value.get::<gio::SocketClientEvent>().ok());
            if event != Some(gio::SocketClientEvent::TlsHandshaked) {
                return None;
            }
            let connection =
                values.get(2).and_then(|value| value.get::<Option<gio::IOStream>>().ok()).flatten();
            let digest = connection
                .and_then(|connection| connection.downcast::<gio::TlsConnection>().ok())
                .as_ref()
                .and_then(gio::prelude::TlsConnectionExt::peer_certificate)
                .and_then(|certificate| {
                    certificate.property::<Option<glib::ByteArray>>("certificate")
                })
                .and_then(|der| sha256(&der));
            if !digest.is_some_and(|digest| pins.allows(&host, &digest)) {
                *verdict.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some(format!("{host}'s certificate matches none of its pins"));
                cancel.cancel();
            }
            None
        });
    }
    let (done, result) = tokio::sync::oneshot::channel();
    SESSION.with(soup::Session::clone).send_and_read_async(
        &message,
        glib::Priority::DEFAULT,
        Some(&cancellable),
        move |outcome| {
            let _ = done.send(outcome);
        },
    );
    let outcome = result.await.map_err(|_| ServiceError::new("libsoup dropped the request"))?;
    if let Some(reason) = refused.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take() {
        return Err(ServiceError::new(reason));
    }
    let body = outcome
        .map_err(|error| ServiceError::new(format!("the request to {host} failed: {error}")))?;
    let mut response_headers = Vec::new();
    if let Some(received) = message.response_headers() {
        received.foreach(|name, value| response_headers.push((name.to_owned(), value.to_owned())));
    }
    let status = u16::try_from(message.status().into_glib()).unwrap_or(0);
    Ok(HttpResponse::new(status, response_headers, body.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run<T>(future: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime")
            .block_on(future)
    }

    #[test]
    fn sha256_matches_the_standard_vector() {
        let digest = sha256(b"abc").expect("GLib computes SHA-256");
        assert_eq!(digest[..4], [0xba, 0x78, 0x16, 0xbf]);
        assert_eq!(digest[28..], [0xf2, 0x00, 0x15, 0xad]);
    }

    #[test]
    fn a_request_round_trips_through_a_local_server() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let port = listener.local_addr().map_or(0, |address| address.port());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("a connection");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            while !String::from_utf8_lossy(&request).contains("hello") {
                let read = stream.read(&mut buffer).unwrap_or(0);
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            let _ = stream.write_all(b"HTTP/1.1 201 Created\r\nX-Test: yes\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
            String::from_utf8_lossy(&request).into_owned()
        });
        let request = HttpRequest::new(
            rustnative_core::Method::Post,
            format!("http://127.0.0.1:{port}/echo"),
        )
        .header("X-Client", "rustnative")
        .body(b"hello".to_vec());
        let response = run(SoupHttp::new().execute(request)).expect("a response");
        assert_eq!(response.status(), 201);
        assert_eq!(response.body_bytes(), b"ok");
        assert!(response.headers().iter().any(|(name, value)| name == "X-Test" && value == "yes"));
        let seen = server.join().unwrap_or_default();
        assert!(seen.starts_with("POST /echo") && seen.contains("X-Client: rustnative"), "{seen}");
    }

    #[test]
    fn a_pinned_host_is_refused_over_plain_http_and_on_a_mismatch() {
        let pins = CertificatePins::new().pin("example.com", [0; 32]);
        let http = SoupHttp::new().with_pins(pins);
        let outcome = run(http.execute(HttpRequest::get("http://example.com/")));
        assert!(outcome.is_err_and(|error| error.to_string().contains("plain HTTP")));
        // Over TLS the real certificate matches no pin: refused, if this
        // machine can reach the host at all.
        match run(http.execute(HttpRequest::get("https://example.com/"))) {
            Err(error) if error.to_string().contains("none of its pins") => {}
            Err(error) => eprintln!("skipped the TLS half (offline?): {error}"),
            Ok(response) => panic!("a mismatched pin was accepted: {}", response.status()),
        }
    }
}
