//! `Lifecycle::LowMemory`, as Windows reports it.
//!
//! A desktop process gets no low-memory *message*; what Windows offers is a
//! memory resource notification object, signalled while available physical
//! memory is low. A watcher thread waits on it and posts
//! [`WM_FRAMEWORK_LOW_MEMORY`] to the primary window, which delivers the
//! lifecycle event on the UI thread (flushing state first, like every
//! lifecycle point). The object stays signalled for as long as memory is
//! low, so after reporting once the watcher waits for the *high*-memory
//! object before it will report again — one event per episode, not a
//! stream.

use std::thread::JoinHandle;

use rustnative_core::WindowId;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND, WAIT_OBJECT_0};
use windows_sys::Win32::System::Memory::{
    CreateMemoryResourceNotification, HighMemoryResourceNotification, LowMemoryResourceNotification,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use super::win32::{best_effort, ignored_by_contract};

/// Posted to the primary window when available memory becomes low.
pub(crate) const WM_FRAMEWORK_LOW_MEMORY: u32 = WM_APP + 7;

/// A handle the watcher thread may use: an opaque kernel object value,
/// carried across threads only to be handed back to a Win32 call.
#[derive(Clone, Copy)]
struct SendHandle(isize);

/// The running watcher; dropping it stops the thread.
pub(crate) struct MemoryWatcher {
    stop: isize,
    thread: Option<JoinHandle<()>>,
}

impl MemoryWatcher {
    /// Starts watching, or returns `None` if the notification objects could
    /// not be created — in which case the application simply never hears
    /// `LowMemory`, which is the honest answer on a host that cannot say.
    pub(crate) fn start() -> Option<Self> {
        // SAFETY: plain object creation; each handle is checked for null
        // and owned by the thread below, which closes them.
        let low = unsafe { CreateMemoryResourceNotification(LowMemoryResourceNotification) };
        // SAFETY: as above.
        let high = unsafe { CreateMemoryResourceNotification(HighMemoryResourceNotification) };
        // SAFETY: a manual-reset, unsignalled, unnamed event.
        let stop = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if low.is_null() || high.is_null() || stop.is_null() {
            for handle in [low, high, stop] {
                if !handle.is_null() {
                    // SAFETY: a handle created above and not yet shared.
                    ignored_by_contract(unsafe { CloseHandle(handle) });
                }
            }
            return None;
        }
        let (low, high, stop_for_thread) =
            (SendHandle(low as isize), SendHandle(high as isize), SendHandle(stop as isize));
        let thread = std::thread::Builder::new()
            .name("framework-low-memory".into())
            .spawn(move || watch(low, high, stop_for_thread))
            .ok()?;
        Some(Self { stop: stop as isize, thread: Some(thread) })
    }
}

fn watch(low: SendHandle, high: SendHandle, stop: SendHandle) {
    let (low, high, stop) = (low.0 as HANDLE, high.0 as HANDLE, stop.0 as HANDLE);
    loop {
        if !wait_for_either(stop, low) {
            break;
        }
        if let Some(primary) = super::window_handles::get(WindowId::PRIMARY) {
            // SAFETY: posting to a window from another thread is what
            // `PostMessageW` is for; the handle is only passed through.
            let posted =
                unsafe { PostMessageW(primary as HWND, WM_FRAMEWORK_LOW_MEMORY, 0, 0) } != 0;
            best_effort(posted, "PostMessageW(low memory)", "this episode goes unreported");
        }
        // One report per episode: wait until memory is plentiful again.
        if !wait_for_either(stop, high) {
            break;
        }
    }
    for handle in [low, high] {
        // SAFETY: this thread owns both notification handles.
        ignored_by_contract(unsafe { CloseHandle(handle) });
    }
}

/// Waits for `stop` or `signal`; returns whether it was `signal`.
fn wait_for_either(stop: HANDLE, signal: HANDLE) -> bool {
    let handles = [stop, signal];
    // SAFETY: both handles are live for the duration of the wait.
    let result = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
    result == WAIT_OBJECT_0 + 1
}

impl Drop for MemoryWatcher {
    fn drop(&mut self) {
        let stop = self.stop as HANDLE;
        // SAFETY: `stop` is the event this value owns.
        best_effort(
            unsafe { SetEvent(stop) } != 0,
            "SetEvent(stop watcher)",
            "the thread is leaked",
        );
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        // SAFETY: the thread has exited; nothing else holds the event.
        ignored_by_contract(unsafe { CloseHandle(stop) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_watcher_starts_and_stops_cleanly() {
        let watcher = MemoryWatcher::start().expect("memory notification objects are available");
        drop(watcher);
    }
}
