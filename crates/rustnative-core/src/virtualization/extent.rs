//! Where each item in a virtual list starts, and how long the whole list
//! is.
//!
//! A virtual list knows how many items it has but has only ever *seen* the
//! handful it realized, so two questions have to be answerable for an item
//! that was never measured: where does item `n` begin, and which item is at
//! offset `y`? [`ExtentCache`] answers both.
//!
//! # Two shapes, because two costs
//!
//! A list of uniformly sized items needs no storage at all: every answer is
//! a multiplication or a division. A list of measured items needs per-item
//! storage, and a linear scan over it would make scrolling near the end of
//! a hundred-thousand-item list cost a hundred thousand additions *per
//! frame*. The measured shape is therefore a [Fenwick tree] (a binary
//! indexed tree): `O(log n)` to record a measurement, to sum a prefix, and
//! to find the item at an offset.
//!
//! [Fenwick tree]: https://en.wikipedia.org/wiki/Fenwick_tree
//!
//! # Saturation
//!
//! Extents are summed in `u64` and returned as `u32`, saturating. A list
//! long enough to overflow a `u32` of pixels cannot be scrolled to its end
//! by any real backend anyway, and saturating keeps the type honest instead
//! of wrapping a scroll position around to the top.

use super::ItemExtent;

/// Where every item in one virtual list starts.
///
/// Built from the list's [`ItemExtent`] policy and refined by
/// [`record`](Self::record) as the backend measures the items it realizes.
///
/// # Example
///
/// ```
/// use rustnative_core::{ExtentCache, ItemExtent};
///
/// // 100,000 rows, estimated at 24 px until measured.
/// let mut extents = ExtentCache::new(100_000, ItemExtent::Estimated(24));
/// assert_eq!(extents.offset_of(10), 240);
///
/// // The backend realized row 0 and it came out taller than estimated.
/// extents.record(0, 40);
/// assert_eq!(extents.offset_of(10), 40 + 9 * 24, "only the measured row changed");
/// assert_eq!(extents.index_at(40), 1, "row 1 now starts where row 0 ends");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtentCache {
    kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    /// Every item is exactly `extent` long, so nothing is stored per item.
    Uniform { count: usize, extent: u32 },
    /// Items start at `estimate` and are corrected as they are measured.
    Measured {
        estimate: u32,
        extents: Vec<u32>,
        /// A Fenwick tree over `extents`, one-based: `tree[i]` holds the
        /// sum of a block of extents ending at `i`.
        tree: Vec<u64>,
    },
}

impl ExtentCache {
    /// A cache for `count` items sized by `extent`.
    #[must_use]
    pub fn new(count: usize, extent: ItemExtent) -> Self {
        let kind = match extent {
            ItemExtent::Fixed(extent) => Kind::Uniform { count, extent },
            ItemExtent::Estimated(estimate) => {
                let extents = vec![estimate; count];
                let tree = build(&extents);
                Kind::Measured { estimate, extents, tree }
            }
        };
        Self { kind }
    }

    /// How many items this cache covers.
    #[must_use]
    pub fn count(&self) -> usize {
        match &self.kind {
            Kind::Uniform { count, .. } => *count,
            Kind::Measured { extents, .. } => extents.len(),
        }
    }

    /// Grows or shrinks the cache to `count` items, keeping what has
    /// already been measured about the items that remain.
    ///
    /// New items start at the estimate, exactly as they would have in a
    /// freshly built cache; this is what makes appending to a list cheap
    /// rather than a full remeasure.
    pub fn set_count(&mut self, count: usize) {
        match &mut self.kind {
            Kind::Uniform { count: current, .. } => *current = count,
            Kind::Measured { estimate, extents, tree } => {
                if extents.len() == count {
                    return;
                }
                extents.resize(count, *estimate);
                *tree = build(extents);
            }
        }
    }

