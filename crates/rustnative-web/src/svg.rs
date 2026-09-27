//! A canvas's draw list as inline SVG: the drawing is part of the
//! document — scalable, selectable where it is text, printable, and present
//! before any script runs — rather than pixels painted by script into a
//! `<canvas>` after load.

use std::fmt::Write as _;

use rustnative_core::{Color, DrawCommand, DrawList, Paint, Path, PathSegment, RectF, Transform2D};

use crate::css::fixed3;
use crate::dom::Element;

fn number(value: f32) -> String {
    fixed3(value)
}

fn paint(color: Color) -> String {
    rustnative_style::model::hex(color)
}

fn rect(tag: &str, rect: RectF) -> Element {
    Element::new(tag)
        .attr("x", number(rect.x.get()))
        .attr("y", number(rect.y.get()))
        .attr("width", number(rect.width.get()))
        .attr("height", number(rect.height.get()))
}

fn fill(element: Element, paint_: Paint) -> Element {
    element.attr("fill", paint(paint_.color))
}

fn stroke(element: Element, paint_: Paint) -> Element {
    element
        .attr("fill", "none")
        .attr("stroke", paint(paint_.color))
        .attr("stroke-width", number(paint_.stroke_width.get()))
}

fn path_data(path: &Path) -> String {
    let mut data = String::new();
    for segment in path.segments() {
        if !data.is_empty() {
            data.push(' ');
        }
        match segment {
            PathSegment::MoveTo(point) => {
                let _ = write!(data, "M{} {}", number(point.x.get()), number(point.y.get()));
            }
            PathSegment::LineTo(point) => {
                let _ = write!(data, "L{} {}", number(point.x.get()), number(point.y.get()));
            }
            PathSegment::QuadTo { control, to } => {
                let _ = write!(
                    data,
                    "Q{} {} {} {}",
                    number(control.x.get()),
                    number(control.y.get()),
                    number(to.x.get()),
                    number(to.y.get())
                );
            }
            PathSegment::CubicTo { first, second, to } => {
                let _ = write!(
                    data,
                    "C{} {} {} {} {} {}",
                    number(first.x.get()),
                    number(first.y.get()),
                    number(second.x.get()),
                    number(second.y.get()),
                    number(to.x.get()),
                    number(to.y.get())
                );
            }
            PathSegment::Close => data.push('Z'),
        }
    }
    data
}

fn matrix(transform: &Transform2D) -> String {
    format!(
        "matrix({} {} {} {} {} {})",
        number(transform.m11.get()),
        number(transform.m12.get()),
        number(transform.m21.get()),
        number(transform.m22.get()),
        number(transform.dx.get()),
        number(transform.dy.get())
    )
}

