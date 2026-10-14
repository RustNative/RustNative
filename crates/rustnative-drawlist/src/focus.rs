//! Focus and hit-testing for hosts with no native controls.
//!
//! On a native host the toolkit moves focus and decides what a click hit.
//! A drawn host has to do both itself, and must do them exactly as the
//! portable model says (`PLAN.md` Milestone 11, and Milestone 38's "focus
//! and traversal reuse the portable focus model unchanged"):
//!
//! - the traversal order is every focusable, enabled, shown node in tree
//!   (preorder) order — the order Tab visits on every host;
//! - Tab moves forward and wraps, Shift+Tab moves back and wraps, and with
//!   nothing focused they start from the first or the last;
//! - a click hits the last-drawn (deepest, latest) node whose visible area
//!   contains the point.

use rustnative_core::{NodeId, Rect, TreeSnapshot};

/// The keyboard traversal order of one realized tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FocusModel {
    order: Vec<NodeId>,
}

impl FocusModel {
    /// The traversal order of `snapshot`.
    #[must_use]
    pub fn from_snapshot(snapshot: &TreeSnapshot) -> Self {
        let order = snapshot
            .ordered_nodes()
            .into_iter()
            .filter(|node| {
                node.accessibility.is_focusable()
                    && !node.disabled
                    && !snapshot.is_effectively_hidden(node.id)
            })
            .map(|node| node.id)
            .collect();
        Self { order }
    }

    /// An explicit order (for a host that filters further, such as by what
    /// is on screen).
    #[must_use]
    pub const fn from_order(order: Vec<NodeId>) -> Self {
        Self { order }
    }

    /// The nodes Tab visits, in order.
    #[must_use]
    pub fn order(&self) -> &[NodeId] {
        &self.order
    }

    /// Whether `node` can take focus.
    #[must_use]
    pub fn contains(&self, node: NodeId) -> bool {
        self.order.contains(&node)
    }

    /// Where Tab (`backwards` false) or Shift+Tab (`backwards` true) moves
    /// focus from `current`; `None` only when nothing can take focus.
    #[must_use]
    pub fn next(&self, current: Option<NodeId>, backwards: bool) -> Option<NodeId> {
        let count = self.order.len();
        if count == 0 {
            return None;
        }
        let position = current.and_then(|id| self.order.iter().position(|node| *node == id));
        let index = match (position, backwards) {
            (None, false) => 0,
            (None, true) => count - 1,
            (Some(index), false) => (index + 1) % count,
            (Some(index), true) => (index + count - 1) % count,
        };
        Some(self.order[index])
    }
}

/// The last of `items` (in drawing order) whose rectangle contains `point`.
pub fn hit_test(
    items: impl IntoIterator<Item = (NodeId, Rect)>,
    point: rustnative_core::Point,
) -> Option<NodeId> {
    items
        .into_iter()
        .filter(|(_, rect)| {
            point.x >= rect.x
                && point.y >= rect.y
                && point.x < rect.x.saturating_add(rect.width)
                && point.y < rect.y.saturating_add(rect.height)
        })
        .last()
        .map(|(id, _)| id)
}

#[cfg(test)]
mod tests {
    use rustnative_core::{Node, Point};

    use super::*;

    fn snapshot() -> TreeSnapshot {
        let view = Node::column(
            "root",
            [
                Node::label("title", "Title"),
                Node::button("first", "First"),
                Node::button("off", "Off").disabled(true),
                Node::text_input("field", ""),
                Node::button("gone", "Gone").hidden(true),
                Node::button("last", "Last"),
            ],
        );
        TreeSnapshot::from_node(&view).expect("unique keys")
    }

    #[test]
    fn traversal_skips_what_cannot_take_focus_and_wraps() {
        let model = FocusModel::from_snapshot(&snapshot());
        let keys: Vec<_> = model.order().iter().filter_map(|id| id.local_key()).collect();
        assert_eq!(keys, ["first", "field", "last"]);
        let first = NodeId::from_key("first");
        let last = NodeId::from_key("last");
        assert_eq!(model.next(None, false), Some(first));
        assert_eq!(model.next(None, true), Some(last));
        assert_eq!(model.next(Some(last), false), Some(first), "wraps forward");
        assert_eq!(model.next(Some(first), true), Some(last), "wraps back");
        assert_eq!(FocusModel::default().next(None, false), None);
    }

    #[test]
    fn the_topmost_hit_wins() {
        let a = NodeId::from_key("a");
        let b = NodeId::from_key("b");
        let items = [(a, Rect::new(0, 0, 100, 100)), (b, Rect::new(10, 10, 10, 10))];
        assert_eq!(hit_test(items, Point::new(15, 15)), Some(b));
        assert_eq!(hit_test(items, Point::new(50, 50)), Some(a));
        assert_eq!(hit_test(items, Point::new(100, 0)), None);
    }
}
