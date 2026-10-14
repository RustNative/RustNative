//! Logical pixels ↔ a host's grid (`PLAN.md` 2.11).
//!
//! The core lays out in logical pixels. A host that addresses space in
//! coarser units — a terminal's character cells, a display's pixels at some
//! scale — converts at its own boundary, here, with one rule:
//!
//! - **lengths** round half away from zero, and a nonzero length is never
//!   zero units (a one-pixel gap is still a gap);
//! - **rectangles** snap their two edges separately, so boxes that touch in
//!   logical pixels still touch in units and never overlap or open a seam;
//!   a nonempty rectangle keeps at least one unit on each axis;
//! - **points** belong to the unit they fall in (floor division), which is
//!   what hit-testing needs.
//!
//! ```
//! use rustnative_core::Rect;
//! use rustnative_drawlist::{GridMapping, GridRect};
//!
//! let cells = GridMapping::new(8, 16);
//! // Two boxes side by side stay side by side.
//! let left = cells.rect(Rect::new(0, 0, 44, 16));
//! let right = cells.rect(Rect::new(44, 0, 44, 16));
//! assert_eq!(left.right(), right.x);
//! // A one-pixel line is still one cell.
//! assert_eq!(cells.rect(Rect::new(0, 0, 1, 1)), GridRect::new(0, 0, 1, 1));
//! ```

use rustnative_core::{Point, Rect, Size};

/// A rectangle in grid units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct GridRect {
    /// The left column.
    pub x: i32,
    /// The top row.
    pub y: i32,
    /// Columns.
    pub width: i32,
    /// Rows.
    pub height: i32,
}

impl GridRect {
    /// A rectangle at column `x`, row `y`, `width` by `height` units.
    #[must_use]
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self { x, y, width, height }
    }

    /// One past the last column.
    #[must_use]
    pub const fn right(&self) -> i32 {
        self.x.saturating_add(self.width)
    }

    /// One past the last row.
    #[must_use]
    pub const fn bottom(&self) -> i32 {
        self.y.saturating_add(self.height)
    }

    /// Whether it covers no unit.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    /// How many units it covers.
    #[must_use]
    pub fn area(&self) -> i64 {
        if self.is_empty() { 0 } else { i64::from(self.width) * i64::from(self.height) }
    }

    /// Whether unit (`x`, `y`) is inside it.
    #[must_use]
    pub const fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }

    /// The units both cover (empty when they are disjoint).
    #[must_use]
    pub fn intersect(&self, other: &Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right <= x || bottom <= y {
            Self::new(x, y, 0, 0)
        } else {
            Self::new(x, y, right - x, bottom - y)
        }
    }

    /// The smallest rectangle covering both (an empty one is ignored).
    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Self::new(x, y, self.right().max(other.right()) - x, self.bottom().max(other.bottom()) - y)
    }

    /// Moved by (`dx`, `dy`).
    #[must_use]
    pub const fn offset(&self, dx: i32, dy: i32) -> Self {
        Self::new(self.x.saturating_add(dx), self.y.saturating_add(dy), self.width, self.height)
    }
}

/// How many logical pixels one grid unit is, per axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GridMapping {
    unit_width: u32,
    unit_height: u32,
}

impl Default for GridMapping {
    /// The terminal default: a cell 8 logical pixels wide and 16 high.
    fn default() -> Self {
        Self::new(8, 16)
    }
}

/// `value / divisor`, rounded half away from zero.
fn round_div(value: i64, divisor: i64) -> i64 {
    let half = divisor / 2;
    if value >= 0 { (value + half) / divisor } else { -((-value + half) / divisor) }
}

fn clamp_i32(value: i64) -> i32 {
    i32::try_from(value.clamp(i64::from(i32::MIN), i64::from(i32::MAX))).unwrap_or(0)
}

