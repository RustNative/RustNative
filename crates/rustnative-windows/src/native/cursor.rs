//! `Node::with_cursor`, realized with the system's own cursors.
//!
//! `WM_SETCURSOR` goes first to the window under the pointer and, through
//! `DefWindowProc`, up to its parents — which are this crate's containers
//! and top-level window. There the node under the pointer is found, the
//! nearest declared cursor walking up from it is loaded from the system's
//! set (`IDC_*`), and the message is answered as handled. A node with no
//! declared cursor, anywhere up its chain, gets the default processing, so
//! a text field still shows its I-beam and a button its arrow.

use rustnative_core::Cursor;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    HTCLIENT, IDC_APPSTARTING, IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_HELP, IDC_IBEAM, IDC_NO,
    IDC_SIZEALL, IDC_SIZENS, IDC_SIZEWE, IDC_WAIT, LoadCursorW, SetCursor,
};

use super::runtime::Runtime;

/// The system cursor for `cursor`.
fn system_cursor(cursor: Cursor) -> windows_sys::core::PCWSTR {
    match cursor {
        Cursor::Default => IDC_ARROW,
        Cursor::Pointer => IDC_HAND,
        Cursor::Text => IDC_IBEAM,
        Cursor::Crosshair => IDC_CROSS,
        Cursor::Move => IDC_SIZEALL,
        Cursor::NotAllowed => IDC_NO,
        Cursor::ResizeVertical => IDC_SIZENS,
        Cursor::ResizeHorizontal => IDC_SIZEWE,
        Cursor::Wait => IDC_WAIT,
        Cursor::Progress => IDC_APPSTARTING,
        Cursor::Help => IDC_HELP,
    }
}

/// The cursor declared for the node realized by `under`, or its nearest
/// ancestor that declares one.
pub(crate) fn declared_cursor(runtime: &Runtime, under: HWND) -> Option<Cursor> {
    let snapshot = runtime.renderer.snapshot();
    let mut current = runtime.renderer.registry.id_for_hwnd(under);
    while let Some(id) = current {
        let node = snapshot.get(id)?;
        if let Some(cursor) = node.cursor {
            return Some(cursor);
        }
        current = node.parent;
    }
    None
}

/// Handles `WM_SETCURSOR`: returns `true` if a declared cursor was set.
pub(crate) fn set_cursor(runtime: &Runtime, under: HWND, hit_test: u32) -> bool {
    if hit_test != HTCLIENT {
        return false;
    }
    let Some(cursor) = declared_cursor(runtime, under) else {
        return false;
    };
    // SAFETY: loading a predefined system cursor with a null instance is
    // the documented use; the handle is shared and never destroyed.
    let handle = unsafe { LoadCursorW(std::ptr::null_mut(), system_cursor(cursor)) };
    if handle.is_null() {
        return false;
    }
    // SAFETY: `handle` is a valid cursor.
    unsafe { SetCursor(handle) };
    true
}
