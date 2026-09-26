//! The window procedure for `Column`/`Row` container windows: forwards
//! control notifications up to the owning top-level window and paints its
//! own background.

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{FillRect, HDC};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetClientRect, SendMessageW, WM_COMMAND, WM_CTLCOLORBTN, WM_CTLCOLOREDIT,
    WM_CTLCOLORSTATIC, WM_ERASEBKGND, WM_GETOBJECT, WM_HSCROLL, WM_NCDESTROY, WM_NOTIFY, WM_SIZE,
};

use super::context::{root_window, with_runtime};
use super::input::ime;
use super::message_loop::{panic_payload_message, poison_runtime_and_quit};
use super::rendering::styling::background_brush_for;
use super::user_data::RuntimeSlot;
use super::win32::best_effort;

pub(crate) unsafe extern "system" fn container_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    container_wndproc_boundary(hwnd, move || container_proc_impl(hwnd, message, wparam, lparam))
}

/// Same purpose as `message_loop::wndproc_boundary`, for container windows.
/// A container's own `GWLP_USERDATA` holds a cached background color (see
/// `WM_ERASEBKGND` in `container_proc_impl`), not a `*mut Runtime` — see
/// `user_data`'s module docs — so the owning `Runtime` is resolved through
/// the top-level ancestor window instead, via [`RuntimeSlot::get`] — the
/// same lookup `WM_COMMAND`/`WM_CTLCOLOR*` forwarding already relies on
/// below.
fn container_wndproc_boundary<F>(hwnd: HWND, f: F) -> LRESULT
where
    F: FnOnce() -> LRESULT + std::panic::UnwindSafe,
{
    match std::panic::catch_unwind(f) {
        Ok(result) => result,
        Err(payload) => {
            let message = panic_payload_message(payload.as_ref());
            poison_runtime_and_quit(RuntimeSlot::get(root_window(hwnd)), message);
            0
        }
    }
}

fn container_proc_impl(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: `hwnd`/`message`/`wparam`/`lparam` are exactly what Win32
    // just delivered this callback with; `DefWindowProcW`'s documented
    // default handling is valid for any window message.
    let default = || unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };

    match message {
        WM_COMMAND | WM_NOTIFY | WM_HSCROLL | WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT
        | WM_CTLCOLORBTN => {
            // These are all sent to a control's *immediate* parent. A
            // container is frequently just one link in a chain of nested
            // containers, so forward up to the top-level window, whose
            // `window_proc` owns the `Runtime` these need.
            let root = root_window(hwnd);
            if root.is_null() {
                return default();
            }
            // SAFETY: `root` was just checked non-null and, per
            // `GetAncestor`'s contract, is a live top-level HWND;
            // `message`/`wparam`/`lparam` are forwarded unchanged from what
            // Win32 delivered to this callback, which `window_proc`'s
            // handlers for these same message values already expect in that
            // shape.
            unsafe { SendMessageW(root, message, wparam, lparam) }
        }
        windows_sys::Win32::UI::WindowsAndMessaging::WM_SETCURSOR => {
            // The hit-test code is the low word of `lParam`.
            let hit_test = u32::try_from(lparam & 0xFFFF).unwrap_or(0);
            let set = with_runtime(root_window(hwnd), |runtime| {
                super::cursor::set_cursor(runtime, wparam as HWND, hit_test)
            })
            .unwrap_or(false);
            if set {
                1
            } else {
                // SAFETY: exactly what Win32 delivered this callback with.
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
        }
        WM_GETOBJECT => super::uia::get_object(
            hwnd,
            windows::Win32::Foundation::WPARAM(wparam),
            windows::Win32::Foundation::LPARAM(lparam),
        )
        .map_or_else(default, |result| result.0),
        ime::WM_IME_STARTCOMPOSITION | ime::WM_IME_COMPOSITION | ime::WM_IME_ENDCOMPOSITION => {
            // A focusable container composes IME text itself; see
            // `native::input::ime`.
            let handled = with_runtime(root_window(hwnd), |runtime| {
                ime::handle(runtime, hwnd, message, lparam)
            })
            .unwrap_or(false);
            if handled { 0 } else { default() }
        }
        WM_ERASEBKGND => {
            // A container paints its own background directly, because —
            // unlike the messages above — this one is sent to the container
            // itself, not to a parent that could resolve a `Runtime` and
            // read the renderer's own style cache.
            //
            // The brush comes from the process-wide cache in
            // `native::styling` rather than being created and destroyed per
            // message. `WM_ERASEBKGND` arrives on every repaint — during a
            // drag-resize, many times a second, for every container in the
            // tree — and an earlier revision called
            // `CreateSolidBrush`/`DeleteObject` on each one, which is the
            // standards audit's P2.30 finding ("unnecessary churn exists
            // because the renderer already maintains style resources").
            // Caching by color also means repeated erases of the same
            // container reuse one GDI object instead of cycling through the
            // process's GDI handle quota.
            let brush = background_brush_for(hwnd);
            if brush.is_null() {
                // GDI handle exhaustion; let Win32's own class-brush erase
                // stand rather than filling with an invalid brush.
                return default();
            }
            let hdc = wparam as HDC;
            let mut client = RECT::default();
            // SAFETY: `hwnd` is a live HWND owned by this callback;
            // `client` is a valid, exclusively borrowed `RECT` for
            // `GetClientRect` to write into.
            let measured = unsafe { GetClientRect(hwnd, &raw mut client) } != 0;
            if !measured {
                // Nothing to fill if the client rect could not be read;
                // fall back to the default erase rather than filling a
                // zero or garbage rectangle.
                best_effort(measured, "GetClientRect", "the default erase still paints");
                return default();
            }
            // SAFETY: `hdc` is the `HDC` Win32 passed via `wparam` for this
            // erase-background message, valid for the duration of this
            // callback; `client` was just filled in by the successful
            // `GetClientRect` above; `brush` is a live, cache-owned
            // `HBRUSH` this call only reads (see `native::styling` for its
            // ownership).
            let filled = unsafe { FillRect(hdc, &raw const client, brush) } != 0;
            best_effort(filled, "FillRect", "the container is left unpainted for one frame");
            super::rendering::shape::paint_border(hwnd, hdc, &client);
            // Non-zero tells Win32 the background is erased and it must not
            // erase it again with the class brush.
            1
        }
        WM_SIZE => {
            // A rounded container's region follows its size.
            super::rendering::shape::apply_region(hwnd);
            default()
        }
        WM_NCDESTROY => {
            super::rendering::shape::forget(hwnd);
            default()
        }
        _ => default(),
    }
}
