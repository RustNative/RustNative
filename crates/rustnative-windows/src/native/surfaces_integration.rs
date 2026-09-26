//! Surfaces beyond the window on Windows (`PLAN.md` Milestone 57): the
//! tray icon exists while the application asks for it, its clicks and
//! menu choices come back as `Event::SurfaceAction`, and the jump list and
//! taskbar progress are accepted by the shell.

use std::cell::RefCell;

use rustnative_core::capability::SurfaceKind;
use rustnative_core::surfaces::{ACTIVATE, JumpTask, NOTIFICATION, TrayMenuItem};
use rustnative_core::{
    Application, Component, ComponentContext, Event, Node, Size, Window, WindowId,
};
use windows_sys::Win32::UI::Shell::{NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect};
use windows_sys::Win32::UI::WindowsAndMessaging::WM_LBUTTONUP;

use super::harness::NativeHarness;

thread_local! {
    static ACTIONS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

struct TrayApp {
    asked: bool,
}

impl Component for TrayApp {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { asked: false }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("tray-app", [])
    }
    fn update(&mut self, event: Event) {
        if let Event::SurfaceAction { surface: SurfaceKind::TrayExtra, action, .. } = event {
            ACTIONS.with(|actions| actions.borrow_mut().push(action));
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        if !self.asked {
            self.asked = true;
            let surfaces = context.services().surfaces();
            surfaces.show_tray("Rust Native test", vec![TrayMenuItem::new("export", "Export")]);
            surfaces.set_progress(Some(0.5));
        }
        Node::column("tray-app", [Node::label("hello", "Hello")])
    }
}

fn icon_exists(hwnd: windows_sys::Win32::Foundation::HWND) -> bool {
    // SAFETY: zeroed is a valid identifier before its fields are set.
    let mut identifier: NOTIFYICONIDENTIFIER = unsafe { std::mem::zeroed() };
    identifier.cbSize = u32::try_from(std::mem::size_of::<NOTIFYICONIDENTIFIER>()).unwrap_or(0);
    identifier.hWnd = hwnd;
    identifier.uID = 1;
    let mut rect = windows_sys::Win32::Foundation::RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: a valid identifier and an out-rectangle.
    unsafe { Shell_NotifyIconGetRect(&raw const identifier, &raw mut rect) >= 0 }
}

#[test]
fn native_tray_icon_answers_with_surface_actions() {
    let mut application =
        Application::new(TrayApp::new(()), Window::new("tray", Size::new(320, 200)));
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    harness.pump();
    let hwnd = harness.hwnd(WindowId::PRIMARY);
    assert!(
        harness.with_runtime(WindowId::PRIMARY, |runtime| runtime.tray.shown),
        "the icon was added"
    );
    assert!(icon_exists(hwnd), "the shell knows the icon");

    ACTIONS.with(|actions| actions.borrow_mut().clear());
    let click = isize::try_from(WM_LBUTTONUP).unwrap_or(0);
    harness.send(hwnd, super::surfaces::WM_FRAMEWORK_TRAY, 0, click);
    harness.with_runtime_mut(WindowId::PRIMARY, |runtime| {
        super::surfaces::tray_callback(
            runtime,
            windows_sys::Win32::UI::Shell::NIN_BALLOONUSERCLICK,
        );
        super::surfaces::dispatch_action(runtime, "export".into());
    });
    assert_eq!(
        ACTIONS.with(|actions| actions.borrow().clone()),
        [ACTIVATE, NOTIFICATION, "export"],
        "a click, a notification click, and a menu choice"
    );

    // The shell takes the jump list and the progress.
    super::surfaces::set_jump_list(&[JumpTask {
        label: "New note".into(),
        arguments: "--new".into(),
    }])
    .expect("the jump list is committed");
    super::surfaces::set_jump_list(&[]).expect("and emptied again, leaving the shell as it was");
    super::surfaces::set_progress(hwnd, None).expect("progress is cleared");

    harness.request_close(WindowId::PRIMARY);
    harness.pump();
    assert!(!icon_exists(hwnd), "the icon goes with its window");
}
