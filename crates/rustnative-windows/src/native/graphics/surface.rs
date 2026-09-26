//! Native surfaces: bare child windows an application renders into itself.
//!
//! The framework creates the window, positions and sizes it with layout,
//! and otherwise leaves it alone — no painting, no background erase — so a
//! swapchain attached to it owns every pixel. The window is identified to
//! the application by a [`SurfaceId`], handed out in
//! `Event::SurfaceResized`, which [`crate::native_surface`] turns back into
//! a handle. The lookup goes through a per-thread table rather than through
//! the window's `Runtime`, so it works from inside a component's `update`,
//! where the runtime is already borrowed.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use rustnative_core::SurfaceId;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ValidateRect;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, WM_ERASEBKGND, WM_GETOBJECT, WM_NCDESTROY, WM_PAINT,
};

/// The registered class name for native-surface windows.
pub(crate) const SURFACE_CLASS_NAME: &str = "RustNativeFrameworkSurface";

thread_local! {
    static SURFACES: RefCell<HashMap<SurfaceId, HWND>> = RefCell::new(HashMap::new());
    static NEXT: Cell<u64> = const { Cell::new(1) };
}

/// Registers a newly created surface window, returning its identifier.
pub(crate) fn register(hwnd: HWND) -> SurfaceId {
    let id = NEXT.with(|next| {
        let id = next.get();
        next.set(id.saturating_add(1));
        SurfaceId::from_raw(id)
    });
    SURFACES.with(|surfaces| surfaces.borrow_mut().insert(id, hwnd));
    id
}

/// The window for `id`, if the surface still exists on this thread.
pub(crate) fn lookup(id: SurfaceId) -> Option<HWND> {
    SURFACES.with(|surfaces| surfaces.borrow().get(&id).copied())
}

fn unregister(hwnd: HWND) {
    SURFACES.with(|surfaces| surfaces.borrow_mut().retain(|_, surface| *surface != hwnd));
}

pub(crate) unsafe extern "system" fn surface_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: exactly what Win32 delivered; the default handling is valid
    // for any message.
    let default = || unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    match message {
        // The application's renderer owns every pixel: the framework
        // neither erases nor paints, only acknowledges the paint so Windows
        // stops asking for one.
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            // SAFETY: `hwnd` is live; a null rectangle validates the whole
            // client area.
            unsafe { ValidateRect(hwnd, std::ptr::null()) };
            0
        }
        WM_GETOBJECT => crate::native::uia::get_object(
            hwnd,
            windows::Win32::Foundation::WPARAM(wparam),
            windows::Win32::Foundation::LPARAM(lparam),
        )
        .map_or_else(default, |result| result.0),
        WM_NCDESTROY => {
            unregister(hwnd);
            default()
        }
        _ => default(),
    }
}
