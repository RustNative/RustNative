//! What generated bindings call at run time: the handle table, panic
//! containment, the thread check, and string ownership.
//!
//! Every generated `extern "C"` function returns a [`status`] code and
//! writes its result through an out-pointer, so no failure — a panic, a
//! wrong thread, a stale handle, invalid UTF-8 — can cross the boundary as
//! anything but a number, and the message behind it is available from the
//! library's `last_error`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread::ThreadId;

/// The codes every generated function returns.
pub mod status {
    /// Success.
    pub const OK: i32 = 0;
    /// The implementation panicked; the panic was contained.
    pub const PANICKED: i32 = 1;
    /// An owner-affine call from a thread that is not the owner.
    pub const WRONG_THREAD: i32 = 2;
    /// The handle is not (or no longer) an instance.
    pub const INVALID_HANDLE: i32 = 3;
    /// A null pointer where a value was required, or invalid UTF-8.
    pub const INVALID_ARGUMENT: i32 = 4;
    /// The instance is in a call already (an event handler called back
    /// into the instance that raised it).
    pub const BUSY: i32 = 5;
}

/// Which threads may call a method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Affinity {
    /// Only the thread that created the instance.
    Owner,
    /// Any thread.
    Any,
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

/// Records the message the library's `last_error` returns on this thread.
pub fn set_last_error(message: &str) {
    let message = CString::new(message.replace('\0', " ")).unwrap_or_default();
    LAST_ERROR.with(|slot| *slot.borrow_mut() = message);
}

/// This thread's last error message, valid until the next failing call on
/// this thread.
#[must_use]
pub fn last_error() -> *const c_char {
    LAST_ERROR.with(|slot| slot.borrow().as_ptr())
}

/// Runs a generated function's body, containing a panic and recording the
/// message of any failure.
pub fn guard(body: impl FnOnce() -> Result<(), (i32, String)>) -> i32 {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(())) => status::OK,
        Ok(Err((code, message))) => {
            set_last_error(&message);
            code
        }
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "the implementation panicked".to_owned());
            set_last_error(&format!("panicked: {message}"));
            status::PANICKED
        }
    }
}

/// A failure to return from a body passed to [`guard`].
#[must_use]
pub fn fail(code: i32, message: impl Into<String>) -> (i32, String) {
    (code, message.into())
}

/// Borrows a NUL-terminated UTF-8 argument.
///
/// # Safety
///
/// `pointer` is null or points at a NUL-terminated string that stays valid
/// and unmodified for `'a` — the IDL's "borrowed for the call".
///
/// # Errors
///
/// Null, or not UTF-8.
pub unsafe fn borrow_str<'a>(pointer: *const c_char, name: &str) -> Result<&'a str, (i32, String)> {
    if pointer.is_null() {
        return Err(fail(status::INVALID_ARGUMENT, format!("`{name}` is null")));
    }
    // SAFETY: non-null, and NUL-terminated and live per this function's
    // contract.
    unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .map_err(|_| fail(status::INVALID_ARGUMENT, format!("`{name}` is not UTF-8")))
}

/// Hands an owned string to the caller, who frees it with the library's
/// `string_free`. An interior NUL is replaced (C strings cannot hold one).
#[must_use]
pub fn owned_string(value: &str) -> *mut c_char {
    CString::new(value.replace('\0', "\u{FFFD}")).unwrap_or_default().into_raw()
}

/// Frees a string [`owned_string`] returned.
///
/// # Safety
///
/// `pointer` is null or came from [`owned_string`] and was not freed.
pub unsafe fn free_string(pointer: *mut c_char) {
    if !pointer.is_null() {
        // SAFETY: per this function's contract, `pointer` is a
        // `CString::into_raw` result not yet reclaimed.
        drop(unsafe { CString::from_raw(pointer) });
    }
}

/// Writes a result through an out-pointer.
///
/// # Safety
///
/// `out` is null or valid for a write of `T`.
///
/// # Errors
///
/// `out` is null.
pub unsafe fn write<T>(out: *mut T, value: T) -> Result<(), (i32, String)> {
    if out.is_null() {
        return Err(fail(status::INVALID_ARGUMENT, "the result pointer is null"));
    }
    // SAFETY: non-null and valid for a write per this function's contract.
    unsafe { out.write(value) };
    Ok(())
}

