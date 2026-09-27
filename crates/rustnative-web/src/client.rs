//! Client components (`PLAN.md` Web milestone A): client logic, written as
//! ordinary Rust in a restricted subset, that runs in the browser as
//! JavaScript generated at compile time — and, on every other target and on
//! the server, as the Rust it is.
//!
//! A client component is a module marked `#[rustnative_web::client]`
//! holding a state type and its `update`, `view`, and optionally `message`
//! and `init`:
//!
//! ```ignore
//! #[rustnative_web::client]
//! pub mod counter {
//!     use rustnative_core::{Event, Node, NodeId, rsx};
//!     use rustnative_web::Effects;
//!
//!     #[derive(Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
//!     pub struct Counter { pub count: i32 }
//!
//!     impl Counter {
//!         pub fn update(&mut self, event: Event, _fx: &mut Effects<()>) {
//!             if let Event::Click { target } = event {
//!                 if target == NodeId::from_key("add") { self.count += 1; }
//!             }
//!         }
//!         pub fn view(&self) -> Node {
//!             rsx! {
//!                 <Column key="counter">
//!                     <Label key="count" text={format!("Count: {}", self.count)} />
//!                     <Button key="add" text="Add" />
//!                 </Column>
//!             }
//!         }
//!     }
//! }
//! // `counter::Island` is a `Component` whose props are its initial state:
//! // `<counter::Island key="c" count={3} />`.
//! ```
//!
//! The macro implements [`ClientLogic`] for the state type — its generated
//! JavaScript, the CSS its class strings need, the server functions it
//! calls — and leaves the Rust as written, except that integer arithmetic
//! becomes the checked arithmetic of [`rt`], so an overflow is the same
//! error in both languages. [`Client`] is the component that runs it.
//!
//! Effects — a server call, a navigation, a timer, the clipboard — are
//! requested through [`Effects`] rather than performed in `update`: the
//! browser's runtime carries them out there, and [`Client::render`] carries
//! them out through the tree's services everywhere else.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rustnative_core::server_fn::{ServerFn, ServerFnError};
use rustnative_core::{
    Component, ComponentContext, ComponentId, Event, HttpRequest, HttpResponse, Method, Node,
    NodeId, Services,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// A client component's compiled form: what the `#[client]` macro writes for
/// the browser.
#[derive(Debug)]
pub struct ClientModule {
    /// Its name (the module's), unique in the application.
    pub name: &'static str,
    /// Its JavaScript: an ES module whose default export takes the runtime
    /// and returns `{ update, message, init, view }`.
    pub js: &'static str,
    /// The CSS its class strings need: `(class, rule)`.
    pub css: &'static [(&'static str, &'static str)],
    /// The theme tokens those rules refer to.
    pub tokens: &'static [&'static str],
    /// The server functions it calls: `(type name, path)`.
    pub server_fns: &'static [(&'static str, &'static str)],
    /// The Rust file its logic is in, for source maps.
    pub file: &'static str,
    /// For each line of [`Self::js`], the Rust line it came from (`0`: none).
    pub lines: &'static [u32],
}

impl ClientModule {
    /// The hash its URL carries, so the URL changes when the code does and
    /// the file can be cached forever.
    #[must_use]
    pub fn hash(&self) -> String {
        crate::hash::class_name("", self.js)
    }

    /// Its URL below `base` (the runtime's asset path, `/_rn/`).
    #[must_use]
    pub fn url(&self, base: &str) -> String {
        format!("{base}m/{}.{}.js", self.name, self.hash())
    }

    /// Its source map (Source Map v3): each generated line to the line of
    /// Rust it came from, so a browser's debugger and error stacks show the
    /// client logic as written. Served beside the module in development.
    #[must_use]
    pub fn source_map(&self) -> String {
        let mut mappings = String::new();
        let mut previous_line: i64 = 0;
        for (index, line) in self.lines.iter().enumerate() {
            if index > 0 {
                mappings.push(';');
            }
            // A line with no span of its own belongs to the statement before
            // it (a statement can expand to several lines).
            let line = if *line == 0 {
                if index == 0
                    || previous_line == 0 && self.lines[..index].iter().all(|line| *line == 0)
                {
                    continue;
                }
                previous_line
            } else {
                i64::from(*line) - 1
            };
            // generated column 0, source 0 (a delta of 0 after the first),
            // source line as a delta, source column 0.
            vlq(&mut mappings, 0);
            vlq(&mut mappings, 0);
            vlq(&mut mappings, line - previous_line);
            vlq(&mut mappings, 0);
            previous_line = line;
        }
        serde_json::json!({
            "version": 3,
            "file": format!("{}.{}.js", self.name, self.hash()),
            "sources": [self.file.replace('\\', "/")],
            "names": [],
            "mappings": mappings,
        })
        .to_string()
    }
}

/// Appends `value` as a Base64 VLQ (the source map encoding).
fn vlq(out: &mut String, value: i64) {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rest = if value < 0 { ((-value) << 1) | 1 } else { value << 1 };
    loop {
        let mut digit = rest & 0b1_1111;
        rest >>= 5;
        if rest > 0 {
            digit |= 0b10_0000;
        }
        out.push(char::from(DIGITS[usize::try_from(digit).unwrap_or(0)]));
        if rest == 0 {
            break;
        }
    }
}

/// A client component's logic; implemented by `#[client]`.
pub trait ClientLogic: Clone + PartialEq + Serialize + DeserializeOwned + 'static {
    /// What its effects answer with.
    type Message: Send + 'static;

    /// Its compiled form.
    const MODULE: &'static ClientModule;

    /// Handles `event`.
    fn update(&mut self, event: Event, fx: &mut Effects<Self::Message>);

    /// Handles an effect's answer.
    fn message(&mut self, message: Self::Message, fx: &mut Effects<Self::Message>) {
        let _ = (message, fx);
    }

    /// Runs once where the component comes alive: in the browser when it
    /// attaches, and natively when it mounts — never while a server renders
    /// its initial state.
    fn init(&mut self, fx: &mut Effects<Self::Message>) {
        let _ = fx;
    }

    /// Its view.
    fn view(&self) -> Node;
}

pub(crate) type Task<M> = Pin<Box<dyn Future<Output = Option<M>> + Send>>;
type Perform<M> = Box<dyn FnOnce(&Services) -> Option<Task<M>> + Send>;

/// One requested effect: what it is (for tests and the inspector) and how a
/// native host carries it out.
pub struct Effect<M> {
    /// What it is, as the browser's runtime names it (`call`, `navigate`,
    /// `after`, …).
    pub kind: &'static str,
    /// Its arguments, as the browser's runtime receives them.
    pub args: serde_json::Value,
    perform: Perform<M>,
}

impl<M> Effect<M> {
    /// Carries the effect out through services, as a native host does:
    /// the task that answers it, if it answers.
    #[must_use]
    pub fn perform(
        self,
        services: &Services,
    ) -> Option<Pin<Box<dyn Future<Output = Option<M>> + Send>>> {
        (self.perform)(services)
    }
}

impl<M> std::fmt::Debug for Effect<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Effect")
            .field("kind", &self.kind)
            .field("args", &self.args)
            .finish_non_exhaustive()
    }
}

/// Where a native client component's server calls go, and what they carry:
/// a `Services` extension.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientConfig {
    /// The server's base URL (`https://example.com`).
    pub server: String,
    /// Headers every call carries (an `Authorization`).
    pub headers: Vec<(String, String)>,
}

