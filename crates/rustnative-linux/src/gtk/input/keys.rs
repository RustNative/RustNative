//! XKB key translation: key codes and modifier state.
//!
//! GDK hands the backend an XKB keysym (already shaped by the layout and
//! the held Shift) and the hardware keycode. A letter or digit key is
//! reported by its *unshifted* keysym on the active layout, as a Windows
//! virtual key is — so `Ctrl+Shift+S` is `Character('S')` with Shift held,
//! and `Shift+1` is `Character('1')`, not `'!'` — while any other printable
//! key is reported by the character it types.

use gtk::gdk;
use gtk::prelude::*;
use rustnative_core::{KeyCode, KeyModifiers};

/// The portable key for `keyval` (and, for letters and digits, the
/// unshifted keysym of `keycode` in `group`, when the display knows it).
pub(crate) fn key_code(keyval: gdk::Key, unshifted: Option<gdk::Key>) -> KeyCode {
    use gdk::Key;
    let named = match keyval {
        Key::Return | Key::KP_Enter | Key::ISO_Enter => Some(KeyCode::Enter),
        Key::space | Key::KP_Space => Some(KeyCode::Space),
        Key::Tab | Key::ISO_Left_Tab | Key::KP_Tab => Some(KeyCode::Tab),
        Key::Escape => Some(KeyCode::Escape),
        Key::BackSpace => Some(KeyCode::Backspace),
        Key::Left | Key::KP_Left => Some(KeyCode::ArrowLeft),
        Key::Right | Key::KP_Right => Some(KeyCode::ArrowRight),
        Key::Up | Key::KP_Up => Some(KeyCode::ArrowUp),
        Key::Down | Key::KP_Down => Some(KeyCode::ArrowDown),
        Key::Delete | Key::KP_Delete => Some(KeyCode::Delete),
        Key::Insert | Key::KP_Insert => Some(KeyCode::Insert),
        Key::Home | Key::KP_Home => Some(KeyCode::Home),
        Key::End | Key::KP_End => Some(KeyCode::End),
        Key::Page_Up | Key::KP_Page_Up => Some(KeyCode::PageUp),
        Key::Page_Down | Key::KP_Page_Down => Some(KeyCode::PageDown),
        _ => None,
    };
    if let Some(named) = named {
        return named;
    }
    let raw = keyval.into_glib();
    let f1 = Key::F1.into_glib();
    if (f1..f1 + 24).contains(&raw) {
        // F1..F24 are one contiguous run of keysyms.
        return KeyCode::Function(u8::try_from(raw - f1 + 1).unwrap_or(1));
    }
    let base = unshifted.and_then(|key| key.to_unicode());
    if let Some(character) = base.filter(char::is_ascii_alphanumeric) {
        return KeyCode::Character(character.to_ascii_uppercase());
    }
    match keyval.to_unicode().filter(|character| !character.is_control()) {
        Some(character) if character.is_alphabetic() => {
            KeyCode::Character(character.to_uppercase().next().unwrap_or(character))
        }
        Some(character) => KeyCode::Character(character),
        None => KeyCode::Unknown(raw),
    }
}

/// The portable modifiers held in `state`.
pub(crate) fn modifiers(state: gdk::ModifierType) -> KeyModifiers {
    KeyModifiers {
        shift: state.contains(gdk::ModifierType::SHIFT_MASK),
        ctrl: state.contains(gdk::ModifierType::CONTROL_MASK),
        alt: state.contains(gdk::ModifierType::ALT_MASK),
        meta: state.intersects(gdk::ModifierType::SUPER_MASK | gdk::ModifierType::META_MASK),
    }
}

/// The unshifted keysym of `keycode` on `display`'s active layout `group`.
pub(crate) fn unshifted(display: &gdk::Display, keycode: u32, group: u32) -> Option<gdk::Key> {
    let group = i32::try_from(group).unwrap_or(0);
    display
        .translate_key(keycode, gdk::ModifierType::empty(), group)
        .map(|(keyval, _, _, _)| keyval)
}

use gtk::glib::translate::IntoGlib as _;

#[cfg(test)]
mod tests {
    use super::*;
    use gdk::Key;

    #[test]
    fn named_keys_map_to_their_portable_codes() {
        assert_eq!(key_code(Key::Return, None), KeyCode::Enter);
        assert_eq!(key_code(Key::KP_Enter, None), KeyCode::Enter);
        assert_eq!(key_code(Key::ISO_Left_Tab, None), KeyCode::Tab);
        assert_eq!(key_code(Key::Page_Down, None), KeyCode::PageDown);
        assert_eq!(key_code(Key::F1, None), KeyCode::Function(1));
        assert_eq!(key_code(Key::F24, None), KeyCode::Function(24));
    }

    #[test]
    fn letters_and_digits_are_their_unshifted_key() {
        assert_eq!(key_code(Key::S, Some(Key::s)), KeyCode::Character('S'));
        assert_eq!(key_code(Key::s, Some(Key::s)), KeyCode::Character('S'));
        assert_eq!(key_code(Key::exclam, Some(Key::_1)), KeyCode::Character('1'));
        // Other printable keys type what they type.
        assert_eq!(key_code(Key::comma, Some(Key::comma)), KeyCode::Character(','));
        assert_eq!(key_code(Key::Shift_L, None), KeyCode::Unknown(Key::Shift_L.into_glib()));
    }

    #[test]
    fn modifier_masks_map() {
        let held = modifiers(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK);
        assert!(held.ctrl && held.shift && !held.alt && !held.meta);
        assert!(modifiers(gdk::ModifierType::SUPER_MASK).meta);
    }
}
