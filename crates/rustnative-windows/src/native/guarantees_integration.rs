//! The Milestone 41 guarantees on the Windows backend (`docs/guarantees.md`):
//! the shared suites from `rustnative-conformance`, run through the native
//! harness, and the guarantees only a real host can answer — the GDI/USER
//! leak gate, the host's modal loops, fidelity to the system's own
//! controls and settings, and text through the system's own stack.

use std::time::{Duration, Instant};

use rustnative_conformance::host::{ConformanceHost, Driver};
use rustnative_core::{Application, Component, Size, Window, WindowId};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowRect, WM_CHAR, WM_MOUSEWHEEL};

use super::harness::NativeHarness;

/// The native harness as a conformance host.
struct WindowsHost;

struct WindowsDriver<'a> {
    harness: &'a mut NativeHarness,
}

fn screen_rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is a live window of this thread; `rect` is writable.
    unsafe { GetWindowRect(hwnd, &raw mut rect) };
    rect
}

impl Driver for WindowsDriver<'_> {
    fn click(&mut self, key: &str) {
        self.harness.click(WindowId::PRIMARY, key);
    }

    fn type_text(&mut self, key: &str, text: &str) {
        let field = self.harness.expect_control(WindowId::PRIMARY, key);
        // One `WM_CHAR` per UTF-16 unit, sent to the EDIT control itself —
        // the control inserts it and raises `EN_CHANGE`, as for a keystroke.
        for unit in text.encode_utf16() {
            self.harness.send(field, WM_CHAR, usize::from(unit), 1);
            self.harness.pump();
        }
    }

    fn scroll(&mut self, key: &str, dy: i32) {
        let viewport = screen_rect(self.harness.expect_control(WindowId::PRIMARY, key));
        let (x, y) = (viewport.left + 5, viewport.top + 5);
        #[allow(
            clippy::cast_sign_loss,
            clippy::cast_possible_wrap,
            reason = "the documented WM_MOUSEWHEEL layout: two 16-bit coordinates in one LPARAM"
        )]
        let lparam = ((((y as u32) & 0xFFFF) << 16) | ((x as u32) & 0xFFFF)) as isize;
        // One detent (-120, scrolling down) moves 40 px.
        let wparam = (u32::from(0xFF88_u16) << 16) as usize;
        let window = self.harness.hwnd(WindowId::PRIMARY);
        for _ in 0..(dy / 40).max(1) {
            self.harness.post(window, WM_MOUSEWHEEL, wparam, lparam);
        }
        self.harness.pump();
    }

    fn advance(&mut self, duration: Duration) {
        let until = Instant::now() + duration;
        while Instant::now() < until {
            self.harness.pump();
            std::thread::sleep(Duration::from_millis(5));
        }
        self.harness.pump();
    }

    fn realized_objects(&self) -> usize {
        self.harness.with_runtime(WindowId::PRIMARY, |runtime| runtime.renderer.registry.len())
    }
}

impl ConformanceHost for WindowsHost {
    fn name(&self) -> &'static str {
        "windows"
    }

    fn run<C, F>(&mut self, window: Window, root: F, script: &mut dyn FnMut(&mut dyn Driver))
    where
        C: Component,
        F: Fn() -> C + 'static,
    {
        let mut application = Application::new(root(), window);
        // SAFETY: `application` is declared before the harness and outlives
        // it (the harness is dropped at the end of this scope, first).
        let mut harness = unsafe { NativeHarness::attach(&mut application) };
        script(&mut WindowsDriver { harness: &mut harness });
    }
}

#[test]
fn the_shared_guarantee_suites_hold_on_windows() {
    rustnative_conformance::suites::all(&mut WindowsHost);
}

// ---------------------------------------------------------------------------
// Native-object lifetime: the GDI/USER leak gate
// ---------------------------------------------------------------------------

use std::cell::RefCell;
use std::rc::Rc;

