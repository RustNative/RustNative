//! Finding nodes the way a person — or assistive technology — would.
//!
//! A test that finds "the button named Save" survives a refactor that
//! renames keys, reorders containers, or rewrites a screen from the builder
//! syntax into markup; a test that finds "the third child of the second
//! row" does not. It is also an accessibility check for free: a control
//! that cannot be found by role and name cannot be found by a screen reader
//! either. This is `C60` of the concept survey.

use std::fmt;
use std::fmt::Write as _;

use rustnative_core::AccessibilityRole;

use crate::tree::{HeadlessTree, RealizedNode};

/// A description of the node a test is looking for. Every condition given
/// must hold.
///
/// ```
/// use rustnative_core::AccessibilityRole;
/// use rustnative_headless::Query;
///
/// let save = Query::role(AccessibilityRole::Button).name("Save");
/// assert_eq!(save.to_string(), "role=Button name=\"Save\"");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    role: Option<AccessibilityRole>,
    name: Option<String>,
    text: Option<String>,
    key: Option<String>,
    disabled: Option<bool>,
    focused: Option<bool>,
    labelled_control: bool,
    include_hidden: bool,
}

impl Query {
    /// Nodes with accessibility role `role`.
    #[must_use]
    pub fn role(role: AccessibilityRole) -> Self {
        Self { role: Some(role), ..Self::default() }
    }

    /// Nodes whose accessible name is exactly `name`.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self { name: Some(name.into()), ..Self::default() }
    }

    /// Nodes showing exactly `text`.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self { text: Some(text.into()), ..Self::default() }
    }

    /// The control labelled `label` — by an associated label node or by an
    /// explicit accessible name, as a screen reader announces it. The label
    /// node itself is not a match: it is the field a test wants.
    #[must_use]
    pub fn label(label: impl Into<String>) -> Self {
        Self { name: Some(label.into()), labelled_control: true, ..Self::default() }
    }

    /// The node created with key `key`. The escape hatch for a node with no
    /// accessible identity of its own; prefer role and name.
    #[must_use]
    pub fn key(key: impl Into<String>) -> Self {
        Self { key: Some(key.into()), ..Self::default() }
    }

    /// Additionally requires the accessible name `name`.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Additionally requires the node to be disabled (or enabled).
    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = Some(disabled);
        self
    }

    /// Additionally requires the node to be focused (or not).
    #[must_use]
    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = Some(focused);
        self
    }

    /// Also considers hidden nodes, which are otherwise invisible to a query
    /// exactly as they are to a person.
    #[must_use]
    pub const fn include_hidden(mut self) -> Self {
        self.include_hidden = true;
        self
    }

    fn matches(&self, node: &RealizedNode) -> bool {
        (self.include_hidden || !node.hidden)
            && self.role.is_none_or(|role| node.accessibility.role() == role)
            && self.name.as_ref().is_none_or(|name| node.name.as_ref() == Some(name))
            && self.text.as_ref().is_none_or(|text| node.text.as_ref() == Some(text))
            && self.key.as_ref().is_none_or(|key| node.key.as_ref() == Some(key))
            && self.disabled.is_none_or(|disabled| node.disabled == disabled)
            && self.focused.is_none_or(|focused| node.focused == focused)
            && (!self.labelled_control || node.accessibility.role() != AccessibilityRole::Label)
    }

    /// Every node in `tree` this query matches, in tree order.
    #[must_use]
    pub fn all<'a>(&self, tree: &'a HeadlessTree) -> Vec<&'a RealizedNode> {
        tree.nodes().filter(|node| self.matches(node)).collect()
    }

    /// The one node in `tree` this query matches.
    ///
    /// # Errors
    ///
    /// [`QueryError::NotFound`] when nothing matches — listing what the
    /// tree does contain — and [`QueryError::Ambiguous`] when several do.
    pub fn one<'a>(&self, tree: &'a HeadlessTree) -> Result<&'a RealizedNode, QueryError> {
        let matches = self.all(tree);
        match matches.as_slice() {
            [node] => Ok(node),
            [] => Err(QueryError::NotFound { query: self.to_string(), tree: outline(tree) }),
            several => Err(QueryError::Ambiguous {
                query: self.to_string(),
                matches: several.iter().map(|node| describe(node)).collect(),
            }),
        }
    }
}

impl fmt::Display for Query {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if let Some(role) = self.role {
            parts.push(format!("role={role:?}"));
        }
        if let Some(name) = &self.name {
            parts.push(format!("name={name:?}"));
        }
        if let Some(text) = &self.text {
            parts.push(format!("text={text:?}"));
        }
        if let Some(key) = &self.key {
            parts.push(format!("key={key:?}"));
        }
        if let Some(disabled) = self.disabled {
            parts.push(format!("disabled={disabled}"));
        }
        if let Some(focused) = self.focused {
            parts.push(format!("focused={focused}"));
        }
        if parts.is_empty() {
            parts.push("any node".to_owned());
        }
        f.write_str(&parts.join(" "))
    }
}

/// Why a query, or an interaction through one, failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    /// Nothing matched.
    NotFound {
        /// The query.
        query: String,
        /// What the accessible tree does contain, one node per line.
        tree: String,
    },
    /// More than one node matched a query that needed exactly one.
    Ambiguous {
        /// The query.
        query: String,
        /// Each match, described.
        matches: Vec<String>,
    },
    /// The node exists but a person could not do what the test asked:
    /// it is disabled, hidden, off screen, or the wrong kind of control.
    NotInteractable {
        /// The node.
        node: String,
        /// Why not.
        reason: String,
    },
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { query, tree } => {
                write!(f, "no node matches `{query}`; the tree contains:\n{tree}")
            }
            Self::Ambiguous { query, matches } => {
                write!(f, "{} nodes match `{query}`:\n  {}", matches.len(), matches.join("\n  "))
            }
            Self::NotInteractable { node, reason } => {
                write!(f, "cannot interact with {node}: {reason}")
            }
        }
    }
}

impl std::error::Error for QueryError {}

pub(crate) fn describe(node: &RealizedNode) -> String {
    let mut text = format!("{:?}", node.accessibility.role());
    if let Some(name) = &node.name {
        let _ = write!(text, " {name:?}");
    } else if let Some(shown) = &node.text {
        let _ = write!(text, " text={shown:?}");
    }
    if let Some(key) = &node.key {
        let _ = write!(text, " (key {key:?})");
    }
    if node.disabled {
        text.push_str(" disabled");
    }
    if node.hidden {
        text.push_str(" hidden");
    }
    text
}

fn outline(tree: &HeadlessTree) -> String {
    let lines: Vec<String> = tree
        .nodes()
        .filter(|node| !node.hidden)
        .map(|node| format!("  {}", describe(node)))
        .collect();
    if lines.is_empty() { "  (nothing)".to_owned() } else { lines.join("\n") }
}

/// A located node, for assertions.
pub type Found<'a> = &'a RealizedNode;
