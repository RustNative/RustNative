//! Plain geometric and layout-direction value types.
//!
//! These are intentionally simple public-field data carriers rather than
//! encapsulated types with accessor methods: they have no invariants beyond
//! "these are numbers", every arithmetic consumer (the layout engine, both
//! crates' tests, a platform backend translating a `Rect` into a native
//! placement call) needs unrestricted field access, and getters here would
//! add call-site noise without protecting anything — the same reasoning the
//! Rust ecosystem generally applies to types like `std::ops::Range` or a
//! `Point { x, y }`. Contrast this with `rustnative_core::Constraints`, whose fields *do* carry a real invariant (`min <= max`) and
//! are therefore private.

/// A size in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Size {
    /// The width, in logical pixels.
    pub width: u32,
    /// The height, in logical pixels.
    pub height: u32,
}

impl Size {
    /// Creates a size from its width and height.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

/// A rectangle in a native parent's local coordinate space (see
/// `rustnative_core::LayoutEngine` for the coordinate-space contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rect {
    /// The x-coordinate of the rectangle's top-left corner.
    pub x: i32,
    /// The y-coordinate of the rectangle's top-left corner.
    pub y: i32,
    /// The rectangle's width.
    pub width: i32,
    /// The rectangle's height.
    pub height: i32,
}

impl Rect {
    /// Creates a rectangle from its top-left corner and size.
    #[must_use]
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self { x, y, width, height }
    }
}

/// How a node's width or height along one axis is determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SizeMode {
    /// Sized to fit the node's content.
    #[default]
    Auto,
    /// Sized to an exact value, in logical pixels.
    Fixed(i32),
    /// Sized to fill the remaining space its parent offers.
    Fill,
}

/// Cross-axis (or `align_self`) alignment within the space a node is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Alignment {
    /// Aligned to the start of the cross axis.
    Start,
    /// Centered on the cross axis.
    Center,
    /// Aligned to the end of the cross axis.
    End,
    /// Stretched to fill the cross axis.
    #[default]
    Stretch,
}

/// A position in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Point {
    /// The x-coordinate.
    pub x: i32,
    /// The y-coordinate.
    pub y: i32,
}

impl Point {
    /// Creates a point from its coordinates.
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// How a container handles content that exceeds its bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Overflow {
    /// Content is drawn even where it exceeds the container's bounds.
    Visible,
    /// Content beyond the container's bounds is clipped.
    #[default]
    Clip,
    /// Content beyond the container's bounds is reachable by scrolling.
    Scroll,
}

/// Independent top/right/bottom/left insets (padding or margin).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EdgeInsets {
    /// The top inset.
    pub top: i32,
    /// The inset at the end of a line of text: the right in a
    /// left-to-right layout, the left in a right-to-left one.
    pub end: i32,
    /// The bottom inset.
    pub bottom: i32,
    /// The inset at the start of a line of text: the left in a
    /// left-to-right layout, the right in a right-to-left one.
    pub start: i32,
}

impl EdgeInsets {
    /// Creates equal insets on all four edges.
    #[must_use]
    pub const fn all(value: i32) -> Self {
        Self { top: value, end: value, bottom: value, start: value }
    }

    /// Creates insets that are equal on the top/bottom edges and equal on
    /// the start/end edges.
    #[must_use]
    pub const fn symmetric(vertical: i32, horizontal: i32) -> Self {
        Self { top: vertical, end: horizontal, bottom: vertical, start: horizontal }
    }

    /// The sum of the start and end insets. Saturating: an adversarial or
    /// pathologically large edge-inset pair clamps instead of overflowing
    /// (every layout computation in `rustnative_core` makes this same
    /// choice).
    #[must_use]
    pub const fn horizontal(self) -> i32 {
        self.start.saturating_add(self.end)
    }

    /// The sum of the top and bottom insets. See [`Self::horizontal`].
    #[must_use]
    pub const fn vertical(self) -> i32 {
        self.top.saturating_add(self.bottom)
    }
}

impl EdgeInsets {
    /// Insets in logical order — top, end, bottom, start — the order CSS's
    /// logical shorthands use. Start and end follow the layout direction,
    /// which is what lets a right-to-left locale mirror a screen without
    /// the application restating its spacing.
    #[must_use]
    pub const fn logical(top: i32, end: i32, bottom: i32, start: i32) -> Self {
        Self { top, end, bottom, start }
    }

    /// The physical left inset under `direction`.
    #[must_use]
    pub const fn left(self, direction: LayoutDirection) -> i32 {
        match direction {
            LayoutDirection::Ltr => self.start,
            LayoutDirection::Rtl => self.end,
        }
    }

    /// The physical right inset under `direction`.
    #[must_use]
    pub const fn right(self, direction: LayoutDirection) -> i32 {
        match direction {
            LayoutDirection::Ltr => self.end,
            LayoutDirection::Rtl => self.start,
        }
    }
}

/// Which way lines of text — and therefore rows, start/end insets, and
/// start/end alignment — run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LayoutDirection {
    /// Left to right: Latin, Cyrillic, Greek, CJK, Devanagari, …
    #[default]
    Ltr,
    /// Right to left: Arabic, Hebrew, Persian, Urdu, …
    Rtl,
}

impl LayoutDirection {
    /// Whether this is right-to-left.
    #[must_use]
    pub const fn is_rtl(self) -> bool {
        matches!(self, Self::Rtl)
    }
}

impl Default for EdgeInsets {
    fn default() -> Self {
        Self::all(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_insets_sums_saturate_instead_of_overflowing() {
        let insets = EdgeInsets::all(i32::MAX);
        assert_eq!(insets.horizontal(), i32::MAX);
        assert_eq!(insets.vertical(), i32::MAX);
    }
}
