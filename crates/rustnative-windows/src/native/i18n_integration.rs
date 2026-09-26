//! Runtime locale switching on the native backend (`PLAN.md` Milestone 46):
//! the controls that exist take the new locale's text, a right-to-left
//! locale mirrors the subtree it covers, and the host formats numbers for
//! the locale.

use std::sync::Arc;

use rustnative_core::i18n::{Catalogues, LocaleService, Message};
use rustnative_core::{
    Application, Component, ComponentContext, Event, LayoutStyle, Locale, Node, NodeId, Services,
    Size, Window, WindowId, keys,
};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, GetWindowTextW, WS_EX_LAYOUTRTL,
};

use super::harness::NativeHarness;

const EN: &str =
    "greeting = Hello\ncount = { $n ->\n    [one] One item\n   *[other] { $n } items\n}\n";
const AR: &str = "greeting = مرحبا\ncount = { $n ->\n    [zero] لا عناصر\n    [one] عنصر واحد\n    [two] عنصران\n    [few] { $n } عناصر\n    [many] { $n } عنصرًا\n   *[other] { $n } عنصر\n}\n";

struct Switcher {
    locale: Locale,
}

impl Component for Switcher {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { locale: Locale::new("en") }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("root", [])
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { target } if target == NodeId::from_key("arabic")) {
            self.locale = Locale::new("ar");
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        context.provide_env(&keys::LOCALE, self.locale.clone());
        let screen = context.child::<Screen>("screen");
        Node::column("root", [Node::button("arabic", "العربية"), screen])
    }
}

struct Screen;

impl Component for Screen {
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
        Node::column("stage", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let locale = context.env(&keys::LOCALE);
        Node::column_with_layout(
            "stage",
            [
                Node::label("greeting", Message::new("greeting")),
                Node::label("count", Message::new("count").arg("n", 3)),
            ],
            LayoutStyle::new().direction(locale.direction()),
            rustnative_core::ColumnStyle::new(),
        )
    }
}

fn text(hwnd: HWND) -> String {
    let mut buffer = [0_u16; 128];
    // SAFETY: a live control; the buffer's length is passed.
    let length = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), 128) };
    String::from_utf16_lossy(&buffer[..usize::try_from(length).unwrap_or(0)])
}

#[test]
fn native_locale_switch_retexts_the_controls_that_exist_and_mirrors() {
    let catalogues = Catalogues::parse("en", &[("en", EN), ("ar", AR)]).unwrap();
    let services = Services::default().with_catalogues(Arc::new(catalogues));
    let mut application = Application::with_services(
        Switcher::new(()),
        Window::new("i18n", Size::new(400, 240)),
        services,
    );
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let greeting = harness.expect_control(WindowId::PRIMARY, "greeting");
    assert_eq!(text(greeting), "Hello");
    assert_eq!(text(harness.expect_control(WindowId::PRIMARY, "count")), "3 items");

    harness.click(WindowId::PRIMARY, "arabic");
    assert_eq!(harness.expect_control(WindowId::PRIMARY, "greeting"), greeting, "the same control");
    assert_eq!(text(greeting), "مرحبا");
    assert_eq!(text(harness.expect_control(WindowId::PRIMARY, "count")), "3 عناصر", "Arabic's few");
    let stage = harness.expect_control(WindowId::PRIMARY, "stage");
    // SAFETY: a live window of the harness.
    let style = unsafe { GetWindowLongPtrW(stage, GWL_EXSTYLE) };
    assert_ne!(
        style & isize::try_from(WS_EX_LAYOUTRTL).unwrap(),
        0,
        "the Arabic subtree is mirrored"
    );
}

#[test]
fn native_formatting_follows_the_locale() {
    let service = crate::WindowsLocale;
    assert_eq!(service.format_number(&Locale::new("de-DE"), 1234.5, 1), "1.234,5");
    assert_eq!(service.format_number(&Locale::new("en-NZ"), 1234.5, 1), "1,234.5");
}
