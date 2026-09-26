//! Explicit classification of Win32 return values.
//!
//! Almost every Win32 function returns something, and almost none of those
//! returns mean the same thing. Before this module existed, this backend
//! made the "does this return value matter?" decision *implicitly*, by
//! either writing `if unsafe { Foo(..) } == 0 { return Err(..) }` or by
//! discarding the value with no statement of why — and a discarded value
//! reads identically whether the author decided the failure is
//! unreportable, decided the return isn't a status code at all, or simply
//! never considered it. That ambiguity is the standards audit's P2.31
//! finding ("several Win32 return values are ignored ... each ignored
//! result should be deliberate").
//!
//! The four functions here are the vocabulary that makes the decision
//! explicit at the call site. They are deliberately *not* a safety
//! abstraction — every one of them takes an already-evaluated value, so
//! the `unsafe` call itself still stands, with its own `SAFETY` comment,
//! next to the classification.
//!
//! | Wrapper | Meaning |
//! |---|---|
//! | [`must_succeed`] | Failure is a real error the caller must propagate. |
//! | [`best_effort`] | Failure is survivable; the UI degrades but stays correct. |
//! | [`informational`] | The return is data (a previous value, a count), not a status. |
//! | [`ignored_by_contract`] | Microsoft documents the return as meaningless in this call's context. |

use crate::Error;

/// The call must succeed: a failure is a genuine error and is converted
/// into [`Error::windows_api`] (which snapshots `GetLastError` on the
/// calling thread, so this must be invoked immediately after the failing
/// call, before anything else can overwrite the thread-local code).
///
/// `succeeded` is the caller's own reading of the API's documented success
/// condition — `!handle.is_null()`, `result != 0`, `hresult >= 0`, and so
/// on — because that condition differs per function and is not something
/// this helper can infer from a bare integer.
pub(crate) fn must_succeed(succeeded: bool, operation: &'static str) -> Result<(), Error> {
    if succeeded { Ok(()) } else { Err(Error::windows_api(operation)) }
}

/// The call is expected to succeed, but a failure is survivable: the UI
/// degrades (a control does not repaint, a style is not applied) without
/// becoming *incorrect*, and there is a good reason not to fail the whole
/// operation around it.
///
/// A failure here is recorded as a `debug_assert!` rather than silently
/// dropped, so a development or test build surfaces it — including the
/// native integration tests in `tests/`, which run in debug — while a
/// release build keeps the degraded-but-working behavior the
/// classification promises. `reason` documents why this particular call is
/// survivable, and is what a reader sees in the assertion message.
#[track_caller]
pub(crate) fn best_effort(succeeded: bool, operation: &'static str, reason: &'static str) {
    debug_assert!(succeeded, "Win32 {operation} failed on a best-effort path ({reason})");
}

/// The return value is *data*, not a status code — a previously set value,
/// a count, a handle the caller does not adopt — and this call site has no
/// use for it.
///
/// Naming it is what distinguishes "read and deliberately unused" from
/// "never looked at". Common examples in this crate are
/// `SetWindowLongPtrW` (returns the *previous* value) and `SendMessageW`
/// for messages documented to return nothing meaningful.
#[inline]
pub(crate) fn informational<T>(value: T) {
    drop(value);
}

/// Microsoft's documentation states the return value carries no meaning
/// for this call's specific context, so there is nothing to check.
///
/// The canonical example is `ShowWindow`, whose return reports the
/// window's *previous* visibility rather than whether the call worked, so
/// treating it as a status code would be actively wrong.
#[inline]
pub(crate) fn ignored_by_contract<T>(value: T) {
    drop(value);
}
