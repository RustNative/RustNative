//! One window's virtual lists: what each has measured, where it is
//! scrolled, and which items it therefore needs realized.
//!
//! The portable half of this lives in `rustnative_core::virtualization` —
//! extents, ranges, anchors, and the layout that places items at them. What
//! is left here is the part that needs a live window: reading each list's
//! current scroll offset and viewport, noticing when the range those imply
//! has changed, and asking the component for that new window of items.
//!
//! # Why this is not in the renderer
//!
//! `Renderer` already coordinates realization, layout, styling, and
//! scrolling. Virtualization touches all four, which is exactly the reason
//! to keep its state in one place rather than spread across them: the
//! renderer asks this module a question at each phase boundary
//! ("what are the caches?", "did measuring move anything?", "did any range
//! change?") and stays a coordinator.

use std::collections::HashMap;

use rustnative_core::Axis;
use rustnative_core::{
    ExtentCache, MeasuredItem, NodeId, Point, Rect, ScrollAnchor, TreeSnapshot, VirtualListStyle,
    VirtualRange,
};

/// What one virtual list is, beyond its measured extents.
#[derive(Debug)]
struct ListMeta {
    style: VirtualListStyle,
    /// The range last reported to the component.
    range: VirtualRange,
    /// The item to hold still across the render in flight, if one was
    /// captured (see [`VirtualLists::capture_anchors`]).
    anchor: Option<ScrollAnchor>,
}

/// Every virtual list in one window.
#[derive(Debug, Default)]
pub(crate) struct VirtualLists {
    /// Each list's extent cache, in the shape the layout engine takes.
    extents: HashMap<NodeId, ExtentCache>,
    meta: HashMap<NodeId, ListMeta>,
    /// Ranges that changed and have not yet been reported to a component.
    pending: Vec<(NodeId, VirtualRange)>,
    /// Whether a `VisibleRangeChanged` dispatch is already in flight for
    /// this window — see [`Self::is_dispatching`].
    dispatching: bool,
}

impl VirtualLists {
    /// Whether this window has any virtual list at all.
    ///
    /// Every phase hook below is a no-op for a window without one, and this
    /// is what lets the renderer skip them outright.
    pub(crate) fn is_empty(&self) -> bool {
        self.meta.is_empty()
    }

    /// The extent caches, keyed by list, for the layout engine.
    pub(crate) fn extents(&self) -> &HashMap<NodeId, ExtentCache> {
        &self.extents
    }

    /// Adds, updates, and drops list state to match `snapshot`.
    ///
    /// A list whose item count changed keeps what it has measured about the
    /// items that remain (see [`ExtentCache::set_count`]); one whose extent
    /// policy changed starts over, because what it measured was measured
    /// under a different policy.
    pub(crate) fn sync(&mut self, snapshot: &TreeSnapshot) {
        for node in snapshot.nodes() {
            let Some(style) = node.virtualization else {
                continue;
            };
            match self.meta.get_mut(&node.id) {
                Some(meta) if meta.style.extent == style.extent => {
                    meta.style = style;
                    if let Some(extents) = self.extents.get_mut(&node.id) {
                        extents.set_count(style.item_count);
                    }
                }
                _ => {
                    self.extents.insert(node.id, style.extents());
                    self.meta.insert(
                        node.id,
                        ListMeta { style, range: VirtualRange::EMPTY, anchor: None },
                    );
                }
            }
        }
        self.extents.retain(|id, _| snapshot.contains(*id));
        self.meta.retain(|id, _| snapshot.contains(*id));
    }

    /// Records what layout measured, reporting whether any of it moved an
    /// item's offset.
    ///
    /// A `true` here is the renderer's signal to lay out once more: the
    /// offsets it just used are now known to be wrong.
    pub(crate) fn record(&mut self, measured: &[MeasuredItem]) -> bool {
        let mut changed = false;
        for item in measured {
            if let Some(extents) = self.extents.get_mut(&item.list) {
                changed |= extents.record(item.index, item.extent);
            }
        }
        changed
    }