struct Slot<T> {
    value: Option<Box<T>>,
    owner: ThreadId,
}

/// Instances the host holds by handle. A call takes the instance out of
/// the table for its duration, so an event handler that calls back into
/// the same instance gets [`status::BUSY`] rather than a deadlock or an
/// aliased `&mut`.
pub struct HandleTable<T> {
    slots: Mutex<BTreeMap<u64, Slot<T>>>,
    next: AtomicU64,
}

impl<T: Send> HandleTable<T> {
    /// An empty table (usable in a `static`).
    #[must_use]
    pub const fn new() -> Self {
        Self { slots: Mutex::new(BTreeMap::new()), next: AtomicU64::new(1) }
    }

    /// Adds an instance owned by the calling thread; returns its handle
    /// (never 0).
    pub fn insert(&self, value: T) -> u64 {
        let handle = self.next.fetch_add(1, Ordering::Relaxed);
        let slot = Slot { value: Some(Box::new(value)), owner: std::thread::current().id() };
        self.slots.lock().unwrap_or_else(PoisonError::into_inner).insert(handle, slot);
        handle
    }

    fn check(slot: &Slot<T>, affinity: Affinity) -> Result<(), (i32, String)> {
        if affinity == Affinity::Owner && slot.owner != std::thread::current().id() {
            return Err(fail(
                status::WRONG_THREAD,
                "called from a thread that does not own the instance",
            ));
        }
        Ok(())
    }

    /// Calls `f` with the instance.
    ///
    /// # Errors
    ///
    /// A stale handle, the wrong thread, or a call already in progress.
    pub fn with<R>(
        &self,
        handle: u64,
        affinity: Affinity,
        f: impl FnOnce(&mut T) -> R,
    ) -> Result<R, (i32, String)> {
        // Put the instance back even if `f` panics.
        struct Restore<'a, T> {
            table: &'a HandleTable<T>,
            handle: u64,
            value: Option<Box<T>>,
        }
        impl<T> Drop for Restore<'_, T> {
            fn drop(&mut self) {
                let mut slots = self.table.slots.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(slot) = slots.get_mut(&self.handle) {
                    slot.value = self.value.take();
                }
            }
        }
        let value = {
            let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
            let slot = slots.get_mut(&handle).ok_or_else(|| {
                fail(status::INVALID_HANDLE, format!("{handle} is not an instance"))
            })?;
            Self::check(slot, affinity)?;
            slot.value
                .take()
                .ok_or_else(|| fail(status::BUSY, "the instance is already in a call"))?
        };
        let mut restore = Restore { table: self, handle, value: Some(value) };
        restore
            .value
            .as_deref_mut()
            .map(f)
            .ok_or_else(|| fail(status::BUSY, "the instance is already in a call"))
    }

    /// Removes and drops an instance.
    ///
    /// # Errors
    ///
    /// A stale handle, the wrong thread, or a call in progress.
    pub fn remove(&self, handle: u64) -> Result<(), (i32, String)> {
        let removed = {
            let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
            let slot = slots.get(&handle).ok_or_else(|| {
                fail(status::INVALID_HANDLE, format!("{handle} is not an instance"))
            })?;
            Self::check(slot, Affinity::Owner)?;
            if slot.value.is_none() {
                return Err(fail(status::BUSY, "the instance is in a call"));
            }
            slots.remove(&handle)
        };
        drop(removed);
        Ok(())
    }
}

impl<T: Send> Default for HandleTable<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// A value only ever touched on the thread that created it.
struct OwnerSlot<T> {
    value: Option<Box<T>>,
    owner: ThreadId,
}

// SAFETY: an `OwnerSlot`'s value is read, written, and dropped only after
// `OwnerTable` has checked that the current thread is `owner` (see
// `OwnerTable::with` and `OwnerTable::remove`); other threads only move the
// `Box` pointer within the map, never the value it points at.
unsafe impl<T> Send for OwnerSlot<T> {}

/// Instances of a service whose every method is `[thread = owner]`: the
/// implementation need not be `Send` (it may hold a component tree, which
/// is not), because nothing but its owner thread ever reaches it.
pub struct OwnerTable<T> {
    slots: Mutex<BTreeMap<u64, OwnerSlot<T>>>,
    next: AtomicU64,
}

