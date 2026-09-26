//! The accessible projection of one rendered tree.
//!
//! [`AccessibilityTree`] answers the questions every backend's
//! accessibility bridge asks, once and portably, instead of each backend
//! re-deriving them from the raw [`TreeSnapshot`]:
//!
//! - **Which nodes are elements at all.** A node with
//!   [`AccessibilityRole::None`] is structure, not semantics: its children
//!   are presented as children of its nearest exposed ancestor, which is how
//!   every platform accessibility API treats a role-less container.
//! - **What a node is called.** [`AccessibilityTree::name_of`] applies the
//!   standard precedence (the same one WAI-ARIA's accessible-name
//!   computation uses): an explicit name, then the text of the node it is
//!   labelled by, then its own visible text.
//! - **What its relationships point at.** Only targets present in this
//!   render resolve; a dangling relationship is dropped rather than exposed
//!   as a reference to nothing.

use std::collections::HashMap;

use super::{AccessibilityInfo, AccessibilityRole};
use crate::identity::NodeId;
use crate::reconcile::TreeSnapshot;

/// A relationship kind, for [`AccessibilityTree::resolve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Relation {
    /// The node whose text labels this one.
    LabelledBy,
    /// Nodes whose text describes this one.
    DescribedBy,
    /// Nodes this one controls.
    Controls,
}

/// One exposed element of an [`AccessibilityTree`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessibleNode {
    /// The node's identity.
    pub id: NodeId,
    /// Its accessibility metadata.
    pub info: AccessibilityInfo,
    /// Its nearest *exposed* ancestor, or `None` at the top.
    pub parent: Option<NodeId>,
    /// Its visible text, if it has any.
    pub text: Option<String>,
    /// Whether it is disabled.
    pub disabled: bool,
}

/// The accessible projection of a [`TreeSnapshot`]. See the module
/// documentation.
///
/// # Example
///
/// ```
/// use rustnative_core::{
///     AccessibilityInfo, AccessibilityRole, AccessibilityTree, Node, NodeId, TreeSnapshot,
/// };
///
/// let tree = Node::column(
///     "form",
///     [
///         Node::label("caption", "Email address"),
///         Node::text_input("email", "").with_accessibility(
///             AccessibilityInfo::new(AccessibilityRole::TextInput)
///                 .labelled_by("caption")
///                 .focusable(true),
///         ),
///     ],
/// )
/// // A purely structural container: not an element of its own.
/// .with_accessibility(AccessibilityInfo::new(AccessibilityRole::None));
///
/// let snapshot = TreeSnapshot::from_node(&tree)?;
/// let accessible = AccessibilityTree::from_snapshot(&snapshot);
///
/// // The field is named by its label's text...
/// assert_eq!(accessible.name_of(NodeId::from_key("email")).as_deref(), Some("Email address"));
/// // ...and, with the column flattened away, both are top-level elements.
/// assert_eq!(accessible.roots().count(), 2);
///
/// // The same tree in markup:
/// let field = AccessibilityInfo::new(AccessibilityRole::TextInput).labelled_by("caption").focusable(true);
/// let markup = rustnative_core::rsx! {
///     <Column key="form" accessibility={AccessibilityInfo::new(AccessibilityRole::None)}>
///         <Label key="caption" text="Email address" />
///         <TextInput key="email" value="" accessibility={field} />
///     </Column>
/// };
/// assert_eq!(markup, tree);
/// # Ok::<(), rustnative_core::TreeError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AccessibilityTree {
    nodes: HashMap<NodeId, AccessibleNode>,
    children: HashMap<Option<NodeId>, Vec<NodeId>>,
}

impl AccessibilityTree {
    /// Projects `snapshot`.
    #[must_use]
    pub fn from_snapshot(snapshot: &TreeSnapshot) -> Self {
        let mut tree = Self::default();
        // Preorder, so every node's exposed ancestor is resolved before it.
        let mut exposed_parent: HashMap<NodeId, Option<NodeId>> = HashMap::new();
        // A hidden node and everything inside it are not there for anyone:
        // not on screen, so not to assistive technology either.
        let mut hidden = std::collections::HashSet::new();
        for node in snapshot.ordered_nodes() {
            if node.hidden || node.parent.is_some_and(|parent| hidden.contains(&parent)) {
                hidden.insert(node.id);
                continue;
            }
            let inherited =
                node.parent.and_then(|parent| exposed_parent.get(&parent).copied()).flatten();
            let exposed = node.accessibility.role() != AccessibilityRole::None;
            // What this node's own children see as their exposed parent.
            exposed_parent.insert(node.id, if exposed { Some(node.id) } else { inherited });
            if !exposed {
                continue;
            }
            tree.children.entry(inherited).or_default().push(node.id);
            tree.nodes.insert(
                node.id,
                AccessibleNode {
                    id: node.id,
                    info: node.accessibility.clone(),
                    parent: inherited,
                    text: node.text.clone(),
                    disabled: node.disabled,
                },
            );
        }
        tree
    }

    /// The element for `id`, if it is exposed.
    #[must_use]
    pub fn node(&self, id: NodeId) -> Option<&AccessibleNode> {
        self.nodes.get(&id)
    }

    /// The exposed children of `id`, in declarative order.
    pub fn children(&self, id: NodeId) -> impl Iterator<Item = &AccessibleNode> {
        self.children_of(Some(id))
    }

