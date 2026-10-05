//! Milestone 25 native integration tests: advanced input against real
//! windows, real window procedures, and the production message loop.
//!
//! Like `native::integration`, every scenario drives the backend through
//! [`NativeHarness`], so messages travel the same `handle_message` →
//! pre-dispatch → `DispatchMessageW` path real input does. Where a scenario
//! needs a device this machine may not have (a touch screen, a controller),
//! the test stands in for the *device* — a `POINTER_INFO` describing a
//! contact, a controller source returning snapshots — and nothing else:
//! targeting, capture, gesture recognition, dispatch, and rendering are the
//! production code.
//!
//! # Tests that need the interactive desktop
//!
//! Two behaviors are decided by system state a test process cannot fake:
//! hover (`WM_MOUSELEAVE` fires from where the *real* cursor is) and the
//! system clipboard. Their tests are `#[ignore]`d with a reason naming what
//! they need, rather than degraded into passing without checking anything,
//! and CI runs them explicitly (`cargo test -- --ignored` on the Windows
//! runner). A development shell that is denied the clipboard
//! (`OpenClipboard` failing with `ERROR_ACCESS_DENIED`, as a job object with
//! UI restrictions does) reports them as ignored, not passed.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use rustnative_core::{
    AccessibilityInfo, AccessibilityRole, Application, ColumnStyle, Component, ComponentContext,
    DropEffect, Event, GamepadButton, GamepadSource, GamepadState, InputInterest, InputRequests,
    LayoutStyle, Node, NodeId, Overflow, PointerPhase, Size, SizeMode, Window, WindowId,
};
use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetCapture, GetFocus, GetKeyboardState, ReleaseCapture, SetActiveWindow, SetFocus,
    SetKeyboardState, VK_CONTROL,
};
use windows_sys::Win32::UI::Input::Pointer::POINTER_INFO;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_TIMER,
};

use super::harness::NativeHarness;
use super::input::gamepad::{GAMEPAD_TIMER_ID, GamepadInputState};
use super::input::{gamepad, ime, pointer};

type Log = Rc<RefCell<Vec<String>>>;

const KEYS: [&str; 7] = ["root", "pad", "plain", "wheelzone", "scroller", "zone", "editor"];

fn name(id: NodeId) -> String {
    KEYS.iter()
        .find(|key| NodeId::from_key(key) == id)
        .map_or_else(|| format!("#{}", id.get()), |key| (*key).to_owned())
}

fn describe(event: &Event) -> String {
    match event {
        Event::PointerDown { target, pointer } => {
            format!("down:{}@{},{}", name(*target), pointer.position().x, pointer.position().y)
        }
        Event::PointerMove { target, pointer } => {
            format!("move:{}@{},{}", name(*target), pointer.position().x, pointer.position().y)
        }
        Event::PointerUp { target, pointer } => {
            format!("up:{}@{},{}", name(*target), pointer.position().x, pointer.position().y)
        }
        Event::PointerCancel { target, .. } => format!("cancel:{}", name(*target)),
        Event::PointerEnter { target } => format!("enter:{}", name(*target)),
        Event::PointerLeave { target } => format!("leave:{}", name(*target)),
        Event::Wheel { target, delta } => format!("wheel:{}:{delta:?}", name(*target)),
        Event::Gesture { target, gesture } => format!("gesture:{}:{gesture:?}", name(*target)),
        Event::Clipboard { action, .. } => format!("clip:{action:?}"),
        Event::ClipboardChanged { .. } => "clipboard-changed".to_owned(),
        Event::KeyUp { key, .. } => format!("keyup:{key:?}"),
        Event::KeyDown { key, .. } => format!("keydown:{key:?}"),
        Event::Composition { composition, .. } => format!("ime:{composition:?}"),
        Event::TextInput { text, .. } => format!("text:{text}"),
        Event::DragEnter { target, data, .. } => {
            format!("dragenter:{}:{}", name(*target), data.files().len())
        }
        Event::DragOver { target, .. } => format!("dragover:{}", name(*target)),
        Event::DragLeave { target } => format!("dragleave:{}", name(*target)),
        Event::Drop { target, data, .. } => format!("drop:{}:{:?}", name(*target), data.files()),
        Event::Gamepad { target, gamepad, input } => {
            format!("gamepad:{}:{gamepad}:{input:?}", name(*target))
        }
        other => format!("other:{other:?}"),
    }
}

