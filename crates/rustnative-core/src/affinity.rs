//! Thread affinity, enforced rather than described.
//!
//! Everything that holds the declarative tree — [`crate::Application`],
//! [`crate::ComponentTree`], [`crate::ComponentContext`],
//! [`crate::TaskScope`] — is `!Send`: the type system already refuses to move
//! it to another thread, which is the strongest enforcement Rust offers and
//! the one this crate relies on first. [`UiThread`] extends it to APIs that
//! are not themselves tree-holding types: a function that must run on the
//! UI thread takes a `&UiThread`, which only the thread that claimed it can
//! produce. Where a type must be `Send` for other reasons, a
//! [`ThreadAffinity`] records its home thread and debug builds assert every
//! use is from there (`PLAN.md` Milestone 39).

use std::marker::PhantomData;
use std::thread::ThreadId;

/// Proof that the holder is on the UI thread of the application that
/// issued it. `!Send` and `!Sync`, so it cannot leave that thread.
#[derive(Debug)]
pub struct UiThread {
    home: ThreadId,
    _not_send: PhantomData<*const ()>,
}

impl UiThread {
    /// Claims the current thread as a UI thread. A backend calls this once,
    /// on the thread that runs its message loop, and lends out `&UiThread`
    /// to the code that needs it.
    #[must_use]
    pub fn claim() -> Self {
        Self { home: std::thread::current().id(), _not_send: PhantomData }
    }

    /// The thread this proof belongs to.
    #[must_use]
    pub fn id(&self) -> ThreadId {
        self.home
    }
}

/// A recorded home thread, for a value that is `Send` but whose use is
/// only valid on the thread that created it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadAffinity {
    home: ThreadId,
}

impl Default for ThreadAffinity {
    fn default() -> Self {
        Self::current()
    }
}

impl ThreadAffinity {
    /// Records the current thread as home.
    #[must_use]
    pub fn current() -> Self {
        Self { home: std::thread::current().id() }
    }

    /// Whether the current thread is home.
    #[must_use]
    pub fn is_home(&self) -> bool {
        std::thread::current().id() == self.home
    }

    /// Panics, in debug builds, if called off the home thread. `what` names
    /// the operation for the message.
    ///
    /// # Panics
    ///
    /// In debug builds, when called from any thread but the home one: that
    /// is the bug this exists to catch.
    #[track_caller]
    pub fn debug_assert_home(&self, what: &str) {
        debug_assert!(
            self.is_home(),
            "{what} must be used on the thread that created it ({:?}), not {:?}",
            self.home,
            std::thread::current().id()
        );
    }
}

/// Compile-time evidence that `T` is `!Send`: this function only accepts
/// `T` when the auto-trait machinery reports it is not `Send`. Used by the
/// thread-affinity tests to prove the tree-holding types cannot cross
/// threads; see `tests/affinity.rs`.
pub trait NotSend<Marker> {}
impl<T: ?Sized> NotSend<()> for T {}
impl<T: ?Sized + Send> NotSend<u8> for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affinity_knows_its_home() {
        let affinity = ThreadAffinity::current();
        assert!(affinity.is_home());
        let elsewhere = std::thread::spawn(move || affinity.is_home()).join().unwrap_or(true);
        assert!(!elsewhere);
    }

    #[test]
    #[cfg(debug_assertions)]
    fn a_debug_build_catches_use_off_the_home_thread() {
        let affinity = ThreadAffinity::current();
        let result = std::thread::spawn(move || {
            std::panic::catch_unwind(|| affinity.debug_assert_home("the widget handle")).is_err()
        })
        .join();
        assert_eq!(result.ok(), Some(true));
    }

    #[test]
    fn a_ui_thread_proof_names_its_thread() {
        let ui = UiThread::claim();
        assert_eq!(ui.id(), std::thread::current().id());
    }
}
