//! The one place this backend turns a raw Win32 handle or a stored raw
//! pointer back into a live Rust reference.
//!
//! # What this module is for
//!
//! The standards audit's P0.2 finding is not that any single dereference in
//! this crate is obviously wrong — it is that *too many* of them were
//! spread across the backend, each re-deriving the same global lifetime
//! argument in its own `SAFETY` comment:
//!
//! > The issue is that too many invariants are global, informal, and
//! > reentrancy-dependent. [...] Native callback code should be designed so
//! > that incorrect lifetime usage is difficult or impossible to express,
//! > rather than relying on hundreds of local safety comments to maintain
//! > one global invariant.
//!
//! The audit compares three fixes and recommends the third:
//!
//! - **A — more comments.** Rejected: does not shrink the unsafe surface.
//! - **B — `NonNull` newtypes plus centralized access.** "A good
//!   intermediate step."
//! - **C — a native context object callbacks resolve through
//!   (`NativeWindowContext::from_hwnd()`), with all raw-pointer
//!   manipulation in a very small module.** Recommended.
//!
//! This module implements both B and C together: [`HostRef`] is the
//! `NonNull` newtype (B) that replaces `Runtime`'s bare `*mut Application`
//! and `*mut WindowRegistry` back-references, and [`with_runtime`] is
//! the callback-facing resolution step (C) that
//! replaces the "read the slot, null-check it, `unsafe { &mut * }` it"
//! sequence every `WNDPROC` arm used to write out by hand.
//!
//! # The invariant, stated once
//!
//! Everything here rests on a small set of facts about this backend's
//! structure, which is why they can be stated once here instead of at every
//! call site:
//!
//! 1. `WindowsPlatform::run` takes `&mut Application` and does not return
//!    until the message loop has exited, so the `Application` outlives
//!    every `Runtime`, every native window, and every message dispatched
//!    to one.
//! 2. `native::app::run_application` owns the single `WindowRegistry` as a
//!    local, and it likewise outlives the message loop it then runs.
//! 3. Each `Runtime` is boxed and inserted into `WindowRegistry::runtimes`,
//!    and is *never removed while the loop runs* — a window that closes is
//!    only marked `destroyed` (see `WindowRegistry`'s type documentation
//!    for why removing it would be a use-after-free against a caller
//!    further up a reentrant call stack). So a `*mut Runtime` published
//!    into `GWLP_USERDATA` at `WM_NCCREATE` stays valid for the whole loop.
//! 4. The Win32 message loop is single-threaded. Win32 can *reenter* it
//!    (a synchronous `SendMessageW` during `CreateWindowExW`, for example),
//!    but it cannot run two dispatches in parallel.
//!
//! Point 4 is the sharp one, and it is why the resolution functions below
//! are **closure-scoped** rather than returning a `&mut Runtime`. A
//! returned reference could be held live across a nested dispatch that
//! resolves the *same* `Runtime` again, producing two live `&mut` to one
//! object — instant undefined behavior, with nothing in the type system to
//! object. Handing the reference to a closure makes the borrow's extent
//! visible in the source instead: whatever reentrancy happens inside `f`,
//! the borrow provably ends when `f` returns.

use std::marker::PhantomData;
use std::ptr::NonNull;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{GA_PARENT, GA_ROOT, GetAncestor};

use super::runtime::Runtime;
use super::user_data::RuntimeSlot;

/// A non-null, non-owning reference to a value the Win32 message loop
/// borrows for the entire duration of its run — currently the
/// `Application` (owned by `WindowsPlatform::run`'s caller) and the
/// `WindowRegistry` (owned by `native::app::run_application`'s stack
/// frame).
///
/// This exists instead of a bare `*mut T` for three reasons: nullability
/// becomes a type-level fact rather than a runtime check repeated at every
/// use; the `PhantomData<*mut T>` keeps the type correctly invariant in
/// `T` and correctly `!Send`/`!Sync`, so a `Runtime` holding one cannot
/// accidentally be moved to another thread; and every dereference is
/// funnelled through [`HostRef::with`], whose safety contract is stated
/// once, here.
pub(crate) struct HostRef<T> {
    ptr: NonNull<T>,
    /// Makes this type invariant in `T` and neither `Send` nor `Sync`,
    /// matching the aliasing and thread-affinity rules of the `&mut T` it
    /// stands in for.
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
    /// Captures a borrow the message loop will hold for its whole run.
    ///
    /// Taking `&mut T` rather than a raw pointer is deliberate: the caller
    /// must actually *have* the unique borrow it is promising, so this
    /// constructor cannot be reached from a dangling or shared pointer by
    /// accident.
    ///
    /// # Safety
    ///
    /// The referent must outlive every use of this `HostRef` and of every
    /// copy of it — in this crate, that means it must outlive the message
    /// loop, which points 1-3 of the module documentation establish for
    /// both of its two uses. While any copy of this `HostRef` may still be
    /// dereferenced, the caller must not use the original `&mut T` (or
    /// create another `&mut T` to the same value by any other route);
    /// [`HostRef::with`] is the only sanctioned access path.
    pub(crate) unsafe fn new(value: &mut T) -> Self {
        Self { ptr: NonNull::from(value), _owner: PhantomData }
    }

