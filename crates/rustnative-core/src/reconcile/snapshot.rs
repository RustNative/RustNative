//! An immutable, indexed snapshot of one rendered [`crate::Node`] tree.
//!
//! [`TreeSnapshot`] is what [`super::diff::TreeDiff`] compares two of, and
//! what [`crate::layout::LayoutEngine`] and a platform backend read
//! structure from. It pre-computes a children index and a depth index at
//! construction time specifically so that later operations —
//! `children_of`, `ordered_nodes`, depth lookups during diffing — are O(1)/
//! O(children) instead of rescanning every node in the tree, which is what
//! the standards audit's P1.11 finding ("avoidable O(n²) work") flags.

use std::collections::HashMap;

use crate::event::AccessibilityInfo;
use crate::graphics::DrawList;
use crate::identity::NodeId;
use crate::input::{InputInterest, Scalar};
use crate::layout::{ColumnStyle, LayoutStyle, RowStyle};
use crate::node::NodeTransition;
use crate::node::{Node, Tabs, TreeError};
#[cfg(test)]
use crate::style::VisualStyle;
use crate::style::{ControlState, ResolvedStyle, StyleOverride, Theme};
use crate::virtualization::VirtualListStyle;

/// A resolved, backend-facing view of one node.
///
/// Fields are public and read directly by every platform backend (this is
/// the primary contract `rustnative-windows`, and any future backend, reads
/// structure from). Unlike types such as [`crate::style::VisualStyle`],
/// encapsulating this behind accessor methods would not protect any
/// invariant — nothing outside `crate::reconcile` ever constructs or
/// mutates a `TreeNode` — it would only add call-site indirection for a
/// type whose whole purpose is to be read, so this crate makes a deliberate
/// exception to the "prefer private fields" default here (see the
/// standards audit's P1.21, "a good boundary... not... one file per
/// concern").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    /// The node's identity.
    pub id: NodeId,
    /// The node's kind.
    pub kind: crate::node::NodeKind,
    /// The node's parent, or `None` for the tree's root.
    pub parent: Option<NodeId>,
    /// The node's index among its siblings, in declarative order.
    pub index: usize,
    /// The node's text content, for `Label`/`Button`/`TextInput` nodes;
    /// `None` for containers.
    pub text: Option<String>,
    /// The node's layout style.
    pub layout: LayoutStyle,
    /// The node's column style, if it is a `Column`.
    pub column_style: Option<ColumnStyle>,
    /// The node's row style, if it is a `Row`.
    pub row_style: Option<RowStyle>,
    /// The node's accessibility metadata.
    pub accessibility: AccessibilityInfo,
    /// The unresolved visual override, exactly as the application authored
    /// it. Kept alongside the theme-resolved `visual_style` below so a
    /// platform backend can re-resolve a single node's style against a
    /// live-only interaction state (hover/press/focus) without losing the
    /// application's own customization.
    ///
    /// The two are distinct *types*, not two fields of the same type, so
    /// that reaching for the wrong one is a compile error rather than a
    /// control painted with every themed property missing — see
    /// [`crate::style::StyleOverride`] and the standards audit's P2.28
    /// finding.
    pub style_override: StyleOverride,
    /// The fully theme-resolved style: [`Self::style_override`] merged onto
    /// the active [`Theme`]'s default for this node's kind, in this node's
    /// `Normal`/`Disabled` state (see
    /// [`TreeSnapshot::from_node_with_theme`]). A backend applies *this*
    /// field for an ordinary render; it re-derives a fresh value from
    /// `style_override` only when synchronizing a transient interaction
    /// state (hover/press/focus) that a declarative rerender never observes.
    pub visual_style: ResolvedStyle,
    /// Whether the node is disabled.
    pub disabled: bool,
    /// Which advanced input the node wants delivered (see
    /// [`crate::InputInterest`]).
    pub input: InputInterest,
    /// The node's opacity, `0.0` to `1.0`.
    pub opacity: Scalar,
    /// How this node's properties move when they change (see
    /// [`crate::animation`]).
    pub transitions: Vec<NodeTransition>,
    /// Which item of its virtual list this node realizes, if it is inside
    /// one (see [`crate::Node::with_item_index`]).
    pub item_index: Option<usize>,
    /// This node's virtual-list declaration, if it is one (see
    /// [`crate::Node::virtual_list`]).
    pub virtualization: Option<VirtualListStyle>,
    /// What this node draws, if it is a canvas (see
    /// [`crate::Node::canvas`]). Shared with the node tree, not copied.
    pub draw_list: Option<DrawList>,
    /// This node's tabs, if it is a tab bar (see [`crate::Node::tab_bar`]).
    pub tabs: Option<Tabs>,
    /// This node's control, if it is one (see [`crate::Node::control`]).
    pub control: Option<crate::control::Control>,
    /// This node's grid tracks, if it is a grid (see [`crate::Node::grid`]).
    pub grid: Option<crate::layout::GridStyle>,
    /// Whether this node is hidden (see [`crate::Node::hidden`]). Only the
    /// node's own flag: a node inside a hidden container is hidden too,
    /// which [`TreeSnapshot::is_effectively_hidden`] answers.
    pub hidden: bool,
    /// The pointer cursor declared for this node (see
    /// [`crate::Node::with_cursor`]); a backend shows the nearest declared
    /// cursor walking up from the node under the pointer.
    pub cursor: Option<crate::input::Cursor>,
    /// This node's shared identity (see [`crate::Node::with_shared_id`]).
    pub shared_id: Option<NodeId>,
    /// The factory kind of a foreign object this node adopts (see
    /// [`crate::Node::foreign`]).
    pub foreign: Option<String>,
}

