//! The Windows backend's answers to the inspection protocol (`PLAN.md`
//! Milestone 44), and the in-application overlay.
//!
//! The realized objects are the renderer's registry — each with its window
//! class and `HWND`, and its rectangle as Win32 has it — so the mapping
//! between the declarative tree and the host objects is read, not
//! reconstructed. Rectangles for the layout explanation are the ones the
//! renderer positioned the windows at.
//!
//! The overlay is a canvas window — the same class, and the same Direct2D
//! path, a [`rustnative_core::Node::canvas`] draws through — made a layered,
//! click-through, topmost popup over the window's client area, its
//! background keyed transparent.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::null;

use rustnative_core::inspect::{InspectBackend, Lifetimes, MapperEntry, RealizedObject, node_name};
use rustnative_core::{Color, NodeId, Platform, PlatformCapabilities, Rect, WindowId};
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{ClientToScreen, MapWindowPoints};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetClientRect, GetParent, GetWindowRect, HWND_TOPMOST,
    IsWindow, LWA_COLORKEY, PostMessageW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SetLayeredWindowAttributes, SetWindowPos, ShowWindow, WM_CLOSE, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use super::graphics::canvas;
use super::rendering::Renderer;
use super::runtime::Runtime;
use super::util::{module_instance, wide};
use super::win32::{best_effort, ignored_by_contract};

/// The overlay's key colour: drawn where nothing is, and keyed out.
const KEY: Color = Color::rgb(255, 0, 255);

thread_local! {
    /// Each top-level window's overlay popup, by the window's handle.
    static OVERLAYS: RefCell<HashMap<isize, isize>> = RefCell::new(HashMap::new());
}

/// One window's renderer, answering for that window.
pub(crate) struct WindowsInspect<'a> {
    pub(crate) window: WindowId,
    pub(crate) renderer: &'a Renderer,
}

fn parent_rect(hwnd: HWND) -> Option<[i32; 4]> {
    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: `hwnd` is a live window the registry owns; `rect` is a
    // valid out-parameter.
    if unsafe { GetWindowRect(hwnd, &raw mut rect) } == 0 {
        return None;
    }
    let mut points = [POINT { x: rect.left, y: rect.top }, POINT { x: rect.right, y: rect.bottom }];
    // SAFETY: as above; the parent of a live child window is live (or
    // null, which maps to screen coordinates); `points` holds the two
    // points the count names.
    unsafe { MapWindowPoints(std::ptr::null_mut(), GetParent(hwnd), points.as_mut_ptr(), 2) };
    Some([points[0].x, points[0].y, points[1].x - points[0].x, points[1].y - points[0].y])
}

impl WindowsInspect<'_> {
    /// Every laid-out node at its window rectangle: its own offset plus
    /// every ancestor's (scroll offsets are not applied).
    fn window_rects(&self) -> HashMap<NodeId, Rect> {
        let layout = self.renderer.layout_rects();
        let snapshot = self.renderer.snapshot();
        layout
            .iter()
            .map(|(id, rect)| {
                let (mut x, mut y) = (rect.x, rect.y);
                let mut parent = snapshot.get(*id).and_then(|node| node.parent);
                while let Some(ancestor) = parent {
                    if let Some(offset) = layout.get(&ancestor) {
                        x += offset.x;
                        y += offset.y;
                    }
                    parent = snapshot.get(ancestor).and_then(|node| node.parent);
                }
                (*id, Rect::new(x, y, rect.width, rect.height))
            })
            .collect()
    }
}

impl InspectBackend for WindowsInspect<'_> {
    fn name(&self) -> &'static str {
        "windows"
    }

    fn realized(&self, window: WindowId) -> Vec<RealizedObject> {
        if window != self.window {
            return Vec::new();
        }
        let mut objects: Vec<RealizedObject> = self
            .renderer
            .registry
            .iter()
            .map(|(id, object)| RealizedObject {
                node: node_name(*id),
                key: id.local_key(),
                host_type: object.host_type(),
                handle: Some(format!("{:#x}", object.hwnd() as usize)),
                rect: parent_rect(object.hwnd()),
            })
            .collect();
        objects.sort_by(|a, b| a.node.cmp(&b.node));
        objects
    }

    fn rects(&self, window: WindowId) -> Option<HashMap<NodeId, Rect>> {
        (window == self.window).then(|| self.renderer.layout_rects().clone())
    }

    fn lifetimes(&self) -> Lifetimes {
        self.renderer.registry.lifetimes()
    }

    fn capabilities(&self) -> PlatformCapabilities {
        crate::WindowsPlatform::new().capabilities()
    }

    fn style_capabilities(&self) -> rustnative_style::StyleCapabilities {
        rustnative_style::WINDOWS
    }

    fn unit_mapping(&self) -> Option<rustnative_style::UnitMapping> {
        Some(rustnative_style::WINDOWS_UNITS)
    }

    fn mappers(&self) -> Vec<MapperEntry> {
        crate::mappers::active_mappers()
            .into_iter()
            .map(|mapper| MapperEntry {
                target: match mapper.target {
                    crate::MapperTarget::Kind(kind) => format!("every {kind:?}"),
                    crate::MapperTarget::Key(key) => format!("the node `{key}`"),
                },
                property: format!("{:?}", mapper.property),
                mode: format!("{:?}", mapper.mode).to_lowercase(),
            })
            .collect()
    }
}

