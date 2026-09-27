//! Forms that work without JavaScript (`W-IS-3`, progressive enhancement).
//!
//! [`form`] groups inputs and buttons as a form posting to `action`. On a
//! server-rendered page it is a real `<form method="post">` carrying the
//! request-forgery token, its buttons are submit buttons named by their
//! keys, and its inputs are named by theirs — so a browser with scripts off
//! still submits it, and the handler at `action` reads the fields with the
//! server's `Form<T>` extractor. Everywhere else it is a column.
//!
//! ```
//! use rustnative_core::{Component, ComponentContext, Event, Node};
//! use rustnative_web::form::form;
//!
//! struct NewNote;
//! impl Component for NewNote {
//!     type Props = ();
//!     type Message = ();
//!     fn new(_: ()) -> Self { Self }
//!     fn props(&self) -> &() { &() }
//!     fn set_props(&mut self, _: ()) {}
//!     fn view(&self) -> Node { Node::column("new", []) }
//!     fn update(&mut self, _: Event) {}
//!     fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
//!         form(context, "new", "/notes", [Node::text_input("title", ""), Node::button("save", "Save")])
//!     }
//! }
//! ```

use rustnative_core::{ComponentContext, Node};

use crate::client::ServerRender;

/// `children` as a form keyed `key` that posts to `action`.
pub fn form<M: Send + 'static>(
    context: &ComponentContext<'_, M>,
    key: &str,
    action: &str,
    children: impl IntoIterator<Item = Node>,
) -> Node {
    if let Some(render) = context.services().extension::<ServerRender>() {
        render.note_form(crate::client::global_id(context.id(), key), action);
    }
    Node::column(key, children)
}

/// A link keyed `key` showing `text` that goes to `href`: on a
/// server-rendered page a real `<a href>`, which works without JavaScript
/// and which the runtime turns into a client-side navigation; everywhere
/// else a link node whose click is the application's to handle.
#[must_use]
pub fn link<M: Send + 'static>(
    context: &ComponentContext<'_, M>,
    key: &str,
    text: &str,
    href: &str,
) -> Node {
    if let Some(render) = context.services().extension::<ServerRender>() {
        render.note_link(crate::client::global_id(context.id(), key), href);
    }
    Node::link(key, text)
}
