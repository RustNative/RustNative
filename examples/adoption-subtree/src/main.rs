//! The embedded-subtree rung of the adoption ladder (`PLAN.md` Milestone
//! 40): an existing Win32 application — its own window class, window, and
//! message loop, written against `windows-sys` and nothing else — hosts a
//! Rust Native subtree in part of its window.
//!
//! Three things in the host change to adopt it: `WindowsPlatform::embed`
//! after the host window exists, `handle_message` in the loop, and
//! `set_bounds` when the host lays itself out. Run with `--self-test` to
//! drive it and exit (what `tests/subtree.rs` does).

use rustnative_core::{Application, Component, Event, Node, NodeId, Size, Window};
use rustnative_windows::{EmbeddedRoot, WindowsPlatform};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_CLICK, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW,
    DestroyWindow, DispatchMessageW, EnumChildWindows, GetClientRect, GetMessageW, GetWindowTextW,
    IsWindow, MSG, PM_REMOVE, PeekMessageW, PostMessageW, PostQuitMessage, RegisterClassW, SW_SHOW,
    SWP_NOZORDER, SendMessageW, SetWindowPos, ShowWindow, TranslateMessage, WM_APP, WM_DESTROY,
    WM_SIZE, WNDCLASSW, WS_CLIPCHILDREN, WS_OVERLAPPEDWINDOW,
};

/// Posted by the host's own window procedure when it resizes, so the loop
/// (which holds the embedded root) lays the subtree out again.
const WM_HOST_RELAYOUT: u32 = WM_APP + 7;
/// The strip the host keeps for itself above the embedded subtree.
const HOST_BAND: i32 = 40;

/// The Rust Native part: a counter.
struct Counter {
    count: u32,
}

impl Component for Counter {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { count: 0 }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column(
            "root",
            [
                Node::label("count", format!("Count: {}", self.count)),
                Node::button("increment", "Increment"),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { target } if target == NodeId::from_key("increment")) {
            self.count += 1;
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn host_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_SIZE => {
            // SAFETY: posting to this thread's own live window.
            unsafe { PostMessageW(hwnd, WM_HOST_RELAYOUT, 0, 0) };
            0
        }
        WM_DESTROY => {
            // SAFETY: takes an exit code only.
            unsafe { PostQuitMessage(0) };
            0
        }
        // SAFETY: exactly what Windows delivered.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// The host's own window, created the way it always was.
fn create_host_window() -> HWND {
    let class = wide("AdoptionSubtreeHost");
    let title = wide("An existing application");
    // SAFETY: plain Win32 class registration and window creation with
    // NUL-terminated names that outlive the calls; `COLOR_WINDOW + 1` is the
    // documented system-colour brush encoding.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class_info = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(host_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            hbrBackground: 6 as _,
            ..std::mem::zeroed()
        };
        RegisterClassW(&raw const class_info);
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            480,
            320,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    }
}

fn client_size(hwnd: HWND) -> (i32, i32) {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a writable `RECT`; `hwnd` is a window of this thread.
    unsafe { GetClientRect(hwnd, &raw mut rect) };
    (rect.right - rect.left, rect.bottom - rect.top)
}

struct Search {
    text: Vec<u16>,
    found: Option<HWND>,
}

unsafe extern "system" fn visit(hwnd: HWND, search: LPARAM) -> i32 {
    // SAFETY: `search` is the `&mut Search` `find_child` passes, alive for
    // the enumeration.
    let search = unsafe { &mut *(search as *mut Search) };
    let mut buffer = [0_u16; 128];
    // SAFETY: `buffer` is writable for its length.
    let length = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), 128) };
    if usize::try_from(length).is_ok_and(|length| buffer[..length] == search.text[..]) {
        search.found = Some(hwnd);
        return 0;
    }
    1
}

/// The first descendant window whose text is `text`.
fn find_child(parent: HWND, text: &str) -> Option<HWND> {
    let mut search = Search { text: text.encode_utf16().collect(), found: None };
    // SAFETY: `visit` only uses `search` through the pointer passed here,
    // for the duration of the call.
    unsafe { EnumChildWindows(parent, Some(visit), (&raw mut search) as LPARAM) };
    search.found
}

