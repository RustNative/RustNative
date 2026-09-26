//! Embedding outward (`PLAN.md` Milestone 40): a Rust Native application
//! hosting two controls it did not write — the system month calendar
//! (`SysMonthCal32`), which the framework owns once adopted, and a date
//! picker (`SysDateTimePick32`), which the application keeps — as leaves of
//! its tree, measured, laid out, and removed by the framework's rules.
//!
//! Run with `--self-test` to drive it under an external loop and exit
//! (what `tests/foreign.rs` does).

use rustnative_core::{
    Alignment, Application, Component, Event, LayoutStyle, Node, NodeId, Platform, Size, SizeMode,
    Window,
};
use rustnative_windows::{ForeignControl, Ownership, WindowsPlatform, register_foreign};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::UI::Controls::{
    ICC_DATE_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_CLICK, CreateWindowExW, DestroyWindow, DispatchMessageW, EnumChildWindows, GA_PARENT,
    GetAncestor, GetClassNameW, GetWindowRect, GetWindowTextW, IsWindow, IsWindowVisible, MSG,
    PM_REMOVE, PeekMessageW, SendMessageW, TranslateMessage, WS_CHILD, WS_TABSTOP, WS_VISIBLE,
};

/// The preferred sizes the factories report, which layout honours.
const CALENDAR: Size = Size::new(240, 180);
const PICKER: Size = Size::new(160, 26);

struct Planner {
    showing: bool,
}

impl Component for Planner {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { showing: true }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        // Aligned to the start, so each keeps its natural width rather than
        // stretching across the column.
        let auto = LayoutStyle::new()
            .width(SizeMode::Auto)
            .height(SizeMode::Auto)
            .align_self(Alignment::Start);
        let mut children = vec![Node::button("toggle", if self.showing { "Hide" } else { "Show" })];
        if self.showing {
            children.push(Node::foreign("calendar", "month-calendar", auto));
            children.push(Node::foreign("picker", "date-picker", auto));
        }
        Node::column("root", children)
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { target } if target == NodeId::from_key("toggle")) {
            self.showing = !self.showing;
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn create_child(class: &str, parent: HWND) -> HWND {
    let class = wide(class);
    // SAFETY: a system control class registered by `InitCommonControlsEx`,
    // created as a child of the window the framework passed in.
    unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            std::ptr::null(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            0,
            0,
            0,
            parent,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    }
}

fn register_controls() {
    let init = INITCOMMONCONTROLSEX {
        dwSize: u32::try_from(std::mem::size_of::<INITCOMMONCONTROLSEX>()).unwrap_or(0),
        dwICC: ICC_DATE_CLASSES,
    };
    // SAFETY: `init` is a correctly sized, initialized structure.
    unsafe { InitCommonControlsEx(&raw const init) };
    register_foreign("month-calendar", CALENDAR, |parent| {
        let hwnd = create_child("SysMonthCal32", parent);
        (!hwnd.is_null()).then_some(ForeignControl { hwnd, ownership: Ownership::Owned })
    });
    register_foreign("date-picker", PICKER, |parent| {
        let hwnd = create_child("SysDateTimePick32", parent);
        (!hwnd.is_null()).then_some(ForeignControl { hwnd, ownership: Ownership::Borrowed })
    });
}

struct Search {
    class: Option<Vec<u16>>,
    text: Option<Vec<u16>>,
    found: Option<HWND>,
}

unsafe extern "system" fn visit(hwnd: HWND, search: isize) -> i32 {
    // SAFETY: `search` is the `&mut Search` `find` passes, alive for the call.
    let search = unsafe { &mut *(search as *mut Search) };
    let mut buffer = [0_u16; 128];
    let (wanted, length) = if let Some(class) = &search.class {
        // SAFETY: `buffer` is writable for its length.
        (class, unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), 128) })
    } else if let Some(text) = &search.text {
        // SAFETY: as above.
        (text, unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), 128) })
    } else {
        return 0;
    };
    if usize::try_from(length).is_ok_and(|length| buffer[..length] == wanted[..]) {
        search.found = Some(hwnd);
        return 0;
    }
    1
}

