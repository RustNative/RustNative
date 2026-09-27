//! `rustnative serve lambda <binary>`: a function runtime's API (AWS
//! Lambda's) in front of a function binary, and an HTTP front that turns
//! each request into an API Gateway v2 event.
//!
//! - The binary runs with `AWS_LAMBDA_RUNTIME_API` pointing at the
//!   emulator, `AWS_LAMBDA_FUNCTION_MEMORY_SIZE`, and
//!   `AWS_LAMBDA_FUNCTION_TIMEOUT`. It takes one invocation at a time, as
//!   an instance does.
//! - An invocation carries its deadline (`lambda-runtime-deadline-ms`). One
//!   not answered by then is `504`, and the instance is replaced (a cold
//!   start), as a host does with a function that timed out.
//! - `POST /2015-03-31/functions/function/invocations` invokes the function
//!   with a raw event (an event batch) and returns its raw reply, as the
//!   runtime interface emulator does.
//! - The memory size is reported to the function, not enforced: the host
//!   enforces it; the emulator is for behavior, not isolation.

use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

use super::{HttpRequest, io, read_request, write_error, write_response};
use crate::error::Result;

/// How the emulated function is configured.
#[derive(Debug, Clone, Copy)]
pub struct LambdaOptions {
    /// The memory size reported to the function, in megabytes.
    pub memory_mb: u64,
    /// How long an invocation may take.
    pub timeout: Duration,
    /// Stop after this many front requests (for tests).
    pub requests: Option<usize>,
}

impl Default for LambdaOptions {
    fn default() -> Self {
        Self { memory_mb: 128, timeout: Duration::from_secs(10), requests: None }
    }
}

/// The function's reply: its body, or the error it reported.
type Reply = std::result::Result<Vec<u8>, Vec<u8>>;

struct Invocation {
    id: String,
    event: Vec<u8>,
    deadline_ms: u64,
    reply: Sender<Reply>,
}

#[derive(Default)]
struct Queue {
    waiting: VecDeque<Invocation>,
    running: HashMap<String, Sender<Reply>>,
    next_id: u64,
}

/// The runtime API's shared state.
#[derive(Default)]
struct Runtime {
    queue: Mutex<Queue>,
    ready: Condvar,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
}

impl Runtime {
    /// Queues `event`; the reply arrives on the receiver.
    fn invoke(&self, event: Vec<u8>, timeout: Duration) -> Receiver<Reply> {
        let (reply, receiver) = mpsc::channel();
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        queue.next_id += 1;
        let id = format!("rn-{:08}", queue.next_id);
        let deadline_ms = now_ms() + u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX / 2);
        queue.waiting.push_back(Invocation { id, event, deadline_ms, reply });
        drop(queue);
        self.ready.notify_all();
        receiver
    }

    /// Answers one exchange from the function.
    fn answer(&self, mut stream: TcpStream) {
        let Some(request) = read_request(&mut stream) else { return };
        let path = request.path().to_owned();
        if request.method == "GET" && path == "/2018-06-01/runtime/invocation/next" {
            let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
            let invocation = loop {
                if let Some(invocation) = queue.waiting.pop_front() {
                    break invocation;
                }
                queue = self.ready.wait(queue).unwrap_or_else(PoisonError::into_inner);
            };
            queue.running.insert(invocation.id.clone(), invocation.reply);
            drop(queue);
            write_response(
                &mut stream,
                200,
                &[
                    ("content-type".into(), "application/json".into()),
                    ("lambda-runtime-aws-request-id".into(), invocation.id),
                    ("lambda-runtime-deadline-ms".into(), invocation.deadline_ms.to_string()),
                    (
                        "lambda-runtime-invoked-function-arn".into(),
                        "arn:aws:lambda:local:000000000000:function:rustnative".into(),
                    ),
                ],
                &invocation.event,
            );
            return;
        }
        let rest = path.strip_prefix("/2018-06-01/runtime/invocation/");
        if let (Some(rest), "POST") = (rest, request.method.as_str()) {
            if let Some((id, outcome)) = rest.split_once('/') {
                let sender =
                    self.queue.lock().unwrap_or_else(PoisonError::into_inner).running.remove(id);
                if let Some(sender) = sender {
                    let reply =
                        if outcome == "response" { Ok(request.body) } else { Err(request.body) };
                    let _ = sender.send(reply);
                    write_response(&mut stream, 202, &[], b"");
                    return;
                }
            }
        }
        if path == "/2018-06-01/runtime/init/error" {
            eprintln!(
                "lambda: the function failed to start: {}",
                String::from_utf8_lossy(&request.body)
            );
            write_response(&mut stream, 202, &[], b"");
            return;
        }
        write_error(&mut stream, 404, "not a runtime API path");
    }
}

