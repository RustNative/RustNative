//! Intrinsic (content-driven) size measurement using real GDI text metrics.
//!
//! This is the Windows implementation of `rustnative_core`'s
//! [`IntrinsicMeasurer`] seam: the layout engine asks "how large does this
//! label's own content want to be?" and this answers with what the system
//! font actually measures, rather than the character-count heuristic
//! `DefaultIntrinsicMeasurer` falls back to.
//!
//! Measurement is the *only* thing here. The GDI resources that realize a
//! node's colors and font live in `super::rendering::styling`, which owns
//! their lifetime — see that subsystem's `mod.rs` for why measuring text and
//! owning brushes are separate responsibilities.

use rustnative_core::{IntrinsicMeasurer, NodeKind, Size, Typography};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{DrawTextW, GetDC, HGDIOBJ, ReleaseDC, SelectObject};

use super::win32::best_effort;

/// Measures text against a real device context for one window, so a label's
/// intrinsic size reflects the font that will actually draw it.
pub(crate) struct WindowsIntrinsicMeasurer {
    pub(crate) window: HWND,
    /// The text scale fonts are realized at, so text measures in the font
    /// that will draw it.
    pub(crate) text_scale: f32,
}

thread_local! {
    /// Fonts for measuring, by family, size, weight, and scale (in
    /// thousandths) — a handful per application, kept for its life.
    static FONTS: std::cell::RefCell<std::collections::HashMap<(String, u16, u16, u32), isize>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// What a measurement depends on: the kind (its padding and wrapping), the
/// text, the width it may take, the font, and the text scale.
type MeasureKey = (NodeKind, String, Option<i32>, Option<(String, u16, u16)>, u32);

/// Measurements are kept up to this many; then the cache starts over.
const MEASUREMENTS_KEPT: usize = 8_192;

thread_local! {
    /// Measurements already taken. Measuring is a pure function of its
    /// key — a font measures a string the same way every time — and a
    /// relayout asks for the same few hundred again (the budget scenario
    /// found each costing a device context and a `DrawTextW`, 20 ms a
    /// relayout for a form of forty rows; `PLAN.md` Milestone 42).
    static MEASUREMENTS: std::cell::RefCell<std::collections::HashMap<MeasureKey, Size>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

fn text_scale_milli(scale: f32) -> u32 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a text scale in thousandths, small and positive"
    )]
    let milli = (scale * 1_000.0).round().max(0.0) as u32;
    milli
}

fn measuring_font(typography: &Typography, scale: f32) -> HGDIOBJ {
    let key =
        (typography.family.clone(), typography.size, typography.weight, text_scale_milli(scale));
    FONTS.with(|fonts| {
        *fonts
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| super::rendering::styling::create_font(typography, scale) as isize)
    }) as HGDIOBJ
}

impl IntrinsicMeasurer for WindowsIntrinsicMeasurer {
    fn measure_foreign(&self, kind: &str) -> Size {
        super::foreign::preferred_size(kind).unwrap_or(Size::new(0, 0))
    }

    fn measure(&self, kind: NodeKind, text: Option<&str>, max_width: Option<i32>) -> Size {
        self.measure_styled(kind, text, max_width, None)
    }

    fn measure_styled(
        &self,
        kind: NodeKind,
        text: Option<&str>,
        max_width: Option<i32>,
        typography: Option<&Typography>,
    ) -> Size {
        let key = (
            kind,
            text.unwrap_or_default().to_owned(),
            max_width,
            typography
                .map(|typography| (typography.family.clone(), typography.size, typography.weight)),
            text_scale_milli(self.text_scale),
        );
        if let Some(size) = MEASUREMENTS.with(|cache| cache.borrow().get(&key).copied()) {
            return size;
        }
        let size = self.measure_uncached(kind, text, max_width, typography);
        MEASUREMENTS.with(|cache| {
            let mut cache = cache.borrow_mut();
            if cache.len() >= MEASUREMENTS_KEPT {
                cache.clear();
            }
            cache.insert(key, size);
        });
        size
    }
}

