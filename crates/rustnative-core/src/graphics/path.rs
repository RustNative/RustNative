//! Arbitrary outlines: lines and Bézier curves, open or closed.

use std::sync::Arc;

use super::geometry::Vec2;

/// One step of a [`Path`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathSegment {
    /// Starts a new figure at a point, without drawing to it.
    MoveTo(Vec2),
    /// A straight line from the current point.
    LineTo(Vec2),
    /// A quadratic Bézier curve through one control point.
    QuadTo {
        /// The control point.
        control: Vec2,
        /// Where the curve ends.
        to: Vec2,
    },
    /// A cubic Bézier curve through two control points.
    CubicTo {
        /// The first control point.
        first: Vec2,
        /// The second control point.
        second: Vec2,
        /// Where the curve ends.
        to: Vec2,
    },
    /// Closes the current figure back to where it started.
    Close,
}

/// An outline built from [`PathSegment`]s, filled or stroked by a
/// [`super::DrawList`].
///
/// Cheap to clone: segments are shared, and copied only if a clone is then
/// extended.
///
/// # Example
///
/// ```
/// use rustnative_core::Path;
///
/// // A triangle.
/// let triangle = Path::new().move_to(10.0, 0.0).line_to(20.0, 20.0).line_to(0.0, 20.0).close();
/// assert_eq!(triangle.segments().len(), 4);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Path {
    segments: Arc<Vec<PathSegment>>,
}

impl Path {
    /// An empty path.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn push(mut self, segment: PathSegment) -> Self {
        Arc::make_mut(&mut self.segments).push(segment);
        self
    }

    /// Starts a new figure at `(x, y)`.
    #[must_use]
    pub fn move_to(self, x: f32, y: f32) -> Self {
        self.push(PathSegment::MoveTo(Vec2::new(x, y)))
    }

    /// A straight line to `(x, y)`.
    #[must_use]
    pub fn line_to(self, x: f32, y: f32) -> Self {
        self.push(PathSegment::LineTo(Vec2::new(x, y)))
    }

    /// A quadratic curve to `(x, y)` bent toward `(cx, cy)`.
    #[must_use]
    pub fn quad_to(self, cx: f32, cy: f32, x: f32, y: f32) -> Self {
        self.push(PathSegment::QuadTo { control: Vec2::new(cx, cy), to: Vec2::new(x, y) })
    }

    /// A cubic curve to `(x, y)` bent toward `(c1x, c1y)` then `(c2x, c2y)`.
    #[must_use]
    pub fn cubic_to(self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) -> Self {
        self.push(PathSegment::CubicTo {
            first: Vec2::new(c1x, c1y),
            second: Vec2::new(c2x, c2y),
            to: Vec2::new(x, y),
        })
    }

    /// Closes the current figure.
    #[must_use]
    pub fn close(self) -> Self {
        self.push(PathSegment::Close)
    }

    /// The segments, in order.
    #[must_use]
    pub fn segments(&self) -> &[PathSegment] {
        &self.segments
    }
}
