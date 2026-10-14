//! A [`DrawList`] rasterized onto a host's grid (`PLAN.md` Milestones 29,
//! 37, and 38).
//!
//! The portable model draws in logical pixels, with fractional coordinates,
//! transforms, clips, and opacity. A grid host — a terminal's cells, an
//! embedded display's pixels — has units, so the rasterizer decides, per
//! command, which units it covers, and hands each to a [`RasterTarget`]
//! that knows what a unit can show:
//!
//! - **fills** (rectangles, rounded rectangles, ellipses, paths with
//!   non-zero winding) cover the units whose centres they contain; a
//!   nonempty shape that contains no centre still covers the unit its own
//!   centre falls in, so nothing drawn vanishes;
//! - **strokes** (rectangle outlines, lines, path outlines) are one unit
//!   wide, and each unit says which neighbours the stroke continues to
//!   ([`Edges`]) — so a terminal can choose `─ │ ┌ ┼` and a display can
//!   just set the pixel;
//! - **text** is handed over at the unit of its top-left corner, with its
//!   size in units, for the host to measure and draw;
//! - **images** are sampled at two points per unit (the upper and the lower
//!   half), which a terminal shows as half blocks and a display averages;
//! - **transforms** that only translate and scale are exact; any other
//!   (a rotation, a skew) is approximated by the bounding box of what it
//!   moves — stated, because a grid cannot rotate a unit;
//! - **clips** are the bounding box of the transformed clip rectangle;
//! - **opacity** multiplies each colour's alpha, which the target blends.
//!
//! What the target is given is exactly what it must draw: everything is
//! already clipped to the target's [`RasterTarget::bounds`] and the clips
//! in force. Hit regions come back as unit rectangles, topmost last.

use std::collections::BTreeMap;

use rustnative_core::{
    Color, DrawCommand, DrawList, ImageData, Path, PathSegment, RectF, Transform2D,
};

use crate::grid::{GridMapping, GridRect};

/// Which neighbouring units a one-unit stroke continues to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Edges(u8);

impl Edges {
    /// Continues upward.
    pub const NORTH: Self = Self(1);
    /// Continues rightward.
    pub const EAST: Self = Self(2);
    /// Continues downward.
    pub const SOUTH: Self = Self(4);
    /// Continues leftward.
    pub const WEST: Self = Self(8);
    /// A stroke that continues nowhere: a dot.
    pub const NONE: Self = Self(0);

    /// Both sets of edges.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every edge of `other` is set.
    #[must_use]
    pub const fn has(self, other: Self) -> bool {
        self.0 & other.0 == other.0 && other.0 != 0
    }

    /// The raw bits: north 1, east 2, south 4, west 8.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }
}

/// A grid that draw-list commands are rasterized onto.
pub trait RasterTarget {
    /// The units that may be drawn, in grid coordinates.
    fn bounds(&self) -> GridRect;

    /// Covers unit (`x`, `y`) with `color` (blend by its alpha).
    fn fill(&mut self, x: i32, y: i32, color: Color);

    /// Draws a one-unit stroke through unit (`x`, `y`), continuing to
    /// `edges`. By default the unit is filled.
    fn stroke(&mut self, x: i32, y: i32, edges: Edges, color: Color) {
        let _ = edges;
        self.fill(x, y, color);
    }

    /// Draws `text` with its top-left corner at unit (`x`, `y`), `rows`
    /// units tall, in `color`, showing only what lies inside `clip`.
    fn text(&mut self, x: i32, y: i32, text: &str, rows: f32, color: Color, clip: GridRect);

    /// Shows an image's samples for unit (`x`, `y`): `upper` at the upper
    /// half's centre and `lower` at the lower half's. By default the unit
    /// is filled with their average.
    fn image(&mut self, x: i32, y: i32, upper: Color, lower: Color) {
        let mix = |a: u8, b: u8| u8::try_from((u16::from(a) + u16::from(b)) / 2).unwrap_or(a);
        self.fill(
            x,
            y,
            Color::rgba(
                mix(upper.red, lower.red),
                mix(upper.green, lower.green),
                mix(upper.blue, lower.blue),
                mix(upper.alpha, lower.alpha),
            ),
        );
    }
}

