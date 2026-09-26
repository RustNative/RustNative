//! Putting the host back: `rustnative_core::TeardownPolicy`, on Windows.
//!
//! Run when the message loop ends — normally, or because a component
//! panicked and the panic policy terminates — and verified by a test that
//! panics on purpose with the pointer captured and the cursor clipped.
//!
//! | Restoration | How |
//! |---|---|
//! | `PointerCapture` | `ReleaseCapture` |
//! | `Cursor` | `ClipCursor(NULL)`, and the arrow restored |
//! | `FullScreen` | nothing to undo: this backend has no exclusive mode |
//! | `Ime` | the IME context of the focused window is told to cancel |
//! | `Timers` | every framework timer dies with its window, destroyed in teardown |
//! | `Registrations` | drop targets, clipboard listeners, and the low-memory watcher are revoked as their owners drop |
//! | `PersistedState` | flushed by `Lifecycle::Terminating` (`native::app`) |

use rustnative_core::{Restoration, TeardownPolicy};
use windows_sys::Win32::UI::Input::Ime::{
    CPS_CANCEL, ImmGetContext, ImmNotifyIME, ImmReleaseContext, NI_COMPOSITIONSTR,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetCapture, GetFocus, ReleaseCapture};
use windows_sys::Win32::UI::WindowsAndMessaging::{ClipCursor, IDC_ARROW, LoadCursorW, SetCursor};

use super::win32::ignored_by_contract;

/// The restorations this backend performs.
pub(crate) fn policy() -> TeardownPolicy {
    TeardownPolicy::standard()
}

/// Restores the host state `policy` names. Idempotent: safe to run on a
/// panic and again on the way out.
pub(crate) fn restore(policy: &TeardownPolicy) {
    // SAFETY: every call below takes no pointers (or null, where null is
    // the documented "reset" argument) and is valid from the UI thread.
    unsafe {
        if policy.restores(Restoration::PointerCapture) && !GetCapture().is_null() {
            ignored_by_contract(ReleaseCapture());
        }
        if policy.restores(Restoration::Cursor) {
            ignored_by_contract(ClipCursor(std::ptr::null()));
            let arrow = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
            if !arrow.is_null() {
                SetCursor(arrow);
            }
        }
        if policy.restores(Restoration::Ime) {
            let focused = GetFocus();
            if !focused.is_null() {
                let context = ImmGetContext(focused);
                if !context.is_null() {
                    ignored_by_contract(ImmNotifyIME(context, NI_COMPOSITIONSTR, CPS_CANCEL, 0));
                    ignored_by_contract(ImmReleaseContext(focused, context));
                }
            }
        }
    }
}
