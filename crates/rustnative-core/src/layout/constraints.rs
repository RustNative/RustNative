//! Per-node layout parameters: size constraints and container styling.

use super::geometry::{Alignment, EdgeInsets, Overflow, SizeMode};

/// Min/max size bounds for a node's layout.
///
/// Fields are private and every constructor/setter enforces
/// `0 <= min <= max` (when a max is set), so an invalid `Constraints` (for
/// example `max_width < min_width`) can never be constructed at all, rather
/// than being silently re-clamped at every place a value is measured against
/// it. This is the model this crate follows wherever a type's fields carry a
/// real invariant — contrast with the plain data carriers in
/// `crate::layout::geometry`, which have none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(from = "ConstraintsRepr")]
pub struct Constraints {
    min_width: i32,
    max_width: Option<i32>,
    min_height: i32,
    max_height: Option<i32>,
}

/// The wire form of [`Constraints`], rebuilt through its setters so a
/// transmitted value cannot break the `min <= max` invariant.
#[derive(serde::Deserialize)]
struct ConstraintsRepr {
    min_width: i32,
    max_width: Option<i32>,
    min_height: i32,
    max_height: Option<i32>,
}

impl From<ConstraintsRepr> for Constraints {
    fn from(repr: ConstraintsRepr) -> Self {
        let mut constraints =
            Self::new().with_min_width(repr.min_width).with_min_height(repr.min_height);
        if let Some(max) = repr.max_width {
            constraints = constraints.with_max_width(max);
        }
        if let Some(max) = repr.max_height {
            constraints = constraints.with_max_height(max);
        }
        constraints
    }
}

impl Constraints {
    /// Creates unconstrained bounds (`min == 0`, no `max`).
    #[must_use]
    pub const fn new() -> Self {
        Self { min_width: 0, max_width: None, min_height: 0, max_height: None }
    }

    /// Sets the minimum width, clamped to be non-negative; raises the
    /// maximum width to match if it would otherwise fall below the new
    /// minimum.
    #[must_use]
    pub const fn with_min_width(mut self, value: i32) -> Self {
        self.min_width = non_negative(value);
        self.max_width = raise_to(self.max_width, self.min_width);
        self
    }

    /// Sets the maximum width, clamped to be non-negative and no lower than
    /// the current minimum width.
    #[must_use]
    pub const fn with_max_width(mut self, value: i32) -> Self {
        let value = non_negative(value);
        self.max_width = Some(if value > self.min_width { value } else { self.min_width });
        self
    }

    /// Sets the minimum height, clamped to be non-negative; raises the
    /// maximum height to match if it would otherwise fall below the new
    /// minimum.
    #[must_use]
    pub const fn with_min_height(mut self, value: i32) -> Self {
        self.min_height = non_negative(value);
        self.max_height = raise_to(self.max_height, self.min_height);
        self
    }

    /// Sets the maximum height, clamped to be non-negative and no lower
    /// than the current minimum height.
    #[must_use]
    pub const fn with_max_height(mut self, value: i32) -> Self {
        let value = non_negative(value);
        self.max_height = Some(if value > self.min_height { value } else { self.min_height });
        self
    }

    /// Returns the minimum width.
    #[must_use]
    pub const fn min_width(&self) -> i32 {
        self.min_width
    }

    /// Returns the maximum width, if set.
    #[must_use]
    pub const fn max_width(&self) -> Option<i32> {
        self.max_width
    }

    /// Returns the minimum height.
    #[must_use]
    pub const fn min_height(&self) -> i32 {
        self.min_height
    }

    /// Returns the maximum height, if set.
    #[must_use]
    pub const fn max_height(&self) -> Option<i32> {
        self.max_height
    }

    /// Clamps `value` into `[min_width, max_width]`.
    #[must_use]
    pub const fn clamp_width(self, value: i32) -> i32 {
        let value = if value < self.min_width { self.min_width } else { value };
        match self.max_width {
            // Invariant established by the constructors above:
            // `max_width >= min_width` always holds here, so no defensive
            // re-clamp of `max` against `min` is needed at this use site.
            Some(max) if value > max => max,
            _ => value,
        }
    }

    /// Clamps `value` into `[min_height, max_height]`.
    #[must_use]
    pub const fn clamp_height(self, value: i32) -> i32 {
        let value = if value < self.min_height { self.min_height } else { value };
        match self.max_height {
            Some(max) if value > max => max,
            _ => value,
        }
    }
}

const fn non_negative(value: i32) -> i32 {
    if value > 0 { value } else { 0 }
}

const fn raise_to(max: Option<i32>, min: i32) -> Option<i32> {
    match max {
        Some(max) if max < min => Some(min),
        other => other,
    }
}

/// A node's own sizing, margin, alignment, and constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LayoutStyle {
    /// How this node's width is determined.
    pub width: SizeMode,
    /// How this node's height is determined.
    pub height: SizeMode,
    /// Space reserved outside this node's border, on each edge.
    pub margin: EdgeInsets,
    /// This node's own cross-axis alignment override, if any, taking
    /// precedence over its parent container's `align_items`.
    pub align_self: Option<Alignment>,
    /// Min/max size bounds applied after `width`/`height` are resolved.
    pub constraints: Constraints,
    /// This node's layout direction, overriding the one it inherits: how
    /// a subtree of one script is embedded in a screen of another. `None`
    /// inherits from the parent, and the window root from the
    /// environment's [`crate::keys::LAYOUT_DIRECTION`].
    pub direction: Option<super::LayoutDirection>,
    /// Where this node goes in a parent grid (see [`crate::Node::grid`]);
    /// ignored by every other container.
    pub grid: Option<super::GridPlacement>,
}