impl TreeNode {
    fn from_node(node: &Node, parent: Option<NodeId>, index: usize) -> Self {
        let text = match node {
            Node::Label(label) => Some(label.text().to_owned()),
            Node::Button(button) => Some(button.text().to_owned()),
            Node::TextInput(input) => Some(input.value().to_owned()),
            // A tab strip measures like the text of its labels side by side.
            Node::TabBar(bar) => Some(bar.tabs().labels().join("     ")),
            Node::Control(control) => control.control().text(),
            Node::Column(_) | Node::Row(_) | Node::Canvas(_) | Node::Surface(_) => None,
        };

        Self {
            id: node.id(),
            kind: node.kind(),
            parent,
            index,
            text,
            layout: node.layout(),
            column_style: node.column_style(),
            row_style: node.row_style(),
            accessibility: node.accessibility().clone(),
            style_override: StyleOverride::new(node.visual_style().clone())
                .with_states(node.state_styles().clone()),
            // Left at its resting default until `from_node_with_theme`
            // resolves it; `from_node` deliberately produces a snapshot with
            // no theme applied (see its own doc comment).
            visual_style: ResolvedStyle::default(),
            disabled: node.is_disabled(),
            input: node.input(),
            opacity: Scalar::new(node.opacity()),
            transitions: node.transitions().to_vec(),
            item_index: node.item_index(),
            foreign: node.foreign_kind().map(str::to_owned),
            virtualization: node.virtualization(),
            draw_list: node.draw_list().cloned(),
            tabs: node.tabs().cloned(),
            control: node.control_state().cloned(),
            grid: node.grid_style().cloned(),
            hidden: node.is_hidden(),
            cursor: node.cursor(),
            shared_id: node.shared_id(),
        }
    }
}

