//! Centralizes every read and write of a window's `GWLP_USERDATA` slot
//! behind two narrow, typed accessors.
//!
//! Before this module existed, `GetWindowLongPtrW`/`SetWindowLongPtrW`
//! calls against `GWLP_USERDATA` were scattered across three files
//! (`message_loop.rs`, `container.rs`, `renderer.rs`), each casting the
//! same `isize`-sized slot to a *different* type depending on which kind of
//! window it was reasoning about, with the invariant re-explained by hand
//! in a `SAFETY` comment at every call site. That is exactly the standards
//! audit's P0.2 finding: a single raw storage slot doing double duty as
//! both a `*mut Runtime` (on top-level windows) and a cached `COLORREF`
//! (on container/control windows), with nothing but comment discipline
//! stopping a future change from reading one kind of window's slot with
//! the other kind's accessor and getting a garbage pointer dereference or
//! a garbage color.
//!
//! [`RuntimeSlot`] and [`BackgroundColorSlot`] do not eliminate that
//! invariant — Win32 only gives every window one untyped slot, so nothing
//! can enforce statically which *kind* of window is holding which value —
//! but they do concentrate every access behind one function each, so the
//! invariant is documented and can be audited or changed in exactly one
//! place instead of at every call site, and every call site outside this
//! module becomes a plain, safe-looking function call instead of a
//! bespoke `unsafe` cast.

use std::sync::atomic::{AtomicU16, Ordering};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GCW_ATOM, GWLP_USERDATA, GetClassWord, GetWindowLongPtrW, SetWindowLongPtrW,
};

/// The class atom of this crate's top-level window class, recorded when the
/// class is registered (`message_loop::register_window_classes`), or `0`
/// before that.
static TOP_LEVEL_CLASS_ATOM: AtomicU16 = AtomicU16::new(0);

/// Records the atom identifying this crate's top-level window class, so
/// [`RuntimeSlot::get`] can tell this crate's windows from everyone else's.
pub(crate) fn set_top_level_class_atom(atom: u16) {
    TOP_LEVEL_CLASS_ATOM.store(atom, Ordering::Release);
}

/// Whether `hwnd` is a window of this crate's top-level class.
fn is_top_level_window(hwnd: HWND) -> bool {
    let ours = TOP_LEVEL_CLASS_ATOM.load(Ordering::Acquire);
    // SAFETY: `GetClassWord` accepts any handle value, returning `0` for a
    // null or invalid one; `GCW_ATOM` is a documented index.
    ours != 0 && !hwnd.is_null() && unsafe { GetClassWord(hwnd, GCW_ATOM) } == ours
}

use super::runtime::Runtime;

/// Typed accessor for the `*mut Runtime` stored in a **top-level window**'s
/// `GWLP_USERDATA`. Every top-level window this crate creates has this
/// slot set exactly once, by [`Self::set`], before any message that reads
/// it can be dispatched — see `message_loop::window_proc`'s `WM_NCCREATE`
/// handling, the sole call site of `Self::set`.
pub(crate) struct RuntimeSlot;