#[derive(Clone, PartialEq)]
struct ProbeProps {
    log: Log,
    renders: Rc<Cell<u32>>,
    capture_on_down: bool,
}

/// One component exercising every M25 stream: a pointer/gesture/gamepad
/// pad, an uninterested label, a wheel-interested zone, a scroll container,
/// a drop zone, and a focusable custom editor.
struct Probe {
    props: ProbeProps,
    input: Option<InputRequests>,
}

fn fixed(width: i32, height: i32) -> LayoutStyle {
    LayoutStyle::new().width(SizeMode::Fixed(width)).height(SizeMode::Fixed(height))
}

impl Component for Probe {
    type Props = ProbeProps;
    type Message = ();

    fn new(props: ProbeProps) -> Self {
        Self { props, input: None }
    }
    fn props(&self) -> &ProbeProps {
        &self.props
    }
    fn set_props(&mut self, props: ProbeProps) {
        self.props = props;
    }

    fn view(&self) -> Node {
        let tall = (0..12)
            .map(|i| Node::label_with_layout(format!("row{i}"), format!("row {i}"), fixed(90, 24)));
        Node::column(
            "root",
            [
                Node::column_with_layout("pad", [], fixed(120, 80), ColumnStyle::new())
                    .with_input(InputInterest::new().pointer().gestures().gamepad()),
                Node::label_with_layout("plain", "plain", fixed(120, 24)),
                Node::column_with_layout("wheelzone", [], fixed(120, 50), ColumnStyle::new())
                    .with_input(InputInterest::new().wheel()),
                Node::column_with_layout(
                    "scroller",
                    tall,
                    fixed(120, 60),
                    ColumnStyle::new().overflow(Overflow::Scroll),
                ),
                Node::column_with_layout("zone", [], fixed(120, 50), ColumnStyle::new())
                    .with_input(InputInterest::new().drop_target()),
                Node::column_with_layout("editor", [], fixed(120, 30), ColumnStyle::new())
                    .with_accessibility(
                        AccessibilityInfo::new(AccessibilityRole::TextInput).focusable(true),
                    ),
            ],
        )
    }

    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        self.props.renders.set(self.props.renders.get() + 1);
        self.input = Some(context.input());
        self.view()
    }

    fn update(&mut self, event: Event) {
        let input = self.input.as_ref().expect("rendered before any event");
        match &event {
            Event::PointerDown { pointer, .. } if self.props.capture_on_down => {
                input.capture_pointer("pad", pointer.pointer_id());
            }
            Event::PointerUp { pointer, .. } if self.props.capture_on_down => {
                input.release_pointer("pad", pointer.pointer_id());
            }
            Event::DragEnter { data, .. } | Event::DragOver { data, .. } => {
                let effect =
                    if data.files().is_empty() { DropEffect::None } else { DropEffect::Copy };
                input.set_drop_effect(effect);
            }
            _ => {}
        }
        self.props.log.borrow_mut().push(describe(&event));
    }
}

struct Fixture {
    log: Log,
    renders: Rc<Cell<u32>>,
}

impl Fixture {
    fn new() -> Self {
        Self { log: Log::default(), renders: Rc::new(Cell::new(0)) }
    }

    fn application(&self, capture_on_down: bool) -> Application {
        let props =
            ProbeProps { log: self.log.clone(), renders: self.renders.clone(), capture_on_down };
        Application::new(Probe::new(props), Window::new("input", Size::new(480, 640)))
    }

    fn entries(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.log.borrow_mut())
    }
}

fn client_point(x: i32, y: i32) -> LPARAM {
    // `MAKELPARAM`: two 16-bit coordinates, x in the low word.
    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "packing two 16-bit words into an LPARAM is a bit-level operation"
    )]
    let packed = (((y as u32 & 0xffff) << 16) | (x as u32 & 0xffff)) as LPARAM;
    packed
}

