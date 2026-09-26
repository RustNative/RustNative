//! `WM_GETOBJECT` for the system controls this crate creates.
//!
//! `BUTTON`, `EDIT`, and `STATIC` run the system's own window procedures,
//! which answer UI Automation with nothing but their default proxy. A
//! comctl32 subclass puts this crate's procedure in front of theirs for
//! exactly one message; everything else goes straight through
//! `DefSubclassProc`, so the control behaves exactly as before.

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{WM_GETOBJECT, WM_NCDESTROY};

use super::super::win32::best_effort;

/// This crate's subclass id (any value unique among subclasses on the same
/// window works; this spells "UIA").
const SUBCLASS_ID: usize = 0x0055_4941;

/// Installs the `WM_GETOBJECT` hook on a system control. Best effort: a
/// control whose subclass cannot be installed keeps its native proxy.
pub(crate) fn install(hwnd: HWND) {
    // SAFETY: `hwnd` is a live control this thread just created;
    // `subclass_proc` has the signature `SetWindowSubclass` requires and
    // outlives the window (it is a function item).
    let installed = unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0) } != 0;
    best_effort(installed, "SetWindowSubclass", "the control keeps its native accessibility only");
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    let handled = std::panic::catch_unwind(|| {
        if message == WM_GETOBJECT {
            super::get_object(
                hwnd,
                windows::Win32::Foundation::WPARAM(wparam),
                windows::Win32::Foundation::LPARAM(lparam),
            )
            .map(|result| result.0)
        } else {
            None
        }
    });
    if message == WM_NCDESTROY {
        // SAFETY: removes exactly the subclass `install` added, from its own
        // window, during that window's final message, as documented.
        let _ = unsafe { RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID) };
    }
    match handled {
        Ok(Some(result)) => result,
        Ok(None) => {
            // SAFETY: forwards the message unchanged to the next procedure
            // in the subclass chain, as a subclass procedure must.
            unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
        }
        Err(payload) => {
            // The same boundary every other callback in this crate has: the
            // panic must not unwind across `extern "system"`, and the
            // application's `PanicPolicy` decides what happens next.
            let message = super::super::message_loop::panic_payload_message(payload.as_ref());
            super::super::message_loop::poison_runtime_and_quit(
                super::super::user_data::RuntimeSlot::get(super::super::context::root_window(hwnd)),
                message,
            );
            0
        }
    }
}
