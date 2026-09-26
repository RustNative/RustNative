//! Win32 virtual-key translation: key codes, modifier state, and the
//! platform's clipboard shortcuts.

use rustnative_core::{KeyCode, KeyModifiers};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F24,
    VK_HOME, VK_INSERT, VK_LEFT, VK_LWIN, VK_MENU, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_RWIN,
    VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};

pub(crate) fn key_code(vkey: u32) -> KeyCode {
    // `vkey` is documented by every caller (see `message_loop.rs`'s
    // `WM_KEYDOWN` handler) to come from a Win32 virtual-key code, which
    // Microsoft documents as always fitting in a `u16` (in practice,
    // almost always a single byte) — the `u32` parameter type exists only
    // because `WPARAM` is `usize`-sized, not because callers ever pass a
    // value this truncation could actually lose.
    #[allow(clippy::cast_possible_truncation)]
    match vkey as u16 {
        VK_RETURN => KeyCode::Enter,
        VK_SPACE => KeyCode::Space,
        VK_TAB => KeyCode::Tab,
        VK_ESCAPE => KeyCode::Escape,
        VK_BACK => KeyCode::Backspace,
        VK_LEFT => KeyCode::ArrowLeft,
        VK_RIGHT => KeyCode::ArrowRight,
        VK_UP => KeyCode::ArrowUp,
        VK_DOWN => KeyCode::ArrowDown,
        VK_DELETE => KeyCode::Delete,
        VK_INSERT => KeyCode::Insert,
        VK_HOME => KeyCode::Home,
        VK_END => KeyCode::End,
        VK_PRIOR => KeyCode::PageUp,
        VK_NEXT => KeyCode::PageDown,
        value @ VK_F1..=VK_F24 => {
            // `VK_F1..=VK_F24` is a contiguous 24-code range, so the
            // offset is 0..=23 and the function-key number 1..=24.
            #[allow(clippy::cast_possible_truncation)]
            let number = (value - VK_F1 + 1) as u8;
            KeyCode::Function(number)
        }
        value if (0x30..=0x5A).contains(&value) => {
            KeyCode::Character(char::from_u32(u32::from(value)).unwrap_or('?'))
        }
        value => KeyCode::Unknown(u32::from(value)),
    }
}

/// Whether `vkey` is currently held, per this thread's view of the keyboard
/// (the state as of the message being processed, not the physical key).
fn held(vkey: u16) -> bool {
    // SAFETY: `GetKeyState` takes a plain virtual-key-code integer and no
    // pointer arguments; it is always safe to call, from any thread.
    let state = unsafe { GetKeyState(i32::from(vkey)) };
    state & i16::MIN != 0
}

pub(crate) fn modifiers() -> KeyModifiers {
    KeyModifiers {
        shift: held(VK_SHIFT),
        ctrl: held(VK_CONTROL),
        alt: held(VK_MENU),
        meta: held(VK_LWIN) || held(VK_RWIN),
    }
}

/// The clipboard operation a key press performs on Windows, if any: the
/// Ctrl+C/X/V set, plus the IBM CUA equivalents (Ctrl+Insert, Shift+Delete,
/// Shift+Insert) that every native Windows text control also honors.
///
/// Paste is reported without text here; the caller reads the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClipboardShortcut {
    Copy,
    Cut,
    Paste,
}

pub(crate) fn clipboard_shortcut(
    key: KeyCode,
    modifiers: KeyModifiers,
) -> Option<ClipboardShortcut> {
    if modifiers.alt || modifiers.meta {
        return None;
    }
    match (key, modifiers.ctrl, modifiers.shift) {
        (KeyCode::Character('C') | KeyCode::Insert, true, false) => Some(ClipboardShortcut::Copy),
        (KeyCode::Character('X'), true, false) | (KeyCode::Delete, false, true) => {
            Some(ClipboardShortcut::Cut)
        }
        (KeyCode::Character('V'), true, false) | (KeyCode::Insert, false, true) => {
            Some(ClipboardShortcut::Paste)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_and_function_keys_have_portable_names() {
        assert_eq!(key_code(u32::from(VK_HOME)), KeyCode::Home);
        assert_eq!(key_code(u32::from(VK_NEXT)), KeyCode::PageDown);
        assert_eq!(key_code(u32::from(VK_DELETE)), KeyCode::Delete);
        assert_eq!(key_code(u32::from(VK_F1)), KeyCode::Function(1));
        assert_eq!(key_code(u32::from(VK_F24)), KeyCode::Function(24));
        assert_eq!(key_code(0x41), KeyCode::Character('A'));
    }

    #[test]
    fn clipboard_shortcuts_follow_both_windows_conventions() {
        let ctrl = KeyModifiers { ctrl: true, ..KeyModifiers::default() };
        let shift = KeyModifiers { shift: true, ..KeyModifiers::default() };
        assert_eq!(
            clipboard_shortcut(KeyCode::Character('C'), ctrl),
            Some(ClipboardShortcut::Copy)
        );
        assert_eq!(clipboard_shortcut(KeyCode::Insert, ctrl), Some(ClipboardShortcut::Copy));
        assert_eq!(clipboard_shortcut(KeyCode::Character('X'), ctrl), Some(ClipboardShortcut::Cut));
        assert_eq!(clipboard_shortcut(KeyCode::Delete, shift), Some(ClipboardShortcut::Cut));
        assert_eq!(
            clipboard_shortcut(KeyCode::Character('V'), ctrl),
            Some(ClipboardShortcut::Paste)
        );
        assert_eq!(clipboard_shortcut(KeyCode::Insert, shift), Some(ClipboardShortcut::Paste));
        assert_eq!(clipboard_shortcut(KeyCode::Character('V'), KeyModifiers::default()), None);
        let ctrl_alt = KeyModifiers { ctrl: true, alt: true, ..KeyModifiers::default() };
        assert_eq!(
            clipboard_shortcut(KeyCode::Character('V'), ctrl_alt),
            None,
            "Ctrl+Alt is AltGr on many layouts and types characters, not a paste"
        );
    }
}
