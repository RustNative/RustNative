//! Native surfaces (Milestone 29's escape hatch): a rendered `Surface` node
//! gets a surface of the display server's own, which an application's GPU
//! renderer (wgpu, ash, glutin) attaches to through `raw-window-handle`.
//!
//! | Display server | Surface |
//! |---|---|
//! | Wayland | a `wl_subsurface` of the toplevel's `wl_surface`, desynchronized, positioned at the node |
//! | X11 | a child X window of the toplevel, mapped at the node's rectangle |
//!
//! The surface follows its node: it is placed each time GTK allocates the
//! node's widget, reported to the component as `Event::SurfaceResized` when
//! its size or scale changes, and destroyed with the node.

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Mutex;

use gtk::prelude::*;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle, XlibDisplayHandle,
    XlibWindowHandle,
};
use rustnative_core::{NodeId, Rect, SurfaceId};
use wayland_client::protocol::{
    wl_compositor, wl_registry, wl_subcompositor, wl_subsurface, wl_surface,
};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

/// Surfaces that still exist, by id — consulted by a [`SurfaceHandle`] on
/// whatever thread it is used, so a handle kept past its node reports
/// itself unavailable rather than naming a destroyed surface.
static ALIVE: Mutex<Option<HashSet<u64>>> = Mutex::new(None);

fn alive(id: SurfaceId) -> bool {
    ALIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .is_some_and(|set| set.contains(&id.raw()))
}

fn set_alive(id: SurfaceId, live: bool) {
    let mut set = ALIVE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let set = set.get_or_insert_with(HashSet::new);
    if live {
        set.insert(id.raw());
    } else {
        set.remove(&id.raw());
    }
}

/// The handles of every live surface, for [`native_surface`] (UI thread).
static HANDLES: Mutex<Option<HashMap<u64, SurfaceHandle>>> = Mutex::new(None);

/// A native surface, in the form every Rust graphics library accepts: it
/// implements [`HasWindowHandle`] and [`HasDisplayHandle`].
///
/// Valid while its node exists; afterwards [`HasWindowHandle::window_handle`]
/// reports [`HandleError::Unavailable`]. `Send` and `Sync`, so it can be
/// given to a render thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceHandle {
    id: SurfaceId,
    window: RawWindowHandle,
    display: RawDisplayHandle,
}

// SAFETY: the raw handles are plain identifiers of display-server objects
// (an X window id, or the address of a `wl_surface` proxy the display
// server owns); `raw-window-handle` documents them as shareable across
// threads, and every use through this type first checks the surface is
// still alive.
unsafe impl Send for SurfaceHandle {}
// SAFETY: as for `Send`: the type is an immutable pair of identifiers.
unsafe impl Sync for SurfaceHandle {}

impl SurfaceHandle {
    /// The surface's id.
    #[must_use]
    pub const fn id(&self) -> SurfaceId {
        self.id
    }
}

impl HasWindowHandle for SurfaceHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        if !alive(self.id) {
            return Err(HandleError::Unavailable);
        }
        // SAFETY: the surface was just confirmed alive, and it lives exactly
        // as long as its node; the borrow is tied to `self`.
        Ok(unsafe { WindowHandle::borrow_raw(self.window) })
    }
}

impl HasDisplayHandle for SurfaceHandle {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        if !alive(self.id) {
            return Err(HandleError::Unavailable);
        }
        // SAFETY: the display connection outlives every surface on it.
        Ok(unsafe { DisplayHandle::borrow_raw(self.display) })
    }
}

/// The native surface `surface`, if it still exists — what a component
/// hands its renderer when `Event::SurfaceResized` arrives.
///
/// ```no_run
/// use rustnative_core::Event;
///
/// # fn update(event: Event) {
/// if let Event::SurfaceResized { surface, size, .. } = event {
///     if let Some(handle) = rustnative_linux::native_surface(surface) {
///         // e.g. `instance.create_surface(handle)` with wgpu, sized to `size`.
///         let _ = (handle, size);
///     }
/// }
/// # }
/// ```
#[must_use]
pub fn native_surface(surface: SurfaceId) -> Option<SurfaceHandle> {
    HANDLES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()?
        .get(&surface.raw())
        .copied()
}

/// The Wayland objects behind one surface.
struct WaylandSurface {
    connection: Connection,
    queue: wayland_client::EventQueue<Dispatcher>,
    surface: wl_surface::WlSurface,
    subsurface: wl_subsurface::WlSubsurface,
}

/// One node's native surface.
enum Native {
    Wayland(WaylandSurface),
    X11 { display: *mut x11::xlib::Display, window: x11::xlib::Window },
}

