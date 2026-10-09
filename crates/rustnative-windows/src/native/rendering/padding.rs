//! A leaf's own padding (`VisualStyle::padding`) on the system controls.
//!
//! A `BUTTON` takes it as text margins (`BCM_SETTEXTMARGIN`), added to its
//! own. `STATIC` and `EDIT` have no content inset of their own, so the padding
//! becomes non-client area: a comctl32 subclass shrinks the client rectangle
//! by it in `WM_NCCALCSIZE` (start and end follow the control's direction)
//! and fills the band in `WM_NCPAINT` with the brush the control's own
//! background comes from — the control's `WM_CTLCOLOR*` answer — so the
//! band reads as the control's box and its text sits inset inside it. The
//! layout engine has already measured the box with the padding.
//!
//! The insets live in two window properties (`SetPropW`), start and end in
//! one and top and bottom in the other, 16 bits each; the subclass removes
//! them at `WM_NCDESTROY`.

use rustnative_core::EdgeInsets;
use windows_sys::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    ClientToScreen, ExcludeClipRect, FillRect, GetWindowDC, HBRUSH, HDC, ReleaseDC,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetClientRect, GetParent, GetPropW, GetWindowLongPtrW, GetWindowRect, HTNOWHERE,
    RemovePropW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SendMessageW, SetPropW, SetWindowPos, WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC, WM_NCCALCSIZE,
    WM_NCDESTROY, WM_NCHITTEST, WM_NCPAINT, WS_EX_LAYOUTRTL,
};

use crate::native::win32::{best_effort, informational};

/// `BCM_SETTEXTMARGIN` and `BCM_GETTEXTMARGIN` (comctl32 6).
const BCM_SETTEXTMARGIN: u32 = 0x1604;
const BCM_GETTEXTMARGIN: u32 = 0x1605;

/// This subclass's id ("PAD").
const SUBCLASS_ID: usize = 0x0050_4144;
const HORIZONTAL: &str = "RustNative.PaddingH";
const VERTICAL: &str = "RustNative.PaddingV";

fn name(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn pack(low: i32, high: i32) -> usize {
    let half = |value: i32| u16::try_from(value.max(0)).unwrap_or(u16::MAX) as usize;
    half(low) | half(high) << 16
}

/// Records `hwnd`'s padding (or removes it) and recomputes its client area
/// when it changed. `is_edit` picks the `WM_CTLCOLOR*` the band is filled
/// from.
pub(crate) fn set(hwnd: HWND, padding: Option<EdgeInsets>, is_edit: bool) {
    let padding = padding.filter(|padding| *padding != EdgeInsets::default());
    if padding_of(hwnd) == padding {
        return;
    }
    match padding {
        Some(padding) => {
            put(hwnd, HORIZONTAL, Some(pack(padding.start, padding.end)));
            put(hwnd, VERTICAL, Some(pack(padding.top, padding.bottom)));
            // SAFETY: `hwnd` is a live control this thread created;
            // `subclass_proc` has the signature `SetWindowSubclass` requires
            // and outlives the window. Installing it again only updates its
            // data.
            let installed = unsafe {
                SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, usize::from(is_edit))
            } != 0;
            best_effort(installed, "SetWindowSubclass", "the text is not inset");
        }
        None => forget(hwnd),
    }
    // SAFETY: `hwnd` is live; `SWP_FRAMECHANGED` with no move, size, or
    // z-order change only asks the window to recompute its frame.
    let refreshed = unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        )
    } != 0;
    best_effort(refreshed, "SetWindowPos(frame changed)", "the old inset shows until resized");
}