/// Answers waiting inspection requests for `runtime`'s window. Returns
/// whether any was answered.
pub(crate) fn poll(runtime: &Runtime) -> bool {
    let backend = WindowsInspect { window: runtime.window_id, renderer: &runtime.renderer };
    let answered = runtime.with_application(|application| application.poll_inspection(&backend));
    // Asked to close (the development loop restarting it): closed as the
    // person closing the window would, so state is flushed and placement
    // saved.
    if runtime.with_application(rustnative_core::Application::take_quit_request) {
        // SAFETY: posting to this runtime's own live window.
        let _ = unsafe { PostMessageW(runtime.window, WM_CLOSE, 0, 0) };
    }
    answered
}

/// Shows, moves, redraws, or removes `runtime`'s overlay to match the
/// application's overlay mode.
pub(crate) fn sync_overlay(runtime: &Runtime) {
    let backend = WindowsInspect { window: runtime.window_id, renderer: &runtime.renderer };
    let rects = backend.window_rects();
    let list = runtime
        .with_application(|application| application.overlay_draw_list(runtime.window_id, &rects));
    let root = runtime.window;
    let existing = OVERLAYS.with(|overlays| overlays.borrow().get(&(root as isize)).copied());
    // SAFETY: `IsWindow` accepts any value, stale or not.
    let existing = existing.map(|hwnd| hwnd as HWND).filter(|hwnd| unsafe { IsWindow(*hwnd) } != 0);
    let Some(list) = list else {
        if let Some(overlay) = existing {
            // SAFETY: `overlay` is a live popup this module created.
            ignored_by_contract(unsafe { DestroyWindow(overlay) });
            OVERLAYS.with(|overlays| overlays.borrow_mut().remove(&(root as isize)));
        }
        return;
    };
    let overlay = if let Some(overlay) = existing {
        overlay
    } else {
        let Some(overlay) = create_overlay(root) else { return };
        OVERLAYS.with(|overlays| overlays.borrow_mut().insert(root as isize, overlay as isize));
        overlay
    };
    canvas::set_draw_list(overlay, &list);
    place(root, overlay);
}

fn create_overlay(root: HWND) -> Option<HWND> {
    let class = wide(canvas::CANVAS_CLASS_NAME);
    // SAFETY: the canvas class is registered before any window exists;
    // `root` is a live top-level window, which owns the popup (so the popup
    // is destroyed with it); a null `lpParam` is not read by the class.
    let overlay = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            class.as_ptr(),
            null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            root,
            std::ptr::null_mut(),
            module_instance(),
            null(),
        )
    };
    best_effort(!overlay.is_null(), "CreateWindowExW(overlay)", "the overlay is not shown");
    if overlay.is_null() {
        return None;
    }
    canvas::attach(overlay, rustnative_core::DrawList::new());
    canvas::set_background(overlay, Some(KEY));
    let key = u32::from(KEY.red) | (u32::from(KEY.green) << 8) | (u32::from(KEY.blue) << 16);
    // SAFETY: `overlay` was just created with `WS_EX_LAYERED`.
    let keyed = unsafe { SetLayeredWindowAttributes(overlay, key, 0, LWA_COLORKEY) } != 0;
    best_effort(keyed, "SetLayeredWindowAttributes(overlay)", "the overlay hides the window");
    // SAFETY: `overlay` is live; showing without activating keeps focus
    // where the person had it.
    ignored_by_contract(unsafe { ShowWindow(overlay, SW_SHOWNOACTIVATE) });
    Some(overlay)
}

/// Puts `overlay` exactly over `root`'s client area.
fn place(root: HWND, overlay: HWND) {
    let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let mut origin = POINT { x: 0, y: 0 };
    // SAFETY: `root` is live; both out-parameters are valid.
    let measured = unsafe {
        GetClientRect(root, &raw mut client) != 0 && ClientToScreen(root, &raw mut origin) != 0
    };
    if !measured {
        return;
    }
    // SAFETY: both windows are live.
    let placed = unsafe {
        SetWindowPos(
            overlay,
            HWND_TOPMOST,
            origin.x,
            origin.y,
            client.right - client.left,
            client.bottom - client.top,
            SWP_NOACTIVATE,
        )
    } != 0;
    best_effort(placed, "SetWindowPos(overlay)", "the overlay stays where it was");
}

/// The overlay popup over `root`, if one is shown.
#[cfg(test)]
pub(crate) fn overlay_of(root: HWND) -> Option<HWND> {
    OVERLAYS
        .with(|overlays| overlays.borrow().get(&(root as isize)).map(|hwnd| *hwnd as HWND))
        // SAFETY: `IsWindow` accepts any value.
        .filter(|hwnd| unsafe { IsWindow(*hwnd) } != 0)
}
