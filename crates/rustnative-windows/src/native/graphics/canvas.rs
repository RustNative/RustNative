//! The canvas window: a child `HWND` that paints a [`DrawList`] with
//! Direct2D.
//!
//! # Where the draw list lives
//!
//! On the window itself, in a [`CanvasState`] owned through
//! `GWLP_USERDATA`. `WM_PAINT` must be answerable without reaching the
//! window's `Runtime` — a paint can be delivered synchronously while the
//! runtime is already borrowed (an `UpdateWindow` during a render), and
//! resolving the runtime again there is exactly the reentrancy this backend
//! forbids. The renderer hands the window its list ([`set_draw_list`]);
//! the window paints whatever it last received.
//!
//! The slot cannot be misread as anything else: `RuntimeSlot::get` only
//! reads windows of the top-level class, and this is not one.
//!
//! # Device loss
//!
//! Direct2D reports a lost device (a driver reset, a remote session
//! reconnecting) as `D2DERR_RECREATE_TARGET` from `EndDraw`. The render
//! target and everything created from it are then discarded and the window
//! invalidated, so the next paint recreates them from the draw list — which
//! is never lost, because it is data, not device state.

use rustnative_core::{Color, DrawList};
use windows::Win32::Foundation::D2DERR_RECREATE_TARGET;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_UNKNOWN, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    ID2D1HwndRenderTarget, ID2D1RenderTarget,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;
use windows::core::Interface;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GWLP_USERDATA, GetClientRect, GetWindowLongPtrW, SetWindowLongPtrW,
    WM_ERASEBKGND, WM_GETOBJECT, WM_NCDESTROY, WM_PAINT, WM_SIZE,
};

use super::d2d;
use crate::native::context::root_window;
use crate::native::message_loop::{panic_payload_message, poison_runtime_and_quit};
use crate::native::user_data::RuntimeSlot;
use crate::native::win32::best_effort;

/// The registered class name for canvas windows.
pub(crate) const CANVAS_CLASS_NAME: &str = "RustNativeFrameworkCanvas";

/// Everything one canvas window owns.
#[derive(Default)]
pub(crate) struct CanvasState {
    draw_list: DrawList,
    background: Option<Color>,
    /// Device-dependent: dropped on device loss and recreated on demand.
    target: Option<ID2D1HwndRenderTarget>,
    /// How many render targets this window has created — observable proof
    /// that a lost device was actually recovered from.
    pub(crate) targets_created: u32,
}

/// Attaches fresh state to a newly created canvas window.
pub(crate) fn attach(hwnd: HWND, draw_list: DrawList) {
    let state = Box::new(CanvasState { draw_list, ..CanvasState::default() });
    // SAFETY: `hwnd` is a live canvas window this crate just created; its
    // user data is unused until now, and ownership of the box passes to the
    // window, which releases it in `WM_NCDESTROY`.
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize) };
}

/// Runs `f` against `hwnd`'s canvas state, if it has any.
fn with_state<R>(hwnd: HWND, f: impl FnOnce(&mut CanvasState) -> R) -> Option<R> {
    // SAFETY: `hwnd` is a canvas window (the only callers are this class's
    // window procedure and the renderer holding a canvas handle); its user
    // data is either null or the box `attach` stored, which only
    // `WM_NCDESTROY` frees. Every access happens on the window's own thread
    // and none holds the reference across a call that could re-enter it.
    let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut CanvasState;
    if state.is_null() {
        return None;
    }
    // SAFETY: as above — non-null means it is the live, uniquely borrowed
    // state box.
    Some(f(unsafe { &mut *state }))
}

/// Gives the canvas a new list to draw, repainting only if it changed.
pub(crate) fn set_draw_list(hwnd: HWND, draw_list: &DrawList) {
    let changed = with_state(hwnd, |state| {
        if state.draw_list == *draw_list {
            return false;
        }
        state.draw_list = draw_list.clone();
        true
    });
    if changed == Some(true) {
        invalidate(hwnd);
    }
}

/// Sets the color painted behind the drawing.
pub(crate) fn set_background(hwnd: HWND, background: Option<Color>) {
    let changed = with_state(hwnd, |state| {
        let changed = state.background != background;
        state.background = background;
        changed
    });
    if changed == Some(true) {
        invalidate(hwnd);
    }
}

/// Discards the render target as a lost device would, so the next paint
/// has to recreate it.
#[cfg(test)]
pub(crate) fn discard_device_resources(hwnd: HWND) {
    with_state(hwnd, |state| state.target = None);
    invalidate(hwnd);
}

/// The list `hwnd` currently draws.
#[cfg(test)]
pub(crate) fn draw_list_of(hwnd: HWND) -> Option<DrawList> {
    with_state(hwnd, |state| state.draw_list.clone())
}

/// How many render targets `hwnd` has created.
#[cfg(test)]
pub(crate) fn targets_created(hwnd: HWND) -> u32 {
    with_state(hwnd, |state| state.targets_created).unwrap_or(0)
}

fn invalidate(hwnd: HWND) {
    // SAFETY: `hwnd` is a live window; a null rectangle invalidates the
    // whole client area.
    let invalidated = unsafe { InvalidateRect(hwnd, std::ptr::null(), 0) } != 0;
    best_effort(invalidated, "InvalidateRect(canvas)", "the canvas repaints on the next paint");
}