    /// The top-level exposed elements, in declarative order.
    pub fn roots(&self) -> impl Iterator<Item = &AccessibleNode> {
        self.children_of(None)
    }

    fn children_of(&self, parent: Option<NodeId>) -> impl Iterator<Item = &AccessibleNode> {
        self.children.get(&parent).into_iter().flatten().filter_map(|id| self.nodes.get(id))
    }

    /// How many elements are exposed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether no element is exposed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The targets of `id`'s `relation` that are exposed elements of this
    /// tree, in declaration order. A target with
    /// [`AccessibilityRole::None`] is structure, not an element assistive
    /// technology could navigate to, so it does not resolve.
    #[must_use]
    pub fn resolve(&self, id: NodeId, relation: Relation) -> Vec<NodeId> {
        let Some(node) = self.nodes.get(&id) else {
            return Vec::new();
        };
        let targets: Vec<NodeId> = match relation {
            Relation::LabelledBy => node.info.labelled_by_node().into_iter().collect(),
            Relation::DescribedBy => node.info.described_by_nodes().to_vec(),
            Relation::Controls => node.info.controls_nodes().to_vec(),
        };
        targets.into_iter().filter(|target| self.nodes.contains_key(target)).collect()
    }

    /// The accessible name of `id`: its explicit name, else the name of the
    /// node it is labelled by, else its own visible text.
    #[must_use]
    pub fn name_of(&self, id: NodeId) -> Option<String> {
        self.name_with_depth(id, 0)
    }

    fn name_with_depth(&self, id: NodeId, depth: usize) -> Option<String> {
        // A labelled-by chain is followed, but a cycle (A labelled by B
        // labelled by A) must terminate; two hops is what the ARIA
        // algorithm itself allows before falling back to content.
        const MAX_LABEL_HOPS: usize = 2;
        let node = self.nodes.get(&id)?;
        if let Some(name) = node.info.name_hint() {
            return Some(name.to_owned());
        }
        if depth < MAX_LABEL_HOPS {
            if let Some(label) = self.resolve(id, Relation::LabelledBy).first() {
                if let Some(name) = self.name_with_depth(*label, depth + 1) {
                    return Some(name);
                }
            }
        }
        node.text.clone().filter(|text| !text.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    fn project(node: &Node) -> AccessibilityTree {
        AccessibilityTree::from_snapshot(&TreeSnapshot::from_node(node).unwrap())
    }

    #[test]
    fn role_less_containers_are_flattened_into_their_exposed_ancestor() {
        let tree = project(&Node::column(
            "root",
            [Node::column("wrapper", [Node::button("a", "A"), Node::button("b", "B")])
                .with_accessibility(AccessibilityInfo::new(AccessibilityRole::None))],
        ));
        let root = NodeId::from_key("root");
        let children: Vec<_> = tree.children(root).map(|node| node.id).collect();
        assert_eq!(children, [NodeId::from_key("a"), NodeId::from_key("b")]);
        assert!(tree.node(NodeId::from_key("wrapper")).is_none());
        assert_eq!(tree.node(NodeId::from_key("a")).unwrap().parent, Some(root));
    }

    #[test]
    fn names_follow_explicit_then_label_then_text() {
        let tree = project(&Node::column(
            "root",
            [
                Node::label("caption", "Caption text"),
                Node::button("explicit", "Visible").with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::Button).name("Named"),
                ),
                Node::text_input("labelled", "").with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::TextInput).labelled_by("caption"),
                ),
                Node::button("plain", "Plain text"),
            ],
        ));
        assert_eq!(tree.name_of(NodeId::from_key("explicit")).as_deref(), Some("Named"));
        assert_eq!(tree.name_of(NodeId::from_key("labelled")).as_deref(), Some("Caption text"));
        assert_eq!(tree.name_of(NodeId::from_key("plain")).as_deref(), Some("Plain text"));
    }

    #[test]
    fn a_labelling_cycle_terminates() {
        let tree = project(&Node::column(
            "root",
            [
                Node::text_input("a", "").with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::TextInput).labelled_by("b"),
                ),
                Node::text_input("b", "").with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::TextInput).labelled_by("a"),
                ),
            ],
        ));
        assert_eq!(tree.name_of(NodeId::from_key("a")), None, "no name, and no infinite loop");
    }

    #[test]
    fn dangling_relationships_do_not_resolve() {
        let tree = project(&Node::column(
            "root",
            [Node::button("b", "B").with_accessibility(
                AccessibilityInfo::new(AccessibilityRole::Button)
                    .described_by("gone")
                    .described_by("root"),
            )],
        ));
        assert_eq!(
            tree.resolve(NodeId::from_key("b"), Relation::DescribedBy),
            [NodeId::from_key("root")]
        );
    }
    #[test]
    fn a_hidden_subtree_is_not_exposed() {
        use crate::node::Node;
        let tree = Node::column(
            "root",
            [
                Node::button("shown", "Shown"),
                Node::column("screen", [Node::button("inside", "Inside")]).hidden(true),
            ],
        );
        let snapshot = TreeSnapshot::from_node(&tree).unwrap();
        let accessible = AccessibilityTree::from_snapshot(&snapshot);
        assert!(accessible.node(NodeId::from_key("shown")).is_some());
        assert!(accessible.node(NodeId::from_key("screen")).is_none());
        assert!(accessible.node(NodeId::from_key("inside")).is_none(), "nor anything inside it");
    }
}