    /// Runs `f` with a unique borrow of the referent.
    ///
    /// # Safety
    ///
    /// No other borrow of the referent may be live for the duration of
    /// `f`. Concretely, `f` must not reach back into a code path that
    /// calls `with` on a `HostRef` to the same value — for the
    /// `Application` and `WindowRegistry` this crate stores, that means a
    /// nested Win32 dispatch inside `f` must not re-enter the same
    /// accessor, which the call sites uphold by keeping the borrow narrow
    /// (read what is needed, end the borrow, then dispatch).
    pub(crate) unsafe fn with<R>(self, f: impl FnOnce(&mut T) -> R) -> R {
        // SAFETY: `ptr` is non-null by construction and, per this
        // function's documented contract plus points 1-3 of the module
        // documentation, still points at a live `T` that nothing else
        // currently borrows.
        f(unsafe { &mut *self.ptr.as_ptr() })
    }
}

/// Resolves `hwnd` — which must be one of this crate's **top-level**
/// windows — to its owning [`Runtime`] and runs `f` against it, returning
/// `None` if the window has no runtime published yet.
///
/// The `None` case is normal, not exceptional: `WM_NCCREATE` is what
/// publishes the pointer (see `message_loop::window_proc`), so the handful
/// of messages Win32 delivers before it legitimately find an empty slot.
///
/// This is the audit's recommended `NativeWindowContext::from_hwnd()` step.
/// Every `WNDPROC` arm that used to read `RuntimeSlot::get`, null-check the
/// result, and write its own `unsafe { &mut *runtime_ptr }` now calls this
/// instead, so the dereference exists in exactly one place.
pub(crate) fn with_runtime<R>(hwnd: HWND, f: impl FnOnce(&mut Runtime) -> R) -> Option<R> {
    // Point 4 below — "the loop is single-threaded" — is only true of calls
    // made *on the loop's thread*. Anything else that can reach a window
    // handle (a COM object called from another apartment, a service on a
    // worker) must not get a `&mut Runtime`, however it came by the handle.
    // Checked rather than assumed since Milestone 26 found COM objects this
    // crate implemented being called on UI Automation's threads (they were
    // agile by default; see `native::uia::provider`).
    match window_thread(hwnd) {
        owner if owner == current_thread() => {}
        // A null or destroyed handle belongs to no thread: nothing to
        // resolve, and nothing wrong with having asked.
        0 => return None,
        _ => {
            debug_assert!(
                false,
                "a runtime was resolved from a thread that does not own its window"
            );
            return None;
        }
    }
    let runtime = RuntimeSlot::get(hwnd);
    if runtime.is_null() {
        return None;
    }
    // SAFETY: `runtime` was just checked non-null, so per `RuntimeSlot`'s
    // invariant it is the pointer `WM_NCCREATE` published for this window:
    // a `Box<Runtime>` in `WindowRegistry::runtimes` that, per point 3 of
    // the module documentation, is never removed while the message loop
    // runs and therefore outlives this call. Point 4 gives exclusivity:
    // the loop is single-threaded, so no other thread holds a borrow, and
    // the borrow handed to `f` ends when `f` returns rather than escaping
    // to overlap a later nested dispatch.
    Some(f(unsafe { &mut *runtime }))
}

/// The id of the thread that created `hwnd`, or `0` for a null or
/// destroyed handle.
fn window_thread(hwnd: HWND) -> u32 {
    // SAFETY: `GetWindowThreadProcessId` accepts any handle value,
    // returning 0 for an invalid one, and a null process-id pointer is
    // documented as "not requested".
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
            hwnd,
            std::ptr::null_mut(),
        )
    }
}

