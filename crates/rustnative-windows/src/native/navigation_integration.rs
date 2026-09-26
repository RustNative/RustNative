//! Milestone 30 native integration tests: hidden screens, native tab bars,
//! lifecycle flushing, second-instance deep links, and window placement —
//! against real windows, through the production message loop.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use rustnative_core::{
    Application, Component, ComponentContext, Event, LayoutStyle, Lifecycle, MemoryStateStore,
    Node, Persisted, Services, Size, StateStore, Window, WindowId,
};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::UI::Controls::TCM_GETITEMRECT;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetFocus, VK_TAB};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, IsWindowVisible, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW, SetWindowPos,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_QUERYENDSESSION, WM_TIMER,
};

use super::harness::NativeHarness;
use super::lifecycle::{FLUSH_TIMER_ID, saved_placement};
use super::single_instance::{Claim, claim, forward};

#[derive(Clone, PartialEq, Default)]
struct Log {
    events: Rc<RefCell<Vec<String>>>,
}

/// Two tabs, each a page with a button; only the selected page is shown.
/// Also persists how many times a tab was chosen, and logs lifecycle
/// events and deep links.
struct Tabbed {
    log: Log,
    selected: usize,
    choices: Option<Persisted<u32>>,
}

impl Component for Tabbed {
    type Props = Log;
    type Message = ();
    fn new(log: Log) -> Self {
        Self { log, selected: 0, choices: None }
    }
    fn props(&self) -> &Log {
        &self.log
    }
    fn set_props(&mut self, log: Log) {
        self.log = log;
    }
    fn view(&self) -> Node {
        Node::label("unused", "")
    }

    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        self.choices = Some(context.persisted("choices", 0u32));
        Node::column(
            "root",
            [
                Node::tab_bar("tabs", ["First", "Second"], self.selected, LayoutStyle::new()),
                Node::column("page-0", [Node::button("button-0", "On the first page")])
                    .hidden(self.selected != 0),
                Node::column("page-1", [Node::button("button-1", "On the second page")])
                    .hidden(self.selected != 1),
            ],
        )
    }

    fn update(&mut self, event: Event) {
        let mut events = self.log.events.borrow_mut();
        match event {
            Event::TabSelected { index, .. } => {
                events.push(format!("tab {index}"));
                self.selected = index;
                if let Some(choices) = &self.choices {
                    choices.update(|count| *count += 1);
                }
            }
            Event::Lifecycle(lifecycle) => events.push(format!("{lifecycle:?}")),
            Event::DeepLink { url } => events.push(format!("link {url}")),
            _ => {}
        }
    }
}

fn application(log: &Log, store: &MemoryStateStore) -> Application {
    let services = Services::default().with_state_store(Arc::new(store.clone()));
    Application::with_services(
        Tabbed::new(log.clone()),
        Window::new("tabs", Size::new(360, 300)),
        services,
    )
}

fn visible(hwnd: HWND) -> bool {
    // SAFETY: `IsWindowVisible` accepts any handle value.
    unsafe { IsWindowVisible(hwnd) != 0 }
}

fn rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is a live window; `rect` is exclusively borrowed.
    let read = unsafe { GetWindowRect(hwnd, &raw mut rect) } != 0;
    assert!(read, "GetWindowRect on a live window must succeed");
    rect
}

/// Clicks tab `index` of the tab control the way a mouse does: a press and
/// release in the middle of that tab's own rectangle.
fn click_tab(harness: &mut NativeHarness, tabs: HWND, index: usize) {
    let mut item = RECT::default();
    // SAFETY: `tabs` is a live tab control; `item` receives the tab's
    // rectangle in the control's client coordinates.
    let found = unsafe { SendMessageW(tabs, TCM_GETITEMRECT, index, (&raw mut item) as isize) };
    assert!(found != 0, "tab {index} exists");
    let (x, y) = ((item.left + item.right) / 2, (item.top + item.bottom) / 2);
    let point = (isize::try_from(y).unwrap() << 16) | isize::try_from(x).unwrap();
    harness.post(tabs, WM_LBUTTONDOWN, 1, point);
    harness.post(tabs, WM_LBUTTONUP, 0, point);
}

#[test]
fn choosing_a_tab_is_reported_and_only_its_page_is_shown() {
    let log = Log::default();
    let store = MemoryStateStore::new();
    let mut application = application(&log, &store);
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let tabs = harness.expect_control(WindowId::PRIMARY, "tabs");
    let page_0 = harness.expect_control(WindowId::PRIMARY, "page-0");
    let page_1 = harness.expect_control(WindowId::PRIMARY, "page-1");
    assert!(visible(page_0) && !visible(page_1), "the second page starts hidden");

    click_tab(&mut harness, tabs, 1);

    assert_eq!(*log.events.borrow(), vec!["tab 1".to_owned()]);
    assert_eq!(super::rendering::tabs::selection(tabs), Some(1));
    assert!(!visible(page_0) && visible(page_1), "the pages swapped");
    assert_eq!(
        harness.expect_control(WindowId::PRIMARY, "page-0"),
        page_0,
        "a hidden page is kept, not destroyed"
    );
}

