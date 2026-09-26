//! Small helpers shared across the native window backend: the current
//! module's `HINSTANCE`, reading a control's text back, and encoding text
//! for Win32's wide-character APIs.

use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::Foundation::{HINSTANCE, HWND};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};

pub(crate) fn module_instance() -> HINSTANCE {
    // SAFETY: a null `lpModuleName` is `GetModuleHandleW`'s documented
    // way to retrieve the calling process's own module handle; it takes
    // no other arguments.
    unsafe { GetModuleHandleW(std::ptr::null()) }
}

pub(crate) fn window_text(hwnd: HWND) -> String {
    // SAFETY: `hwnd` is a live HWND supplied by callers, all of which
    // hold it from this renderer's registry or a Win32 callback
    // parameter; `GetWindowTextLengthW` takes no pointer arguments.
    let length = unsafe { GetWindowTextLengthW(hwnd) };
    if length <= 0 {
        return String::new();
    }

    // `length` was just checked positive above, so this cannot lose a
    // sign; a window's text length also never approaches `usize`'s
    // range on any platform this crate targets.
    #[allow(clippy::cast_sign_loss)]
    let mut buffer = vec![0u16; length as usize + 1];
    // A buffer sized from a real `GetWindowTextLengthW` result plus one
    // NUL terminator never approaches `i32::MAX` UTF-16 code units for
    // any window text a real application would ever set.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let capacity = buffer.len() as i32;
    // SAFETY: `hwnd` is the same live HWND validated above; `buffer` is
    // sized `length + 1` wide chars, matching `GetWindowTextW`'s
    // documented contract of needing room for the text plus a
    // NUL terminator, and `capacity` (== `buffer.len()`) is passed as
    // the exact capacity so the call cannot write past it.
    let copied = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), capacity) };
    // `GetWindowTextW` documents its return value as the number of
    // characters copied, never negative, and never more than `capacity`
    // (== `buffer.len()`) was just given as.
    #[allow(clippy::cast_sign_loss)]
    let copied_len = copied as usize;
    String::from_utf16_lossy(&buffer[..copied_len])
}

pub(crate) fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(once(0)).collect()
}

/// Extracts the low 16 bits of a packed Win32 message parameter — the
/// `LOWORD` macro from `windows.h`. Several window messages this crate
/// handles (`WM_SIZE`, `WM_COMMAND`, `WM_MOUSEWHEEL`, ...) pack two
/// logical 16-bit fields into one pointer-sized `WPARAM`/`LPARAM`, and
/// document that only the low 32 bits are ever meaningful for these
/// specific messages — this helper is only ever called on that
/// documented basis, not as a general-purpose truncation.
pub(crate) fn loword(value: usize) -> u16 {
    #[allow(clippy::cast_possible_truncation)]
    let word = (value & 0xffff) as u16;
    word
}

/// The `HIWORD` macro from `windows.h` — see [`loword`].
pub(crate) fn hiword(value: usize) -> u16 {
    #[allow(clippy::cast_possible_truncation)]
    let word = ((value >> 16) & 0xffff) as u16;
    word
}

/// [`loword`] for a signed `LPARAM`. The sign-to-unsigned reinterpretation
/// this performs is exactly what `windows.h`'s `LOWORD`/`GET_X_LPARAM`-style
/// macros do in C (a plain bit-reinterpretation of the same pointer-sized
/// storage, not a value-preserving numeric conversion) — callers that need
/// a signed logical field back (e.g. `WM_MOVE`'s coordinates, which can be
/// negative on a multi-monitor setup) reinterpret this function's `u16`
/// result as `i16` themselves, exactly as the C macros' typical call sites
/// do.
pub(crate) fn loword_signed(value: isize) -> u16 {
    #[allow(clippy::cast_sign_loss)]
    let bits = value as usize;
    loword(bits)
}

/// [`hiword`] for a signed `LPARAM` — see [`loword_signed`].
pub(crate) fn hiword_signed(value: isize) -> u16 {
    #[allow(clippy::cast_sign_loss)]
    let bits = value as usize;
    hiword(bits)
}