fn current_thread() -> u32 {
    // SAFETY: takes no arguments.
    unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() }
}

/// The top-level ancestor of `hwnd`, or a null handle if there is none.
///
/// Returning the null handle rather than an `Option` matches how the
/// callers here use it: every consumer feeds the result straight into
/// [`with_runtime`]/[`RuntimeSlot::get`], both of which already treat a
/// null handle as "no runtime" — `GetWindowLongPtrW` is documented to
/// accept any handle value, including an invalid one, and simply return
/// `0`.
///
/// "Top-level" means the nearest ancestor (or `hwnd` itself) that is one of
/// this backend's root windows — which, for a tree embedded in a host's
/// window (`EmbeddedRoot`), is a child window, not `GA_ROOT`'s answer (the
/// host's own top-level window, which has no runtime). A window with no
/// such ancestor answers as `GA_ROOT` does.
pub(crate) fn root_window(hwnd: HWND) -> HWND {
    let mut current = hwnd;
    while !current.is_null() {
        if is_framework_root(current) {
            return current;
        }
        // SAFETY: `GetAncestor` accepts any window handle and returns null
        // past the top; it takes no pointer arguments.
        current = unsafe { GetAncestor(current, GA_PARENT) };
    }
    // SAFETY: `GetAncestor` accepts any window handle, including a null or
    // already-destroyed one, and returns null when there is no such
    // ancestor; it takes no pointer arguments.
    unsafe { GetAncestor(hwnd, GA_ROOT) }
}

/// Whether `hwnd` is of this backend's root window class.
fn is_framework_root(hwnd: HWND) -> bool {
    let expected: Vec<u16> = super::WINDOW_CLASS_NAME.encode_utf16().collect();
    let mut name = [0_u16; 64];
    // SAFETY: `name` is a writable buffer of the length passed; the call
    // accepts any window handle and returns 0 for an invalid one.
    let length = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW(hwnd, name.as_mut_ptr(), 64)
    };
    usize::try_from(length).is_ok_and(|length| name.get(..length) == Some(expected.as_slice()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_ref_round_trips_a_borrow_it_was_built_from() {
        let mut value = 41_u32;
        // SAFETY: `value` outlives `host` (both are locals of this test,
        // and `host` is dropped first), and the original `&mut value` is
        // not touched again while `host` is alive.
        let host = unsafe { HostRef::new(&mut value) };
        // SAFETY: no other borrow of `value` is live here.
        unsafe { host.with(|slot| *slot += 1) };
        // SAFETY: as above.
        let observed = unsafe { host.with(|slot| *slot) };
        assert_eq!(observed, 42);
    }

    #[test]
    fn with_runtime_on_a_window_that_never_published_one_is_none() {
        let window = super::super::test_support::TestWindow::new();
        assert!(
            with_runtime(window.hwnd, |_| ()).is_none(),
            "a window whose GWLP_USERDATA was never set must resolve to no runtime"
        );
    }

    #[test]
    fn a_runtime_is_never_resolved_from_a_thread_that_does_not_own_the_window() {
        // A window of the crate's own class, owned by this thread. From any
        // other thread it must never resolve — in a debug build that is a
        // loud programming error, in release a quiet `None`; never a
        // `&mut Runtime` on the wrong thread.
        let window = super::super::test_support::TestWindow::top_level_class();
        let hwnd = window.hwnd as usize;
        let outcome =
            std::thread::spawn(move || with_runtime(hwnd as HWND, |_| ()).is_none()).join();
        if cfg!(debug_assertions) {
            assert!(outcome.is_err(), "a debug build reports the misuse");
        } else {
            assert_eq!(outcome.ok(), Some(true));
        }
    }

    #[test]
    fn with_runtime_on_a_null_handle_is_none_rather_than_a_crash() {
        assert!(with_runtime(std::ptr::null_mut(), |_| ()).is_none());
    }

    #[test]
    fn root_window_of_a_message_only_window_is_itself() {
        let window = super::super::test_support::TestWindow::new();
        assert_eq!(
            root_window(window.hwnd),
            window.hwnd,
            "a message-only window has no further top-level ancestor, so GA_ROOT is itself"
        );
    }
}
