//! The transport: line-delimited JSON over TCP.
//!
//! Each line a client sends is `{"token": …, "version": 1, "request":
//! {"request": "tree", …}}`; each line the server writes back is a
//! [`Reply`]. The listener binds loopback unless the host binds another
//! address explicitly (a device on the network, inspected from a
//! development machine); either way nothing is answered without the token,
//! which is generated per process and handed to the client out of band
//! (the endpoint file `rustnative inspect` reads, or the host's own log).
//!
//! Connections are served on their own threads, at most
//! [`MAX_CONNECTIONS`] at once; requests are answered on the UI thread,
//! which the server wakes and which drains them in [`InspectServer::poll`]
//! — the application is never touched off its thread. A line is at most
//! [`MAX_LINE`] bytes, and a connection that has not yet sent the token
//! has [`AUTH_TIMEOUT`] for each line.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{PROTOCOL_VERSION, Reply, Request};

/// How long a connection waits for the UI thread to answer.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

/// The longest line a client may send, in bytes (1 MiB). A longer one is
/// answered with an error and the connection is closed.
pub const MAX_LINE: usize = 1024 * 1024;

/// The most connections served at once; one more is answered with an
/// error and closed.
pub const MAX_CONNECTIONS: usize = 16;

/// How long a connection that has not yet sent the token may take over
/// each line.
pub const AUTH_TIMEOUT: Duration = Duration::from_secs(10);

/// Where an inspection server listens, and the token it requires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    /// The address.
    pub addr: SocketAddr,
    /// The token.
    pub token: String,
    /// The serving process.
    pub pid: u32,
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    token: String,
    version: u32,
    request: Request,
}

struct Pending {
    request: Request,
    reply: Sender<Reply>,
}

/// A running inspection server.
pub struct InspectServer {
    endpoint: Endpoint,
    requests: Receiver<Pending>,
    file: Option<PathBuf>,
}

impl std::fmt::Debug for InspectServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InspectServer").field("addr", &self.endpoint.addr).finish_non_exhaustive()
    }
}

/// A token no other process can predict: 128 bits from the operating
/// system's cryptographic random number generator.
///
/// # Errors
///
/// The generator is unavailable.
pub fn new_token() -> io::Result<String> {
    let bytes = random_bytes()?;
    Ok(bytes.iter().fold(String::with_capacity(32), |mut token, byte| {
        use std::fmt::Write as _;
        let _ = write!(token, "{byte:02x}");
        token
    }))
}

#[cfg(feature = "os-random")]
fn random_bytes() -> io::Result<[u8; 16]> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
    Ok(bytes)
}

/// The inspection server is a native host's: a build without the operating
/// system's random source (a browser subtree) has none, and refuses to start
/// one rather than serve a guessable token.
#[cfg(not(feature = "os-random"))]
fn random_bytes() -> io::Result<[u8; 16]> {
    Err(io::Error::other("this build has no random source for an inspection token"))
}

impl InspectServer {
    /// Listens on `bind`, calling `wake` whenever a request is waiting for
    /// [`Self::poll`].
    ///
    /// # Errors
    ///
    /// The address cannot be bound.
    pub fn start(bind: SocketAddr, wake: Arc<dyn Fn() + Send + Sync>) -> io::Result<Self> {
        let listener = TcpListener::bind(bind)?;
        let endpoint =
            Endpoint { addr: listener.local_addr()?, token: new_token()?, pid: std::process::id() };
        let (sender, requests) = mpsc::channel();
        let token = endpoint.token.clone();
        std::thread::Builder::new().name("rustnative-inspect".into()).spawn(move || {
            let connections = Arc::new(AtomicUsize::new(0));
            while let Some(mut stream) = accept(&listener) {
                if connections.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    connections.fetch_sub(1, Ordering::SeqCst);
                    let busy = Reply::Error("too many inspection connections".into());
                    let _ = writeln!(stream, "{}", encode(&busy));
                    continue;
                }
                let slot = Slot(Arc::clone(&connections));
                let (sender, wake, token) = (sender.clone(), Arc::clone(&wake), token.clone());
                let _ = std::thread::Builder::new()
                    .name("rustnative-inspect-connection".into())
                    .spawn(move || {
                        let _slot = slot;
                        serve(stream, &sender, wake.as_ref(), &token);
                    });
            }
        })?;
        Ok(Self { endpoint, requests, file: None })
    }

    /// Where it listens.
    #[must_use]
    pub const fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Writes the endpoint where `rustnative inspect` finds it:
    /// `<temp>/rustnative-inspect/<pid>.json`, removed when the server
    /// stops. Returns the file's path.
    ///
    /// # Errors
    ///
    /// The file cannot be written.
    pub fn publish(&mut self) -> io::Result<PathBuf> {
        let directory = endpoint_directory();
        std::fs::create_dir_all(&directory)?;
        let path = directory.join(format!("{}.json", self.endpoint.pid));
        std::fs::write(&path, serde_json::to_vec(&self.endpoint).map_err(io::Error::other)?)?;
        self.file = Some(path.clone());
        Ok(path)
    }

    /// Answers every waiting request with `answer`. Returns how many.
    pub fn poll(&self, mut answer: impl FnMut(&Request) -> Reply) -> usize {
        let mut answered = 0;
        while let Ok(pending) = self.requests.try_recv() {
            let _ = pending.reply.send(answer(&pending.request));
            answered += 1;
        }
        answered
    }
}