use rustnative_core::environment::keys;
use rustnative_core::{
    AnimatedProperty, AnimatedValue, Animation, AnimationRequests, ComponentContext, Contrast,
    Event, Node, NodeId, Transition, classes,
};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_WINDOW, COLOR_WINDOWTEXT, CreateCompatibleDC, DeleteDC, GetBkColor, GetObjectW,
    GetSysColor, GetTextColor, GetTextExtentPoint32W, HGDIOBJ, LOGFONTW, SelectObject,
};
use windows_sys::Win32::System::Threading::{
    GR_GDIOBJECTS, GR_USEROBJECTS, GetCurrentProcess, GetGuiResources,
};
use windows_sys::Win32::UI::Controls::EM_POSFROMCHAR;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, EndMenu, GetClassNameW, GetWindowTextLengthW,
    GetWindowTextW, KillTimer, MF_STRING, SC_SIZE, SetTimer, TPM_RETURNCMD, TrackPopupMenu,
    UISF_HIDEFOCUS, WM_CANCELMODE, WM_CTLCOLORSTATIC, WM_GETFONT, WM_QUERYUISTATE, WM_SYSCOMMAND,
};

fn gui_resources() -> (u32, u32) {
    // SAFETY: the pseudo-handle for this process and documented flags.
    unsafe {
        let process = GetCurrentProcess();
        (GetGuiResources(process, GR_GDIOBJECTS), GetGuiResources(process, GR_USEROBJECTS))
    }
}

struct Churn {
    showing: bool,
}

impl Component for Churn {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { showing: false }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        let mut children = vec![Node::button("toggle", "Toggle")];
        if self.showing {
            children.push(
                Node::column(
                    "panel",
                    (0..10).map(|index| {
                        Node::label(format!("item-{index}"), format!("Item {index}"))
                            .with_class(classes!("font-bold text-sm bg-sky-100 text-sky-900"))
                    }),
                )
                .with_class(classes!("bg-white border-sky-300 rounded-lg")),
            );
        }
        Node::column("root", children)
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { target } if target == NodeId::from_key("toggle")) {
            self.showing = !self.showing;
        }
    }
}

/// Mounting and unmounting a styled subtree — windows, fonts, brushes,
/// window regions — a hundred times returns the process's GDI and USER
/// object counts to where they were: every native resource is released
/// with its node.
#[test]
fn gdi_and_user_objects_return_to_baseline() {
    let mut application = Application::new(Churn::new(()), window());
    // SAFETY: `application` outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    // One warm-up cycle fills the caches that are bounded by design (the
    // colour-keyed brush cache, the measuring fonts).
    harness.click(WindowId::PRIMARY, "toggle");
    harness.click(WindowId::PRIMARY, "toggle");
    let (gdi, user) = gui_resources();
    for _ in 0..100 {
        harness.click(WindowId::PRIMARY, "toggle");
        harness.click(WindowId::PRIMARY, "toggle");
    }
    let (gdi_after, user_after) = gui_resources();
    assert!(gdi_after <= gdi + 2, "GDI objects: {gdi} before, {gdi_after} after 100 cycles");
    assert!(user_after <= user + 2, "USER objects: {user} before, {user_after} after 100 cycles");
}

// ---------------------------------------------------------------------------
// Modal operations
// ---------------------------------------------------------------------------

type Log = Rc<RefCell<Vec<&'static str>>>;

struct Busy {
    log: Log,
    animations: Option<AnimationRequests>,
    started: bool,
}

impl Component for Busy {
    type Props = Log;
    type Message = &'static str;
    fn new(log: Log) -> Self {
        Self { log, animations: None, started: false }
    }
    fn props(&self) -> &Log {
        &self.log
    }
    fn set_props(&mut self, log: Log) {
        self.log = log;
    }
    fn view(&self) -> Node {
        Node::column("root", [Node::button("start", "Start"), Node::label("moving", "Moving")])
    }
    fn render(&mut self, context: &mut ComponentContext<'_, &'static str>) -> Node {
        self.animations = Some(context.animations());
        if self.started {
            context.effect("work", (), |effects| {
                let delay = effects.sleep(Duration::from_millis(60));
                effects.spawn(async move {
                    delay.await;
                    "task"
                });
                Box::new(|| {})
            });
        }
        self.view()
    }
    fn update(&mut self, event: Event) {
        match event {
            Event::Click { target } if target == NodeId::from_key("start") => {
                self.started = true;
                if let Some(animations) = &self.animations {
                    animations.animate(
                        "moving",
                        Animation::new(
                            AnimatedProperty::Opacity,
                            AnimatedValue::Scalar(rustnative_core::input::Scalar::new(0.2)),
                            Transition::new(Duration::from_millis(120)),
                        ),
                    );
                }
            }
            Event::AnimationFinished { .. } => self.log.borrow_mut().push("animation"),
            _ => {}
        }
    }
    fn message(&mut self, message: &'static str) {
        self.log.borrow_mut().push(message);
    }
}

