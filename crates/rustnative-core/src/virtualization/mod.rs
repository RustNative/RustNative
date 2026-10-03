//! Showing a list of a hundred thousand items with a screenful of native
//! objects.
//!
//! # What virtualization is here
//!
//! A virtual list is **not a second list runtime**. It is an ordinary
//! container node ([`crate::Node::virtual_list`] builds a scrollable
//! [`Column`](crate::Node::column) or [`Row`](crate::Node::row)) carrying
//! one extra piece of information: how many items it *logically* has. Its
//! children are the window of items currently realized, each tagged with
//! its item index by [`Node::with_item_index`](crate::Node::with_item_index).
//! Everything else — identity, diffing, layout, native object recycling,
//! scrolling, accessibility — is the machinery that was already there.
//!
//! ```text
//!   item_count: 100_000          the list says how long it is
//!          │
//!          ▼
//!   ExtentCache                  where each item starts, measured or estimated
//!          │
//!          ▼
//!   VirtualRange::compute        which items the viewport needs right now
//!          │
//!          ▼
//!   Event::VisibleRangeChanged   the component renders that window of items
//! ```
//!
//! # What scrolling costs
//!
//! Scrolling inside a range costs nothing: the backend moves the container's
//! content window and no component is asked for anything (see
//! `rustnative_windows::native::rendering::scrolling`). Only crossing a range
//! boundary reaches the component, as one
//! [`Event::VisibleRangeChanged`](crate::Event::VisibleRangeChanged), and
//! only the items that entered or left the range are inserted or removed.
//! On Windows those insertions reuse the native windows the removals freed,
//! so a list scrolled from end to end creates a screenful of controls once.
//!
//! # Placement, padding, and gaps
//!
//! Items are laid end to end from the list's content origin, each at
//! [`ExtentCache::offset_of`] along the list's axis and filling it across.
//! A virtual list therefore ignores the container padding and gap an
//! ordinary column would apply: those would have to be folded into every
//! offset computation and every hit test, and an item that wants space
//! around it can simply include it. [`crate::Node::virtual_list`] builds a
//! container style with neither, so what is drawn matches what is
//! computed.

mod anchor;
mod extent;
mod lists;
mod range;

pub use anchor::ScrollAnchor;
pub use extent::ExtentCache;
pub use lists::{VirtualLists, index_of, item_at};
pub use range::VirtualRange;

/// The axis a virtual list runs along.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Axis {
    /// Items are stacked top to bottom; the list scrolls vertically.
    #[default]
    Vertical,
    /// Items are laid left to right; the list scrolls horizontally.
    Horizontal,
}

/// How long each item in a virtual list is along the list's axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ItemExtent {
    /// Every item is exactly this long. Nothing is measured and nothing is
    /// stored per item, so a list of any length costs the same.
    Fixed(u32),
    /// Items start at this estimate and are corrected as the backend
    /// measures the ones it realizes (see [`ExtentCache::record`]).
    ///
    /// The estimate is what the scrollbar and every unrealized item's
    /// position are computed from, so a badly wrong estimate shows up as a
    /// scroll range that shifts while scrolling — correct, but visibly
    /// restless. Estimating slightly high is kinder than estimating low.
    Estimated(u32),
}

impl Default for ItemExtent {
    fn default() -> Self {
        Self::Estimated(24)
    }
}

/// What a node needs to declare to become a virtual list: how many items it
/// has, how long they are, and how much to realize beyond the viewport.
///
/// # Example
///
/// ```
/// use rustnative_core::{ItemExtent, VirtualListStyle};
///
/// let style = VirtualListStyle::new(50_000, ItemExtent::Fixed(28)).overscan(4);
/// assert_eq!(style.item_count, 50_000);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VirtualListStyle {
    /// How many items the list logically has, realized or not.
    pub item_count: usize,
    /// How long each item is along [`Self::axis`].
    pub extent: ItemExtent,
    /// How many extra items to realize beyond each edge of the viewport, so
    /// a small scroll reveals an item that already exists.
    pub overscan: usize,
    /// The axis items are laid along, and the list scrolls in.
    pub axis: Axis,
}

impl Default for VirtualListStyle {
    fn default() -> Self {
        Self::new(0, ItemExtent::default())
    }
}

impl VirtualListStyle {
    /// A list of `item_count` items sized by `extent`, with the default
    /// overscan of two items on each side, running vertically.
    #[must_use]
    pub const fn new(item_count: usize, extent: ItemExtent) -> Self {
        Self { item_count, extent, overscan: 2, axis: Axis::Vertical }
    }

    /// Sets how many extra items are realized beyond each edge of the
    /// viewport.
    #[must_use]
    pub const fn overscan(mut self, overscan: usize) -> Self {
        self.overscan = overscan;
        self
    }

    /// Sets the axis the list runs along.
    #[must_use]
    pub const fn axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }

    /// An [`ExtentCache`] for this list.
    #[must_use]
    pub fn extents(&self) -> ExtentCache {
        ExtentCache::new(self.item_count, self.extent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_style_builds_the_cache_its_items_are_placed_from() {
        let extents = VirtualListStyle::new(1000, ItemExtent::Fixed(30)).extents();
        assert_eq!(extents.count(), 1000);
        assert_eq!(extents.offset_of(10), 300);
    }

    #[test]
    fn overscan_and_axis_round_trip() {
        let style =
            VirtualListStyle::new(10, ItemExtent::Fixed(1)).overscan(7).axis(Axis::Horizontal);
        assert_eq!(style.overscan, 7);
        assert_eq!(style.axis, Axis::Horizontal);
    }
}
