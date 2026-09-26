//! Interpolating the animatable value kinds.
//!
//! Geometry interpolates in its own integer space (rounded, since a native
//! object is positioned in whole pixels), and color per channel. Mixing
//! kinds — a position animated toward a color — is not an error to guess
//! at: [`between`] reports it as `None` and the caller drops the frame.

use super::AnimatedValue;
use crate::input::Scalar;
use crate::layout::{Point, Size};
use crate::style::Color;

pub(super) fn between(
    from: AnimatedValue,
    to: AnimatedValue,
    progress: f32,
) -> Option<AnimatedValue> {
    Some(match (from, to) {
        (AnimatedValue::Offset(from), AnimatedValue::Offset(to)) => AnimatedValue::Offset(
            Point::new(coordinate(from.x, to.x, progress), coordinate(from.y, to.y, progress)),
        ),
        (AnimatedValue::Size(from), AnimatedValue::Size(to)) => AnimatedValue::Size(Size::new(
            dimension(from.width, to.width, progress),
            dimension(from.height, to.height, progress),
        )),
        (AnimatedValue::Scalar(from), AnimatedValue::Scalar(to)) => {
            AnimatedValue::Scalar(Scalar::new(lerp(from.get(), to.get(), progress)))
        }
        (AnimatedValue::Color(from), AnimatedValue::Color(to)) => {
            AnimatedValue::Color(color(from, to, progress))
        }
        _ => return None,
    })
}

fn lerp(from: f32, to: f32, progress: f32) -> f32 {
    from + (to - from) * progress
}

/// Interpolates one signed coordinate, rounding to the nearest pixel and
/// saturating rather than wrapping at the extremes.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "on-screen coordinates are far inside f32's exact-integer range, and the result is \
              clamped to i32 before conversion"
)]
fn coordinate(from: i32, to: i32, progress: f32) -> i32 {
    let value = lerp(from as f32, to as f32, progress).round();
    value.clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

/// Interpolates one unsigned dimension. A size never goes negative, however
/// far a spring overshoots.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "clamped into u32's range immediately before conversion"
)]
fn dimension(from: u32, to: u32, progress: f32) -> u32 {
    let value = lerp(from as f32, to as f32, progress).round();
    value.clamp(0.0, u32::MAX as f32) as u32
}

fn color(from: Color, to: Color, progress: f32) -> Color {
    Color::rgba(
        channel(from.red, to.red, progress),
        channel(from.green, to.green, progress),
        channel(from.blue, to.blue, progress),
        channel(from.alpha, to.alpha, progress),
    )
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped into u8's range immediately before conversion"
)]
fn channel(from: u8, to: u8, progress: f32) -> u8 {
    let value = lerp(f32::from(from), f32::from(to), progress).round();
    value.clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_exact() {
        let from = AnimatedValue::Offset(Point::new(-10, 5));
        let to = AnimatedValue::Offset(Point::new(90, 5));
        assert_eq!(between(from, to, 0.0), Some(from));
        assert_eq!(between(from, to, 1.0), Some(to));
    }

    #[test]
    fn geometry_moves_proportionally_and_rounds_to_whole_pixels() {
        let from = AnimatedValue::Offset(Point::new(0, 0));
        let to = AnimatedValue::Offset(Point::new(10, 100));
        assert_eq!(between(from, to, 0.25), Some(AnimatedValue::Offset(Point::new(3, 25))));
        let from = AnimatedValue::Size(Size::new(10, 10));
        let to = AnimatedValue::Size(Size::new(20, 0));
        assert_eq!(between(from, to, 0.5), Some(AnimatedValue::Size(Size::new(15, 5))));
    }

    #[test]
    fn overshoot_never_produces_a_negative_size() {
        let from = AnimatedValue::Size(Size::new(10, 10));
        let to = AnimatedValue::Size(Size::new(0, 0));
        assert_eq!(between(from, to, 1.5), Some(AnimatedValue::Size(Size::new(0, 0))));
    }

    #[test]
    fn colors_interpolate_per_channel_including_alpha() {
        let from = AnimatedValue::Color(Color::rgba(0, 0, 0, 0));
        let to = AnimatedValue::Color(Color::rgba(255, 100, 50, 200));
        assert_eq!(
            between(from, to, 0.5),
            Some(AnimatedValue::Color(Color::rgba(128, 50, 25, 100)))
        );
    }

    #[test]
    fn mismatched_kinds_do_not_interpolate() {
        let offset = AnimatedValue::Offset(Point::new(0, 0));
        let scalar = AnimatedValue::Scalar(Scalar::ONE);
        assert_eq!(between(offset, scalar, 0.5), None);
    }
}