fn screen_rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is a live control; `rect` is exclusively borrowed.
    let read = unsafe { GetWindowRect(hwnd, &raw mut rect) } != 0;
    assert!(read, "GetWindowRect on a live control must succeed");
    rect
}

fn screen_center(hwnd: HWND) -> POINT {
    let rect = screen_rect(hwnd);
    POINT { x: (rect.left + rect.right) / 2, y: (rect.top + rect.bottom) / 2 }
}

fn wheel_wparam(delta: i16) -> WPARAM {
    // The delta is the signed high word; reinterpreting its bits is the
    // documented packing.
    #[allow(clippy::cast_sign_loss)]
    let bits = delta as u16;
    usize::from(bits) << 16
}

/// Pointer samples reach the node that asked for them, in that node's own
/// coordinates, and never a node that did not ask; a press and release
/// without movement is recognized as a tap.
///
/// Catches: coordinates left in the addressed window's space (off by the
/// node's position), a pointer stream that leaks to every node, and gesture
/// recognition never fed from the mouse.
#[test]
fn native_pointer_events_reach_only_the_interested_node_in_local_coordinates() {
    let fixture = Fixture::new();
    let mut application = fixture.application(false);
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let pad = harness.expect_control(WindowId::PRIMARY, "pad");
    let plain = harness.expect_control(WindowId::PRIMARY, "plain");
    fixture.take();

    harness.post(pad, WM_LBUTTONDOWN, 0x0001, client_point(10, 20));
    harness.post(pad, WM_MOUSEMOVE, 0x0001, client_point(13, 22));
    harness.post(pad, WM_LBUTTONUP, 0, client_point(13, 22));
    harness.post(plain, WM_LBUTTONDOWN, 0x0001, client_point(3, 3));
    harness.post(plain, WM_LBUTTONUP, 0, client_point(3, 3));
    // Hover is decided by where the real cursor is, which this test does not
    // control; `native_pointer_hover_follows_the_real_cursor` covers it.
    let log: Vec<String> = fixture
        .take()
        .into_iter()
        .filter(|entry| !entry.starts_with("enter:") && !entry.starts_with("leave:"))
        .collect();
    assert_eq!(
        log,
        vec![
            "down:pad@10,20",
            "move:pad@13,22",
            "up:pad@13,22",
            "gesture:pad:Tap { position: Point { x: 10, y: 20 } }",
        ],
        "nothing is delivered for the label, which declared no pointer interest"
    );
}

/// Hover enter/leave follow the real cursor between an interested node and
/// an uninterested one.
///
/// Catches: `WM_MOUSELEAVE` tracking never armed, and a hover that sticks
/// after the pointer has moved on.
#[test]
#[ignore = "needs the interactive desktop: hover follows the real cursor (see module docs)"]
fn native_pointer_hover_follows_the_real_cursor() {
    let fixture = Fixture::new();
    let mut application = fixture.application(false);
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let pad = harness.expect_control(WindowId::PRIMARY, "pad");
    let plain = harness.expect_control(WindowId::PRIMARY, "plain");
    fixture.take();

    // Hover is decided from where the *real* cursor is (that is how
    // `WM_MOUSELEAVE` works), so this test puts the real cursor where its
    // synthetic input says it is, over a window kept topmost for the
    // duration. The cursor is restored afterwards.
    let _cursor = RealCursor::save();
    make_topmost(harness.hwnd(WindowId::PRIMARY));
    RealCursor::move_to(pad, 10, 20);
    harness.pump();
    let hover = fixture.take();
    assert_eq!(hover.first().map(String::as_str), Some("enter:pad"), "{hover:?}");
    assert!(hover.contains(&"move:pad@10,20".to_owned()), "{hover:?}");

    RealCursor::move_to(plain, 3, 3);
    harness.pump();
    assert_eq!(fixture.take(), vec!["leave:pad"], "moving onto the label ends the pad's hover");
}

/// The real cursor, saved on creation and restored on drop.
struct RealCursor(POINT);

impl RealCursor {
    fn save() -> Self {
        let mut point = POINT { x: 0, y: 0 };
        // SAFETY: `point` is a valid, exclusively borrowed `POINT`.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos(&raw mut point) };
        Self(point)
    }