/// Where a native client component's navigations go: a `Services`
/// extension a host with its own router provides.
pub trait Navigate: Send + Sync {
    /// Go to `url`.
    fn navigate(&self, url: &str);
    /// Go back.
    fn back(&self);
}

/// Page-shared values between client components, by topic: a `Services`
/// extension. In the browser the runtime keeps them for the page.
#[derive(Default)]
pub struct Topics {
    values: Mutex<BTreeMap<String, serde_json::Value>>,
    #[allow(clippy::type_complexity, reason = "a list of subscriber callbacks")]
    subscribers: Mutex<Vec<(String, Arc<dyn Fn(serde_json::Value) + Send + Sync>)>>,
}

impl std::fmt::Debug for Topics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Topics").finish_non_exhaustive()
    }
}

impl Topics {
    /// Sets `topic` to `value` and tells its subscribers.
    pub fn publish(&self, topic: &str, value: serde_json::Value) {
        self.values.lock().unwrap_or_else(PoisonError::into_inner).insert(topic.to_owned(), value);
        let subscribers: Vec<_> = self
            .subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(name, _)| name == topic)
            .map(|(_, callback)| Arc::clone(callback))
            .collect();
        let Some(value) = self.get(topic) else { return };
        for callback in subscribers {
            callback(value.clone());
        }
    }

    /// `topic`'s current value.
    #[must_use]
    pub fn get(&self, topic: &str) -> Option<serde_json::Value> {
        self.values.lock().unwrap_or_else(PoisonError::into_inner).get(topic).cloned()
    }

    fn subscribe(&self, topic: &str, callback: Arc<dyn Fn(serde_json::Value) + Send + Sync>) {
        self.subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((topic.to_owned(), callback));
    }
}

/// The effects a client component requests from `update`, `message`, or
/// `init`; see the [module documentation](self).
pub struct Effects<M> {
    queue: Vec<Effect<M>>,
    subscriptions: Vec<Subscribe<M>>,
}

impl<M> Default for Effects<M> {
    fn default() -> Self {
        Self { queue: Vec::new(), subscriptions: Vec::new() }
    }
}

impl<M> std::fmt::Debug for Effects<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(&self.queue).finish()
    }
}

impl<M: Send + 'static> Effects<M> {
    /// No effects.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The requested effects, in order.
    #[must_use]
    pub fn requested(&self) -> &[Effect<M>] {
        &self.queue
    }

    /// Takes the requested effects.
    pub fn take(&mut self) -> Vec<Effect<M>> {
        std::mem::take(&mut self.queue)
    }

    fn take_subscriptions(&mut self) -> Vec<Subscribe<M>> {
        std::mem::take(&mut self.subscriptions)
    }

    pub(crate) fn push(
        &mut self,
        kind: &'static str,
        args: serde_json::Value,
        perform: impl FnOnce(&Services) -> Option<Task<M>> + Send + 'static,
    ) {
        self.queue.push(Effect { kind, args, perform: Box::new(perform) });
    }

    /// Calls server function `F` with `input`; `reply` turns its answer into
    /// a message.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "the input is owned at the call site, as a native call reads"
    )]
    pub fn call<F: ServerFn>(
        &mut self,
        input: F::Input,
        reply: impl FnOnce(Result<F::Output, ServerFnError>) -> M + Send + 'static,
    ) {
        let value = serde_json::to_value(&input).unwrap_or_default();
        let body = serde_json::to_vec(&input);
        let args = serde_json::json!([F::PATH, value]);
        self.push("call", args, move |services| {
            let http = services.http().cloned();
            let config = services
                .extension::<ClientConfig>()
                .map(|config| (*config).clone())
                .unwrap_or_default();
            Some(Box::pin(async move {
                let result = match (http, body) {
                    (Some(http), Ok(body)) => {
                        let mut request = HttpRequest::new(Method::Post, F::url(&config.server))
                            .header("content-type", "application/json")
                            .header("accept", "application/json")
                            .body(body);
                        for (name, value) in &config.headers {
                            request = request.header(name.clone(), value.clone());
                        }
                        match http.execute(request).await {
                            Ok(response) => decode::<F::Output>(&response),
                            Err(error) => Err(ServerFnError::Transport(error.to_string())),
                        }
                    }
                    (None, _) => Err(ServerFnError::Transport("no HTTP service".into())),
                    (_, Err(error)) => Err(ServerFnError::Decode(error.to_string())),
                };
                Some(reply(result))
            }))
        });
    }

    /// Delivers `message` after `milliseconds`.
    pub fn after(&mut self, milliseconds: u64, message: M) {
        // The delay is the tree's own clock's: `Client::render` sleeps
        // through the component's context before this answers.
        self.push("after", serde_json::json!([milliseconds]), move |_| {
            Some(Box::pin(async move { Some(message) }))
        });
    }

    /// Navigates to `url`: in the browser a same-origin page is fetched and
    /// swapped in place (Web milestone G); natively the host's
    /// [`Navigate`] extension decides.
    pub fn navigate(&mut self, url: impl Into<String>) {
        let url = url.into();
        self.push("navigate", serde_json::json!([url.clone()]), move |services| {
            if let Some(navigate) = services.extension::<Box<dyn Navigate>>() {
                navigate.navigate(&url);
            }
            None
        });
    }

    /// Goes back in the history.
    pub fn back(&mut self) {
        self.push("back", serde_json::json!([]), |services| {
            if let Some(navigate) = services.extension::<Box<dyn Navigate>>() {
                navigate.back();
            }
            None
        });
    }

    /// Moves keyboard focus to the node keyed `key` in this component.
    pub fn focus(&mut self, key: impl Into<String>) {
        self.push("focus", serde_json::json!([key.into()]), |_| None);
    }

    /// Puts `text` on the clipboard.
    pub fn copy(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.push("copy", serde_json::json!([text.clone()]), move |services| {
            let clipboard = services.clipboard().cloned()?;
            Some(Box::pin(async move {
                let _ = clipboard.write_text(text).await;
                None
            }))
        });
    }

    /// Keeps `value` under `key` on this device (Web Storage in the
    /// browser, the storage service elsewhere).
    pub fn store(&mut self, key: impl Into<String>, value: &impl Serialize) {
        let key = key.into();
        let value = serde_json::to_value(value).unwrap_or_default();
        self.push("store", serde_json::json!([key.clone(), value.clone()]), move |services| {
            let storage = services.storage().cloned()?;
            Some(Box::pin(async move {
                let _ = storage.set(key, value.to_string().into_bytes()).await;
                None
            }))
        });
    }

    /// Reads what [`Self::store`] kept under `key`.
    pub fn load<T: DeserializeOwned + Send + 'static>(
        &mut self,
        key: impl Into<String>,
        reply: impl FnOnce(Option<T>) -> M + Send + 'static,
    ) {
        let key = key.into();
        self.push("load", serde_json::json!([key.clone()]), move |services| {
            let storage = services.storage().cloned();
            Some(Box::pin(async move {
                let bytes = match storage {
                    Some(storage) => storage.get(key).await.ok().flatten(),
                    None => None,
                };
                let value = bytes.and_then(|bytes| serde_json::from_slice(&bytes).ok());
                Some(reply(value))
            }))
        });
    }

    /// Posts a notification.
    pub fn notify(&mut self, title: impl Into<String>, body: impl Into<String>) {
        let (title, body) = (title.into(), body.into());
        self.push("notify", serde_json::json!([title.clone(), body.clone()]), move |services| {
            let system = services.system().cloned()?;
            Some(Box::pin(async move {
                let _ = system.notify(title, body).await;
                None
            }))
        });
    }

    /// Sends an HTTP request; `reply` turns the response into a message.
    pub fn fetch(
        &mut self,
        request: HttpRequest,
        reply: impl FnOnce(Result<HttpResponse, String>) -> M + Send + 'static,
    ) {
        let args = serde_json::json!([
            request.url(),
            method_name(request.method()),
            String::from_utf8_lossy(request.body_bytes())
        ]);
        self.push("fetch", args, move |services| {
            let http = services.http().cloned();
            Some(Box::pin(async move {
                let result = match http {
                    Some(http) => http.execute(request).await.map_err(|error| error.to_string()),
                    None => Err("no HTTP service".to_owned()),
                };
                Some(reply(result))
            }))
        });
    }

    /// Sets the page-shared value `topic` (see [`Topics`]).
    pub fn publish(&mut self, topic: impl Into<String>, value: &impl Serialize) {
        let topic = topic.into();
        let value = serde_json::to_value(value).unwrap_or_default();
        self.push("publish", serde_json::json!([topic.clone(), value.clone()]), move |services| {
            if let Some(topics) = services.extension::<Topics>() {
                topics.publish(&topic, value);
            }
            None
        });
    }

    /// Hands a file to the person to save.
    pub fn download(&mut self, name: impl Into<String>, text: impl Into<String>) {
        self.push("download", serde_json::json!([name.into(), text.into()]), |_| None);
    }

    /// Calls hand-written JavaScript — the native escape hatch for what the
    /// client subset does not express: `function` exported by the module at
    /// `module` (a URL the page can import), with `args`. Natively there is
    /// no JavaScript, and `reply` receives an error.
    pub fn js(
        &mut self,
        module: impl Into<String>,
        function: impl Into<String>,
        args: serde_json::Value,
        reply: impl FnOnce(Result<serde_json::Value, String>) -> M + Send + 'static,
    ) {
        self.push(
            "js",
            serde_json::Value::Array(vec![module.into().into(), function.into().into(), args]),
            move |_| {
                Some(Box::pin(async move {
                    Some(reply(Err("hand-written JavaScript runs only in a browser".into())))
                }))
            },
        );
    }
}

