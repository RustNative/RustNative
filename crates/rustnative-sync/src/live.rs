//! Server-interactive mode: a component tree per connection on the
//! server, its trees sent to the client, the client's events sent back.
//!
//! - The server ([`LiveServer`]) runs each session's `ComponentTree` on its
//!   own thread (trees are not `Send`), renders after every event and task,
//!   and sends the tree as a [`WireNode`] whenever it changed.
//! - The client ([`LiveClient`] and the [`RemoteView`] component) shows the
//!   latest tree through the ordinary reconciler and sends events back.
//!   Typing is echoed locally at once (the optimistic hook) and replaced by
//!   the server's tree when it arrives.
//! - A dropped connection reconnects with its session id: within the grace
//!   period the server still holds the tree, and nothing is lost. After
//!   it, or on another instance, the client offers the state snapshot it
//!   last received, and the new session starts from it.
//! - [`LiveServer::drain`] (a deploy) sends every client its state and
//!   where to reconnect; the clients move to the new instance without
//!   losing state.
//!
//! The transport is a WebSocket carrying JSON frames.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use rustnative_core::wire::{WireNode, node_id};
use rustnative_core::{Component, ComponentTree, Event, Node};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc as async_mpsc, oneshot, watch};
use tokio_tungstenite::tungstenite::Message;

/// An event, as it travels from the client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum WireEvent {
    /// A click.
    Click {
        /// The node's wire key.
        target: String,
    },
    /// New text.
    TextChanged {
        /// The node's wire key.
        target: String,
        /// The whole text.
        value: String,
    },
    /// A checkbox or toggle.
    Toggled {
        /// The node's wire key.
        target: String,
        /// Its new state.
        on: bool,
    },
    /// A slider or spinner.
    ValueChanged {
        /// The node's wire key.
        target: String,
        /// Its new value.
        value: i64,
    },
}

impl WireEvent {
    /// The wire form of a client event, for the events the mode carries.
    #[must_use]
    pub fn from_event(event: &Event) -> Option<Self> {
        let key = |id: rustnative_core::NodeId| rustnative_core::wire::key_of(id);
        Some(match event {
            Event::Click { target } => Self::Click { target: key(*target) },
            Event::TextChanged { target, value } => {
                Self::TextChanged { target: key(*target), value: value.clone() }
            }
            Event::Toggled { target, on } => Self::Toggled { target: key(*target), on: *on },
            Event::ValueChanged { target, value } => {
                Self::ValueChanged { target: key(*target), value: *value }
            }
            _ => return None,
        })
    }

    /// The server-side event.
    #[must_use]
    pub fn into_event(self) -> Event {
        match self {
            Self::Click { target } => Event::Click { target: node_id(&target) },
            Self::TextChanged { target, value } => {
                Event::TextChanged { target: node_id(&target), value }
            }
            Self::Toggled { target, on } => Event::Toggled { target: node_id(&target), on },
            Self::ValueChanged { target, value } => {
                Event::ValueChanged { target: node_id(&target), value }
            }
        }
    }
}

/// What the client sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "frame", rename_all = "snake_case")]
pub enum ClientFrame {
    /// The first frame: the session to resume, and the state to start
    /// from if the server no longer has it.
    Hello {
        /// The session to resume.
        session: Option<String>,
        /// The last state snapshot the client received.
        snapshot: Option<Value>,
        /// A browser's options (its element-id scope and layout flow): the
        /// client wants rendered elements ([`ServerFrame::Dom`]) rather
        /// than trees, encoded by the server's browser encoding
        /// ([`LiveServer::set_browser_encoding`]).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dom: Option<Value>,
    },
    /// An event, numbered so the server can acknowledge it.
    Event {
        /// The event.
        event: WireEvent,
        /// Its number.
        sequence: u64,
    },
}