/// What rasterizing a list produced besides drawing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rasterized {
    /// Every hit region, as the units it covers (clipped), in drawing
    /// order — the last one containing a unit is the one hit.
    pub hit_regions: Vec<(u32, GridRect)>,
    /// Whether a transform other than translation and scale was
    /// approximated by its bounding box.
    pub approximated_transform: bool,
}

impl Rasterized {
    /// The hit region at unit (`x`, `y`), topmost first.
    #[must_use]
    pub fn hit(&self, x: i32, y: i32) -> Option<u32> {
        self.hit_regions.iter().rev().find(|(_, rect)| rect.contains(x, y)).map(|(id, _)| *id)
    }
}

#[derive(Debug, Clone, Copy)]
enum Scope {
    Transform,
    Clip,
    Opacity,
}

struct State<'a, T: RasterTarget> {
    target: &'a mut T,
    mapping: GridMapping,
    origin: GridRect,
    transforms: Vec<Transform2D>,
    clips: Vec<GridRect>,
    opacities: Vec<f32>,
    out: Rasterized,
}

/// Rasterizes `list` onto `target`, with the list's logical origin at the
/// top-left of `origin` (the canvas node's units) and drawing confined to
/// `origin`, at `mapping`'s logical pixels per unit.
///
/// ```
/// use rustnative_core::{Color, DrawList, Paint, RectF};
/// use rustnative_drawlist::{GridMapping, GridRect, RasterTarget, rasterize};
///
/// struct Cells(Vec<(i32, i32)>);
/// impl RasterTarget for Cells {
///     fn bounds(&self) -> GridRect { GridRect::new(0, 0, 10, 10) }
///     fn fill(&mut self, x: i32, y: i32, _: Color) { self.0.push((x, y)); }
///     fn text(&mut self, _: i32, _: i32, _: &str, _: f32, _: Color, _: GridRect) {}
/// }
///
/// let list = DrawList::new().fill_rect(RectF::new(0.0, 0.0, 16.0, 16.0), Paint::color(Color::rgb(255, 0, 0)));
/// let mut cells = Cells(Vec::new());
/// rasterize(&list, GridRect::new(2, 3, 5, 5), GridMapping::new(8, 16), &mut cells);
/// assert_eq!(cells.0, [(2, 3), (3, 3)]);
/// ```
pub fn rasterize<T: RasterTarget>(
    list: &DrawList,
    origin: GridRect,
    mapping: GridMapping,
    target: &mut T,
) -> Rasterized {
    let clip = target.bounds().intersect(&origin);
    let mut state = State {
        target,
        mapping,
        origin,
        transforms: vec![Transform2D::identity()],
        clips: vec![clip],
        opacities: vec![1.0],
        out: Rasterized::default(),
    };
    let mut scopes = Vec::new();
    for command in list.commands() {
        state.command(command, &mut scopes);
    }
    state.out
}