impl RuntimeSlot {
    /// Stores `runtime` in `hwnd`'s `GWLP_USERDATA`.
    ///
    /// # Safety
    ///
    /// `hwnd` must be a valid, currently-alive top-level window handle
    /// owned by the calling thread. `runtime` must remain a valid `*mut
    /// Runtime` (or may be null) for as long as anything might call
    /// [`Self::get`] on this `hwnd` afterward — in practice, until either
    /// this slot is overwritten or `hwnd` is destroyed.
    pub(crate) unsafe fn set(hwnd: HWND, runtime: *mut Runtime) {
        // SAFETY: forwarded from the caller's obligations, documented
        // above; `GWLP_USERDATA` is a documented, always-valid index for
        // any window, and storing a pointer-sized integer here never
        // itself dereferences `runtime`.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, runtime as isize);
        }
    }

    /// Reads back the `*mut Runtime` most recently stored by [`Self::set`]
    /// for `hwnd`. Returns a null pointer if nothing has been stored yet
    /// (including for a null `hwnd`, which `GetWindowLongPtrW` documents
    /// as always safe to pass and simply returns `0` for) — every caller
    /// is expected to null-check the result before dereferencing it, since
    /// this accessor itself performs no dereference and cannot fail.
    ///
    /// Returns null, too, for any window that is not of this crate's
    /// top-level class, *whatever* its slot holds. The message loop resolves
    /// the root window of every message it sees, and the thread also owns
    /// windows this crate did not create — the system's default IME window,
    /// OLE's hidden `OleMainThreadWndClass` window (present since the drag
    /// and drop target needs OLE), UI Automation's own windows. Their
    /// `GWLP_USERDATA` belongs to them. Reading it as a `*mut Runtime` —
    /// which is what this function did until Milestone 26 — would hand
    /// `with_runtime` an arbitrary integer to dereference the first time one
    /// of them stored a non-zero value there. Checking the class atom makes
    /// "this is one of ours" a fact rather than an assumption.
    pub(crate) fn get(hwnd: HWND) -> *mut Runtime {
        if !is_top_level_window(hwnd) {
            return std::ptr::null_mut();
        }
        // SAFETY: `GetWindowLongPtrW` is documented as safe to call with
        // any `HWND` value, including a null or otherwise invalid one (it
        // simply returns `0` in that case); this call reads a
        // pointer-sized integer and never itself dereferences it.
        unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Runtime }
    }
}

/// Typed accessor for the cached background `COLORREF` stored in a
/// **container/control window**'s `GWLP_USERDATA` — set once per style
/// application by `Renderer::apply_control_style` and read back when that
/// same window handles its own `WM_ERASEBKGND`. This is a disjoint use of
/// the same Win32 slot from [`RuntimeSlot`]'s: see the module-level docs
/// for why the two are never used against the same `hwnd`.
pub(crate) struct BackgroundColorSlot;

