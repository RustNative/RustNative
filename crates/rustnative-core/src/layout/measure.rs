//! Platform-supplied intrinsic (content-driven) measurement.
//!
//! See `crate::layout::engine`'s module doc for why this file's `i32`/
//! `usize`/`u32` casts are allowed at the module level: every cast here is
//! likewise preceded by an explicit `.min`/`.max` establishing the value is
//! in range, which this module's own
//! `measurement_never_panics_on_pathologically_large_input` test exercises
//! directly.
#![allow(clippy::cast_sign_loss, clippy::cast_possible_wrap, clippy::cast_possible_truncation)]

use super::geometry::Size;
use crate::node::NodeKind;

/// Supplies platform-specific intrinsic measurements for leaf nodes (how
/// large a label/button/text-input's own content wants to be, independent of
/// the space a parent container happens to offer it).
pub trait IntrinsicMeasurer {
    /// Returns the natural size a node of `kind` with `text` content wants,
    /// wrapping within `max_width` if given.
    fn measure(&self, kind: NodeKind, text: Option<&str>, max_width: Option<i32>) -> Size;

    /// [`Self::measure`] in the font the node will be drawn in, where the
    /// node's resolved style names one — what a host measuring with real
    /// font metrics needs, so a node never measures in one font and draws
    /// in another.
    fn measure_styled(
        &self,
        kind: NodeKind,
        text: Option<&str>,
        max_width: Option<i32>,
        typography: Option<&crate::style::Typography>,
    ) -> Size {
        let _ = typography;
        self.measure(kind, text, max_width)
    }

    /// The natural size of a native control (Milestone 48): its text's
    /// size plus the control's own chrome, or a fixed size for controls
    /// without text. A backend with real metrics measures the text; the
    /// chrome here is the conventional desktop size of each control.
    fn measure_control(&self, control: &crate::control::Control) -> Size {
        use crate::control::Control;
        let text = |text: &str, chrome: u32, height: u32| {
            let measured = self.measure(NodeKind::Label, Some(text), None);
            Size::new(measured.width.saturating_add(chrome), measured.height.max(height))
        };
        match control {
            Control::Checkbox { label, .. }
            | Control::Radio { label, .. }
            | Control::Toggle { label, .. } => text(label, 24, 20),
            Control::Slider { .. } => Size::new(160, 28),
            Control::Progress { .. } => Size::new(160, 16),
            Control::Select { options, .. } => {
                let widest = options
                    .iter()
                    .map(|option| self.measure(NodeKind::Label, Some(option), None).width);
                Size::new(widest.max().unwrap_or(0).saturating_add(32).max(80), 26)
            }
            Control::ListBox { items, .. } => {
                let widest =
                    items.iter().map(|item| self.measure(NodeKind::Label, Some(item), None).width);
                let rows = u32::try_from(items.len().clamp(3, 8)).unwrap_or(8);
                Size::new(widest.max().unwrap_or(0).saturating_add(24).max(120), rows * 18 + 4)
            }
            Control::DatePicker { .. } => Size::new(140, 26),
            Control::Spinner { .. } => Size::new(96, 26),
            Control::Separator => Size::new(0, 2),
            Control::Link { text: link } => text(link, 0, 18),
            Control::MultilineText { .. } => Size::new(240, 80),
            Control::Image { image } => Size::new(image.width(), image.height()),
        }
    }

    /// The natural size of a foreign object of the given factory `kind`
    /// ([`crate::Node::foreign`]): what the backend's factory reports, or
    /// nothing (the layout must size it) when this measurer knows none.
    fn measure_foreign(&self, kind: &str) -> Size {
        let _ = kind;
        Size::new(0, 0)
    }
}

/// A platform-independent measurement heuristic used by tests and any
/// backend that has not yet supplied its own native text metrics.
#[derive(Debug, Default)]
pub struct DefaultIntrinsicMeasurer;

impl IntrinsicMeasurer for DefaultIntrinsicMeasurer {
    fn measure(&self, kind: NodeKind, text: Option<&str>, max_width: Option<i32>) -> Size {
        // Every arithmetic step here is saturating: this measurer sits
        // directly on data an application can make arbitrarily large (an
        // enormous string, `LayoutStyle::constraints` set to `i32::MAX`),
        // and layout must degrade to a clamped value rather than panic
        // (debug builds) or silently wrap (release builds) on such input —
        // see the standards audit's P1.19 finding.
        let characters =
            text.map_or(0, |value| value.chars().count()).min(i32::MAX as usize) as i32;
        let intrinsic_width = characters
            .saturating_mul(8)
            .saturating_add(match kind {
                NodeKind::Button | NodeKind::TextInput => 24,
                NodeKind::TabBar => 48,
                NodeKind::Label
                | NodeKind::Control
                | NodeKind::Column
                | NodeKind::Row
                | NodeKind::Canvas
                | NodeKind::Surface => 0,
            })
            .max(1);
        let available_width = max_width.unwrap_or(intrinsic_width).max(1);
        let lines = intrinsic_width
            .saturating_add(available_width.saturating_sub(1))
            .saturating_div(available_width)
            .max(1);
        let width = intrinsic_width.min(available_width);

        Size::new(width as u32, lines.saturating_mul(32) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measurement_never_panics_on_pathologically_large_input() {
        let measurer = DefaultIntrinsicMeasurer;
        let huge_text = "x".repeat(1_000_000);
        let size = measurer.measure(NodeKind::Label, Some(&huge_text), Some(i32::MAX));
        assert!(size.width > 0);
        assert!(size.height > 0);

        let size = measurer.measure(NodeKind::Label, Some(&huge_text), Some(1));
        assert!(size.width > 0);
        assert!(size.height > 0);
    }
}
