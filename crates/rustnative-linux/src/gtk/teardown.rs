//! What the backend puts back on exit and on a fatal panic (`PLAN.md`
//! Milestone 39's panic and teardown obligation).
//!
//! GTK owns pointer grabs and keyboard grabs per gesture, and releases them
//! when the widget holding them goes away; what the framework itself holds
//! beyond that — an explicit pointer capture, a busy cursor, a composition
//! in progress — is released here, before the loop unwinds.

use std::cell::RefCell;

thread_local! {
    /// Restorations registered by the parts of the backend that change
    /// host state, run once each.
    static RESTORE: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
}

/// Runs every registered restoration, newest first.
pub(crate) fn restore() {
    let pending = RESTORE.with(|list| std::mem::take(&mut *list.borrow_mut()));
    for restore in pending.into_iter().rev() {
        restore();
    }
}