    /// Records item `index`'s measured extent, reporting whether it changed
    /// anything.
    ///
    /// A cache built from [`ItemExtent::Fixed`] ignores measurements: the
    /// application stated the size, and a control that rounded itself to a
    /// different one must not silently move every item after it.
    pub fn record(&mut self, index: usize, extent: u32) -> bool {
        let Kind::Measured { extents, tree, .. } = &mut self.kind else {
            return false;
        };
        let Some(current) = extents.get_mut(index) else {
            return false;
        };
        if *current == extent {
            return false;
        }
        let delta = i64::from(extent) - i64::from(*current);
        *current = extent;
        add(tree, index, delta);
        true
    }

    /// Where item `index` starts, measured from the first item.
    ///
    /// An index past the end returns [`Self::total`], which is where a
    /// hypothetical item after the last one would begin.
    #[must_use]
    pub fn offset_of(&self, index: usize) -> u32 {
        match &self.kind {
            Kind::Uniform { count, extent } => {
                saturate(u64::from(*extent).saturating_mul(index.min(*count) as u64))
            }
            Kind::Measured { extents, tree, .. } => {
                saturate(prefix(tree, index.min(extents.len())))
            }
        }
    }

    /// How long item `index` is, or `0` for an index past the end.
    #[must_use]
    pub fn extent_of(&self, index: usize) -> u32 {
        match &self.kind {
            Kind::Uniform { count, extent } => {
                if index < *count {
                    *extent
                } else {
                    0
                }
            }
            Kind::Measured { extents, .. } => extents.get(index).copied().unwrap_or(0),
        }
    }

    /// The item containing `offset`, clamped to the last item.
    ///
    /// Items of zero extent are skipped rather than returned: an offset
    /// landing exactly where a zero-length item "is" belongs to the next
    /// item that actually occupies space.
    #[must_use]
    pub fn index_at(&self, offset: u32) -> usize {
        let count = self.count();
        if count == 0 {
            return 0;
        }
        let index = match &self.kind {
            Kind::Uniform { extent, .. } => {
                offset.checked_div(*extent).map_or(0, |index| index as usize)
            }
            Kind::Measured { tree, .. } => search(tree, u64::from(offset)),
        };
        index.min(count - 1)
    }

    /// The total length of every item together.
    #[must_use]
    pub fn total(&self) -> u32 {
        self.offset_of(self.count())
    }
}

/// Saturates a `u64` sum of extents into the `u32` this module exposes.
#[allow(
    clippy::cast_possible_truncation,
    reason = "clamped to u32::MAX first; the cast cannot truncate"
)]
fn saturate(value: u64) -> u32 {
    value.min(u64::from(u32::MAX)) as u32
}

/// Builds a Fenwick tree over `extents` in `O(n)`.
fn build(extents: &[u32]) -> Vec<u64> {
    let mut tree = vec![0u64; extents.len() + 1];
    for (index, extent) in extents.iter().enumerate() {
        let one_based = index + 1;
        tree[one_based] = tree[one_based].saturating_add(u64::from(*extent));
        let parent = one_based + lowest_bit(one_based);
        if parent < tree.len() {
            let carried = tree[one_based];
            tree[parent] = tree[parent].saturating_add(carried);
        }
    }
    tree
}

/// Adds `delta` to item `index`'s extent.
fn add(tree: &mut [u64], index: usize, delta: i64) {
    let mut one_based = index + 1;
    while one_based < tree.len() {
        tree[one_based] = tree[one_based].saturating_add_signed(delta);
        one_based += lowest_bit(one_based);
    }
}

/// The sum of the first `count` extents.
fn prefix(tree: &[u64], count: usize) -> u64 {
    let mut sum = 0u64;
    let mut one_based = count;
    while one_based > 0 {
        sum = sum.saturating_add(tree[one_based]);
        one_based -= lowest_bit(one_based);
    }
    sum
}

/// The index of the item containing `offset`: the largest `i` whose prefix
/// sum is still `<= offset`, found by descending the tree's implicit
/// binary structure rather than by scanning.
fn search(tree: &[u64], offset: u64) -> usize {
    let count = tree.len() - 1;
    let mut index = 0usize;
    let mut step = count.next_power_of_two();
    let mut remaining = offset;
    while step > 0 {
        let candidate = index + step;
        if candidate <= count && tree[candidate] <= remaining {
            remaining -= tree[candidate];
            index = candidate;
        }
        step /= 2;
    }
    index
}