unsafe extern "system" fn end_menu(_: HWND, _: u32, timer: usize, _: u32) {
    // SAFETY: ends this thread's menu mode and kills this timer; neither
    // takes pointers.
    unsafe {
        EndMenu();
        KillTimer(std::ptr::null_mut(), timer);
    }
}

static SIZING_WINDOW: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

unsafe extern "system" fn end_sizing(_: HWND, _: u32, timer: usize, _: u32) {
    let hwnd = SIZING_WINDOW.load(std::sync::atomic::Ordering::Relaxed) as HWND;
    // SAFETY: cancels the sizing loop of this thread's window and kills
    // this timer.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(hwnd, WM_CANCELMODE, 0, 0);
        KillTimer(std::ptr::null_mut(), timer);
    }
}

/// While the host runs its own modal loop — here the popup-menu loop —
/// animations keep ticking and scheduled work keeps completing: both reach
/// the component before the menu closes.
#[test]
fn the_application_keeps_running_inside_a_menu_loop() {
    let log = Log::default();
    let mut application = Application::new(Busy::new(log.clone()), window());
    // SAFETY: `application` outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let window = harness.hwnd(WindowId::PRIMARY);
    harness.click(WindowId::PRIMARY, "start");
    let item = super::util::wide("Nothing");
    // SAFETY: a popup menu owned by this test, tracked on this thread's
    // window, ended by the thread timer, and destroyed.
    unsafe {
        let menu = CreatePopupMenu();
        AppendMenuW(menu, MF_STRING, 1, item.as_ptr());
        SetTimer(std::ptr::null_mut(), 0, 600, Some(end_menu));
        TrackPopupMenu(menu, TPM_RETURNCMD, 10, 10, 0, window, std::ptr::null());
        DestroyMenu(menu);
    }
    // Read before pumping anything: what is here arrived inside the loop.
    let seen = log.borrow().clone();
    assert!(seen.contains(&"task"), "the scheduled task completed inside the menu loop: {seen:?}");
    assert!(seen.contains(&"animation"), "the animation finished inside the menu loop: {seen:?}");
}

/// The same inside the window's own size loop.
#[test]
fn the_application_keeps_running_inside_the_size_loop() {
    let log = Log::default();
    let mut application = Application::new(Busy::new(log.clone()), window());
    // SAFETY: `application` outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let window = harness.hwnd(WindowId::PRIMARY);
    harness.click(WindowId::PRIMARY, "start");
    SIZING_WINDOW.store(window as isize, std::sync::atomic::Ordering::Relaxed);
    // SAFETY: enters the system's keyboard sizing loop on this thread's
    // window; the thread timer cancels it.
    unsafe {
        SetTimer(std::ptr::null_mut(), 0, 600, Some(end_sizing));
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
            window,
            WM_SYSCOMMAND,
            SC_SIZE as usize,
            0,
        );
    }
    let seen = log.borrow().clone();
    assert!(seen.contains(&"task"), "the scheduled task completed inside the size loop: {seen:?}");
    assert!(seen.contains(&"animation"), "the animation finished inside the size loop: {seen:?}");
}

// ---------------------------------------------------------------------------
// Fidelity
// ---------------------------------------------------------------------------

struct Controls;

impl Component for Controls {
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
                Node::label("label", "A label").with_class(classes!("bg-[#123456] text-[#abcdef]")),
                Node::button("button", "A button"),
                Node::text_input("input", "text"),
                Node::tab_bar("tabs", ["One", "Two"], 0, rustnative_core::LayoutStyle::default()),
            ],
        )
    }
    fn update(&mut self, _: Event) {}
}

fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0_u16; 64];
    // SAFETY: `buffer` is writable for its length.
    let length = unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), 64) };
    String::from_utf16_lossy(&buffer[..usize::try_from(length).unwrap_or(0)])
}

fn font_height(harness: &mut NativeHarness, control: HWND) -> i32 {
    let font = harness.send(control, WM_GETFONT, 0, 0);
    let mut logfont = LOGFONTW::default();
    let size = i32::try_from(std::mem::size_of::<LOGFONTW>()).unwrap();
    // SAFETY: `font` is the control's live font; `logfont` is writable.
    unsafe { GetObjectW(font as HGDIOBJ, size, (&raw mut logfont).cast()) };
    logfont.lfHeight
}

fn painted_colors(harness: &mut NativeHarness, control: HWND) -> (u32, u32) {
    let window = harness.hwnd(WindowId::PRIMARY);
    // SAFETY: a memory DC, released below.
    let hdc = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
    harness.send(window, WM_CTLCOLORSTATIC, hdc as usize, control as isize);
    // SAFETY: `hdc` is live until deleted.
    let colors = unsafe { (GetTextColor(hdc), GetBkColor(hdc)) };
    // SAFETY: deleted once.
    unsafe { DeleteDC(hdc) };
    colors
}

/// Every control is the system's own class — never an imitation.
#[test]
fn controls_are_the_systems_own_classes() {
    let mut application = Application::new(Controls, window());
    // SAFETY: `application` outlives the harness.
    let harness = unsafe { NativeHarness::attach(&mut application) };
    for (key, class) in
        [("label", "Static"), ("button", "Button"), ("input", "Edit"), ("tabs", "SysTabControl32")]
    {
        let actual = class_name(harness.expect_control(WindowId::PRIMARY, key));
        assert!(actual.eq_ignore_ascii_case(class), "`{key}` is a {class}, not {actual}");
    }
}

/// Keyboard navigation shows focus rectangles, as the system's dialog
/// manager does.
#[test]
fn keyboard_traversal_shows_focus_cues() {
    let mut application = Application::new(Controls, window());
    // SAFETY: `application` outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    harness.press_key(
        WindowId::PRIMARY,
        u32::from(windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_TAB),
    );
    let button = harness.expect_control(WindowId::PRIMARY, "button");
    let state = harness.send(button, WM_QUERYUISTATE, 0, 0);
    assert_eq!(
        state & isize::try_from(UISF_HIDEFOCUS).unwrap(),
        0,
        "focus cues are shown after Tab"
    );
}

/// High contrast gives every control the system's colours — over the
/// theme and over the application's own classes — with no application
/// code, on the same windows.
#[test]
fn high_contrast_uses_the_system_colours() {
    let mut application = Application::new(Controls, window());
    // SAFETY: `application` outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let label = harness.expect_control(WindowId::PRIMARY, "label");
    assert_eq!(
        painted_colors(&mut harness, label),
        (0x00EF_CDAB, 0x0056_3412),
        "the class colours, normally"
    );
    harness.with_runtime_mut(WindowId::PRIMARY, |runtime| {
        runtime.with_application(|application| {
            application.set_environment(&keys::CONTRAST, Contrast::High);
        });
        runtime.render().unwrap();
    });
    // SAFETY: documented indexes, no pointers.
    let system = unsafe { (GetSysColor(COLOR_WINDOWTEXT), GetSysColor(COLOR_WINDOW)) };
    assert_eq!(
        painted_colors(&mut harness, label),
        system,
        "the system's colours under high contrast"
    );
    assert_eq!(harness.expect_control(WindowId::PRIMARY, "label"), label, "the same window");
}

