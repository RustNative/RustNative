//! The retained display list a canvas node draws.

use std::sync::Arc;

use crate::input::Scalar;
use crate::style::Color;

use super::geometry::{RectF, Transform2D, Vec2};
use super::image::ImageData;
use super::path::Path;

/// How a shape is filled or outlined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Paint {
    /// The color, including alpha.
    pub color: Color,
    /// Line width for strokes, in canvas units. Ignored by fills.
    pub stroke_width: Scalar,
}

impl Paint {
    /// A solid paint of `color`, one unit wide when stroked.
    #[must_use]
    pub fn color(color: Color) -> Self {
        Self { color, stroke_width: Scalar::ONE }
    }

    /// Sets the stroke width.
    #[must_use]
    pub fn stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = Scalar::new(width.max(0.0));
        self
    }
}

/// One instruction in a [`DrawList`].
///
/// `Push*` commands open a scope that the next unmatched [`Self::Pop`]
/// closes; a list that ends with scopes still open has them closed for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawCommand {
    /// Fills a rectangle.
    FillRect(RectF, Paint),
    /// Outlines a rectangle.
    StrokeRect(RectF, Paint),
    /// Fills a rectangle with rounded corners of `radius`.
    FillRoundedRect(RectF, Scalar, Paint),
    /// Fills the ellipse inscribed in a rectangle.
    FillEllipse(RectF, Paint),
    /// Draws a straight line.
    StrokeLine(Vec2, Vec2, Paint),
    /// Fills a path (non-zero winding).
    FillPath(Path, Paint),
    /// Outlines a path.
    StrokePath(Path, Paint),
    /// Draws a line of text with its top-left corner at a point.
    Text {
        /// Where the text's layout box starts.
        origin: Vec2,
        /// The text.
        text: String,
        /// Font size in canvas units.
        size: Scalar,
        /// The text color.
        color: Color,
    },
    /// Draws an image scaled into a rectangle.
    Image(ImageData, RectF),
    /// Applies a transform, on top of the current one, until the matching
    /// [`Self::Pop`].
    PushTransform(Transform2D),
    /// Clips everything until the matching [`Self::Pop`] to a rectangle
    /// (under the transform current when it was pushed).
    PushClip(RectF),
    /// Draws everything until the matching [`Self::Pop`] as one layer at
    /// this opacity — overlapping shapes inside do not show through each
    /// other.
    PushOpacity(Scalar),
    /// Closes the innermost open `Push*` scope.
    Pop,
    /// Declares a region pointer input on this canvas reports as `id`
    /// (see [`crate::PointerEvent::region`]). Draws nothing.
    HitRegion(u32, RectF),
}

/// A retained, portable list of drawing instructions: what a
/// [`crate::Node::canvas`] draws.
///
/// A draw list is data, exactly like the rest of the node tree: rebuilding
/// an identical one is a no-op for the backend, and changing it redraws
/// that canvas and nothing else. Cloning is cheap (the commands are shared
/// until a clone is extended), and two clones of the same list compare
/// equal without comparing their commands.
///
/// # Example
///
/// ```
/// use rustnative_core::{Color, DrawList, Paint, RectF, Transform2D};
///
/// let chart = DrawList::new()
///     .fill_rect(RectF::new(0.0, 0.0, 200.0, 100.0), Paint::color(Color::rgb(255, 255, 255)))
///     .push_transform(Transform2D::translation(20.0, 10.0))
///     .fill_rect(RectF::new(0.0, 0.0, 30.0, 80.0), Paint::color(Color::rgb(40, 90, 200)))
///     .hit_region(1, RectF::new(0.0, 0.0, 30.0, 80.0))
///     .pop();
///
/// // The bar was drawn at (20, 10), so that is where it is hit.
/// assert_eq!(chart.hit_test(25.0, 50.0), Some(1));
/// assert_eq!(chart.hit_test(5.0, 50.0), None);
/// ```
#[derive(Debug, Clone, Default, Eq)]
pub struct DrawList {
    commands: Arc<Vec<DrawCommand>>,
}

impl PartialEq for DrawList {
    fn eq(&self, other: &Self) -> bool {
        // The common case in a rerender — the component returned the list
        // it already had — is a pointer comparison, not a walk over every
        // command.
        Arc::ptr_eq(&self.commands, &other.commands) || self.commands == other.commands
    }
}