/// What the server sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "frame", rename_all = "snake_case")]
pub enum ServerFrame {
    /// The session this connection is.
    Welcome {
        /// Its id.
        session: String,
    },
    /// The tree, with the last event it reflects and the state snapshot.
    Tree {
        /// The tree.
        tree: Box<WireNode>,
        /// The last event applied.
        acknowledged: u64,
        /// The root component's state.
        snapshot: Option<Value>,
    },
    /// The tree as a browser patches it: its elements and the rules they
    /// need, from the server's browser encoding.
    Dom {
        /// The root element.
        element: Value,
        /// The `(class, rule)` pairs its classes need.
        rules: Value,
        /// The last event applied.
        acknowledged: u64,
        /// The root component's state.
        snapshot: Option<Value>,
    },
    /// This instance is going away: reconnect to `to` after a pause.
    Drain {
        /// Where to reconnect (empty: the same address).
        to: String,
        /// How long to wait first.
        after_ms: u64,
        /// The state to resume with.
        snapshot: Option<Value>,
        /// The last event the state reflects.
        acknowledged: u64,
    },
}

/// A server-interactive application: how to build the root component,
/// from a state snapshot when one is given (see
/// [`rustnative_core::Component::inspect`], which produces it).
pub trait LiveApp: Send + Sync + 'static {
    /// The root component.
    type Root: Component;
    /// The root, started from `snapshot` when there is one.
    fn root(&self, snapshot: Option<Value>) -> Self::Root;
}

#[cfg(feature = "server")]
impl<A: LiveApp> LiveServer<A> {
    /// Serves this server's sessions on `app`'s own listener at `path` (a
    /// WebSocket upgrade), for pages whose live subtrees connect back to the
    /// origin that served them. A connection from a page of another origin
    /// is refused: a WebSocket carries the person's cookies, and only the
    /// application's own pages may use them.
    #[must_use]
    pub fn mount(
        self: &Arc<Self>,
        app: rustnative_server::ServerApp,
        path: &str,
    ) -> rustnative_server::ServerApp {
        use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
        use tokio_tungstenite::tungstenite::protocol::Role;

        let server = Arc::clone(self);
        app.upgrade(
            path,
            Arc::new(move |mut request: http::Request<hyper::body::Incoming>, _peer| {
                let refuse = |status: u16| {
                    let mut response =
                        http::Response::new(rustnative_server::Body::from(bytes::Bytes::new()));
                    *response.status_mut() =
                        http::StatusCode::from_u16(status).unwrap_or(http::StatusCode::BAD_REQUEST);
                    response
                };
                let header = |name: &str| {
                    request
                        .headers()
                        .get(name)
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_owned)
                };
                let host = header("host").unwrap_or_default();
                if let Some(origin) = header("origin") {
                    let origin_host =
                        origin.split_once("://").map_or(origin.as_str(), |(_, rest)| rest);
                    if origin_host != host {
                        return refuse(403);
                    }
                }
                let Some(key) = header("sec-websocket-key") else { return refuse(400) };
                if !header("upgrade")
                    .is_some_and(|upgrade| upgrade.eq_ignore_ascii_case("websocket"))
                {
                    return refuse(400);
                }
                let accept = derive_accept_key(key.as_bytes());
                let upgrading = hyper::upgrade::on(&mut request);
                let server = Arc::clone(&server);
                tokio::spawn(async move {
                    if let Ok(upgraded) = upgrading.await {
                        let io = hyper_util::rt::TokioIo::new(upgraded);
                        let socket = tokio_tungstenite::WebSocketStream::from_raw_socket(
                            io,
                            Role::Server,
                            None,
                        )
                        .await;
                        server.accept(socket).await;
                    }
                });
                let mut response = refuse(101);
                let headers = response.headers_mut();
                headers.insert(http::header::CONNECTION, http::HeaderValue::from_static("upgrade"));
                headers.insert(http::header::UPGRADE, http::HeaderValue::from_static("websocket"));
                if let Ok(value) = http::HeaderValue::from_str(&accept) {
                    headers.insert(http::header::SEC_WEBSOCKET_ACCEPT, value);
                }
                response
            }),
        )
    }
}
/// How a browser client's trees are encoded: the view and the options the
/// browser sent in its hello, to an object with `element` and `rules`.
pub type BrowserEncoding = Arc<dyn Fn(&Node, &Value) -> Value + Send + Sync>;

