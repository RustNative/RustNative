//! A persistent hidden message-only window whose sole purpose is to give
//! the process's tray notification icon a stable, real `HWND` identity.
//!
//! The prior design called `Shell_NotifyIconW` with `hWnd = NULL`, which
//! Windows happens to still accept but which is not a documented, supported
//! input for the function — every example and every field description in
//! Microsoft's own `NOTIFYICONDATA` reference assumes a real owning window
//! (standards audit P1.4). Creating one real (if invisible) window and
//! keeping it alive for the life of the process, rather than treating
//! "no window" as an acceptable identity, is the fix.
//!
//! This window intentionally has no interactive behavior: it never needs to
//! pump its own message queue (its `WNDPROC` only ever runs
//! `DefWindowProcW`), since this crate does not yet implement click-through
//! handling for notifications (`NIN_SELECT`/`NIN_BALLOONUSERCLICK`) — a
//! real feature this could grow into later without changing the identity
//! model established here.

#[cfg(windows)]
use windows_sys::Win32::Foundation::HWND;

#[cfg(windows)]
use crate::ffi::wide_string;

#[cfg(windows)]
pub(crate) struct NotificationHost {
    pub(crate) hwnd: HWND,
    /// Whether `Shell_NotifyIconW(NIM_ADD, ..)` has already run for
    /// `hwnd`/`uID = 1`; once true, `notify` uses `NIM_MODIFY` instead (see
    /// its own doc comment for why).
    pub(crate) icon_added: bool,
}

#[cfg(windows)]
// SAFETY: `HWND` is an opaque handle value (not a Rust-aliasing-relevant
// pointer to data this process reads/writes through directly). The Win32
// APIs this type's `hwnd` is used with (`Shell_NotifyIconW`) are documented
// to operate correctly when called from a thread other than the one that
// created the window — the thread-affinity Win32 does document is about
// *that window's own message queue* (`GetMessage`/`PeekMessage`/`SendMessage`
// requiring the owning thread to pump it), which this host never needs
// (see its own doc comment: it has no interactive behavior). Every access
// to this type is additionally serialized through the `Mutex` it is stored
// behind in `notification_host` below, so no two threads ever call into
// Shell32 for this icon concurrently even if that were otherwise a concern.
unsafe impl Send for NotificationHost {}

/// Returns the process-wide notification host, creating it on first call.
#[cfg(windows)]
pub(crate) fn notification_host() -> &'static std::sync::Mutex<Result<NotificationHost, String>> {
    static HOST: std::sync::OnceLock<std::sync::Mutex<Result<NotificationHost, String>>> =
        std::sync::OnceLock::new();
    HOST.get_or_init(|| std::sync::Mutex::new(create_notification_host()))
}

#[cfg(windows)]
fn create_notification_host() -> Result<NotificationHost, String> {
    use windows_sys::Win32::Foundation::{GetLastError, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, HWND_MESSAGE, RegisterClassW,
        WNDCLASSW,
    };

    const CLASS_NAME: &str = "NativeRustFrameworkNotificationHost";

    // SAFETY: this host window has no interactive behavior (see
    // `NotificationHost`'s doc comment) — every message it could ever
    // receive is handled correctly by the default window procedure's
    // documented behavior for that message.
    unsafe extern "system" fn notification_host_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // SAFETY: forwarding every message to the default window
        // procedure is always valid; this host implements no custom
        // behavior (see its module-level doc comment).
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    // SAFETY: a null `lpModuleName` is `GetModuleHandleW`'s documented way
    // to retrieve the calling process's own module handle; it takes no
    // other arguments.
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let class_name = wide_string(CLASS_NAME);

    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(notification_host_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: std::ptr::null_mut(),
        hCursor: std::ptr::null_mut(),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_name.as_ptr(),
    };

    // SAFETY: `class` is a fully initialized `WNDCLASSW`, exclusively
    // borrowed for the duration of this call; `class_name` (borrowed by
    // `class.lpszClassName`) outlives this call.
    let atom = unsafe { RegisterClassW(&raw const class) };
    if atom == 0 {
        const ERROR_CLASS_ALREADY_EXISTS: u32 = 1410;
        // SAFETY: `GetLastError` takes no arguments and is called
        // immediately after `RegisterClassW` reported failure, on the same
        // thread, before any other call could overwrite the thread-local
        // error code.
        let error = unsafe { GetLastError() };
        if error != ERROR_CLASS_ALREADY_EXISTS {
            return Err(format!("RegisterClassW failed with error {error}"));
        }
    }

    // SAFETY: `class_name` is a NUL-terminated wide buffer naming the class
    // just registered above; `HWND_MESSAGE` is the documented pseudo-parent
    // for a message-only window, which needs no visible style, position,
    // title, or size; a null `lpParam` is a documented-valid value this
    // window's default procedure never reads; a null return (checked
    // below) is `CreateWindowExW`'s documented failure signal.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            std::ptr::null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };

    if hwnd.is_null() {
        // SAFETY: same reasoning as above.
        let error = unsafe { GetLastError() };
        return Err(format!("CreateWindowExW failed with error {error}"));
    }

    Ok(NotificationHost { hwnd, icon_added: false })
}

/// Copies `text` into a fixed-size `NOTIFYICONDATAW` wide-character field,
/// truncating (and always null-terminating) when it doesn't fit.
#[cfg(windows)]
pub(crate) fn copy_into_wide_buffer(text: &str, buffer: &mut [u16]) {
    if buffer.is_empty() {
        return;
    }
    let encoded: Vec<u16> = text.encode_utf16().collect();
    let usable = (buffer.len() - 1).min(encoded.len());
    buffer[..usable].copy_from_slice(&encoded[..usable]);
    buffer[usable] = 0;
}
