//! Per-monitor DPI: the boundary between the core's logical pixels and the
//! device pixels Win32 addresses (`PLAN.md` Milestone 69, "Scale").
//!
//! The application manifest declares per-monitor V2 awareness, so Windows
//! hands this process device pixels and never stretches its windows. Layout
//! therefore runs in logical pixels — a 96-DPI pixel, the unit of every
//! `Size`, `Rect`, `EdgeInsets`, and style length — and this module converts
//! at the boundary, once per value:
//!
//! - geometry going *to* Win32 (window and control rectangles, scroll
//!   offsets, window regions) is multiplied by the window's scale;
//! - geometry coming *from* Win32 (client sizes, pointer positions, text
//!   measurements) is divided by it;
//! - fonts are created at their logical size times the scale (and the
//!   person's text scale), so text is as large as the layout made room for.
//!
//! A window's scale is its DPI over 96, read from `GetDpiForWindow` and
//! followed through `WM_DPICHANGED`. Rectangles convert their *edges* rather
//! than their origin and size separately, so neighbours that touch in
//! logical pixels still touch on screen at a fractional scale.

use rustnative_core::Rect;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;

/// The baseline DPI a logical pixel is defined at.
pub(crate) const BASELINE_DPI: u32 = 96;

#[cfg(test)]
thread_local! {
    /// A scale tests impose on every window of this thread, so geometry at
    /// 150 % is exercised on a machine at 96 DPI.
    static FORCED: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

/// Makes every window on this thread report `dpi` until the guard drops.
#[cfg(test)]
pub(crate) fn force_dpi(dpi: u32) -> ForcedDpi {
    FORCED.with(|forced| forced.set(Some(dpi)));
    ForcedDpi
}

/// Restores the real DPI when dropped.
#[cfg(test)]
pub(crate) struct ForcedDpi;

#[cfg(test)]
impl Drop for ForcedDpi {
    fn drop(&mut self) {
        FORCED.with(|forced| forced.set(None));
    }
}

/// `window`'s DPI (96 when it cannot be read).
pub(crate) fn dpi_of(window: HWND) -> u32 {
    #[cfg(test)]
    if let Some(dpi) = FORCED.with(std::cell::Cell::get) {
        return dpi;
    }
    if window.is_null() {
        return BASELINE_DPI;
    }
    // SAFETY: `GetDpiForWindow` takes a window handle by value and returns
    // 0 for one that is not a window, which is handled below.
    let dpi = unsafe { GetDpiForWindow(window) };
    if dpi == 0 { BASELINE_DPI } else { dpi }
}

/// The system DPI — the primary monitor's — which a window opens at before
/// it knows its own.
pub(crate) fn system_dpi() -> u32 {
    #[cfg(test)]
    if let Some(dpi) = FORCED.with(std::cell::Cell::get) {
        return dpi;
    }
    // SAFETY: no preconditions.
    let dpi = unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForSystem() };
    if dpi == 0 { BASELINE_DPI } else { dpi }
}

/// The scale a DPI stands for: device pixels per logical pixel. A DPI of 0
/// (not yet known) is the baseline.
#[must_use]
pub(crate) fn scale(dpi: u32) -> f64 {
    if dpi == 0 { 1.0 } else { f64::from(dpi) / f64::from(BASELINE_DPI) }
}

/// Rounds half away from zero into `i32`'s range.
fn round(value: f64) -> i32 {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "clamped to i32's range on the line above the cast"
    )]
    let rounded = value.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
    rounded
}

/// A logical length in device pixels.
#[must_use]
pub(crate) fn to_device(value: i32, dpi: u32) -> i32 {
    if dpi == BASELINE_DPI || dpi == 0 {
        return value;
    }
    round(f64::from(value) * scale(dpi))
}

/// A device length in logical pixels.
#[must_use]
pub(crate) fn to_logical(value: i32, dpi: u32) -> i32 {
    if dpi == BASELINE_DPI || dpi == 0 {
        return value;
    }
    round(f64::from(value) / scale(dpi))
}

/// A device length in logical pixels, rounded up — for a measurement, so
/// what was measured never ends up clipped by a pixel.
#[must_use]
pub(crate) fn to_logical_ceil(value: i32, dpi: u32) -> i32 {
    if dpi == BASELINE_DPI || dpi == 0 {
        return value;
    }
    #[allow(clippy::cast_possible_truncation, reason = "clamped to i32's range before the cast")]
    let rounded = (f64::from(value) / scale(dpi))
        .ceil()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
    rounded
}

/// A logical rectangle in device pixels, converting its edges so that
/// rectangles which touch still touch.
#[must_use]
pub(crate) fn rect_to_device(rect: Rect, dpi: u32) -> Rect {
    if dpi == BASELINE_DPI || dpi == 0 {
        return rect;
    }
    let left = to_device(rect.x, dpi);
    let top = to_device(rect.y, dpi);
    let right = to_device(rect.x.saturating_add(rect.width), dpi);
    let bottom = to_device(rect.y.saturating_add(rect.height), dpi);
    Rect::new(left, top, right.saturating_sub(left), bottom.saturating_sub(top))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_baseline_is_the_identity() {
        assert_eq!(to_device(37, 96), 37);
        assert_eq!(to_logical(37, 96), 37);
        let rect = Rect::new(3, 4, 5, 6);
        assert_eq!(rect_to_device(rect, 96), rect);
    }

    #[test]
    fn lengths_scale_and_round_half_away_from_zero() {
        assert_eq!(to_device(10, 144), 15);
        assert_eq!(to_device(1, 120), 1, "1.25 rounds to 1");
        assert_eq!(to_device(2, 120), 3, "2.5 rounds away from zero");
        assert_eq!(to_device(-2, 120), -3);
        assert_eq!(to_logical(15, 144), 10);
        assert_eq!(to_logical_ceil(16, 144), 11, "a measurement is never clipped");
    }

    #[test]
    fn touching_rectangles_still_touch_at_a_fractional_scale() {
        let left = Rect::new(0, 0, 7, 10);
        let right = Rect::new(7, 0, 7, 10);
        let (a, b) = (rect_to_device(left, 120), rect_to_device(right, 120));
        assert_eq!(a.x + a.width, b.x, "no gap and no overlap between neighbours");
        assert_eq!(rect_to_device(Rect::new(0, 0, 100, 50), 192), Rect::new(0, 0, 200, 100));
    }
}
