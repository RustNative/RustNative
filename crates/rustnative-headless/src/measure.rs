//! Deterministic text metrics.

use rustnative_core::{DefaultIntrinsicMeasurer, IntrinsicMeasurer, NodeKind, Size};

/// The headless backend's text measurer: fixed-advance metrics, identical
/// on every machine, so a layout assertion or a golden file written on one
/// computer holds on every other.
///
/// The metrics are [`DefaultIntrinsicMeasurer`]'s — 8 logical pixels per
/// character, 32 per line, 24 of chrome around a button or text field —
/// multiplied by a text scale, which is how the layout conformance suite
/// (Milestone 41) exercises a person's larger-text setting without a
/// host that has one.
///
/// ```
/// use rustnative_core::{IntrinsicMeasurer, NodeKind};
/// use rustnative_headless::HeadlessMeasurer;
///
/// let normal = HeadlessMeasurer::new().measure(NodeKind::Label, Some("abcd"), None);
/// let large = HeadlessMeasurer::with_text_scale(2.0).measure(NodeKind::Label, Some("abcd"), None);
/// assert_eq!(normal.width, 32);
/// assert_eq!(large.width, 64);
/// assert_eq!(large.height, normal.height * 2);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeadlessMeasurer {
    text_scale: f32,
}

impl Default for HeadlessMeasurer {
    fn default() -> Self {
        Self::new()
    }
}

impl HeadlessMeasurer {
    /// Metrics at a text scale of 1.0.
    #[must_use]
    pub const fn new() -> Self {
        Self { text_scale: 1.0 }
    }

    /// Metrics at `scale` times the normal text size (clamped to 0.5–4.0,
    /// the range hosts offer).
    #[must_use]
    pub fn with_text_scale(scale: f32) -> Self {
        Self { text_scale: scale.clamp(0.5, 4.0) }
    }

    /// The text scale these metrics apply.
    #[must_use]
    pub const fn text_scale(&self) -> f32 {
        self.text_scale
    }
}

impl IntrinsicMeasurer for HeadlessMeasurer {
    fn measure(&self, kind: NodeKind, text: Option<&str>, max_width: Option<i32>) -> Size {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "scale is clamped to 0.5..=4.0 and sizes are logical pixels far below f32's \
                      exact-integer range; the result is clamped back into u32/i32 range"
        )]
        {
            let scale = self.text_scale;
            if (scale - 1.0).abs() < f32::EPSILON {
                return DefaultIntrinsicMeasurer.measure(kind, text, max_width);
            }
            // Measure unwrapped at scale 1, scale, then wrap against the
            // real available width — so larger text wraps sooner, exactly
            // as it does on a host.
            let unscaled_limit = max_width.map(|width| ((width as f32) / scale).max(1.0) as i32);
            let size = DefaultIntrinsicMeasurer.measure(kind, text, unscaled_limit);
            let width = ((size.width as f32) * scale).min(i32::MAX as f32) as u32;
            let height = ((size.height as f32) * scale).min(i32::MAX as f32) as u32;
            let width = max_width
                .map_or(width, |limit| width.min(u32::try_from(limit.max(1)).unwrap_or(1)));
            Size::new(width.max(1), height)
        }
    }
}