/// The person's text size scales every font — themed or declared — and
/// layout measures in the scaled font, so text still fits.
#[test]
fn the_text_scale_scales_every_font_and_the_layout_follows() {
    let mut application = Application::new(Controls, window());
    // SAFETY: `application` outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let label = harness.expect_control(WindowId::PRIMARY, "label");
    let before = font_height(&mut harness, label);
    let height_before = screen_rect(label).bottom - screen_rect(label).top;
    harness.with_runtime_mut(WindowId::PRIMARY, |runtime| {
        runtime.with_application(|application| {
            application
                .set_environment(&keys::TEXT_SCALE, rustnative_core::input::Scalar::new(1.5));
        });
        runtime.render().unwrap();
    });
    assert_eq!(font_height(&mut harness, label), before * 3 / 2, "the font is 1.5 times as tall");
    let height_after = screen_rect(label).bottom - screen_rect(label).top;
    assert!(
        height_after > height_before,
        "the label grew with its font: {height_before} → {height_after}"
    );
}

// ---------------------------------------------------------------------------
// Text through the system's own stack
// ---------------------------------------------------------------------------

const SCRIPTS: [(&str, &str); 6] = [
    ("arabic", "مرحبا بالعالم"),
    ("mixed-bidi", "שלום world 123 עולם"),
    ("devanagari", "नमस्ते दुनिया"),
    ("emoji-zwj", "👩‍👩‍👧‍👦 family"),
    ("thai", "สวัสดีชาวโลกยินดีต้อนรับทุกคน"),
    ("cjk", "你好，世界。日本語のテキスト"),
];

struct Scripts;

impl Component for Scripts {
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
            SCRIPTS.iter().flat_map(|(key, text)| {
                [
                    Node::label(format!("{key}-label"), *text),
                    Node::text_input(format!("{key}-input"), *text),
                ]
            }),
        )
    }
    fn update(&mut self, _: Event) {}
}

/// Complex scripts, mixed bidirectional text, grapheme clusters, emoji
/// sequences, scripts without spaces, and CJK go through the system's own
/// text stack: every string round-trips through the edit control exactly,
/// every caret position resolves, and the framework's measurement agrees
/// with the system's own for the font that draws it.
#[test]
fn text_goes_through_the_systems_own_stack() {
    let mut application = Application::new(Scripts, Window::new("scripts", Size::new(600, 600)));
    // SAFETY: `application` outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    for (key, text) in SCRIPTS {
        let input = harness.expect_control(WindowId::PRIMARY, &format!("{key}-input"));
        // SAFETY: a live control of this thread; `buffer` is writable.
        let round_trip = unsafe {
            let length = GetWindowTextLengthW(input);
            let mut buffer = vec![0_u16; usize::try_from(length).unwrap() + 1];
            let copied = GetWindowTextW(input, buffer.as_mut_ptr(), length + 1);
            String::from_utf16_lossy(&buffer[..usize::try_from(copied).unwrap()])
        };
        assert_eq!(round_trip, text, "`{key}` round-trips through the edit control");
        for index in 0..text.encode_utf16().count() {
            let position = harness.send(input, EM_POSFROMCHAR, index, 0);
            assert_ne!(position, -1, "`{key}`: caret position {index} resolves");
        }

        let label = harness.expect_control(WindowId::PRIMARY, &format!("{key}-label"));
        let font = harness.send(label, WM_GETFONT, 0, 0);
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: a memory DC with the label's own font selected, released
        // below; `extent` is writable.
        let system_width = unsafe {
            let hdc = CreateCompatibleDC(std::ptr::null_mut());
            let previous = SelectObject(hdc, font as HGDIOBJ);
            let mut extent = windows_sys::Win32::Foundation::SIZE::default();
            GetTextExtentPoint32W(
                hdc,
                wide.as_ptr(),
                i32::try_from(wide.len()).unwrap(),
                &raw mut extent,
            );
            SelectObject(hdc, previous);
            DeleteDC(hdc);
            extent.cx
        };
        let measurer = super::measure::WindowsIntrinsicMeasurer {
            window: harness.hwnd(WindowId::PRIMARY),
            text_scale: 1.0,
        };
        let ours = rustnative_core::IntrinsicMeasurer::measure_styled(
            &measurer,
            rustnative_core::NodeKind::Label,
            Some(text),
            None,
            Some(&rustnative_core::Typography::default()),
        );
        let ours = i32::try_from(ours.width).unwrap();
        assert!(ours > 0 && system_width > 0, "`{key}` measures");
        assert!(
            (ours - system_width).abs() <= 2,
            "`{key}`: our measurement {ours} and the system's {system_width} agree"
        );
    }
}