fn is_axis_aligned(transform: &Transform2D) -> bool {
    transform.m12.get().abs() < f32::EPSILON && transform.m21.get().abs() < f32::EPSILON
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "unit coordinates are clamped to the i32 range before the cast"
)]
fn floor_unit(value: f32) -> i32 {
    value.floor().clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

impl<T: RasterTarget> State<'_, T> {
    fn transform(&self) -> Transform2D {
        self.transforms.last().copied().unwrap_or_else(Transform2D::identity)
    }

    fn clip(&self) -> GridRect {
        self.clips.last().copied().unwrap_or_default()
    }

    fn color(&self, color: Color) -> Color {
        let opacity = self.opacities.last().copied().unwrap_or(1.0);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "alpha × opacity in 0..=1 stays in 0..=255"
        )]
        let alpha = (f32::from(color.alpha) * opacity).round().clamp(0.0, 255.0) as u8;
        Color::rgba(color.red, color.green, color.blue, alpha)
    }

    /// A logical point of the list, in fractional grid units.
    fn to_units(&self, x: f32, y: f32) -> (f32, f32) {
        let (x, y) = self.transform().apply(x, y);
        let (ux, uy) = self.mapping.to_units(x, y);
        #[allow(clippy::cast_precision_loss, reason = "grid origins are small integers")]
        (ux + self.origin.x as f32, uy + self.origin.y as f32)
    }

    /// The transformed bounding box of `rect`, in fractional units.
    fn bounds_of(&mut self, rect: RectF) -> (f32, f32, f32, f32) {
        if !is_axis_aligned(&self.transform()) {
            self.out.approximated_transform = true;
        }
        let corners = [
            self.to_units(rect.x.get(), rect.y.get()),
            self.to_units(rect.right(), rect.y.get()),
            self.to_units(rect.x.get(), rect.bottom()),
            self.to_units(rect.right(), rect.bottom()),
        ];
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (x, y) in corners {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        (x0, y0, x1, y1)
    }

    /// The units whose centres a fractional box contains (at least the unit
    /// of its centre, when it has any area).
    fn covered(x0: f32, y0: f32, x1: f32, y1: f32) -> GridRect {
        if x1 <= x0 || y1 <= y0 {
            return GridRect::default();
        }
        let first_x = floor_unit(x0 - 0.5) + 1;
        let first_y = floor_unit(y0 - 0.5) + 1;
        let end_x = floor_unit(x1 - 0.5) + 1;
        let end_y = floor_unit(y1 - 0.5) + 1;
        let (first_x, end_x) = if end_x > first_x {
            (first_x, end_x)
        } else {
            let center = floor_unit(f32::midpoint(x0, x1));
            (center, center + 1)
        };
        let (first_y, end_y) = if end_y > first_y {
            (first_y, end_y)
        } else {
            let center = floor_unit(f32::midpoint(y0, y1));
            (center, center + 1)
        };
        GridRect::new(first_x, first_y, end_x - first_x, end_y - first_y)
    }

    fn fill_units(&mut self, units: GridRect, color: Color) {
        let visible = units.intersect(&self.clip());
        let color = self.color(color);
        for y in visible.y..visible.bottom() {
            for x in visible.x..visible.right() {
                self.target.fill(x, y, color);
            }
        }
    }

    fn emit_strokes(&mut self, strokes: BTreeMap<(i32, i32), Edges>, color: Color) {
        let clip = self.clip();
        let color = self.color(color);
        for ((x, y), edges) in strokes {
            if clip.contains(x, y) {
                self.target.stroke(x, y, edges, color);
            }
        }
    }

    fn command(&mut self, command: &DrawCommand, scopes: &mut Vec<Scope>) {
        match command {
            DrawCommand::FillRect(rect, paint) | DrawCommand::FillRoundedRect(rect, _, paint) => {
                let (x0, y0, x1, y1) = self.bounds_of(*rect);
                self.fill_units(Self::covered(x0, y0, x1, y1), paint.color);
            }
            DrawCommand::StrokeRect(rect, paint) => {
                let (x0, y0, x1, y1) = self.bounds_of(*rect);
                let units = Self::covered(x0, y0, x1, y1);
                let mut strokes = BTreeMap::new();
                outline(units, &mut strokes);
                self.emit_strokes(strokes, paint.color);
            }
            DrawCommand::FillEllipse(rect, paint) => {
                let (x0, y0, x1, y1) = self.bounds_of(*rect);
                self.fill_ellipse(x0, y0, x1, y1, paint.color);
            }
            DrawCommand::StrokeLine(from, to, paint) => {
                let a = self.to_units(from.x.get(), from.y.get());
                let b = self.to_units(to.x.get(), to.y.get());
                let mut strokes = BTreeMap::new();
                line(a, b, &mut strokes);
                self.emit_strokes(strokes, paint.color);
            }
            DrawCommand::FillPath(path, paint) => {
                let polygons = self.flatten(path);
                self.fill_polygons(&polygons, paint.color);
            }
            DrawCommand::StrokePath(path, paint) => {
                let polygons = self.flatten(path);
                let mut strokes = BTreeMap::new();
                for polygon in &polygons {
                    for pair in polygon.points.windows(2) {
                        line(pair[0], pair[1], &mut strokes);
                    }
                    if polygon.closed {
                        if let (Some(last), Some(first)) =
                            (polygon.points.last(), polygon.points.first())
                        {
                            line(*last, *first, &mut strokes);
                        }
                    }
                }
                self.emit_strokes(strokes, paint.color);
            }
            DrawCommand::Text { origin, text, size, color } => {
                let (x, y) = self.to_units(origin.x.get(), origin.y.get());
                let (_, rows) = self.mapping.to_units(0.0, size.get());
                let clip = self.clip();
                let color = self.color(*color);
                self.target.text(floor_unit(x + 0.5), floor_unit(y + 0.5), text, rows, color, clip);
            }
            DrawCommand::Image(image, rect) => self.image(image, *rect),
            DrawCommand::PushTransform(transform) => {
                let next = transform.then(self.transform());
                self.transforms.push(next);
                scopes.push(Scope::Transform);
            }
            DrawCommand::PushClip(rect) => {
                let (x0, y0, x1, y1) = self.bounds_of(*rect);
                let units = Self::covered(x0, y0, x1, y1);
                let next = self.clip().intersect(&units);
                self.clips.push(next);
                scopes.push(Scope::Clip);
            }
            DrawCommand::PushOpacity(opacity) => {
                let next = self.opacities.last().copied().unwrap_or(1.0) * opacity.get();
                self.opacities.push(next);
                scopes.push(Scope::Opacity);
            }
            DrawCommand::Pop => match scopes.pop() {
                Some(Scope::Transform) => {
                    self.transforms.pop();
                }
                Some(Scope::Clip) => {
                    self.clips.pop();
                }
                Some(Scope::Opacity) => {
                    self.opacities.pop();
                }
                None => {}
            },
            DrawCommand::HitRegion(id, rect) => {
                let (x0, y0, x1, y1) = self.bounds_of(*rect);
                let units = Self::covered(x0, y0, x1, y1).intersect(&self.clip());
                if !units.is_empty() {
                    self.out.hit_regions.push((*id, units));
                }
            }
        }
    }

    fn fill_ellipse(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: Color) {
        let bounds = Self::covered(x0, y0, x1, y1);
        let (cx, cy) = (f32::midpoint(x0, x1), f32::midpoint(y0, y1));
        let (rx, ry) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
        let clip = self.clip();
        let color = self.color(color);
        let mut any = false;
        let mut center_unit = None;
        for y in bounds.y..bounds.bottom() {
            for x in bounds.x..bounds.right() {
                #[allow(clippy::cast_precision_loss, reason = "small grid coordinates")]
                let (dx, dy) = ((x as f32 + 0.5 - cx) / rx, (y as f32 + 0.5 - cy) / ry);
                if dx * dx + dy * dy <= 1.0 {
                    any = true;
                    if clip.contains(x, y) {
                        self.target.fill(x, y, color);
                    }
                }
                center_unit.get_or_insert((floor_unit(cx), floor_unit(cy)));
            }
        }
        if !any {
            if let Some((x, y)) = center_unit {
                if clip.contains(x, y) {
                    self.target.fill(x, y, color);
                }
            }
        }
    }

    fn flatten(&mut self, path: &Path) -> Vec<Polygon> {
        let mut polygons: Vec<Polygon> = Vec::new();
        let mut current: Option<Polygon> = None;
        let mut at = (0.0_f32, 0.0_f32);
        let mut start = at;
        const STEPS: usize = 8;
        for segment in path.segments() {
            match segment {
                PathSegment::MoveTo(point) => {
                    if let Some(polygon) = current.take() {
                        polygons.push(polygon);
                    }
                    at = (point.x.get(), point.y.get());
                    start = at;
                    current =
                        Some(Polygon { points: vec![self.to_units(at.0, at.1)], closed: false });
                }
                PathSegment::LineTo(point) => {
                    at = (point.x.get(), point.y.get());
                    let unit = self.to_units(at.0, at.1);
                    current
                        .get_or_insert_with(|| Polygon { points: Vec::new(), closed: false })
                        .points
                        .push(unit);
                }
                PathSegment::QuadTo { control, to } => {
                    let (c, end) = ((control.x.get(), control.y.get()), (to.x.get(), to.y.get()));
                    for step in 1..=STEPS {
                        #[allow(clippy::cast_precision_loss, reason = "a small step count")]
                        let t = step as f32 / STEPS as f32;
                        let u = 1.0 - t;
                        let x = u * u * at.0 + 2.0 * u * t * c.0 + t * t * end.0;
                        let y = u * u * at.1 + 2.0 * u * t * c.1 + t * t * end.1;
                        let unit = self.to_units(x, y);
                        current
                            .get_or_insert_with(|| Polygon { points: Vec::new(), closed: false })
                            .points
                            .push(unit);
                    }
                    at = end;
                }
                PathSegment::CubicTo { first, second, to } => {
                    let (c1, c2, end) = (
                        (first.x.get(), first.y.get()),
                        (second.x.get(), second.y.get()),
                        (to.x.get(), to.y.get()),
                    );
                    for step in 1..=STEPS {
                        #[allow(clippy::cast_precision_loss, reason = "a small step count")]
                        let t = step as f32 / STEPS as f32;
                        let u = 1.0 - t;
                        let x = u * u * u * at.0
                            + 3.0 * u * u * t * c1.0
                            + 3.0 * u * t * t * c2.0
                            + t * t * t * end.0;
                        let y = u * u * u * at.1
                            + 3.0 * u * u * t * c1.1
                            + 3.0 * u * t * t * c2.1
                            + t * t * t * end.1;
                        let unit = self.to_units(x, y);
                        current
                            .get_or_insert_with(|| Polygon { points: Vec::new(), closed: false })
                            .points
                            .push(unit);
                    }
                    at = end;
                }
                PathSegment::Close => {
                    if let Some(mut polygon) = current.take() {
                        polygon.closed = true;
                        polygons.push(polygon);
                    }
                    at = start;
                }
            }
        }
        if let Some(polygon) = current {
            polygons.push(polygon);
        }
        polygons
    }

    fn fill_polygons(&mut self, polygons: &[Polygon], color: Color) {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for point in polygons.iter().flat_map(|polygon| &polygon.points) {
            x0 = x0.min(point.0);
            y0 = y0.min(point.1);
            x1 = x1.max(point.0);
            y1 = y1.max(point.1);
        }
        let bounds = Self::covered(x0, y0, x1, y1);
        let clip = self.clip();
        let color = self.color(color);
        let mut any = false;
        for y in bounds.y..bounds.bottom() {
            for x in bounds.x..bounds.right() {
                #[allow(clippy::cast_precision_loss, reason = "small grid coordinates")]
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                if winding(polygons, px, py) != 0 {
                    any = true;
                    if clip.contains(x, y) {
                        self.target.fill(x, y, color);
                    }
                }
            }
        }
        if !any && !bounds.is_empty() {
            let (x, y) = (floor_unit(f32::midpoint(x0, x1)), floor_unit(f32::midpoint(y0, y1)));
            if clip.contains(x, y) {
                self.target.fill(x, y, color);
            }
        }
    }

    fn image(&mut self, image: &ImageData, rect: RectF) {
        let (x0, y0, x1, y1) = self.bounds_of(rect);
        let units = Self::covered(x0, y0, x1, y1);
        let visible = units.intersect(&self.clip());
        if image.width() == 0 || image.height() == 0 || x1 <= x0 || y1 <= y0 {
            return;
        }
        let opacity = self.opacities.last().copied().unwrap_or(1.0);
        // An image smaller than the units it covers is sampled across those
        // units, so a tiny image still shows its whole self.
        #[allow(clippy::cast_precision_loss, reason = "small grid coordinates")]
        let (sx0, sx1) = if x1 - x0 < units.width as f32 {
            (units.x as f32, units.right() as f32)
        } else {
            (x0, x1)
        };
        #[allow(clippy::cast_precision_loss, reason = "small grid coordinates")]
        let (sy0, sy1) = if y1 - y0 < units.height as f32 {
            (units.y as f32, units.bottom() as f32)
        } else {
            (y0, y1)
        };
        for y in visible.y..visible.bottom() {
            for x in visible.x..visible.right() {
                #[allow(clippy::cast_precision_loss, reason = "small grid coordinates")]
                let (fx, upper_y, lower_y) = (x as f32 + 0.5, y as f32 + 0.25, y as f32 + 0.75);
                let sample = |sy: f32| {
                    let u = ((fx - sx0) / (sx1 - sx0)).clamp(0.0, 0.999_999);
                    let v = ((sy - sy0) / (sy1 - sy0)).clamp(0.0, 0.999_999);
                    pixel(image, u, v, opacity)
                };
                self.target.image(x, y, sample(upper_y), sample(lower_y));
            }
        }
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "u and v are clamped into 0..1, so the index is inside the image"
)]
fn pixel(image: &ImageData, u: f32, v: f32, opacity: f32) -> Color {
    let px = ((u * image.width() as f32) as u32).min(image.width() - 1);
    let py = ((v * image.height() as f32) as u32).min(image.height() - 1);
    let index = ((py as usize) * (image.width() as usize) + px as usize) * 4;
    let pixels = image.pixels();
    let Some(rgba) = pixels.get(index..index + 4) else { return Color::rgba(0, 0, 0, 0) };
    let (mut r, mut g, mut b, a) = (rgba[0], rgba[1], rgba[2], rgba[3]);
    if image.is_premultiplied() && a > 0 && a < 255 {
        let un = |c: u8| u8::try_from((u16::from(c) * 255 / u16::from(a)).min(255)).unwrap_or(c);
        (r, g, b) = (un(r), un(g), un(b));
    }
    let alpha = (f32::from(a) * opacity).round().clamp(0.0, 255.0) as u8;
    Color::rgba(r, g, b, alpha)
}