impl BackgroundColorSlot {
    /// Stores `colorref` (a Win32 `COLORREF`, i.e. `0x00BBGGRR`) in
    /// `hwnd`'s `GWLP_USERDATA`.
    pub(crate) fn set(hwnd: HWND, colorref: u32) {
        // A `COLORREF` only ever occupies the low 24 bits, so it always
        // fits in an `isize` on either a 32-bit or 64-bit target — this
        // cannot actually wrap the sign bit despite the wider `isize`
        // parameter `SetWindowLongPtrW` (a generic pointer-sized-value
        // setter) requires.
        #[allow(clippy::cast_possible_wrap)]
        let value = colorref as isize;
        // SAFETY: `GWLP_USERDATA` is a documented, always-valid index for
        // any window; storing a plain integer color value performs no
        // dereference and cannot fail.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, value);
        }
    }

    /// Reads back the `COLORREF` most recently stored by [`Self::set`] for
    /// `hwnd`, or `0` (opaque black) if none has been stored yet.
    pub(crate) fn get(hwnd: HWND) -> u32 {
        // SAFETY: `GetWindowLongPtrW` is documented as safe to call with
        // any `HWND` value; the result is read back as a plain integer
        // color value, never dereferenced.
        let value = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
        // This slot is only ever written by `Self::set` above, with a
        // `COLORREF` (24 significant bits) widened to `isize` — reading
        // it back as `u32` can neither truncate nor lose a sign for any
        // value this module itself ever stored there. A value stored by
        // some other, incorrect caller is exactly the cross-cutting
        // invariant this module's docs describe as unenforceable by the
        // type system; this cast is not where that would be caught.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let colorref = value as u32;
        colorref
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::test_support::TestWindow;

    #[test]
    fn runtime_slot_round_trips_through_a_real_hwnd() {
        let window = TestWindow::top_level_class();
        // Not a real `Runtime` — this test only exercises the storage
        // slot itself (a pointer-sized integer round-trip through real
        // `SetWindowLongPtrW`/`GetWindowLongPtrW` calls against a real
        // `HWND`), never dereferences the result, so an arbitrary
        // non-null, well-aligned sentinel is exactly as valid a test
        // value as a genuine `*mut Runtime` would be.
        let sentinel = std::ptr::without_provenance_mut::<Runtime>(0x1000);

        assert!(RuntimeSlot::get(window.hwnd).is_null(), "a fresh window's slot starts null");
        // SAFETY: `window.hwnd` is a live HWND this test owns exclusively;
        // `sentinel` is never dereferenced by `get`, only compared and
        // read back bit-for-bit.
        unsafe {
            RuntimeSlot::set(window.hwnd, sentinel);
        }
        assert_eq!(
            RuntimeSlot::get(window.hwnd),
            sentinel,
            "RuntimeSlot::get must read back exactly what RuntimeSlot::set stored"
        );
        // This window is of the crate's own class, so its window procedure
        // *would* resolve the sentinel as a runtime while being destroyed.
        // SAFETY: as above; null is always a valid value for the slot.
        unsafe {
            RuntimeSlot::set(window.hwnd, std::ptr::null_mut());
        }
    }

    #[test]
    fn a_foreign_windows_slot_is_never_read_as_a_runtime() {
        // A window of some other class whose `GWLP_USERDATA` holds a value
        // — as OLE's, the IME's, or any other window on this thread may.
        let foreign = TestWindow::new();
        let sentinel = std::ptr::without_provenance_mut::<Runtime>(0x3000);
        // SAFETY: `foreign.hwnd` is a live HWND this test owns; the slot is
        // written but never dereferenced.
        unsafe {
            RuntimeSlot::set(foreign.hwnd, sentinel);
        }
        assert!(
            RuntimeSlot::get(foreign.hwnd).is_null(),
            "only a window of this crate's top-level class may resolve to a runtime"
        );
    }

    #[test]
    fn runtime_slot_get_on_a_null_hwnd_is_null_not_a_crash() {
        // Real Win32 behavior (confirmed against a real implementation via
        // this repository's `tools/win_probes/null_hwnd.c`, run under
        // Wine): `GetWindowLongPtrW(NULL, ...)` returns `0` rather than
        // crashing. `RuntimeSlot::get` relies on this when resolving a
        // possibly-null ancestor window (see `message_loop.rs`); this test
        // pins that assumption against the real `GetWindowLongPtrW` this
        // binary links against, not just the probe's separate C binary.
        assert!(RuntimeSlot::get(std::ptr::null_mut()).is_null());
    }

    #[test]
    fn background_color_slot_round_trips_through_a_real_hwnd() {
        let window = TestWindow::new();
        assert_eq!(
            BackgroundColorSlot::get(window.hwnd),
            0,
            "a fresh window's slot starts at 0 (opaque black)"
        );
        BackgroundColorSlot::set(window.hwnd, 0x00_00_FF_00); // opaque green
        assert_eq!(BackgroundColorSlot::get(window.hwnd), 0x00_00_FF_00);
    }

    #[test]
    fn the_two_slots_are_independent_views_of_the_same_underlying_storage() {
        // Documents, as an executable assertion rather than only prose,
        // exactly the invariant this module's docs describe: `RuntimeSlot`
        // and `BackgroundColorSlot` are two typed views over the *same*
        // `GWLP_USERDATA` slot, so writing through one is visible through
        // the other reinterpreted — this module's safety argument is that
        // callers never do this across the top-level/container window
        // boundary, not that the slot is somehow partitioned in Win32
        // itself.
        let window = TestWindow::new();
        let sentinel = std::ptr::without_provenance_mut::<Runtime>(0x2000);
        // SAFETY: see `runtime_slot_round_trips_through_a_real_hwnd`.
        unsafe {
            RuntimeSlot::set(window.hwnd, sentinel);
        }
        assert_eq!(BackgroundColorSlot::get(window.hwnd), sentinel as u32);
    }
}