    /// Recomputes every list's visible range from where it is scrolled and
    /// how big its viewport is, queueing the ones that changed.
    ///
    /// `viewport_of` gives a list's laid-out rectangle and `offset_of` its
    /// current scroll offset; both come from the renderer, which owns them.
    pub(crate) fn update_ranges(
        &mut self,
        viewport_of: impl Fn(NodeId) -> Option<Rect>,
        offset_of: impl Fn(NodeId) -> Point,
    ) {
        for (id, meta) in &mut self.meta {
            let Some(viewport) = viewport_of(*id) else {
                continue;
            };
            let Some(extents) = self.extents.get(id) else {
                continue;
            };
            let (offset, length) = along(meta.style.axis, offset_of(*id), viewport);
            let range = VirtualRange::compute(offset, length, extents, meta.style.overscan);
            if range == meta.range {
                continue;
            }
            meta.range = range;
            self.pending.retain(|(pending, _)| pending != id);
            self.pending.push((*id, range));
        }
    }

    /// The range changes not yet reported to a component.
    pub(crate) fn take_changes(&mut self) -> Vec<(NodeId, VirtualRange)> {
        std::mem::take(&mut self.pending)
    }

    /// Whether a range change is already being dispatched for this window.
    ///
    /// Answering a `VisibleRangeChanged` renders new items, which lays out,
    /// which can compute a further range — the ordinary way a list settles
    /// after its items turn out to be a different size than estimated.
    /// Letting that recurse would nest one dispatch inside another; instead
    /// the inner pass queues its change and the outer loop picks it up.
    pub(crate) fn is_dispatching(&self) -> bool {
        self.dispatching
    }

    /// Marks the start or end of a dispatch loop (see
    /// [`Self::is_dispatching`]).
    pub(crate) fn set_dispatching(&mut self, dispatching: bool) {
        self.dispatching = dispatching;
    }

    /// Remembers, for each list, which item is at the top of its viewport,
    /// so a render that inserts or removes items above it can put it back.
    ///
    /// `item_at` finds the node realizing one item of one list.
    pub(crate) fn capture_anchors(
        &mut self,
        offset_of: impl Fn(NodeId) -> Point,
        item_at: impl Fn(NodeId, usize) -> Option<NodeId>,
    ) {
        for (id, meta) in &mut self.meta {
            let Some(extents) = self.extents.get(id) else {
                continue;
            };
            let (offset, _) = along(meta.style.axis, offset_of(*id), Rect::new(0, 0, 0, 0));
            meta.anchor = ScrollAnchor::capture(offset, extents, |index| item_at(*id, index));
        }
    }

    /// The scroll offset each list should be at for its anchored item to be
    /// back where it was, for the lists where that is known.
    ///
    /// `index_of` gives the item index a node now realizes.
    pub(crate) fn resolve_anchors(
        &mut self,
        index_of: impl Fn(NodeId) -> Option<usize>,
    ) -> Vec<(NodeId, Point)> {
        let mut offsets = Vec::new();
        for (id, meta) in &mut self.meta {
            let Some(anchor) = meta.anchor.take() else {
                continue;
            };
            let Some(extents) = self.extents.get(id) else {
                continue;
            };
            let Some(offset) = anchor.resolve(extents, &index_of) else {
                continue;
            };
            let offset = offset.min(i32::MAX.unsigned_abs());
            #[allow(clippy::cast_possible_wrap, reason = "clamped into i32's range above")]
            let offset = offset as i32;
            offsets.push((
                *id,
                match meta.style.axis {
                    Axis::Vertical => Point::new(0, offset),
                    Axis::Horizontal => Point::new(offset, 0),
                },
            ));
        }
        offsets
    }
}

/// A list's scroll offset and viewport length along its own axis.
fn along(axis: Axis, offset: Point, viewport: Rect) -> (u32, u32) {
    #[allow(clippy::cast_sign_loss, reason = "floored at zero first")]
    let to_u32 = |value: i32| value.max(0) as u32;
    match axis {
        Axis::Vertical => (to_u32(offset.y), to_u32(viewport.height)),
        Axis::Horizontal => (to_u32(offset.x), to_u32(viewport.width)),
    }
}

#[cfg(test)]
mod tests {
    use rustnative_core::{ItemExtent, Node};

    use super::*;

    fn list(count: usize, extent: ItemExtent) -> TreeSnapshot {
        let rows = (0..count.min(3))
            .map(|index| Node::label(format!("row-{index}"), "row").with_item_index(index));
        TreeSnapshot::from_node(&Node::virtual_list(
            "list",
            VirtualListStyle::new(count, extent).overscan(0),
            rows,
        ))
        .expect("a valid tree")
    }

    fn id() -> NodeId {
        NodeId::from_key("list")
    }

