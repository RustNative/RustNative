//! Native Win32 clipboard service.

#[cfg(windows)]
use super::run_blocking;
#[cfg(windows)]
use crate::ffi::GlobalFree;

/// Native Win32 clipboard service. Applications opt into it by injecting an
/// `Arc<WindowsClipboard>` into `rustnative_core::Services`.
#[cfg(windows)]
#[derive(Debug, Default)]
pub struct WindowsClipboard;

#[cfg(windows)]
#[async_trait::async_trait]
impl rustnative_core::ClipboardService for WindowsClipboard {
    async fn read_text(&self) -> Result<Option<String>, rustnative_core::ServiceError> {
        run_blocking(read_text_now).await
    }

    async fn write_text(&self, text: String) -> Result<(), rustnative_core::ServiceError> {
        run_blocking(move || write_text_now(&text)).await
    }
}

/// Reads the clipboard's plain text synchronously on the calling thread.
///
/// Shared by [`WindowsClipboard::read_text`] (which runs it on the blocking
/// pool) and the native backend's paste handling (which runs it on the UI
/// thread, inside the key press that asked for it).
#[cfg(windows)]
pub(crate) fn read_text_now() -> Result<Option<String>, rustnative_core::ServiceError> {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, OpenClipboard,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};

    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            // SAFETY: this guard is only ever constructed
            // immediately after `OpenClipboard` above succeeded on
            // this same thread, and is not `Clone`, so exactly one
            // matching `CloseClipboard` runs per successful open.
            unsafe {
                CloseClipboard();
            }
        }
    }

    const CF_UNICODETEXT: u32 = 13;
    // SAFETY: a null owner window is a documented-valid argument to
    // `OpenClipboard` for reads not tied to a specific window.
    if unsafe { OpenClipboard(std::ptr::null_mut()) } == 0 {
        return Err(rustnative_core::ServiceError::new("OpenClipboard failed"));
    }
    let _guard = ClipboardGuard;
    // SAFETY: `CF_UNICODETEXT` is a standard predefined clipboard
    // format; `GetClipboardData` may validly return null (checked
    // below) when no data of that format is present.
    let handle = unsafe { GetClipboardData(CF_UNICODETEXT) };
    if handle.is_null() {
        return Ok(None);
    }
    // SAFETY: `handle` is non-null and was just returned by
    // `GetClipboardData` while the clipboard is open on this
    // thread; `GlobalLock` on a valid `HGLOBAL` either returns a
    // valid pointer or null, which is checked immediately below.
    let value = unsafe { GlobalLock(handle) }.cast::<u16>();
    if value.is_null() {
        return Err(rustnative_core::ServiceError::new("GlobalLock clipboard data failed"));
    }
    let mut length = 0usize;
    // SAFETY: `CF_UNICODETEXT` data is contractually a
    // NUL-terminated UTF-16 buffer; `value` was just validated
    // non-null and remains locked (and therefore stable) for the
    // duration of this loop.
    while unsafe { *value.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: `value` points at `length` contiguous, initialized
    // `u16`s (just walked above) inside memory still locked by the
    // `GlobalLock` call above.
    let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(value, length) });
    // SAFETY: `handle` is the same still-open handle locked above;
    // this pairs that lock exactly once.
    unsafe {
        GlobalUnlock(handle);
    }
    Ok(Some(text))
}

