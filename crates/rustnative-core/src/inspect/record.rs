//! Record, replay, and time travel (`C61`).
//!
//! State changes only through messages, time only through the clock
//! service, and work only through the executor — so replaying the same
//! input and the same service responses against the same program reaches
//! the same state. A [`Recording`] holds the input (by node key and owning
//! component, not by id, so it replays in another process), the HTTP
//! responses a [`RecordingHttp`] saw, and the inspectable state it ended
//! in; [`Recording::replay`] drives an [`Application`] through it, and
//! [`Recording::to_test`] writes it out as a regression test.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::application::Application;
use crate::component::ComponentTree;
use crate::event::{Event, KeyCode, KeyModifiers};
use crate::identity::{NodeId, WindowId};
use crate::layout::Size;
use crate::services::{HttpRequest, HttpResponse, HttpService, Method, ServiceError};

/// Text recorded in place of a redacted value.
const REDACTED: &str = "[redacted]";

/// Headers never recorded.
const SECRET_HEADERS: [&str; 4] = ["authorization", "cookie", "set-cookie", "proxy-authorization"];

/// A node, named the way it survives a restart: the key path of the
/// component that rendered it, relative to the window's root (`""` for the
/// root, `/child` below it), and the key its author wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeRef {
    /// The owning component's key path, relative to the root.
    pub component: String,
    /// The node's key.
    pub key: String,
}

impl NodeRef {
    fn of(tree: &ComponentTree, id: NodeId) -> Option<Self> {
        let path = tree.node_component_path(id)?;
        let component = path.strip_prefix(tree.root_path()).unwrap_or(&path).to_owned();
        Some(Self { component, key: id.local_key()? })
    }

    fn resolve(&self, tree: &ComponentTree) -> Result<NodeId, ReplayError> {
        tree.find_node(Some(&format!("{}{}", tree.root_path(), self.component)), &self.key)
            .ok_or_else(|| ReplayError::MissingNode(self.clone()))
    }
}

/// One recorded input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
#[non_exhaustive]
pub enum RecordedEvent {
    /// A click.
    Click {
        /// The target.
        node: NodeRef,
    },
    /// Focus arrived.
    FocusGained {
        /// The target.
        node: NodeRef,
    },
    /// Focus left.
    FocusLost {
        /// The target.
        node: NodeRef,
    },
    /// A text field's value changed.
    TextChanged {
        /// The target.
        node: NodeRef,
        /// The new value, or `[redacted]`.
        value: String,
    },
    /// Text typed.
    TextInput {
        /// The target.
        node: Option<NodeRef>,
        /// The text, or `[redacted]`.
        text: String,
    },
    /// A key went down.
    KeyDown {
        /// The target.
        node: Option<NodeRef>,
        /// The key.
        key: KeyCode,
        /// The modifiers held.
        modifiers: KeyModifiers,
    },
    /// A key came up.
    KeyUp {
        /// The target.
        node: Option<NodeRef>,
        /// The key.
        key: KeyCode,
        /// The modifiers held.
        modifiers: KeyModifiers,
    },
    /// The window was resized.
    Resized {
        /// Its new width.
        width: u32,
        /// Its new height.
        height: u32,
    },
    /// An event this format does not replay (pointer streams, gestures,
    /// drags), kept so the recording says it happened.
    Unreplayable {
        /// The event, as debug text.
        description: String,
    },
}

/// One input and when it arrived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedInput {
    /// Milliseconds since the recording started, by the application's
    /// clock service.
    pub at_ms: u64,
    /// The input.
    pub event: RecordedEvent,
}

/// One HTTP exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedHttp {
    /// The request's method.
    pub method: String,
    /// The request's URL.
    pub url: String,
    /// The response's status, or `None` when the request failed.
    pub status: Option<u16>,
    /// The response's headers, secrets removed.
    pub headers: Vec<(String, String)>,
    /// The response's body, as UTF-8 (lossily).
    pub body: String,
    /// Why the request failed, when it did.
    pub error: Option<String>,
}