/// The lowest set bit of a one-based Fenwick index — the size of the block
/// that index summarizes.
fn lowest_bit(index: usize) -> usize {
    index & index.wrapping_neg()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_list_needs_no_storage_and_answers_by_arithmetic() {
        let extents = ExtentCache::new(1_000_000, ItemExtent::Fixed(20));
        assert_eq!(extents.offset_of(999_999), 19_999_980);
        assert_eq!(extents.index_at(19_999_980), 999_999);
        assert_eq!(extents.total(), 20_000_000);
    }

    #[test]
    fn a_fixed_list_ignores_measurements() {
        let mut extents = ExtentCache::new(10, ItemExtent::Fixed(20));
        assert!(!extents.record(3, 45), "the application stated this size");
        assert_eq!(extents.offset_of(4), 80);
    }

    #[test]
    fn measurements_move_every_later_item_and_nothing_before_it() {
        let mut extents = ExtentCache::new(5, ItemExtent::Estimated(10));
        assert!(extents.record(2, 30));
        assert_eq!(extents.offset_of(0), 0);
        assert_eq!(extents.offset_of(2), 20, "items before the measured one do not move");
        assert_eq!(extents.offset_of(3), 50, "items after it shift by the difference");
        assert_eq!(extents.total(), 70);
    }

    #[test]
    fn recording_the_same_extent_twice_reports_no_change() {
        let mut extents = ExtentCache::new(3, ItemExtent::Estimated(10));
        assert!(extents.record(1, 12));
        assert!(!extents.record(1, 12), "nothing to recompute");
    }

    #[test]
    fn an_offset_inside_an_item_resolves_to_that_item() {
        let mut extents = ExtentCache::new(4, ItemExtent::Estimated(10));
        extents.record(1, 50);
        assert_eq!(extents.index_at(0), 0);
        assert_eq!(extents.index_at(9), 0);
        assert_eq!(extents.index_at(10), 1, "exactly on the boundary is the later item");
        assert_eq!(extents.index_at(59), 1);
        assert_eq!(extents.index_at(60), 2);
    }

    #[test]
    fn an_offset_past_the_end_clamps_to_the_last_item() {
        let extents = ExtentCache::new(3, ItemExtent::Estimated(10));
        assert_eq!(extents.index_at(u32::MAX), 2);
    }

    #[test]
    fn an_empty_list_has_no_extent_and_no_items() {
        let extents = ExtentCache::new(0, ItemExtent::Estimated(10));
        assert_eq!(extents.total(), 0);
        assert_eq!(extents.index_at(0), 0);
        assert_eq!(extents.offset_of(5), 0);
    }

    #[test]
    fn growing_keeps_what_was_already_measured() {
        let mut extents = ExtentCache::new(3, ItemExtent::Estimated(10));
        extents.record(0, 25);
        extents.set_count(6);
        assert_eq!(extents.count(), 6);
        assert_eq!(extents.offset_of(1), 25, "the measured first item is still 25 long");
        assert_eq!(extents.total(), 25 + 5 * 10);
    }

    #[test]
    fn shrinking_drops_the_items_that_are_gone() {
        let mut extents = ExtentCache::new(6, ItemExtent::Estimated(10));
        extents.record(5, 99);
        extents.set_count(3);
        assert_eq!(extents.total(), 30);
    }

    #[test]
    fn zero_extent_items_do_not_swallow_an_offset() {
        let mut extents = ExtentCache::new(4, ItemExtent::Estimated(10));
        extents.record(1, 0);
        extents.record(2, 0);
        assert_eq!(extents.index_at(10), 3, "the first item that occupies offset 10");
    }

    #[test]
    fn a_pathological_list_saturates_instead_of_wrapping() {
        let extents = ExtentCache::new(usize::MAX, ItemExtent::Fixed(u32::MAX));
        assert_eq!(extents.total(), u32::MAX);
    }
}
