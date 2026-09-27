//! Boundaries a page streams (Web milestone H): a subtree whose data is
//! still loading shows its fallback in the first bytes the browser gets,
//! and the subtree replaces it in place when the data arrives — on the same
//! response, with no request from the browser.
//!
//! ```
//! use rustnative_core::{Component, ComponentContext, Event, Node};
//! use rustnative_web::pending::pending;
//!
//! struct Notes { loaded: Option<Vec<String>> }
//!
//! impl Component for Notes {
//!     type Props = ();
//!     type Message = Vec<String>;
//!     fn new(_: ()) -> Self { Self { loaded: None } }
//!     fn props(&self) -> &() { &() }
//!     fn set_props(&mut self, _: ()) {}
//!     fn view(&self) -> Node { Node::column("page", []) }
//!     fn update(&mut self, _: Event) {}
//!     fn message(&mut self, notes: Vec<String>) { self.loaded = Some(notes); }
//!     fn render(&mut self, context: &mut ComponentContext<'_, Vec<String>>) -> Node {
//!         let notes = self.loaded.as_ref().map(|notes| {
//!             Node::column("list", notes.iter().enumerate().map(|(i, note)| Node::label(format!("n{i}"), note.clone())))
//!         });
//!         Node::column("page", [
//!             Node::label("title", "Notes"),
//!             pending(context, "notes", notes, || Node::label("loading", "Loading…")),
//!         ])
//!     }
//! }
//! ```

use rustnative_core::{AccessibilityInfo, AccessibilityRole, ComponentContext, Node};

use crate::client::ServerRender;

/// `loaded` content in a boundary keyed `key` — or, until there is some,
/// `fallback`, marked busy for assistive technology. In a streamed page
/// render, the fallback is sent first and the content streamed when it
/// arrives; everywhere else this is just the content or the fallback.
pub fn pending<M: Send + 'static>(
    context: &ComponentContext<'_, M>,
    key: &str,
    loaded: Option<Node>,
    fallback: impl FnOnce() -> Node,
) -> Node {
    let resolved = loaded.is_some();
    if let Some(render) = context.services().extension::<ServerRender>() {
        render.note_pending(crate::client::global_id(context.id(), key), resolved);
    }
    let inner = loaded.unwrap_or_else(fallback);
    let boundary = Node::column(key, [inner]);
    if resolved {
        boundary
    } else {
        boundary.with_accessibility(AccessibilityInfo::new(AccessibilityRole::Group).busy(true))
    }
}
