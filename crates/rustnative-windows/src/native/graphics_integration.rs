//! Milestone 29 native integration tests: canvases and native surfaces in
//! real windows, driven through the production message loop.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use rustnative_core::{
    Application, Color, Component, ComponentContext, DrawList, Event, InputInterest, LayoutStyle,
    Node, NodeId, Paint, RectF, Size, SizeMode, SurfaceId, Window, WindowId,
};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::UpdateWindow;
use windows_sys::Win32::System::SystemServices::MK_LBUTTON;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowRect, WM_DPICHANGED, WM_LBUTTONDOWN};

use super::graphics::canvas;
use super::harness::NativeHarness;

#[derive(Clone, PartialEq, Default)]
struct Log {
    renders: Rc<Cell<u32>>,
    regions: Rc<RefCell<Vec<Option<u32>>>>,
    surfaces: Rc<RefCell<Vec<(SurfaceId, Size)>>>,
    scales: Rc<RefCell<Vec<f32>>>,
}

/// A canvas whose color a button toggles, next to a label that never
/// changes, and a native surface a second button removes.
struct Scene {
    log: Log,
    blue: bool,
    surface_shown: bool,
}

fn fixed(width: i32, height: i32) -> LayoutStyle {
    LayoutStyle::new().width(SizeMode::Fixed(width)).height(SizeMode::Fixed(height))
}

impl Scene {
    fn drawing(&self) -> DrawList {
        let color = if self.blue { Color::rgb(0, 0, 255) } else { Color::rgb(255, 0, 0) };
        DrawList::new()
            .fill_rect(RectF::new(0.0, 0.0, 100.0, 60.0), Paint::color(color))
            // Two regions: left half is 1, right half is 2.
            .hit_region(1, RectF::new(0.0, 0.0, 50.0, 60.0))
            .hit_region(2, RectF::new(50.0, 0.0, 50.0, 60.0))
    }
}

impl Component for Scene {
    type Props = Log;
    type Message = ();

    fn new(log: Log) -> Self {
        Self { log, blue: false, surface_shown: true }
    }
    fn props(&self) -> &Log {
        &self.log
    }
    fn set_props(&mut self, log: Log) {
        self.log = log;
    }

    fn view(&self) -> Node {
        let mut children = vec![
            Node::canvas("canvas", self.drawing(), fixed(100, 60))
                .with_input(InputInterest::new().pointer()),
            Node::label("steady", "I never change"),
            Node::button("recolor", "Recolor"),
            Node::button("drop-surface", "Remove surface"),
        ];
        if self.surface_shown {
            children.push(Node::native_surface("surface", fixed(120, 80)));
        }
        Node::column("root", children)
    }

    fn render(&mut self, _context: &mut ComponentContext<'_, ()>) -> Node {
        self.log.renders.set(self.log.renders.get() + 1);
        self.view()
    }

    fn update(&mut self, event: Event) {
        match event {
            Event::Click { target } if target == NodeId::from_key("recolor") => {
                self.blue = !self.blue;
            }
            Event::Click { target } if target == NodeId::from_key("drop-surface") => {
                self.surface_shown = false;
            }
            Event::PointerDown { pointer, .. } => {
                self.log.regions.borrow_mut().push(pointer.region());
            }
            Event::SurfaceResized { surface, size, scale_factor, .. } => {
                self.log.surfaces.borrow_mut().push((surface, size));
                self.log.scales.borrow_mut().push(scale_factor.get());
            }
            _ => {}
        }
    }
}

fn application(log: &Log) -> Application {
    Application::new(Scene::new(log.clone()), Window::new("graphics", Size::new(360, 420)))
}

fn rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is a live window; `rect` is exclusively borrowed.
    let read = unsafe { GetWindowRect(hwnd, &raw mut rect) } != 0;
    assert!(read, "GetWindowRect on a live window must succeed");
    rect
}

/// Paints `hwnd` now, synchronously, through its own window procedure.
fn paint_now(hwnd: HWND) {
    // SAFETY: `hwnd` is a live window on this thread.
    unsafe { UpdateWindow(hwnd) };
}

/// Changing a canvas's draw list reaches that canvas — same window, new
/// list — and costs one ordinary render; nothing else is rebuilt.
#[test]
fn a_changed_draw_list_redraws_the_same_canvas_window() {
    let log = Log::default();
    let mut application = application(&log);
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let canvas_hwnd = harness.expect_control(WindowId::PRIMARY, "canvas");
    let steady = harness.expect_control(WindowId::PRIMARY, "steady");
    paint_now(canvas_hwnd);
    let red = canvas::draw_list_of(canvas_hwnd).expect("a canvas has a list");
    assert_eq!(canvas::targets_created(canvas_hwnd), 1, "painting created the render target");

    let renders = log.renders.get();
    harness.click(WindowId::PRIMARY, "recolor");
    paint_now(canvas_hwnd);

    assert_eq!(harness.expect_control(WindowId::PRIMARY, "canvas"), canvas_hwnd, "not recreated");
    assert_eq!(harness.expect_control(WindowId::PRIMARY, "steady"), steady);
    let blue = canvas::draw_list_of(canvas_hwnd).expect("still a canvas");
    assert_ne!(blue, red, "the window has the new list");
    assert_eq!(log.renders.get(), renders + 1, "one render for one click");
    assert_eq!(
        canvas::targets_created(canvas_hwnd),
        1,
        "a new drawing reuses the device resources rather than rebuilding them"
    );
}

