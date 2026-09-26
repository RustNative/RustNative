//! Fractional geometry for drawing: points, rectangles, and affine
//! transforms.
//!
//! Layout works in whole pixels ([`crate::Rect`]) because native controls
//! are positioned in whole pixels. Drawing does not: an antialiased edge
//! half a pixel over is a different image from one that is not. These types
//! carry [`Scalar`]s — finite `f32`s with lawful equality — so a
//! [`super::DrawList`] made of them can live in the `Eq` node tree and be
//! diffed like everything else.

use crate::input::Scalar;

/// A point in a canvas's coordinate space, in device-independent pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Vec2 {
    /// Horizontal position, growing rightward.
    pub x: Scalar,
    /// Vertical position, growing downward.
    pub y: Scalar,
}

impl Vec2 {
    /// A point at `(x, y)`.
    #[must_use]
    pub fn new(x: f32, y: f32) -> Self {
        Self { x: Scalar::new(x), y: Scalar::new(y) }
    }
}

/// An axis-aligned rectangle in a canvas's coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RectF {
    /// Left edge.
    pub x: Scalar,
    /// Top edge.
    pub y: Scalar,
    /// Width; a negative width is treated as empty.
    pub width: Scalar,
    /// Height; a negative height is treated as empty.
    pub height: Scalar,
}

impl RectF {
    /// A rectangle with its top-left corner at `(x, y)`.
    #[must_use]
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x: Scalar::new(x),
            y: Scalar::new(y),
            width: Scalar::new(width),
            height: Scalar::new(height),
        }
    }

    /// The right edge.
    #[must_use]
    pub fn right(&self) -> f32 {
        self.x.get() + self.width.get()
    }

    /// The bottom edge.
    #[must_use]
    pub fn bottom(&self) -> f32 {
        self.y.get() + self.height.get()
    }

    /// Whether `(x, y)` is inside, counting the top and left edges but not
    /// the bottom and right — so two rectangles sharing an edge never both
    /// claim a point on it.
    #[must_use]
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x.get() && y >= self.y.get() && x < self.right() && y < self.bottom()
    }
}

/// A 2D affine transform: the six-number matrix every native 2D API takes.
///
/// A point `(x, y)` maps to
/// `(x * m11 + y * m21 + dx, x * m12 + y * m22 + dy)` — the row-vector
/// convention Direct2D, Core Graphics, and the HTML canvas all share, so a
/// backend passes these through unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Transform2D {
    /// Row 1, column 1.
    pub m11: Scalar,
    /// Row 1, column 2.
    pub m12: Scalar,
    /// Row 2, column 1.
    pub m21: Scalar,
    /// Row 2, column 2.
    pub m22: Scalar,
    /// Horizontal translation.
    pub dx: Scalar,
    /// Vertical translation.
    pub dy: Scalar,
}

impl Default for Transform2D {
    fn default() -> Self {
        Self::identity()
    }
}