impl WindowsIntrinsicMeasurer {
    fn measure_uncached(
        &self,
        kind: NodeKind,
        text: Option<&str>,
        max_width: Option<i32>,
        typography: Option<&Typography>,
    ) -> Size {
        const DT_WORDBREAK: u32 = 0x0000_0010;
        const DT_CALCRECT: u32 = 0x0000_0400;

        let text = text.unwrap_or_default();
        if text.is_empty() {
            return Size::new(
                match kind {
                    NodeKind::Button => 24,
                    _ => 1,
                },
                32,
            );
        }

        // SAFETY: `self.window` is a live HWND owned by this measurer for
        // its whole lifetime; a null return (checked below) is
        // `GetDC`'s documented failure signal.
        let hdc = unsafe { GetDC(self.window) };
        if hdc.is_null() {
            return Size::new(1, 32);
        }

        // Measured in the font that will draw it — the node's own, or the
        // default the theme gives text — at the person's text scale.
        let font = measuring_font(&typography.cloned().unwrap_or_default(), self.text_scale);
        let previous = if font.is_null() {
            std::ptr::null_mut()
        } else {
            // SAFETY: `hdc` is the live DC just obtained; `font` is a live
            // font this module keeps; the previous object is restored below.
            unsafe { SelectObject(hdc, font) }
        };

        let text_wide = super::util::wide(text);
        let padding = match kind {
            NodeKind::Button | NodeKind::TextInput => 24,
            // Each tab has its own padding; the labels arrive joined with
            // wide gaps (see the core snapshot), so a little more covers
            // the strip's own frame.
            NodeKind::TabBar => 32,
            NodeKind::Label
            | NodeKind::Control
            | NodeKind::Column
            | NodeKind::Row
            | NodeKind::Canvas
            | NodeKind::Surface => 0,
        };
        // `i32::MAX / 4` rather than `i32::MAX`: `DrawTextW` computes a
        // bounding box by adding to these bounds, so leaving three quarters
        // of the range as headroom keeps the arithmetic inside GDI from
        // overflowing on a pathologically long single line.
        let available_width = max_width.map_or(i32::MAX / 4, |width| (width - padding).max(1));
        let mut rect = RECT { left: 0, top: 0, right: available_width, bottom: i32::MAX / 4 };
        let flags = DT_CALCRECT
            | if matches!(kind, NodeKind::Label) && max_width.is_some() { DT_WORDBREAK } else { 0 };

        // SAFETY: `hdc` was just validated non-null and is still owned by
        // this call (not yet released); `text_wide` is a NUL-terminated
        // wide buffer, matching the `-1`-length-means-NUL-terminated
        // contract; `rect` is a valid, exclusively borrowed `RECT` for
        // `DrawTextW` to write the calculated bounds into with
        // `DT_CALCRECT`.
        let measured = unsafe { DrawTextW(hdc, text_wide.as_ptr(), -1, &raw mut rect, flags) };
        if !previous.is_null() {
            // SAFETY: restores the DC's own font before it is released.
            unsafe { SelectObject(hdc, previous) };
        }
        // SAFETY: `hdc` was obtained from `GetDC(self.window)` above and
        // is released here exactly once, pairing that call as its
        // documented contract requires.
        let released = unsafe { ReleaseDC(self.window, hdc) } != 0;
        // Best effort *only* because there is nothing to report it to: a
        // genuinely failed release would leak a device context from the
        // system's small common cache, so it is asserted in debug builds
        // rather than passed over.
        best_effort(released, "ReleaseDC", "a leaked DC would exhaust the common cache");

        if measured == 0 {
            return Size::new(1, 32);
        }

        // Both `.max(1)` calls also bound the subtraction: `DrawTextW` with
        // `DT_CALCRECT` writes a normalized rectangle, so `right >= left`
        // and `bottom >= top`, and neither difference can exceed the
        // `i32::MAX / 4` bounds given above.
        let width = (rect.right - rect.left + padding).max(1);
        let height = (rect.bottom - rect.top).max(1);
        // `.max(1)` above guarantees both are positive, so the cast to an
        // unsigned `Size` cannot actually lose a sign — clippy's
        // `cast_sign_loss` can't see that guarantee through `.max`, so it
        // is named explicitly here rather than left for a reader to
        // re-derive.
        #[allow(clippy::cast_sign_loss)]
        Size::new(width as u32, height as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::test_support::TestWindow;

    #[test]
    fn measuring_empty_text_does_not_touch_the_device_context() {
        let window = TestWindow::new();
        let measurer = WindowsIntrinsicMeasurer { window: window.hwnd, text_scale: 1.0 };
        let size = measurer.measure(NodeKind::Label, None, None);
        assert!(size.width > 0 && size.height > 0, "even an empty label reserves some space");
    }

    #[test]
    fn measuring_more_text_never_produces_a_narrower_result() {
        let window = TestWindow::new();
        let measurer = WindowsIntrinsicMeasurer { window: window.hwnd, text_scale: 1.0 };
        let short = measurer.measure(NodeKind::Label, Some("Hi"), None);
        let long = measurer.measure(
            NodeKind::Label,
            Some("This is a substantially longer piece of label text"),
            None,
        );
        assert!(
            long.width >= short.width,
            "real GDI measurement of longer text must not report a narrower box \
             (short={short:?}, long={long:?})"
        );
    }

    #[test]
    fn measuring_respects_a_max_width_constraint_for_wrapping_kinds() {
        let window = TestWindow::new();
        let measurer = WindowsIntrinsicMeasurer { window: window.hwnd, text_scale: 1.0 };
        let long_text = "word ".repeat(40);
        let unconstrained = measurer.measure(NodeKind::Label, Some(&long_text), None);
        let constrained = measurer.measure(NodeKind::Label, Some(&long_text), Some(80));
        assert!(
            constrained.width <= unconstrained.width,
            "constraining max_width must not report a wider result than unconstrained \
             (constrained={constrained:?}, unconstrained={unconstrained:?})"
        );
    }

    #[test]
    fn measuring_a_pathologically_long_line_stays_within_the_coordinate_domain() {
        let window = TestWindow::new();
        let measurer = WindowsIntrinsicMeasurer { window: window.hwnd, text_scale: 1.0 };
        let huge = "x".repeat(100_000);
        let size = measurer.measure(NodeKind::Label, Some(&huge), None);
        assert!(size.width > 0, "an enormous single line still measures to a usable width");
        assert!(size.height > 0);
    }
}
