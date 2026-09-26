//! Inspection and diagnostics (`PLAN.md` Milestone 44): one protocol the
//! runtime answers, whatever the backend.
//!
//! A compiled framework does not inherit introspection from a runtime, so
//! it exposes its own. The protocol covers the declarative tree and the
//! components that produced it, the realized host objects and the mapping
//! between the two, component state (readable, and editable where a
//! component says how — see [`crate::Component::edit`]), a per-node
//! explanation of layout and of style provenance, event and render tracing
//! with every component's render-or-skip reason, tasks, host-object
//! lifetimes, capabilities and what was refused, active mapper
//! customizations, state history, and recording.
//!
//! [`crate::Application::inspect`] answers a [`Request`]; what only the
//! backend knows (its objects, their rectangles, its capability tables)
//! comes through [`InspectBackend`]. [`InspectServer`] carries requests
//! over loopback TCP (or any address the host binds explicitly) as
//! line-delimited JSON, authenticated by a per-process token; the
//! `rustnative inspect` client speaks it. [`compact`] is the reduced form
//! for targets with no second screen: numeric message ids and arguments,
//! formatted on the host side.
//!
//! ```
//! use rustnative_core::inspect::{NoBackend, Reply, Request};
//! use rustnative_core::{Application, Component, Event, Node, Size, Window};
//!
//! struct Counter(u32);
//! impl Component for Counter {
//!     type Props = ();
//!     type Message = u32;
//!     fn new((): ()) -> Self { Self(0) }
//!     fn props(&self) -> &() { &() }
//!     fn set_props(&mut self, (): ()) {}
//!     fn view(&self) -> Node { Node::label("count", format!("{}", self.0)) }
//!     fn update(&mut self, _: Event) {}
//!     fn message(&mut self, value: u32) { self.0 = value; }
//!     fn inspect(&self) -> Option<serde_json::Value> { Some(serde_json::json!({ "count": self.0 })) }
//!     fn edit(&self, field: &str, value: &serde_json::Value) -> Option<u32> {
//!         (field == "count").then(|| value.as_u64()).flatten().and_then(|v| u32::try_from(v).ok())
//!     }
//! }
//!
//! let mut app = Application::new(Counter(0), Window::new("Counter", Size::new(200, 100)));
//! let path = app.components().inspect_components()[0].path.clone();
//! let edit = Request::SetState { path, field: "count".into(), value: 7.into(), window: None };
//! assert!(matches!(app.inspect(&edit, &NoBackend), Reply::Ok(_)));
//! assert_eq!(app.view(), Node::label("count", "7"));
//! // The same view, in markup:
//! assert_eq!(app.view(), rustnative_core::rsx! { <Label key="count" text="7" /> });
//! ```

mod answer;
pub mod compact;
mod overlay;
mod record;
mod server;
mod trace;

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::capability::PlatformCapabilities;
use crate::identity::{NodeId, WindowId};
use crate::layout::Rect;

pub use answer::INSPECT_VARIABLE;
pub use overlay::OverlayMode;
pub use record::{
    HttpTape, NodeRef, RecordedEvent, RecordedHttp, RecordedInput, Recording, RecordingHttp,
    ReplayError,
};
pub use server::{Endpoint, InspectServer, endpoint_directory, new_token, request as send_request};
pub(crate) use trace::Inspection;
pub use trace::{PassInfo, RenderInfo, TraceEntry, TraceKind};

/// The protocol's version. A client sends it with every request; a server
/// refuses a version it does not speak rather than answering it wrongly.
pub const PROTOCOL_VERSION: u32 = 1;

