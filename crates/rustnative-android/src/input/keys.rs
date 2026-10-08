//! Android key codes and meta state in the portable key model.
//!
//! | Portable | Android |
//! |---|---|
//! | `Enter` | `KEYCODE_ENTER`, `KEYCODE_NUMPAD_ENTER` |
//! | `Space`, `Tab`, `Escape` | `KEYCODE_SPACE`, `KEYCODE_TAB`, `KEYCODE_ESCAPE` |
//! | `Backspace` | `KEYCODE_DEL` (Android's "delete" deletes backwards) |
//! | `Delete` | `KEYCODE_FORWARD_DEL` |
//! | arrows | `KEYCODE_DPAD_LEFT/RIGHT/UP/DOWN` |
//! | `Insert`, `Home`, `End`, `PageUp`, `PageDown` | `KEYCODE_INSERT`, `MOVE_HOME`, `MOVE_END`, `PAGE_UP`, `PAGE_DOWN` |
//! | `Function(n)` | `KEYCODE_F1`…`KEYCODE_F12` |
//! | `Character(c)` | letters and digits by key (lower-case, unshifted, so shortcuts match whatever the layout's shift does), else the character the key produces |
//! | `Unknown(code)` | everything else, with Android's key code |

use rustnative_core::{KeyCode, KeyModifiers};

// `android.view.KeyEvent` key codes.
const KEYCODE_0: i32 = 7;
const KEYCODE_9: i32 = 16;
const KEYCODE_DPAD_UP: i32 = 19;
const KEYCODE_DPAD_DOWN: i32 = 20;
const KEYCODE_DPAD_LEFT: i32 = 21;
const KEYCODE_DPAD_RIGHT: i32 = 22;
const KEYCODE_A: i32 = 29;
const KEYCODE_Z: i32 = 54;
const KEYCODE_TAB: i32 = 61;
const KEYCODE_SPACE: i32 = 62;
const KEYCODE_ENTER: i32 = 66;
const KEYCODE_DEL: i32 = 67;
const KEYCODE_PAGE_UP: i32 = 92;
const KEYCODE_PAGE_DOWN: i32 = 93;
const KEYCODE_ESCAPE: i32 = 111;
const KEYCODE_FORWARD_DEL: i32 = 112;
const KEYCODE_MOVE_HOME: i32 = 122;
const KEYCODE_MOVE_END: i32 = 123;
const KEYCODE_INSERT: i32 = 124;
const KEYCODE_F1: i32 = 131;
const KEYCODE_F12: i32 = 142;
const KEYCODE_NUMPAD_ENTER: i32 = 160;

// `android.view.KeyEvent` meta state.
const META_SHIFT_ON: i32 = 0x1;
const META_ALT_ON: i32 = 0x2;
const META_CTRL_ON: i32 = 0x1000;
const META_META_ON: i32 = 0x10000;

/// The portable key for Android's `key_code`, given the character it
/// produces (`KeyEvent.getUnicodeChar`, 0 when none).
pub(crate) fn key_code(key_code: i32, unicode: i32) -> KeyCode {
    match key_code {
        KEYCODE_ENTER | KEYCODE_NUMPAD_ENTER => KeyCode::Enter,
        KEYCODE_SPACE => KeyCode::Space,
        KEYCODE_TAB => KeyCode::Tab,
        KEYCODE_ESCAPE => KeyCode::Escape,
        KEYCODE_DEL => KeyCode::Backspace,
        KEYCODE_FORWARD_DEL => KeyCode::Delete,
        KEYCODE_DPAD_LEFT => KeyCode::ArrowLeft,
        KEYCODE_DPAD_RIGHT => KeyCode::ArrowRight,
        KEYCODE_DPAD_UP => KeyCode::ArrowUp,
        KEYCODE_DPAD_DOWN => KeyCode::ArrowDown,
        KEYCODE_INSERT => KeyCode::Insert,
        KEYCODE_MOVE_HOME => KeyCode::Home,
        KEYCODE_MOVE_END => KeyCode::End,
        KEYCODE_PAGE_UP => KeyCode::PageUp,
        KEYCODE_PAGE_DOWN => KeyCode::PageDown,
        KEYCODE_F1..=KEYCODE_F12 => {
            KeyCode::Function(u8::try_from(key_code - KEYCODE_F1 + 1).unwrap_or(0))
        }
        KEYCODE_A..=KEYCODE_Z => {
            KeyCode::Character(char::from(b'a' + u8::try_from(key_code - KEYCODE_A).unwrap_or(0)))
        }
        KEYCODE_0..=KEYCODE_9 => {
            KeyCode::Character(char::from(b'0' + u8::try_from(key_code - KEYCODE_0).unwrap_or(0)))
        }
        _ => u32::try_from(unicode)
            .ok()
            .filter(|unicode| *unicode != 0)
            .and_then(char::from_u32)
            .filter(|character| !character.is_control())
            .map_or_else(
                || KeyCode::Unknown(u32::try_from(key_code).unwrap_or(0)),
                KeyCode::Character,
            ),
    }
}

/// The portable modifiers in Android's meta state.
pub(crate) const fn modifiers(meta: i32) -> KeyModifiers {
    KeyModifiers {
        shift: meta & META_SHIFT_ON != 0,
        ctrl: meta & META_CTRL_ON != 0,
        alt: meta & META_ALT_ON != 0,
        meta: meta & META_META_ON != 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_table_is_what_the_documentation_says() {
        assert_eq!(key_code(66, 10), KeyCode::Enter);
        assert_eq!(key_code(160, 0), KeyCode::Enter);
        assert_eq!(key_code(67, 0), KeyCode::Backspace);
        assert_eq!(key_code(112, 0), KeyCode::Delete);
        assert_eq!(key_code(21, 0), KeyCode::ArrowLeft);
        assert_eq!(key_code(131, 0), KeyCode::Function(1));
        assert_eq!(key_code(142, 0), KeyCode::Function(12));
        // Shift+S produces 'S', but the key is 's' (shortcuts match keys).
        assert_eq!(key_code(47, i32::from(b'S')), KeyCode::Character('s'));
        assert_eq!(key_code(10, i32::from(b'!')), KeyCode::Character('3'));
        // A key with no letter: its character.
        assert_eq!(key_code(55, i32::from(b',')), KeyCode::Character(','));
        assert_eq!(key_code(3, 0), KeyCode::Unknown(3)); // KEYCODE_HOME
    }

    #[test]
    fn meta_state_maps_to_modifiers() {
        let all = modifiers(0x1 | 0x2 | 0x1000 | 0x10000);
        assert!(all.shift && all.alt && all.ctrl && all.meta);
        assert_eq!(modifiers(0), KeyModifiers::default());
        // The left/right bits come with the general bit; only it is read.
        assert!(modifiers(0x2000 | 0x1000).ctrl);
    }
}