impl DrawList {
    /// An empty list.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends `command`.
    #[must_use]
    pub fn push(mut self, command: DrawCommand) -> Self {
        Arc::make_mut(&mut self.commands).push(command);
        self
    }

    /// Fills `rect`.
    #[must_use]
    pub fn fill_rect(self, rect: RectF, paint: Paint) -> Self {
        self.push(DrawCommand::FillRect(rect, paint))
    }

    /// Outlines `rect`.
    #[must_use]
    pub fn stroke_rect(self, rect: RectF, paint: Paint) -> Self {
        self.push(DrawCommand::StrokeRect(rect, paint))
    }

    /// Fills `rect` with corners rounded to `radius`.
    #[must_use]
    pub fn fill_rounded_rect(self, rect: RectF, radius: f32, paint: Paint) -> Self {
        self.push(DrawCommand::FillRoundedRect(rect, Scalar::new(radius.max(0.0)), paint))
    }

    /// Fills the ellipse inscribed in `rect`.
    #[must_use]
    pub fn fill_ellipse(self, rect: RectF, paint: Paint) -> Self {
        self.push(DrawCommand::FillEllipse(rect, paint))
    }

    /// Draws a line from `from` to `to`.
    #[must_use]
    pub fn stroke_line(self, from: Vec2, to: Vec2, paint: Paint) -> Self {
        self.push(DrawCommand::StrokeLine(from, to, paint))
    }

    /// Fills `path`.
    #[must_use]
    pub fn fill_path(self, path: Path, paint: Paint) -> Self {
        self.push(DrawCommand::FillPath(path, paint))
    }

    /// Outlines `path`.
    #[must_use]
    pub fn stroke_path(self, path: Path, paint: Paint) -> Self {
        self.push(DrawCommand::StrokePath(path, paint))
    }

    /// Draws `text` at `origin`, `size` units tall, in `color`.
    #[must_use]
    pub fn text(self, origin: Vec2, text: impl Into<String>, size: f32, color: Color) -> Self {
        self.push(DrawCommand::Text {
            origin,
            text: text.into(),
            size: Scalar::new(size.max(0.0)),
            color,
        })
    }

    /// Draws `image` scaled into `rect`.
    #[must_use]
    pub fn image(self, image: ImageData, rect: RectF) -> Self {
        self.push(DrawCommand::Image(image, rect))
    }

    /// Opens a transform scope.
    #[must_use]
    pub fn push_transform(self, transform: Transform2D) -> Self {
        self.push(DrawCommand::PushTransform(transform))
    }

    /// Opens a clip scope.
    #[must_use]
    pub fn push_clip(self, rect: RectF) -> Self {
        self.push(DrawCommand::PushClip(rect))
    }

    /// Opens an opacity layer.
    #[must_use]
    pub fn push_opacity(self, opacity: f32) -> Self {
        self.push(DrawCommand::PushOpacity(Scalar::new(opacity.clamp(0.0, 1.0))))
    }

    /// Closes the innermost open scope.
    #[must_use]
    pub fn pop(self) -> Self {
        self.push(DrawCommand::Pop)
    }

    /// Declares a pointer hit region (see [`DrawCommand::HitRegion`]).
    #[must_use]
    pub fn hit_region(self, id: u32, rect: RectF) -> Self {
        self.push(DrawCommand::HitRegion(id, rect))
    }

    /// The commands, in drawing order.
    #[must_use]
    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }

    /// The hit region at canvas point `(x, y)`, if any: the *last* declared
    /// region containing the point, since later drawing is on top.
    ///
    /// Regions are tested in the space they were declared in — under the
    /// transforms open at the time — and only where every clip open at the
    /// time lets the point through, so what is hit is exactly what was
    /// drawn there.
    #[must_use]
    pub fn hit_test(&self, x: f32, y: f32) -> Option<u32> {
        // Each open scope: the transform in force inside it, and the clips
        // (with the transform each was declared under) that apply.
        let mut transforms = vec![Transform2D::identity()];
        let mut clips: Vec<(Transform2D, RectF)> = Vec::new();
        let mut scopes: Vec<Scope> = Vec::new();
        let mut hit = None;
        for command in self.commands.iter() {
            let current = *transforms.last().unwrap_or(&Transform2D::identity());
            match command {
                DrawCommand::PushTransform(transform) => {
                    transforms.push(transform.then(current));
                    scopes.push(Scope::Transform);
                }
                DrawCommand::PushClip(rect) => {
                    clips.push((current, *rect));
                    scopes.push(Scope::Clip);
                }
                DrawCommand::PushOpacity(_) => scopes.push(Scope::Other),
                DrawCommand::Pop => match scopes.pop() {
                    Some(Scope::Transform) => {
                        transforms.pop();
                    }
                    Some(Scope::Clip) => {
                        clips.pop();
                    }
                    Some(Scope::Other) | None => {}
                },
                DrawCommand::HitRegion(id, rect)
                    if contains_under(current, *rect, x, y)
                        && clips
                            .iter()
                            .all(|(transform, clip)| contains_under(*transform, *clip, x, y)) =>
                {
                    hit = Some(*id);
                }
                _ => {}
            }
        }
        hit
    }
}