/// An HTTP request as an API Gateway v2 event.
#[must_use]
pub fn http_event(request: &HttpRequest, peer: Option<SocketAddr>) -> Value {
    let mut headers = Map::new();
    let mut cookies = Vec::new();
    for (name, value) in &request.headers {
        if name == "cookie" {
            cookies.extend(value.split(';').map(|cookie| json!(cookie.trim())));
        } else if let Some(Value::String(existing)) = headers.get_mut(name) {
            existing.push(',');
            existing.push_str(value);
        } else {
            headers.insert(name.clone(), json!(value));
        }
    }
    let (body, encoded) = match std::str::from_utf8(&request.body) {
        Ok(text) => (text.to_owned(), false),
        Err(_) => (base64(&request.body), true),
    };
    json!({
        "version": "2.0",
        "routeKey": "$default",
        "rawPath": request.path(),
        "rawQueryString": request.query(),
        "cookies": cookies,
        "headers": headers,
        "requestContext": {
            "http": {
                "method": request.method,
                "path": request.path(),
                "protocol": "HTTP/1.1",
                "sourceIp": peer.map_or_else(|| "127.0.0.1".to_owned(), |peer| peer.ip().to_string()),
                "userAgent": request.header("user-agent").unwrap_or_default(),
            },
            "requestId": "local",
            "stage": "$default",
            "timeEpoch": now_ms(),
        },
        "body": body,
        "isBase64Encoded": encoded,
    })
}