struct Polygon {
    points: Vec<(f32, f32)>,
    closed: bool,
}

/// The non-zero winding number of `(x, y)` with respect to every polygon,
/// each closed implicitly (a fill closes an open figure).
fn winding(polygons: &[Polygon], x: f32, y: f32) -> i32 {
    let mut winding = 0;
    for polygon in polygons {
        let points = &polygon.points;
        if points.len() < 3 {
            continue;
        }
        for index in 0..points.len() {
            let a = points[index];
            let b = points[(index + 1) % points.len()];
            if a.1 <= y {
                if b.1 > y && cross(a, b, (x, y)) > 0.0 {
                    winding += 1;
                }
            } else if b.1 <= y && cross(a, b, (x, y)) < 0.0 {
                winding -= 1;
            }
        }
    }
    winding
}

fn cross(a: (f32, f32), b: (f32, f32), p: (f32, f32)) -> f32 {
    (b.0 - a.0) * (p.1 - a.1) - (p.0 - a.0) * (b.1 - a.1)
}

fn connect(strokes: &mut BTreeMap<(i32, i32), Edges>, from: (i32, i32), to: (i32, i32)) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let (out, back) = match (dx.signum(), dy.signum()) {
        (1, 0) => (Edges::EAST, Edges::WEST),
        (-1, 0) => (Edges::WEST, Edges::EAST),
        (0, 1) => (Edges::SOUTH, Edges::NORTH),
        (0, -1) => (Edges::NORTH, Edges::SOUTH),
        // A diagonal step continues along the axis that leads.
        _ if dx.abs() >= dy.abs() => {
            if dx > 0 {
                (Edges::EAST, Edges::WEST)
            } else {
                (Edges::WEST, Edges::EAST)
            }
        }
        _ => {
            if dy > 0 {
                (Edges::SOUTH, Edges::NORTH)
            } else {
                (Edges::NORTH, Edges::SOUTH)
            }
        }
    };
    let entry = strokes.entry(from).or_default();
    *entry = entry.with(out);
    let entry = strokes.entry(to).or_default();
    *entry = entry.with(back);
}