// ---------------------------------------------------------------------------
// Layout conformance with the system's own font metrics
// ---------------------------------------------------------------------------

/// The text a label or button needs, in its own font, at `width` (wrapping)
/// or unconstrained.
fn needed(harness: &mut NativeHarness, control: HWND, width: Option<i32>) -> (i32, i32) {
    const DT_CALCRECT: u32 = 0x0400;
    const DT_WORDBREAK: u32 = 0x0010;
    let length = {
        // SAFETY: a live control of this thread.
        unsafe { GetWindowTextLengthW(control) }
    };
    let mut buffer = vec![0_u16; usize::try_from(length).unwrap() + 1];
    // SAFETY: `buffer` is writable for its length.
    unsafe { GetWindowTextW(control, buffer.as_mut_ptr(), length + 1) };
    let font = harness.send(control, WM_GETFONT, 0, 0);
    let mut rect =
        RECT { left: 0, top: 0, right: width.unwrap_or(i32::MAX / 4), bottom: i32::MAX / 4 };
    // SAFETY: a memory DC with the control's font, released below; `rect`
    // is writable.
    unsafe {
        let hdc = CreateCompatibleDC(std::ptr::null_mut());
        let previous = SelectObject(hdc, font as HGDIOBJ);
        let flags = DT_CALCRECT | if width.is_some() { DT_WORDBREAK } else { 0 };
        windows_sys::Win32::Graphics::Gdi::DrawTextW(
            hdc,
            buffer.as_ptr(),
            length,
            &raw mut rect,
            flags,
        );
        SelectObject(hdc, previous);
        DeleteDC(hdc);
    }
    (rect.right - rect.left, rect.bottom - rect.top)
}

/// The reference screen at text scales 1.0, 1.5, and 2.0, plain and
/// pseudo-localized, measured with the fonts that draw it: no label or
/// button is smaller than its text needs.
#[test]
fn the_reference_screen_fits_its_text_at_every_scale() {
    use rustnative_conformance::reference::{ReferenceScreen, Variant};
    let mut problems = Vec::new();
    for scale in [1.0_f32, 1.5, 2.0] {
        for pseudo in [false, true] {
            let variant = Variant { pseudo, right_to_left: false };
            let mut application = Application::new(
                ReferenceScreen::new(variant),
                Window::new("reference", Size::new(480, 900)),
            );
            // SAFETY: `application` outlives the harness.
            let mut harness = unsafe { NativeHarness::attach(&mut application) };
            harness.with_runtime_mut(WindowId::PRIMARY, |runtime| {
                runtime.with_application(|application| {
                    application.set_environment(
                        &keys::TEXT_SCALE,
                        rustnative_core::input::Scalar::new(scale),
                    );
                });
                runtime.render().unwrap();
            });
            for key in ["heading", "intro", "name-label", "email-label"] {
                let label = harness.expect_control(WindowId::PRIMARY, key);
                let rect = screen_rect(label);
                let (_, height) = needed(&mut harness, label, Some(rect.right - rect.left));
                if height > rect.bottom - rect.top {
                    problems.push(format!(
                        "scale {scale}, pseudo {pseudo}: `{key}` needs {height}px, has {}",
                        rect.bottom - rect.top
                    ));
                }
            }
            for key in ["cancel", "save"] {
                let button = harness.expect_control(WindowId::PRIMARY, key);
                let rect = screen_rect(button);
                let (width, height) = needed(&mut harness, button, None);
                if width > rect.right - rect.left || height > rect.bottom - rect.top {
                    problems.push(format!(
                        "scale {scale}, pseudo {pseudo}: `{key}` needs {width}×{height}, has {}×{}",
                        rect.right - rect.left,
                        rect.bottom - rect.top
                    ));
                }
            }
        }
    }
    assert!(problems.is_empty(), "layout conformance on Windows:\n  {}", problems.join("\n  "));
}

fn window() -> Window {
    Window::new("guarantees", Size::new(360, 240))
}