    /// Moves the cursor to client point `(x, y)` of `hwnd`.
    fn move_to(hwnd: HWND, x: i32, y: i32) {
        let origin = screen_rect(hwnd);
        // SAFETY: `SetCursorPos` takes two plain integers.
        let moved = unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos(
                origin.left + x,
                origin.top + y,
            )
        } != 0;
        assert!(moved, "the test must be able to position the cursor");
    }
}

impl Drop for RealCursor {
    fn drop(&mut self) {
        // SAFETY: as above.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos(self.0.x, self.0.y) };
    }
}

fn make_topmost(hwnd: HWND) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    };
    // SAFETY: `hwnd` is this test's live top-level window; the flags keep
    // its size and position.
    let placed =
        unsafe { SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE) } != 0;
    assert!(placed, "the test window must be able to go topmost");
}

/// A captured pointer keeps delivering to its node outside the node's
/// bounds, capture is held natively on the top-level window, releasing it
/// ends that, and a capture Windows takes away becomes a cancel.
///
/// Catches: capture implemented only as bookkeeping (moves outside the node
/// go nowhere), capture never released, and a lost capture the component is
/// never told about — the drag that sticks to the cursor forever.
#[test]
fn native_pointer_capture_routes_outside_the_node_and_loss_cancels() {
    let fixture = Fixture::new();
    let mut application = fixture.application(true);
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let top = harness.hwnd(WindowId::PRIMARY);
    let pad = harness.expect_control(WindowId::PRIMARY, "pad");
    let plain = harness.expect_control(WindowId::PRIMARY, "plain");
    let pad_origin = screen_rect(pad);
    let plain_origin = screen_rect(plain);
    fixture.take();

    harness.post(pad, WM_LBUTTONDOWN, 0x0001, client_point(5, 5));
    // SAFETY: `GetCapture` takes no arguments.
    assert_eq!(unsafe { GetCapture() }, top, "capture is held by the top-level window");

    // A move over a different node, as Windows addresses it while captured.
    harness.post(plain, WM_MOUSEMOVE, 0x0001, client_point(2, 2));
    let expected_x = plain_origin.left + 2 - pad_origin.left;
    let expected_y = plain_origin.top + 2 - pad_origin.top;
    assert!(
        fixture.entries().contains(&format!("move:pad@{expected_x},{expected_y}")),
        "a captured move is delivered to the capturing node, in its coordinates: {:?}",
        fixture.entries()
    );

    harness.post(pad, WM_LBUTTONUP, 0, client_point(5, 5));
    // SAFETY: as above.
    assert!(unsafe { GetCapture() }.is_null(), "releasing capture releases it natively");

    fixture.take();
    harness.post(pad, WM_LBUTTONDOWN, 0x0001, client_point(5, 5));
    // Something else takes the mouse away mid-drag.
    // SAFETY: releases this thread's capture; no arguments.
    let _ = unsafe { ReleaseCapture() };
    harness.pump();
    let log = fixture.take();
    assert!(log.contains(&"cancel:pad".to_owned()), "a lost capture is a cancel: {log:?}");
    assert!(
        harness
            .with_runtime(WindowId::PRIMARY, |runtime| runtime
                .input
                .pointer
                .captured(pointer::MOUSE_POINTER_ID))
            .is_none()
    );
}