fn find(parent: HWND, class: Option<&str>, text: Option<&str>) -> Option<HWND> {
    let mut search = Search {
        class: class.map(|class| class.encode_utf16().collect()),
        text: text.map(|text| text.encode_utf16().collect()),
        found: None,
    };
    // SAFETY: `visit` uses `search` only for the duration of the call.
    unsafe { EnumChildWindows(parent, Some(visit), (&raw mut search) as isize) };
    search.found
}

fn size_of_window(hwnd: HWND) -> (i32, i32) {
    let mut rect = RECT::default();
    // SAFETY: `rect` is writable.
    unsafe { GetWindowRect(hwnd, &raw mut rect) };
    (rect.right - rect.left, rect.bottom - rect.top)
}

fn pump(guest: &mut rustnative_windows::ExternalLoop<'_>) {
    let mut message = MSG::default();
    // SAFETY: `message` is writable.
    while unsafe { PeekMessageW(&raw mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
        if !guest.handle_message(&message).unwrap_or(false) {
            // SAFETY: an ordinary retrieved message.
            unsafe {
                TranslateMessage(&raw const message);
                DispatchMessageW(&raw const message);
            }
        }
    }
}

fn self_test() -> Result<(), String> {
    let mut application =
        Application::new(Planner::new(()), Window::new("Planner", Size::new(480, 480)));
    let mut guest = WindowsPlatform::new()
        .start_external(&mut application)
        .map_err(|error| error.to_string())?;
    pump(&mut guest);
    let window = guest.window(rustnative_core::WindowId::PRIMARY).ok_or("the window exists")?;
    let calendar =
        find(window, Some("SysMonthCal32"), None).ok_or("the calendar is adopted into the tree")?;
    let picker = find(window, Some("SysDateTimePick32"), None)
        .ok_or("the picker is adopted into the tree")?;
    let expected = |size: Size| {
        (i32::try_from(size.width).unwrap_or(0), i32::try_from(size.height).unwrap_or(0))
    };
    if size_of_window(calendar) != expected(CALENDAR) || size_of_window(picker) != expected(PICKER)
    {
        return Err(format!(
            "layout sizes each to its factory's preferred size: calendar {:?}, picker {:?}",
            size_of_window(calendar),
            size_of_window(picker)
        ));
    }
    let toggle = find(window, None, Some("Hide")).ok_or("the toggle button")?;
    // SAFETY: clicking a live button of this thread.
    unsafe { SendMessageW(toggle, BM_CLICK, 0, 0) };
    pump(&mut guest);
    // SAFETY: `IsWindow`/`IsWindowVisible`/`GetAncestor` accept any handle.
    let (calendar_alive, picker_alive, picker_visible, picker_parent) = unsafe {
        (
            IsWindow(calendar) != 0,
            IsWindow(picker) != 0,
            IsWindowVisible(picker) != 0,
            GetAncestor(picker, GA_PARENT),
        )
    };
    if calendar_alive {
        return Err("removing an owned foreign node destroys its object".to_owned());
    }
    if !picker_alive || picker_visible || picker_parent == window {
        return Err("removing a borrowed foreign node hides it and hands it back".to_owned());
    }
    drop(guest);
    // SAFETY: the picker is the application's; it destroys it.
    unsafe { DestroyWindow(picker) };
    Ok(())
}

fn main() {
    register_controls();
    if std::env::args().any(|argument| argument == "--self-test") {
        match self_test() {
            Ok(()) => println!("ok"),
            Err(problem) => {
                eprintln!("{problem}");
                std::process::exit(1);
            }
        }
        return;
    }
    let mut application =
        Application::new(Planner::new(()), Window::new("Planner", Size::new(480, 480)));
    if let Err(error) = WindowsPlatform::new().run(&mut application) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
