//! Structural diffing between two [`TreeSnapshot`]s.

use super::snapshot::{TreeNode, TreeSnapshot};
use crate::identity::NodeId;

/// One structural change a backend must apply to keep its native objects
/// in sync with a new [`TreeSnapshot`] (see [`TreeDiff`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeOp {
    /// A node that did not exist in the previous snapshot.
    Insert(TreeNode),
    /// A node whose data changed since the previous snapshot.
    Update(TreeNode),
    /// A node whose parent or sibling position changed.
    Move {
        /// The moved node's identity.
        id: NodeId,
        /// The node's new parent, or `None` for the root.
        parent: Option<NodeId>,
        /// The node's new position among its siblings.
        index: usize,
    },
    /// A node that existed in the previous snapshot but not this one.
    Remove(TreeNode),
}

/// The set of operations needed to turn one [`TreeSnapshot`] into another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeDiff {
    operations: Vec<TreeOp>,
    layout_dirty: bool,
}

impl TreeDiff {
    /// Computes the operations needed to turn `previous` into `next`.
    pub fn between(previous: &TreeSnapshot, next: &TreeSnapshot) -> Self {
        let mut operations = Vec::new();
        let mut layout_dirty = false;

        // Remove deepest descendants first so platform backends can safely
        // dispose child objects before their logical parents.
        let mut removals =
            previous.nodes().filter(|node| !next.contains(node.id)).cloned().collect::<Vec<_>>();
        removals.sort_by_key(|node| std::cmp::Reverse(previous.depth(node.id)));
        layout_dirty |= !removals.is_empty();
        operations.extend(removals.into_iter().map(TreeOp::Remove));

        // Insert shallow nodes before descendants so native parents can exist
        // before their children.
        let mut inserts =
            next.nodes().filter(|node| !previous.contains(node.id)).cloned().collect::<Vec<_>>();
        inserts.sort_by_key(|node| next.depth(node.id));
        layout_dirty |= !inserts.is_empty();
        operations.extend(inserts.into_iter().map(TreeOp::Insert));

        // Existing nodes are updated only when their semantic data changed;
        // order/parent changes are represented separately as Move operations.
        for node in next.nodes() {
            let Some(previous_node) = previous.get(node.id) else {
                continue;
            };

            if node_changed(previous_node, node) {
                layout_dirty |= is_layout_relevant_change(previous_node, node);
                operations.push(TreeOp::Update(node.clone()));
            }

            if previous_node.parent != node.parent || previous_node.index != node.index {
                layout_dirty = true;
                operations.push(TreeOp::Move {
                    id: node.id,
                    parent: node.parent,
                    index: node.index,
                });
            }
        }

        Self { operations, layout_dirty }
    }

    /// The operations a backend must apply, in dependency-safe order
    /// (removals deepest-first, then insertions shallowest-first, then
    /// updates/moves).
    #[must_use]
    pub fn operations(&self) -> &[TreeOp] {
        &self.operations
    }

    /// Whether any operation in this diff can affect node geometry and
    /// therefore requires a full layout pass.
    ///
    /// This is deliberately *not* "any `Update` happened": a purely visual
    /// change (color, border) or an accessibility-only change (name,
    /// description) never affects size or position, so it must not force a
    /// relayout on every such update — see `is_layout_relevant_change` for
    /// the exact classification, and the standards audit's P1.12 finding
    /// ("layout invalidation is far too coarse") for why this matters.
    #[must_use]
    pub fn invalidates_layout(&self) -> bool {
        self.layout_dirty
    }
}

fn node_changed(previous: &TreeNode, next: &TreeNode) -> bool {
    previous.kind != next.kind
        || previous.text != next.text
        || previous.layout != next.layout
        || previous.column_style != next.column_style
        || previous.row_style != next.row_style
        || previous.accessibility != next.accessibility
        || previous.style_override != next.style_override
        || previous.visual_style != next.visual_style
        || previous.disabled != next.disabled
        || previous.input != next.input
        || previous.opacity != next.opacity
        || previous.transitions != next.transitions
        || previous.item_index != next.item_index
        || previous.virtualization != next.virtualization
        // Redraws the canvas; never a layout change, since a canvas is as
        // big as layout makes it, whatever it draws.
        || previous.draw_list != next.draw_list
        || previous.tabs != next.tabs
        || previous.hidden != next.hidden
}

/// Classifies whether the difference between two versions of the same node
/// can change its geometry (size, wrapping, or child layout), as opposed to
/// being purely visual (colors, borders) or accessibility-only (name,
/// description, role). Only a layout-relevant change needs to invalidate
/// layout; the caller ([`TreeDiff::between`]) still emits an `Update`
/// operation either way, since a backend must repaint/re-realize the node
/// regardless of which kind of change it was.
fn is_layout_relevant_change(previous: &TreeNode, next: &TreeNode) -> bool {
    previous.kind != next.kind
        || previous.text != next.text
        || previous.layout != next.layout
        || previous.column_style != next.column_style
        || previous.row_style != next.row_style
        // An item that changed which index it realizes moves to that
        // index's offset, and a list whose item count or extent policy
        // changed has a different length and a different scroll range.
        || previous.item_index != next.item_index
        || previous.virtualization != next.virtualization
        // A hidden node gives up its space, and a shown one takes it back.
        || previous.hidden != next.hidden
        || typography_changed(previous, next)
}