/// Sets a button's text margins to its own plus `padding`, recording
/// `padding` so the button's own can be recovered when it changes.
pub(crate) fn set_button(hwnd: HWND, padding: Option<EdgeInsets>) {
    let padding = padding.filter(|padding| *padding != EdgeInsets::default());
    let recorded = padding_of(hwnd);
    if recorded == padding {
        return;
    }
    let mut margin = RECT::default();
    // SAFETY: `hwnd` is a live button; the message writes one `RECT`.
    unsafe { SendMessageW(hwnd, BCM_GETTEXTMARGIN, 0, &raw mut margin as LPARAM) };
    // A mirrored button draws mirrored: its left margin is its start.
    let shift = |margin: &mut RECT, padding: EdgeInsets, sign: i32| {
        margin.left += sign * padding.start;
        margin.top += sign * padding.top;
        margin.right += sign * padding.end;
        margin.bottom += sign * padding.bottom;
    };
    if let Some(recorded) = recorded {
        shift(&mut margin, recorded, -1);
    }
    if let Some(padding) = padding {
        shift(&mut margin, padding, 1);
        put(hwnd, HORIZONTAL, Some(pack(padding.start, padding.end)));
        put(hwnd, VERTICAL, Some(pack(padding.top, padding.bottom)));
    } else {
        forget(hwnd);
    }
    // SAFETY: as above, reading one `RECT`.
    unsafe { SendMessageW(hwnd, BCM_SETTEXTMARGIN, 0, &raw const margin as LPARAM) };
}

/// The text margins a button draws with, or `None` without comctl32 6.
#[cfg(test)]
pub(crate) fn button_margin(hwnd: HWND) -> Option<RECT> {
    let mut margin = RECT::default();
    // SAFETY: as in `set_button`.
    let answered = unsafe { SendMessageW(hwnd, BCM_GETTEXTMARGIN, 0, &raw mut margin as LPARAM) };
    (answered != 0).then_some(margin)
}

/// `hwnd`'s recorded padding, if any.
pub(crate) fn padding_of(hwnd: HWND) -> Option<EdgeInsets> {
    let (horizontal, vertical) = (get(hwnd, HORIZONTAL)?, get(hwnd, VERTICAL)?);
    let half = |value: usize, shift: u32| i32::try_from((value >> shift) & 0xffff).unwrap_or(0);
    Some(EdgeInsets {
        top: half(vertical, 0),
        end: half(horizontal, 16),
        bottom: half(vertical, 16),
        start: half(horizontal, 0),
    })
}

fn forget(hwnd: HWND) {
    put(hwnd, HORIZONTAL, None);
    put(hwnd, VERTICAL, None);
}

/// Stored as `value + 1` so that "no property" (a null handle) means "none".
fn put(hwnd: HWND, property: &str, value: Option<usize>) {
    let key = name(property);
    match value {
        // SAFETY: `hwnd` is live; `key` is NUL-terminated and outlives the
        // call; the handle is an integer tag, never dereferenced.
        Some(value) => best_effort(
            unsafe { SetPropW(hwnd, key.as_ptr(), (value + 1) as HANDLE) } != 0,
            "SetPropW",
            "the control keeps its previous inset",
        ),
        // SAFETY: as above; removing an absent property is a no-op.
        None => informational(unsafe { RemovePropW(hwnd, key.as_ptr()) }),
    }
}

fn get(hwnd: HWND, property: &str) -> Option<usize> {
    let key = name(property);
    // SAFETY: as in `put`.
    let value = unsafe { GetPropW(hwnd, key.as_ptr()) } as usize;
    value.checked_sub(1)
}

fn is_rtl(hwnd: HWND) -> bool {
    // SAFETY: reading a live window's extended style.
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    style & isize::try_from(WS_EX_LAYOUTRTL).unwrap_or(0) != 0
}

/// The client rectangle in window coordinates, and the window's size.
fn client_in_window(hwnd: HWND) -> Option<(RECT, i32, i32)> {
    let (mut window, mut client) = (RECT::default(), RECT::default());
    let mut origin = POINT::default();
    // SAFETY: `hwnd` is live; each out-parameter is a valid, exclusively
    // borrowed struct.
    let ok = unsafe {
        GetWindowRect(hwnd, &raw mut window) != 0
            && GetClientRect(hwnd, &raw mut client) != 0
            && ClientToScreen(hwnd, &raw mut origin) != 0
    };
    if !ok {
        return None;
    }
    // A mirrored window's client origin is its right edge, and so is the
    // origin of its window DC.
    let left = if is_rtl(hwnd) { window.right - origin.x } else { origin.x - window.left };
    let top = origin.y - window.top;
    Some((
        RECT { left, top, right: left + client.right, bottom: top + client.bottom },
        window.right - window.left,
        window.bottom - window.top,
    ))
}

