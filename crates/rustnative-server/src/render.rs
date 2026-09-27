//! Server rendering: a component tree as an HTML document, so the same
//! view a native client realizes can be served to a browser as a page.
//!
//! The document is `rustnative_web`'s: every node becomes its semantic
//! element (a label a `<span>`, a button a `<button>`, a column a flex
//! `<div>`), with its accessible name and role; text is escaped; layout and
//! style are a stylesheet carrying the response's CSP nonce, so the strict
//! policy holds. This function serves a static document; a page with
//! interactive subtrees is a `rustnative_web` page (`crate::web`).
//!
//! ```
//! use rustnative_core::Node;
//! use rustnative_server::head::Head;
//! use rustnative_server::render::page;
//!
//! let view = Node::column("notes", [Node::label("title", "<Notes>"), Node::button("add", "Add")]);
//! let html = page(&Head::new("Notes", "Your notes, on every device."), &view, "n0nce");
//! assert!(html.as_str().contains("<span id=\"title\"") && html.as_str().contains(">&lt;Notes&gt;</span>"));
//! assert!(html.as_str().contains("<style nonce=\"n0nce\">"));
//! ```

use rustnative_core::{Node, Theme};

use crate::head::Head;
use crate::response::{Html, escape_into};

/// `view` as a complete HTML document with `head`, its stylesheet carrying
/// the response's CSP `nonce`.
#[must_use]
pub fn page(head: &Head, view: &Node, nonce: &str) -> Html {
    let theme = Theme::default();
    let rendered = rustnative_web::render(view, &theme);
    let mut document = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\">",
    );
    document.push_str(head.render(nonce).as_str());
    document.push_str("<style nonce=\"");
    escape_into(&mut document, nonce);
    document.push_str("\">");
    document.push_str(&rendered.css(&theme));
    document.push_str("</style></head><body>");
    document.push_str(&rendered.html);
    document.push_str("</body></html>");
    Html::from_escaped(document)
}
