//! The native controls of `rustnative_core::control` on Windows (`PLAN.md`
//! Milestone 48): each is the system's own control, raises its event when
//! the person changes it, and shows what the component decided afterwards.

use rustnative_core::{
    Application, CalendarDate, Component, Event, Node, NodeId, Size, Window, WindowId,
};
use windows_sys::Win32::Foundation::{HWND, SYSTEMTIME};
use windows_sys::Win32::UI::Controls::{
    DTM_SETSYSTEMTIME, DTN_DATETIMECHANGE, GDT_VALID, NMHDR, PBM_GETPOS, TB_THUMBPOSITION,
    TBM_SETPOS,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, CB_GETCURSEL, CB_SETCURSEL, CBN_SELCHANGE, GetClassNameW, GetParent, SendMessageW,
    SetWindowTextW, WM_COMMAND, WM_NOTIFY,
};

use super::harness::NativeHarness;

struct Settings {
    remember: bool,
    locked: bool,
    volume: i64,
    copies: i64,
    fruit: Option<usize>,
    due: CalendarDate,
    notes: String,
}

impl Component for Settings {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self {
            remember: false,
            locked: false,
            volume: 10,
            copies: 1,
            fruit: None,
            due: CalendarDate::new(2026, 9, 25).unwrap_or_default(),
            notes: String::new(),
        }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column(
            "settings",
            [
                Node::checkbox("remember", "Remember me", self.remember),
                Node::checkbox("locked", "Locked", self.locked),
                Node::slider("volume", self.volume, 0, 100),
                Node::spinner("copies", self.copies, 1, 9),
                Node::select("fruit", ["Apple", "Pear", "Plum"], self.fruit),
                Node::date_picker("due", self.due),
                Node::multiline_text("notes", self.notes.clone()),
                Node::progress("progress", Some(40)),
                Node::separator("rule"),
                Node::link("help", "Help"),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        let key = |target: NodeId, key: &str| target == NodeId::from_key(key);
        match event {
            Event::Toggled { target, on } if key(target, "remember") => self.remember = on,
            Event::ValueChanged { target, value } if key(target, "volume") => self.volume = value,
            Event::ValueChanged { target, value } if key(target, "copies") => self.copies = value,
            Event::SelectionChanged { index, .. } => self.fruit = index,
            Event::DateChanged { date, .. } => self.due = date,
            Event::TextChanged { value, .. } => self.notes = value,
            _ => {}
        }
    }
}

fn class(hwnd: HWND) -> String {
    let mut buffer = [0_u16; 64];
    // SAFETY: a live window; the buffer's length is passed.
    let length = unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), 64) };
    String::from_utf16_lossy(&buffer[..usize::try_from(length).unwrap_or(0)])
}

fn send(hwnd: HWND, message: u32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: a live control of this thread; each message takes plain values
    // or a pointer valid for the call.
    unsafe { SendMessageW(hwnd, message, wparam, lparam) }
}

fn set_text(hwnd: HWND, text: &str) {
    let text = super::util::wide(text);
    // SAFETY: a live control; NUL-terminated text alive for the call.
    unsafe { SetWindowTextW(hwnd, text.as_ptr()) };
}