impl<M: Send + 'static> Effects<M> {
    /// Subscribes to the page-shared value `topic`: `reply` turns each new
    /// value into a message, for as long as the component lives. Natively
    /// this needs a [`Topics`] extension.
    pub fn subscribe<T: DeserializeOwned + Send + 'static>(
        &mut self,
        topic: impl Into<String>,
        reply: impl Fn(T) -> M + Send + Sync + 'static,
    ) {
        let topic = topic.into();
        let topic_name = topic.clone();
        self.subscriptions.push(Box::new(move |services: &Services| {
            let topics = services.extension::<Topics>()?;
            let channel = Arc::new(Channel::default());
            let sender = Arc::clone(&channel);
            topics.subscribe(
                &topic,
                Arc::new(move |value| {
                    if let Ok(value) = serde_json::from_value(value) {
                        sender.send(reply(value));
                    }
                }),
            );
            Some(channel)
        }));
        self.push("subscribe", serde_json::json!([topic_name]), |_| None);
    }
}

/// Messages from outside the tree, awaited one at a time.
struct Channel<M> {
    queue: Mutex<(std::collections::VecDeque<M>, Option<std::task::Waker>)>,
}

impl<M> Default for Channel<M> {
    fn default() -> Self {
        Self { queue: Mutex::new((std::collections::VecDeque::new(), None)) }
    }
}

impl<M> Channel<M> {
    fn send(&self, message: M) {
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        queue.0.push_back(message);
        if let Some(waker) = queue.1.take() {
            waker.wake();
        }
    }

    fn next(self: Arc<Self>) -> impl Future<Output = M> {
        std::future::poll_fn(move |context| {
            let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(message) = queue.0.pop_front() {
                std::task::Poll::Ready(message)
            } else {
                queue.1 = Some(context.waker().clone());
                std::task::Poll::Pending
            }
        })
    }
}

type Subscribe<M> = Box<dyn FnOnce(&Services) -> Option<Arc<Channel<M>>> + Send>;

/// A server function's answer: its output, or the error the server gave.
fn decode<T: DeserializeOwned>(response: &HttpResponse) -> Result<T, ServerFnError> {
    if !(200..300).contains(&response.status()) {
        let message = serde_json::from_slice::<serde_json::Value>(response.body_bytes())
            .ok()
            .and_then(|value| value.get("error")?.as_str().map(str::to_owned))
            .unwrap_or_default();
        return Err(ServerFnError::Server { status: response.status(), message });
    }
    serde_json::from_slice(response.body_bytes())
        .map_err(|error| ServerFnError::Decode(error.to_string()))
}

fn method_name(method: &Method) -> &'static str {
    match method {
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Patch => "PATCH",
        Method::Delete => "DELETE",
        _ => "GET",
    }
}

/// Where a render is happening: a server rendering a page's initial state
/// sets this `Services` extension, and a client component then neither
/// runs `init` nor performs effects, and registers itself as an island.
#[derive(Debug, Default)]
pub struct ServerRender {
    islands: Mutex<Vec<Island>>,
    marks: Mutex<RenderMarks>,
}

/// What a render's components noted besides islands.
#[derive(Debug, Default)]
struct RenderMarks {
    /// Streamed boundaries, and whether each has its content.
    boundaries: Vec<(NodeId, bool)>,
    /// Forms, and where each posts.
    forms: HashMap<NodeId, String>,
    /// Links, and where each goes.
    links: HashMap<NodeId, String>,
    /// Components that read the request.
    dynamic: HashSet<ComponentId>,
    /// Bumped whenever a boundary's content arrives.
    revision: u64,
}

/// The identity the tree gives node `key` of component `owner` — the
/// same scoping `ComponentTree` applies to every node a component renders.
#[must_use]
pub fn global_id(owner: ComponentId, key: &str) -> NodeId {
    if owner == ComponentId::ROOT {
        NodeId::from_key(key)
    } else {
        rustnative_core::wire::node_id(&format!("{}~{key}", owner.get()))
    }
}

/// What a page's interactive subtree needs in the browser.
#[derive(Debug, Clone, PartialEq)]
pub struct Island {
    /// The component whose view is the subtree's root.
    pub owner: ComponentId,
    /// How it runs in the browser.
    pub kind: IslandKind,
    /// Its state, as the browser receives it.
    pub state: serde_json::Value,
    /// Its state is kept in the browser's storage and outlives the page
    /// ([`Persisted`]).
    pub persist: bool,
}

/// How an island runs in the browser.
#[derive(Debug, Clone, PartialEq)]
pub enum IslandKind {
    /// Generated JavaScript.
    Client(&'static ClientModule),
    /// A component in a WebAssembly module (`crate::wasm`).
    Wasm {
        /// The module's name.
        module: &'static str,
        /// The component's name in it.
        component: &'static str,
        /// Whether it runs in a Web Worker.
        worker: bool,
    },
    /// Held on the server, over a persistent connection (`C33`).
    Live {
        /// The WebSocket URL of its server session.
        url: String,
        /// The client module that takes over in `Auto` mode, if any.
        then: Option<&'static ClientModule>,
    },
}

impl PartialEq for ClientModule {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.js == other.js
    }
}