/// Fills the band between the control's frame and its client area.
fn paint_band(hwnd: HWND, is_edit: bool) {
    let Some((client, width, height)) = client_in_window(hwnd) else { return };
    let padding = padding_of(hwnd).unwrap_or_default();
    // In the window DC's own (mirrored, when right to left) coordinates,
    // the start is always on the low side.
    let band = RECT {
        left: client.left - padding.start,
        top: client.top - padding.top,
        right: (client.right + padding.end).min(width),
        bottom: (client.bottom + padding.bottom).min(height),
    };
    // SAFETY: `hwnd` is live; the DC is released below.
    let hdc: HDC = unsafe { GetWindowDC(hwnd) };
    if hdc.is_null() {
        return;
    }
    let message = if is_edit { WM_CTLCOLOREDIT } else { WM_CTLCOLORSTATIC };
    // SAFETY: asks the parent for the control's background brush exactly
    // as the control itself does when it paints, with a live DC.
    let brush =
        unsafe { SendMessageW(GetParent(hwnd), message, hdc as WPARAM, hwnd as LPARAM) } as HBRUSH;
    if !brush.is_null() {
        // SAFETY: `hdc` is live; `client` and `band` are valid rectangles.
        unsafe {
            ExcludeClipRect(hdc, client.left, client.top, client.right, client.bottom);
            informational(FillRect(hdc, &raw const band, brush));
        }
    }
    // SAFETY: releases the DC obtained above, once.
    informational(unsafe { ReleaseDC(hwnd, hdc) });
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    is_edit: usize,
) -> LRESULT {
    // SAFETY: forwards the message to the next procedure in the chain, as a
    // subclass procedure must.
    let next = || unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    match message {
        WM_NCCALCSIZE => {
            let result = next();
            if let Some(padding) = padding_of(hwnd) {
                // Either form of the message starts with the proposed
                // window rectangle, which becomes the client rectangle.
                // SAFETY: `lparam` points at that `RECT` for the duration
                // of the message.
                let rect = unsafe { &mut *(lparam as *mut RECT) };
                let (left, right) = if is_rtl(hwnd) {
                    (padding.end, padding.start)
                } else {
                    (padding.start, padding.end)
                };
                rect.left += left;
                rect.top += padding.top;
                rect.right = (rect.right - right).max(rect.left);
                rect.bottom = (rect.bottom - padding.bottom).max(rect.top);
            }
            result
        }
        WM_NCPAINT => {
            let result = next();
            paint_band(hwnd, is_edit != 0);
            result
        }
        // The band is the control's too: a point in it answers as the
        // client's centre would.
        WM_NCHITTEST => {
            let result = next();
            if result != LRESULT::try_from(HTNOWHERE).unwrap_or_default() {
                return result;
            }
            let mut client = RECT::default();
            let mut centre = POINT::default();
            // SAFETY: `hwnd` is live; the out-parameters are valid.
            unsafe {
                GetClientRect(hwnd, &raw mut client);
                centre.x = client.right / 2;
                centre.y = client.bottom / 2;
                ClientToScreen(hwnd, &raw mut centre);
            }
            let packed = (centre.x & 0xffff) | (centre.y & 0xffff) << 16;
            // SAFETY: as `next`, asking about a point inside the client.
            unsafe { DefSubclassProc(hwnd, WM_NCHITTEST, 0, packed as LPARAM) }
        }
        WM_NCDESTROY => {
            forget(hwnd);
            // SAFETY: removes exactly this subclass from its own window
            // during that window's final message, as documented.
            let _ = unsafe { RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID) };
            next()
        }
        _ => next(),
    }
}