/// What a `Push*` command opened, so `Pop` knows what to close.
#[derive(Debug, Clone, Copy)]
enum Scope {
    Transform,
    Clip,
    Other,
}

/// Whether canvas point `(x, y)` lands in `rect` as drawn under `transform`.
fn contains_under(transform: Transform2D, rect: RectF, x: f32, y: f32) -> bool {
    // A transform that collapses the plane draws nothing, so hits nothing.
    transform.inverse().is_some_and(|inverse| {
        let (local_x, local_y) = inverse.apply(x, y);
        rect.contains(local_x, local_y)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn red() -> Paint {
        Paint::color(Color::rgb(255, 0, 0))
    }

    #[test]
    fn a_clone_is_equal_without_comparing_commands_and_diverges_on_extension() {
        let list = DrawList::new().fill_rect(RectF::new(0.0, 0.0, 1.0, 1.0), red());
        let copy = list.clone();
        assert_eq!(list, copy);
        let extended = copy.pop();
        assert_ne!(list, extended, "extending a clone must not change the original");
        assert_eq!(list.commands().len(), 1);
    }

    #[test]
    fn independently_built_identical_lists_are_equal() {
        let build = || DrawList::new().fill_rect(RectF::new(0.0, 0.0, 5.0, 5.0), red());
        assert_eq!(build(), build(), "equal content is equal, not just shared storage");
    }

    #[test]
    fn hit_testing_respects_transforms() {
        let list = DrawList::new()
            .push_transform(Transform2D::scale(2.0, 2.0))
            .push_transform(Transform2D::translation(10.0, 0.0))
            .hit_region(7, RectF::new(0.0, 0.0, 5.0, 5.0))
            .pop()
            .pop();
        // Translated 10 in scaled space, then scaled: the region spans
        // x 20..30, y 0..10 on the canvas.
        assert_eq!(list.hit_test(25.0, 5.0), Some(7));
        assert_eq!(list.hit_test(12.0, 5.0), None);
    }

    #[test]
    fn hit_testing_respects_clips() {
        let list = DrawList::new()
            .push_clip(RectF::new(0.0, 0.0, 10.0, 10.0))
            .hit_region(1, RectF::new(0.0, 0.0, 100.0, 100.0))
            .pop();
        assert_eq!(list.hit_test(5.0, 5.0), Some(1));
        assert_eq!(list.hit_test(50.0, 50.0), None, "the clip hides the rest of the region");
    }

    #[test]
    fn the_topmost_region_wins_and_popped_scopes_stop_applying() {
        let list = DrawList::new()
            .hit_region(1, RectF::new(0.0, 0.0, 100.0, 100.0))
            .push_transform(Transform2D::translation(50.0, 50.0))
            .hit_region(2, RectF::new(0.0, 0.0, 10.0, 10.0))
            .pop()
            .hit_region(3, RectF::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!(list.hit_test(55.0, 55.0), Some(2), "drawn later, so on top");
        assert_eq!(list.hit_test(5.0, 5.0), Some(3), "the translation no longer applies");
        assert_eq!(list.hit_test(80.0, 20.0), Some(1));
    }

    #[test]
    fn a_collapsed_transform_hits_nothing() {
        let list = DrawList::new()
            .push_transform(Transform2D::scale(0.0, 0.0))
            .hit_region(1, RectF::new(-10.0, -10.0, 20.0, 20.0))
            .pop();
        assert_eq!(list.hit_test(0.0, 0.0), None);
    }
}