struct Placed {
    id: SurfaceId,
    native: Native,
    rect: Rect,
    /// What was last reported: device-pixel size and scale.
    reported: Option<(rustnative_core::Size, f64)>,
}

/// Every native surface of one window.
#[derive(Default)]
pub(crate) struct NativeSurfaces {
    surfaces: HashMap<NodeId, Placed>,
}

impl std::fmt::Debug for NativeSurfaces {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeSurfaces").field("count", &self.surfaces.len()).finish()
    }
}

fn next_id() -> SurfaceId {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    SurfaceId::from_raw(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// A change to report to the node's component.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SurfaceChange {
    pub(crate) node: NodeId,
    pub(crate) surface: SurfaceId,
    pub(crate) size: rustnative_core::Size,
    pub(crate) scale: f64,
}

impl NativeSurfaces {
    /// Creates (or moves) `node`'s surface to where `host` is now
    /// allocated in `window`. Returns what changed for the component.
    pub(crate) fn place(
        &mut self,
        node: NodeId,
        host: &gtk::Widget,
        window: &gtk::Window,
    ) -> Option<SurfaceChange> {
        let surface = window.surface()?;
        let (offset_x, offset_y) = window.surface_transform();
        let origin = host.compute_point(window, &gtk::graphene::Point::new(0.0, 0.0))?;
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a pixel position inside one window, rounded"
        )]
        let rect = Rect::new(
            (f64::from(origin.x()) + offset_x).round() as i32,
            (f64::from(origin.y()) + offset_y).round() as i32,
            host.width().max(1),
            host.height().max(1),
        );
        let scale = surface.scale();
        if let std::collections::hash_map::Entry::Vacant(slot) = self.surfaces.entry(node) {
            let native = create(&surface, rect, scale)?;
            let id = next_id();
            set_alive(id, true);
            let handle = handle(id, &native);
            HANDLES
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get_or_insert_with(HashMap::new)
                .insert(id.raw(), handle);
            slot.insert(Placed { id, native, rect, reported: None });
        }
        let placed = self.surfaces.get_mut(&node)?;
        if placed.rect != rect {
            reposition(&mut placed.native, rect, scale);
            placed.rect = rect;
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a widget size times a display scale: small, positive, rounded"
        )]
        let device = |value: i32| (f64::from(value.max(1)) * scale).round() as u32;
        let size = rustnative_core::Size::new(device(rect.width), device(rect.height));
        if placed.reported == Some((size, scale)) {
            return None;
        }
        placed.reported = Some((size, scale));
        Some(SurfaceChange { node, surface: placed.id, size, scale })
    }

    /// Destroys the surfaces of nodes no longer in `keep`.
    pub(crate) fn retain(&mut self, keep: impl Fn(NodeId) -> bool) {
        let gone: Vec<NodeId> = self.surfaces.keys().copied().filter(|node| !keep(*node)).collect();
        for node in gone {
            if let Some(placed) = self.surfaces.remove(&node) {
                destroy(placed);
            }
        }
    }

    /// The nodes that have a surface.
    pub(crate) fn nodes(&self) -> Vec<NodeId> {
        self.surfaces.keys().copied().collect()
    }

    /// Destroys every surface.
    pub(crate) fn release(&mut self) {
        self.retain(|_| false);
    }
}

fn handle(id: SurfaceId, native: &Native) -> SurfaceHandle {
    match native {
        Native::Wayland(wayland) => {
            let surface = NonNull::new(wayland.surface.id().as_ptr().cast::<c_void>())
                .unwrap_or(NonNull::dangling());
            let display = NonNull::new(wayland.connection.backend().display_ptr().cast::<c_void>())
                .unwrap_or(NonNull::dangling());
            SurfaceHandle {
                id,
                window: RawWindowHandle::Wayland(WaylandWindowHandle::new(surface)),
                display: RawDisplayHandle::Wayland(WaylandDisplayHandle::new(display)),
            }
        }
        Native::X11 { display, window } => SurfaceHandle {
            id,
            window: RawWindowHandle::Xlib(XlibWindowHandle::new(*window)),
            display: RawDisplayHandle::Xlib(XlibDisplayHandle::new(
                NonNull::new(display.cast::<c_void>()),
                0,
            )),
        },
    }
}