/// One question for the runtime.
///
/// `window` defaults to the primary window. A node is named by its key (the
/// first match in document order), by `component path::key` to pick one
/// component's node, or by its numeric id as the tree reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Request {
    /// The protocol version, the backend, and the open windows.
    Hello,
    /// The declarative tree, with the component that rendered each node.
    Tree {
        /// The window.
        #[serde(default)]
        window: Option<u64>,
    },
    /// Every component: path, type, parent, state, and task count.
    Components {
        /// The window.
        #[serde(default)]
        window: Option<u64>,
    },
    /// The host objects realizing the tree, and which node each realizes.
    Realized {
        /// The window.
        #[serde(default)]
        window: Option<u64>,
    },
    /// One component's state.
    State {
        /// The component's key path, as [`Request::Components`] lists it.
        path: String,
        /// The window.
        #[serde(default)]
        window: Option<u64>,
    },
    /// Sets one field of a component's state, through the message the
    /// component names for it (see [`crate::Component::edit`]).
    SetState {
        /// The component's key path.
        path: String,
        /// The field.
        field: String,
        /// The new value.
        value: Value,
        /// The window.
        #[serde(default)]
        window: Option<u64>,
    },
    /// Why a node has the geometry it has.
    ExplainLayout {
        /// The node.
        node: String,
        /// The window.
        #[serde(default)]
        window: Option<u64>,
    },
    /// Where each of a node's style properties came from.
    ExplainStyle {
        /// The node.
        node: String,
        /// The window.
        #[serde(default)]
        window: Option<u64>,
    },
    /// Events, task deliveries, and every component's render-or-skip
    /// reason, since sequence number `since`.
    Trace {
        /// The first sequence number wanted.
        #[serde(default)]
        since: u64,
    },
    /// Tasks, by owning component.
    Tasks {
        /// The window.
        #[serde(default)]
        window: Option<u64>,
    },
    /// Host objects created and destroyed.
    Lifetimes,
    /// What the host advertises, and what it refused and why.
    Capabilities,
    /// Active per-property mapper customizations (`C24-2`).
    Mappers,
    /// The recorded state history of inspectable components, oldest first.
    History {
        /// Only this component's state, by key path.
        #[serde(default)]
        component: Option<String>,
    },
    /// Shows (or, with `None`, hides) the in-application overlay.
    Overlay {
        /// What the overlay draws.
        #[serde(default)]
        mode: Option<OverlayMode>,
    },
    /// Starts recording input and service responses.
    StartRecording {
        /// Node keys whose text is recorded as `[redacted]`; any key
        /// containing one of these matches.
        #[serde(default)]
        redact: Vec<String>,
    },
    /// Stops recording, answering the [`Recording`].
    StopRecording,
    /// Re-resolves the running application's style against a new style
    /// file's tokens, without a rebuild — the development loop's live theme
    /// edit (Milestone 43). Only token values may differ from the file the
    /// application was built with; a new or removed utility is a rebuild.
    SetStyleFile {
        /// The style file's text.
        css: String,
    },
    /// Replaces one locale's catalogue file in the running application —
    /// the development loop's live translation edit (Milestone 46).
    SetCatalogue {
        /// The locale.
        locale: String,
        /// The file's text.
        ftl: String,
    },
    /// Asks the application to close, as the person closing its window
    /// would: state is flushed and placement saved.
    Quit,
    /// The live inspectable stores (`crate::Store::inspectable`) and their
    /// values.
    Stores,
    /// A server's durable jobs (`rustnative_server::jobs`), newest first:
    /// kind, status, attempts, and last error. An application process has
    /// none and says so.
    Jobs,
}

/// The runtime's answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reply {
    /// The answer: one of this module's types, as JSON.
    Ok(Value),
    /// Why there is no answer.
    Error(String),
}

impl Reply {
    fn of(value: &impl Serialize) -> Self {
        serde_json::to_value(value).map_or_else(|error| Self::Error(error.to_string()), Self::Ok)
    }
}

/// [`Request::Hello`]'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// [`PROTOCOL_VERSION`].
    pub version: u32,
    /// The backend's name.
    pub backend: String,
    /// The open windows' ids, the primary first.
    pub windows: Vec<u64>,
}

/// One node of the declarative tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeInfo {
    /// The node's id, in decimal.
    pub id: String,
    /// The key its author wrote.
    pub key: Option<String>,
    /// Its kind.
    pub kind: String,
    /// Its text, for text-bearing nodes.
    pub text: Option<String>,
    /// The key path of the component that rendered it.
    pub component: String,
    /// The class and declaration sets applied to it, as written.
    pub classes: Vec<String>,
    /// Whether it is hidden.
    pub hidden: bool,
    /// Its children.
    pub children: Vec<NodeInfo>,
}

/// One component.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComponentInfo {
    /// The component's id.
    pub id: u64,
    /// Its key path from the window's root: what persisted state, the
    /// render trace, and state editing name it by.
    pub path: String,
    /// Its Rust type.
    pub type_name: String,
    /// Its parent's id.
    pub parent: Option<u64>,
    /// Its state, if it shows any (see [`crate::Component::inspect`]).
    pub state: Option<Value>,
    /// Tasks it owns that have not finished.
    pub tasks: usize,
}

/// One host object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealizedObject {
    /// The node it realizes, in decimal.
    pub node: String,
    /// That node's key.
    pub key: Option<String>,
    /// The host's type for it (a window class, a view type).
    pub host_type: String,
    /// The host's handle for it, if it has one.
    pub handle: Option<String>,
    /// Its rectangle in its parent's coordinates, as the host has it.
    pub rect: Option<[i32; 4]>,
}

/// Why a node has its geometry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutExplanation {
    /// The node, in decimal.
    pub node: String,
    /// Its key.
    pub key: Option<String>,
    /// Its rectangle in its parent's coordinates: x, y, width, height.
    pub rect: [i32; 4],
    /// Whether the rectangle is the backend's (`true`) or the shared
    /// engine's, computed for the window's size (`false`).
    pub realized: bool,
    /// The reasons, outermost first.
    pub reasons: Vec<String>,
}