/// A wheel over a wheel-interested node is delivered to it (and consumed);
/// over a scroll container the container scrolls natively with no render.
///
/// Catches: the wheel stream swallowing container scrolling (or the other
/// way round), the vertical sign convention inverted, and a scroll that
/// rerenders the component tree on every notch (`PLAN.md` §2.9).
#[test]
fn native_wheel_goes_to_interested_nodes_and_scrolls_containers_without_rendering() {
    let fixture = Fixture::new();
    let mut application = fixture.application(false);
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let top = harness.hwnd(WindowId::PRIMARY);
    let zone = harness.expect_control(WindowId::PRIMARY, "wheelzone");
    let scroller = harness.expect_control(WindowId::PRIMARY, "scroller");
    fixture.take();

    let at = screen_center(zone);
    harness.post(top, WM_MOUSEWHEEL, wheel_wparam(120), client_point(at.x, at.y));
    assert_eq!(
        fixture.take(),
        vec!["wheel:wheelzone:Lines { x: 0, y: -120 }"],
        "one notch away from the user scrolls content up: negative y in reading direction"
    );

    let first_row = harness.expect_control(WindowId::PRIMARY, "row0");
    let before = screen_rect(first_row).top;
    let renders = fixture.renders.get();
    let at = screen_center(scroller);
    harness.post(top, WM_MOUSEWHEEL, wheel_wparam(-120), client_point(at.x, at.y));
    assert!(fixture.take().is_empty(), "a container scroll is not an event");
    assert_eq!(fixture.renders.get(), renders, "scrolling must not rerender");
    assert!(screen_rect(first_row).top < before, "the container's content moved up natively");
}

/// Holds Ctrl in this thread's keyboard state for its lifetime, as a real
/// Ctrl press would have — which is what `GetKeyState` reads.
struct CtrlHeld([u8; 256]);

impl CtrlHeld {
    fn press() -> Self {
        let mut keys = [0u8; 256];
        // SAFETY: `keys` is the 256-byte buffer both calls require.
        unsafe { GetKeyboardState(keys.as_mut_ptr()) };
        let saved = keys;
        keys[usize::from(VK_CONTROL)] = 0x80;
        // SAFETY: as above; affects only this thread's view of the keyboard.
        unsafe { SetKeyboardState(keys.as_ptr()) };
        Self(saved)
    }
}

impl Drop for CtrlHeld {
    fn drop(&mut self) {
        // SAFETY: restores the state saved in `press`.
        unsafe { SetKeyboardState(self.0.as_ptr()) };
    }
}

/// Ctrl+C on the focused node delivers the key press, then a copy; the key
/// release arrives as `KeyUp`. No clipboard access is needed for either.
///
/// Catches: the shortcut table missing the modifier check, and key releases
/// dropped on the floor.
#[test]
fn native_copy_shortcut_and_key_up() {
    let fixture = Fixture::new();
    let mut application = fixture.application(false);
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let top = harness.hwnd(WindowId::PRIMARY);
    fixture.take();

    {
        let _ctrl = CtrlHeld::press();
        harness.post(top, WM_KEYDOWN, usize::from(b'C'), 0);
        harness.post(top, WM_KEYUP, usize::from(b'C'), 0);
    }
    harness.post(top, WM_KEYDOWN, usize::from(b'C'), 0);
    assert_eq!(
        fixture.take(),
        vec![
            "keydown:Character('C')",
            "clip:Copy",
            "keyup:Character('C')",
            "keydown:Character('C')",
            "text:c",
        ],
        "C without Ctrl is a key and a character, not a copy; Ctrl+C types nothing"
    );
}