/// `list` as an `<svg>` element in canvas units (one unit, one CSS pixel),
/// sized to its container.
#[must_use]
pub fn draw_list(list: &DrawList) -> Element {
    // Each open `Push*` is an element on the stack; `Pop` closes it into
    // its parent.
    let mut stack: Vec<Element> = vec![
        Element::new("svg")
            .attr("xmlns", "http://www.w3.org/2000/svg")
            .attr("aria-hidden", "true")
            .attr("focusable", "false"),
    ];
    let push = |stack: &mut Vec<Element>, element: Element| {
        if let Some(top) = stack.last_mut() {
            top.children.push(crate::dom::Child::Element(element));
        }
    };
    for command in list.commands() {
        match command {
            DrawCommand::FillRect(area, style) => {
                push(&mut stack, fill(rect("rect", *area), *style));
            }
            DrawCommand::StrokeRect(area, style) => {
                push(&mut stack, stroke(rect("rect", *area), *style));
            }
            DrawCommand::FillRoundedRect(area, radius, style) => {
                let element = rect("rect", *area)
                    .attr("rx", number(radius.get()))
                    .attr("ry", number(radius.get()));
                push(&mut stack, fill(element, *style));
            }
            DrawCommand::FillEllipse(area, style) => {
                let element = Element::new("ellipse")
                    .attr("cx", number(area.x.get() + area.width.get() / 2.0))
                    .attr("cy", number(area.y.get() + area.height.get() / 2.0))
                    .attr("rx", number(area.width.get() / 2.0))
                    .attr("ry", number(area.height.get() / 2.0));
                push(&mut stack, fill(element, *style));
            }
            DrawCommand::StrokeLine(from, to, style) => {
                let element = Element::new("line")
                    .attr("x1", number(from.x.get()))
                    .attr("y1", number(from.y.get()))
                    .attr("x2", number(to.x.get()))
                    .attr("y2", number(to.y.get()));
                push(&mut stack, stroke(element, *style));
            }
            DrawCommand::FillPath(path, style) => {
                push(&mut stack, fill(Element::new("path").attr("d", path_data(path)), *style));
            }
            DrawCommand::StrokePath(path, style) => {
                push(&mut stack, stroke(Element::new("path").attr("d", path_data(path)), *style));
            }
            DrawCommand::Text { origin, text, size, color } => {
                let element = Element::new("text")
                    .attr("x", number(origin.x.get()))
                    .attr("y", number(origin.y.get()))
                    .attr("font-size", number(size.get()))
                    .attr("fill", paint(*color))
                    .attr("dominant-baseline", "hanging")
                    .text(text.clone());
                push(&mut stack, element);
            }
            DrawCommand::Image(image, area) => {
                let element = rect("image", *area)
                    .attr("href", crate::png::data_uri(image))
                    .attr("preserveAspectRatio", "none");
                push(&mut stack, element);
            }
            DrawCommand::PushTransform(transform) => {
                stack.push(Element::new("g").attr("transform", matrix(transform)));
            }
            DrawCommand::PushClip(area) => {
                // A nested viewport clips to its bounds and, with a view box
                // equal to them, keeps the drawing's coordinates.
                let viewport = rect("svg", *area)
                    .attr(
                        "viewBox",
                        format!(
                            "{} {} {} {}",
                            number(area.x.get()),
                            number(area.y.get()),
                            number(area.width.get()),
                            number(area.height.get())
                        ),
                    )
                    .attr("overflow", "hidden");
                stack.push(viewport);
            }
            DrawCommand::PushOpacity(opacity) => {
                stack.push(Element::new("g").attr("opacity", number(opacity.get())));
            }
            DrawCommand::Pop => {
                if stack.len() > 1 {
                    if let Some(closed) = stack.pop() {
                        push(&mut stack, closed);
                    }
                }
            }
            DrawCommand::HitRegion(id, area) => {
                let element = rect("rect", *area)
                    .attr("fill", "#000")
                    .attr("fill-opacity", "0")
                    .attr("data-region", id.to_string());
                push(&mut stack, element);
            }
        }
    }
    // A list that forgot a `Pop` still draws what it pushed.
    while stack.len() > 1 {
        if let Some(closed) = stack.pop() {
            push(&mut stack, closed);
        }
    }
    stack.pop().unwrap_or_else(|| Element::new("svg"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustnative_core::Vec2;

    #[test]
    fn a_chart_becomes_shapes_in_groups() {
        let list = DrawList::new()
            .fill_rect(RectF::new(0.0, 0.0, 200.0, 100.0), Paint::color(Color::rgb(255, 255, 255)))
            .push_transform(Transform2D::translation(20.0, 10.0))
            .fill_rect(RectF::new(0.0, 0.0, 30.0, 80.0), Paint::color(Color::rgb(40, 90, 200)))
            .hit_region(1, RectF::new(0.0, 0.0, 30.0, 80.0))
            .pop()
            .stroke_line(
                Vec2::new(0.0, 99.5),
                Vec2::new(200.0, 99.5),
                Paint::color(Color::rgb(0, 0, 0)).stroke_width(1.0),
            );
        let svg = draw_list(&list);
        assert_eq!(svg.tag, "svg");
        let tags: Vec<&str> = svg
            .children
            .iter()
            .filter_map(|child| match child {
                crate::dom::Child::Element(element) => Some(element.tag.as_str()),
                crate::dom::Child::Text(_) => None,
            })
            .collect();
        assert_eq!(tags, ["rect", "g", "line"]);
        let html = crate::html::render(&svg);
        assert!(html.contains("<g transform=\"matrix(1 0 0 1 20 10)\">"), "{html}");
        assert!(html.contains("data-region=\"1\""), "{html}");
        assert!(html.contains("y1=\"99.5\""), "{html}");
    }
}