    fn viewport(height: i32) -> impl Fn(NodeId) -> Option<Rect> {
        move |_| Some(Rect::new(0, 0, 100, height))
    }

    #[test]
    fn a_list_gets_a_cache_covering_every_item_not_just_the_realized_ones() {
        let mut lists = VirtualLists::default();
        lists.sync(&list(100_000, ItemExtent::Fixed(20)));
        assert_eq!(lists.extents()[&id()].count(), 100_000);
        assert_eq!(lists.extents()[&id()].total(), 2_000_000);
    }

    #[test]
    fn only_a_changed_range_is_reported() {
        let mut lists = VirtualLists::default();
        lists.sync(&list(1000, ItemExtent::Fixed(20)));
        // A 90 px viewport over 20 px rows shows rows 0..5.
        lists.update_ranges(viewport(90), |_| Point::new(0, 0));
        assert_eq!(
            lists.take_changes(),
            vec![(id(), VirtualRange { first: 0, last_exclusive: 5 })]
        );

        // Scrolled 5 px: the same five rows still cover the viewport.
        lists.update_ranges(viewport(90), |_| Point::new(0, 5));
        assert!(lists.take_changes().is_empty(), "scrolling inside a range must reach no one");

        // A whole row further down, and the window of rows moves with it.
        lists.update_ranges(viewport(90), |_| Point::new(0, 20));
        assert_eq!(
            lists.take_changes(),
            vec![(id(), VirtualRange { first: 1, last_exclusive: 6 })]
        );
    }

    #[test]
    fn measuring_an_item_moves_the_offsets_after_it() {
        let mut lists = VirtualLists::default();
        lists.sync(&list(100, ItemExtent::Estimated(20)));
        assert!(lists.record(&[MeasuredItem { list: id(), index: 0, extent: 60 }]));
        assert_eq!(lists.extents()[&id()].offset_of(1), 60);
        assert!(
            !lists.record(&[MeasuredItem { list: id(), index: 0, extent: 60 }]),
            "the same measurement twice must not trigger another layout pass"
        );
    }

    #[test]
    fn a_fixed_list_learns_nothing_from_measuring() {
        let mut lists = VirtualLists::default();
        lists.sync(&list(100, ItemExtent::Fixed(20)));
        assert!(!lists.record(&[MeasuredItem { list: id(), index: 0, extent: 999 }]));
    }

    #[test]
    fn growing_a_list_keeps_what_it_measured() {
        let mut lists = VirtualLists::default();
        lists.sync(&list(100, ItemExtent::Estimated(20)));
        lists.record(&[MeasuredItem { list: id(), index: 0, extent: 60 }]);
        lists.sync(&list(200, ItemExtent::Estimated(20)));
        assert_eq!(lists.extents()[&id()].count(), 200);
        assert_eq!(lists.extents()[&id()].offset_of(1), 60, "the measured row is still 60 long");
    }

    #[test]
    fn changing_the_extent_policy_starts_the_measurements_over() {
        let mut lists = VirtualLists::default();
        lists.sync(&list(100, ItemExtent::Estimated(20)));
        lists.record(&[MeasuredItem { list: id(), index: 0, extent: 60 }]);
        lists.sync(&list(100, ItemExtent::Fixed(30)));
        assert_eq!(lists.extents()[&id()].offset_of(1), 30);
    }

    #[test]
    fn a_removed_list_stops_being_tracked() {
        let mut lists = VirtualLists::default();
        lists.sync(&list(100, ItemExtent::Fixed(20)));
        let empty = TreeSnapshot::from_node(&Node::label("nothing", "")).expect("a valid tree");
        lists.sync(&empty);
        assert!(lists.is_empty());
        assert!(lists.extents().is_empty());
    }

    #[test]
    fn an_anchor_puts_the_same_item_back_under_the_same_pixel() {
        let mut lists = VirtualLists::default();
        lists.sync(&list(100, ItemExtent::Fixed(20)));
        // Scrolled to row 5, with 3 px of it above the viewport.
        lists.capture_anchors(
            |_| Point::new(0, 103),
            |_, index| Some(NodeId::from_key(&format!("row-{index}"))),
        );
        // Ten rows were inserted above it, so it is row 15 now.
        let resolved = lists.resolve_anchors(|_| Some(15));
        assert_eq!(resolved, vec![(id(), Point::new(0, 303))]);
        assert!(lists.resolve_anchors(|_| Some(15)).is_empty(), "an anchor is used once");
    }
}