enum Command {
    Event(WireEvent, u64),
    Attach(async_mpsc::UnboundedSender<ServerFrame>, Option<(BrowserEncoding, Value)>),
    Detach,
    Snapshot(oneshot::Sender<Option<Value>>),
    Drain(String, u64),
    Stop,
}

struct SessionHandle {
    commands: mpsc::Sender<Command>,
    detached_at: Option<Instant>,
}

/// The server side of server-interactive mode.
pub struct LiveServer<A: LiveApp> {
    app: Arc<A>,
    sessions: Mutex<HashMap<String, SessionHandle>>,
    grace: Duration,
    draining: AtomicBool,
    drain_to: Mutex<String>,
    next: AtomicU64,
    instance: u64,
    encoding: Mutex<Option<BrowserEncoding>>,
}

fn root_snapshot(tree: &ComponentTree) -> Option<Value> {
    tree.inspect_components()
        .into_iter()
        .find(|component| component.parent.is_none())
        .and_then(|root| root.state)
}

/// One session's thread: owns the tree, applies events, pumps tasks, and
/// sends the tree whenever it changed.
fn run_session<A: LiveApp>(app: &A, snapshot: Option<Value>, commands: &mpsc::Receiver<Command>) {
    let mut tree = ComponentTree::new(app.root(snapshot));
    let _ = tree.render();
    let mut outbound: Option<async_mpsc::UnboundedSender<ServerFrame>> = None;
    let mut browser: Option<(BrowserEncoding, Value)> = None;
    let mut acknowledged = 0;
    let mut last: Option<Node> = None;
    loop {
        let command = commands.recv_timeout(Duration::from_millis(25));
        match command {
            Ok(Command::Event(event, sequence)) => {
                // Events are resent after a reconnect; each is applied once.
                if sequence > acknowledged {
                    tree.dispatch(event.into_event());
                    acknowledged = sequence;
                }
            }
            Ok(Command::Attach(sender, encoding)) => {
                outbound = Some(sender);
                browser = encoding;
                last = None;
            }
            Ok(Command::Detach) => outbound = None,
            Ok(Command::Snapshot(reply)) => {
                let _ = reply.send(root_snapshot(&tree));
            }
            Ok(Command::Drain(to, after_ms)) => {
                if let Some(sender) = &outbound {
                    let _ = sender.send(ServerFrame::Drain {
                        to,
                        after_ms,
                        snapshot: root_snapshot(&tree),
                        acknowledged,
                    });
                }
                return;
            }
            Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        tree.pump_tasks();
        let _ = tree.render();
        // A browser resolves declarations itself: it gets the unresolved
        // view.
        let view = if browser.is_some() { tree.unresolved_view() } else { tree.view() };
        if last.as_ref() != Some(&view) {
            if let Some(sender) = &outbound {
                let frame = match &browser {
                    Some((encode, options)) => {
                        let mut encoded = encode(&view, options);
                        ServerFrame::Dom {
                            element: encoded
                                .get_mut("element")
                                .map(Value::take)
                                .unwrap_or_default(),
                            rules: encoded.get_mut("rules").map(Value::take).unwrap_or_default(),
                            acknowledged,
                            snapshot: root_snapshot(&tree),
                        }
                    }
                    None => ServerFrame::Tree {
                        tree: Box::new(WireNode::from_node(&view)),
                        acknowledged,
                        snapshot: root_snapshot(&tree),
                    },
                };
                if sender.send(frame).is_err() {
                    outbound = None;
                }
            }
            if outbound.is_some() {
                last = Some(view);
            }
        }
    }
}

impl<A: LiveApp> LiveServer<A> {
    /// A server for `app`, keeping a disconnected session's tree for
    /// `grace`.
    #[must_use]
    pub fn new(app: A, grace: Duration) -> Arc<Self> {
        Arc::new(Self {
            app: Arc::new(app),
            sessions: Mutex::new(HashMap::new()),
            grace,
            draining: AtomicBool::new(false),
            drain_to: Mutex::new(String::new()),
            next: AtomicU64::new(1),
            encoding: Mutex::new(None),
            // A per-instance random part, so two instances (even in one
            // process) never mint the same session id.
            instance: {
                use std::hash::{BuildHasher, Hasher};
                std::collections::hash_map::RandomState::new().build_hasher().finish()
            },
        })
    }

    fn start_session(&self, snapshot: Option<Value>) -> (String, mpsc::Sender<Command>) {
        let id = format!("s{:x}-{}", self.instance, self.next.fetch_add(1, Ordering::SeqCst));
        let (sender, receiver) = mpsc::channel();
        let app = Arc::clone(&self.app);
        std::thread::spawn(move || run_session(&*app, snapshot, &receiver));
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id.clone(), SessionHandle { commands: sender.clone(), detached_at: None });
        (id, sender)
    }

    /// Stops sessions detached longer than the grace period.
    fn reap(&self) {
        let grace = self.grace;
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner).retain(|_, handle| {
            let expired = handle.detached_at.is_some_and(|at| at.elapsed() > grace);
            if expired {
                let _ = handle.commands.send(Command::Stop);
            }
            !expired
        });
    }

    /// How many sessions the server holds.
    #[must_use]
    pub fn sessions(&self) -> usize {
        self.reap();
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    /// How browsers get the tree ([`ServerFrame::Dom`]): the web
    /// backend's element encoding (`rustnative_web::live::encode`).
    pub fn set_browser_encoding(&self, encoding: BrowserEncoding) {
        *self.encoding.lock().unwrap_or_else(PoisonError::into_inner) = Some(encoding);
    }

    /// Serves connections on `listener` until the task is dropped.
    pub async fn serve(self: Arc<Self>, listener: tokio::net::TcpListener) {
        while let Ok((stream, _)) = listener.accept().await {
            let server = Arc::clone(&self);
            tokio::spawn(async move {
                if let Ok(socket) = tokio_tungstenite::accept_async(stream).await {
                    server.accept(socket).await;
                }
            });
        }
    }

    /// Runs one connection that has already been upgraded to a WebSocket
    /// (by this server's listener, or by an application's own, as
    /// `rustnative-server` does), until it closes.
    pub async fn accept<S>(&self, socket: tokio_tungstenite::WebSocketStream<S>)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let (mut sink, mut stream) = socket.split();
        let hello = loop {
            match stream.next().await {
                Some(Ok(Message::Text(text))) => match serde_json::from_str::<ClientFrame>(&text) {
                    Ok(ClientFrame::Hello { session, snapshot, dom }) => {
                        break (session, snapshot, dom);
                    }
                    _ => return,
                },
                Some(Ok(_)) => {}
                _ => return,
            }
        };
        self.reap();
        let send =
            |frame: &ServerFrame| Message::Text(serde_json::to_string(frame).unwrap_or_default());
        if self.draining.load(Ordering::SeqCst) {
            let to = self.drain_to.lock().unwrap_or_else(PoisonError::into_inner).clone();
            let _ = sink
                .send(send(&ServerFrame::Drain {
                    to,
                    after_ms: 0,
                    snapshot: hello.1,
                    acknowledged: 0,
                }))
                .await;
            return;
        }
        let existing = hello.0.as_ref().and_then(|id| {
            let sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
            sessions.get(id).map(|handle| (id.clone(), handle.commands.clone()))
        });
        let (id, commands) = existing.unwrap_or_else(|| self.start_session(hello.1));
        let (outbound, mut frames) = async_mpsc::unbounded_channel();
        if let Some(handle) =
            self.sessions.lock().unwrap_or_else(PoisonError::into_inner).get_mut(&id)
        {
            handle.detached_at = None;
        }
        let encoding = hello.2.and_then(|options| {
            let encoding = self.encoding.lock().unwrap_or_else(PoisonError::into_inner).clone()?;
            Some((encoding, options))
        });
        let _ = commands.send(Command::Attach(outbound, encoding));
        if sink.send(send(&ServerFrame::Welcome { session: id.clone() })).await.is_err() {
            return;
        }
        loop {
            tokio::select! {
                frame = frames.recv() => {
                    let Some(frame) = frame else { break };
                    if sink.send(send(&frame)).await.is_err() {
                        break;
                    }
                }
                message = stream.next() => {
                    match message {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(ClientFrame::Event { event, sequence }) = serde_json::from_str(&text) {
                                let _ = commands.send(Command::Event(event, sequence));
                            }
                        }
                        Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                        Some(Ok(_)) => {}
                    }
                }
            }
        }
        let _ = commands.send(Command::Detach);
        if let Some(handle) =
            self.sessions.lock().unwrap_or_else(PoisonError::into_inner).get_mut(&id)
        {
            handle.detached_at = Some(Instant::now());
        }
    }

    /// Drops every connection (as a network failure would); the sessions
    /// stay for the grace period.
    pub fn drop_connections(&self) {
        for handle in self.sessions.lock().unwrap_or_else(PoisonError::into_inner).values() {
            let _ = handle.commands.send(Command::Detach);
        }
    }

    /// Drains the instance for a deploy: every session is sent its state
    /// and told to reconnect to `to` after `after`, and stops; new
    /// connections are sent there too.
    pub fn drain(&self, to: &str, after: Duration) {
        self.draining.store(true, Ordering::SeqCst);
        to.clone_into(&mut self.drain_to.lock().unwrap_or_else(PoisonError::into_inner));
        let after_ms = u64::try_from(after.as_millis()).unwrap_or(u64::MAX);
        for (_, handle) in self.sessions.lock().unwrap_or_else(PoisonError::into_inner).drain() {
            let _ = handle.commands.send(Command::Drain(to.to_owned(), after_ms));
        }
    }

    /// The state of session `id`, as a snapshot.
    pub async fn snapshot(&self, id: &str) -> Option<Value> {
        let commands =
            self.sessions.lock().unwrap_or_else(PoisonError::into_inner).get(id)?.commands.clone();
        let (reply, answer) = oneshot::channel();
        commands.send(Command::Snapshot(reply)).ok()?;
        answer.await.ok().flatten()
    }
}