/// Where one style property's value came from, by precedence level
/// (`C18-2`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "level", rename_all = "snake_case")]
pub enum Origin {
    /// A utility class.
    Class {
        /// The class, as written.
        class: String,
    },
    /// A declaration written with `styles!` (or `style=`).
    Declaration {
        /// The declaration, as written.
        text: String,
    },
    /// A typed override set in code (`.with_style(…)`).
    TypedOverride,
    /// The theme's default for the node's kind: the component default.
    ComponentDefault {
        /// The kind whose default it is.
        kind: String,
    },
}

/// One style property's value and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StyleSource {
    /// The property.
    pub property: String,
    /// Its value as written (a token reads `var(--name)`).
    pub value: String,
    /// The value after the theme's tokens, when it differs.
    pub resolved: Option<String>,
    /// Where it came from.
    pub origin: Origin,
    /// The condition it is written under (`hover:`, `dark:`, `md:`…).
    pub condition: Option<String>,
    /// Whether it applies now. A later applying source of the same
    /// property, under the same state, wins.
    pub applies: bool,
}

/// Where each of a node's style properties came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StyleExplanation {
    /// The node, in decimal.
    pub node: String,
    /// Its key.
    pub key: Option<String>,
    /// Its kind.
    pub kind: String,
    /// Every source, in the order they are applied.
    pub sources: Vec<StyleSource>,
}

/// One component's tasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskInfo {
    /// The component's key path.
    pub component: String,
    /// Tasks it owns that have not finished; they are cancelled when it
    /// unmounts.
    pub tasks: usize,
}

/// Host objects created and destroyed.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Lifetimes {
    /// Objects created, over the process's life.
    pub created: u64,
    /// Objects destroyed.
    pub destroyed: u64,
    /// Objects alive now.
    pub live: u64,
    /// The most recent creations and destructions, oldest first.
    pub recent: Vec<LifetimeEvent>,
}

/// One host object created or destroyed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifetimeEvent {
    /// `true` for a creation.
    pub created: bool,
    /// The node, in decimal.
    pub node: String,
    /// The node's key.
    pub key: Option<String>,
    /// The host's type for it.
    pub host_type: String,
}

/// What the host advertises and what it refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityReport {
    /// The backend.
    pub backend: String,
    /// Capabilities it advertises.
    pub advertised: Vec<String>,
    /// Capabilities it does not, and services the application did not
    /// provide — each with why.
    pub refused: Vec<Refusal>,
    /// Every style property with the backend's answer (2.14).
    pub style: Vec<Refusal>,
    /// How the backend maps the vocabulary's units.
    pub units: Option<BTreeMap<String, String>>,
    /// Services provided to the application.
    pub services: Vec<String>,
}

/// Something and the reason given for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    /// What.
    pub what: String,
    /// Why, or the answer.
    pub why: String,
}

/// One active mapper customization (`C24-2`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapperEntry {
    /// What it applies to: a kind, or one node's key.
    pub target: String,
    /// The property it maps.
    pub property: String,
    /// `extend` or `replace`.
    pub mode: String,
}

/// Inspectable components' state after one change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// The trace sequence number of the change.
    pub seq: u64,
    /// What changed it.
    pub cause: String,
    /// Each inspectable component's state, by key path.
    pub states: BTreeMap<String, Value>,
}

/// What only the backend knows. Every method has a default meaning "this
/// backend has nothing to say", so a backend answers what it can.
pub trait InspectBackend {
    /// The backend's name.
    fn name(&self) -> &'static str;

    /// The host objects realizing `window`'s tree.
    fn realized(&self, _window: WindowId) -> Vec<RealizedObject> {
        Vec::new()
    }

    /// The rectangles the backend laid `window`'s nodes out at, in their
    /// parents' coordinates. `None` has the shared engine compute them.
    fn rects(&self, _window: WindowId) -> Option<HashMap<NodeId, Rect>> {
        None
    }

    /// Host objects created and destroyed.
    fn lifetimes(&self) -> Lifetimes {
        Lifetimes::default()
    }

    /// What the host advertises.
    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::default()
    }

    /// The backend's style capability table.
    fn style_capabilities(&self) -> rustnative_style::StyleCapabilities {
        rustnative_style::StyleCapabilities::NONE
    }

    /// The backend's unit mapping.
    fn unit_mapping(&self) -> Option<rustnative_style::UnitMapping> {
        None
    }

    /// Active mapper customizations.
    fn mappers(&self) -> Vec<MapperEntry> {
        Vec::new()
    }
}

/// A backend with nothing to add: the core answers alone.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoBackend;

impl InspectBackend for NoBackend {
    fn name(&self) -> &'static str {
        "core"
    }
}

/// `id` in decimal, as the protocol names nodes.
#[must_use]
pub fn node_name(id: NodeId) -> String {
    id.get().to_string()
}