/// An immutable, indexed snapshot of one rendered [`Node`] tree.
///
/// # Example
///
/// ```
/// use rustnative_core::{Node, NodeId, TreeDiff, TreeOp, TreeSnapshot};
///
/// let before = TreeSnapshot::from_node(&Node::column(
///     "root",
///     [Node::label("a", "first"), Node::label("b", "second")],
/// ))?;
/// let after = TreeSnapshot::from_node(&Node::column(
///     "root",
///     [Node::label("a", "changed"), Node::label("c", "new")],
/// ))?;
///
/// // Structure is indexed at construction, so this is a lookup, not a scan.
/// assert_eq!(before.children_of(NodeId::from_key("root")).count(), 2);
///
/// // A diff says exactly what a backend must do, in a dependency-safe
/// // order: removals deepest-first, then insertions shallowest-first.
/// let diff = TreeDiff::between(&before, &after);
/// assert!(diff.operations().iter().any(|op| {
///     matches!(op, TreeOp::Remove(node) if node.id == NodeId::from_key("b"))
/// }));
/// assert!(diff.operations().iter().any(|op| {
///     matches!(op, TreeOp::Insert(node) if node.id == NodeId::from_key("c"))
/// }));
/// // A text change moves geometry, so layout must run again.
/// assert!(diff.invalidates_layout());
///
/// // The tree before, in markup:
/// let markup = rustnative_core::rsx! {
///     <Column key="root">
///         <Label key="a" text="first" />
///         <Label key="b" text="second" />
///     </Column>
/// };
/// assert_eq!(TreeSnapshot::from_node(&markup)?, before);
/// # Ok::<(), rustnative_core::TreeError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TreeSnapshot {
    nodes: HashMap<NodeId, TreeNode>,
    children: HashMap<Option<NodeId>, Vec<NodeId>>,
    depths: HashMap<NodeId, usize>,
}

impl TreeSnapshot {
    /// Builds an immutable snapshot of `root` for the layout/hit-testing
    /// passes to read from.
    ///
    /// # Errors
    ///
    /// Returns `TreeError::DuplicateNodeId` if two nodes in `root` share the
    /// same `NodeId` — every node key must be unique within a single view.
    pub fn from_node(root: &Node) -> Result<Self, TreeError> {
        root.validate_unique_ids()?;

        let mut nodes = HashMap::new();
        let mut children: HashMap<Option<NodeId>, Vec<NodeId>> = HashMap::new();
        let mut depths = HashMap::new();
        root.visit(&mut |node, parent, index| {
            let id = node.id();
            let depth = parent
                .and_then(|parent| depths.get(&parent).copied())
                .map_or(0usize, |parent_depth: usize| parent_depth.saturating_add(1));
            nodes.insert(id, TreeNode::from_node(node, parent, index));
            children.entry(parent).or_default().push(id);
            depths.insert(id, depth);
        });

        // A virtual list's items announce their true place in the whole
        // list ("row 5,012 of 100,000"), not among the dozen realized
        // siblings — unless an item already said where it is.
        let lists = nodes
            .values()
            .filter_map(|node| node.virtualization.map(|style| (node.id, style.item_count)))
            .collect::<HashMap<_, _>>();
        if !lists.is_empty() {
            let saturate = |value: usize| u32::try_from(value).unwrap_or(u32::MAX);
            for node in nodes.values_mut() {
                let Some(count) = node.parent.and_then(|parent| lists.get(&parent)) else {
                    continue;
                };
                if node.accessibility.position().is_some() {
                    continue;
                }
                let index = node.item_index.unwrap_or(node.index);
                node.accessibility = node
                    .accessibility
                    .clone()
                    .position_in_set(saturate(index.saturating_add(1)), saturate(*count));
            }
        }

        Ok(Self { nodes, children, depths })
    }

