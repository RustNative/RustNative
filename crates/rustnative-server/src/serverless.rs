//! Serverless shapes (Web milestone K): the same application —
//! [`crate::AppService::handle`], its routes, security, sessions, server
//! functions, and pages — answering one invocation at a time on the
//! invocation's own thread, in two shapes:
//!
//! - [`lambda`]: a native binary speaking a function runtime's API (AWS
//!   Lambda's), taking HTTP events (API Gateway v2, function URLs) and any
//!   other event (an event batch) through a hook;
//! - [`wagi`]: a `wasm32-wasip1` module run per request by an edge host,
//!   the request in CGI variables and standard input, the response on
//!   standard output (WAGI, as Spin and wagi run it).
//!
//! [`run`] picks the shape from the environment. Configuration and secrets
//! are read from the environment per invocation; nothing the application
//! keeps in a process-wide static crosses invocations except what a service
//! holds. What the host allows a request — its deadline, its memory, its
//! response size — is a [`HostLimits`] in the request's values, which pages
//! read through `rustnative_web::request(&context).limits()`.

use std::future::Future;

use bytes::Bytes;
pub use rustnative_web::HostLimits;

use crate::body::Streaming;
use crate::{AppService, ServerApp};

/// Runs `future` to completion on this thread, on a single-threaded
/// runtime that is the invocation's own, dropped when it returns: whatever
/// the invocation spawned and left running is cancelled then — no work
/// outlives the response.
///
/// # Panics
///
/// The runtime could not be made (never on a supported target).
#[allow(clippy::expect_used, reason = "a single-threaded runtime always builds")]
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut builder = tokio::runtime::Builder::new_current_thread();
    #[cfg(feature = "serve")]
    builder.enable_io();
    builder.enable_time().build().expect("the invocation's runtime").block_on(future)
}

/// Answers one request: what both shapes do with the request they decoded.
/// A streamed page is answered whole — neither shape streams.
#[must_use]
pub fn answer(
    service: &AppService,
    mut request: http::Request<Bytes>,
    limits: HostLimits,
) -> crate::Response {
    request.extensions_mut().insert(limits);
    block_on(async {
        let response = service.handle(request, None).await;
        let stream = response.extensions().get::<Streaming>().and_then(Streaming::take);
        let Some(mut chunks) = stream else { return response };
        let mut body = Vec::new();
        while let Some(chunk) = chunks.recv().await {
            body.extend_from_slice(&chunk);
        }
        response.map(|_| Bytes::from(body))
    })
}

/// Runs `app` in the serverless shape the environment names: a function
/// runtime (`AWS_LAMBDA_RUNTIME_API`), or one WAGI request
/// (`REQUEST_METHOD`).
#[must_use]
pub fn run(app: ServerApp) -> std::process::ExitCode {
    let service = app.into_service();
    if std::env::var_os("AWS_LAMBDA_RUNTIME_API").is_some() {
        lambda::run(&service, None);
    }
    if std::env::var_os("REQUEST_METHOD").is_some() {
        return wagi::run(&service);
    }
    eprintln!("not in a serverless host: no AWS_LAMBDA_RUNTIME_API and no REQUEST_METHOD");
    std::process::ExitCode::FAILURE
}

/// Whether `bytes` is text a JSON string can carry as it is.
fn is_text(bytes: &[u8], content_type: Option<&str>) -> bool {
    let textual = content_type.is_none_or(|kind| {
        kind.starts_with("text/")
            || kind.contains("json")
            || kind.contains("javascript")
            || kind.contains("xml")
            || kind.contains("x-www-form-urlencoded")
    });
    textual && std::str::from_utf8(bytes).is_ok()
}

/// A function runtime's API (AWS Lambda's): the invocation loop.
pub mod lambda {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use base64::Engine;
    use bytes::Bytes;
    use serde_json::{Map, Value, json};

    use super::{HostLimits, answer, is_text};
    use crate::AppService;

    /// Answers events that are not HTTP requests (an event batch): the
    /// event, and the time left; the reply.
    pub type EventHook = Box<dyn Fn(Value, Duration) -> Value>;

    /// A response's headers, names lowercased.
    type Headers = Vec<(String, String)>;

