//! Render modes in the browser (`C07-1`, `C33`): a subtree held on the
//! server — its events go there over a WebSocket, and the trees it renders
//! come back and are patched in — and `Auto`, which is held on the server
//! until the browser has the component's client module, then runs it there,
//! from the server session's last state.
//!
//! [`Live`] is the component a page puts where such a subtree goes. It
//! renders the component on the server for the page's first bytes and
//! registers it as a live island; the runtime connects to `url`, where the
//! application serves the component's sessions (`rustnative-sync`'s
//! `LiveServer`, with [`encode`] as its browser encoding).
//!
//! | Mode | How |
//! |---|---|
//! | static | render the component in the page; no island |
//! | server-interactive | [`LiveProps::server`] |
//! | client-interactive | a client component (`#[client]`), [`crate::Client`] |
//! | auto | [`LiveProps::auto`] |

use rustnative_core::{Component, ComponentContext, Event, Node};
use serde::Serialize;
use serde_json::{Value, json};

use crate::client::{Client, ClientLogic, ClientModule, Island, IslandKind, ServerRender};
use crate::css::{Flow, StyleSheet};
use crate::dom::{Marks, Realizer};

/// A live subtree: the component, its initial props, where its sessions
/// are served, and the client module that takes over in `Auto` mode.
pub struct LiveProps<C: Component> {
    /// The WebSocket path of its sessions (`/_rn/live/notes`).
    pub url: String,
    /// The component's props, which are also the state a new session
    /// starts from.
    pub props: C::Props,
    /// In `Auto` mode, the client module that takes over.
    pub then: Option<&'static ClientModule>,
}

impl<C: Component> Clone for LiveProps<C> {
    fn clone(&self) -> Self {
        Self { url: self.url.clone(), props: self.props.clone(), then: self.then }
    }
}

impl<C: Component> PartialEq for LiveProps<C> {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url
            && self.props == other.props
            && self.then.map(|m| m.name) == other.then.map(|m| m.name)
    }
}

impl<C: Component> LiveProps<C> {
    /// Server-interactive: held on the server for as long as the page is
    /// open.
    pub fn server(url: impl Into<String>, props: C::Props) -> Self {
        Self { url: url.into(), props, then: None }
    }
}

impl<S: ClientLogic> LiveProps<Client<S>> {
    /// `Auto`: held on the server until the browser has the client module,
    /// then run in the browser from the session's last state.
    pub fn auto(url: impl Into<String>, state: S) -> Self {
        Self { url: url.into(), props: state, then: Some(S::MODULE) }
    }
}

/// See the [module documentation](self).
pub struct Live<C: Component> {
    props: LiveProps<C>,
}

impl<C: Component> Component for Live<C>
where
    C::Props: Serialize,
{
    type Props = LiveProps<C>;
    type Message = ();

    fn new(props: LiveProps<C>) -> Self {
        Self { props }
    }

    fn props(&self) -> &LiveProps<C> {
        &self.props
    }

    fn set_props(&mut self, props: LiveProps<C>) {
        self.props = props;
    }

    fn view(&self) -> Node {
        Node::column("live", [])
    }

    fn update(&mut self, _: Event) {}

    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let node = context.child_with_props::<C, _>("live", self.props.props.clone(), C::new);
        if let Some(render) = context.services().extension::<ServerRender>() {
            if let (Some(owner), Ok(state)) =
                (node.id().owner(), serde_json::to_value(&self.props.props))
            {
                render.register(Island {
                    owner,
                    kind: IslandKind::Live { url: self.props.url.clone(), then: self.props.then },
                    state,
                    persist: false,
                });
            }
        }
        node
    }
}

/// A live session's tree as the browser patches it: its elements and the
/// rules they need. `options` are what the browser said when it connected:
/// the island's id `scope` (`i0-`) and how its parent lays it out
/// (`flow`).
#[must_use]
pub fn encode(view: &Node, options: &Value) -> Value {
    let scope = options.get("scope").and_then(Value::as_str).unwrap_or_default();
    let flow = options.get("flow").map_or(Flow::Root, Flow::from_json);
    let mut sheet = StyleSheet::new();
    let element =
        Realizer::with_marks(&mut sheet, scope, view, &Marks::default()).element(view, flow);
    json!({ "element": element, "rules": sheet.entries() })
}