impl ServerRender {
    /// A render collecting islands.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records (or replaces) `island`.
    pub fn register(&self, island: Island) {
        let mut islands = self.islands.lock().unwrap_or_else(PoisonError::into_inner);
        islands.retain(|existing| existing.owner != island.owner);
        islands.push(island);
    }

    /// The islands registered, in registration order.
    #[must_use]
    pub fn islands(&self) -> Vec<Island> {
        self.islands.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn marks(&self) -> std::sync::MutexGuard<'_, RenderMarks> {
        self.marks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records a streamed boundary and whether it has its content yet.
    pub fn note_pending(&self, id: NodeId, resolved: bool) {
        let mut marks = self.marks();
        match marks.boundaries.iter_mut().find(|(known, _)| *known == id) {
            Some((_, state)) if *state == resolved => {}
            Some((_, state)) => {
                *state = resolved;
                marks.revision += 1;
            }
            None => {
                marks.boundaries.push((id, resolved));
                marks.revision += 1;
            }
        }
    }

    /// Records a form that posts to `action`.
    pub fn note_form(&self, id: NodeId, action: &str) {
        self.marks().forms.insert(id, action.to_owned());
    }

    /// Records a link to `href`.
    pub fn note_link(&self, id: NodeId, href: &str) {
        self.marks().links.insert(id, href.to_owned());
    }

    /// The links, with where each goes.
    #[must_use]
    pub fn links(&self) -> HashMap<NodeId, String> {
        self.marks().links.clone()
    }

    /// Records that `component` read the request.
    pub fn note_dynamic(&self, component: ComponentId) {
        self.marks().dynamic.insert(component);
    }

    /// The streamed boundaries, in the order first rendered, with whether
    /// each has its content.
    #[must_use]
    pub fn boundaries(&self) -> Vec<(NodeId, bool)> {
        self.marks().boundaries.clone()
    }

    /// The boundaries still showing their fallback.
    #[must_use]
    pub fn unresolved(&self) -> HashSet<NodeId> {
        self.marks()
            .boundaries
            .iter()
            .filter(|(_, resolved)| !resolved)
            .map(|(id, _)| *id)
            .collect()
    }

    /// The forms, with where each posts.
    #[must_use]
    pub fn forms(&self) -> HashMap<NodeId, String> {
        self.marks().forms.clone()
    }

    /// Whether any component read the request.
    #[must_use]
    pub fn is_dynamic(&self) -> bool {
        !self.marks().dynamic.is_empty()
    }

    /// How many components read the request.
    #[must_use]
    pub fn dynamic_count(&self) -> usize {
        self.marks().dynamic.len()
    }

    /// Changes whenever a boundary's content arrives.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.marks().revision
    }
}

/// `event` as the browser's runtime delivers it to generated client code
/// (`{ type: "Click", target: "key" }`, …), or `None` for an event the
/// browser does not deliver to client logic. Targets are local keys.
#[must_use]
pub fn event_json(event: &Event) -> Option<serde_json::Value> {
    use serde_json::json;
    let key = |id: rustnative_core::NodeId| id.local_key().unwrap_or_default();
    let key_code = |code: &rustnative_core::KeyCode| match code {
        rustnative_core::KeyCode::Character(ch) => json!({ "Character": ch.to_string() }),
        rustnative_core::KeyCode::Function(n) => json!({ "Function": n }),
        rustnative_core::KeyCode::Unknown(n) => json!({ "Unknown": n }),
        other => json!(format!("{other:?}")),
    };
    Some(match event {
        Event::Click { target } => json!({ "type": "Click", "target": key(*target) }),
        Event::FocusGained { target } => json!({ "type": "FocusGained", "target": key(*target) }),
        Event::FocusLost { target } => json!({ "type": "FocusLost", "target": key(*target) }),
        Event::TextChanged { target, value } => {
            json!({ "type": "TextChanged", "target": key(*target), "value": value })
        }
        Event::Toggled { target, on } => {
            json!({ "type": "Toggled", "target": key(*target), "on": on })
        }
        Event::ValueChanged { target, value } => {
            json!({ "type": "ValueChanged", "target": key(*target), "value": value })
        }
        Event::SelectionChanged { target, index } => {
            json!({ "type": "SelectionChanged", "target": key(*target), "index": index })
        }
        Event::DateChanged { target, date } => {
            json!({ "type": "DateChanged", "target": key(*target), "date": date })
        }
        Event::TabSelected { target, index } => {
            json!({ "type": "TabSelected", "target": key(*target), "index": index })
        }
        Event::KeyDown { target, key: code, modifiers }
        | Event::KeyUp { target, key: code, modifiers } => json!({
            "type": if matches!(event, Event::KeyDown { .. }) { "KeyDown" } else { "KeyUp" },
            "target": target.map(key),
            "key": key_code(code),
            "modifiers": { "shift": modifiers.shift, "ctrl": modifiers.ctrl, "alt": modifiers.alt, "meta": modifiers.meta },
        }),
        Event::TextInput { target, text } => {
            json!({ "type": "TextInput", "target": target.map(key), "text": text })
        }
        Event::PointerDown { target, pointer }
        | Event::PointerMove { target, pointer }
        | Event::PointerUp { target, pointer }
        | Event::PointerCancel { target, pointer } => json!({
            "type": match event {
                Event::PointerDown { .. } => "PointerDown",
                Event::PointerMove { .. } => "PointerMove",
                Event::PointerUp { .. } => "PointerUp",
                _ => "PointerCancel",
            },
            "target": key(*target),
            "pointer": pointer_json(pointer),
        }),
        Event::PointerEnter { target } => json!({ "type": "PointerEnter", "target": key(*target) }),
        Event::PointerLeave { target } => json!({ "type": "PointerLeave", "target": key(*target) }),
        Event::Wheel { target, delta } => {
            let (variant, x, y) = match delta {
                rustnative_core::WheelDelta::Lines { x, y } => ("Lines", x, y),
                rustnative_core::WheelDelta::Pixels { x, y } => ("Pixels", x, y),
            };
            json!({ "type": "Wheel", "target": key(*target), "delta": { variant: { "x": x, "y": y } } })
        }
        Event::Composition { target, composition } => {
            let composition = match composition {
                rustnative_core::Composition::Started => json!("Started"),
                rustnative_core::Composition::Updated { text, cursor } => {
                    json!({ "Updated": { "text": text, "cursor": cursor } })
                }
                rustnative_core::Composition::Committed { text } => {
                    json!({ "Committed": { "text": text } })
                }
                rustnative_core::Composition::Cancelled => json!("Cancelled"),
            };
            json!({ "type": "Composition", "target": target.map(key), "composition": composition })
        }
        Event::Clipboard { target, action } => {
            let action = match action {
                rustnative_core::ClipboardAction::Copy => json!("Copy"),
                rustnative_core::ClipboardAction::Cut => json!("Cut"),
                rustnative_core::ClipboardAction::Paste { text } => {
                    json!({ "Paste": { "text": text } })
                }
            };
            json!({ "type": "Clipboard", "target": target.map(key), "action": action })
        }
        Event::DeepLink { url } => json!({ "type": "DeepLink", "url": url }),
        Event::Lifecycle(lifecycle) => {
            json!({ "type": "Lifecycle", "value": format!("{lifecycle:?}") })
        }
        _ => return None,
    })
}