impl GridMapping {
    /// One unit is `unit_width` × `unit_height` logical pixels (each at
    /// least 1).
    #[must_use]
    pub const fn new(unit_width: u32, unit_height: u32) -> Self {
        Self {
            unit_width: if unit_width == 0 { 1 } else { unit_width },
            unit_height: if unit_height == 0 { 1 } else { unit_height },
        }
    }

    /// Logical pixels per unit across.
    #[must_use]
    pub const fn unit_width(&self) -> u32 {
        self.unit_width
    }

    /// Logical pixels per unit down.
    #[must_use]
    pub const fn unit_height(&self) -> u32 {
        self.unit_height
    }

    fn length(value: i32, unit: u32) -> i32 {
        if value == 0 {
            return 0;
        }
        let units = round_div(i64::from(value), i64::from(unit));
        let units = if units == 0 { i64::from(value.signum()) } else { units };
        clamp_i32(units)
    }

    /// A horizontal length in columns.
    #[must_use]
    pub fn columns(&self, length: i32) -> i32 {
        Self::length(length, self.unit_width)
    }

    /// A vertical length in rows.
    #[must_use]
    pub fn rows(&self, length: i32) -> i32 {
        Self::length(length, self.unit_height)
    }

    fn edges(start: i32, length: i32, unit: u32) -> (i32, i32) {
        let unit = i64::from(unit);
        let first = round_div(i64::from(start), unit);
        let mut last = round_div(i64::from(start) + i64::from(length.max(0)), unit);
        if length > 0 && last <= first {
            last = first + 1;
        }
        (clamp_i32(first), clamp_i32(last - first))
    }

    /// A rectangle in units, edges snapped separately.
    #[must_use]
    pub fn rect(&self, rect: Rect) -> GridRect {
        let (x, width) = Self::edges(rect.x, rect.width, self.unit_width);
        let (y, height) = Self::edges(rect.y, rect.height, self.unit_height);
        GridRect::new(x, y, width, height)
    }

    /// The unit a logical point falls in.
    #[must_use]
    pub fn point(&self, point: Point) -> (i32, i32) {
        (
            clamp_i32(i64::from(point.x).div_euclid(i64::from(self.unit_width))),
            clamp_i32(i64::from(point.y).div_euclid(i64::from(self.unit_height))),
        )
    }

    /// The logical point at the centre of unit (`x`, `y`) — where a pointer
    /// reported at that unit is taken to be.
    #[must_use]
    pub fn unit_center(&self, x: i32, y: i32) -> Point {
        let w = i64::from(self.unit_width);
        let h = i64::from(self.unit_height);
        Point::new(clamp_i32(i64::from(x) * w + w / 2), clamp_i32(i64::from(y) * h + h / 2))
    }

    /// The logical point at the top-left corner of unit (`x`, `y`).
    #[must_use]
    pub fn unit_origin(&self, x: i32, y: i32) -> Point {
        Point::new(
            clamp_i32(i64::from(x) * i64::from(self.unit_width)),
            clamp_i32(i64::from(y) * i64::from(self.unit_height)),
        )
    }

    /// The logical size of a grid `columns` × `rows` units — what the core
    /// lays a window out in.
    #[must_use]
    pub fn logical_size(&self, columns: u16, rows: u16) -> Size {
        Size::new(u32::from(columns) * self.unit_width, u32::from(rows) * self.unit_height)
    }

    /// A horizontal length as whole units, in logical pixels again.
    #[must_use]
    pub fn snap_width(&self, length: i32) -> i32 {
        self.columns(length).saturating_mul(i32::try_from(self.unit_width).unwrap_or(i32::MAX))
    }

    /// A vertical length as whole units, in logical pixels again.
    #[must_use]
    pub fn snap_height(&self, length: i32) -> i32 {
        self.rows(length).saturating_mul(i32::try_from(self.unit_height).unwrap_or(i32::MAX))
    }

