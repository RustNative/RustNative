//! Server rendering: a component tree as an HTML document, so the same
//! view a native client realizes can be served to a browser as a page.
//!
//! Every node becomes its semantic element (a label a `<span>`, a button a
//! `<button>`, a column a flex `<div>`), with its accessible name and role;
//! text is escaped; layout is a stylesheet carrying the response's CSP
//! nonce, so the strict policy holds. Interactivity in the browser is the
//! Web track's (owed); a server-rendered page works as a document and as
//! plain forms.
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

use std::collections::BTreeSet;
use std::fmt::Write as _;

use rustnative_core::{AccessibilityRole, Control, Node, SizeMode};

use crate::head::Head;
use crate::response::{Html, escape_into};

struct Out {
    html: String,
    classes: BTreeSet<String>,
}

fn attribute(out: &mut String, name: &str, value: &str) {
    let _ = write!(out, " {name}=\"");
    escape_into(out, value);
    out.push('"');
}

fn common(out: &mut Out, node: &Node) {
    common_with(out, node, &[]);
}

/// The attributes every element carries, with `classes` joined into the
/// one `class` attribute.
fn common_with(out: &mut Out, node: &Node, classes: &[String]) {
    let key = node.id().local_key().unwrap_or_default();
    if !key.is_empty() {
        attribute(&mut out.html, "id", &key);
    }
    let info = node.accessibility();
    if let Some(name) = info.name_hint() {
        attribute(&mut out.html, "aria-label", name);
    }
    let role = match info.role() {
        AccessibilityRole::Heading { level } => Some(format!("heading\" aria-level=\"{level}")),
        AccessibilityRole::List => Some("list".into()),
        AccessibilityRole::ListItem => Some("listitem".into()),
        AccessibilityRole::Table => Some("table".into()),
        AccessibilityRole::Dialog => Some("dialog".into()),
        AccessibilityRole::Status => Some("status".into()),
        AccessibilityRole::Alert => Some("alert".into()),
        _ => None,
    };
    if let Some(role) = role {
        // The role strings are this function's own; nothing to escape.
        let _ = write!(out.html, " role=\"{role}\"");
    }
    if node.is_hidden() {
        out.html.push_str(" hidden");
    }
    if node.is_disabled() {
        out.html.push_str(" disabled");
    }
    let mut all = classes.to_vec();
    let layout = node.layout();
    for (axis, mode) in [("w", layout.width), ("h", layout.height)] {
        match mode {
            SizeMode::Fixed(pixels) => all.push(format!("rn-{axis}{pixels}")),
            SizeMode::Fill => all.push(format!("rn-{axis}fill")),
            SizeMode::Auto => {}
        }
    }
    if !all.is_empty() {
        let _ = write!(out.html, " class=\"{}\"", all.join(" "));
        out.classes.extend(all);
    }
}

fn node(out: &mut Out, node: &Node) {
    let text = |out: &mut Out, value: &str| escape_into(&mut out.html, value);
    match node {
        Node::Label(label) => {
            out.html.push_str("<span");
            common(out, node);
            out.html.push('>');
            text(out, label.text());
            out.html.push_str("</span>");
        }
        Node::Button(button) => {
            out.html.push_str("<button");
            common(out, node);
            out.html.push('>');
            text(out, button.text());
            out.html.push_str("</button>");
        }
        Node::TextInput(input) => {
            out.html.push_str("<input");
            common(out, node);
            attribute(&mut out.html, "name", &node.id().local_key().unwrap_or_default());
            attribute(&mut out.html, "value", input.value());
            out.html.push('>');
        }
        Node::TabBar(bar) => {
            out.html.push_str("<nav role=\"tablist\"");
            common(out, node);
            out.html.push('>');
            for (index, label) in bar.tabs().labels().iter().enumerate() {
                let selected = index == bar.tabs().selected();
                let _ = write!(out.html, "<button role=\"tab\" aria-selected=\"{selected}\">");
                text(out, label);
                out.html.push_str("</button>");
            }
            out.html.push_str("</nav>");
        }
        Node::Control(_) => control(out, node),
        Node::Canvas(_) | Node::Surface(_) => {
            // Drawn or native content has no document form; its accessible
            // name stands in.
            out.html.push_str("<div");
            common(out, node);
            out.html.push_str("></div>");
        }
        Node::Column(column) => container(
            out,
            node,
            "rn-col",
            column.children(),
            node.column_style().map(|style| style.gap),
        ),
        Node::Row(row) => {
            container(out, node, "rn-row", row.children(), node.row_style().map(|style| style.gap));
        }
    }
}

fn container(out: &mut Out, owner: &Node, kind: &str, children: &[Node], gap: Option<i32>) {
    let gap_class = format!("rn-gap{}", gap.unwrap_or(0).max(0));
    out.html.push_str("<div");
    common_with(out, owner, &[kind.to_owned(), gap_class]);
    out.html.push('>');
    for child in children {
        node(out, child);
    }
    out.html.push_str("</div>");
}

fn control(out: &mut Out, owner: &Node) {
    let Some(control) = owner.control_state() else { return };
    match control {
        Control::Checkbox { label, checked } | Control::Toggle { label, on: checked } => {
            out.html.push_str("<label><input type=\"checkbox\"");
            common(out, owner);
            if *checked {
                out.html.push_str(" checked");
            }
            out.html.push('>');
            escape_into(&mut out.html, label);
            out.html.push_str("</label>");
        }
        Control::Progress { percent: value } => {
            out.html.push_str("<progress max=\"100\"");
            common(out, owner);
            if let Some(value) = value {
                let _ = write!(out.html, " value=\"{value}\"");
            }
            out.html.push_str("></progress>");
        }
        Control::Separator => out.html.push_str("<hr>"),
        Control::Link { text } => {
            out.html.push_str("<a href=\"#\"");
            common(out, owner);
            out.html.push('>');
            escape_into(&mut out.html, text);
            out.html.push_str("</a>");
        }
        _ => {
            out.html.push_str("<div");
            common(out, owner);
            out.html.push_str("></div>");
        }
    }
}

/// `view` as a complete HTML document with `head`, its stylesheet carrying
/// the response's CSP `nonce`.
#[must_use]
pub fn page(head: &Head, view: &Node, nonce: &str) -> Html {
    let mut out = Out { html: String::new(), classes: BTreeSet::new() };
    node(&mut out, view);
    let mut css = String::from(
        ".rn-col{display:flex;flex-direction:column}.rn-row{display:flex;flex-direction:row}\
         .rn-wfill{flex:1 1 auto;width:100%}.rn-hfill{flex:1 1 auto}",
    );
    for class in &out.classes {
        if let Some(pixels) = class.strip_prefix("rn-gap") {
            let _ = write!(css, ".{class}{{gap:{pixels}px}}");
        } else if let Some(pixels) =
            class.strip_prefix("rn-w").filter(|rest| rest.parse::<i32>().is_ok())
        {
            let _ = write!(css, ".{class}{{width:{pixels}px}}");
        } else if let Some(pixels) =
            class.strip_prefix("rn-h").filter(|rest| rest.parse::<i32>().is_ok())
        {
            let _ = write!(css, ".{class}{{height:{pixels}px}}");
        }
    }
    let mut document = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\">",
    );
    document.push_str(head.render(nonce).as_str());
    document.push_str("<style nonce=\"");
    escape_into(&mut document, nonce);
    document.push_str("\">");
    document.push_str(&css);
    document.push_str("</style></head><body>");
    document.push_str(&out.html);
    document.push_str("</body></html>");
    Html::from_escaped(document)
}
