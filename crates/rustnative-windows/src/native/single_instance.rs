//! One running instance per application, and deep links handed from a
//! second launch to the first.
//!
//! # How
//!
//! The first instance creates a named mutex (`Local\RustNative.<app-id>`)
//! and a message-only window whose class name is derived from the same id.
//! A later launch finds the mutex already exists, finds that window, and
//! sends it the URL it was launched with in a `WM_COPYDATA` — the standard
//! cross-process way to hand a window a buffer — then exits. `Local\` scopes
//! the mutex to the user's session, so two people signed in to one machine
//! each get their own instance.
//!
//! # Reentrancy
//!
//! `WM_COPYDATA` is *sent*, from another process, so it can arrive while
//! this thread is in the middle of anything that pumps sent messages. The
//! listener therefore never touches a `Runtime`: it queues the URL and
//! posts [`WM_FRAMEWORK_DEEP_LINK`] to the primary window, which delivers
//! the queued links from its own message handling, on its own terms —
//! the same discipline every other asynchronous source in this backend
//! follows.

use std::cell::RefCell;
use std::collections::VecDeque;

use rustnative_core::WindowId;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowExW, HWND_MESSAGE, PostMessageW,
    RegisterClassW, SW_RESTORE, SendMessageW, SetForegroundWindow, ShowWindow, WM_APP, WM_COPYDATA,
    WNDCLASSW,
};

use super::util::{module_instance, wide};
use super::win32::{best_effort, ignored_by_contract};

/// Posted to the primary window when deep links are waiting.
pub(crate) const WM_FRAMEWORK_DEEP_LINK: u32 = WM_APP + 6;

/// Identifies a deep-link `WM_COPYDATA` among any others the window might
/// receive: "RNDL".
const COPY_DATA_TAG: usize = 0x524E_444C;

thread_local! {
    /// Links received and not yet delivered.
    static PENDING: RefCell<VecDeque<String>> = const { RefCell::new(VecDeque::new()) };
}

/// Queues `url` for the primary window and tells it so.
pub(crate) fn deliver_later(url: String) {
    PENDING.with(|pending| pending.borrow_mut().push_back(url));
    if let Some(primary) = super::window_handles::get(WindowId::PRIMARY) {
        let primary = primary as HWND;
        // SAFETY: `primary` is a live window on this thread (the table
        // only holds live ones); the message carries no pointers.
        let posted = unsafe { PostMessageW(primary, WM_FRAMEWORK_DEEP_LINK, 0, 0) } != 0;
        best_effort(posted, "PostMessageW(deep link)", "the link is delivered with the next one");
    }
}

/// Takes every link waiting for delivery.
pub(crate) fn take_pending() -> Vec<String> {
    PENDING.with(|pending| pending.borrow_mut().drain(..).collect())
}

/// This process's claim to be the one instance of an application.
///
/// Holding it keeps the mutex and the listener window alive; dropping it
/// releases both, after which a new launch becomes the instance.
#[derive(Debug)]
pub(crate) struct Instance {
    mutex: HANDLE,
    listener: HWND,
}

impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: both handles were created by `claim` and are owned here.
        unsafe {
            ignored_by_contract(DestroyWindow(self.listener));
            ignored_by_contract(CloseHandle(self.mutex));
        }
    }
}

/// What claiming the instance found.
#[derive(Debug)]
pub(crate) enum Claim {
    /// This process is the instance.
    First(Instance),
    /// Another process already is; this one should hand over and exit.
    AlreadyRunning,
}

fn listener_class(app_id: &str) -> Vec<u16> {
    wide(format!("RustNative.DeepLink.{app_id}"))
}