/// The client side: one connection, kept up.
#[derive(Clone)]
pub struct LiveClient {
    inner: Arc<ClientInner>,
}

struct ClientInner {
    outbound: async_mpsc::UnboundedSender<ClientFrame>,
    state: Mutex<ClientState>,
    changed: watch::Sender<u64>,
    sequence: AtomicU64,
}

#[derive(Default)]
struct ClientState {
    tree: Option<WireNode>,
    acknowledged: u64,
    session: Option<String>,
    snapshot: Option<Value>,
    echoes: BTreeMap<String, (String, u64)>,
    connected: bool,
    /// Events sent but not yet reflected in a tree: resent after a
    /// reconnect (the server applies each once).
    unacknowledged: Vec<ClientFrame>,
}

impl ClientState {
    fn acknowledge(&mut self, acknowledged: u64) {
        self.acknowledged = self.acknowledged.max(acknowledged);
        let done = self.acknowledged;
        self.echoes.retain(|_, (_, sequence)| *sequence > done);
        self.unacknowledged.retain(
            |frame| !matches!(frame, ClientFrame::Event { sequence, .. } if *sequence <= done),
        );
    }
}

impl LiveClient {
    /// Connects to `url` (`ws://host:port`) and keeps the connection up,
    /// on the current tokio runtime.
    #[must_use]
    pub fn connect(url: &str) -> Self {
        let (outbound, receiver) = async_mpsc::unbounded_channel();
        let inner = Arc::new(ClientInner {
            outbound,
            state: Mutex::new(ClientState::default()),
            changed: watch::channel(0).0,
            sequence: AtomicU64::new(0),
        });
        tokio::spawn(Self::maintain(Arc::clone(&inner), url.to_owned(), receiver));
        Self { inner }
    }

