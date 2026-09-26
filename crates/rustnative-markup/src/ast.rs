//! The markup syntax tree.

use proc_macro2::Span;
use syn::{Expr, Ident, Lit, Pat, Path};

/// A whole markup expression: an optional component context and one root
/// element.
#[derive(Debug, Clone)]
pub struct Markup {
    /// The `in context,` prefix, if written.
    pub context: Option<Expr>,
    /// The root element.
    pub root: Element,
}

/// One element: `<Name attrs…/>` or `<Name attrs…>children…</Name>`.
#[derive(Debug, Clone)]
pub struct Element {
    /// The element's name — a built-in node kind or a component type.
    pub name: Path,
    /// Its attributes, in the order written.
    pub attrs: Vec<Attr>,
    /// `..{modifier}` spreads, applied after every attribute, in order.
    pub spreads: Vec<Expr>,
    /// Its children (empty for a self-closing element).
    pub children: Vec<Child>,
    /// The span of the opening `<`.
    pub open_span: Span,
    /// The span of the element's final `>`.
    pub close_span: Span,
    /// Whether it was written self-closing.
    pub self_closing: bool,
}

impl Element {
    /// The element's name as written (`Column`, `ui::Card`).
    #[must_use]
    pub fn name_string(&self) -> String {
        self.name
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>()
            .join("::")
    }

    /// The attribute named `name`, if written.
    #[must_use]
    pub fn attr(&self, name: &str) -> Option<&Attr> {
        self.attrs.iter().find(|attr| attr.name == name)
    }
}

/// One attribute.
#[derive(Debug, Clone)]
pub struct Attr {
    /// Its name.
    pub name: Ident,
    /// Its value.
    pub value: AttrValue,
}

/// What an attribute is set to.
#[derive(Debug, Clone)]
#[allow(
    clippy::large_enum_variant,
    reason = "a parse tree, built once per macro call and never stored"
)]
pub enum AttrValue {
    /// Present with no value: `disabled`.
    Flag,
    /// A literal: `text="Save"`, `gap=8`.
    Lit(Lit),
    /// A braced Rust expression: `text={label}`.
    Expr(Expr),
}

impl AttrValue {
    /// The span of the value (or of nothing, for a flag).
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Flag => Span::call_site(),
            Self::Lit(lit) => lit.span(),
            Self::Expr(expr) => syn::spanned::Spanned::span(expr),
        }
    }
}

/// One child in element content.
#[derive(Debug, Clone)]
#[allow(
    clippy::large_enum_variant,
    reason = "a parse tree, built once per macro call and never stored"
)]
pub enum Child {
    /// A nested element.
    Element(Element),
    /// `{expr}`: a `Node`, or anything iterable over `Node`s.
    Expr(Expr),
    /// `<>…</>`: several children where one is expected.
    Fragment(Vec<Child>),
    /// `if cond { … } else if … { … } else { … }`.
    If(IfChild),
    /// `match expr { pat => { … }, … }`.
    Match {
        /// The scrutinee.
        expr: Expr,
        /// The arms.
        arms: Vec<MatchArm>,
    },
    /// `for pat in expr { … }`.
    For {
        /// The loop pattern.
        pat: Pat,
        /// The iterated expression.
        expr: Expr,
        /// The body.
        body: Vec<Child>,
    },
}

/// An `if` in child position.
#[derive(Debug, Clone)]
pub struct IfChild {
    /// The condition (may be `let` pattern = expr).
    pub cond: Expr,
    /// The children when it holds.
    pub then: Vec<Child>,
    /// The `else` branch.
    pub otherwise: Option<Box<ElseBranch>>,
}

/// The `else` of an [`IfChild`].
#[derive(Debug, Clone)]
#[allow(
    clippy::large_enum_variant,
    reason = "a parse tree, built once per macro call and never stored"
)]
pub enum ElseBranch {
    /// `else if …`.
    If(IfChild),
    /// `else { … }`.
    Block(Vec<Child>),
}

/// One `match` arm in child position.
#[derive(Debug, Clone)]
pub struct MatchArm {
    /// The pattern.
    pub pat: Pat,
    /// The guard, if any.
    pub guard: Option<Expr>,
    /// The children the arm yields.
    pub body: Vec<Child>,
}

/// Every Rust expression inside `element` and its descendants: attribute
/// values, spreads, braced children, conditions, scrutinees, guards, and
/// iterated expressions — everything that is Rust rather than markup.
#[must_use]
pub fn rust_expressions(element: &Element) -> Vec<&Expr> {
    let mut out = Vec::new();
    collect_element(element, &mut out);
    out
}

fn collect_element<'a>(element: &'a Element, out: &mut Vec<&'a Expr>) {
    for attr in &element.attrs {
        if let AttrValue::Expr(expr) = &attr.value {
            out.push(expr);
        }
    }
    out.extend(element.spreads.iter());
    for child in &element.children {
        collect_child(child, out);
    }
}

fn collect_child<'a>(child: &'a Child, out: &mut Vec<&'a Expr>) {
    match child {
        Child::Element(element) => collect_element(element, out),
        Child::Expr(expr) => out.push(expr),
        Child::Fragment(children) => {
            for child in children {
                collect_child(child, out);
            }
        }
        Child::If(branch) => collect_if(branch, out),
        Child::Match { expr, arms } => {
            out.push(expr);
            for arm in arms {
                out.extend(arm.guard.iter());
                for child in &arm.body {
                    collect_child(child, out);
                }
            }
        }
        Child::For { expr, body, .. } => {
            out.push(expr);
            for child in body {
                collect_child(child, out);
            }
        }
    }
}

fn collect_if<'a>(branch: &'a IfChild, out: &mut Vec<&'a Expr>) {
    out.push(&branch.cond);
    branch.then.iter().for_each(|child| collect_child(child, out));
    match branch.otherwise.as_deref() {
        Some(ElseBranch::If(next)) => collect_if(next, out),
        Some(ElseBranch::Block(children)) => {
            for child in children {
                collect_child(child, out);
            }
        }
        None => {}
    }
}

/// Whether `element` (or any descendant) is a component element rather
/// than a built-in node kind — what decides whether a context is needed.
#[must_use]
pub fn contains_component(element: &Element) -> bool {
    !crate::table::is_builtin(&element.name_string())
        || element.children.iter().any(child_contains_component)
}

fn child_contains_component(child: &Child) -> bool {
    match child {
        Child::Element(element) => contains_component(element),
        Child::Expr(_) => false,
        Child::Fragment(children) => children.iter().any(child_contains_component),
        Child::If(branch) => if_contains_component(branch),
        Child::Match { arms, .. } => {
            arms.iter().any(|arm| arm.body.iter().any(child_contains_component))
        }
        Child::For { body, .. } => body.iter().any(child_contains_component),
    }
}

fn if_contains_component(branch: &IfChild) -> bool {
    branch.then.iter().any(child_contains_component)
        || branch.otherwise.as_deref().is_some_and(|otherwise| match otherwise {
            ElseBranch::If(next) => if_contains_component(next),
            ElseBranch::Block(children) => children.iter().any(child_contains_component),
        })
}