/// The event `json` describes, as the runtime delivers it ([`event_json`]'s
/// inverse): how a WebAssembly subtree receives its events. Targets are
/// local keys.
#[must_use]
pub fn event_from_json(json: &serde_json::Value) -> Option<Event> {
    use rustnative_core::{
        CalendarDate, ClipboardAction, Composition, KeyCode, KeyModifiers, Lifecycle, NodeId,
        WheelDelta,
    };
    use serde_json::Value;
    let text = |field: &str| json.get(field).and_then(Value::as_str).map(str::to_owned);
    let node = |value: Option<&Value>| value.and_then(Value::as_str).map(NodeId::from_key);
    let target = || node(json.get("target"));
    let key_code = |value: &Value| -> KeyCode {
        match value {
            Value::String(name) => match name.as_str() {
                "Enter" => KeyCode::Enter,
                "Space" => KeyCode::Space,
                "Tab" => KeyCode::Tab,
                "Escape" => KeyCode::Escape,
                "Backspace" => KeyCode::Backspace,
                "ArrowLeft" => KeyCode::ArrowLeft,
                "ArrowRight" => KeyCode::ArrowRight,
                "ArrowUp" => KeyCode::ArrowUp,
                "ArrowDown" => KeyCode::ArrowDown,
                "Delete" => KeyCode::Delete,
                "Insert" => KeyCode::Insert,
                "Home" => KeyCode::Home,
                "End" => KeyCode::End,
                "PageUp" => KeyCode::PageUp,
                "PageDown" => KeyCode::PageDown,
                _ => KeyCode::Unknown(0),
            },
            other => {
                if let Some(character) = other
                    .get("Character")
                    .and_then(Value::as_str)
                    .and_then(|text| text.chars().next())
                {
                    KeyCode::Character(character)
                } else if let Some(number) = other.get("Function").and_then(Value::as_u64) {
                    KeyCode::Function(u8::try_from(number).unwrap_or(0))
                } else {
                    KeyCode::Unknown(
                        other
                            .get("Unknown")
                            .and_then(Value::as_u64)
                            .and_then(|code| u32::try_from(code).ok())
                            .unwrap_or(0),
                    )
                }
            }
        }
    };
    let modifiers = |value: Option<&Value>| {
        let flag = |name: &str| {
            value
                .and_then(|modifiers| modifiers.get(name))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        };
        KeyModifiers {
            shift: flag("shift"),
            ctrl: flag("ctrl"),
            alt: flag("alt"),
            meta: flag("meta"),
        }
    };
    let kind = json.get("type").and_then(Value::as_str)?;
    Some(match kind {
        "Click" => Event::Click { target: target()? },
        "FocusGained" => Event::FocusGained { target: target()? },
        "FocusLost" => Event::FocusLost { target: target()? },
        "PointerEnter" => Event::PointerEnter { target: target()? },
        "PointerLeave" => Event::PointerLeave { target: target()? },
        "TextChanged" => Event::TextChanged { target: target()?, value: text("value")? },
        "TextInput" => Event::TextInput { target: target(), text: text("text")? },
        "Toggled" => Event::Toggled { target: target()?, on: json.get("on")?.as_bool()? },
        "ValueChanged" => {
            Event::ValueChanged { target: target()?, value: json.get("value")?.as_i64()? }
        }
        "SelectionChanged" => Event::SelectionChanged {
            target: target()?,
            index: json
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|index| usize::try_from(index).ok()),
        },
        "TabSelected" => Event::TabSelected {
            target: target()?,
            index: usize::try_from(json.get("index")?.as_u64()?).ok()?,
        },
        "DateChanged" => {
            let date = json.get("date")?;
            let part = |name: &str| date.get(name).and_then(Value::as_i64);
            Event::DateChanged {
                target: target()?,
                date: CalendarDate::new(
                    i32::try_from(part("year")?).ok()?,
                    u8::try_from(part("month")?).ok()?,
                    u8::try_from(part("day")?).ok()?,
                )?,
            }
        }
        "KeyDown" | "KeyUp" => {
            let (key, modifiers) = (key_code(json.get("key")?), modifiers(json.get("modifiers")));
            if kind == "KeyDown" {
                Event::KeyDown { target: target(), key, modifiers }
            } else {
                Event::KeyUp { target: target(), key, modifiers }
            }
        }
        "PointerDown" | "PointerMove" | "PointerUp" | "PointerCancel" => {
            let pointer = pointer_from_json(
                json.get("pointer")?,
                modifiers(json.pointer("/pointer/modifiers")),
            )?;
            let target = target()?;
            match kind {
                "PointerDown" => Event::PointerDown { target, pointer },
                "PointerMove" => Event::PointerMove { target, pointer },
                "PointerUp" => Event::PointerUp { target, pointer },
                _ => Event::PointerCancel { target, pointer },
            }
        }
        "Wheel" => {
            let delta = json.get("delta")?;
            let amount = |variant: &Value| -> Option<(i32, i32)> {
                Some((
                    i32::try_from(variant.get("x")?.as_i64()?).ok()?,
                    i32::try_from(variant.get("y")?.as_i64()?).ok()?,
                ))
            };
            let delta = if let Some((x, y)) = delta.get("Lines").and_then(amount) {
                WheelDelta::Lines { x, y }
            } else {
                let (x, y) = delta.get("Pixels").and_then(amount)?;
                WheelDelta::Pixels { x, y }
            };
            Event::Wheel { target: target()?, delta }
        }
        "Composition" => {
            let value = json.get("composition")?;
            let composition = match value.as_str() {
                Some("Started") => Composition::Started,
                Some("Cancelled") => Composition::Cancelled,
                _ => {
                    if let Some(updated) = value.get("Updated") {
                        Composition::Updated {
                            text: updated.get("text")?.as_str()?.to_owned(),
                            cursor: usize::try_from(updated.get("cursor")?.as_u64()?).ok()?,
                        }
                    } else {
                        Composition::Committed {
                            text: value.get("Committed")?.get("text")?.as_str()?.to_owned(),
                        }
                    }
                }
            };
            Event::Composition { target: target(), composition }
        }
        "Clipboard" => {
            let value = json.get("action")?;
            let action = match value.as_str() {
                Some("Copy") => ClipboardAction::Copy,
                Some("Cut") => ClipboardAction::Cut,
                _ => ClipboardAction::Paste {
                    text: value
                        .get("Paste")?
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                },
            };
            Event::Clipboard { target: target(), action }
        }
        "Lifecycle" => Event::Lifecycle(match json.get("value")?.as_str()? {
            "Suspending" => Lifecycle::Suspending,
            "Resuming" => Lifecycle::Resuming,
            "Terminating" => Lifecycle::Terminating,
            _ => Lifecycle::LowMemory,
        }),
        "DeepLink" => Event::DeepLink { url: text("url")? },
        _ => return None,
    })
}