    async fn maintain(
        inner: Arc<ClientInner>,
        mut url: String,
        mut outbound: async_mpsc::UnboundedReceiver<ClientFrame>,
    ) {
        loop {
            let Ok((socket, _)) = tokio_tungstenite::connect_async(url.as_str()).await else {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            };
            let (mut sink, mut stream) = socket.split();
            let (session, snapshot) = {
                let state = inner.state.lock().unwrap_or_else(PoisonError::into_inner);
                (state.session.clone(), state.snapshot.clone())
            };
            let hello = ClientFrame::Hello { session, snapshot, dom: None };
            if sink
                .send(Message::Text(serde_json::to_string(&hello).unwrap_or_default()))
                .await
                .is_err()
            {
                continue;
            }
            // Events not yet reflected by the server go (again) first.
            let resend =
                inner.state.lock().unwrap_or_else(PoisonError::into_inner).unacknowledged.clone();
            for frame in resend {
                let _ = sink
                    .send(Message::Text(serde_json::to_string(&frame).unwrap_or_default()))
                    .await;
            }
            let mut redirect = None;
            loop {
                tokio::select! {
                    frame = outbound.recv() => {
                        let Some(frame) = frame else { return };
                        if sink.send(Message::Text(serde_json::to_string(&frame).unwrap_or_default())).await.is_err() {
                            break;
                        }
                    }
                    message = stream.next() => {
                        let Some(Ok(Message::Text(text))) = message else {
                            if matches!(message, Some(Ok(_))) { continue; }
                            break;
                        };
                        match serde_json::from_str::<ServerFrame>(&text) {
                            Ok(ServerFrame::Welcome { session }) => {
                                let mut state = inner.state.lock().unwrap_or_else(PoisonError::into_inner);
                                state.session = Some(session);
                                state.connected = true;
                                drop(state);
                                inner.changed.send_modify(|version| *version += 1);
                            }
                            Ok(ServerFrame::Tree { tree, acknowledged, snapshot }) => {
                                {
                                    let mut state = inner.state.lock().unwrap_or_else(PoisonError::into_inner);
                                    state.tree = Some(*tree);
                                    state.snapshot = snapshot;
                                    state.acknowledge(acknowledged);
                                }
                                inner.changed.send_modify(|version| *version += 1);
                            }
                            Ok(ServerFrame::Drain { to, after_ms, snapshot, acknowledged }) => {
                                {
                                    let mut state = inner.state.lock().unwrap_or_else(PoisonError::into_inner);
                                    if snapshot.is_some() {
                                        state.snapshot = snapshot;
                                    }
                                    state.session = None;
                                    // The snapshot includes these; the new
                                    // instance must not apply them again.
                                    state.acknowledge(acknowledged);
                                    let done = state.acknowledged;
                                    state.unacknowledged.retain(|frame| !matches!(frame, ClientFrame::Event { sequence, .. } if *sequence <= done));
                                }
                                tokio::time::sleep(Duration::from_millis(after_ms)).await;
                                redirect = (!to.is_empty()).then_some(to);
                                break;
                            }
                            // This client never asks for elements.
                            Ok(ServerFrame::Dom { .. }) | Err(_) => {}
                        }
                    }
                }
            }
            inner.state.lock().unwrap_or_else(PoisonError::into_inner).connected = false;
            inner.changed.send_modify(|version| *version += 1);
            if let Some(to) = redirect {
                url = to;
            } else {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }

    /// Sends a client event; typing is echoed locally at once.
    pub fn send(&self, event: &Event) {
        let Some(wire) = WireEvent::from_event(event) else { return };
        let sequence = self.inner.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        if let WireEvent::TextChanged { target, value } = &wire {
            self.inner
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .echoes
                .insert(target.clone(), (value.clone(), sequence));
            self.inner.changed.send_modify(|version| *version += 1);
        }
        let frame = ClientFrame::Event { event: wire, sequence };
        self.inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .unacknowledged
            .push(frame.clone());
        let _ = self.inner.outbound.send(frame);
    }

    /// The latest tree, with local echoes applied.
    #[must_use]
    pub fn view(&self) -> Option<Node> {
        let state = self.inner.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut tree = state.tree.clone()?;
        for (key, (value, _)) in &state.echoes {
            echo(&mut tree, key, value);
        }
        Some(tree.into_node())
    }

    /// Whether the connection is up.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.inner.state.lock().unwrap_or_else(PoisonError::into_inner).connected
    }

    /// The session's id.
    #[must_use]
    pub fn session(&self) -> Option<String> {
        self.inner.state.lock().unwrap_or_else(PoisonError::into_inner).session.clone()
    }

    /// Waits until something changes.
    pub async fn changed(&self) {
        let mut receiver = self.inner.changed.subscribe();
        let _ = receiver.changed().await;
    }
}

fn echo(tree: &mut WireNode, key: &str, value: &str) {
    use rustnative_core::wire::WireKind;
    if tree.key == key {
        if let WireKind::TextInput { value: current } = &mut tree.content {
            value.clone_into(current);
        }
        return;
    }
    if let WireKind::Column { children, .. } | WireKind::Row { children, .. } = &mut tree.content {
        for child in children {
            echo(child, key, value);
        }
    }
}

/// The client component showing a live tree: its view is the server's
/// tree, reconciled like any other; its events go to the server.
pub struct RemoteView {
    client: LiveClient,
}

/// A remote tree arrived.
#[derive(Debug, Clone, Copy)]
pub struct Changed;

impl Component for RemoteView {
    type Props = LiveClient;
    type Message = Changed;
    fn new(client: LiveClient) -> Self {
        Self { client }
    }
    fn props(&self) -> &LiveClient {
        &self.client
    }
    fn set_props(&mut self, client: LiveClient) {
        self.client = client;
    }
    fn view(&self) -> Node {
        self.client.view().unwrap_or_else(|| Node::label("connecting", "Connecting…"))
    }
    fn update(&mut self, event: Event) {
        self.client.send(&event);
    }
    fn render(&mut self, context: &mut rustnative_core::ComponentContext<'_, Changed>) -> Node {
        let client = self.client.clone();
        context.spawn(async move {
            client.changed().await;
            Changed
        });
        self.view()
    }
}

impl PartialEq for LiveClient {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl LiveClient {
    /// The root component's state, as last received.
    #[must_use]
    pub fn snapshot(&self) -> Option<Value> {
        self.inner.state.lock().unwrap_or_else(PoisonError::into_inner).snapshot.clone()
    }
}

/// How a subtree is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    /// Rendered once on the server; no interaction.
    Static,
    /// Held on the server; events go there.
    ServerInteractive,
    /// Run on the client.
    ClientInteractive,
    /// Server-interactive until the client has the component, then
    /// client-interactive, carrying the state across.
    Auto,
}

type Mount = Arc<
    dyn Fn(&mut rustnative_core::ComponentContext<'_, Changed>, Option<Value>) -> Node
        + Send
        + Sync,
>;

/// The components the client has, by name — the client's module.
#[derive(Clone, Default)]
pub struct ClientModules {
    mounts: Arc<Mutex<BTreeMap<String, Mount>>>,
}

impl ClientModules {
    /// No components yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes component `C` available as `name`, its props built from the
    /// state snapshot it takes over (none: a fresh start).
    pub fn register<C: Component>(
        &self,
        name: &str,
        props: impl Fn(Option<Value>) -> C::Props + Send + Sync + 'static,
    ) where
        C::Props: Clone,
    {
        let key = format!("local-{name}");
        let mount: Mount = Arc::new(move |context, snapshot| {
            let props = props(snapshot);
            context.child_with_props::<C, _>(&key, props, C::new)
        });
        self.mounts.lock().unwrap_or_else(PoisonError::into_inner).insert(name.to_owned(), mount);
    }

    fn get(&self, name: &str) -> Option<Mount> {
        self.mounts.lock().unwrap_or_else(PoisonError::into_inner).get(name).cloned()
    }
}

/// A subtree in one of the [`RenderMode`]s.
#[derive(Clone)]
pub struct SubtreeProps {
    /// The component's name, on the server and in [`ClientModules`].
    pub name: String,
    /// How it renders.
    pub mode: RenderMode,
    /// The connection to its server session.
    pub client: LiveClient,
    /// The client's components.
    pub modules: ClientModules,
}

impl PartialEq for SubtreeProps {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.mode == other.mode && self.client == other.client
    }
}

/// See [`RenderMode`]. On a switch from the server to the client, the
/// client component starts from the server session's last state snapshot:
/// nothing the person did is lost.
pub struct Subtree {
    props: SubtreeProps,
    frozen: Option<Node>,
    /// Once on the client: the state it started from.
    local: Option<Carried>,
}

/// The state a subtree carried from the server (none: a fresh start).
#[derive(Debug, Clone)]
struct Carried(Option<Value>);

impl Component for Subtree {
    type Props = SubtreeProps;
    type Message = Changed;
    fn new(props: SubtreeProps) -> Self {
        Self { props, frozen: None, local: None }
    }
    fn props(&self) -> &SubtreeProps {
        &self.props
    }
    fn set_props(&mut self, props: SubtreeProps) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::column("subtree", [])
    }
    fn update(&mut self, event: Event) {
        if self.local.is_none() && self.props.mode != RenderMode::Static {
            self.props.client.send(&event);
        }
    }
    fn render(&mut self, context: &mut rustnative_core::ComponentContext<'_, Changed>) -> Node {
        let mount = match self.props.mode {
            RenderMode::ClientInteractive => {
                self.local.get_or_insert(Carried(None));
                self.props.modules.get(&self.props.name)
            }
            RenderMode::Auto => {
                let mount = self.props.modules.get(&self.props.name);
                if mount.is_some() && self.local.is_none() {
                    // The switch: take over the server session's state.
                    self.local = Some(Carried(self.props.client.snapshot()));
                }
                mount
            }
            RenderMode::Static | RenderMode::ServerInteractive => None,
        };
        if let (Some(mount), Some(snapshot)) = (mount, &self.local) {
            return Node::column("subtree", [mount(context, snapshot.0.clone())]);
        }
        let client = self.props.client.clone();
        if self.props.mode == RenderMode::Static {
            if self.frozen.is_none() {
                self.frozen = client.view();
                if self.frozen.is_none() {
                    context.spawn(async move {
                        client.changed().await;
                        Changed
                    });
                }
            }
            return self.frozen.clone().unwrap_or_else(|| Node::label("loading", "Loading…"));
        }
        context.spawn(async move {
            client.changed().await;
            Changed
        });
        self.props.client.view().unwrap_or_else(|| Node::label("connecting", "Connecting…"))
    }
}
