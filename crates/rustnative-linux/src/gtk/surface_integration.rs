//! Native surfaces on the running display server (Phase 5).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle};
use rustnative_core::{
    Application, Component, Event, LayoutStyle, Node, NodeId, Size, SizeMode, SurfaceId, Window,
};

use super::testing::{Harness, on_gtk, pump_until};

type Log = Rc<RefCell<Vec<(SurfaceId, Size)>>>;

struct Viewport {
    log: Log,
    wide: bool,
    shown: bool,
}

impl Component for Viewport {
    type Props = Log;
    type Message = ();
    fn new(log: Log) -> Self {
        Self { log, wide: false, shown: true }
    }
    fn props(&self) -> &Log {
        &self.log
    }
    fn set_props(&mut self, log: Log) {
        self.log = log;
    }
    fn view(&self) -> Node {
        let width = if self.wide { 120 } else { 80 };
        let mut children = vec![Node::button("grow", "Grow"), Node::button("hide", "Hide")];
        if self.shown {
            children.push(Node::native_surface(
                "scene",
                LayoutStyle::new().width(SizeMode::Fixed(width)).height(SizeMode::Fixed(60)),
            ));
        }
        Node::column("root", children)
    }
    fn update(&mut self, event: Event) {
        match event {
            Event::Click { target } if target == NodeId::from_key("grow") => self.wide = true,
            Event::Click { target } if target == NodeId::from_key("hide") => self.shown = false,
            Event::SurfaceResized { surface, size, .. } => {
                self.log.borrow_mut().push((surface, size));
            }
            _ => {}
        }
    }
}

/// The surface's pixel at `(x, y)` after filling it red with Xlib, read back
/// with `XGetImage` — proof the window is real and is where the handle says.
fn x11_fill_and_read(display: *mut x11::xlib::Display, window: x11::xlib::Window) -> u64 {
    // SAFETY: `display` and `window` come from a live surface handle; the
    // GC and image are freed before returning.
    unsafe {
        let gc = x11::xlib::XCreateGC(display, window, 0, std::ptr::null_mut());
        x11::xlib::XSetForeground(display, gc, 0x00ff_0000);
        x11::xlib::XFillRectangle(display, window, gc, 0, 0, 40, 40);
        x11::xlib::XSync(display, 0);
        let image = x11::xlib::XGetImage(display, window, 5, 5, 1, 1, !0, x11::xlib::ZPixmap);
        let pixel = if image.is_null() { 0 } else { x11::xlib::XGetPixel(image, 0, 0) };
        if !image.is_null() {
            x11::xlib::XDestroyImage(image);
        }
        x11::xlib::XFreeGC(display, gc);
        pixel
    }
}

#[test]
fn a_rendered_surface_is_a_live_native_surface_that_follows_its_node() {
    on_gtk(|| {
        let log: Log = Rc::default();
        let mut application = Application::new(
            Viewport::new(log.clone()),
            Window::new("Surface", Size::new(240, 200)),
        );
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        pump_until("the surface's first size", Duration::from_secs(5), || {
            harness.pump();
            !log.borrow().is_empty()
        });
        let (surface, size) = log.borrow()[0];
        let scale = harness
            .gtk_window(rustnative_core::WindowId::PRIMARY)
            .map_or(1, |window| gtk::prelude::WidgetExt::scale_factor(&window));
        let device = |logical: u32| logical * u32::try_from(scale).unwrap_or(1);
        assert!(
            size.width >= device(80) && size.height >= device(60),
            "device pixels of an 80×60 node at scale {scale}: {size:?}"
        );
        // A scale notification at the same scale reports nothing new; a
        // different one reports the new device size (mixed DPI).
        let reports = log.borrow().len();
        super::backend::post(super::backend::Work::ScaleChanged(
            rustnative_core::WindowId::PRIMARY,
        ));
        harness.pump();
        assert_eq!(log.borrow().len(), reports, "an unchanged scale is not reported again");
        let handle = crate::native_surface(surface).expect("a live surface");
        let raw = handle.window_handle().expect("available").as_raw();
        match (std::env::var("GDK_BACKEND").as_deref(), raw) {
            (Ok("wayland"), RawWindowHandle::Wayland(_)) => {}
            (Ok("x11"), RawWindowHandle::Xlib(xlib)) => {
                let raw_display = raw_window_handle::HasDisplayHandle::display_handle(&handle)
                    .expect("a display")
                    .as_raw();
                let raw_window_handle::RawDisplayHandle::Xlib(display) = raw_display else {
                    panic!("an Xlib display")
                };
                let display = display.display.expect("a display pointer").as_ptr().cast();
                assert_eq!(
                    x11_fill_and_read(display, xlib.window) & 0x00ff_ffff,
                    0x00ff_0000,
                    "drawn into the child window"
                );
            }
            (backend, raw) => panic!("{backend:?} produced {raw:?}"),
        }

        harness.click("grow");
        pump_until("the resize report", Duration::from_secs(5), || {
            harness.pump();
            log.borrow().iter().any(|(_, size)| size.width >= 120)
        });
        assert_eq!(
            log.borrow().iter().filter(|(id, _)| *id == surface).count(),
            log.borrow().len(),
            "the same surface"
        );

        harness.click("hide");
        assert!(
            matches!(handle.window_handle(), Err(HandleError::Unavailable)),
            "a removed node's surface is gone"
        );
        assert!(crate::native_surface(surface).is_none());
    });
}
