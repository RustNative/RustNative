//! Test-only support for creating throwaway real `HWND`s.
//!
//! Every test that needs a live window handle to exercise real Win32
//! behavior (not just data-structure logic) goes through
//! [`message_only_window`]: a **message-only window** (`HWND_MESSAGE` as
//! its parent — see Microsoft's documentation on message-only windows)
//! never has any on-screen presence and needs no display driver, so these
//! tests run the same way whether or not a display is attached — including
//! under Wine's "null" graphics driver, not only under a real display or
//! Xvfb. That matters because it means these tests are meaningfully
//! stronger than the crate's pre-existing single unit test
//! (`platform::tests::capabilities_only_advertise_realized_backend_features`,
//! which doesn't touch `native` at all): they exercise real `HWND`
//! lifetime, real `GetWindowLongPtrW`/`SetWindowLongPtrW` round-trips, and
//! real registry bookkeeping against handles Win32 itself issued, not
//! against hand-constructed test doubles.
//!
//! Each call registers its own uniquely named window class (via an atomic
//! counter) rather than reusing [`super::WINDOW_CLASS_NAME`]/
//! [`super::CONTAINER_CLASS_NAME`]: Rust's test harness runs tests in one
//! process, often concurrently, and `RegisterClassW` fails
//! (`ERROR_CLASS_ALREADY_EXISTS`) on a second registration of the same
//! class name — a per-test unique name sidesteps that instead of requiring
//! tests to coordinate a single shared registration.

use std::sync::atomic::{AtomicU32, Ordering};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_MESSAGE, RegisterClassW,
    WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

use super::util::{module_instance, wide};

static CLASS_COUNTER: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn minimal_wndproc(
    hwnd: HWND,
    message: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    // SAFETY: forwarding every message to the default window procedure is
    // always valid; this test double implements no custom behavior.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// A throwaway, message-only `HWND` that is destroyed when this guard is
/// dropped, so a test cannot leak a window handle even if it panics after
/// creating one (`DestroyWindow` on drop; failure there is intentionally
/// ignored, matching [`super::registry::NativeObject::destroy`]'s own
/// "nothing left to report the failure to" reasoning for shutdown-path
/// cleanup).
pub(crate) struct TestWindow {
    pub(crate) hwnd: HWND,
}

impl TestWindow {
    /// Creates a new message-only window with a freshly registered,
    /// uniquely named window class.
    ///
    /// # Panics
    ///
    /// Panics if class registration or window creation fails — both are
    /// treated as test infrastructure failures, not conditions under test.
    pub(crate) fn new() -> Self {
        Self::create(HWND_MESSAGE, 0)
    }

    /// Creates a visible, ordinary top-level window — for tests that need
    /// a window that can be *activated* (which a message-only window never
    /// can), such as a second window taking activation away from one under
    /// test.
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    pub(crate) fn new_top_level() -> Self {
        Self::create(std::ptr::null_mut(), WS_VISIBLE)
    }

    /// Creates a message-only window of this crate's own top-level class —
    /// for tests of code that must recognize this crate's windows (and only
    /// them). Its `WM_NCCREATE` receives no runtime, so its slot starts
    /// null.
    ///
    /// # Panics
    ///
    /// As [`Self::new`], or if the crate's window classes cannot be
    /// registered.
    pub(crate) fn top_level_class() -> Self {
        super::message_loop::register_window_classes(module_instance())
            .expect("registering this backend's window classes");
        let class_name = wide(super::WINDOW_CLASS_NAME);
        // SAFETY: the class was registered just above; `class_name`
        // outlives the call; `HWND_MESSAGE` makes a message-only window; a
        // null `lpParam` is what `window_proc`'s `WM_NCCREATE` handles as
        // "no runtime".
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                std::ptr::null(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                module_instance(),
                std::ptr::null_mut(),
            )
        };
        assert!(!hwnd.is_null(), "CreateWindowExW failed for a top-level-class test window");
        Self { hwnd }
    }

    fn create(parent: HWND, extra_style: u32) -> Self {
        let class_name = wide(format!(
            "framework-test-window-{}",
            CLASS_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let instance = module_instance();
        let class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(minimal_wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        // SAFETY: `class` is a fully initialized `WNDCLASSW` with a
        // process-unique `lpszClassName` (via `CLASS_COUNTER`), so this
        // cannot collide with any other registration in this test binary.
        let atom = unsafe { RegisterClassW(&raw const class) };
        assert_ne!(atom, 0, "RegisterClassW failed for a test-only window class");

        // SAFETY: `class_name` was just registered above and outlives
        // this call; `HWND_MESSAGE` is Win32's documented sentinel parent
        // for a message-only window, which requires no display driver.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                std::ptr::null(),
                WS_OVERLAPPEDWINDOW | extra_style,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                parent,
                std::ptr::null_mut(),
                instance,
                std::ptr::null_mut(),
            )
        };
        assert!(!hwnd.is_null(), "CreateWindowExW failed for a test-only message-only window");
        Self { hwnd }
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        // SAFETY: `self.hwnd` was created by `Self::new` above and is not
        // shared with anything else; destroying it on drop is exactly the
        // ordinary RAII cleanup every other native window owner in this
        // crate performs.
        unsafe {
            DestroyWindow(self.hwnd);
        }
    }
}
