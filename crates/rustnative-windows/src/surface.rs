//! The public half of the native-surface escape hatch: turning the
//! [`SurfaceId`] an application receives in `Event::SurfaceResized` into a
//! window handle its own renderer can attach to.

use std::num::NonZeroIsize;

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle,
    Win32WindowHandle, WindowHandle,
};
use rustnative_core::SurfaceId;
use windows_sys::Win32::UI::WindowsAndMessaging::{GWLP_HINSTANCE, GetWindowLongPtrW, IsWindow};

/// A native surface's window, in the form every Rust graphics library
/// accepts: it implements [`HasWindowHandle`] and [`HasDisplayHandle`], so
/// it can be handed straight to `wgpu::Instance::create_surface`,
/// `ash-window`, `glutin`, and friends.
///
/// The handle is valid for as long as the surface node exists. A handle
/// kept past that — the node was removed, or its window closed — does not
/// dangle into another window: [`HasWindowHandle::window_handle`] checks
/// the window is still alive and reports [`HandleError::Unavailable`]
/// otherwise. (Windows can reuse a handle value for a new window, so an
/// application should still drop its swapchain when the node goes away.)
///
/// `Send` and `Sync`, so it can be given to a render thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceHandle {
    hwnd: NonZeroIsize,
    hinstance: Option<NonZeroIsize>,
}

impl SurfaceHandle {
    /// The raw `HWND` value, for APIs that take one directly (DXGI's
    /// `CreateSwapChainForHwnd`, for instance).
    #[must_use]
    pub const fn hwnd(&self) -> isize {
        self.hwnd.get()
    }
}

impl HasWindowHandle for SurfaceHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: `IsWindow` accepts any handle value, live or not, and
        // takes no pointers.
        if unsafe { IsWindow(self.hwnd.get() as windows_sys::Win32::Foundation::HWND) } == 0 {
            return Err(HandleError::Unavailable);
        }
        let mut raw = Win32WindowHandle::new(self.hwnd);
        raw.hinstance = self.hinstance;
        // SAFETY: the window was just confirmed alive, and a surface's
        // window lives exactly as long as its node; the borrow is tied to
        // `self`, which the application holds only while it uses it.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(raw)) })
    }
}

impl HasDisplayHandle for SurfaceHandle {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::windows())
    }
}

/// The window behind native surface `surface`, if it still exists.
///
/// Call it on the thread that runs the application — typically from a
/// component's `update` when `Event::SurfaceResized` arrives, which is also
/// where the surface's size is known. Surfaces belong to the UI thread that
/// created them, so this returns `None` on any other thread; hand the
/// resulting [`SurfaceHandle`] to a render thread instead.
///
/// ```no_run
/// use rustnative_core::Event;
///
/// # fn update(event: Event) {
/// if let Event::SurfaceResized { surface, size, .. } = event {
///     if let Some(handle) = rustnative_windows::native_surface(surface) {
///         // e.g. `instance.create_surface(handle)` with wgpu, sized to `size`.
///         let _ = (handle, size);
///     }
/// }
/// # }
/// ```
#[must_use]
pub fn native_surface(surface: SurfaceId) -> Option<SurfaceHandle> {
    let hwnd = crate::native::graphics::surface::lookup(surface)?;
    let hwnd = NonZeroIsize::new(hwnd as isize)?;
    // SAFETY: `hwnd` is a live window this thread created; `GWLP_HINSTANCE`
    // is a documented index.
    let hinstance = unsafe {
        GetWindowLongPtrW(hwnd.get() as windows_sys::Win32::Foundation::HWND, GWLP_HINSTANCE)
    };
    Some(SurfaceHandle { hwnd, hinstance: NonZeroIsize::new(hinstance) })
}
