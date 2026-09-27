//! The Web backend of Rust Native (`PLAN.md`, Web milestones A–K).
//!
//! All application code that runs on a server is Rust; the browser receives
//! HTML, CSS, and JavaScript the framework emits at compile time, plus a
//! WebAssembly module for each subtree that opts into one. This crate is the
//! browser's side of that division:
//!
//! - [`dom`] realizes the tree as semantic elements (milestone B);
//! - [`css`] states the layout and style model in CSS (milestone C);
//! - [`html`] writes the elements as HTML, and [`page`] renders a page for a
//!   request — whole, streamed ([`pending`](mod@pending)), or partially prerendered —
//!   with its islands, forms ([`form`]), and live subtrees ([`live`])
//!   (milestone H);
//! - [`svg`] and [`png`] carry drawn and pixel content into the document.
//!
//! ```
//! use rustnative_core::{Node, Theme};
//!
//! let view = Node::column("notes", [Node::label("title", "Notes"), Node::button("add", "Add")]);
//! let rendered = rustnative_web::render(&view, &Theme::default());
//! assert!(rendered.html.starts_with("<div id=\"notes\" class=\"rn rn-col"));
//! assert!(rendered.html.contains("<button type=\"button\" id=\"add\""));
//! assert!(rendered.css(&Theme::default()).contains(".rn-col{display:flex;flex-direction:column}"));
//!
//! // The same view in markup renders the same document:
//! let markup = rustnative_core::rsx! {
//!     <Column key="notes">
//!         <Label key="title" text="Notes" />
//!         <Button key="add" text="Add" />
//!     </Column>
//! };
//! assert_eq!(rustnative_web::render(&markup, &Theme::default()), rendered);
//! ```
//!
//! Server-only code stays on the server at compile time (`C05-2`): a
//! `#[server]` function's definition is there for every caller, but its body
//! exists only with the crate's `server` feature, so a build of the client
//! side cannot reach it.
//!
//! ```
//! #[rustnative_web::server]
//! pub async fn add_one(value: i32) -> Result<i32, String> {
//!     Ok(value + 1)
//! }
//!
//! use rustnative_core::server_fn::ServerFn;
//! assert_eq!(AddOne::PATH, "add_one");
//! ```
//!
//! ```compile_fail
//! #[rustnative_web::server]
//! pub async fn add_one(value: i32) -> Result<i32, String> {
//!     Ok(value + 1)
//! }
//!
//! // error[E0425]: cannot find function `add_one` in this scope
//! let _ = add_one(1);
//! ```
#![deny(missing_docs)]

pub mod capability;
pub mod client;
pub mod css;
pub mod dom;
pub mod form;
pub mod hash;
pub mod head;
pub mod html;
pub mod jsnode;
pub mod live;
pub mod page;
pub mod pending;
pub mod png;
pub mod request;
pub mod runtime;
pub mod svg;

pub use client::{Client, ClientLogic, ClientModule, Effects};
pub use head::Head;
pub use page::{Page, PageContext, Strategy};
pub use pending::pending;
pub use request::{HostLimits, RequestInfo, request};
/// Client logic compiled to JavaScript: see [client](mod@client).
pub use rustnative_web_macros::client;
/// A typed server function from one `async fn`; see `rustnative_webgen::server`.
pub use rustnative_web_macros::server;

use rustnative_core::{Node, Theme};

/// A tree rendered for the browser: its markup and the rules it needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// The root element as HTML.
    pub html: String,
    /// The rules the markup's classes name.
    pub sheet: css::StyleSheet,
}

impl Rendered {
    /// The complete stylesheet for this markup under `theme`: the base
    /// rules, the theme's, the tokens referred to, and the collected rules.
    #[must_use]
    pub fn css(&self, theme: &Theme) -> String {
        self.sheet.css(theme)
    }
}

/// Renders `view` as the page's root, with element ids unprefixed.
///
/// `view` should be a tree's unresolved output
/// ([`rustnative_core::ComponentTree::unresolved_view`]): the browser
/// evaluates declarations' conditions itself.
#[must_use]
pub fn render(view: &Node, theme: &Theme) -> Rendered {
    let _ = theme;
    let mut sheet = css::StyleSheet::new();
    let element = dom::Realizer::new(&mut sheet, "", view).root(view);
    Rendered { html: html::render(&element), sheet }
}