impl RecordedHttp {
    /// The request's method.
    #[must_use]
    pub fn method(&self) -> Method {
        match self.method.as_str() {
            "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "DELETE" => Method::Delete,
            "PATCH" => Method::Patch,
            "HEAD" => Method::Head,
            "OPTIONS" => Method::Options,
            other => Method::Other(other.to_owned()),
        }
    }

    /// The response, or the failure, as the service returned it.
    ///
    /// # Errors
    ///
    /// The recorded failure.
    pub fn response(&self) -> Result<HttpResponse, ServiceError> {
        match (self.status, &self.error) {
            (Some(status), _) => {
                Ok(HttpResponse::new(status, self.headers.clone(), self.body.clone().into_bytes()))
            }
            (None, error) => Err(ServiceError::new(error.clone().unwrap_or_default())),
        }
    }
}

/// A recording: input, service responses, and the state it ended in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recording {
    /// The protocol version it was recorded under.
    pub version: u32,
    /// The input, in order.
    pub inputs: Vec<RecordedInput>,
    /// The HTTP exchanges, in order.
    pub http: Vec<RecordedHttp>,
    /// The node keys whose text was redacted.
    pub redacted: Vec<String>,
    /// Each inspectable component's state when recording stopped, by key
    /// path relative to the root (see [`ComponentTree::relative_states`]).
    pub final_state: BTreeMap<String, serde_json::Value>,
}

/// Why a recording could not be replayed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReplayError {
    /// A recorded target is not in the tree at that point: the program
    /// differs from the one recorded.
    MissingNode(NodeRef),
    /// The window is not open.
    MissingWindow(WindowId),
    /// A recorded value was redacted, so replaying it would not reproduce
    /// the session.
    Redacted(NodeRef),
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingNode(node) => write!(
                f,
                "`{}` in `{}` is not in the tree when the recording reaches it",
                node.key, node.component
            ),
            Self::MissingWindow(window) => write!(f, "window {} is not open", window.get()),
            Self::Redacted(node) => write!(
                f,
                "the text entered into `{}` was redacted when recorded, so it cannot be replayed",
                node.key
            ),
        }
    }
}

impl std::error::Error for ReplayError {}

impl Recording {
    /// Replays the input into `window` of `app`, calling `between` with the
    /// time that passed before each input (a headless host advances its
    /// executor; a native one pumps). Unreplayable inputs are skipped.
    /// Returns how many inputs were replayed.
    ///
    /// # Errors
    ///
    /// The first input whose target is missing, or whose text was
    /// redacted.
    pub fn replay(
        &self,
        app: &mut Application,
        window: WindowId,
        mut between: impl FnMut(&mut Application, Duration),
    ) -> Result<usize, ReplayError> {
        let mut replayed = 0;
        let mut previous = 0;
        for input in &self.inputs {
            between(app, Duration::from_millis(input.at_ms.saturating_sub(previous)));
            previous = input.at_ms;
            let tree = app.components_for(window).ok_or(ReplayError::MissingWindow(window))?;
            let Some(event) = self.event(tree, window, &input.event)? else { continue };
            app.dispatch_to_window(window, event);
            replayed += 1;
        }
        between(app, Duration::ZERO);
        Ok(replayed)
    }