impl Default for LayoutStyle {
    fn default() -> Self {
        Self {
            width: SizeMode::Fill,
            height: SizeMode::Auto,
            margin: EdgeInsets::default(),
            align_self: None,
            constraints: Constraints::default(),
            direction: None,
            grid: None,
        }
    }
}

impl LayoutStyle {
    /// Creates a layout style with the default width/height modes, no
    /// margin, no self-alignment override, and unconstrained bounds.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            width: SizeMode::Fill,
            height: SizeMode::Auto,
            margin: EdgeInsets { top: 0, end: 0, bottom: 0, start: 0 },
            align_self: None,
            constraints: Constraints::new(),
            direction: None,
            grid: None,
        }
    }

    /// Sets the width mode.
    #[must_use]
    pub const fn width(mut self, width: SizeMode) -> Self {
        self.width = width;
        self
    }

    /// Sets the height mode.
    #[must_use]
    pub const fn height(mut self, height: SizeMode) -> Self {
        self.height = height;
        self
    }

    /// Sets the margin.
    #[must_use]
    pub const fn margin(mut self, margin: EdgeInsets) -> Self {
        self.margin = margin;
        self
    }

    /// Sets this node's cross-axis self-alignment override.
    #[must_use]
    pub const fn align_self(mut self, alignment: Alignment) -> Self {
        self.align_self = Some(alignment);
        self
    }

    /// Sets the size constraints.
    #[must_use]
    pub const fn constraints(mut self, constraints: Constraints) -> Self {
        self.constraints = constraints;
        self
    }

    /// Places this node in its parent grid (see [`crate::Node::grid`]).
    #[must_use]
    pub const fn grid(mut self, placement: super::GridPlacement) -> Self {
        self.grid = Some(placement);
        self
    }

    /// Sets this node's layout direction, overriding the inherited one.
    #[must_use]
    pub const fn direction(mut self, direction: super::LayoutDirection) -> Self {
        self.direction = Some(direction);
        self
    }
}

/// A macro-free way to keep [`ColumnStyle`] and [`RowStyle`] — the two
/// axis-specific container styles — from drifting apart, without going as
/// far as merging them into one type and losing the "this is a column vs.
/// this is a row" distinction the rest of the layout/reconciliation code
/// relies on ([`crate::node::Node::column`] vs.
/// [`crate::node::Node::row`]).
macro_rules! container_style {
    ($name:ident) => {
        /// Padding, gap, and cross-axis alignment for a container node along
        /// one axis (see [`ColumnStyle`]/[`RowStyle`]).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        pub struct $name {
            /// Space reserved between this container's border and its
            /// children, on each edge.
            pub padding: EdgeInsets,
            /// Space reserved between adjacent children along the main
            /// axis.
            pub gap: i32,
            /// How children are aligned along the cross axis.
            pub align_items: Alignment,
            /// How content that exceeds this container's bounds is
            /// handled.
            pub overflow: Overflow,
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    padding: EdgeInsets::all(24),
                    gap: 12,
                    align_items: Alignment::Stretch,
                    overflow: Overflow::Clip,
                }
            }
        }

        impl $name {
            /// Creates a container style with the default padding, gap,
            /// alignment, and overflow behavior.
            pub const fn new() -> Self {
                Self {
                    padding: EdgeInsets { top: 24, end: 24, bottom: 24, start: 24 },
                    gap: 12,
                    align_items: Alignment::Stretch,
                    overflow: Overflow::Clip,
                }
            }

            /// Sets the padding.
            #[must_use]
            pub const fn padding(mut self, padding: EdgeInsets) -> Self {
                self.padding = padding;
                self
            }

            /// Sets the gap between children.
            #[must_use]
            pub const fn gap(mut self, gap: i32) -> Self {
                self.gap = gap;
                self
            }

            /// Sets the cross-axis alignment of children.
            #[must_use]
            pub const fn align_items(mut self, alignment: Alignment) -> Self {
                self.align_items = alignment;
                self
            }

            /// Sets the overflow behavior.
            #[must_use]
            pub const fn overflow(mut self, overflow: Overflow) -> Self {
                self.overflow = overflow;
                self
            }
        }
    };
}

container_style!(ColumnStyle);
container_style!(RowStyle);

/// The resolved container layout parameters the layout engine's
/// axis-generic helpers need. Grouping `padding`/`gap`/`align_items` here
/// (rather than passing each positionally) removes an argument-order
/// footgun between two same-typed `i32`/`Alignment` parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedContainerStyle {
    pub(crate) padding: EdgeInsets,
    pub(crate) gap: i32,
    pub(crate) align_items: Alignment,
}

impl From<ColumnStyle> for ResolvedContainerStyle {
    fn from(style: ColumnStyle) -> Self {
        Self { padding: style.padding, gap: style.gap, align_items: style.align_items }
    }
}

impl From<RowStyle> for ResolvedContainerStyle {
    fn from(style: RowStyle) -> Self {
        Self { padding: style.padding, gap: style.gap, align_items: style.align_items }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constraints_cannot_represent_max_below_min() {
        let constraints = Constraints::new().with_min_width(100).with_max_width(10);
        assert_eq!(constraints.min_width(), 100);
        assert_eq!(constraints.max_width(), Some(100));

        let constraints = Constraints::new().with_max_width(10).with_min_width(100);
        assert_eq!(constraints.max_width(), Some(100));
    }

    #[test]
    fn constraints_reject_negative_bounds() {
        let constraints = Constraints::new().with_min_width(-5).with_max_height(-1);
        assert_eq!(constraints.min_width(), 0);
        assert_eq!(constraints.max_height(), Some(0));
    }
}