fn typography_changed(previous: &TreeNode, next: &TreeNode) -> bool {
    previous.visual_style.properties().typography_override()
        != next.visual_style.properties().typography_override()
        || previous.style_override.properties().typography_override()
            != next.style_override.properties().typography_override()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::LayoutStyle;
    use crate::node::Node;
    use crate::style::{Color, VisualStyle};

    fn label(key: &str, text: &str) -> Node {
        Node::label(key, text)
    }

    #[test]
    fn a_new_drawing_updates_only_its_canvas_and_leaves_layout_alone() {
        use crate::graphics::{DrawList, Paint, RectF};
        use crate::style::Color;

        let scene = |color: Color| {
            Node::column(
                "root",
                [
                    Node::canvas(
                        "canvas",
                        DrawList::new()
                            .fill_rect(RectF::new(0.0, 0.0, 9.0, 9.0), Paint::color(color)),
                        LayoutStyle::new(),
                    ),
                    Node::label("steady", "unchanged"),
                ],
            )
        };
        let previous = TreeSnapshot::from_node(&scene(Color::rgb(255, 0, 0))).unwrap();
        let next = TreeSnapshot::from_node(&scene(Color::rgb(0, 0, 255))).unwrap();
        let diff = TreeDiff::between(&previous, &next);
        assert_eq!(diff.operations().len(), 1, "{:?}", diff.operations());
        assert!(matches!(
            &diff.operations()[0],
            TreeOp::Update(node) if node.id == NodeId::from_key("canvas")
        ));
        assert!(!diff.invalidates_layout(), "a canvas is as big as layout makes it");

        let same = TreeSnapshot::from_node(&scene(Color::rgb(255, 0, 0))).unwrap();
        assert!(TreeDiff::between(&previous, &same).operations().is_empty());
    }

    #[test]
    fn diff_updates_existing_node_without_inserting_it() {
        let previous = TreeSnapshot::from_node(&label("a", "one")).unwrap();
        let next = TreeSnapshot::from_node(&label("a", "two")).unwrap();
        let diff = TreeDiff::between(&previous, &next);
        assert_eq!(diff.operations().len(), 1);
        assert!(matches!(diff.operations()[0], TreeOp::Update(_)));
    }

    #[test]
    fn diff_detects_insert_remove_and_move() {
        let previous = Node::column("root", [label("a", "a"), label("b", "b")]);
        let next = Node::column("root", [label("b", "b"), label("c", "c")]);
        let previous_snapshot = TreeSnapshot::from_node(&previous).unwrap();
        let next_snapshot = TreeSnapshot::from_node(&next).unwrap();
        let diff = TreeDiff::between(&previous_snapshot, &next_snapshot);

        assert!(
            diff.operations()
                .iter()
                .any(|op| matches!(op, TreeOp::Remove(node) if node.id == NodeId::from_key("a")))
        );
        assert!(
            diff.operations()
                .iter()
                .any(|op| matches!(op, TreeOp::Insert(node) if node.id == NodeId::from_key("c")))
        );
        assert!(diff.operations().iter().any(|op| matches!(
            op,
            TreeOp::Move { id, .. } if *id == NodeId::from_key("b")
        )));
    }

    #[test]
    fn text_update_invalidates_layout() {
        let previous = TreeSnapshot::from_node(&label("a", "one")).unwrap();
        let next = TreeSnapshot::from_node(&label("a", "a much longer piece of text")).unwrap();
        let diff = TreeDiff::between(&previous, &next);
        assert!(diff.invalidates_layout());
    }

    #[test]
    fn tree_diff_emits_update_when_only_visual_style_changes() {
        let previous_node = Node::label("a", "same").with_style(VisualStyle::new());
        let next_node =
            Node::label("a", "same").with_style(VisualStyle::new().foreground(Color::rgb(1, 2, 3)));
        let previous = TreeSnapshot::from_node(&previous_node).unwrap();
        let next = TreeSnapshot::from_node(&next_node).unwrap();
        let diff = TreeDiff::between(&previous, &next);

        assert_eq!(diff.operations().len(), 1);
        assert!(matches!(diff.operations()[0], TreeOp::Update(_)));
        assert!(
            !diff.invalidates_layout(),
            "a purely visual style change must not force a relayout (P1.12)"
        );
    }

    #[test]
    fn diff_updates_an_existing_node_when_its_disabled_state_changes() {
        let previous = TreeSnapshot::from_node(&Node::button("go", "Go")).unwrap();
        let next = TreeSnapshot::from_node(&Node::button("go", "Go").disabled(true)).unwrap();
        let diff = TreeDiff::between(&previous, &next);
        assert_eq!(diff.operations().len(), 1);
        assert!(matches!(diff.operations()[0], TreeOp::Update(_)));
    }

    #[test]
    fn layout_style_change_does_invalidate_layout() {
        let previous =
            TreeSnapshot::from_node(&Node::label_with_layout("a", "x", LayoutStyle::new()))
                .unwrap();
        let next = TreeSnapshot::from_node(&Node::label_with_layout(
            "a",
            "x",
            LayoutStyle::new().width(crate::layout::SizeMode::Fixed(20)),
        ))
        .unwrap();
        assert!(TreeDiff::between(&previous, &next).invalidates_layout());
    }
}
