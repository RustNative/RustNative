//! Clipboard *events*: the person's copy/cut/paste shortcuts on the focused
//! node, and system-wide clipboard-change notifications.
//!
//! (Reading and writing the clipboard is a service —
//! `services::clipboard` — not input; this module only uses its synchronous
//! reader to attach the pasted text to a paste event.)

use rustnative_core::{ClipboardAction, Event, KeyCode, KeyModifiers};
use windows_sys::Win32::System::DataExchange::{
    AddClipboardFormatListener, RemoveClipboardFormatListener,
};
use windows_sys::Win32::UI::WindowsAndMessaging::WM_APP;

use super::super::runtime::Runtime;
use super::super::win32::best_effort;
use super::focus::focused_node;
use super::keys::{ClipboardShortcut, clipboard_shortcut};

/// `WM_CLIPBOARDUPDATE`.
pub(crate) const WM_CLIPBOARDUPDATE: u32 = 0x031D;

/// Posted to a top-level window in response to `WM_CLIPBOARDUPDATE`.
///
/// `WM_CLIPBOARDUPDATE` is *sent*, and a message sent from another thread is
/// delivered whenever this thread next waits inside a Win32 call that
/// processes sent messages — including calls made while this window's
/// `Runtime` is borrowed mid-render. Re-posting it moves the actual dispatch
/// to an unnested turn of the message loop.
pub(crate) const WM_FRAMEWORK_CLIPBOARD_CHANGED: u32 = WM_APP + 3;

/// Subscribes the window to clipboard-change notifications.
pub(crate) fn listen(runtime: &Runtime) {
    // SAFETY: `runtime.window` is this runtime's live top-level HWND.
    let listening = unsafe { AddClipboardFormatListener(runtime.window) } != 0;
    best_effort(listening, "AddClipboardFormatListener", "clipboard changes are not reported");
}

/// Unsubscribes; called from `WM_DESTROY`.
pub(crate) fn stop_listening(runtime: &Runtime) {
    // SAFETY: `runtime.window` is still live inside its own `WM_DESTROY`.
    let stopped = unsafe { RemoveClipboardFormatListener(runtime.window) } != 0;
    best_effort(stopped, "RemoveClipboardFormatListener", "the listener ends with the window");
}

/// Reports a clipboard shortcut on the focused node, after the key press
/// itself has been delivered. Native text controls perform the operation
/// themselves; this is the notification (see `ClipboardAction`).
pub(crate) fn shortcut(runtime: &mut Runtime, key: KeyCode, modifiers: KeyModifiers) {
    let Some(shortcut) = clipboard_shortcut(key, modifiers) else {
        return;
    };
    let action = match shortcut {
        ClipboardShortcut::Copy => ClipboardAction::Copy,
        ClipboardShortcut::Cut => ClipboardAction::Cut,
        // A clipboard held open by another process is reported as "no
        // text" rather than failing the key press.
        ClipboardShortcut::Paste => ClipboardAction::Paste {
            text: crate::services::clipboard::read_text_now().ok().flatten(),
        },
    };
    let target = focused_node(runtime);
    runtime.dispatch_or_quit(Event::Clipboard { target, action });
}

/// [`WM_FRAMEWORK_CLIPBOARD_CHANGED`].
pub(crate) fn changed(runtime: &mut Runtime) {
    let window = runtime.window_id;
    runtime.dispatch_or_quit(Event::ClipboardChanged { window });
}