/// Replaces the clipboard's content with `text`, synchronously. See
/// [`read_text_now`].
#[cfg(windows)]
pub(crate) fn write_text_now(text: &str) -> Result<(), rustnative_core::ServiceError> {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
    };

    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            // SAFETY: this guard is only constructed immediately
            // after `OpenClipboard` above succeeded on this same
            // thread, and is not `Clone`, so exactly one matching
            // `CloseClipboard` runs per successful open.
            unsafe {
                CloseClipboard();
            }
        }
    }

    const CF_UNICODETEXT: u32 = 13;
    let mut utf16 = text.encode_utf16().collect::<Vec<_>>();
    utf16.push(0);
    // A write needs a real owner window. `OpenClipboard(NULL)` is valid for
    // *reading*, but Microsoft documents that after opening with a NULL
    // owner, `EmptyClipboard` "sets the clipboard owner to NULL", which
    // "causes SetClipboardData to fail" — and it did: until Milestone 25's
    // native tests wrote the clipboard for real, every
    // `WindowsClipboard::write_text` call returned "SetClipboardData
    // failed". The owner is a throwaway message-only window on this thread,
    // destroyed after the clipboard is closed; the data it placed there is
    // rendered immediately (not delayed), so it outlives its owner.
    let owner = OwnerWindow::create()?;
    // SAFETY: `owner.0` is a live window this thread just created.
    if unsafe { OpenClipboard(owner.0) } == 0 {
        return Err(rustnative_core::ServiceError::new("OpenClipboard failed"));
    }
    let _guard = ClipboardGuard;
    // SAFETY: the clipboard is open on this thread (checked above);
    // `EmptyClipboard` takes no pointer arguments.
    if unsafe { EmptyClipboard() } == 0 {
        return Err(rustnative_core::ServiceError::new("EmptyClipboard failed"));
    }
    // SAFETY: `GMEM_MOVEABLE` with a nonzero byte count is a
    // documented-valid `GlobalAlloc` call; a null return (checked
    // below) is the documented failure signal.
    let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, utf16.len() * std::mem::size_of::<u16>()) };
    if memory.is_null() {
        return Err(rustnative_core::ServiceError::new("GlobalAlloc clipboard data failed"));
    }
    // SAFETY: `memory` was just returned non-null by `GlobalAlloc`
    // above and is still owned by this function (not yet handed to
    // `SetClipboardData`).
    let destination = unsafe { GlobalLock(memory) }.cast::<u16>();
    if destination.is_null() {
        // SAFETY: `memory` is the same handle allocated above and
        // has not been freed or transferred yet, so freeing it here
        // on the lock-failure path is the correct, matching release.
        unsafe {
            GlobalFree(memory);
        }
        return Err(rustnative_core::ServiceError::new("GlobalLock clipboard data failed"));
    }
    // SAFETY: `destination` is a non-null pointer to `utf16.len()`
    // `u16`s of freshly allocated, locked memory (validated above);
    // `utf16` is a distinct source allocation, so the ranges cannot
    // overlap. `GlobalUnlock` pairs the `GlobalLock` immediately
    // above on the same still-valid `memory` handle.
    unsafe {
        std::ptr::copy_nonoverlapping(utf16.as_ptr(), destination, utf16.len());
        GlobalUnlock(memory);
    }
    // SAFETY: `memory` is a valid `GMEM_MOVEABLE` handle populated
    // with `CF_UNICODETEXT`-format data above.
    if unsafe { SetClipboardData(CF_UNICODETEXT, memory) }.is_null() {
        // SAFETY: `SetClipboardData` failed, so ownership of
        // `memory` was never transferred to the system clipboard
        // (per its documented contract) and this function must free
        // it itself, exactly as on every other error path above.
        unsafe {
            GlobalFree(memory);
        }
        return Err(rustnative_core::ServiceError::new("SetClipboardData failed"));
    }
    // On success, `SetClipboardData` has taken ownership of
    // `memory`; the system frees it, and it must NOT be freed here.
    Ok(())
}

/// A message-only window that owns the clipboard for the duration of one
/// write (see [`write_text_now`]), destroyed on drop.
#[cfg(windows)]
struct OwnerWindow(windows_sys::Win32::Foundation::HWND);

#[cfg(windows)]
impl OwnerWindow {
    fn create() -> Result<Self, rustnative_core::ServiceError> {
        use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, HWND_MESSAGE};

        let class: Vec<u16> = "STATIC".encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `STATIC` is a predefined system class; `class` is a
        // NUL-terminated wide string that outlives the call; `HWND_MESSAGE`
        // makes this a message-only window with no on-screen presence; a
        // null return is the documented failure signal, checked below.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return Err(rustnative_core::ServiceError::new("creating the clipboard owner failed"));
        }
        Ok(Self(hwnd))
    }
}

#[cfg(windows)]
impl Drop for OwnerWindow {
    fn drop(&mut self) {
        // SAFETY: `self.0` is the window `create` made on this thread; it is
        // destroyed exactly once, after the clipboard guard has closed the
        // clipboard (locals drop in reverse declaration order).
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(self.0);
        }
    }
}

/// Serializes every test that touches the system clipboard: it is one
/// process-wide (indeed system-wide) resource, and the test harness runs
/// tests concurrently.
#[cfg(all(test, windows))]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(all(test, windows))]
mod tests {
    use rustnative_core::ClipboardService;

    use super::*;

    /// Guards against the NULL-owner write bug documented in
    /// `write_text_now`: a write must be readable back, through both the
    /// synchronous helpers and the async service.
    #[test]
    #[ignore = "needs clipboard access, which a UI-restricted job object denies; CI runs it with --ignored"]
    fn written_text_reads_back() {
        let _clipboard = test_lock();
        let marker = format!("rustnative clipboard round trip {}", std::process::id());
        let mut result = Err(rustnative_core::ServiceError::new("never attempted"));
        for _ in 0..20 {
            // The clipboard is a system-wide resource another process may
            // briefly hold open.
            result = write_text_now(&marker);
            if result.is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        result.expect("writing the clipboard");
        assert_eq!(read_text_now().expect("reading the clipboard"), Some(marker.clone()));

        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let service = WindowsClipboard;
        let async_marker = format!("{marker} (async)");
        runtime.block_on(service.write_text(async_marker.clone())).expect("async write");
        assert_eq!(runtime.block_on(service.read_text()).expect("async read"), Some(async_marker));
    }
}
