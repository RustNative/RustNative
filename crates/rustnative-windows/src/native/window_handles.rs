//! A small, thread-safe `WindowId -> HWND` table for top-level windows,
//! kept in sync by `WindowRegistry` as windows are created and destroyed.
//!
//! # Why this exists
//!
//! Every other native window/control handle in this crate is confined to
//! the single Win32 message-loop thread and reached through `Runtime`/
//! `WindowRegistry`, which are plain (non-atomic, non-`Send`) structures —
//! correct, because nothing outside that thread ever needs to see them.
//! Native dialogs are the one exception: `services::dialogs` deliberately
//! runs each `IFileDialog` on its own dedicated STA thread (see
//! `services::run_sta`'s doc comment for why), not the message-loop thread,
//! so giving a dialog a real parent/owner window means resolving a
//! `rustnative_core::WindowId` to a native handle from a *different* thread
//! than the one that owns it.
//!
//! This closes exactly the gap `Audit.md`'s Phase 3 roadmap item 15
//! describes: `FileDialogRequest::owner` (in `rustnative-core`) carries a
//! window identity as far as the platform boundary, and this module is
//! what lets `services::dialogs` turn that identity into a real `HWND` it
//! can pass to `IFileDialog::Show` — without needing the STA thread to
//! reach into `WindowRegistry`/`Runtime` at all, which would reintroduce
//! exactly the kind of cross-thread raw-pointer sharing the rest of this
//! crate's architecture avoids.
//!
//! # Why a raw `isize`, not `HWND` itself
//!
//! An `HWND` is `windows_sys`' typedef for a raw pointer
//! (`*mut c_void`), which is not `Send`/`Sync` by default. The value
//! itself, though, is Win32's own opaque per-process handle table index —
//! copying the bits to another thread and using them only as an *opaque
//! argument* to a documented Win32/COM API (never dereferencing them as a
//! Rust pointer, never reading through them) is exactly what Win32's own
//! handle model supports; many real applications route `HWND`s to worker
//! threads for exactly this "use as an opaque owner/target" purpose (for
//! example, `PostMessageW`, called from a non-owning thread, is already
//! this crate's own waker mechanism — see `runtime.rs`). Storing the
//! handle as a plain `isize` here documents that distinction explicitly:
//! this table is not claiming the handle is safe to dereference or use for
//! anything beyond being handed back to a Win32/COM call that itself
//! accepts a handle value.
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use rustnative_core::WindowId;
use windows_sys::Win32::Foundation::HWND;

fn table() -> &'static Mutex<HashMap<WindowId, isize>> {
    static TABLE: OnceLock<Mutex<HashMap<WindowId, isize>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Records `hwnd` as the current native handle for `window_id`. Called by
/// `WindowRegistry::create_window_once` once a top-level window's `HWND` is
/// known, before any dialog request naming this `window_id` as an owner
/// could plausibly resolve it.
pub(crate) fn set(window_id: WindowId, hwnd: HWND) {
    // A poisoned lock (a panic elsewhere while holding it) would otherwise
    // make every future dialog silently lose owner-window resolution; a
    // dialog missing its owner is a presentation regression, not a
    // correctness one (see `FileDialogRequest::owner`'s doc comment), so
    // recovering the poisoned guard and proceeding is strictly better here
    // than propagating the panic into unrelated window-creation code.
    let mut table = table().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    table.insert(window_id, hwnd as isize);
}

/// Removes `window_id`'s entry, called from `WM_DESTROY` handling once its
/// `HWND` is no longer live. A dialog request racing this removal simply
/// sees no owner for that window id (falls back to `Self::get` returning
/// `None`), which is the same "unowned dialog" fallback used for any
/// `window_id` this table never held in the first place.
pub(crate) fn clear(window_id: WindowId) {
    let mut table = table().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    table.remove(&window_id);
}

/// Resolves `window_id`'s current native handle, if it names a live
/// top-level window this process created. Safe to call from any thread;
/// see the module-level docs for why the returned `isize` is safe to carry
/// across threads even though `HWND` itself is not `Send`.
pub(crate) fn get(window_id: WindowId) -> Option<isize> {
    let table = table().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    table.get(&window_id).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    // `table()` is a single process-wide `static`, and `cargo test` runs
    // test functions from the same binary concurrently on separate threads
    // by default. `rustnative_core::WindowId` has no public constructor
    // other than `PRIMARY`, so every test in this module necessarily reads
    // and writes the *same* key — without serializing them against each
    // other, one test's `clear` can race another's `set`/`get` and produce
    // exactly the same false-failure symptom `measure.rs`'s
    // `GDI_ACCOUNTING_LOCK` documents for the same underlying reason
    // (shared process-global state, concurrent test execution). Held for
    // each test's entire body, not just around individual calls.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn a_window_id_with_no_recorded_handle_resolves_to_none() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        clear(WindowId::PRIMARY);
        assert_eq!(get(WindowId::PRIMARY), None);
    }

    #[test]
    fn set_then_get_round_trips_the_same_raw_handle_value() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let sentinel: HWND = std::ptr::without_provenance_mut(0x1234);
        set(WindowId::PRIMARY, sentinel);
        assert_eq!(get(WindowId::PRIMARY), Some(sentinel as isize));
        clear(WindowId::PRIMARY);
    }

    #[test]
    fn clear_removes_the_entry_rather_than_leaving_a_stale_handle() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let sentinel: HWND = std::ptr::without_provenance_mut(0x5678);
        set(WindowId::PRIMARY, sentinel);
        clear(WindowId::PRIMARY);
        assert_eq!(get(WindowId::PRIMARY), None);
    }
}
