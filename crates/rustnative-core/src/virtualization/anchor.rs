//! Keeping a scrolled list still while the data under it changes.
//!
//! A scroll offset is a number of pixels from the top of the list, which is
//! exactly the wrong thing to preserve when ten items are inserted *above*
//! the viewport: the pixels stay put and the content slides. What a person
//! expects is the opposite — the item they are looking at stays where it
//! is, and the list grows above it.
//!
//! [`ScrollAnchor`] is that expectation written down: the item at the top
//! of the viewport, and how far into it the viewport starts. Resolving the
//! anchor against the new list gives the offset that puts that item back
//! under the same pixel.

use crate::identity::NodeId;

use super::extent::ExtentCache;

/// Where a virtual list is scrolled to, expressed as an item rather than a
/// pixel offset.
///
/// # Example
///
/// ```
/// use rustnative_core::{ExtentCache, ItemExtent, NodeId, ScrollAnchor};
///
/// let extents = ExtentCache::new(100, ItemExtent::Fixed(20));
/// let item_at = |index: usize| Some(NodeId::from_key(&format!("row-{index}")));
///
/// // Scrolled so that row 10 is 5 px above the top of the viewport.
/// let anchor = ScrollAnchor::capture(205, &extents, item_at).expect("a list with items");
///
/// // Ten rows are inserted above it, so the row it anchored is now row 20.
/// let index_of = |node: NodeId| (node == NodeId::from_key("row-10")).then_some(20);
/// assert_eq!(anchor.resolve(&extents, index_of), Some(405), "the same row, same pixel");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollAnchor {
    item: NodeId,
    within: u32,
}

impl ScrollAnchor {
    /// Captures the anchor for `offset`: the item the viewport starts
    /// inside, and how far into it.
    ///
    /// `item_at` maps an item index to the node realizing it; it returns
    /// `None` for an item that is not realized, in which case there is
    /// nothing to anchor to and the caller keeps its pixel offset.
    #[must_use]
    pub fn capture(
        offset: u32,
        extents: &ExtentCache,
        item_at: impl Fn(usize) -> Option<NodeId>,
    ) -> Option<Self> {
        if extents.count() == 0 {
            return None;
        }
        let index = extents.index_at(offset);
        let item = item_at(index)?;
        Some(Self { item, within: offset.saturating_sub(extents.offset_of(index)) })
    }

    /// The node this anchor holds on to.
    #[must_use]
    pub const fn item(&self) -> NodeId {
        self.item
    }

    /// The scroll offset that puts the anchored item back where it was, or
    /// `None` if that item is gone — a list whose anchor was deleted has no
    /// opinion about where it should be, and the caller keeps what it has.
    #[must_use]
    pub fn resolve(
        &self,
        extents: &ExtentCache,
        index_of: impl Fn(NodeId) -> Option<usize>,
    ) -> Option<u32> {
        let index = index_of(self.item)?;
        Some(extents.offset_of(index).saturating_add(self.within))
    }
}

#[cfg(test)]
mod tests {
    use super::super::ItemExtent;
    use super::*;

    fn row(index: usize) -> NodeId {
        NodeId::from_key(&format!("row-{index}"))
    }

    fn rows(count: usize) -> ExtentCache {
        ExtentCache::new(count, ItemExtent::Fixed(20))
    }

    #[test]
    fn inserting_items_above_the_viewport_leaves_the_content_stationary() {
        let extents = rows(50);
        let anchor = ScrollAnchor::capture(200, &extents, |index| Some(row(index)))
            .expect("the list has items");
        assert_eq!(anchor.item(), row(10));

        // Ten rows inserted above: every old row is now ten later.
        let shifted = rows(60);
        let resolved = anchor
            .resolve(&shifted, |node| (0..50).find(|old| row(*old) == node).map(|old| old + 10))
            .expect("the anchored row is still in the list");
        assert_eq!(resolved, 400, "row 10 became row 20, so the offset follows it");
    }

    #[test]
    fn an_offset_inside_an_item_is_preserved_exactly() {
        let extents = rows(50);
        let anchor =
            ScrollAnchor::capture(207, &extents, |index| Some(row(index))).expect("has items");
        assert_eq!(
            anchor.resolve(&extents, |node| (node == row(10)).then_some(10)),
            Some(207),
            "an unchanged list resolves back to where it started"
        );
    }

    #[test]
    fn a_deleted_anchor_leaves_the_caller_to_keep_its_offset() {
        let extents = rows(50);
        let anchor =
            ScrollAnchor::capture(200, &extents, |index| Some(row(index))).expect("has items");
        assert_eq!(anchor.resolve(&extents, |_| None), None);
    }

    #[test]
    fn an_empty_list_has_nothing_to_anchor_to() {
        assert!(ScrollAnchor::capture(0, &rows(0), |index| Some(row(index))).is_none());
    }

    #[test]
    fn an_unrealized_anchor_item_is_not_captured() {
        let extents = rows(50);
        assert!(ScrollAnchor::capture(200, &extents, |_| None).is_none());
    }

    #[test]
    fn measured_items_anchor_by_their_own_extent() {
        let mut extents = ExtentCache::new(10, ItemExtent::Estimated(20));
        extents.record(0, 100);
        // Row 1 starts at 100 now; the viewport is 10 px into it.
        let anchor =
            ScrollAnchor::capture(110, &extents, |index| Some(row(index))).expect("has items");
        assert_eq!(anchor.item(), row(1));
        assert_eq!(anchor.resolve(&extents, |node| (node == row(1)).then_some(1)), Some(110));
    }
}