pub(crate) unsafe extern "system" fn canvas_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match std::panic::catch_unwind(move || canvas_proc_impl(hwnd, message, wparam, lparam)) {
        Ok(result) => result,
        Err(payload) => {
            let message = panic_payload_message(payload.as_ref());
            poison_runtime_and_quit(RuntimeSlot::get(root_window(hwnd)), message);
            0
        }
    }
}

fn canvas_proc_impl(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: exactly what Win32 delivered; the default handling is valid
    // for any message.
    let default = || unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    match message {
        WM_PAINT => {
            paint(hwnd);
            0
        }
        // Direct2D paints every pixel; erasing first would only flicker.
        WM_ERASEBKGND => 1,
        WM_SIZE => {
            resize(hwnd);
            0
        }
        WM_GETOBJECT => crate::native::uia::get_object(
            hwnd,
            windows::Win32::Foundation::WPARAM(wparam),
            windows::Win32::Foundation::LPARAM(lparam),
        )
        .map_or_else(default, |result| result.0),
        WM_NCDESTROY => {
            // SAFETY: the window is being destroyed; its user data is the
            // box `attach` stored (or null), and nothing reads it after
            // this message.
            let state = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) } as *mut CanvasState;
            if !state.is_null() {
                // SAFETY: `state` came from `Box::into_raw` in `attach` and
                // is released exactly once, here.
                drop(unsafe { Box::from_raw(state) });
            }
            default()
        }
        _ => default(),
    }
}

fn client_size(hwnd: HWND) -> D2D_SIZE_U {
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is live; `rect` is exclusively borrowed.
    unsafe { GetClientRect(hwnd, &raw mut rect) };
    D2D_SIZE_U {
        width: u32::try_from(rect.right - rect.left).unwrap_or(0),
        height: u32::try_from(rect.bottom - rect.top).unwrap_or(0),
    }
}

fn resize(hwnd: HWND) {
    let size = client_size(hwnd);
    with_state(hwnd, |state| {
        if let Some(target) = &state.target {
            // SAFETY: `size` is a plain value borrowed for the call. A
            // failure leaves the old size, and the next paint's
            // `EndDraw` reports whether the target is still usable.
            if unsafe { target.Resize(&raw const size) }.is_err() {
                state.target = None;
            }
        }
    });
}

fn paint(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    // SAFETY: `hwnd` is live and `paint` exclusively borrowed; every
    // `BeginPaint` is matched by the `EndPaint` below.
    unsafe { BeginPaint(hwnd, &raw mut paint) };
    let lost = with_state(hwnd, |state| render(hwnd, state)).unwrap_or(false);
    // SAFETY: matches the `BeginPaint` above.
    unsafe { EndPaint(hwnd, &raw const paint) };
    if lost {
        // The device is gone; the next paint builds a new one.
        invalidate(hwnd);
    }
}

/// Draws the state's list, returning whether the device was lost.
fn render(hwnd: HWND, state: &mut CanvasState) -> bool {
    d2d::with_factories(|factories| {
        if state.target.is_none() {
            state.target = create_target(hwnd, factories);
            if state.target.is_some() {
                state.targets_created += 1;
            }
        }
        let Some(target) = state.target.clone() else {
            return false;
        };
        let Ok(target): Result<ID2D1RenderTarget, _> = target.cast() else {
            return false;
        };
        let background = state.background.unwrap_or(Color::rgb(255, 255, 255));
        // SAFETY: `target` is live; `BeginDraw`/`EndDraw` bracket all drawing.
        unsafe {
            target.BeginDraw();
            target.Clear(Some(&d2d::d2d_color(background)));
        }
        let drawn = d2d::draw(&target, factories, &state.draw_list);
        // SAFETY: matches the `BeginDraw` above.
        let ended = unsafe { target.EndDraw(None, None) };
        best_effort(drawn.is_ok(), "Direct2D resource creation", "the canvas draws partially");
        match ended {
            Err(error) if error.code() == D2DERR_RECREATE_TARGET => {
                state.target = None;
                true
            }
            Err(_) => {
                best_effort(false, "EndDraw(canvas)", "the canvas shows its previous frame");
                false
            }
            Ok(()) => false,
        }
    })
    .unwrap_or(false)
}

fn create_target(hwnd: HWND, factories: &d2d::Factories) -> Option<ID2D1HwndRenderTarget> {
    let properties = D2D1_RENDER_TARGET_PROPERTIES {
        r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_UNKNOWN,
            alphaMode: D2D1_ALPHA_MODE_UNKNOWN,
        },
        // Canvas units are layout units, which this backend keeps in
        // physical pixels; pinning 96 DPI makes one the other.
        dpiX: 96.0,
        dpiY: 96.0,
        usage: D2D1_RENDER_TARGET_USAGE_NONE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    };
    let hwnd_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
        hwnd: windows::Win32::Foundation::HWND(hwnd),
        pixelSize: client_size(hwnd),
        presentOptions: D2D1_PRESENT_OPTIONS_NONE,
    };
    // SAFETY: both property structs are fully initialized and borrowed for
    // the call; `hwnd` is the live window the target will draw into.
    let created = unsafe {
        factories.d2d.CreateHwndRenderTarget(&raw const properties, &raw const hwnd_properties)
    };
    best_effort(created.is_ok(), "CreateHwndRenderTarget", "the canvas paints nothing");
    created.ok()
}
