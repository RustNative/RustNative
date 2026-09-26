//! Which items a virtual list has to realize right now.

use super::extent::ExtentCache;

/// A half-open range of item indices: the items a virtual list should
/// realize.
///
/// # Example
///
/// ```
/// use rustnative_core::{ExtentCache, ItemExtent, VirtualRange};
///
/// let extents = ExtentCache::new(100_000, ItemExtent::Fixed(20));
/// // Scrolled 1000 px down a 300 px viewport, keeping two extra items
/// // above and below so a scroll does not reveal an empty strip.
/// let range = VirtualRange::compute(1000, 300, &extents, 2);
///
/// assert_eq!(range.first, 48, "item 50 is at 1000 px, less two of overscan");
/// assert_eq!(range.last_exclusive, 67);
/// assert_eq!(range.len(), 19, "19 items realized out of 100,000");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VirtualRange {
    /// The first item to realize.
    pub first: usize,
    /// One past the last item to realize.
    pub last_exclusive: usize,
}

impl VirtualRange {
    /// The range that realizes nothing.
    pub const EMPTY: Self = Self { first: 0, last_exclusive: 0 };

    /// The items needed to fill `viewport` at scroll position `offset`,
    /// plus `overscan` extra items on each side.
    ///
    /// Overscan is counted in *items*, not pixels: it exists so that a
    /// scroll of a few pixels reveals an item that already exists rather
    /// than one that has to be created mid-gesture, and "one more row" is
    /// the unit that actually means.
    ///
    /// The returned range always covers the viewport completely. A viewport
    /// of zero (a window being sized, a collapsed container) still realizes
    /// the item at `offset`, so a list is never left with nothing to show
    /// the moment it gains a size again.
    #[must_use]
    pub fn compute(offset: u32, viewport: u32, extents: &ExtentCache, overscan: usize) -> Self {
        let count = extents.count();
        if count == 0 {
            return Self::EMPTY;
        }
        let first_visible = extents.index_at(offset);
        // The viewport covers `offset .. offset + viewport`, exclusive at
        // the far end: an item starting exactly on the bottom edge is not
        // visible, so the last visible pixel is what decides the last item.
        let last_pixel = offset.saturating_add(viewport.max(1) - 1);
        let last_visible = extents.index_at(last_pixel);
        let first = first_visible.saturating_sub(overscan);
        let last_exclusive = last_visible.saturating_add(1).saturating_add(overscan).min(count);
        Self { first, last_exclusive }
    }

    /// How many items this range realizes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.last_exclusive.saturating_sub(self.first)
    }

    /// Whether this range realizes nothing.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether `index` is in this range.
    #[must_use]
    pub const fn contains(&self, index: usize) -> bool {
        index >= self.first && index < self.last_exclusive
    }

    /// The indices this range realizes, in order.
    #[must_use]
    pub fn indices(&self) -> std::ops::Range<usize> {
        self.first..self.last_exclusive
    }
}

#[cfg(test)]
mod tests {
    use super::super::ItemExtent;
    use super::*;

    fn fixed(count: usize) -> ExtentCache {
        ExtentCache::new(count, ItemExtent::Fixed(10))
    }

    #[test]
    fn a_range_covers_the_whole_viewport() {
        let extents = fixed(1000);
        let range = VirtualRange::compute(95, 30, &extents, 0);
        assert!(range.contains(9), "the partially visible item at the top is realized");
        assert!(range.contains(12), "so is the partially visible one at the bottom");
        assert_eq!(range, VirtualRange { first: 9, last_exclusive: 13 });
    }

    #[test]
    fn overscan_widens_the_range_on_both_sides() {
        let extents = fixed(1000);
        let plain = VirtualRange::compute(500, 100, &extents, 0);
        let padded = VirtualRange::compute(500, 100, &extents, 3);
        assert_eq!(padded.first, plain.first - 3);
        assert_eq!(padded.last_exclusive, plain.last_exclusive + 3);
    }

    #[test]
    fn overscan_never_runs_past_either_end_of_the_list() {
        let extents = fixed(20);
        let top = VirtualRange::compute(0, 50, &extents, 10);
        assert_eq!(top.first, 0);
        let bottom = VirtualRange::compute(150, 50, &extents, 10);
        assert_eq!(bottom.last_exclusive, 20);
    }

    #[test]
    fn an_empty_list_realizes_nothing() {
        let extents = fixed(0);
        assert_eq!(VirtualRange::compute(0, 500, &extents, 4), VirtualRange::EMPTY);
        assert!(VirtualRange::EMPTY.is_empty());
    }

    #[test]
    fn a_zero_height_viewport_still_realizes_the_item_it_is_scrolled_to() {
        let extents = fixed(100);
        let range = VirtualRange::compute(250, 0, &extents, 0);
        assert_eq!(range, VirtualRange { first: 25, last_exclusive: 26 });
    }

    #[test]
    fn scrolling_past_the_end_stays_inside_the_list() {
        let extents = fixed(100);
        let range = VirtualRange::compute(u32::MAX, 500, &extents, 5);
        assert_eq!(range.last_exclusive, 100);
        assert!(range.first < 100);
    }

    #[test]
    fn measured_items_move_the_range_with_them() {
        let mut extents = ExtentCache::new(100, ItemExtent::Estimated(10));
        // The first item turned out to be ten times its estimate, so a
        // viewport at the top now holds far fewer items.
        extents.record(0, 100);
        let range = VirtualRange::compute(0, 100, &extents, 0);
        assert_eq!(range, VirtualRange { first: 0, last_exclusive: 1 });
    }
}