#[test]
fn native_controls_are_the_systems_own_and_controlled() {
    let mut application =
        Application::new(Settings::new(()), Window::new("controls", Size::new(420, 720)));
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let control = |harness: &NativeHarness, key| harness.expect_control(WindowId::PRIMARY, key);

    for (key, expected) in [
        ("remember", "Button"),
        ("volume", "msctls_trackbar32"),
        ("copies", "Edit"),
        ("fruit", "ComboBox"),
        ("due", "SysDateTimePick32"),
        ("notes", "Edit"),
        ("progress", "msctls_progress32"),
        ("rule", "Static"),
    ] {
        assert_eq!(class(control(&harness, key)), expected, "{key}");
    }
    // `SysLink` needs Common Controls 6, which an application's manifest
    // brings and this test binary lacks: then a link is clickable text.
    assert!(["SysLink", "Static"].contains(&class(control(&harness, "help")).as_str()));
    assert_eq!(send(control(&harness, "progress"), PBM_GETPOS, 0, 0), 40);

    // A check box the component accepts, and one it refuses.
    harness.click(WindowId::PRIMARY, "remember");
    assert_eq!(send(control(&harness, "remember"), BM_GETCHECK, 0, 0), 1);
    harness.click(WindowId::PRIMARY, "locked");
    assert_eq!(send(control(&harness, "locked"), BM_GETCHECK, 0, 0), 0, "put back");

    // The trackbar, moved as a drag would (it notifies with `WM_HSCROLL`).
    let volume = control(&harness, "volume");
    send(volume, TBM_SETPOS, 1, 60);
    // SAFETY: a live control.
    let parent = unsafe { GetParent(volume) };
    send(
        parent,
        windows_sys::Win32::UI::WindowsAndMessaging::WM_HSCROLL,
        TB_THUMBPOSITION as usize,
        volume as isize,
    );
    harness.pump();
    let volume = &application_state(&harness).volume;
    assert_eq!(*volume, 60);

    // The spinner's field, typed into.
    set_text(control(&harness, "copies"), "4");
    harness.pump();
    assert_eq!(application_state(&harness).copies, 4);

    // The select, chosen from.
    let fruit = control(&harness, "fruit");
    send(fruit, CB_SETCURSEL, 2, 0);
    // SAFETY: a live control.
    let parent = unsafe { GetParent(fruit) };
    send(parent, WM_COMMAND, (CBN_SELCHANGE as usize) << 16, fruit as isize);
    harness.pump();
    assert_eq!(application_state(&harness).fruit, Some(2));
    assert_eq!(send(fruit, CB_GETCURSEL, 0, 0), 2);

    // The date picker, picked from.
    let due = control(&harness, "due");
    let time = SYSTEMTIME { wYear: 2027, wMonth: 1, wDay: 31, ..SYSTEMTIME::default() };
    send(due, DTM_SETSYSTEMTIME, GDT_VALID as usize, (&raw const time) as isize);
    let header = NMHDR { hwndFrom: due, idFrom: 0, code: DTN_DATETIMECHANGE };
    // SAFETY: a live control.
    let parent = unsafe { GetParent(due) };
    send(parent, WM_NOTIFY, 0, (&raw const header) as isize);
    harness.pump();
    assert_eq!(application_state(&harness).due, CalendarDate::new(2027, 1, 31).unwrap_or_default());

    // The multi-line field.
    // The multi-line field, typed into (a multi-line `EDIT` reports no
    // change for text set programmatically, only for typing).
    let notes = control(&harness, "notes");
    for character in "hi".encode_utf16() {
        send(
            notes,
            windows_sys::Win32::UI::WindowsAndMessaging::WM_CHAR,
            usize::from(character),
            0,
        );
        harness.pump();
    }
    assert_eq!(application_state(&harness).notes, "hi");
}

/// The component's state, read back through the inspector's view of it.
fn application_state(harness: &NativeHarness) -> Observed {
    harness.with_runtime(WindowId::PRIMARY, |runtime| {
        let node = |key: &str| {
            runtime
                .renderer
                .snapshot
                .nodes()
                .find(|node| node.id.local_key().as_deref() == Some(key))
                .and_then(|node| node.control.clone())
        };
        let mut observed = Observed::default();
        if let Some(rustnative_core::Control::Slider { value, .. }) = node("volume") {
            observed.volume = value;
        }
        if let Some(rustnative_core::Control::Spinner { value, .. }) = node("copies") {
            observed.copies = value;
        }
        if let Some(rustnative_core::Control::Select { selected, .. }) = node("fruit") {
            observed.fruit = selected;
        }
        if let Some(rustnative_core::Control::DatePicker { date }) = node("due") {
            observed.due = date;
        }
        if let Some(rustnative_core::Control::MultilineText { value }) = node("notes") {
            observed.notes = value;
        }
        observed
    })
}

#[derive(Default)]
struct Observed {
    volume: i64,
    copies: i64,
    fruit: Option<usize>,
    due: CalendarDate,
    notes: String,
}

/// Host content (`C28`): media is the system's `MCIWnd` player, and web
/// content — which this backend does not offer — is the application's
/// fallback, never an empty rectangle.
struct Player;

impl Component for Player {
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
        let capabilities = rustnative_core::Platform::capabilities(&crate::WindowsPlatform::new());
        let layout = rustnative_core::LayoutStyle::new();
        Node::column(
            "player",
            [
                rustnative_core::host_content(
                    "clip",
                    &rustnative_core::HostContent::Media { source: "missing-clip.wav".into() },
                    layout,
                    &capabilities,
                    || Node::label("clip-fallback", "No media"),
                ),
                rustnative_core::host_content(
                    "page",
                    &rustnative_core::HostContent::Web { url: "https://example.com".into() },
                    layout,
                    &capabilities,
                    || Node::label("page-fallback", "Open example.com in the browser"),
                ),
            ],
        )
    }
    fn update(&mut self, _: Event) {}
}

#[test]
fn native_host_content_is_the_hosts_player_or_the_stated_fallback() {
    let mut application =
        Application::new(Player::new(()), Window::new("player", Size::new(640, 600)));
    // SAFETY: `application` is declared first, so it outlives the harness.
    let harness = unsafe { NativeHarness::attach(&mut application) };
    assert_eq!(class(harness.expect_control(WindowId::PRIMARY, "clip")), "MCIWndClass");
    assert!(harness.control(WindowId::PRIMARY, "page").is_none());
    assert!(harness.control(WindowId::PRIMARY, "page-fallback").is_some());
}