    /// Builds a snapshot whose `visual_style` on every node is fully resolved
    /// against `theme`: the theme's per-kind default merged with that node's
    /// own override, in the node's `Normal` (or `Disabled`, when the node is
    /// marked disabled) state. A platform backend can apply the resulting
    /// colors and typography directly without needing to know about `Theme`
    /// merge order itself, mirroring how layout geometry is already fully
    /// resolved in the core before a backend applies it.
    ///
    /// Interactive states that only the backend can observe live — hover,
    /// press, and focus — remain the backend's responsibility: it already
    /// tracks focus for Tab traversal and can re-resolve a single focused
    /// node's style against the same theme when focus changes.
    ///
    /// # Errors
    ///
    /// Returns `TreeError::DuplicateNodeId` under the same condition as
    /// [`Self::from_node`].
    pub fn from_node_with_theme(root: &Node, theme: &Theme) -> Result<Self, TreeError> {
        let mut snapshot = Self::from_node(root)?;
        for node in snapshot.nodes.values_mut() {
            let state = if node.disabled { ControlState::Disabled } else { ControlState::Normal };
            // Resolved from `style_override`, never from `visual_style`.
            // An earlier revision read the latter — harmless only because
            // `from_node` initialized both fields to the same value, so
            // resolution happened to see the application's override anyway.
            // Splitting the two phases into distinct types (P2.28) is what
            // turned that latent confusion into a compile error.
            node.visual_style = theme.resolve(node.kind, state, &node.style_override);
        }
        Ok(snapshot)
    }

    /// Returns node `id`'s resolved data, if it exists in this snapshot.
    #[must_use]
    pub fn get(&self, id: NodeId) -> Option<&TreeNode> {
        self.nodes.get(&id)
    }

    /// Whether `id` is hidden, by its own flag or by any ancestor's.
    #[must_use]
    pub fn is_effectively_hidden(&self, id: NodeId) -> bool {
        let mut current = self.nodes.get(&id);
        while let Some(node) = current {
            if node.hidden {
                return true;
            }
            current = node.parent.and_then(|parent| self.nodes.get(&parent));
        }
        false
    }

    /// Returns whether `id` names a node in this snapshot.
    #[must_use]
    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }

    /// Iterates every node in this snapshot, in unspecified order (see
    /// [`Self::ordered_nodes`] for declarative preorder).
    pub fn nodes(&self) -> impl Iterator<Item = &TreeNode> {
        self.nodes.values()
    }

    /// Returns a node's children in declarative sibling order without
    /// rescanning the complete snapshot (backed by the children index built
    /// in [`Self::from_node`]).
    pub fn children_of(&self, parent: NodeId) -> impl Iterator<Item = &TreeNode> {
        self.children.get(&Some(parent)).into_iter().flatten().filter_map(|id| self.nodes.get(id))
    }

    /// Returns nodes in declarative preorder, preserving sibling indices.
    #[must_use]
    pub fn ordered_nodes(&self) -> Vec<&TreeNode> {
        let mut ordered = Vec::with_capacity(self.nodes.len());
        if let Some(root) = self.children.get(&None).and_then(|roots| roots.first()).copied() {
            collect_ordered_nodes(self, root, &mut ordered);
        }
        ordered
    }

    /// O(1) depth lookup backed by the index built in [`Self::from_node`],
    /// rather than walking the parent chain on every call.
    pub(crate) fn depth(&self, id: NodeId) -> usize {
        self.depths.get(&id).copied().unwrap_or(0)
    }
}