fn pointer_from_json(
    json: &serde_json::Value,
    modifiers: rustnative_core::KeyModifiers,
) -> Option<rustnative_core::PointerEvent> {
    use rustnative_core::{Point, PointerButton, PointerButtons, PointerEvent, PointerKind};
    use serde_json::Value;
    let button = |name: &str| match name {
        "Primary" => Some(PointerButton::Primary),
        "Secondary" => Some(PointerButton::Secondary),
        "Middle" => Some(PointerButton::Middle),
        "Back" => Some(PointerButton::Back),
        "Forward" => Some(PointerButton::Forward),
        _ => None,
    };
    let kind = match json.get("kind").and_then(Value::as_str) {
        Some("Touch") => PointerKind::Touch,
        Some("Pen") => PointerKind::Pen,
        _ => PointerKind::Mouse,
    };
    let coordinate = |name: &str| {
        json.pointer(&format!("/position/{name}"))
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
    };
    let bits = json.get("buttons").and_then(Value::as_u64).unwrap_or(0);
    let all = [
        PointerButton::Primary,
        PointerButton::Secondary,
        PointerButton::Middle,
        PointerButton::Back,
        PointerButton::Forward,
    ];
    let buttons = all
        .into_iter()
        .enumerate()
        .filter(|(index, _)| bits & (1 << index) != 0)
        .fold(PointerButtons::none(), |buttons, (_, button)| buttons.with(button));
    let id = u32::try_from(json.get("pointer_id")?.as_u64()?).ok()?;
    let mut pointer = PointerEvent::new(
        id,
        kind,
        Point::new(coordinate("x")?, coordinate("y")?),
        std::time::Duration::ZERO,
    )
    .with_buttons(buttons)
    .with_modifiers(modifiers)
    .with_region(
        json.get("region").and_then(Value::as_u64).and_then(|region| u32::try_from(region).ok()),
    );
    if let Some(pressed) = json.get("button").and_then(Value::as_str).and_then(button) {
        pointer = pointer.with_button(pressed);
    }
    if let Some(pressure) = json.get("pressure").and_then(Value::as_f64) {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a pressure in [0, 1], which an f32 holds"
        )]
        let pressure = pressure as f32;
        pointer = pointer.with_pressure(pressure);
    }
    Some(pointer)
}
/// A pointer sample as the runtime delivers it (`rn.pointerOf`).
fn pointer_json(pointer: &rustnative_core::PointerEvent) -> serde_json::Value {
    use rustnative_core::{PointerButton, PointerKind};
    let name = |button: PointerButton| match button {
        PointerButton::Primary => "Primary",
        PointerButton::Secondary => "Secondary",
        PointerButton::Middle => "Middle",
        PointerButton::Back => "Back",
        PointerButton::Forward => "Forward",
    };
    let all = [
        PointerButton::Primary,
        PointerButton::Secondary,
        PointerButton::Middle,
        PointerButton::Back,
        PointerButton::Forward,
    ];
    let bits = all
        .iter()
        .enumerate()
        .filter(|(_, button)| pointer.buttons().contains(**button))
        .fold(0, |bits, (index, _)| bits | (1 << index));
    let modifiers = pointer.modifiers();
    serde_json::json!({
        "pointer_id": pointer.pointer_id(),
        "kind": match pointer.kind() { PointerKind::Mouse => "Mouse", PointerKind::Touch => "Touch", PointerKind::Pen => "Pen" },
        "position": { "x": pointer.position().x, "y": pointer.position().y },
        "button": pointer.button().map(name),
        "buttons": bits,
        "modifiers": { "shift": modifiers.shift, "ctrl": modifiers.ctrl, "alt": modifiers.alt, "meta": modifiers.meta },
        "pressure": pointer.pressure(),
        "region": pointer.region(),
    })
}

/// The largest integer JavaScript represents exactly, 2⁵³ − 1: client state
/// and client arithmetic stay within it on both sides.
pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// Why a client component's state cannot cross to the browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    /// It did not serialize.
    Serialize(String),
    /// A number is beyond what JavaScript represents exactly.
    Unsafe(String),
}

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Serialize(error) => write!(f, "the client state did not serialize: {error}"),
            Self::Unsafe(number) => write!(
                f,
                "the client state holds {number}, beyond ±(2^53 − 1), which JavaScript cannot represent exactly"
            ),
        }
    }
}

impl std::error::Error for StateError {}

/// `state` as the JSON the browser receives, checked: every integer within
/// ±(2⁵³ − 1).
///
/// # Errors
///
/// It does not serialize, or holds an integer JavaScript cannot represent.
pub fn state_json(state: &impl Serialize) -> Result<serde_json::Value, StateError> {
    fn check(value: &serde_json::Value) -> Result<(), StateError> {
        match value {
            serde_json::Value::Number(number) => {
                let safe = number.as_i64().map_or_else(
                    || number.as_u64().is_none_or(|value| value <= MAX_SAFE_INTEGER.unsigned_abs()),
                    |value| value.unsigned_abs() <= MAX_SAFE_INTEGER.unsigned_abs(),
                );
                if safe { Ok(()) } else { Err(StateError::Unsafe(number.to_string())) }
            }
            serde_json::Value::Array(items) => items.iter().try_for_each(check),
            serde_json::Value::Object(fields) => fields.values().try_for_each(check),
            _ => Ok(()),
        }
    }
    let value =
        serde_json::to_value(state).map_err(|error| StateError::Serialize(error.to_string()))?;
    check(&value)?;
    Ok(value)
}

/// The component that runs a client component's logic ([`ClientLogic`])
/// on a native target and on the server; see the [module
/// documentation](self). Its props are the initial state.
pub struct Client<S: ClientLogic, const PERSIST: bool = false> {
    initial: S,
    state: S,
    fx: Effects<S::Message>,
    started: bool,
    /// Topic subscriptions, and whether a task is waiting on each.
    channels: Vec<(Arc<Channel<S::Message>>, bool)>,
}

/// What a [`Client`]'s own tasks deliver.
pub enum ClientMessage<M> {
    /// An effect's answer, for the logic's `message`.
    Reply(M),
    /// An effect finished with no answer.
    Done,
    /// Subscription   delivered a value.
    Topic(usize, M),
}

impl<M> std::fmt::Debug for ClientMessage<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Reply(_) => f.write_str("Reply"),
            Self::Done => f.write_str("Done"),
            Self::Topic(index, _) => write!(f, "Topic({index})"),
        }
    }
}

/// A client component whose state the browser keeps (`localStorage`):
/// it comes back as the person left it, on any page that has it, until
/// the site's storage is cleared. Natively it is a [`Client`].
pub type Persisted<S> = Client<S, true>;

impl<S: ClientLogic, const PERSIST: bool> Client<S, PERSIST> {
    /// The current state.
    #[must_use]
    pub const fn state(&self) -> &S {
        &self.state
    }
}

impl<S: ClientLogic, const PERSIST: bool> Component for Client<S, PERSIST> {
    type Props = S;
    type Message = ClientMessage<S::Message>;

    fn new(props: S) -> Self {
        Self {
            initial: props.clone(),
            state: props,
            fx: Effects::new(),
            started: false,
            channels: Vec::new(),
        }
    }

    fn props(&self) -> &S {
        &self.initial
    }

    fn set_props(&mut self, props: S) {
        // New initial state from the parent replaces the running state, as a
        // new server render would.
        self.initial = props.clone();
        self.state = props;
    }

    fn view(&self) -> Node {
        self.state.view()
    }

    fn update(&mut self, event: Event) {
        self.state.update(event, &mut self.fx);
    }

    fn message(&mut self, message: Self::Message) {
        match message {
            ClientMessage::Reply(message) => self.state.message(message, &mut self.fx),
            ClientMessage::Topic(index, message) => {
                if let Some((_, waiting)) = self.channels.get_mut(index) {
                    *waiting = false;
                }
                self.state.message(message, &mut self.fx);
            }
            ClientMessage::Done => {}
        }
    }

    fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
        if let Some(render) = context.services().extension::<ServerRender>() {
            if let Ok(state) = state_json(&self.state) {
                render.register(Island {
                    owner: context.id(),
                    kind: IslandKind::Client(S::MODULE),
                    state,
                    persist: PERSIST,
                });
            }
            return self.view();
        }
        if !self.started {
            self.started = true;
            self.state.init(&mut self.fx);
        }
        let services = context.services().clone();
        for effect in self.fx.take() {
            // Pointer capture is the tree's own input request.
            if matches!(effect.kind, "capturePointer" | "releasePointer") {
                let key =
                    effect.args.get(0).and_then(serde_json::Value::as_str).unwrap_or_default();
                let id = effect
                    .args
                    .get(1)
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|id| u32::try_from(id).ok());
                if let Some(id) = id {
                    if effect.kind == "capturePointer" {
                        context.input().capture_pointer(key, id);
                    } else {
                        context.input().release_pointer(key, id);
                    }
                }
                continue;
            }
            let delay = if effect.kind == "after" {
                effect.args.get(0).and_then(serde_json::Value::as_u64)
            } else {
                None
            };
            if let Some(task) = (effect.perform)(&services) {
                let sleep =
                    delay.map(|milliseconds| context.sleep(Duration::from_millis(milliseconds)));
                context.spawn(async move {
                    if let Some(sleep) = sleep {
                        sleep.await;
                    }
                    task.await.map_or(ClientMessage::Done, ClientMessage::Reply)
                });
            }
        }
        for subscribe in self.fx.take_subscriptions() {
            if let Some(channel) = subscribe(&services) {
                self.channels.push((channel, false));
            }
        }
        for (index, (channel, waiting)) in self.channels.iter_mut().enumerate() {
            if !*waiting {
                *waiting = true;
                let next = Arc::clone(channel).next();
                context.spawn(async move { ClientMessage::Topic(index, next.await) });
            }
        }
        self.view()
    }

    fn inspect(&self) -> Option<serde_json::Value> {
        serde_json::to_value(&self.state).ok()
    }
}

/// The checked arithmetic `#[client]` rewrites client logic's integer
/// operators into, so both languages agree on every result: an overflow —
/// beyond the type's range, or for 64-bit integers beyond ±(2⁵³ − 1), where
/// JavaScript stops being exact — is the same error in Rust (this panic, in
/// every build, as a debug build already does) and in the browser (the
/// generated code throws).
pub mod rt {
    /// An integer type client logic may compute with.
    pub trait Int: Copy + PartialOrd {
        /// `self + other`, if it fits.
        fn checked_add(self, other: Self) -> Option<Self>;
        /// `self - other`, if it fits.
        fn checked_sub(self, other: Self) -> Option<Self>;
        /// `self * other`, if it fits.
        fn checked_mul(self, other: Self) -> Option<Self>;
        /// `self / other` truncated toward zero, if defined and it fits.
        fn checked_div(self, other: Self) -> Option<Self>;
        /// `self % other`, if defined.
        fn checked_rem(self, other: Self) -> Option<Self>;
        /// `-self`, if it fits.
        fn checked_neg(self) -> Option<Self>;
        /// Whether JavaScript represents it exactly.
        fn is_safe(self) -> bool;
    }

    macro_rules! narrow {
        ($($type:ty),*) => {$(
            impl Int for $type {
                fn checked_add(self, other: Self) -> Option<Self> { <$type>::checked_add(self, other) }
                fn checked_sub(self, other: Self) -> Option<Self> { <$type>::checked_sub(self, other) }
                fn checked_mul(self, other: Self) -> Option<Self> { <$type>::checked_mul(self, other) }
                fn checked_div(self, other: Self) -> Option<Self> { <$type>::checked_div(self, other) }
                fn checked_rem(self, other: Self) -> Option<Self> { <$type>::checked_rem(self, other) }
                fn checked_neg(self) -> Option<Self> { <$type>::checked_neg(self) }
                fn is_safe(self) -> bool { true }
            }
        )*};
    }

    macro_rules! wide {
        ($($type:ty),*) => {$(
            impl Int for $type {
                fn checked_add(self, other: Self) -> Option<Self> { <$type>::checked_add(self, other) }
                fn checked_sub(self, other: Self) -> Option<Self> { <$type>::checked_sub(self, other) }
                fn checked_mul(self, other: Self) -> Option<Self> { <$type>::checked_mul(self, other) }
                fn checked_div(self, other: Self) -> Option<Self> { <$type>::checked_div(self, other) }
                fn checked_rem(self, other: Self) -> Option<Self> { <$type>::checked_rem(self, other) }
                fn checked_neg(self) -> Option<Self> { <$type>::checked_neg(self) }
                fn is_safe(self) -> bool {
                    let limit = i128::from(super::MAX_SAFE_INTEGER);
                    i128::try_from(self).is_ok_and(|value| value <= limit && value >= -limit)
                }
            }
        )*};
    }

    narrow!(i8, i16, i32, u8, u16, u32);
    wide!(i64, u64, isize, usize);

