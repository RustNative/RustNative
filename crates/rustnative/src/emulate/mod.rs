//! Local emulators for the serverless hosts (`PLAN.md` Web milestone K):
//! `rustnative serve lambda` runs a function binary behind a function
//! runtime's API, and `rustnative serve wagi` runs a `wasm32-wasip1`
//! module per request under an edge host's limits. Both answer HTTP on a
//! local address, so the application can be browsed and tested in the shape
//! it will be deployed in.

pub mod lambda;
pub mod wagi;

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::TcpStream;

use crate::error::Error;

/// Wraps an I/O error with what was being done.
pub fn io(what: impl Into<String>) -> impl FnOnce(std::io::Error) -> Error {
    let what = what.into();
    move |cause| Error::Io { what, cause }
}

/// A request as the emulators read it off a socket.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpRequest {
    /// `GET`, `POST`, ….
    pub method: String,
    /// The path and query.
    pub target: String,
    /// The headers, names lowercased, in order.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Vec<u8>,
}

impl HttpRequest {
    /// The first header named `name` (lowercase).
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    /// The path, without the query.
    #[must_use]
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("/")
    }

    /// The query, without the `?`.
    #[must_use]
    pub fn query(&self) -> &str {
        self.target.split_once('?').map_or("", |(_, query)| query)
    }
}

/// Reads one request (a body only by `content-length`).
pub fn read_request(stream: &mut TcpStream) -> Option<HttpRequest> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).ok()? == 0 || head.len() > 64 * 1024 {
            return None;
        }
        head.push(byte[0]);
    }
    let text = String::from_utf8_lossy(&head).into_owned();
    let mut lines = text.split("\r\n");
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_owned();
    let target = first.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0; length.min(64 * 1024 * 1024)];
    stream.read_exact(&mut body).ok()?;
    Some(HttpRequest { method, target, headers, body })
}

/// Writes a whole response and closes the exchange.
pub fn write_response(
    stream: &mut TcpStream,
    status: u16,
    headers: &[(String, String)],
    body: &[u8],
) {
    let reason = http_reason(status);
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-length: {}\r\nconnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("content-length") || name.eq_ignore_ascii_case("connection") {
            continue;
        }
        let _ = write!(head, "{name}: {value}\r\n");
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

/// A plain-text error response.
pub fn write_error(stream: &mut TcpStream, status: u16, message: &str) {
    write_response(
        stream,
        status,
        &[("content-type".into(), "text/plain; charset=utf-8".into())],
        message.as_bytes(),
    );
}

const fn http_reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Status",
    }
}