impl Drop for InspectServer {
    fn drop(&mut self) {
        if let Some(file) = self.file.take() {
            let _ = std::fs::remove_file(file);
        }
    }
}

/// Where servers publish their endpoints.
#[must_use]
pub fn endpoint_directory() -> PathBuf {
    std::env::temp_dir().join("rustnative-inspect")
}

/// The next connection; `None` once the listener itself is unusable. An
/// error a passing shortage caused (out of file descriptors) is waited out
/// with a short backoff rather than spun on.
fn accept(listener: &TcpListener) -> Option<TcpStream> {
    let mut backoff = Duration::from_millis(10);
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Some(stream),
            Err(error) if error.kind() == io::ErrorKind::InvalidInput => return None,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionAborted
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(_) => {
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_millis(100));
            }
        }
    }
}

/// Gives a connection's slot back when its thread ends.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// One line of at most [`MAX_LINE`] bytes, without its line ending:
/// `Ok(None)` at the end of the stream, an error for a longer line.
fn read_line(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut line = Vec::new();
    let limit = u64::try_from(MAX_LINE).unwrap_or(u64::MAX).saturating_add(1);
    reader.take(limit).read_until(b'\n', &mut line)?;
    if line.is_empty() {
        return Ok(None);
    }
    if line.last() == Some(&b'\n') {
        line.pop();
    } else if line.len() > MAX_LINE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "line too long"));
    }
    String::from_utf8(line)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn serve(
    stream: TcpStream,
    sender: &Sender<Pending>,
    wake: &(dyn Fn() + Send + Sync),
    token: &str,
) {
    let Ok(mut writer) = stream.try_clone() else { return };
    // Until the token arrives, every line has a deadline.
    if stream.set_read_timeout(Some(AUTH_TIMEOUT)).is_err() {
        return;
    }
    let mut authenticated = false;
    let mut reader = BufReader::new(stream);
    loop {
        let line = match read_line(&mut reader) {
            Ok(Some(line)) => line,
            Ok(None) => return,
            Err(error) => {
                if error.kind() == io::ErrorKind::InvalidData {
                    let refused = Reply::Error(format!("not a request: {error}"));
                    let _ = writeln!(writer, "{}", encode(&refused));
                }
                return;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Envelope>(&line) {
            Err(error) => Reply::Error(format!("not a request: {error}")),
            Ok(envelope) if !same(&envelope.token, token) => {
                // Nothing further is read from a client without the token.
                let _ = writeln!(writer, "{}", encode(&Reply::Error("wrong token".into())));
                return;
            }
            Ok(envelope) => {
                // With the token, a client may stay quiet as long as it likes.
                if !authenticated {
                    if reader.get_ref().set_read_timeout(None).is_err() {
                        return;
                    }
                    authenticated = true;
                }
                if envelope.version == PROTOCOL_VERSION {
                    let (reply, answer) = mpsc::channel();
                    if sender.send(Pending { request: envelope.request, reply }).is_err() {
                        return;
                    }
                    wake();
                    answer.recv_timeout(ANSWER_TIMEOUT).unwrap_or_else(|_| {
                        Reply::Error(
                            "the application did not answer (its UI thread is busy)".into(),
                        )
                    })
                } else {
                    Reply::Error(format!(
                        "this application speaks inspection protocol {PROTOCOL_VERSION}, not {}",
                        envelope.version
                    ))
                }
            }
        };
        if writeln!(writer, "{}", encode(&reply)).is_err() {
            return;
        }
    }
}

/// Compares without stopping at the first difference.
fn same(given: &str, expected: &str) -> bool {
    given.len() == expected.len()
        && given.bytes().zip(expected.bytes()).fold(0, |acc, (a, b)| acc | (a ^ b)) == 0
}

fn encode(reply: &Reply) -> String {
    serde_json::to_string(reply).unwrap_or_else(|_| r#"{"error":"unencodable reply"}"#.into())
}

/// Sends one request to `endpoint` and waits for the reply — what the
/// `rustnative inspect` client does.
///
/// # Errors
///
/// The server cannot be reached, or its reply cannot be read.
pub fn request(endpoint: &Endpoint, request: &Request) -> io::Result<Reply> {
    let mut stream = TcpStream::connect(endpoint.addr)?;
    stream.set_read_timeout(Some(ANSWER_TIMEOUT + Duration::from_secs(5)))?;
    let envelope = Envelope {
        token: endpoint.token.clone(),
        version: PROTOCOL_VERSION,
        request: request.clone(),
    };
    writeln!(stream, "{}", serde_json::to_string(&envelope).map_err(io::Error::other)?)?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    serde_json::from_str(&line).map_err(io::Error::other)
}