/// Claims the single instance of `app_id`, or reports that one is running.
pub(crate) fn claim(app_id: &str) -> Claim {
    let name = wide(format!("Local\\RustNative.{app_id}"));
    // SAFETY: `name` is a NUL-terminated wide string living for the call;
    // null security attributes are the documented default.
    let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    // SAFETY: read immediately after the call it describes, on this thread.
    let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if mutex.is_null() || existed {
        if !mutex.is_null() {
            // SAFETY: a handle this function just opened and will not use.
            ignored_by_contract(unsafe { CloseHandle(mutex) });
        }
        return Claim::AlreadyRunning;
    }
    let class = listener_class(app_id);
    let definition = WNDCLASSW {
        lpfnWndProc: Some(listener_proc),
        hInstance: module_instance(),
        lpszClassName: class.as_ptr(),
        ..WNDCLASSW::default()
    };
    // SAFETY: `definition` and the class name it points to live for the
    // call. Registering a class that already exists (a second claim in one
    // process, as in tests) fails harmlessly and the existing one is used.
    ignored_by_contract(unsafe { RegisterClassW(&raw const definition) });
    // SAFETY: the class was just registered; `HWND_MESSAGE` makes this a
    // message-only window, which is never shown.
    let listener = unsafe {
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
            module_instance(),
            std::ptr::null(),
        )
    };
    best_effort(
        !listener.is_null(),
        "CreateWindowExW(deep-link listener)",
        "a second launch cannot hand over its link",
    );
    Claim::First(Instance { mutex, listener })
}

/// Hands `url` (possibly empty: just "come to the front") to the running
/// instance of `app_id`. Returns whether one was found to hand it to.
pub(crate) fn forward(app_id: &str, url: &str) -> bool {
    let class = listener_class(app_id);
    // SAFETY: `class` lives for the call; searching message-only windows
    // with null parent-after and title is the documented form.
    let listener = unsafe {
        FindWindowExW(HWND_MESSAGE, std::ptr::null_mut(), class.as_ptr(), std::ptr::null())
    };
    if listener.is_null() {
        return false;
    }
    let payload = url.encode_utf16().collect::<Vec<_>>();
    let data = COPYDATASTRUCT {
        dwData: COPY_DATA_TAG,
        cbData: u32::try_from(payload.len() * 2).unwrap_or(u32::MAX),
        lpData: payload.as_ptr().cast_mut().cast(),
    };
    // SAFETY: `data` and the payload it points to live for this synchronous
    // call; Windows marshals them into the receiving process.
    unsafe { SendMessageW(listener, WM_COPYDATA, 0, (&raw const data) as LPARAM) != 0 }
}

unsafe extern "system" fn listener_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message != WM_COPYDATA {
        // SAFETY: exactly what Win32 delivered.
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    // SAFETY: for `WM_COPYDATA`, `lParam` points to a `COPYDATASTRUCT`
    // valid for the duration of this call.
    let data = unsafe { &*(lparam as *const COPYDATASTRUCT) };
    if data.dwData != COPY_DATA_TAG {
        return 0;
    }
    let units = usize::try_from(data.cbData / 2).unwrap_or(0);
    let url = if units == 0 || data.lpData.is_null() {
        String::new()
    } else {
        // SAFETY: the sender described `cbData` bytes at `lpData`, which
        // Windows copied into this process for the duration of the call.
        let slice = unsafe { std::slice::from_raw_parts(data.lpData.cast::<u16>(), units) };
        String::from_utf16_lossy(slice)
    };
    if let Some(primary) = super::window_handles::get(WindowId::PRIMARY) {
        let primary = primary as HWND;
        // A second launch means "show me the application": bring it forward
        // whether or not there is a link. Windows allows it because the
        // sender, the process the person just started, is the foreground.
        // SAFETY: `primary` is a live window on this thread.
        unsafe {
            ignored_by_contract(ShowWindow(primary, SW_RESTORE));
            ignored_by_contract(SetForegroundWindow(primary));
        }
    }
    if !url.is_empty() {
        deliver_later(url);
    }
    1
}

/// The first command-line argument that looks like a URL (`scheme://...`),
/// which is how Windows launches an application for a protocol it handles.
pub(crate) fn launch_url(arguments: impl IntoIterator<Item = String>) -> Option<String> {
    arguments.into_iter().skip(1).find(|argument| {
        argument.split_once("://").is_some_and(|(scheme, _)| {
            !scheme.is_empty()
                && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launch_url_is_the_first_argument_that_is_a_url() {
        let args =
            ["app.exe", "--flag", "C:\\file.txt", "myapp://open/7", "other://x"].map(str::to_owned);
        assert_eq!(launch_url(args).as_deref(), Some("myapp://open/7"));
        assert_eq!(
            launch_url(["app.exe", "C:\\a://b"].map(str::to_owned)),
            None,
            "a path is not a URL"
        );
        assert_eq!(launch_url(["myapp://skipped-program-name"].map(str::to_owned)), None);
    }
}
