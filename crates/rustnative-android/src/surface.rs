//! Native surfaces (`Node::native_surface`; `docs/interop/surface-handoff.md`)
//! on Android: an `RnSurfaceView` the framework places and never paints,
//! whose `Surface` is handed to the application as an `ANativeWindow` in
//! `raw-window-handle` form — what `wgpu`, `ash-window`, and `glutin` take.
//!
//! The application learns of a surface from `Event::SurfaceResized`, raised
//! when the surface is created and whenever its size changes, with its size
//! in device pixels and the display density as `scale_factor`; it turns the
//! event's `SurfaceId` into a handle with [`native_surface`]. A handle kept
//! past the surface's destruction does not dangle: its `window_handle()`
//! reports `HandleError::Unavailable`.

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Mutex;

use raw_window_handle::{
    AndroidDisplayHandle, AndroidNdkWindowHandle, DisplayHandle, HandleError, HasDisplayHandle,
    HasWindowHandle, RawDisplayHandle, RawWindowHandle, WindowHandle,
};
use rustnative_core::SurfaceId;

/// Every live surface's `ANativeWindow`, by surface id (as an address: the
/// pointer itself is not `Send`).
static WINDOWS: Mutex<Option<HashMap<u64, usize>>> = Mutex::new(None);

fn with_windows<R>(f: impl FnOnce(&mut HashMap<u64, usize>) -> R) -> R {
    let mut windows = WINDOWS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(windows.get_or_insert_with(HashMap::new))
}

/// A native surface's handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceHandle {
    id: SurfaceId,
    window: NonNull<c_void>,
}

// SAFETY: an `ANativeWindow` is reference counted and may be used from any
// thread (the NDK's contract); the handle is only a pointer to it, and its
// use is gated on the surface being alive.
unsafe impl Send for SurfaceHandle {}
// SAFETY: as above.
unsafe impl Sync for SurfaceHandle {}

impl SurfaceHandle {
    /// The surface it is a handle to.
    #[must_use]
    pub const fn id(&self) -> SurfaceId {
        self.id
    }

    fn alive(&self) -> bool {
        with_windows(|windows| {
            windows.get(&self.id.raw()) == Some(&(self.window.as_ptr() as usize))
        })
    }
}

impl HasWindowHandle for SurfaceHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        if !self.alive() {
            return Err(HandleError::Unavailable);
        }
        let raw = RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(self.window));
        // SAFETY: the window is alive (checked above) and held by the
        // backend until the surface is destroyed.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for SurfaceHandle {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        if !self.alive() {
            return Err(HandleError::Unavailable);
        }
        // SAFETY: Android's display handle carries nothing to dangle.
        Ok(unsafe {
            DisplayHandle::borrow_raw(RawDisplayHandle::Android(AndroidDisplayHandle::new()))
        })
    }
}

/// The handle of surface `id`, while it exists.
#[must_use]
pub fn native_surface(id: SurfaceId) -> Option<SurfaceHandle> {
    let address = with_windows(|windows| windows.get(&id.raw()).copied())?;
    NonNull::new(address as *mut c_void).map(|window| SurfaceHandle { id, window })
}

/// The surface id of the view tagged `tag` in window `window`.
#[allow(clippy::cast_sign_loss, reason = "the tag's bits, as the low half")]
pub(crate) const fn surface_id(window: u64, tag: i32) -> SurfaceId {
    SurfaceId::from_raw((window << 32) | (tag as u32 as u64))
}

#[cfg(target_os = "android")]
pub(crate) use platform::{changed, release_all};

#[cfg(target_os = "android")]
mod platform {
    use rustnative_core::{Event, NodeId, Scalar, Size, WindowId};

    use super::{surface_id, with_windows};
    use crate::Error;
    use crate::jni_host::{Arg, Class, call_static};
    use crate::registry::WindowRegistry;

    const CREATED: i64 = 1;
    const DESTROYED: i64 = 3;

    /// The surface of `node`'s view was created, resized, or destroyed.
    pub(crate) fn changed(
        registry: &mut WindowRegistry,
        window: WindowId,
        node: NodeId,
        tag: i32,
        what: i64,
        packed: i64,
    ) -> Result<(), Error> {
        let id = surface_id(window.get(), tag);
        if what == DESTROYED {
            release(id.raw());
            return Ok(());
        }
        let Some(runtime) = registry.windows.get(&window) else { return Ok(()) };
        let density = runtime.density;
        let Some(view) =
            runtime.renderer.as_ref().and_then(|renderer| renderer.view(node)).cloned()
        else {
            return Ok(());
        };
        if what == CREATED || with_windows(|windows| !windows.contains_key(&id.raw())) {
            let surface = call_static(
                Class::Surface,
                "surface",
                "(Landroid/view/View;)Landroid/view/Surface;",
                &[Arg::Obj(&view)],
            )?
            .obj();
            let Some(surface) = surface else { return Ok(()) };
            release(id.raw());
            let native = crate::jni_host::native_window(&surface)?;
            with_windows(|windows| windows.insert(id.raw(), native as usize));
        }
        let width = u32::try_from((packed >> 32) & 0xffff_ffff).unwrap_or(0);
        let height = u32::try_from(packed & 0xffff_ffff).unwrap_or(0);
        registry.dispatch(
            window,
            Event::SurfaceResized {
                target: node,
                surface: id,
                size: Size::new(width, height),
                scale_factor: Scalar::new(density),
            },
        )
    }

    fn release(raw: u64) {
        if let Some(address) = with_windows(|windows| windows.remove(&raw)) {
            crate::jni_host::release_native_window(address as *mut std::ffi::c_void);
        }
    }

    /// Releases every surface of `window` (its views are going).
    pub(crate) fn release_all(window: WindowId) {
        let prefix = window.get();
        let owned: Vec<u64> = with_windows(|windows| {
            windows.keys().copied().filter(|raw| raw >> 32 == prefix).collect()
        });
        for raw in owned {
            release(raw);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_surface_with_no_window_has_no_handle() {
        let id = surface_id(7, 3);
        assert_eq!(id.raw(), (7 << 32) | 3);
        assert!(native_surface(id).is_none());
        // A handle whose window was released reports it, rather than
        // handing out a dangling pointer.
        let handle = SurfaceHandle { id, window: NonNull::dangling() };
        assert!(matches!(handle.window_handle(), Err(HandleError::Unavailable)));
    }
}
