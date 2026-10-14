//! Per-monitor DPI held end to end (`PLAN.md` Milestone 69, "Scale"): at
//! 150 % the layout stays in logical pixels while every native control is
//! placed, and every font created, at one and a half times that.

use rustnative_core::{Application, Component, Event, Node, NodeId, Size, Window, WindowId};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{GetObjectW, HGDIOBJ, LOGFONTW};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect, WM_GETFONT};

use super::dpi;
use super::harness::NativeHarness;

struct Scaled;

impl Component for Scaled {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("root", [Node::label("title", "Scaled"), Node::button("go", "Go")])
    }
    fn update(&mut self, _: Event) {}
}

fn window_size(hwnd: HWND) -> (i32, i32) {
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is live; `rect` is a valid out-parameter.
    unsafe { GetWindowRect(hwnd, &raw mut rect) };
    (rect.right - rect.left, rect.bottom - rect.top)
}

fn font_height(harness: &mut NativeHarness, control: HWND) -> i32 {
    let font = harness.send(control, WM_GETFONT, 0, 0);
    assert_ne!(font, 0, "the control has a font set");
    let mut logfont = LOGFONTW::default();
    let size = i32::try_from(std::mem::size_of::<LOGFONTW>()).unwrap();
    // SAFETY: `font` is the live `HFONT` the control reports; `logfont` is
    // an exclusively borrowed buffer of the size passed.
    let written = unsafe { GetObjectW(font as HGDIOBJ, size, (&raw mut logfont).cast()) };
    assert_eq!(written, size);
    logfont.lfHeight
}

/// The logical size of `key` and the device size of its control.
fn sizes(harness: &NativeHarness, key: &str) -> (Size, (i32, i32)) {
    let logical = harness
        .with_runtime(WindowId::PRIMARY, |runtime| {
            runtime.renderer.layout_size(NodeId::from_key(key))
        })
        .unwrap();
    (logical, window_size(harness.expect_control(WindowId::PRIMARY, key)))
}

#[test]
fn at_150_percent_layout_is_logical_and_controls_are_scaled() {
    let _forced = dpi::force_dpi(144);
    let mut application = Application::new(Scaled, Window::new("Scaled", Size::new(360, 240)));
    // SAFETY: `application` is declared before the harness and outlives it.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    assert_eq!(
        harness.with_runtime(WindowId::PRIMARY, |runtime| runtime.renderer.scale_dpi()),
        144
    );

    for key in ["title", "go"] {
        let (logical, (width, height)) = sizes(&harness, key);
        let (expected_width, expected_height) = (
            dpi::to_device(i32::try_from(logical.width).unwrap(), 144),
            dpi::to_device(i32::try_from(logical.height).unwrap(), 144),
        );
        // Edges round independently, so a size may differ by one pixel.
        assert!((width - expected_width).abs() <= 1, "{key}: {width} vs {expected_width}");
        assert!((height - expected_height).abs() <= 1, "{key}: {height} vs {expected_height}");
    }

    let root = harness.with_runtime(WindowId::PRIMARY, |runtime| {
        runtime.renderer.layout_size(NodeId::from_key("root"))
    });
    let root = root.unwrap();
    let mut client = RECT::default();
    // SAFETY: the primary window is live; `client` is a valid out-parameter.
    unsafe { GetClientRect(harness.hwnd(WindowId::PRIMARY), &raw mut client) };
    let logical_client = u32::try_from(dpi::to_logical(client.right, 144)).unwrap();
    assert!(
        root.width.abs_diff(logical_client) <= 1,
        "the root fills the client area in logical pixels: {root:?} in {}",
        client.right
    );

    let title = harness.expect_control(WindowId::PRIMARY, "title");
    assert_eq!(font_height(&mut harness, title), -21, "a 14 px font is created at 21 px");
}