/// A one-unit line between two fractional unit points (Bresenham).
fn line(from: (f32, f32), to: (f32, f32), strokes: &mut BTreeMap<(i32, i32), Edges>) {
    let (mut x, mut y) = (floor_unit(from.0), floor_unit(from.1));
    let (x1, y1) = (floor_unit(to.0), floor_unit(to.1));
    strokes.entry((x, y)).or_default();
    let dx = (x1 - x).abs();
    let dy = -(y1 - y).abs();
    let (sx, sy) = ((x1 - x).signum(), (y1 - y).signum());
    let mut error = dx + dy;
    let mut guard = 0_u32;
    while (x, y) != (x1, y1) && guard < 100_000 {
        guard += 1;
        let previous = (x, y);
        let doubled = 2 * error;
        if doubled >= dy {
            error += dy;
            x += sx;
        }
        if doubled <= dx {
            error += dx;
            y += sy;
        }
        connect(strokes, previous, (x, y));
    }
}

/// The outline of a unit rectangle, with its corners joined.
fn outline(rect: GridRect, strokes: &mut BTreeMap<(i32, i32), Edges>) {
    if rect.is_empty() {
        return;
    }
    let (left, top, right, bottom) = (rect.x, rect.y, rect.right() - 1, rect.bottom() - 1);
    if left == right && top == bottom {
        strokes.entry((left, top)).or_default();
        return;
    }
    let corners = [(left, top), (right, top), (right, bottom), (left, bottom)];
    for index in 0..4 {
        let (a, b) = (corners[index], corners[(index + 1) % 4]);
        let mut at = a;
        while at != b {
            let next = (at.0 + (b.0 - at.0).signum(), at.1 + (b.1 - at.1).signum());
            connect(strokes, at, next);
            at = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use rustnative_core::{Paint, Vec2};

    use super::*;

    #[derive(Default)]
    struct Recorder {
        fills: Vec<(i32, i32, Color)>,
        strokes: BTreeMap<(i32, i32), Edges>,
        texts: Vec<(i32, i32, String)>,
        images: Vec<(i32, i32, Color, Color)>,
    }

    impl RasterTarget for Recorder {
        fn bounds(&self) -> GridRect {
            GridRect::new(0, 0, 20, 10)
        }
        fn fill(&mut self, x: i32, y: i32, color: Color) {
            self.fills.push((x, y, color));
        }
        fn stroke(&mut self, x: i32, y: i32, edges: Edges, _: Color) {
            self.strokes.insert((x, y), edges);
        }
        fn text(&mut self, x: i32, y: i32, text: &str, _: f32, _: Color, _: GridRect) {
            self.texts.push((x, y, text.to_owned()));
        }
        fn image(&mut self, x: i32, y: i32, upper: Color, lower: Color) {
            self.images.push((x, y, upper, lower));
        }
    }

    const RED: Color = Color::rgb(255, 0, 0);
    const CELL: GridMapping = GridMapping::new(8, 16);

    fn run(list: &DrawList) -> (Recorder, Rasterized) {
        let mut recorder = Recorder::default();
        let out = rasterize(list, GridRect::new(0, 0, 20, 10), CELL, &mut recorder);
        (recorder, out)
    }

    #[test]
    fn a_tiny_fill_still_covers_a_unit() {
        let (cells, _) =
            run(&DrawList::new().fill_rect(RectF::new(1.0, 1.0, 2.0, 2.0), Paint::color(RED)));
        assert_eq!(cells.fills.len(), 1);
        assert_eq!((cells.fills[0].0, cells.fills[0].1), (0, 0));
    }

    #[test]
    fn a_stroked_rectangle_has_joined_corners() {
        let (cells, _) =
            run(&DrawList::new().stroke_rect(RectF::new(0.0, 0.0, 32.0, 48.0), Paint::color(RED)));
        assert_eq!(cells.strokes[&(0, 0)], Edges::EAST.with(Edges::SOUTH));
        assert_eq!(cells.strokes[&(3, 0)], Edges::WEST.with(Edges::SOUTH));
        assert_eq!(cells.strokes[&(3, 2)], Edges::WEST.with(Edges::NORTH));
        assert_eq!(cells.strokes[&(1, 0)], Edges::EAST.with(Edges::WEST));
        assert_eq!(cells.strokes[&(0, 1)], Edges::NORTH.with(Edges::SOUTH));
        assert!(!cells.strokes.contains_key(&(1, 1)), "an outline is hollow");
    }

    #[test]
    fn a_horizontal_line_connects_its_units() {
        let list = DrawList::new().stroke_line(
            Vec2::new(4.0, 8.0),
            Vec2::new(36.0, 8.0),
            Paint::color(RED),
        );
        let (cells, _) = run(&list);
        let keys: Vec<_> = cells.strokes.keys().copied().collect();
        assert_eq!(keys, [(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)]);
        assert_eq!(cells.strokes[&(2, 0)], Edges::EAST.with(Edges::WEST));
    }

    #[test]
    fn clips_transforms_and_opacity_apply_and_pop() {
        let list = DrawList::new()
            .push_clip(RectF::new(0.0, 0.0, 16.0, 16.0))
            .push_opacity(0.5)
            .fill_rect(RectF::new(0.0, 0.0, 160.0, 16.0), Paint::color(RED))
            .pop()
            .pop()
            .push_transform(Transform2D::translation(80.0, 32.0))
            .fill_rect(RectF::new(0.0, 0.0, 8.0, 16.0), Paint::color(RED))
            .pop();
        let (cells, out) = run(&list);
        assert_eq!(cells.fills.len(), 3);
        assert_eq!(cells.fills[0].2.alpha, 128);
        assert_eq!((cells.fills[2].0, cells.fills[2].1), (10, 2));
        assert_eq!(cells.fills[2].2.alpha, 255, "the opacity was popped");
        assert!(!out.approximated_transform);
    }

    #[test]
    fn a_rotation_is_approximated_and_said_so() {
        let list = DrawList::new()
            .push_transform(Transform2D::rotation(0.5))
            .fill_rect(RectF::new(0.0, 0.0, 16.0, 16.0), Paint::color(RED))
            .pop();
        let (_, out) = run(&list);
        assert!(out.approximated_transform);
    }

    #[test]
    fn ellipses_and_paths_fill_what_they_contain() {
        let (cells, _) =
            run(&DrawList::new().fill_ellipse(RectF::new(0.0, 0.0, 80.0, 80.0), Paint::color(RED)));
        let units: Vec<_> = cells.fills.iter().map(|f| (f.0, f.1)).collect();
        assert!(units.contains(&(5, 2)), "the centre");
        assert!(!units.contains(&(0, 0)), "not the corner");
        let triangle = Path::new().move_to(0.0, 0.0).line_to(80.0, 0.0).line_to(0.0, 80.0).close();
        let (cells, _) = run(&DrawList::new().fill_path(triangle, Paint::color(RED)));
        let units: Vec<_> = cells.fills.iter().map(|f| (f.0, f.1)).collect();
        assert!(units.contains(&(0, 0)));
        assert!(!units.contains(&(9, 4)));
    }

    #[test]
    fn text_and_images_land_where_they_are_drawn() {
        let image = ImageData::rgba(1, 2, vec![255, 0, 0, 255, 0, 0, 255, 255], false).unwrap();
        let list = DrawList::new()
            .text(Vec2::new(16.0, 16.0), "hi", 16.0, RED)
            .image(image, RectF::new(0.0, 0.0, 8.0, 16.0));
        let (cells, _) = run(&list);
        assert_eq!(cells.texts, [(2, 1, "hi".to_owned())]);
        assert_eq!(cells.images.len(), 1);
        assert_eq!(cells.images[0].2, Color::rgb(255, 0, 0));
        assert_eq!(cells.images[0].3, Color::rgb(0, 0, 255));
    }

    #[test]
    fn hit_regions_come_back_in_units() {
        let list = DrawList::new()
            .hit_region(1, RectF::new(0.0, 0.0, 80.0, 32.0))
            .hit_region(2, RectF::new(8.0, 0.0, 8.0, 16.0));
        let (_, out) = run(&list);
        assert_eq!(out.hit(1, 0), Some(2));
        assert_eq!(out.hit(5, 1), Some(1));
        assert_eq!(out.hit(15, 9), None);
    }

    #[test]
    fn nothing_escapes_the_canvas() {
        let mut recorder = Recorder::default();
        let list = DrawList::new()
            .fill_rect(RectF::new(-100.0, -100.0, 1000.0, 1000.0), Paint::color(RED));
        rasterize(&list, GridRect::new(2, 2, 3, 2), CELL, &mut recorder);
        assert_eq!(recorder.fills.len(), 6);
        assert!(recorder.fills.iter().all(|(x, y, _)| (2..5).contains(x) && (2..4).contains(y)));
    }
}
