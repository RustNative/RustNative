//! Input-method composition for focus targets that have no native text
//! editing of their own.
//!
//! A native `EDIT` control runs its own IME session end to end and this
//! module never sees it. A *container* made focusable (a canvas, a custom
//! editor) is a window this crate owns, so the IME's messages reach
//! `container_proc`, which hands them here. The component then draws the
//! in-progress text itself.
//!
//! Handling a composition message here means *not* passing it to
//! `DefWindowProcW`: the default handling of `WM_IME_STARTCOMPOSITION` opens
//! the IME's own floating composition window (the component is drawing the
//! text inline instead), and the default handling of a result string in
//! `WM_IME_COMPOSITION` re-sends it as a stream of `WM_IME_CHAR`/`WM_CHAR`
//! that would deliver the committed text a second time as keystrokes. The
//! committed text is instead delivered exactly once, as both a
//! `Composition::Committed` and one `TextInput` carrying the whole string —
//! so a component that only understands `TextInput` still gets IME text.

use rustnative_core::{Composition, Event};
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::UI::Input::Ime::{
    GCS_COMPSTR, GCS_CURSORPOS, GCS_RESULTSTR, IME_COMPOSITION_STRING, ImmGetCompositionStringW,
    ImmGetContext, ImmReleaseContext,
};

use super::super::registry::NativeObject;
use super::super::runtime::Runtime;
use super::super::win32::ignored_by_contract;
use super::focus::focused_node;

/// `WM_IME_STARTCOMPOSITION`.
pub(crate) const WM_IME_STARTCOMPOSITION: u32 = 0x010D;
/// `WM_IME_ENDCOMPOSITION`.
pub(crate) const WM_IME_ENDCOMPOSITION: u32 = 0x010E;
/// `WM_IME_COMPOSITION`.
pub(crate) const WM_IME_COMPOSITION: u32 = 0x010F;

/// One window's composition session state.
#[derive(Debug, Default)]
pub(crate) struct ImeState {
    composing: bool,
    committed: bool,
}

/// Converts a UTF-16 caret offset within `text` into a `char` offset, which
/// is what [`Composition::Updated`] reports (a UTF-16 offset would be
/// meaningless to Rust string code).
pub(crate) fn char_cursor(units: &[u16], utf16_cursor: usize) -> usize {
    char::decode_utf16(units.iter().copied().take(utf16_cursor)).count()
}

/// Reads one composition string (`GCS_COMPSTR` or `GCS_RESULTSTR`) from the
/// window's input context, as UTF-16 units.
fn read_string(hwnd: HWND, which: IME_COMPOSITION_STRING) -> Option<Vec<u16>> {
    // SAFETY: `hwnd` is the live window the IME message was sent to.
    let context = unsafe { ImmGetContext(hwnd) };
    if context.is_null() {
        return None;
    }
    // SAFETY: `context` was just obtained for `hwnd`; a null buffer with a
    // zero length is the documented way to ask for the required byte size.
    let bytes = unsafe { ImmGetCompositionStringW(context, which, std::ptr::null_mut(), 0) };
    let result = usize::try_from(bytes).ok().map(|bytes| {
        let mut units = vec![0u16; bytes / 2];
        if !units.is_empty() {
            // SAFETY: `units` is exactly `bytes` bytes long, as the call
            // above reported is needed, and exclusively borrowed.
            let copied = unsafe {
                ImmGetCompositionStringW(
                    context,
                    which,
                    units.as_mut_ptr().cast(),
                    bytes.try_into().unwrap_or(0),
                )
            };
            units.truncate(usize::try_from(copied).unwrap_or(0) / 2);
        }
        units
    });
    // SAFETY: releases exactly the context acquired above for `hwnd`.
    ignored_by_contract(unsafe { ImmReleaseContext(hwnd, context) });
    result
}

fn read_cursor(hwnd: HWND) -> usize {
    // SAFETY: as in `read_string`.
    let context = unsafe { ImmGetContext(hwnd) };
    if context.is_null() {
        return 0;
    }
    // SAFETY: `GCS_CURSORPOS` returns the caret position as the value
    // itself, with no buffer.
    let position =
        unsafe { ImmGetCompositionStringW(context, GCS_CURSORPOS, std::ptr::null_mut(), 0) };
    // SAFETY: as in `read_string`.
    ignored_by_contract(unsafe { ImmReleaseContext(hwnd, context) });
    usize::try_from(position).unwrap_or(0)
}

/// Handles an IME message sent to `hwnd`, a container this crate owns.
/// Returns whether it was handled (and so must not reach `DefWindowProcW`).
pub(crate) fn handle(runtime: &mut Runtime, hwnd: HWND, message: u32, lparam: LPARAM) -> bool {
    let Some(target) = focused_node(runtime) else {
        return false;
    };
    if runtime.renderer.registry.get(target).map(NativeObject::hwnd) != Some(hwnd)
        && runtime.renderer.registry.get(target).and_then(NativeObject::content_hwnd) != Some(hwnd)
    {
        // Only the focused node's own window composes.
        return false;
    }
    let target = Some(target);
    match message {
        WM_IME_STARTCOMPOSITION => {
            runtime.input.ime = ImeState { composing: true, committed: false };
            runtime
                .dispatch_or_quit(Event::Composition { target, composition: Composition::Started });
            true
        }
        WM_IME_COMPOSITION => {
            // `lParam` carries the `GCS_*` flags saying which strings changed.
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let flags = lparam as u32;
            if flags & GCS_RESULTSTR != 0 {
                if let Some(units) = read_string(hwnd, GCS_RESULTSTR) {
                    let text = String::from_utf16_lossy(&units);
                    runtime.input.ime.committed = true;
                    if runtime.dispatch_or_quit(Event::Composition {
                        target,
                        composition: Composition::Committed { text: text.clone() },
                    }) && !text.is_empty()
                    {
                        runtime.dispatch_or_quit(Event::TextInput { target, text });
                    }
                }
            }
            if flags & GCS_COMPSTR != 0 {
                let units = read_string(hwnd, GCS_COMPSTR).unwrap_or_default();
                let cursor =
                    if flags & GCS_CURSORPOS != 0 { read_cursor(hwnd) } else { units.len() };
                let text = String::from_utf16_lossy(&units);
                runtime.dispatch_or_quit(Event::Composition {
                    target,
                    composition: Composition::Updated { cursor: char_cursor(&units, cursor), text },
                });
            }
            true
        }
        WM_IME_ENDCOMPOSITION => {
            let state = std::mem::take(&mut runtime.input.ime);
            if state.composing && !state.committed {
                runtime.dispatch_or_quit(Event::Composition {
                    target,
                    composition: Composition::Cancelled,
                });
            }
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_utf16_caret_is_reported_in_chars_across_surrogate_pairs() {
        // "a😀b": the emoji is two UTF-16 units but one `char`.
        let units: Vec<u16> = "a\u{1F600}b".encode_utf16().collect();
        assert_eq!(units.len(), 4);
        assert_eq!(char_cursor(&units, 0), 0);
        assert_eq!(char_cursor(&units, 1), 1);
        assert_eq!(char_cursor(&units, 3), 2, "after the whole surrogate pair");
        assert_eq!(char_cursor(&units, 4), 3);
    }
}