impl Transform2D {
    /// The transform that changes nothing.
    #[must_use]
    pub fn identity() -> Self {
        Self::from_parts(1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
    }

    /// A transform from its six matrix entries.
    #[must_use]
    pub fn from_parts(m11: f32, m12: f32, m21: f32, m22: f32, dx: f32, dy: f32) -> Self {
        Self {
            m11: Scalar::new(m11),
            m12: Scalar::new(m12),
            m21: Scalar::new(m21),
            m22: Scalar::new(m22),
            dx: Scalar::new(dx),
            dy: Scalar::new(dy),
        }
    }

    /// Moves by `(dx, dy)`.
    #[must_use]
    pub fn translation(dx: f32, dy: f32) -> Self {
        Self::from_parts(1.0, 0.0, 0.0, 1.0, dx, dy)
    }

    /// Scales by `(sx, sy)` about the origin.
    #[must_use]
    pub fn scale(sx: f32, sy: f32) -> Self {
        Self::from_parts(sx, 0.0, 0.0, sy, 0.0, 0.0)
    }

    /// Rotates by `radians` about the origin, clockwise on screen (y grows
    /// downward).
    #[must_use]
    pub fn rotation(radians: f32) -> Self {
        let (sin, cos) = radians.sin_cos();
        Self::from_parts(cos, sin, -sin, cos, 0.0, 0.0)
    }

    /// `self` followed by `next`: a point is transformed by `self` first.
    ///
    /// # Example
    ///
    /// ```
    /// use rustnative_core::Transform2D;
    ///
    /// // Scale, then move: the scale does not apply to the movement.
    /// let transform = Transform2D::scale(2.0, 2.0).then(Transform2D::translation(10.0, 0.0));
    /// assert_eq!(transform.apply(1.0, 1.0), (12.0, 2.0));
    /// ```
    #[must_use]
    pub fn then(self, next: Self) -> Self {
        let (a, b) = (self, next);
        Self::from_parts(
            a.m11.get() * b.m11.get() + a.m12.get() * b.m21.get(),
            a.m11.get() * b.m12.get() + a.m12.get() * b.m22.get(),
            a.m21.get() * b.m11.get() + a.m22.get() * b.m21.get(),
            a.m21.get() * b.m12.get() + a.m22.get() * b.m22.get(),
            a.dx.get() * b.m11.get() + a.dy.get() * b.m21.get() + b.dx.get(),
            a.dx.get() * b.m12.get() + a.dy.get() * b.m22.get() + b.dy.get(),
        )
    }

    /// Where `(x, y)` lands under this transform.
    #[must_use]
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            x * self.m11.get() + y * self.m21.get() + self.dx.get(),
            x * self.m12.get() + y * self.m22.get() + self.dy.get(),
        )
    }

    /// The transform that undoes this one, or `None` if this one collapses
    /// the plane (a zero scale) and so cannot be undone.
    #[must_use]
    pub fn inverse(&self) -> Option<Self> {
        let (m11, m12, m21, m22) = (self.m11.get(), self.m12.get(), self.m21.get(), self.m22.get());
        let determinant = m11 * m22 - m12 * m21;
        if determinant.abs() < f32::EPSILON {
            return None;
        }
        let (dx, dy) = (self.dx.get(), self.dy.get());
        Some(Self::from_parts(
            m22 / determinant,
            -m12 / determinant,
            -m21 / determinant,
            m11 / determinant,
            (m21 * dy - m22 * dx) / determinant,
            (m12 * dx - m11 * dy) / determinant,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4
    }

    #[test]
    fn composition_applies_the_first_transform_first() {
        let move_then_scale =
            Transform2D::translation(10.0, 0.0).then(Transform2D::scale(2.0, 2.0));
        assert_eq!(move_then_scale.apply(1.0, 1.0), (22.0, 2.0));
        let scale_then_move =
            Transform2D::scale(2.0, 2.0).then(Transform2D::translation(10.0, 0.0));
        assert_eq!(scale_then_move.apply(1.0, 1.0), (12.0, 2.0));
    }

    #[test]
    fn a_quarter_turn_moves_the_x_axis_onto_the_y_axis() {
        let turned = Transform2D::rotation(std::f32::consts::FRAC_PI_2).apply(1.0, 0.0);
        assert!(close(turned, (0.0, 1.0)), "{turned:?}");
    }

    #[test]
    fn the_inverse_undoes_the_transform() {
        let transform = Transform2D::rotation(0.7)
            .then(Transform2D::scale(3.0, 0.5))
            .then(Transform2D::translation(-4.0, 9.0));
        let inverse = transform.inverse().expect("invertible");
        let (x, y) = transform.apply(5.0, -2.0);
        assert!(close(inverse.apply(x, y), (5.0, -2.0)));
    }

    #[test]
    fn a_collapsed_transform_has_no_inverse() {
        assert!(Transform2D::scale(0.0, 1.0).inverse().is_none());
    }

    #[test]
    fn rectangles_own_their_top_left_edges_only() {
        let rect = RectF::new(0.0, 0.0, 10.0, 10.0);
        assert!(rect.contains(0.0, 0.0));
        assert!(rect.contains(9.99, 9.99));
        assert!(!rect.contains(10.0, 5.0), "the right edge belongs to the neighbour");
    }
}
