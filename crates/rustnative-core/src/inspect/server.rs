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
//! Connections are served on their own threads; requests are answered on
//! the UI thread, which the server wakes and which drains them in
//! [`InspectServer::poll`] — the application is never touched off its
//! thread.

use std::io::{self, BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use super::{PROTOCOL_VERSION, Reply, Request};
use crate::secret;

/// How long a connection waits for the UI thread to answer.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

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
            for stream in listener.incoming().flatten() {
                let (sender, wake, token) = (sender.clone(), Arc::clone(&wake), token.clone());
                let _ = std::thread::Builder::new()
                    .name("rustnative-inspect-connection".into())
                    .spawn(move || serve(stream, &sender, wake.as_ref(), &token));
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
    /// `<endpoint_directory>/<pid>.json`, removed when the server stops.
    /// The file holds the token, so the directory is created readable by
    /// this user alone (and refused if it is not), and the file is created
    /// afresh with mode `0600`, never following a link planted at its name.
    /// Returns the file's path.
    ///
    /// # Errors
    ///
    /// The directory is not private, or the file cannot be written.
    pub fn publish(&mut self) -> io::Result<PathBuf> {
        let directory = endpoint_directory();
        secret::private_directory(&directory)?;
        let path = directory.join(format!("{}.json", self.endpoint.pid));
        let contents = serde_json::to_vec(&self.endpoint).map_err(io::Error::other)?;
        let mut file = match secret::create_private_file(&path) {
            // Left by an earlier process with this id. The directory is
            // this user's alone, so so is the file; removing it unlinks
            // the name without following it.
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                std::fs::remove_file(&path)?;
                secret::create_private_file(&path)?
            }
            created => created?,
        };
        file.write_all(&contents)?;
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

/// Where servers publish their endpoints: `rustnative-inspect` in this
/// user's own directory ([`secret::user_directory`]) — never the shared
/// temporary directory, where another local user could read the token.
#[must_use]
pub fn endpoint_directory() -> PathBuf {
    secret::user_directory("rustnative-inspect")
}

/// Every endpoint published in [`endpoint_directory`], with when each was
/// written. Nothing is read from a directory that is not private to this
/// user.
///
/// # Errors
///
/// The directory exists but is not private, or cannot be listed.
pub fn published_endpoints() -> io::Result<Vec<(SystemTime, Endpoint)>> {
    let directory = endpoint_directory();
    match secret::check_private_directory(&directory) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        checked => checked?,
    }
    let read = |path: &Path| -> Option<(SystemTime, Endpoint)> {
        let modified = std::fs::metadata(path).and_then(|meta| meta.modified()).ok()?;
        let endpoint = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        Some((modified, endpoint))
    };
    Ok(std::fs::read_dir(&directory)?.flatten().filter_map(|entry| read(&entry.path())).collect())
}

/// The endpoint process `pid` published, if it has (completely) yet.
///
/// # Errors
///
/// The directory exists but is not private.
pub fn published_endpoint(pid: u32) -> io::Result<Option<Endpoint>> {
    let directory = endpoint_directory();
    match secret::check_private_directory(&directory) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        checked => checked?,
    }
    let file = directory.join(format!("{pid}.json"));
    Ok(std::fs::read(file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()))
}

fn serve(
    stream: TcpStream,
    sender: &Sender<Pending>,
    wake: &(dyn Fn() + Send + Sync),
    token: &str,
) {
    let Ok(mut writer) = stream.try_clone() else { return };
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { return };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Envelope>(&line) {
            Err(error) => Reply::Error(format!("not a request: {error}")),
            Ok(envelope)
                if !secret::constant_time_eq(envelope.token.as_bytes(), token.as_bytes()) =>
            {
                // Nothing further is read from a client without the token.
                let _ = writeln!(writer, "{}", encode(&Reply::Error("wrong token".into())));
                return;
            }
            Ok(envelope) if envelope.version != PROTOCOL_VERSION => Reply::Error(format!(
                "this application speaks inspection protocol {PROTOCOL_VERSION}, not {}",
                envelope.version
            )),
            Ok(envelope) => {
                let (reply, answer) = mpsc::channel();
                if sender.send(Pending { request: envelope.request, reply }).is_err() {
                    return;
                }
                wake();
                answer.recv_timeout(ANSWER_TIMEOUT).unwrap_or_else(|_| {
                    Reply::Error("the application did not answer (its UI thread is busy)".into())
                })
            }
        };
        if writeln!(writer, "{}", encode(&reply)).is_err() {
            return;
        }
    }
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

#[cfg(all(test, feature = "os-random"))]
mod tests {
    use super::*;

    #[test]
    fn the_published_endpoint_is_readable_by_its_user_alone() {
        let mut server = InspectServer::start(([127, 0, 0, 1], 0).into(), Arc::new(|| {})).unwrap();
        let path = server.publish().unwrap();
        assert_eq!(path.parent(), Some(endpoint_directory().as_path()));
        assert_eq!(
            published_endpoint(std::process::id()).unwrap().as_ref(),
            Some(server.endpoint())
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&path), 0o600, "the token file");
            assert_eq!(mode(&endpoint_directory()), 0o700, "its directory");
        }
        drop(server);
        assert!(!path.exists(), "removed when the server stops");
    }
}