/// Ctrl+V on the focused node delivers the key press and then a paste
/// carrying the clipboard's text, and the clipboard change itself is
/// reported to the window's root.
///
/// Catches: a paste reported without its text, and a clipboard listener that
/// was never registered.
#[test]
#[ignore = "needs clipboard access, which a UI-restricted job object denies (see module docs)"]
fn native_clipboard_paste_and_change_notification() {
    let fixture = Fixture::new();
    let mut application = fixture.application(false);
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let top = harness.hwnd(WindowId::PRIMARY);
    fixture.take();

    let _clipboard = crate::services::clipboard::test_lock();
    let text = "pasted from the M25 test";
    let mut written = false;
    for _ in 0..20 {
        // Another process may hold the clipboard open momentarily.
        if crate::services::clipboard::write_text_now(text).is_ok() {
            written = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(written, "the test must be able to write the clipboard");
    for _ in 0..40 {
        harness.pump();
        if fixture.entries().contains(&"clipboard-changed".to_owned()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(
        fixture.take().contains(&"clipboard-changed".to_owned()),
        "WM_CLIPBOARDUPDATE must reach the root as ClipboardChanged"
    );

    {
        let _ctrl = CtrlHeld::press();
        harness.post(top, WM_KEYDOWN, usize::from(b'V'), 0);
    }
    assert_eq!(
        fixture.take(),
        vec!["keydown:Character('V')".to_owned(), format!("clip:Paste {{ text: Some({text:?}) }}")]
    );
}

/// A real Shell data object for two real files, driven through the
/// window's registered `IDropTarget` exactly as OLE drives it: enter over
/// the drop zone is accepted as the component answered, a drag over a node
/// that is not a drop target is refused, and the drop delivers the paths.
///
/// Catches: a drop target never registered, `CF_HDROP` misread, the
/// component's answer lost between dispatch and the return to OLE, and a
/// drop delivered to a node that never accepted it.
#[test]
fn native_drop_target_negotiates_and_delivers_files() {
    use windows::Win32::Foundation::POINTL;
    use windows::Win32::System::Com::{CoTaskMemFree, IDataObject};
    use windows::Win32::System::Ole::{
        DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_MOVE, DROPEFFECT_NONE,
    };
    use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{
        BHID_DataObject, IShellItemArray, SHCreateShellItemArrayFromIDLists, SHParseDisplayName,
    };
    use windows::core::HSTRING;

    let fixture = Fixture::new();
    let mut application = fixture.application(false);
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let zone = harness.expect_control(WindowId::PRIMARY, "zone");
    let plain = harness.expect_control(WindowId::PRIMARY, "plain");
    let target = harness
        .with_runtime(WindowId::PRIMARY, |runtime| runtime.input.drag.registration.clone())
        .expect("every window registers a drop target");
    fixture.take();

    // The shell hands dropped files back by their long names, and `%TEMP%`
    // may be an 8.3 short one (CI's runners: `RUNNER~1`), so the expected
    // paths start from its long form, without `canonicalize`'s `\\?\`.
    let temp = std::fs::canonicalize(std::env::temp_dir()).unwrap();
    let temp = temp
        .to_str()
        .and_then(|path| path.strip_prefix(r"\\?\"))
        .map_or(temp.clone(), PathBuf::from);
    let dir = temp.join(format!("rustnative-m25-drop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let files: Vec<PathBuf> = ["first.txt", "second.txt"].iter().map(|f| dir.join(f)).collect();
    let mut pidls: Vec<*mut ITEMIDLIST> = Vec::new();
    for file in &files {
        std::fs::write(file, b"m25").unwrap();
        let mut pidl = std::ptr::null_mut();
        // SAFETY: `pidl` receives a Shell-allocated ID list freed below.
        unsafe {
            SHParseDisplayName(
                &HSTRING::from(file.to_string_lossy().as_ref()),
                None,
                &raw mut pidl,
                0,
                None,
            )
        }
        .expect("parsing a real file path");
        pidls.push(pidl);
    }
    let absolute: Vec<*const ITEMIDLIST> = pidls.iter().map(|p| p.cast_const()).collect();
    // SAFETY: `absolute` holds valid absolute ID lists for the call.
    let array: IShellItemArray =
        unsafe { SHCreateShellItemArrayFromIDLists(&absolute) }.expect("shell item array");
    // SAFETY: `BHID_DataObject` asks for the items' `IDataObject`.
    let data: IDataObject =
        unsafe { array.BindToHandler(None, &BHID_DataObject) }.expect("shell data object");

    let point = |hwnd| {
        let center = screen_center(hwnd);
        POINTL { x: center.x, y: center.y }
    };
    let allowed = DROPEFFECT(DROPEFFECT_COPY.0 | DROPEFFECT_MOVE.0);

    let mut effect = DROPEFFECT(allowed.0);
    // SAFETY: `data` is a live data object and `effect` a valid slot.
    unsafe { target.DragEnter(&data, MODIFIERKEYS_FLAGS(0), point(zone), &raw mut effect) }
        .unwrap();
    assert_eq!(effect, DROPEFFECT_COPY, "the component accepted a copy");

    let mut effect = DROPEFFECT(allowed.0);
    // SAFETY: as above.
    unsafe { target.DragOver(MODIFIERKEYS_FLAGS(0), point(plain), &raw mut effect) }.unwrap();
    assert_eq!(effect, DROPEFFECT_NONE, "a node that is not a drop target refuses");

    let mut effect = DROPEFFECT(allowed.0);
    // SAFETY: as above.
    unsafe { target.DragOver(MODIFIERKEYS_FLAGS(0), point(zone), &raw mut effect) }.unwrap();
    assert_eq!(effect, DROPEFFECT_COPY);

    let mut effect = DROPEFFECT(allowed.0);
    // SAFETY: as above.
    unsafe { target.Drop(&data, MODIFIERKEYS_FLAGS(0), point(zone), &raw mut effect) }.unwrap();
    assert_eq!(effect, DROPEFFECT_COPY);
    harness.pump();

    assert_eq!(
        fixture.take(),
        vec![
            "dragenter:zone:2".to_owned(),
            "dragleave:zone".to_owned(),
            "dragenter:zone:2".to_owned(),
            format!("drop:zone:{files:?}"),
        ]
    );

    for pidl in pidls {
        // SAFETY: each was allocated by `SHParseDisplayName` and not freed.
        unsafe { CoTaskMemFree(Some(pidl.cast_const().cast())) };
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// A controller stand-in drives the production polling path: the timer is
/// armed only while a node wants gamepads, events reach that node only
/// while its window is active, and polling slows when nothing is connected.
///
/// Catches: a window that wakes 60 times a second for an application that
/// never asked for controllers, and a background window reacting to input
/// meant for another application.
#[test]
fn native_gamepad_polling_is_opt_in_and_active_window_only() {
    struct Pad(Rc<RefCell<Option<GamepadState>>>);
    impl GamepadSource for Pad {
        fn slots(&self) -> u32 {
            1
        }
        fn poll(&mut self, _slot: u32) -> Option<GamepadState> {
            self.0.borrow().clone()
        }
    }

    let fixture = Fixture::new();
    let mut application = fixture.application(false);
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let top = harness.hwnd(WindowId::PRIMARY);
    let state = Rc::new(RefCell::new(None));
    let shared = state.clone();
    harness.with_runtime_mut(WindowId::PRIMARY, move |runtime| {
        runtime.input.gamepad = GamepadInputState::with_source(Box::new(Pad(shared)));
        gamepad::sync_timer(runtime);
        assert_eq!(runtime.input.gamepad.interval(), Some(16), "a node asked, so polling runs");
    });
    fixture.take();

    // SAFETY: `top` is this thread's live top-level window.
    unsafe { SetActiveWindow(top) };
    *state.borrow_mut() = Some(GamepadState::new().with_button(GamepadButton::South));
    harness.send(top, WM_TIMER, GAMEPAD_TIMER_ID, 0);
    assert_eq!(
        fixture.take(),
        vec!["gamepad:pad:0:Connected", "gamepad:pad:0:Button { button: South, pressed: true }",]
    );

    *state.borrow_mut() = None;
    harness.send(top, WM_TIMER, GAMEPAD_TIMER_ID, 0);
    assert_eq!(fixture.take(), vec!["gamepad:pad:0:Disconnected"]);
    assert_eq!(
        harness.with_runtime(WindowId::PRIMARY, |runtime| runtime.input.gamepad.interval()),
        Some(1_000),
        "with nothing connected, polling backs off"
    );

    // Another window of this thread becomes active: the pad's window must
    // not react, though it keeps tracking state.
    let other = super::test_support::TestWindow::new_top_level();
    // SAFETY: `other.hwnd` is a live top-level window of this thread.
    unsafe { SetActiveWindow(other.hwnd) };
    *state.borrow_mut() = Some(GamepadState::new());
    harness.send(top, WM_TIMER, GAMEPAD_TIMER_ID, 0);
    assert!(fixture.take().is_empty(), "an inactive window ignores the controller");
}

/// IME composition on a focusable custom container: start, commit (which
/// also arrives as ordinary text input), and a session ended without a
/// commit becoming a cancel.
///
/// Catches: IME messages left to `DefWindowProcW` (which would deliver the
/// committed text a second time as keystrokes), and a cancelled session the
/// component never hears end.
#[test]
fn native_ime_composition_on_a_focusable_container() {
    use windows_sys::Win32::UI::Input::Ime::GCS_RESULTSTR;

    let fixture = Fixture::new();
    let mut application = fixture.application(false);
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let editor = harness.expect_control(WindowId::PRIMARY, "editor");
    // SAFETY: `editor` is a live window of this thread.
    unsafe { SetFocus(editor) };
    // The loop reconciles focus after each message it handles; hand it one
    // so the resulting `FocusGained` lands before the log is cleared.
    harness.post(harness.hwnd(WindowId::PRIMARY), 0, 0, 0);
    // SAFETY: no arguments.
    assert_eq!(unsafe { GetFocus() }, editor, "a window of this thread can take focus");
    fixture.take();

    harness.send(editor, ime::WM_IME_STARTCOMPOSITION, 0, 0);
    // No IME is guaranteed on a test machine, so the input context holds no
    // result string: the commit is empty, and so no `TextInput` follows.
    #[allow(clippy::cast_possible_wrap)]
    let result_flag = GCS_RESULTSTR as LPARAM;
    harness.send(editor, ime::WM_IME_COMPOSITION, 0, result_flag);
    harness.send(editor, ime::WM_IME_ENDCOMPOSITION, 0, 0);
    harness.send(editor, ime::WM_IME_STARTCOMPOSITION, 0, 0);
    harness.send(editor, ime::WM_IME_ENDCOMPOSITION, 0, 0);
    assert_eq!(
        fixture.take(),
        vec!["ime:Started", "ime:Committed { text: \"\" }", "ime:Started", "ime:Cancelled",]
    );
}

/// Two touch contacts described by real `POINTER_INFO`s: each is
/// implicitly captured to the node it went down on (so a contact sliding
/// over another node still reports to the pad), and together they pinch.
///
/// Catches: touch contacts retargeted mid-gesture, gesture recognition
/// never fed from touch, and contact bookkeeping that leaks after release.
#[test]
fn native_touch_contacts_are_implicitly_captured_and_pinch() {
    const PT_TOUCH: i32 = 2;
    let fixture = Fixture::new();
    let mut application = fixture.application(false);
    // SAFETY: `application` outlives `harness`.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let pad = harness.expect_control(WindowId::PRIMARY, "pad");
    let plain = harness.expect_control(WindowId::PRIMARY, "plain");
    let origin = screen_rect(pad);
    fixture.take();

    let contact = |x: i32, y: i32| POINTER_INFO {
        pointerType: PT_TOUCH,
        ptPixelLocation: POINT { x: origin.left + x, y: origin.top + y },
        ..POINTER_INFO::default()
    };
    let mut step = |hwnd: HWND, id: u32, phase: PointerPhase, x: i32, y: i32| {
        harness.with_runtime_mut(WindowId::PRIMARY, |runtime| {
            pointer::touch_or_pen(runtime, hwnd, id, phase, &contact(x, y), None);
        });
    };

    step(pad, 7, PointerPhase::Down, 20, 40);
    step(pad, 8, PointerPhase::Down, 60, 40);
    // Contact 8 slides out over the label below; Windows would address it
    // to that label.
    step(plain, 8, PointerPhase::Move, 100, 40);
    step(plain, 8, PointerPhase::Up, 100, 40);
    step(pad, 7, PointerPhase::Up, 20, 40);

    let log = fixture.take();
    assert_eq!(
        log,
        vec![
            "down:pad@20,40",
            "down:pad@60,40",
            "gesture:pad:Pinch { phase: Began, scale: Scalar(1.0), center: Point { x: 40, y: 40 } }",
            "move:pad@100,40",
            "gesture:pad:Pinch { phase: Changed, scale: Scalar(2.0), center: Point { x: 60, y: 40 } }",
            "up:pad@100,40",
            "gesture:pad:Pinch { phase: Ended, scale: Scalar(2.0), center: Point { x: 60, y: 40 } }",
            "up:pad@20,40",
        ]
    );
    harness.with_runtime(WindowId::PRIMARY, |runtime| {
        assert_eq!(runtime.input.pointer.captured(7), None, "released contacts are forgotten");
        assert_eq!(runtime.input.pointer.captured(8), None);
    });
}
