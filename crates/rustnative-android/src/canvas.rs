//! Canvas nodes: a portable `DrawList` encoded once per change into one
//! byte buffer, which `RnCanvasView` replays in `onDraw` with `Canvas`,
//! `Paint`, `Path`, and `StaticLayout` (the host's text shaping and line
//! breaking) — one JNI call per change, not one per shape.
//!
//! The buffer is little-endian. Each command is an opcode byte and its
//! operands: `f32` coordinates in canvas units (dp; the view scales by the
//! density), `u32` ARGB colours, length-prefixed UTF-8 text, and images as
//! their size and premultiplied RGBA pixels.

use rustnative_core::{Color, DrawCommand, DrawList, Paint, PathSegment, RectF, Vec2};

/// The opcodes `RnCanvasView` reads.
pub(crate) mod op {
    pub(crate) const FILL_RECT: u8 = 1;
    pub(crate) const STROKE_RECT: u8 = 2;
    pub(crate) const FILL_ROUNDED_RECT: u8 = 3;
    pub(crate) const FILL_ELLIPSE: u8 = 4;
    pub(crate) const STROKE_LINE: u8 = 5;
    pub(crate) const FILL_PATH: u8 = 6;
    pub(crate) const STROKE_PATH: u8 = 7;
    pub(crate) const TEXT: u8 = 8;
    pub(crate) const IMAGE: u8 = 9;
    pub(crate) const PUSH_TRANSFORM: u8 = 10;
    pub(crate) const PUSH_CLIP: u8 = 11;
    pub(crate) const PUSH_OPACITY: u8 = 12;
    pub(crate) const POP: u8 = 13;
}

/// Path segment tags.
mod segment {
    pub(super) const MOVE: u8 = 0;
    pub(super) const LINE: u8 = 1;
    pub(super) const QUAD: u8 = 2;
    pub(super) const CUBIC: u8 = 3;
    pub(super) const CLOSE: u8 = 4;
}

struct Writer(Vec<u8>);