impl<T> OwnerTable<T> {
    /// An empty table (usable in a `static`).
    #[must_use]
    pub const fn new() -> Self {
        Self { slots: Mutex::new(BTreeMap::new()), next: AtomicU64::new(1) }
    }

    /// Adds an instance owned by the calling thread; returns its handle.
    pub fn insert(&self, value: T) -> u64 {
        let handle = self.next.fetch_add(1, Ordering::Relaxed);
        let slot = OwnerSlot { value: Some(Box::new(value)), owner: std::thread::current().id() };
        self.slots.lock().unwrap_or_else(PoisonError::into_inner).insert(handle, slot);
        handle
    }

    fn take(&self, handle: u64, removing: bool) -> Result<Box<T>, (i32, String)> {
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        let slot = slots
            .get_mut(&handle)
            .ok_or_else(|| fail(status::INVALID_HANDLE, format!("{handle} is not an instance")))?;
        if slot.owner != std::thread::current().id() {
            return Err(fail(
                status::WRONG_THREAD,
                "called from a thread that does not own the instance",
            ));
        }
        let value =
            slot.value.take().ok_or_else(|| fail(status::BUSY, "the instance is in a call"))?;
        if removing {
            slots.remove(&handle);
        }
        Ok(value)
    }

    /// Calls `f` with the instance, on its owner thread only.
    ///
    /// # Errors
    ///
    /// A stale handle, the wrong thread, or a call already in progress.
    pub fn with<R>(
        &self,
        handle: u64,
        _affinity: Affinity,
        f: impl FnOnce(&mut T) -> R,
    ) -> Result<R, (i32, String)> {
        struct Restore<'a, T> {
            table: &'a OwnerTable<T>,
            handle: u64,
            value: Option<Box<T>>,
        }
        impl<T> Drop for Restore<'_, T> {
            fn drop(&mut self) {
                let mut slots = self.table.slots.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(slot) = slots.get_mut(&self.handle) {
                    slot.value = self.value.take();
                }
            }
        }
        let value = self.take(handle, false)?;
        let mut restore = Restore { table: self, handle, value: Some(value) };
        restore
            .value
            .as_deref_mut()
            .map(f)
            .ok_or_else(|| fail(status::BUSY, "the instance is already in a call"))
    }

    /// Removes and drops an instance, on its owner thread.
    ///
    /// # Errors
    ///
    /// A stale handle, the wrong thread, or a call in progress.
    pub fn remove(&self, handle: u64) -> Result<(), (i32, String)> {
        drop(self.take(handle, true)?);
        Ok(())
    }
}

impl<T> Default for OwnerTable<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reentrant_call_is_busy_and_a_panic_is_contained() {
        let table = HandleTable::new();
        let handle = table.insert(5_u32);
        let inner = table.with(handle, Affinity::Owner, |value| {
            *value += 1;
            table.with(handle, Affinity::Owner, |_| ()).unwrap_err().0
        });
        assert_eq!(inner, Ok(status::BUSY));
        assert_eq!(table.with(handle, Affinity::Any, |value| *value), Ok(6));
        let code = guard(|| table.with(handle, Affinity::Owner, |_| -> () { panic!("boom") }));
        assert_eq!(code, status::PANICKED);
        // SAFETY: `last_error` points at this thread's live message.
        let message = unsafe { CStr::from_ptr(last_error()) }.to_str().unwrap().to_owned();
        assert!(message.contains("boom"), "{message}");
        assert_eq!(
            table.with(handle, Affinity::Owner, |value| *value),
            Ok(6),
            "restored after the panic"
        );
        table.remove(handle).unwrap();
        assert_eq!(
            table.with(handle, Affinity::Owner, |_| ()).unwrap_err().0,
            status::INVALID_HANDLE
        );
    }

    #[test]
    fn owner_affinity_is_checked() {
        let table = std::sync::Arc::new(HandleTable::new());
        let handle = table.insert(1_u32);
        let other = std::sync::Arc::clone(&table);
        let codes = std::thread::spawn(move || {
            (
                other.with(handle, Affinity::Owner, |_| ()).unwrap_err().0,
                other.with(handle, Affinity::Any, |value| *value).unwrap(),
            )
        })
        .join()
        .unwrap();
        assert_eq!(codes, (status::WRONG_THREAD, 1));
    }
}
