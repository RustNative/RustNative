//! The one place this backend turns the `Application` borrow `run` holds
//! back into a live reference — the GTK counterpart of
//! `rustnative_windows`'s `native::context`, with the same invariant:
//!
//! 1. `LinuxPlatform::run` takes `&mut Application` and does not return
//!    until the main loop has ended, so the `Application` outlives every
//!    window, every widget callback, and every queued work item that runs
//!    while the loop does.
//! 2. The backend is single-threaded: GTK is only ever touched from the
//!    thread that initialized it, and so is this reference.
//! 3. Every use goes through [`HostRef::with`], whose closure scope makes
//!    the borrow's extent visible; the backend's work queue guarantees no
//!    callback re-enters while one is running (`backend::Backend::enter`).
//!
//! What GTK adds over Win32 is that a callback can outlive the loop: a
//! widget's signal closure lives as long as the widget. Unlike the Windows
//! backend, the reference is therefore *withdrawn* when `run` ends
//! (`backend::Backend::detach`), so a callback reached later finds no
//! application rather than a dangling one.

use std::marker::PhantomData;
use std::ptr::NonNull;

/// A non-null, non-owning reference to a value the main loop borrows for
/// its whole run. `!Send` and `!Sync`, like the `&mut T` it stands in for.
pub(crate) struct HostRef<T> {
    ptr: NonNull<T>,
    _owner: PhantomData<*mut T>,
}

impl<T> Clone for HostRef<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for HostRef<T> {}

impl<T> std::fmt::Debug for HostRef<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostRef").field("type", &std::any::type_name::<T>()).finish()
    }
}

impl<T> HostRef<T> {
    /// Captures a borrow the main loop will hold for its whole run.
    ///
    /// # Safety
    ///
    /// The referent must outlive every use of this `HostRef` and its
    /// copies, and the original `&mut T` must not be used while any copy
    /// may still be dereferenced; [`HostRef::with`] is the only access
    /// path.
    pub(crate) unsafe fn new(value: &mut T) -> Self {
        Self { ptr: NonNull::from(value), _owner: PhantomData }
    }

    /// Runs `f` with a unique borrow of the referent.
    ///
    /// # Safety
    ///
    /// No other borrow of the referent may be live for the duration of
    /// `f`, and `f` must not reach a path that calls `with` on another copy
    /// of this reference.
    pub(crate) unsafe fn with<R>(self, f: impl FnOnce(&mut T) -> R) -> R {
        // SAFETY: non-null by construction; live and unaliased per this
        // function's contract and the module's points 1–3.
        f(unsafe { &mut *self.ptr.as_ptr() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_ref_round_trips_a_borrow_it_was_built_from() {
        let mut value = 41_u32;
        // SAFETY: `value` outlives `host`, and is not otherwise touched
        // while `host` is used.
        let host = unsafe { HostRef::new(&mut value) };
        // SAFETY: no other borrow of `value` is live.
        unsafe { host.with(|slot| *slot += 1) };
        // SAFETY: as above.
        assert_eq!(unsafe { host.with(|slot| *slot) }, 42);
    }
}
