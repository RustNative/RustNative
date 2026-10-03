//! `RnCanvas`: a canvas node's widget, which paints its `DrawList` with
//! cairo — GTK's own 2D renderer — and its text with Pango, in the desktop's
//! UI font.
//!
//! It is an `RnLayout`, so a canvas can carry virtual elements (its hit
//! regions as accessible objects) like any container. GTK paints its CSS
//! background first, so an unstyled canvas matches what it sits in.

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{cairo, glib, pango};
use rustnative_core::{Color, DrawCommand, DrawList, Paint, PathSegment, RectF, Transform2D};

use super::layout_widget::RnLayout;

mod imp {
    use std::cell::RefCell;

    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use gtk::{glib, graphene};
    use rustnative_core::DrawList;

    #[derive(Default)]
    pub struct RnCanvas {
        pub(super) list: RefCell<DrawList>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RnCanvas {
        const NAME: &'static str = "RnCanvas";
        type Type = super::RnCanvas;
        type ParentType = super::RnLayout;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("rn-canvas");
        }
    }

    impl ObjectImpl for RnCanvas {}

    impl WidgetImpl for RnCanvas {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            #[allow(
                clippy::cast_precision_loss,
                reason = "a widget's size, far inside f32's range"
            )]
            let bounds =
                graphene::Rect::new(0.0, 0.0, widget.width() as f32, widget.height() as f32);
            let cr = snapshot.append_cairo(&bounds);
            let pango = widget.pango_context();
            super::paint(&cr, &self.list.borrow(), &pango);
            // Children (a canvas has none of its own) after the drawing.
            self.parent_snapshot(snapshot);
        }
    }

    impl crate::gtk::layout_widget::RnLayoutImpl for RnCanvas {}
}

