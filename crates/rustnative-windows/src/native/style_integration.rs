//! The Windows style capability table, held against what the backend
//! actually applies (`PLAN.md` Milestone 58, `X-L3-16`).
//!
//! Every row `rustnative_style::WINDOWS` answers "realized" or
//! "approximated" is read back from the native objects here — fonts through
//! `WM_GETFONT`, colours through the same `WM_CTLCOLORSTATIC` a control
//! paints with, a container's border and rounded region from the container
//! — and a scheme switch and a token switch are shown to restyle those same
//! objects, creating none. The "unavailable" row (`box-shadow`) is a build
//! error for this target, held by the compile-failure suite
//! (`rustnative-conformance`).

use rustnative_core::environment::keys;
use rustnative_core::{
    Application, Color, ColorScheme, Component, Event, Node, Platform, Size, StyleValue, Theme,
    Window, WindowId, classes,
};
use windows_sys::Win32::Foundation::{HWND, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    COMPLEXREGION, CreateCompatibleDC, CreateRectRgn, DeleteDC, DeleteObject, GetBkColor,
    GetObjectW, GetTextColor, GetWindowRgn, HGDIOBJ, LOGFONTW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{WM_CTLCOLORSTATIC, WM_GETFONT};

use super::harness::NativeHarness;
use super::user_data::BackgroundColorSlot;

struct Styled;

impl Component for Styled {
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
        Node::column(
            "root",
            [
                Node::label("title", "Styled").with_class(classes!(
                    "font-mono font-bold text-white bg-blue-500 dark:bg-blue-900"
                )),
                Node::button("go", "Go"),
            ],
        )
        .with_class(classes!("bg-white dark:bg-black border-red-500 rounded-lg p-2"))
    }
    fn update(&mut self, _: Event) {}
}

fn colorref(color: Color) -> u32 {
    u32::from(color.red) | u32::from(color.green) << 8 | u32::from(color.blue) << 16
}

fn token_color(name: &str) -> Color {
    match Theme::default().tokens().resolve(&StyleValue::Token(name.to_owned().into())) {
        Some(StyleValue::Color(color)) => color,
        other => panic!("{name}: {other:?}"),
    }
}

/// The text and background colours a control paints with, asked the way
/// Windows asks: `WM_CTLCOLORSTATIC` to its top-level window.
fn control_colors(harness: &mut NativeHarness, control: HWND) -> (u32, u32) {
    let window = harness.hwnd(WindowId::PRIMARY);
    // SAFETY: a memory DC compatible with the screen, released below.
    let hdc = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
    assert!(!hdc.is_null());
    harness.send(window, WM_CTLCOLORSTATIC, hdc as WPARAM, control as isize);
    // SAFETY: `hdc` is the live DC created above.
    let colors = unsafe { (GetTextColor(hdc), GetBkColor(hdc)) };
    // SAFETY: as above; deleted once.
    unsafe { DeleteDC(hdc) };
    colors
}

fn font(harness: &mut NativeHarness, control: HWND) -> LOGFONTW {
    let font = harness.send(control, WM_GETFONT, 0, 0);
    assert_ne!(font, 0, "the control has a font set");
    let mut logfont = LOGFONTW::default();
    let size = i32::try_from(std::mem::size_of::<LOGFONTW>()).unwrap();
    // SAFETY: `font` is the live `HFONT` the control reports; `logfont` is
    // an exclusively borrowed buffer of the size passed.
    let written = unsafe { GetObjectW(font as HGDIOBJ, size, (&raw mut logfont).cast()) };
    assert_eq!(written, size);
    logfont
}

fn region_kind(hwnd: HWND) -> i32 {
    // SAFETY: a scratch region, filled by `GetWindowRgn` and deleted.
    unsafe {
        let region = CreateRectRgn(0, 0, 0, 0);
        let kind = GetWindowRgn(hwnd, region);
        DeleteObject(region as HGDIOBJ);
        kind
    }
}

#[test]
fn the_windows_capability_table_is_what_the_backend_applies() {
    assert_eq!(crate::WindowsPlatform::new().style_capabilities(), rustnative_style::WINDOWS);
    assert_eq!(crate::WindowsPlatform::new().unit_mapping(), Some(rustnative_style::WINDOWS_UNITS));

    let mut application = Application::new(Styled, Window::new("Styles", Size::new(360, 240)));
    // SAFETY: `application` is declared before the harness and outlives it.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let root = harness.expect_control(WindowId::PRIMARY, "root");
    let title = harness.expect_control(WindowId::PRIMARY, "title");
    let go = harness.expect_control(WindowId::PRIMARY, "go");

    // Realized: font family (approximated to a Windows face), weight, size.
    let logfont = font(&mut harness, title);
    let face = String::from_utf16_lossy(&logfont.lfFaceName);
    assert!(face.starts_with("Consolas"), "font-mono is Consolas: {face:?}");
    assert_eq!(logfont.lfWeight, 700);
    assert_eq!(logfont.lfHeight, -14);

    // Realized: foreground and background.
    assert_eq!(
        control_colors(&mut harness, title),
        (colorref(Color::rgb(255, 255, 255)), colorref(token_color("color-blue-500")))
    );

    // Approximated on a container: background, border, rounded region.
    assert_eq!(BackgroundColorSlot::get(root), colorref(Color::rgb(255, 255, 255)));
    assert_eq!(
        super::rendering::shape::border_of(root),
        Some(colorref(token_color("color-red-500")))
    );
    assert_eq!(region_kind(root), COMPLEXREGION, "rounded-lg clips to a rounded region");

    // A scheme switch restyles the same windows.
    harness.with_runtime_mut(WindowId::PRIMARY, |runtime| {
        runtime.with_application(|application| {
            application.set_environment(&keys::COLOR_SCHEME, ColorScheme::Dark);
        });
        runtime.render().unwrap();
    });
    assert_eq!(BackgroundColorSlot::get(root), colorref(Color::rgb(0, 0, 0)));
    assert_eq!(control_colors(&mut harness, title).1, colorref(token_color("color-blue-900")));

    // A token switch restyles them too.
    let brand = Color::rgb(1, 2, 3);
    harness.with_runtime_mut(WindowId::PRIMARY, |runtime| {
        runtime.with_application(|application| {
            application.set_environment(&keys::COLOR_SCHEME, ColorScheme::Light);
            application
                .set_theme(Theme::default().with_token("color-blue-500", StyleValue::Color(brand)));
        });
        runtime.render().unwrap();
    });
    assert_eq!(control_colors(&mut harness, title).1, colorref(brand));

    // No window was created or destroyed to do it.
    for (key, hwnd) in [("root", root), ("title", title), ("go", go)] {
        assert_eq!(
            harness.expect_control(WindowId::PRIMARY, key),
            hwnd,
            "`{key}` is the same native object"
        );
    }
}