#[test]
fn a_hidden_page_is_skipped_by_tab_traversal() {
    let log = Log::default();
    let store = MemoryStateStore::new();
    let mut application = application(&log, &store);
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let hidden_button = harness.expect_control(WindowId::PRIMARY, "button-1");

    // Tab through every stop, twice round.
    let mut reached_hidden = false;
    for _ in 0..6 {
        harness.press_key(WindowId::PRIMARY, u32::from(VK_TAB));
        // SAFETY: `GetFocus` takes no arguments.
        reached_hidden |= unsafe { GetFocus() } == hidden_button;
    }
    assert!(!reached_hidden, "focus never lands on a control nobody can see");
}

#[test]
fn ending_the_session_flushes_state_before_the_application_is_told() {
    let log = Log::default();
    let store = MemoryStateStore::new();
    let mut application = application(&log, &store);
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let tabs = harness.expect_control(WindowId::PRIMARY, "tabs");
    click_tab(&mut harness, tabs, 1);
    assert!(store.keys().iter().all(|key| !key.ends_with("#choices")), "buffered, not written");

    let window = harness.hwnd(WindowId::PRIMARY);
    let answer = harness.send(window, WM_QUERYENDSESSION, 0, 0);

    assert_eq!(answer, 1, "the session may end");
    let key = store.keys().into_iter().find(|key| key.ends_with("#choices")).expect("written");
    assert_eq!(store.load(&key).unwrap().as_deref(), Some(&b"1"[..]));
    assert!(log.events.borrow().contains(&format!("{:?}", Lifecycle::Terminating)));
}

#[test]
fn low_memory_flushes_state_before_the_application_is_told() {
    let log = Log::default();
    let store = MemoryStateStore::new();
    let mut application = application(&log, &store);
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let tabs = harness.expect_control(WindowId::PRIMARY, "tabs");
    click_tab(&mut harness, tabs, 1);

    // What the watcher thread posts when the memory resource notification
    // is signalled; the notification itself cannot be provoked in a test.
    let window = harness.hwnd(WindowId::PRIMARY);
    harness.send(window, super::memory_watch::WM_FRAMEWORK_LOW_MEMORY, 0, 0);

    assert!(store.keys().iter().any(|key| key.ends_with("#choices")), "flushed first");
    assert!(log.events.borrow().contains(&format!("{:?}", Lifecycle::LowMemory)));
}

#[test]
fn writes_are_flushed_once_they_go_quiet() {
    let log = Log::default();
    let store = MemoryStateStore::new();
    let mut application = application(&log, &store);
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let tabs = harness.expect_control(WindowId::PRIMARY, "tabs");
    click_tab(&mut harness, tabs, 1);

    // What the idle-flush timer delivers once it elapses.
    let window = harness.hwnd(WindowId::PRIMARY);
    harness.send(window, WM_TIMER, FLUSH_TIMER_ID, 0);
    assert!(store.keys().iter().any(|key| key.ends_with("#choices")));
}

#[test]
fn a_second_launch_hands_its_link_to_the_first_instance() {
    let app_id = format!("rust-native-test.{}", std::process::id());
    let Claim::First(_instance) = claim(&app_id) else { panic!("this process is the first") };
    assert!(matches!(claim(&app_id), Claim::AlreadyRunning), "a second claim sees the first");

    let log = Log::default();
    let store = MemoryStateStore::new();
    let mut application = application(&log, &store);
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };

    // What `run_application` does in a second process: hand the URL over.
    assert!(forward(&app_id, "myapp://open/7"), "the running instance's listener was found");
    harness.pump();

    assert!(log.events.borrow().contains(&"link myapp://open/7".to_owned()));
}

#[test]
fn the_primary_window_reopens_where_it_was_closed() {
    let store = MemoryStateStore::new();
    let placed = {
        let log = Log::default();
        let mut application = application(&log, &store);
        // SAFETY: `application` is declared first, so it outlives the harness.
        let mut harness = unsafe { NativeHarness::attach(&mut application) };
        let window = harness.hwnd(WindowId::PRIMARY);
        // SAFETY: `window` is live; plain values.
        unsafe {
            SetWindowPos(
                window,
                std::ptr::null_mut(),
                140,
                120,
                420,
                330,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        let placed = rect(window);
        harness.request_close(WindowId::PRIMARY);
        placed
    };
    let (saved, maximized) = saved_placement(&store).expect("saved when the window closed");
    assert_eq!(
        (saved.left, saved.top, saved.right, saved.bottom),
        (placed.left, placed.top, placed.right, placed.bottom)
    );
    assert!(!maximized);

    let log = Log::default();
    let mut application = application(&log, &store);
    // SAFETY: `application` is declared first, so it outlives the harness.
    let harness = unsafe { NativeHarness::attach(&mut application) };
    let reopened = rect(harness.hwnd(WindowId::PRIMARY));
    assert_eq!(
        (reopened.left, reopened.top, reopened.right, reopened.bottom),
        (placed.left, placed.top, placed.right, placed.bottom),
        "restored to the same place and size"
    );
}