glib::wrapper! {
    /// The widget realizing a canvas node.
    pub struct RnCanvas(ObjectSubclass<imp::RnCanvas>)
        @extends RnLayout, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl RnCanvas {
    /// A canvas with the accessible `role`.
    pub(crate) fn create(role: gtk::AccessibleRole) -> Self {
        glib::Object::builder().property("accessible-role", role).build()
    }

    /// Draws `list` from the next frame on, if it differs from what is drawn.
    pub(crate) fn set_draw_list(&self, list: &DrawList) {
        let mut current = self.imp().list.borrow_mut();
        if *current != *list {
            *current = list.clone();
            drop(current);
            self.queue_draw();
        }
    }
}

/// Sets `color` as the cairo source.
fn source(cr: &cairo::Context, color: Color) {
    cr.set_source_rgba(
        f64::from(color.red) / 255.0,
        f64::from(color.green) / 255.0,
        f64::from(color.blue) / 255.0,
        f64::from(color.alpha) / 255.0,
    );
}

fn rect(cr: &cairo::Context, r: RectF) {
    cr.rectangle(
        f64::from(r.x.get()),
        f64::from(r.y.get()),
        f64::from(r.width.get()),
        f64::from(r.height.get()),
    );
}

fn stroke_with(cr: &cairo::Context, paint: Paint) {
    source(cr, paint.color);
    cr.set_line_width(f64::from(paint.stroke_width.get()));
    let _ = cr.stroke();
}

fn fill_with(cr: &cairo::Context, paint: Paint) {
    source(cr, paint.color);
    let _ = cr.fill();
}

fn matrix(transform: Transform2D) -> cairo::Matrix {
    // `Transform2D::apply` is x' = m11·x + m21·y + dx, y' = m12·x + m22·y
    // + dy; cairo's matrix is (xx, yx, xy, yy, x0, y0) for the same map.
    cairo::Matrix::new(
        f64::from(transform.m11.get()),
        f64::from(transform.m12.get()),
        f64::from(transform.m21.get()),
        f64::from(transform.m22.get()),
        f64::from(transform.dx.get()),
        f64::from(transform.dy.get()),
    )
}

/// Paints `list` on `cr`, with text laid out in `pango`'s font.
pub(crate) fn paint(cr: &cairo::Context, list: &DrawList, pango: &pango::Context) {
    // Each `Push*` opens a scope its `Pop` closes: transforms and clips as
    // cairo save/restore, opacity as a group painted at that alpha.
    let mut scopes: Vec<Option<f64>> = Vec::new();
    for command in list.commands() {
        match command {
            DrawCommand::FillRect(r, paint) => {
                rect(cr, *r);
                fill_with(cr, *paint);
            }
            DrawCommand::StrokeRect(r, paint) => {
                rect(cr, *r);
                stroke_with(cr, *paint);
            }
            DrawCommand::FillRoundedRect(bounds, radius, paint) => {
                let (x, y, w, h) = (
                    f64::from(bounds.x.get()),
                    f64::from(bounds.y.get()),
                    f64::from(bounds.width.get()),
                    f64::from(bounds.height.get()),
                );
                let radius = f64::from(radius.get()).min(w / 2.0).min(h / 2.0).max(0.0);
                let quarter = std::f64::consts::FRAC_PI_2;
                cr.new_sub_path();
                cr.arc(x + w - radius, y + radius, radius, -quarter, 0.0);
                cr.arc(x + w - radius, y + h - radius, radius, 0.0, quarter);
                cr.arc(x + radius, y + h - radius, radius, quarter, 2.0 * quarter);
                cr.arc(x + radius, y + radius, radius, 2.0 * quarter, 3.0 * quarter);
                cr.close_path();
                fill_with(cr, *paint);
            }
            DrawCommand::FillEllipse(r, paint) => {
                let (w, h) = (f64::from(r.width.get()), f64::from(r.height.get()));
                if w > 0.0 && h > 0.0 {
                    cr.save().ok();
                    cr.translate(f64::from(r.x.get()) + w / 2.0, f64::from(r.y.get()) + h / 2.0);
                    cr.scale(w / 2.0, h / 2.0);
                    cr.arc(0.0, 0.0, 1.0, 0.0, 2.0 * std::f64::consts::PI);
                    cr.restore().ok();
                    fill_with(cr, *paint);
                }
            }
            DrawCommand::StrokeLine(from, to, paint) => {
                cr.move_to(f64::from(from.x.get()), f64::from(from.y.get()));
                cr.line_to(f64::from(to.x.get()), f64::from(to.y.get()));
                stroke_with(cr, *paint);
            }
            DrawCommand::FillPath(path, paint) | DrawCommand::StrokePath(path, paint) => {
                cr.new_path();
                for segment in path.segments() {
                    match segment {
                        PathSegment::MoveTo(target) => {
                            cr.move_to(f64::from(target.x.get()), f64::from(target.y.get()));
                        }
                        PathSegment::LineTo(target) => {
                            cr.line_to(f64::from(target.x.get()), f64::from(target.y.get()));
                        }
                        PathSegment::QuadTo { control, to } => {
                            // A quadratic as the equivalent cubic.
                            let (x0, y0) = cr.current_point().unwrap_or((0.0, 0.0));
                            let (cx, cy) = (f64::from(control.x.get()), f64::from(control.y.get()));
                            let (x, y) = (f64::from(to.x.get()), f64::from(to.y.get()));
                            cr.curve_to(
                                x0 + 2.0 / 3.0 * (cx - x0),
                                y0 + 2.0 / 3.0 * (cy - y0),
                                x + 2.0 / 3.0 * (cx - x),
                                y + 2.0 / 3.0 * (cy - y),
                                x,
                                y,
                            );
                        }
                        PathSegment::CubicTo { first, second, to } => cr.curve_to(
                            f64::from(first.x.get()),
                            f64::from(first.y.get()),
                            f64::from(second.x.get()),
                            f64::from(second.y.get()),
                            f64::from(to.x.get()),
                            f64::from(to.y.get()),
                        ),
                        PathSegment::Close => cr.close_path(),
                    }
                }
                if matches!(command, DrawCommand::FillPath(..)) {
                    cr.set_fill_rule(cairo::FillRule::Winding);
                    fill_with(cr, *paint);
                } else {
                    stroke_with(cr, *paint);
                }
            }
            DrawCommand::Text { origin, text, size, color } => {
                let layout = pango::Layout::new(pango);
                let mut font = pango.font_description().unwrap_or_default();
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "a font size in Pango units, rounded"
                )]
                let pango_size = (f64::from(size.get()) * f64::from(pango::SCALE)).round() as i32;
                font.set_absolute_size(f64::from(pango_size.max(1)));
                layout.set_font_description(Some(&font));
                layout.set_text(text);
                source(cr, *color);
                cr.move_to(f64::from(origin.x.get()), f64::from(origin.y.get()));
                pangocairo::functions::show_layout(cr, &layout);
            }
            DrawCommand::Image(image, target) => {
                let Ok(width) = i32::try_from(image.width()) else { continue };
                let Ok(height) = i32::try_from(image.height()) else { continue };
                let Some(stride) = width.checked_mul(4) else { continue };
                // cairo's ARGB32 is premultiplied, native-endian: BGRA here.
                let Ok(surface) = cairo::ImageSurface::create_for_data(
                    image.premultiplied_bgra(),
                    cairo::Format::ARgb32,
                    width,
                    height,
                    stride,
                ) else {
                    continue;
                };
                cr.save().ok();
                cr.translate(f64::from(target.x.get()), f64::from(target.y.get()));
                cr.scale(
                    f64::from(target.width.get()) / f64::from(width),
                    f64::from(target.height.get()) / f64::from(height),
                );
                cr.set_source_surface(&surface, 0.0, 0.0).ok();
                // Edge pixels extend outward rather than fading into
                // transparency when scaled, and only the target is filled.
                cr.source().set_extend(cairo::Extend::Pad);
                cr.rectangle(0.0, 0.0, f64::from(width), f64::from(height));
                cr.fill().ok();
                cr.restore().ok();
            }
            DrawCommand::PushTransform(transform) => {
                cr.save().ok();
                cr.transform(matrix(*transform));
                scopes.push(None);
            }
            DrawCommand::PushClip(r) => {
                cr.save().ok();
                rect(cr, *r);
                cr.clip();
                scopes.push(None);
            }
            DrawCommand::PushOpacity(alpha) => {
                cr.push_group();
                scopes.push(Some(f64::from(alpha.get())));
            }
            DrawCommand::Pop => match scopes.pop() {
                Some(Some(alpha)) => {
                    if cr.pop_group_to_source().is_ok() {
                        cr.paint_with_alpha(alpha).ok();
                    }
                }
                Some(None) => {
                    cr.restore().ok();
                }
                // An unbalanced `Pop` closes nothing.
                None => {}
            },
            // Hit regions draw nothing; input consults them.
            DrawCommand::HitRegion(..) => {}
        }
    }
    // Scopes left open close at the end of the list.
    while let Some(scope) = scopes.pop() {
        match scope {
            Some(alpha) => {
                if cr.pop_group_to_source().is_ok() {
                    cr.paint_with_alpha(alpha).ok();
                }
            }
            None => {
                cr.restore().ok();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rustnative_core::{Color, DrawList, ImageData, Paint, Path, RectF, Transform2D, Vec2};

    use super::{cairo, paint};

    /// Paints `list` on a 100×100 transparent surface and returns its
    /// pixels as straight RGBA.
    fn render(list: &DrawList) -> impl Fn(i32, i32) -> (u8, u8, u8, u8) {
        let mut surface =
            cairo::ImageSurface::create(cairo::Format::ARgb32, 100, 100).expect("a surface");
        {
            let cr = cairo::Context::new(&surface).expect("a context");
            let pango = pangocairo::functions::create_context(&cr);
            paint(&cr, list, &pango);
        }
        surface.flush();
        let stride = usize::try_from(surface.stride()).expect("a stride");
        let data = surface.data().expect("pixels").to_vec();
        move |x, y| {
            let offset =
                usize::try_from(y).expect("y") * stride + usize::try_from(x).expect("x") * 4;
            // Native-endian premultiplied ARGB32: B, G, R, A in memory here.
            let (b, g, r, a) = (data[offset], data[offset + 1], data[offset + 2], data[offset + 3]);
            (r, g, b, a)
        }
    }

    const RED: Color = Color::rgb(255, 0, 0);
    const BLUE: Color = Color::rgb(0, 0, 255);

    #[test]
    fn fills_strokes_and_shapes_land_where_the_list_says() {
        let list = DrawList::new()
            .fill_rect(RectF::new(0.0, 0.0, 10.0, 10.0), Paint::color(RED))
            .stroke_line(
                Vec2::new(20.0, 5.0),
                Vec2::new(40.0, 5.0),
                Paint::color(BLUE).stroke_width(2.0),
            )
            .fill_ellipse(RectF::new(50.0, 0.0, 20.0, 20.0), Paint::color(RED))
            .fill_rounded_rect(RectF::new(0.0, 50.0, 40.0, 40.0), 15.0, Paint::color(BLUE))
            .fill_path(
                Path::new().move_to(60.0, 60.0).line_to(90.0, 60.0).line_to(60.0, 90.0).close(),
                Paint::color(RED),
            );
        let at = render(&list);
        assert_eq!(at(5, 5), (255, 0, 0, 255), "inside the filled rectangle");
        assert_eq!(at(15, 5).3, 0, "outside it");
        assert_eq!(at(30, 5), (0, 0, 255, 255), "on the line");
        assert_eq!(at(60, 10), (255, 0, 0, 255), "the ellipse's center");
        assert_eq!(at(51, 1).3, 0, "the ellipse's bounding corner is empty");
        assert_eq!(at(20, 70), (0, 0, 255, 255), "inside the rounded rectangle");
        assert_eq!(at(1, 51).3, 0, "its rounded corner is empty");
        assert_eq!(at(65, 65), (255, 0, 0, 255), "inside the path's triangle");
        assert_eq!(at(85, 85).3, 0, "outside its hypotenuse");
    }

    #[test]
    fn transforms_clips_and_opacity_scope_their_contents() {
        let list = DrawList::new()
            .push_transform(Transform2D::translation(50.0, 50.0))
            .fill_rect(RectF::new(0.0, 0.0, 10.0, 10.0), Paint::color(RED))
            .pop()
            .push_clip(RectF::new(0.0, 0.0, 5.0, 100.0))
            .fill_rect(RectF::new(0.0, 20.0, 20.0, 10.0), Paint::color(BLUE))
            .pop()
            .push_opacity(0.5)
            .fill_rect(RectF::new(80.0, 80.0, 10.0, 10.0), Paint::color(RED))
            .fill_rect(RectF::new(80.0, 80.0, 10.0, 10.0), Paint::color(RED))
            .pop();
        let at = render(&list);
        assert_eq!(at(55, 55), (255, 0, 0, 255), "translated");
        assert_eq!(at(5, 5).3, 0, "not drawn at the origin");
        assert_eq!(at(2, 25), (0, 0, 255, 255), "inside the clip");
        assert_eq!(at(10, 25).3, 0, "outside it");
        let (_, _, _, alpha) = at(85, 85);
        assert!((126..=129).contains(&alpha), "one layer at half opacity, not two: {alpha}");
    }

    #[test]
    fn text_and_images_are_drawn() {
        let image = ImageData::rgba(1, 1, vec![0, 255, 0, 255], false).expect("one green pixel");
        let list = DrawList::new()
            .text(Vec2::new(0.0, 0.0), "Hello", 20.0, Color::rgb(0, 0, 0))
            .image(image, RectF::new(50.0, 50.0, 30.0, 30.0));
        let at = render(&list);
        let inked = (0..60)
            .flat_map(|x| (0..30).map(move |y| (x, y)))
            .filter(|&(x, y)| at(x, y).3 > 128)
            .count();
        assert!(inked > 20, "the text left ink: {inked} pixels");
        assert_eq!(at(65, 65), (0, 255, 0, 255), "the image scaled into its rectangle");
    }
}