    /// One HTTP exchange with the runtime API at `api` (`host:port`).
    fn exchange(
        api: &str,
        method: &str,
        path: &str,
        body: &[u8],
    ) -> std::io::Result<(Headers, Vec<u8>)> {
        let mut stream = TcpStream::connect(api)?;
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {api}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )?;
        stream.write_all(body)?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response)?;
        let split = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map_or(response.len(), |at| at + 4);
        let head = String::from_utf8_lossy(&response[..split]).into_owned();
        let headers = head
            .lines()
            .skip(1)
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
            .collect();
        Ok((headers, response[split..].to_vec()))
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
    }

    /// An API Gateway v2 (or function URL) event as a request.
    fn request(event: &Value) -> Option<http::Request<Bytes>> {
        let http = event.pointer("/requestContext/http")?;
        let method = http.get("method")?.as_str()?;
        let path = event.get("rawPath").and_then(Value::as_str).unwrap_or("/");
        let query = event.get("rawQueryString").and_then(Value::as_str).unwrap_or_default();
        let uri = if query.is_empty() { path.to_owned() } else { format!("{path}?{query}") };
        let mut builder = http::Request::builder().method(method).uri(uri);
        if let Some(headers) = event.get("headers").and_then(Value::as_object) {
            for (name, value) in headers {
                if let Some(value) = value.as_str() {
                    builder = builder.header(name.as_str(), value);
                }
            }
        }
        if let Some(cookies) = event.get("cookies").and_then(Value::as_array) {
            let joined: Vec<&str> = cookies.iter().filter_map(Value::as_str).collect();
            if !joined.is_empty() {
                builder = builder.header("cookie", joined.join("; "));
            }
        }
        let body = event.get("body").and_then(Value::as_str).unwrap_or_default();
        let body = if event.get("isBase64Encoded").and_then(Value::as_bool).unwrap_or(false) {
            base64::engine::general_purpose::STANDARD.decode(body).ok()?
        } else {
            body.as_bytes().to_vec()
        };
        builder.body(Bytes::from(body)).ok()
    }

    /// A response as the function runtime returns it.
    fn reply(response: &crate::Response) -> Value {
        let mut headers = Map::new();
        let mut cookies = Vec::new();
        for (name, value) in response.headers() {
            let Ok(value) = value.to_str() else { continue };
            if name == http::header::SET_COOKIE {
                cookies.push(json!(value));
            } else if let Some(Value::String(existing)) = headers.get_mut(name.as_str()) {
                existing.push_str(", ");
                existing.push_str(value);
            } else {
                headers.insert(name.as_str().to_owned(), json!(value));
            }
        }
        let content_type = response
            .headers()
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok());
        let body = response.body();
        let (body, encoded) = if is_text(body, content_type) {
            (String::from_utf8_lossy(body).into_owned(), false)
        } else {
            (base64::engine::general_purpose::STANDARD.encode(body), true)
        };
        json!({
            "statusCode": response.status().as_u16(),
            "headers": headers,
            "cookies": cookies,
            "body": body,
            "isBase64Encoded": encoded,
        })
    }

    /// What this function's host allows each invocation.
    fn limits(remaining: Duration) -> HostLimits {
        let memory = std::env::var("AWS_LAMBDA_FUNCTION_MEMORY_SIZE")
            .ok()
            .and_then(|megabytes| megabytes.parse::<u64>().ok());
        HostLimits {
            deadline: Some(remaining),
            memory: memory.map(|megabytes| megabytes * 1024 * 1024),
            // Only `/tmp`, and only for this instance.
            filesystem: true,
            response_bytes: Some(6 * 1024 * 1024),
            payload_bytes: Some(6 * 1024 * 1024),
        }
    }

    /// Handles invocations from the runtime API in `AWS_LAMBDA_RUNTIME_API`
    /// for as long as the process lives: HTTP events answered by `service`,
    /// anything else by `events`.
    pub fn run(service: &AppService, events: Option<&EventHook>) -> ! {
        let api = std::env::var("AWS_LAMBDA_RUNTIME_API").unwrap_or_default();
        loop {
            if next(&api, service, events).is_err() {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }

    /// Takes the next invocation from the runtime API at `api` and answers
    /// it. An invocation whose deadline has already passed is refused
    /// (reported as an error), not started.
    ///
    /// # Errors
    ///
    /// The runtime API could not be reached.
    pub fn next(
        api: &str,
        service: &AppService,
        events: Option<&EventHook>,
    ) -> std::io::Result<()> {
        let (headers, body) = exchange(api, "GET", "/2018-06-01/runtime/invocation/next", &[])?;
        let header = |name: &str| {
            headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone())
        };
        let Some(id) = header("lambda-runtime-aws-request-id") else {
            return Err(std::io::Error::other("an invocation without an id"));
        };
        let deadline = header("lambda-runtime-deadline-ms")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(u64::MAX);
        let remaining = Duration::from_millis(deadline.saturating_sub(now_ms()));
        let fail = |kind: &str, message: &str| {
            let error = json!({ "errorType": kind, "errorMessage": message });
            let path = format!("/2018-06-01/runtime/invocation/{id}/error");
            exchange(api, "POST", &path, error.to_string().as_bytes()).map(|_| ())
        };
        if remaining.is_zero() {
            return fail(
                "DeadlineExceeded",
                "the invocation's deadline had passed before it started",
            );
        }
        let event: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let reply = if let Some(request) = request(&event) {
            reply(&answer(service, request, limits(remaining)))
        } else if let Some(hook) = events {
            hook(event, remaining)
        } else {
            return fail("UnknownEvent", "not an HTTP event, and no event handler");
        };
        let path = format!("/2018-06-01/runtime/invocation/{id}/response");
        exchange(api, "POST", &path, reply.to_string().as_bytes()).map(|_| ())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn an_http_event_becomes_a_request_and_back() {
            let event = json!({
                "version": "2.0",
                "rawPath": "/notes",
                "rawQueryString": "sort=new",
                "headers": { "accept": "text/html" },
                "cookies": ["a=1", "b=2"],
                "requestContext": { "http": { "method": "POST", "sourceIp": "10.0.0.1" } },
                "body": "dGl0bGU9TWlsaw==",
                "isBase64Encoded": true,
            });
            let request = request(&event).unwrap();
            assert_eq!(request.method(), http::Method::POST);
            assert_eq!(request.uri(), "/notes?sort=new");
            assert_eq!(request.headers()["cookie"], "a=1; b=2");
            assert_eq!(request.body().as_ref(), b"title=Milk");

            let mut response = http::Response::new(Bytes::from_static(b"\x89PNG"));
            response.headers_mut().insert("content-type", "image/png".parse().unwrap());
            response.headers_mut().append("set-cookie", "s=1".parse().unwrap());
            let reply = reply(&response);
            assert_eq!(reply["isBase64Encoded"], true);
            assert_eq!(reply["cookies"], json!(["s=1"]));
            assert!(reply["headers"].get("set-cookie").is_none());
        }
    }
}

