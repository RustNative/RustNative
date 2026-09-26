//! Matched-geometry transitions (`PLAN.md` Milestone 48, `C25`): a node
//! that leaves and a node that arrives with the same declared shared
//! identity are one thing moving, so the arriving one starts where the
//! leaving one was.

use std::collections::HashMap;
use std::time::Duration;

use super::{AnimatedProperty, Transition};
use crate::identity::NodeId;
use crate::layout::Rect;
use crate::reconcile::TreeSnapshot;

/// One arriving node and where it moves from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatchedGeometry {
    /// The node that arrived.
    pub node: NodeId,
    /// The rectangle the node it replaces occupied.
    pub from: Rect,
    /// How it moves: its own position transition, or a short ease.
    pub transition: Transition,
}

/// The default motion for a node that declares no position transition.
const DEFAULT_MOTION: Duration = Duration::from_millis(250);

/// Pairs each node in `next` that did not exist in `previous` with the node
/// of the same shared identity that `previous` had and `next` does not,
/// and says where it moves from (its old rectangle in `previous_rects`).
///
/// A backend calls this between a render and its layout, and starts
/// position and size transitions from `from` to wherever layout puts the
/// arriving node. A shared identity that appears on more than one node of
/// a tree matches none of them: which one moved would be a guess.
#[must_use]
pub fn matched_geometry<S: std::hash::BuildHasher>(
    previous: &TreeSnapshot,
    previous_rects: &HashMap<NodeId, Rect, S>,
    next: &TreeSnapshot,
) -> Vec<MatchedGeometry> {
    let unique = |snapshot: &TreeSnapshot| {
        let mut by_shared: HashMap<NodeId, Option<NodeId>> = HashMap::new();
        for node in snapshot.nodes() {
            if let Some(shared) = node.shared_id {
                by_shared.entry(shared).and_modify(|owner| *owner = None).or_insert(Some(node.id));
            }
        }
        by_shared
    };
    let leaving = unique(previous);
    let mut matched = unique(next)
        .into_iter()
        .filter_map(|(shared, owner)| {
            let arriving = owner?;
            let left = (*leaving.get(&shared)?)?;
            if left == arriving || previous.contains(arriving) || next.contains(left) {
                return None;
            }
            let from = *previous_rects.get(&left)?;
            let transition = next
                .get(arriving)?
                .transitions
                .iter()
                .find(|declared| declared.property == AnimatedProperty::Position)
                .map_or(Transition::new(DEFAULT_MOTION), |declared| declared.transition);
            Some(MatchedGeometry { node: arriving, from, transition })
        })
        .collect::<Vec<_>>();
    matched.sort_by_key(|entry| entry.node);
    matched
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Node;

    #[test]
    fn an_arriving_node_moves_from_the_one_it_replaces() {
        let grid = Node::column("grid", [Node::label("thumb", "7").with_shared_id("photo")]);
        let detail = Node::column("detail", [Node::label("hero", "7").with_shared_id("photo")]);
        let previous = TreeSnapshot::from_node(&grid).unwrap();
        let next = TreeSnapshot::from_node(&detail).unwrap();
        let rects = HashMap::from([(NodeId::from_key("thumb"), Rect::new(10, 20, 30, 40))]);
        let matched = matched_geometry(&previous, &rects, &next);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].node, NodeId::from_key("hero"));
        assert_eq!(matched[0].from, Rect::new(10, 20, 30, 40));

        let same = matched_geometry(&previous, &rects, &previous);
        assert!(same.is_empty(), "a node that stays is laid out, not matched");
    }
}
