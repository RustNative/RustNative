//! The markup formatter: one style for both carriers.
//!
//! Formatting works from the parsed tree plus the original source text:
//! element structure is re-laid-out, while every Rust expression, pattern,
//! and literal is copied byte for byte from what the developer wrote (by
//! the spans the parser recorded), so formatting never rewrites Rust it
//! does not own — `rustfmt` owns that.

use proc_macro2::Span;
use syn::spanned::Spanned;

use crate::ast::{AttrValue, Child, Element, ElseBranch, IfChild, Markup};

/// The widest line the formatter produces before breaking an element's
/// attributes onto their own lines.
pub const MAX_WIDTH: usize = 100;

/// Formats `markup`, whose spans index into `source`, at `indent` spaces.
/// The first line is not indented (it continues wherever the markup
/// started); following lines are.
#[must_use]
pub fn format_markup(markup: &Markup, source: &str, indent: usize) -> String {
    let mut out = String::new();
    if let Some(context) = &markup.context {
        out.push_str("in ");
        out.push_str(&text_of(source, context.span()));
        out.push_str(",\n");
        out.push_str(&" ".repeat(indent));
    }
    format_element(&markup.root, source, indent, &mut out);
    out
}

/// Formats one element (what a `.rsx` file contains at each markup site).
#[must_use]
pub fn format_element_at(element: &Element, source: &str, indent: usize) -> String {
    let mut out = String::new();
    format_element(element, source, indent, &mut out);
    out
}

fn text_of(source: &str, span: Span) -> String {
    let range = span.byte_range();
    source.get(range).map_or_else(|| span.source_text().unwrap_or_default(), str::to_owned)
}

fn joined_span(first: Span, last: Span) -> Span {
    first.join(last).unwrap_or(first)
}

fn attr_text(source: &str, name: &str, value: &AttrValue) -> String {
    match value {
        AttrValue::Flag => name.to_owned(),
        AttrValue::Lit(lit) => format!("{name}={}", text_of(source, lit.span())),
        AttrValue::Expr(expr) => format!("{name}={{{}}}", text_of(source, expr.span())),
    }
}

fn format_element(element: &Element, source: &str, indent: usize, out: &mut String) {
    let name = element.name_string();
    let mut attrs: Vec<String> = element
        .attrs
        .iter()
        .map(|attr| attr_text(source, &attr.name.to_string(), &attr.value))
        .collect();
    attrs.extend(
        element.spreads.iter().map(|spread| format!("..{{{}}}", text_of(source, spread.span()))),
    );
    let closing = if element.children.is_empty() { " />" } else { ">" };
    let one_line = if attrs.is_empty() {
        format!("<{name}{closing}")
    } else {
        format!("<{name} {}{closing}", attrs.join(" "))
    };
    let pad = " ".repeat(indent);
    if indent + one_line.len() <= MAX_WIDTH {
        out.push_str(&one_line);
    } else {
        out.push('<');
        out.push_str(&name);
        for attr in &attrs {
            out.push('\n');
            out.push_str(&pad);
            out.push_str("    ");
            out.push_str(attr);
        }
        out.push('\n');
        out.push_str(&pad);
        out.push_str(if element.children.is_empty() { "/>" } else { ">" });
    }
    if element.children.is_empty() {
        return;
    }
    format_children(&element.children, source, indent + 4, out);
    out.push('\n');
    out.push_str(&pad);
    out.push_str("</");
    out.push_str(&name);
    out.push('>');
}

fn format_children(children: &[Child], source: &str, indent: usize, out: &mut String) {
    let pad = " ".repeat(indent);
    for child in children {
        out.push('\n');
        out.push_str(&pad);
        format_child(child, source, indent, out);
    }
}

fn format_block(children: &[Child], source: &str, indent: usize, out: &mut String) {
    out.push('{');
    format_children(children, source, indent + 4, out);
    out.push('\n');
    out.push_str(&" ".repeat(indent));
    out.push('}');
}

fn format_child(child: &Child, source: &str, indent: usize, out: &mut String) {
    match child {
        Child::Element(element) => format_element(element, source, indent, out),
        Child::Expr(expr) => {
            out.push('{');
            out.push_str(&text_of(source, expr.span()));
            out.push('}');
        }
        Child::Fragment(children) => {
            out.push_str("<>");
            format_children(children, source, indent + 4, out);
            out.push('\n');
            out.push_str(&" ".repeat(indent));
            out.push_str("</>");
        }
        Child::If(branch) => format_if(branch, source, indent, out),
        Child::Match { expr, arms } => {
            out.push_str("match ");
            out.push_str(&text_of(source, expr.span()));
            out.push_str(" {");
            let pad = " ".repeat(indent + 4);
            for arm in arms {
                out.push('\n');
                out.push_str(&pad);
                out.push_str(&text_of(source, arm.pat.span()));
                if let Some(guard) = &arm.guard {
                    out.push_str(" if ");
                    out.push_str(&text_of(source, guard.span()));
                }
                out.push_str(" => ");
                format_block(&arm.body, source, indent + 4, out);
            }
            out.push('\n');
            out.push_str(&" ".repeat(indent));
            out.push('}');
        }
        Child::For { pat, expr, body } => {
            out.push_str("for ");
            out.push_str(&text_of(source, pat.span()));
            out.push_str(" in ");
            out.push_str(&text_of(source, expr.span()));
            out.push(' ');
            format_block(body, source, indent, out);
        }
    }
}

fn format_if(branch: &IfChild, source: &str, indent: usize, out: &mut String) {
    out.push_str("if ");
    out.push_str(&text_of(source, branch.cond.span()));
    out.push(' ');
    format_block(&branch.then, source, indent, out);
    match branch.otherwise.as_deref() {
        None => {}
        Some(ElseBranch::If(next)) => {
            out.push_str(" else ");
            format_if(next, source, indent, out);
        }
        Some(ElseBranch::Block(children)) => {
            out.push_str(" else ");
            format_block(children, source, indent, out);
        }
    }
}

/// The span from an element's opening `<` to its final `>`.
#[must_use]
pub fn element_span(element: &Element) -> Span {
    joined_span(element.open_span, element.close_span)
}
