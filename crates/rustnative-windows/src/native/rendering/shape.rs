//! A container's border and corner radius — the two approximated rows of
//! the Windows style capability table (`rustnative_style::WINDOWS`).
//!
//! A container is a plain child window this backend paints itself, so it
//! can draw a one-pixel border in the resolved border colour and be clipped
//! to a rounded window region. A native control keeps its system border
//! and shape: drawing either would mean owner-drawing it, which 2.14's
//! fourth rule forbids.
//!
//! Both values live in window properties (`SetPropW`), read back by the
//! container's own `WM_ERASEBKGND` and `WM_SIZE`, which cannot reach the
//! renderer; the container removes them at `WM_NCDESTROY`.

use windows_sys::Win32::Foundation::{COLORREF, HANDLE, HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{CreateRoundRectRgn, FrameRect, HDC, SetWindowRgn};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetClientRect, GetPropW, RemovePropW, SetPropW};

use super::styling::cached_brush;
use crate::native::win32::{best_effort, informational};

fn name(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

const BORDER: &str = "RustNative.Border";
const RADIUS: &str = "RustNative.Radius";

/// Stored as `value + 1` so that "no property" (a null handle) means "none".
fn set(hwnd: HWND, property: &str, value: Option<usize>) {
    let key = name(property);
    match value {
        // SAFETY: `hwnd` is a live window owned by the caller; `key` is a
        // NUL-terminated UTF-16 string that outlives the call; the handle
        // is an integer tag, never dereferenced by Windows or by us.
        Some(value) => best_effort(
            unsafe { SetPropW(hwnd, key.as_ptr(), (value + 1) as HANDLE) } != 0,
            "SetPropW",
            "the container keeps its previous border or shape",
        ),
        // SAFETY: as above; removing an absent property is a documented
        // no-op returning null.
        None => informational(unsafe { RemovePropW(hwnd, key.as_ptr()) }),
    }
}

fn get(hwnd: HWND, property: &str) -> Option<usize> {
    let key = name(property);
    // SAFETY: `hwnd` is the window whose procedure is running; `key` is a
    // NUL-terminated UTF-16 string that outlives the call.
    let value = unsafe { GetPropW(hwnd, key.as_ptr()) } as usize;
    value.checked_sub(1)
}

/// Records a container's resolved border colour and radius, and applies
/// the radius to its current size.
pub(crate) fn set_shape(hwnd: HWND, border: Option<COLORREF>, radius: u16) {
    set(hwnd, BORDER, border.map(|color| color as usize));
    set(hwnd, RADIUS, (radius > 0).then_some(usize::from(radius)));
    apply_region(hwnd);
}

/// Clips the container to its rounded rectangle (or removes the clip), at
/// its current client size. Called again on every `WM_SIZE`.
pub(crate) fn apply_region(hwnd: HWND) {
    let radius = get(hwnd, RADIUS).and_then(|radius| i32::try_from(radius).ok());
    let region = match radius {
        Some(radius) => {
            let mut client = RECT::default();
            // SAFETY: `hwnd` is live; `client` is a valid, exclusively
            // borrowed `RECT`.
            if unsafe { GetClientRect(hwnd, &raw mut client) } == 0 {
                return;
            }
            let diameter = radius.saturating_mul(2);
            // SAFETY: plain integer arguments; a null result (GDI handle
            // exhaustion) is handled below by leaving the window unclipped.
            unsafe {
                CreateRoundRectRgn(0, 0, client.right + 1, client.bottom + 1, diameter, diameter)
            }
        }
        None => std::ptr::null_mut(),
    };
    if radius.is_some() && region.is_null() {
        return;
    }
    // SAFETY: `hwnd` is live; `region` is null (remove the clip) or a
    // region created above, whose ownership passes to the system on
    // success — it must not be used or freed afterwards, and it is not.
    // `SetWindowRgn` with a null region succeeds on any window.
    let applied = unsafe { SetWindowRgn(hwnd, region, 1) } != 0;
    best_effort(applied, "SetWindowRgn", "the container keeps its previous shape");
}

/// Draws the container's border, if it has one, over the background just
/// erased into `hdc`.
pub(crate) fn paint_border(hwnd: HWND, hdc: HDC, client: &RECT) {
    let Some(color) = get(hwnd, BORDER).and_then(|color| COLORREF::try_from(color).ok()) else {
        return;
    };
    let brush = cached_brush(color);
    if brush.is_null() {
        return;
    }
    // SAFETY: `hdc` is the erase DC Windows passed for this message;
    // `client` is the container's client rectangle; `brush` is a live,
    // cache-owned brush this call only reads.
    let framed = unsafe { FrameRect(hdc, client, brush) } != 0;
    best_effort(framed, "FrameRect", "the border is missing for one frame");
}

/// The container's recorded border colour, if any.
#[cfg(test)]
pub(crate) fn border_of(hwnd: HWND) -> Option<COLORREF> {
    get(hwnd, BORDER).and_then(|color| COLORREF::try_from(color).ok())
}

/// Removes the properties before the window is destroyed, as `SetPropW`
/// requires.
pub(crate) fn forget(hwnd: HWND) {
    set(hwnd, BORDER, None);
    set(hwnd, RADIUS, None);
}