/// Standard base64 with padding.
#[must_use]
pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let group = (u32::from(chunk[0]) << 16)
            | (u32::from(chunk.get(1).copied().unwrap_or(0)) << 8)
            | u32::from(chunk.get(2).copied().unwrap_or(0));
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(char::from(TABLE[(group >> (18 - 6 * index)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn unbase64(text: &str) -> Option<Vec<u8>> {
    let value = |byte: u8| match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let bytes: Vec<u8> =
        text.bytes().filter(|byte| !byte.is_ascii_whitespace() && *byte != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut group = 0u32;
        for (index, byte) in chunk.iter().enumerate() {
            group |= u32::from(value(*byte)?) << (18 - 6 * index);
        }
        let group = group.to_be_bytes();
        out.extend_from_slice(&group[1..chunk.len()]);
    }
    Some(out)
}

/// A function's reply to an HTTP event as a response: status, headers, body.
#[must_use]
pub fn http_response(reply: &Value) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let status = reply
        .get("statusCode")
        .and_then(Value::as_u64)
        .and_then(|code| u16::try_from(code).ok())
        .unwrap_or(200);
    let mut headers: Vec<(String, String)> = reply
        .get("headers")
        .and_then(Value::as_object)
        .map(|headers| {
            headers
                .iter()
                .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    if let Some(cookies) = reply.get("cookies").and_then(Value::as_array) {
        headers.extend(
            cookies
                .iter()
                .filter_map(Value::as_str)
                .map(|cookie| ("set-cookie".to_owned(), cookie.to_owned())),
        );
    }
    let text = reply.get("body").and_then(Value::as_str).unwrap_or_default();
    let body = if reply.get("isBase64Encoded").and_then(Value::as_bool).unwrap_or(false) {
        unbase64(text).unwrap_or_default()
    } else {
        text.as_bytes().to_vec()
    };
    (status, headers, body)
}

/// The function's process: started on first use, replaced after a timeout.
struct Instance {
    binary: PathBuf,
    api: SocketAddr,
    options: LambdaOptions,
    child: Option<Child>,
}

impl Instance {
    fn ensure(&mut self) {
        if let Some(child) = &mut self.child {
            if matches!(child.try_wait(), Ok(None)) {
                return;
            }
        }
        match Command::new(&self.binary)
            .env("AWS_LAMBDA_RUNTIME_API", self.api.to_string())
            .env("AWS_LAMBDA_FUNCTION_MEMORY_SIZE", self.options.memory_mb.to_string())
            .env("AWS_LAMBDA_FUNCTION_TIMEOUT", self.options.timeout.as_secs().max(1).to_string())
            .env("AWS_LAMBDA_FUNCTION_NAME", "rustnative")
            .spawn()
        {
            Ok(child) => self.child = Some(child),
            Err(error) => eprintln!("lambda: could not start {}: {error}", self.binary.display()),
        }
    }

    fn replace(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        self.replace();
    }
}

/// `rustnative serve lambda`: serves `binary` as a function behind `address`.
///
/// # Errors
///
/// An address could not be bound.
pub fn serve(binary: &Path, address: SocketAddr, options: LambdaOptions) -> Result<()> {
    let front = TcpListener::bind(address).map_err(io(format!("listen on {address}")))?;
    println!("serve lambda: http://{}", front.local_addr().unwrap_or(address));
    serve_on(&front, binary, options)
}

/// [`serve`] on an existing listener.
///
/// # Errors
///
/// The runtime API's address could not be bound.
pub fn serve_on(front: &TcpListener, binary: &Path, options: LambdaOptions) -> Result<()> {
    let api_listener = TcpListener::bind("127.0.0.1:0").map_err(io("bind the runtime API"))?;
    let api = api_listener.local_addr().map_err(io("read the runtime API's address"))?;
    let runtime = Arc::new(Runtime::default());
    let serving = Arc::clone(&runtime);
    std::thread::spawn(move || {
        for stream in api_listener.incoming().flatten() {
            let runtime = Arc::clone(&serving);
            std::thread::spawn(move || runtime.answer(stream));
        }
    });
    let instance =
        Arc::new(Mutex::new(Instance { binary: binary.to_owned(), api, options, child: None }));
    let mut served = 0;
    let mut workers = Vec::new();
    for stream in front.incoming() {
        let Ok(stream) = stream else { continue };
        let runtime = Arc::clone(&runtime);
        let instance = Arc::clone(&instance);
        let timeout = options.timeout;
        workers
            .push(std::thread::spawn(move || front_request(stream, &runtime, &instance, timeout)));
        served += 1;
        if options.requests.is_some_and(|limit| served >= limit) {
            break;
        }
    }
    for worker in workers {
        let _ = worker.join();
    }
    Ok(())
}

fn front_request(
    mut stream: TcpStream,
    runtime: &Runtime,
    instance: &Mutex<Instance>,
    timeout: Duration,
) {
    let peer = stream.peer_addr().ok();
    let Some(request) = read_request(&mut stream) else { return };
    let raw =
        request.method == "POST" && request.path() == "/2015-03-31/functions/function/invocations";
    let event = if raw {
        request.body.clone()
    } else {
        http_event(&request, peer).to_string().into_bytes()
    };
    instance.lock().unwrap_or_else(PoisonError::into_inner).ensure();
    let receiver = runtime.invoke(event, timeout);
    match receiver.recv_timeout(timeout + Duration::from_millis(250)) {
        Ok(Ok(body)) if raw => {
            write_response(
                &mut stream,
                200,
                &[("content-type".into(), "application/json".into())],
                &body,
            );
        }
        Ok(Ok(body)) => {
            let reply: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let (status, headers, body) = http_response(&reply);
            write_response(&mut stream, status, &headers, &body);
        }
        Ok(Err(error)) => {
            write_response(
                &mut stream,
                502,
                &[("content-type".into(), "application/json".into())],
                &error,
            );
        }
        Err(_) => {
            // Timed out: the instance is replaced, as the host does.
            instance.lock().unwrap_or_else(PoisonError::into_inner).replace();
            write_error(&mut stream, 504, "the function did not answer before its timeout");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        for text in [&b""[..], b"M", b"Ma", b"Man", b"\x00\xff\x10binary"] {
            assert_eq!(unbase64(&base64(text)).unwrap(), text);
        }
        assert_eq!(base64(b"Ma"), "TWE=");
    }

    #[test]
    fn a_reply_becomes_a_response() {
        let (status, headers, body) = http_response(&json!({
            "statusCode": 303,
            "headers": { "location": "/notes" },
            "cookies": ["a=1; Path=/", "b=2"],
            "body": "aGk=",
            "isBase64Encoded": true,
        }));
        assert_eq!(status, 303);
        assert_eq!(body, b"hi");
        assert_eq!(headers.iter().filter(|(name, _)| name == "set-cookie").count(), 2);
    }
}