    /// Rounds every layout length of `snapshot` to whole units, by this
    /// mapping's rule, before it is laid out
    /// ([`rustnative_core::TreeSnapshot::map_lengths`]): with measured sizes
    /// already whole units, every rectangle then lands on unit boundaries,
    /// and a declared spacing is the same number of units wherever it
    /// appears.
    pub fn quantize(&self, snapshot: &mut rustnative_core::TreeSnapshot) {
        snapshot.map_lengths(|x| self.snap_width(x), |y| self.snap_height(y));
    }

    /// The same mapping for floating-point coordinates (a draw list's), in
    /// fractional units.
    #[must_use]
    #[allow(clippy::cast_precision_loss, reason = "unit sizes are small integers")]
    pub fn to_units(&self, x: f32, y: f32) -> (f32, f32) {
        (x / self.unit_width as f32, y / self.unit_height as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_round_half_away_from_zero_and_never_vanish() {
        let cells = GridMapping::new(8, 16);
        assert_eq!(cells.columns(0), 0);
        assert_eq!(cells.columns(1), 1, "a nonzero length is at least one unit");
        assert_eq!(cells.columns(4), 1, "half rounds away from zero");
        assert_eq!(cells.columns(11), 1);
        assert_eq!(cells.columns(12), 2);
        assert_eq!(cells.columns(-4), -1);
        assert_eq!(cells.columns(-1), -1);
        assert_eq!(cells.rows(8), 1);
        assert_eq!(cells.rows(23), 1);
        assert_eq!(cells.rows(24), 2);
    }

    #[test]
    fn adjacent_rectangles_stay_adjacent_whatever_their_sizes() {
        let cells = GridMapping::new(8, 16);
        for split in 0..200 {
            let left = cells.rect(Rect::new(3, 0, split, 16));
            let right = cells.rect(Rect::new(3 + split, 0, 97, 16));
            if split > 0 {
                assert!(left.width >= 1);
                assert!(left.right() <= right.x + 1);
            }
            assert!(right.width >= 1);
        }
    }

    #[test]
    fn a_nonempty_rectangle_keeps_a_unit_and_an_empty_one_none() {
        let cells = GridMapping::new(8, 16);
        assert_eq!(cells.rect(Rect::new(9, 9, 1, 1)), GridRect::new(1, 1, 1, 1));
        assert!(cells.rect(Rect::new(9, 9, 0, 5)).is_empty());
        assert!(cells.rect(Rect::new(9, 9, -5, 5)).is_empty());
    }

    #[test]
    fn points_fall_in_their_unit_and_centres_round_trip() {
        let cells = GridMapping::new(8, 16);
        assert_eq!(cells.point(Point::new(0, 0)), (0, 0));
        assert_eq!(cells.point(Point::new(7, 15)), (0, 0));
        assert_eq!(cells.point(Point::new(8, 16)), (1, 1));
        assert_eq!(cells.point(Point::new(-1, -1)), (-1, -1));
        for (x, y) in [(0, 0), (3, 7), (79, 23)] {
            assert_eq!(cells.point(cells.unit_center(x, y)), (x, y));
        }
        assert_eq!(cells.logical_size(80, 24), Size::new(640, 384));
    }

    #[test]
    fn pathological_values_saturate() {
        let cells = GridMapping::new(1, 1);
        let rect = cells.rect(Rect::new(i32::MAX - 1, i32::MIN, i32::MAX, i32::MAX));
        assert!(rect.width >= 0 && rect.height >= 0);
        assert_eq!(GridMapping::new(0, 0), GridMapping::new(1, 1));
    }

    #[test]
    fn rectangle_set_operations() {
        let a = GridRect::new(0, 0, 4, 4);
        let b = GridRect::new(2, 2, 4, 4);
        assert_eq!(a.intersect(&b), GridRect::new(2, 2, 2, 2));
        assert_eq!(a.union(&b), GridRect::new(0, 0, 6, 6));
        assert!(a.intersect(&GridRect::new(10, 10, 1, 1)).is_empty());
        assert_eq!(GridRect::default().union(&b), b);
        assert!(a.contains(3, 3) && !a.contains(4, 0));
        assert_eq!(a.area(), 16);
    }
}