impl Writer {
    fn op(&mut self, op: u8) {
        self.0.push(op);
    }
    fn f32(&mut self, value: f32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    fn rect(&mut self, rect: RectF) {
        for value in [rect.x, rect.y, rect.width, rect.height] {
            self.f32(value.get());
        }
    }
    fn point(&mut self, point: Vec2) {
        self.f32(point.x.get());
        self.f32(point.y.get());
    }
    fn color(&mut self, color: Color) {
        self.u32(u32::from_be_bytes([color.alpha, color.red, color.green, color.blue]));
    }
    fn paint(&mut self, paint: Paint) {
        self.color(paint.color);
    }
    fn stroke(&mut self, paint: Paint) {
        self.color(paint.color);
        self.f32(paint.stroke_width.get());
    }
    fn text(&mut self, text: &str) {
        self.u32(u32::try_from(text.len()).unwrap_or(0));
        self.0.extend_from_slice(text.as_bytes());
    }
    fn path(&mut self, segments: &[PathSegment]) {
        self.u32(u32::try_from(segments.len()).unwrap_or(0));
        for segment in segments {
            match segment {
                PathSegment::MoveTo(to) => {
                    self.0.push(segment::MOVE);
                    self.point(*to);
                }
                PathSegment::LineTo(to) => {
                    self.0.push(segment::LINE);
                    self.point(*to);
                }
                PathSegment::QuadTo { control, to } => {
                    self.0.push(segment::QUAD);
                    self.point(*control);
                    self.point(*to);
                }
                PathSegment::CubicTo { first, second, to } => {
                    self.0.push(segment::CUBIC);
                    self.point(*first);
                    self.point(*second);
                    self.point(*to);
                }
                PathSegment::Close => self.0.push(segment::CLOSE),
            }
        }
    }
}

/// `list`, encoded for `RnCanvasView.setCommands`.
pub(crate) fn encode(list: &DrawList) -> Vec<u8> {
    let mut out = Writer(Vec::new());
    for command in list.commands() {
        match command {
            DrawCommand::FillRect(rect, paint) => {
                out.op(op::FILL_RECT);
                out.rect(*rect);
                out.paint(*paint);
            }
            DrawCommand::StrokeRect(rect, paint) => {
                out.op(op::STROKE_RECT);
                out.rect(*rect);
                out.stroke(*paint);
            }
            DrawCommand::FillRoundedRect(rect, radius, paint) => {
                out.op(op::FILL_ROUNDED_RECT);
                out.rect(*rect);
                out.f32(radius.get());
                out.paint(*paint);
            }
            DrawCommand::FillEllipse(rect, paint) => {
                out.op(op::FILL_ELLIPSE);
                out.rect(*rect);
                out.paint(*paint);
            }
            DrawCommand::StrokeLine(from, to, paint) => {
                out.op(op::STROKE_LINE);
                out.point(*from);
                out.point(*to);
                out.stroke(*paint);
            }
            DrawCommand::FillPath(path, paint) => {
                out.op(op::FILL_PATH);
                out.path(path.segments());
                out.paint(*paint);
            }
            DrawCommand::StrokePath(path, paint) => {
                out.op(op::STROKE_PATH);
                out.path(path.segments());
                out.stroke(*paint);
            }
            DrawCommand::Text { origin, text, size, color } => {
                out.op(op::TEXT);
                out.point(*origin);
                out.f32(size.get());
                out.color(*color);
                out.text(text);
            }
            DrawCommand::Image(image, rect) => {
                out.op(op::IMAGE);
                out.rect(*rect);
                out.u32(image.width());
                out.u32(image.height());
                out.0.extend_from_slice(&crate::pixels::premultiplied_rgba(image));
            }
            DrawCommand::PushTransform(transform) => {
                out.op(op::PUSH_TRANSFORM);
                for value in [
                    transform.m11,
                    transform.m12,
                    transform.m21,
                    transform.m22,
                    transform.dx,
                    transform.dy,
                ] {
                    out.f32(value.get());
                }
            }
            DrawCommand::PushClip(rect) => {
                out.op(op::PUSH_CLIP);
                out.rect(*rect);
            }
            DrawCommand::PushOpacity(opacity) => {
                out.op(op::PUSH_OPACITY);
                out.f32(opacity.get());
            }
            DrawCommand::Pop => out.op(op::POP),
            // A hit region draws nothing; input finds it in the list itself.
            DrawCommand::HitRegion(..) => {}
        }
    }
    out.0
}

#[cfg(test)]
mod tests {
    use rustnative_core::{Path, Scalar};

    use super::*;

    #[test]
    fn commands_encode_as_the_canvas_view_reads_them() {
        let list = DrawList::new()
            .fill_rect(RectF::new(1.0, 2.0, 3.0, 4.0), Paint::color(Color::rgb(255, 0, 0)))
            .push(DrawCommand::FillPath(
                Path::new().move_to(0.0, 0.0).line_to(5.0, 0.0).close(),
                Paint::color(Color::rgb(0, 0, 255)),
            ))
            .push(DrawCommand::Text {
                origin: Vec2::new(1.0, 1.0),
                text: "Hé".into(),
                size: Scalar::new(12.0),
                color: Color::rgb(0, 0, 0),
            })
            .push(DrawCommand::Pop);
        let bytes = encode(&list);
        assert_eq!(bytes[0], op::FILL_RECT);
        assert_eq!(u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]), 1.0_f32.to_bits());
        assert_eq!(u32::from_le_bytes([bytes[17], bytes[18], bytes[19], bytes[20]]), 0xffff_0000);
        // FILL_PATH: three segments.
        assert_eq!(bytes[21], op::FILL_PATH);
        assert_eq!(u32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]), 3);
        let text_at = 26 + (1 + 8) * 2 + 1 + 4;
        assert_eq!(bytes[text_at], op::TEXT);
        // "Hé" is three bytes of UTF-8, length-prefixed.
        let length_at = text_at + 1 + 8 + 4 + 4;
        assert_eq!(
            u32::from_le_bytes([
                bytes[length_at],
                bytes[length_at + 1],
                bytes[length_at + 2],
                bytes[length_at + 3]
            ]),
            3
        );
        assert_eq!(*bytes.last().unwrap_or(&0), op::POP);
    }
}