    #[track_caller]
    #[allow(
        clippy::panic,
        reason = "an overflow in client logic is the error Rust itself raises, in both languages"
    )]
    fn overflow(operation: &str) -> ! {
        panic!("attempt to {operation} with overflow")
    }

    #[track_caller]
    fn checked<T: Int>(result: Option<T>, operation: &str) -> T {
        match result {
            Some(value) if value.is_safe() => value,
            _ => overflow(operation),
        }
    }

    /// An integer operand: the integer, or a reference to one (an iterator's
    /// item, a pattern's binding), as Rust's operators accept both.
    pub trait Value {
        /// The integer type.
        type Int: Int;
        /// The integer.
        fn get(self) -> Self::Int;
    }

    macro_rules! value {
        ($($type:ty),*) => {$(
            impl Value for $type { type Int = $type; fn get(self) -> $type { self } }
            impl Value for &$type { type Int = $type; fn get(self) -> $type { *self } }
            impl Value for &&$type { type Int = $type; fn get(self) -> $type { **self } }
            impl Value for &mut $type { type Int = $type; fn get(self) -> $type { *self } }
        )*};
    }
    value!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

    /// `a + b`.
    #[track_caller]
    pub fn add<A: Value, B: Value<Int = A::Int>>(a: A, b: B) -> A::Int {
        checked(a.get().checked_add(b.get()), "add")
    }

    /// `a - b`.
    #[track_caller]
    pub fn sub<A: Value, B: Value<Int = A::Int>>(a: A, b: B) -> A::Int {
        checked(a.get().checked_sub(b.get()), "subtract")
    }

    /// `a * b`.
    #[track_caller]
    pub fn mul<A: Value, B: Value<Int = A::Int>>(a: A, b: B) -> A::Int {
        checked(a.get().checked_mul(b.get()), "multiply")
    }

    /// `a / b`, truncated toward zero.
    ///
    /// # Panics
    ///
    /// `b` is zero, or the quotient overflows.
    #[track_caller]
    #[allow(clippy::panic, reason = "division by zero is the error Rust itself raises")]
    pub fn div<A: Value, B: Value<Int = A::Int>>(a: A, b: B) -> A::Int {
        let (a, b) = (a.get(), b.get());
        match a.checked_div(b) {
            Some(value) if value.is_safe() => value,
            None if a.checked_sub(a).is_some_and(|zero| b == zero) => {
                panic!("attempt to divide by zero")
            }
            _ => overflow("divide"),
        }
    }

    /// `a % b`.
    ///
    /// # Panics
    ///
    /// `b` is zero.
    #[track_caller]
    #[allow(clippy::panic, reason = "a remainder by zero is the error Rust itself raises")]
    pub fn rem<A: Value, B: Value<Int = A::Int>>(a: A, b: B) -> A::Int {
        let (a, b) = (a.get(), b.get());
        match a.checked_rem(b) {
            Some(value) => value,
            None if a.checked_sub(a).is_some_and(|zero| b == zero) => {
                panic!("attempt to calculate the remainder with a divisor of zero")
            }
            None => overflow("calculate the remainder"),
        }
    }

    /// `-a`.
    #[track_caller]
    pub fn neg<A: Value>(a: A) -> A::Int {
        checked(a.get().checked_neg(), "negate")
    }
    /// An integer as an `i128` and back, with `as`'s wrapping.
    pub trait Cast: Int {
        /// Widened.
        fn widen(self) -> i128;
        /// `value as Self`.
        fn narrow(value: i128) -> Self;
    }

    macro_rules! cast {
        ($($type:ty),*) => {$(
            impl Cast for $type {
                fn widen(self) -> i128 { i128::from(self) }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap, reason = "`as`'s wrapping is the semantics being reproduced")]
                fn narrow(value: i128) -> Self { value as $type }
            }
        )*};
    }
    cast!(i8, i16, i32, i64, u8, u16, u32, u64);

    impl Cast for isize {
        #[allow(clippy::cast_possible_truncation, reason = "64-bit targets; 32-bit ones hold less")]
        fn widen(self) -> i128 {
            self as i128
        }
        #[allow(
            clippy::cast_possible_truncation,
            reason = "`as`'s wrapping is the semantics being reproduced"
        )]
        fn narrow(value: i128) -> Self {
            value as isize
        }
    }

    impl Cast for usize {
        #[allow(clippy::cast_possible_truncation, reason = "64-bit targets; 32-bit ones hold less")]
        fn widen(self) -> i128 {
            self as i128
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "`as`'s wrapping is the semantics being reproduced"
        )]
        fn narrow(value: i128) -> Self {
            value as usize
        }
    }

    /// `value as T` between integers: `as`'s wrapping, with a 64-bit result
    /// beyond ±(2⁵³ − 1) an overflow (what the browser's runtime does).
    #[track_caller]
    pub fn cast<F: Value, T: Cast>(value: F) -> T
    where
        F::Int: Cast,
    {
        let result = T::narrow(value.get().widen());
        if result.is_safe() { result } else { overflow("cast") }
    }

    /// `value as T` from a float: saturating (`NaN` is zero), with a 64-bit
    /// result beyond ±(2⁵³ − 1) an overflow.
    #[track_caller]
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "saturating float-to-integer conversion is `as`'s own"
    )]
    pub fn cast_float<T: Cast>(value: f64) -> T {
        // Narrow targets saturate, as `as` does; a wide result beyond
        // JavaScript's range is an overflow.
        let saturated = saturate::<T>(value);
        if saturated.is_safe() { saturated } else { overflow("cast") }
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        reason = "saturating float-to-integer conversion is `as`'s own"
    )]
    fn saturate<T: Cast>(value: f64) -> T {
        // `f64 as i128` saturates at i128's range; clamping to T's range
        // gives `f64 as T`.
        let wide = value as i128;
        let min = range::<T>(false);
        let max = range::<T>(true);
        T::narrow(wide.clamp(min, max))
    }

    fn range<T: Cast>(max: bool) -> i128 {
        // The type's extremes, found through wrapping: all ones narrowed.
        let all_ones = T::narrow(-1).widen();
        if all_ones == -1 {
            // Signed: 2^(bits-1) - 1 and its negative minus one.
            let top = (0..128)
                .map(|bit| 1_i128 << bit)
                .find(|value| T::narrow(*value).widen() != *value)
                .unwrap_or(1 << 64);
            if max { top - 1 } else { -top }
        } else if max {
            all_ones
        } else {
            0
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn results_within_range_are_exact() {
            assert_eq!(add(2_i32, 3), 5);
            assert_eq!(div(-7_i32, 2), -3, "truncated toward zero");
            assert_eq!(rem(-7_i32, 2), -1);
            assert_eq!(sub(5_usize, 5_usize), 0);
        }

        #[test]
        #[should_panic(expected = "attempt to add with overflow")]
        fn a_narrow_overflow_panics() {
            let _ = add(i32::MAX, 1);
        }

        #[test]
        #[should_panic(expected = "attempt to multiply with overflow")]
        fn a_wide_result_beyond_javascript_panics() {
            let _ = mul(super::super::MAX_SAFE_INTEGER, 2_i64);
        }

        #[test]
        #[should_panic(expected = "attempt to subtract with overflow")]
        fn an_unsigned_underflow_panics() {
            let _ = sub(0_usize, 1_usize);
        }

        #[test]
        #[should_panic(expected = "attempt to divide by zero")]
        fn division_by_zero_panics() {
            let _ = div(1_i32, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_beyond_javascript_is_refused() {
        #[derive(Serialize)]
        struct State {
            id: i64,
        }
        assert!(state_json(&State { id: MAX_SAFE_INTEGER }).is_ok());
        assert!(matches!(
            state_json(&State { id: MAX_SAFE_INTEGER + 1 }),
            Err(StateError::Unsafe(_))
        ));
    }
}

#[cfg(test)]
mod event_tests {
    use rustnative_core::{
        CalendarDate, ClipboardAction, Composition, Event, KeyCode, KeyModifiers, Lifecycle,
        NodeId, Point, PointerButton, PointerButtons, PointerEvent, PointerKind, WheelDelta,
    };

    #[test]
    fn every_browser_event_reads_back_as_itself() {
        let id = || NodeId::from_key("k");
        let pointer =
            PointerEvent::new(3, PointerKind::Pen, Point::new(4, -5), std::time::Duration::ZERO)
                .with_buttons(
                    PointerButtons::none().with(PointerButton::Primary).with(PointerButton::Back),
                )
                .with_button(PointerButton::Primary)
                .with_modifiers(KeyModifiers { shift: true, ..KeyModifiers::default() })
                .with_pressure(0.25);
        let events = [
            Event::Click { target: id() },
            Event::TextChanged { target: id(), value: "héllo".into() },
            Event::Toggled { target: id(), on: true },
            Event::ValueChanged { target: id(), value: -7 },
            Event::SelectionChanged { target: id(), index: Some(2) },
            Event::TabSelected { target: id(), index: 1 },
            Event::DateChanged { target: id(), date: CalendarDate::new(2026, 9, 27).unwrap() },
            Event::KeyDown {
                target: Some(id()),
                key: KeyCode::Character('é'),
                modifiers: KeyModifiers { ctrl: true, ..KeyModifiers::default() },
            },
            Event::KeyUp {
                target: None,
                key: KeyCode::Function(5),
                modifiers: KeyModifiers::default(),
            },
            Event::KeyDown {
                target: None,
                key: KeyCode::ArrowLeft,
                modifiers: KeyModifiers::default(),
            },
            Event::FocusGained { target: id() },
            Event::PointerDown { target: id(), pointer: pointer.clone() },
            Event::PointerMove { target: id(), pointer },
            Event::PointerEnter { target: id() },
            Event::Wheel { target: id(), delta: WheelDelta::Lines { x: 0, y: -120 } },
            Event::Composition {
                target: None,
                composition: Composition::Updated { text: "にほ".into(), cursor: 2 },
            },
            Event::Composition { target: Some(id()), composition: Composition::Cancelled },
            Event::Clipboard { target: None, action: ClipboardAction::Paste { text: None } },
            Event::Lifecycle(Lifecycle::Resuming),
        ];
        for event in events {
            let json = super::event_json(&event).expect("a browser event");
            assert_eq!(super::event_from_json(&json), Some(event), "{json}");
        }
    }
}