    /// The event `event` stands for in `window`, whose tree is `tree`, or
    /// `None` for an input this format does not replay — for a host that
    /// drives replay itself.
    ///
    /// # Errors
    ///
    /// The target is missing, or its text was redacted.
    pub fn event(
        &self,
        tree: &ComponentTree,
        window: WindowId,
        event: &RecordedEvent,
    ) -> Result<Option<Event>, ReplayError> {
        let optional =
            |node: &Option<NodeRef>| node.as_ref().map(|node| node.resolve(tree)).transpose();
        let text = |node: Option<&NodeRef>, text: &str| {
            if text == REDACTED && node.is_some_and(|node| self.is_redacted(&node.key)) {
                Err(ReplayError::Redacted(
                    node.cloned()
                        .unwrap_or(NodeRef { component: String::new(), key: String::new() }),
                ))
            } else {
                Ok(text.to_owned())
            }
        };
        Ok(Some(match event {
            RecordedEvent::Click { node } => Event::Click { target: node.resolve(tree)? },
            RecordedEvent::FocusGained { node } => {
                Event::FocusGained { target: node.resolve(tree)? }
            }
            RecordedEvent::FocusLost { node } => Event::FocusLost { target: node.resolve(tree)? },
            RecordedEvent::TextChanged { node, value } => {
                Event::TextChanged { target: node.resolve(tree)?, value: text(Some(node), value)? }
            }
            RecordedEvent::TextInput { node, text: typed } => {
                Event::TextInput { target: optional(node)?, text: text(node.as_ref(), typed)? }
            }
            RecordedEvent::KeyDown { node, key, modifiers } => {
                Event::KeyDown { target: optional(node)?, key: *key, modifiers: *modifiers }
            }
            RecordedEvent::KeyUp { node, key, modifiers } => {
                Event::KeyUp { target: optional(node)?, key: *key, modifiers: *modifiers }
            }
            RecordedEvent::Resized { width, height } => {
                Event::WindowResized { window, size: Size::new(*width, *height) }
            }
            RecordedEvent::Unreplayable { .. } => return Ok(None),
        }))
    }

    fn is_redacted(&self, key: &str) -> bool {
        self.redacted.iter().any(|pattern| key.contains(pattern.as_str()))
    }

    /// This recording as a regression test on the headless backend:
    /// `launch` is the expression that creates the application's root
    /// component (for example `my_app::Root::new(())`). The test replays
    /// the input and asserts every inspectable component ends in the state
    /// the recording ended in.
    #[must_use]
    pub fn to_test(&self, name: &str, launch: &str) -> String {
        let json = serde_json::to_string_pretty(self).unwrap_or_default();
        // The recording's paths are relative to the root, so the test
        // compares relative states.
        let hashes = "#".repeat(longest_hash_run(&json) + 1);
        format!(
            r#"// Generated by `rustnative inspect to-test` from a recording (`PLAN.md`
// Milestone 44, `C61`): replays the recorded input and service responses on
// the headless backend and checks the state it ends in.

#[test]
fn {name}() {{
    use rustnative_core::Component as _;

    let recording: rustnative_core::inspect::Recording = serde_json::from_str(
        r{hashes}"{json}"{hashes},
    )
    .expect("the recording is valid");
    let mut app = rustnative_headless::HeadlessApp::launch(
        rustnative_core::Window::new("{name}", rustnative_core::Size::new(800, 600)),
        || {launch},
    );
    app.replay(&recording).expect("the recording replays");
    assert_eq!(app.inspected_state(), recording.final_state);
}}
"#
        )
    }
}