fn create(surface: &gtk::gdk::Surface, rect: Rect, scale: f64) -> Option<Native> {
    if let Some(x11) = surface.downcast_ref::<gdk4_x11::X11Surface>() {
        let display = x11.display().downcast::<gdk4_x11::X11Display>().ok()?;
        // SAFETY: GDK's Xlib display for this window's display, valid while
        // the display is open — longer than any window on it.
        let xdisplay = unsafe { display.xdisplay() };
        let device = device_rect(rect, scale);
        // SAFETY: `xdisplay` is open; `xid` is this window's live toplevel;
        // the geometry is clamped to at least one pixel.
        let window = unsafe {
            let window = x11::xlib::XCreateSimpleWindow(
                xdisplay,
                x11.xid(),
                device.x,
                device.y,
                device.width,
                device.height,
                0,
                0,
                0,
            );
            x11::xlib::XMapWindow(xdisplay, window);
            x11::xlib::XFlush(xdisplay);
            window
        };
        return Some(Native::X11 { display: xdisplay, window });
    }
    let display = surface.display().downcast::<gdk4_wayland::WaylandDisplay>().ok()?;
    let parent = gdk4_wayland::prelude::WaylandSurfaceExtManual::wl_surface(
        surface.downcast_ref::<gdk4_wayland::WaylandSurface>()?,
    )?;
    let wl_display = display.wl_display()?;
    let connection = Connection::from_backend(wl_display.backend().upgrade()?);
    let (globals, mut queue) =
        wayland_client::globals::registry_queue_init::<Dispatcher>(&connection).ok()?;
    let handle: QueueHandle<Dispatcher> = queue.handle();
    let compositor: wl_compositor::WlCompositor = globals.bind(&handle, 1..=4, ()).ok()?;
    let subcompositor: wl_subcompositor::WlSubcompositor = globals.bind(&handle, 1..=1, ()).ok()?;
    let child = compositor.create_surface(&handle, ());
    let subsurface = subcompositor.get_subsurface(&child, &parent, &handle, ());
    subsurface.set_position(rect.x, rect.y);
    // The application's renderer presents on its own schedule, not GTK's.
    subsurface.set_desync();
    child.commit();
    let _ = queue.roundtrip(&mut Dispatcher);
    Some(Native::Wayland(WaylandSurface { connection, queue, surface: child, subsurface }))
}

fn device_rect(rect: Rect, scale: f64) -> DeviceRect {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "window coordinates times a display scale: small, rounded, sizes at least one"
    )]
    let device = |value: i32| (f64::from(value) * scale).round() as i32;
    DeviceRect {
        x: device(rect.x),
        y: device(rect.y),
        width: u32::try_from(device(rect.width).max(1)).unwrap_or(1),
        height: u32::try_from(device(rect.height).max(1)).unwrap_or(1),
    }
}

struct DeviceRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn reposition(native: &mut Native, rect: Rect, scale: f64) {
    match native {
        Native::Wayland(wayland) => {
            // Subsurface positions are in the parent's (logical) coordinates
            // and take effect with the parent's next commit, which GTK makes
            // for the frame this allocation belongs to.
            wayland.subsurface.set_position(rect.x, rect.y);
            let _ = wayland.queue.dispatch_pending(&mut Dispatcher);
            let _ = wayland.connection.flush();
        }
        Native::X11 { display, window } => {
            let device = device_rect(rect, scale);
            // SAFETY: the display is open and `window` is this surface's
            // live child window.
            unsafe {
                x11::xlib::XMoveResizeWindow(
                    *display,
                    *window,
                    device.x,
                    device.y,
                    device.width,
                    device.height,
                );
                x11::xlib::XFlush(*display);
            }
        }
    }
}

fn destroy(placed: Placed) {
    set_alive(placed.id, false);
    if let Some(handles) =
        HANDLES.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_mut()
    {
        handles.remove(&placed.id.raw());
    }
    match placed.native {
        Native::Wayland(wayland) => {
            wayland.subsurface.destroy();
            wayland.surface.destroy();
            let _ = wayland.connection.flush();
        }
        Native::X11 { display, window } => {
            // SAFETY: the display is open and `window` was created by this
            // module and not yet destroyed.
            unsafe {
                x11::xlib::XDestroyWindow(display, window);
                x11::xlib::XFlush(display);
            }
        }
    }
}

/// Events on the objects this module creates; none needs an answer.
struct Dispatcher;

impl Dispatch<wl_registry::WlRegistry, wayland_client::globals::GlobalListContents> for Dispatcher {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &wayland_client::globals::GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

macro_rules! ignore_events {
    ($($proxy:ty),*) => {
        $(impl Dispatch<$proxy, ()> for Dispatcher {
            fn event(_: &mut Self, _: &$proxy, _: <$proxy as Proxy>::Event, (): &(), _: &Connection, _: &QueueHandle<Self>) {}
        })*
    };
}

ignore_events!(
    wl_compositor::WlCompositor,
    wl_subcompositor::WlSubcompositor,
    wl_subsurface::WlSubsurface,
    wl_surface::WlSurface
);