/// WAGI: one request per module run, in CGI's shape over WASI.
pub mod wagi {
    use std::io::{Read, Write};
    use std::time::Duration;

    use bytes::Bytes;

    use super::{HostLimits, answer};
    use crate::AppService;

    fn var(name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|value| !value.is_empty())
    }

    /// The request the CGI variables and standard input describe.
    #[must_use]
    pub fn request(body: Vec<u8>) -> Option<http::Request<Bytes>> {
        let method = var("REQUEST_METHOD")?;
        let path = var("PATH_INFO")
            .or_else(|| var("X_RELATIVE_PATH").map(|path| format!("/{path}")))
            .unwrap_or_else(|| "/".into());
        let uri = match var("QUERY_STRING") {
            Some(query) => format!("{path}?{query}"),
            None => path,
        };
        let mut builder = http::Request::builder().method(method.as_str()).uri(uri);
        for (name, value) in std::env::vars() {
            if let Some(header) = name.strip_prefix("HTTP_") {
                builder = builder.header(header.replace('_', "-").to_ascii_lowercase(), value);
            }
        }
        if let Some(kind) = var("CONTENT_TYPE") {
            builder = builder.header("content-type", kind);
        }
        builder.body(Bytes::from(body)).ok()
    }

    /// What the host allows this request, as the emulator (or a host that
    /// follows the same convention) states it.
    #[must_use]
    pub fn limits() -> HostLimits {
        let number = |name: &str| var(name).and_then(|value| value.parse::<u64>().ok());
        HostLimits {
            deadline: number("RUSTNATIVE_LIMIT_DEADLINE_MS").map(Duration::from_millis),
            memory: number("RUSTNATIVE_LIMIT_MEMORY_BYTES"),
            filesystem: false,
            response_bytes: number("RUSTNATIVE_LIMIT_RESPONSE_BYTES"),
            payload_bytes: number("RUSTNATIVE_LIMIT_PAYLOAD_BYTES"),
        }
    }

    /// Answers the one request of this run.
    #[must_use]
    pub fn run(service: &AppService) -> std::process::ExitCode {
        let mut body = Vec::new();
        let _ = std::io::stdin().read_to_end(&mut body);
        let Some(request) = request(body) else {
            print!("Status: 400 Bad Request\r\ncontent-type: text/plain\r\n\r\nnot a CGI request");
            return std::process::ExitCode::FAILURE;
        };
        let response = answer(service, request, limits());
        let mut out = std::io::stdout().lock();
        let _ = write!(out, "Status: {}\r\n", response.status());
        for (name, value) in response.headers() {
            if let Ok(value) = value.to_str() {
                let _ = write!(out, "{name}: {value}\r\n");
            }
        }
        let _ = out.write_all(b"\r\n");
        let _ = out.write_all(response.body());
        let _ = out.flush();
        std::process::ExitCode::SUCCESS
    }
}