fn longest_hash_run(text: &str) -> usize {
    let (mut longest, mut run) = (0, 0);
    for character in text.chars() {
        run = if character == '#' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    longest
}

/// What is being recorded.
#[derive(Debug)]
pub(crate) struct Recorder {
    started: Duration,
    redact: Vec<String>,
    inputs: Vec<RecordedInput>,
}

impl Recorder {
    pub(crate) const fn new(started: Duration, redact: Vec<String>) -> Self {
        Self { started, redact, inputs: Vec::new() }
    }

    fn redacted(&self, node: Option<&NodeRef>, text: &str) -> String {
        if node.is_some_and(|node| {
            self.redact.iter().any(|pattern| node.key.contains(pattern.as_str()))
        }) {
            REDACTED.to_owned()
        } else {
            text.to_owned()
        }
    }

    /// Records `event`, dispatched to a window whose tree is `tree`.
    pub(crate) fn record(&mut self, now: Duration, tree: &ComponentTree, event: &Event) {
        let node = |id: &NodeId| NodeRef::of(tree, *id);
        let optional = |id: &Option<NodeId>| id.as_ref().and_then(node);
        let unreplayable = || RecordedEvent::Unreplayable { description: format!("{event:?}") };
        let recorded = match event {
            Event::Click { target } => node(target).map(|node| RecordedEvent::Click { node }),
            Event::FocusGained { target } => {
                node(target).map(|node| RecordedEvent::FocusGained { node })
            }
            Event::FocusLost { target } => {
                node(target).map(|node| RecordedEvent::FocusLost { node })
            }
            Event::TextChanged { target, value } => node(target).map(|node| {
                let value = self.redacted(Some(&node), value);
                RecordedEvent::TextChanged { node, value }
            }),
            Event::TextInput { target, text } => {
                let node = optional(target);
                let text = self.redacted(node.as_ref(), text);
                Some(RecordedEvent::TextInput { node, text })
            }
            Event::KeyDown { target, key, modifiers } => Some(RecordedEvent::KeyDown {
                node: optional(target),
                key: *key,
                modifiers: *modifiers,
            }),
            Event::KeyUp { target, key, modifiers } => Some(RecordedEvent::KeyUp {
                node: optional(target),
                key: *key,
                modifiers: *modifiers,
            }),
            Event::WindowResized { size, .. } => {
                Some(RecordedEvent::Resized { width: size.width, height: size.height })
            }
            // A text field's native value is text: a password field's
            // pointer and gesture events carry none.
            _ => None,
        }
        .unwrap_or_else(unreplayable);
        let at_ms = u64::try_from(now.saturating_sub(self.started).as_millis()).unwrap_or(u64::MAX);
        self.inputs.push(RecordedInput { at_ms, event: recorded });
    }

    pub(crate) fn finish(
        self,
        http: Vec<RecordedHttp>,
        final_state: BTreeMap<String, serde_json::Value>,
    ) -> Recording {
        Recording {
            version: super::PROTOCOL_VERSION,
            inputs: self.inputs,
            http,
            redacted: self.redact,
            final_state,
        }
    }
}

/// Where a [`RecordingHttp`] writes the exchanges it sees.
#[derive(Debug, Clone, Default)]
pub struct HttpTape(Arc<Mutex<Vec<RecordedHttp>>>);

impl HttpTape {
    /// An empty tape.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Everything recorded since the last call.
    #[must_use]
    pub fn take(&self) -> Vec<RecordedHttp> {
        std::mem::take(&mut *self.0.lock())
    }
}

/// An HTTP service that records every exchange onto a tape, secrets
/// removed, while passing it through. Installed in place of the real
/// service (`Services::with_http`), with its tape handed to
/// [`Application::record_http`].
pub struct RecordingHttp {
    inner: Arc<dyn HttpService>,
    tape: HttpTape,
}

impl fmt::Debug for RecordingHttp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecordingHttp").finish_non_exhaustive()
    }
}

impl RecordingHttp {
    /// Wraps `inner`, recording onto `tape`.
    #[must_use]
    pub fn new(inner: Arc<dyn HttpService>, tape: HttpTape) -> Self {
        Self { inner, tape }
    }
}

#[async_trait::async_trait]
impl HttpService for RecordingHttp {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ServiceError> {
        let method = request.method().as_str().to_owned();
        let url = request.url().to_owned();
        let result = self.inner.execute(request).await;
        let recorded = match &result {
            Ok(response) => RecordedHttp {
                method,
                url,
                status: Some(response.status()),
                headers: response
                    .headers()
                    .iter()
                    .filter(|(name, _)| {
                        !SECRET_HEADERS.contains(&name.to_ascii_lowercase().as_str())
                    })
                    .cloned()
                    .collect(),
                body: String::from_utf8_lossy(response.body_bytes()).into_owned(),
                error: None,
            },
            Err(error) => RecordedHttp {
                method,
                url,
                status: None,
                headers: Vec::new(),
                body: String::new(),
                error: Some(error.to_string()),
            },
        };
        self.tape.0.lock().push(recorded);
        result
    }
}