/// A lost device is recovered from: the render target is rebuilt on the
/// next paint, from the draw list, which survives because it is data.
#[test]
fn a_lost_device_is_rebuilt_on_the_next_paint() {
    let log = Log::default();
    let mut application = application(&log);
    // SAFETY: `application` is declared first, so it outlives the harness.
    let harness = unsafe { NativeHarness::attach(&mut application) };
    let canvas_hwnd = harness.expect_control(WindowId::PRIMARY, "canvas");
    paint_now(canvas_hwnd);
    assert_eq!(canvas::targets_created(canvas_hwnd), 1);

    canvas::discard_device_resources(canvas_hwnd);
    paint_now(canvas_hwnd);
    assert_eq!(canvas::targets_created(canvas_hwnd), 2, "the next paint built a new target");
    assert!(canvas::draw_list_of(canvas_hwnd).is_some(), "and still has what to draw");
}

/// Pointer input on a canvas reports which drawn region it landed in.
#[test]
fn a_pointer_press_on_a_canvas_reports_the_region_under_it() {
    let log = Log::default();
    let mut application = application(&log);
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let canvas_hwnd = harness.expect_control(WindowId::PRIMARY, "canvas");

    // `MAKELPARAM(x, y)` of two client coordinates, as the mouse delivers.
    let press = |x: i16, y: i16| (MK_LBUTTON as usize, (isize::from(y) << 16) | isize::from(x));
    let (wparam, lparam) = press(20, 30);
    harness.post(canvas_hwnd, WM_LBUTTONDOWN, wparam, lparam);
    let (wparam, lparam) = press(80, 30);
    harness.post(canvas_hwnd, WM_LBUTTONDOWN, wparam, lparam);

    assert_eq!(*log.regions.borrow(), vec![Some(1), Some(2)]);
}

/// A native surface is sized by layout, reported with its size, and its
/// handle round-trips through `raw-window-handle` to the very window layout
/// positioned — until the node is removed, after which there is no handle.
#[test]
fn a_native_surface_is_laid_out_reported_and_handed_out() {
    let log = Log::default();
    let mut application = application(&log);
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let surface_hwnd = harness.expect_control(WindowId::PRIMARY, "surface");

    let reported = log.surfaces.borrow().clone();
    assert_eq!(reported.len(), 1, "one report for the first layout, not one per pass");
    let (surface, size) = reported[0];
    assert_eq!(size, Size::new(120, 80));
    let bounds = rect(surface_hwnd);
    assert_eq!((bounds.right - bounds.left, bounds.bottom - bounds.top), (120, 80));

    let handle = crate::native_surface(surface).expect("the surface exists");
    let raw = handle.window_handle().expect("a live window").as_raw();
    let RawWindowHandle::Win32(win32) = raw else { panic!("a Win32 handle, got {raw:?}") };
    assert_eq!(win32.hwnd.get(), surface_hwnd as isize, "the handle is the laid-out window");
    assert!(win32.hinstance.is_some(), "with the module that created it");

    harness.click(WindowId::PRIMARY, "drop-surface");
    assert!(crate::native_surface(surface).is_none(), "a removed surface has no handle");
    assert!(handle.window_handle().is_err(), "and a kept handle reports it unavailable");
}

/// The surface hand-off's DPI contract (`docs/interop/surface-handoff.md`):
/// on `WM_DPICHANGED` the window takes the rectangle Windows suggests, and
/// every surface is told its new scale factor even though its size in
/// layout units is unchanged — what a swapchain needs to re-create at.
#[test]
fn a_dpi_change_moves_the_window_and_re_reports_every_surface() {
    let log = Log::default();
    let mut application = application(&log);
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let window = harness.hwnd(WindowId::PRIMARY);
    let before = log.surfaces.borrow().len();
    let suggested = RECT { left: 40, top: 50, right: 40 + 540, bottom: 50 + 630 };
    harness.send(window, WM_DPICHANGED, 0x0090_0090, (&raw const suggested) as isize);
    harness.pump();
    let after = rect(window);
    assert_eq!(
        (after.left, after.top, after.right, after.bottom),
        (40, 50, 580, 680),
        "the suggested rectangle"
    );
    let surfaces = log.surfaces.borrow();
    assert!(surfaces.len() > before, "the surface was reported again");
    assert_eq!(
        surfaces.last().map(|(_, size)| *size),
        Some(Size::new(120, 80)),
        "its layout size is unchanged"
    );
    assert_eq!(log.scales.borrow().last().copied(), Some(1.5), "and its scale is the new DPI's");
}