fn collect_ordered_nodes<'a>(
    snapshot: &'a TreeSnapshot,
    id: NodeId,
    output: &mut Vec<&'a TreeNode>,
) {
    let Some(node) = snapshot.get(id) else {
        return;
    };
    output.push(node);
    for child in snapshot.children_of(id) {
        collect_ordered_nodes(snapshot, child.id, output);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_flag_round_trips_into_tree_snapshot() {
        let node = Node::label("a", "hello").disabled(true);
        let snapshot = TreeSnapshot::from_node(&node).unwrap();
        assert!(snapshot.get(NodeId::from_key("a")).unwrap().disabled);
    }

    #[test]
    fn a_virtual_list_item_announces_its_place_in_the_whole_list() {
        let list = Node::virtual_list(
            "list",
            crate::VirtualListStyle::new(100_000, crate::ItemExtent::Fixed(20)),
            [
                Node::label("row-5011", "row").with_item_index(5011),
                Node::label("row-5012", "row").with_item_index(5012).with_accessibility(
                    AccessibilityInfo::new(crate::AccessibilityRole::Label).position_in_set(1, 1),
                ),
            ],
        );
        let snapshot = TreeSnapshot::from_node(&list).unwrap();
        let position =
            |key: &str| snapshot.get(NodeId::from_key(key)).unwrap().accessibility.position();
        assert_eq!(position("row-5011"), Some((5012, 100_000)), "1-based, out of the whole list");
        assert_eq!(position("row-5012"), Some((1, 1)), "an explicit position is not overridden");
    }

    #[test]
    fn from_node_with_theme_resolves_normal_and_disabled_states() {
        // Use a theme with an explicit `disabled` override for buttons so
        // this test actually proves state selection, rather than merely
        // confirming that resolution doesn't panic: `Theme::default()`'s
        // button style defines no `disabled` override at all, so a
        // disabled button would legitimately resolve to the same style as
        // an enabled one there.
        let base = Theme::default();
        let button_style = crate::style::ComponentStyle {
            normal: base.button().normal.clone(),
            disabled: Some(VisualStyle::new().foreground(crate::style::Color::rgb(9, 9, 9))),
            ..Default::default()
        };
        let theme = base.with_button(button_style);

        let node = Node::column(
            "root",
            [Node::button("go", "Go"), Node::button("stop", "Stop").disabled(true)],
        );
        let snapshot = TreeSnapshot::from_node_with_theme(&node, &theme).unwrap();
        let go = snapshot.get(NodeId::from_key("go")).unwrap();
        let stop = snapshot.get(NodeId::from_key("stop")).unwrap();
        assert_eq!(
            go.visual_style.properties().foreground_override(),
            theme.button().normal.foreground_override()
        );
        assert_eq!(
            stop.visual_style.properties().foreground_override(),
            Some(crate::style::Color::rgb(9, 9, 9))
        );
        assert_ne!(
            stop.visual_style.properties().foreground_override(),
            go.visual_style.properties().foreground_override()
        );
        // The resolved style also records *which* state it was resolved
        // for, which is what lets a backend tell an ordinary render apart
        // from a transient interaction repaint.
        assert_eq!(go.visual_style.state(), ControlState::Normal);
        assert_eq!(stop.visual_style.state(), ControlState::Disabled);
    }

    #[test]
    fn themed_snapshot_retains_node_override_for_live_platform_states() {
        let theme = Theme::default();
        let override_style = VisualStyle::new().foreground(crate::style::Color::rgb(9, 9, 9));
        let node = Node::button("go", "Go").with_style(override_style.clone());
        let snapshot = TreeSnapshot::from_node_with_theme(&node, &theme).unwrap();
        let go = snapshot.get(NodeId::from_key("go")).unwrap();
        assert_eq!(go.style_override.properties(), &override_style);
    }

    #[test]
    fn an_unthemed_snapshot_leaves_the_resolved_style_at_its_resting_default() {
        // `from_node` performs no theme resolution, so its `visual_style`
        // must be an honest "not resolved yet" rather than a copy of the
        // application's override masquerading as a resolved value — which
        // is what it used to be, and what let `from_node_with_theme` read
        // the wrong field for a while without anyone noticing (P2.28).
        let override_style = VisualStyle::new().foreground(crate::style::Color::rgb(9, 9, 9));
        let node = Node::button("go", "Go").with_style(override_style.clone());
        let snapshot = TreeSnapshot::from_node(&node).unwrap();
        let go = snapshot.get(NodeId::from_key("go")).unwrap();
        assert_eq!(go.style_override.properties(), &override_style);
        assert_eq!(go.visual_style, ResolvedStyle::default());
    }

    #[test]
    fn children_of_and_ordered_nodes_use_the_precomputed_index() {
        let node = Node::column("root", [Node::label("a", "a"), Node::label("b", "b")]);
        let snapshot = TreeSnapshot::from_node(&node).unwrap();
        let children: Vec<_> =
            snapshot.children_of(NodeId::from_key("root")).map(|n| n.id).collect();
        assert_eq!(children, vec![NodeId::from_key("a"), NodeId::from_key("b")]);
        assert_eq!(snapshot.ordered_nodes().len(), 3);
    }
}
