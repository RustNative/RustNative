//! Writing the browser's tree as HTML text.
//!
//! One escaper serves every text node and every attribute value, and
//! [`Element`] has no way to carry unescaped markup, so there is no path by
//! which application text becomes markup (`W-SF-5`: escaping that cannot
//! be accidentally bypassed). Whitespace is never added between elements:
//! the parsed document has exactly the element and text nodes the tree
//! has, which is what lets the runtime attach to it by walking both.

use std::fmt::Write as _;

use crate::dom::{Child, Element};

/// Elements with no content and no end tag.
fn is_void(tag: &str) -> bool {
    matches!(tag, "input" | "hr" | "img" | "br" | "meta" | "link" | "source" | "col" | "wbr")
}

/// Appends `text` escaped for an HTML text node or a quoted attribute
/// value.
pub fn escape_into(out: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
}

/// `text` escaped for HTML.
#[must_use]
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    escape_into(&mut out, text);
    out
}

/// Appends `element` as HTML.
pub fn render_into(out: &mut String, element: &Element) {
    out.push('<');
    out.push_str(&element.tag);
    for (name, value) in &element.attrs {
        out.push(' ');
        out.push_str(name);
        if !value.is_empty() {
            out.push_str("=\"");
            escape_into(out, value);
            out.push('"');
        }
    }
    out.push('>');
    if is_void(&element.tag) {
        return;
    }
    for child in &element.children {
        match child {
            Child::Text(text) => escape_into(out, text),
            Child::Element(child) => render_into(out, child),
        }
    }
    let _ = write!(out, "</{}>", element.tag);
}

/// `element` as HTML.
///
/// ```
/// use rustnative_web::dom::Element;
/// use rustnative_web::html::render;
///
/// let element = Element::new("span").attr("title", "a \"b\"").text("<script>");
/// assert_eq!(render(&element), "<span title=\"a &quot;b&quot;\">&lt;script&gt;</span>");
/// ```
#[must_use]
pub fn render(element: &Element) -> String {
    let mut out = String::new();
    render_into(&mut out, element);
    out
}

/// A JSON value embedded in a `<script type="application/json">`: no
/// `</script`, `<!--`, or line/paragraph separator can end the element or
/// confuse a parser, because every `<`, `>`, `&`, U+2028, and U+2029 is a
/// JSON escape instead.
#[must_use]
pub fn json_for_script(value: &serde_json::Value) -> String {
    let text = value.to_string();
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn void_elements_have_no_end_tag_and_flags_no_value() {
        let input = Element::new("input").attr("value", "x").flag("disabled", true);
        assert_eq!(render(&input), "<input value=\"x\" disabled>");
    }

    #[test]
    fn script_json_cannot_close_its_element() {
        let value = serde_json::json!({ "text": "</script><!-- & \u{2028}" });
        let json = json_for_script(&value);
        assert!(!json.contains('<') && !json.contains('>') && !json.contains('&'));
        let back: serde_json::Value = serde_json::from_str(&json).expect("still JSON");
        assert_eq!(back, value);
    }
}