/// The host's loop body, with the one line the adoption adds.
fn dispatch(root: &mut EmbeddedRoot<'_>, host: HWND, message: &MSG) {
    if message.message == WM_HOST_RELAYOUT {
        let (width, height) = client_size(host);
        root.set_bounds(0, HOST_BAND, width, height - HOST_BAND);
        return;
    }
    // Added for the adoption: the framework's windows take their messages.
    if root.handle_message(message).unwrap_or(false) {
        return;
    }
    // SAFETY: an ordinary retrieved message.
    unsafe {
        TranslateMessage(message);
        DispatchMessageW(message);
    }
}

fn pump(root: &mut EmbeddedRoot<'_>, host: HWND) {
    let mut message = MSG::default();
    // SAFETY: `message` is a writable `MSG`.
    while unsafe { PeekMessageW(&raw mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
        dispatch(root, host, &message);
    }
}

fn check_fills(root: &EmbeddedRoot<'_>, host: HWND, when: &str) -> Result<(), String> {
    let (width, height) = client_size(root.hwnd());
    let (host_width, host_height) = client_size(host);
    if (width, height) == (host_width, host_height - HOST_BAND) {
        Ok(())
    } else {
        Err(format!(
            "{when}, the subtree fills the host's area: {width}x{height} in {host_width}x{host_height}"
        ))
    }
}

fn self_test(root: &mut EmbeddedRoot<'_>, host: HWND) -> Result<(), String> {
    pump(root, host);
    check_fills(root, host, "at first")?;
    let button =
        find_child(host, "Increment").ok_or("the embedded button is a child of the host")?;
    // SAFETY: clicking a live button of this thread.
    unsafe { SendMessageW(button, BM_CLICK, 0, 0) };
    pump(root, host);
    find_child(host, "Count: 1")
        .ok_or("the click reached the component and the label re-rendered")?;
    // The host resizes; the subtree follows.
    // SAFETY: resizing this thread's own window.
    unsafe { SetWindowPos(host, std::ptr::null_mut(), 0, 0, 640, 480, SWP_NOZORDER) };
    pump(root, host);
    check_fills(root, host, "after a resize")
}

fn main() {
    let self_testing = std::env::args().any(|argument| argument == "--self-test");
    let host = create_host_window();
    // SAFETY: showing this thread's own window.
    unsafe { ShowWindow(host, SW_SHOW) };

    let mut application =
        Application::new(Counter::new(()), Window::new("Counter", Size::new(480, 280)));
    // Added for the adoption: the subtree, inside the host's window.
    let mut root = match WindowsPlatform::new().embed(host, &mut application) {
        Ok(root) => root,
        Err(error) => {
            eprintln!("embedding failed: {error}");
            std::process::exit(2);
        }
    };
    let (width, height) = client_size(host);
    root.set_bounds(0, HOST_BAND, width, height - HOST_BAND);

    if self_testing {
        let result = self_test(&mut root, host);
        let embedded = root.hwnd();
        drop(root);
        // SAFETY: `IsWindow` accepts any handle.
        let (host_alive, embedded_alive) =
            unsafe { (IsWindow(host) != 0, IsWindow(embedded) != 0) };
        let result = result.and_then(|()| {
            if host_alive && !embedded_alive {
                Ok(())
            } else {
                Err("dropping the root destroys the framework's window and nothing of the host's"
                    .to_owned())
            }
        });
        // SAFETY: destroying this thread's own window.
        unsafe { DestroyWindow(host) };
        match result {
            Ok(()) => println!("ok"),
            Err(problem) => {
                eprintln!("{problem}");
                std::process::exit(1);
            }
        }
        return;
    }

    let mut message = MSG::default();
    // SAFETY: the host's ordinary loop.
    while unsafe { GetMessageW(&raw mut message, std::ptr::null_mut(), 0, 0) } > 0 {
        dispatch(&mut root, host, &message);
    }
}
