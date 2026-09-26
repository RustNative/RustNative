//! Mouse, touch, and pen input: targeting, pointer capture, hover
//! enter/leave, wheels, and feeding the portable gesture recognizer.
//!
//! # Where the messages come from
//!
//! Mouse messages (`WM_MOUSEMOVE`, `WM_[LRMX]BUTTON*`) and pointer messages
//! (`WM_POINTER*`, which Windows sends for touch and pen) are all *posted*,
//! so the message loop's pre-dispatch pass sees every one of them before it
//! reaches whichever child control is under the pointer — including the
//! system `BUTTON`/`EDIT` classes whose window procedures this crate does
//! not own. That is where all of this runs, and none of it consumes the
//! message: a native button under the pointer still gets its own mouse
//! input and still behaves natively.
//!
//! Mouse input deliberately stays on the classic mouse messages rather than
//! being moved into the pointer stack with `EnableMouseInPointer`: that call
//! is process-global, irreversible, and changes how *standard controls*
//! receive the mouse, which is not a trade a UI framework gets to make on an
//! application's behalf. Touch and pen arrive through `WM_POINTER*`; when
//! Windows then promotes them to compatibility mouse messages, those are
//! recognized by their `GetMessageExtraInfo` signature and skipped, so a tap
//! is delivered once, as touch.
//!
//! # Targeting
//!
//! A sample is delivered to the nearest node — starting from the one whose
//! native window is under the pointer and walking up the declarative tree —
//! that declared pointer (or gesture) interest. While a contact is captured,
//! every sample for it goes to the capturing node instead, in that node's
//! coordinate space, even outside its bounds.
//!
//! # Capture and reentrancy
//!
//! Mouse capture is taken on the **top-level** window, not on the capturing
//! node's own `HWND`: a node may be realized as a system control whose window
//! procedure this crate does not own, and the top-level window is where
//! `WM_CAPTURECHANGED` must arrive for a lost capture to be noticed.
//! `WM_CAPTURECHANGED` is *sent*, synchronously, from inside
//! `SetCapture`/`ReleaseCapture` — which this module calls while the
//! window's `Runtime` is already borrowed. Its handler therefore only posts
//! [`WM_FRAMEWORK_CAPTURE_LOST`] and returns; the posted message is handled
//! on a later, unnested turn of the loop, and checks `GetCapture()` rather
//! than trusting the order messages arrived in.

use std::collections::HashMap;
use std::time::Instant;

use rustnative_core::{
    Event, GestureRecognizer, InputInterest, InputRequest, NodeId, Point, PointerButton,
    PointerButtons, PointerEvent, PointerKind, PointerPhase, WheelDelta,
};
use windows_sys::Win32::Foundation::{HWND, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetCapture, ReleaseCapture, SetCapture};
use windows_sys::Win32::UI::Input::Pointer::{
    GetPointerInfo, GetPointerPenInfo, POINTER_INFO, POINTER_PEN_INFO,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CWP_SKIPINVISIBLE, CWP_SKIPTRANSPARENT, ChildWindowFromPointEx, GetCursorPos,
    GetMessageExtraInfo, GetParent, KillTimer, MSG, SetTimer, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE,
    WM_POINTERDOWN, WM_POINTERUP, WM_POINTERUPDATE, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_XBUTTONDBLCLK, WM_XBUTTONDOWN, WM_XBUTTONUP, WindowFromPoint,
};

use super::super::context::root_window;
use super::super::runtime::Runtime;
use super::super::util::{hiword, hiword_signed, loword, loword_signed};
use super::super::win32::{best_effort, ignored_by_contract};
use super::keys::modifiers;

/// The pointer id the mouse is reported under. Windows' own pointer ids
/// for touch and pen start at 1 in practice, and are never 0.
pub(crate) const MOUSE_POINTER_ID: u32 = 0;

/// Posted (never sent) to a top-level window when it may have lost mouse
/// capture — see the module documentation.
pub(crate) const WM_FRAMEWORK_CAPTURE_LOST: u32 =
    windows_sys::Win32::UI::WindowsAndMessaging::WM_APP + 2;

/// `SetTimer` id for long-press recognition on a top-level window.
pub(crate) const LONG_PRESS_TIMER_ID: usize = 0x4652_0001;

/// `WM_POINTERCAPTURECHANGED` — not exported under this name by every
/// `windows-sys` version this crate accepts.
pub(crate) const WM_POINTERCAPTURECHANGED: u32 = 0x024C;

/// `GetMessageExtraInfo` signature Windows stamps on mouse messages it
/// synthesized from touch or pen input (`MI_WP_SIGNATURE` in `winuser.h`
/// documentation; the low byte varies).
const MI_WP_SIGNATURE: usize = 0xFF51_5700;
const SIGNATURE_MASK: usize = 0xFFFF_FF00;

/// `POINTER_INPUT_TYPE` values (`winuser.h`), inlined for the same reason
/// as the MSAA constants in `rendering::accessibility`: a frozen ABI.
const PT_TOUCH: i32 = 2;
const PT_PEN: i32 = 3;
/// `POINTER_FLAG_CANCELED`.
const POINTER_FLAG_CANCELED: u32 = 0x0000_8000;

/// Which node a contact is routed to, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Capture {
    node: NodeId,
    /// Implicit captures are taken by the backend itself for the duration
    /// of one press (every touch contact, and a mouse press on a
    /// gesture-interested node) and end with that press; explicit ones are
    /// requested by a component and end when it releases them.
    implicit: bool,
}

/// One window's pointer bookkeeping.
#[derive(Debug)]
pub(crate) struct PointerState {
    epoch: Instant,
    hover: Option<NodeId>,
    captures: HashMap<u32, Capture>,
    recognizers: HashMap<NodeId, GestureRecognizer>,
}

impl Default for PointerState {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
            hover: None,
            captures: HashMap::new(),
            recognizers: HashMap::new(),
        }
    }
}

impl PointerState {
    /// The node `pointer_id` is captured to, if any.
    pub(crate) fn captured(&self, pointer_id: u32) -> Option<NodeId> {
        self.captures.get(&pointer_id).map(|capture| capture.node)
    }
}

/// The deepest visible, non-transparent descendant of `root` under the
/// screen point `screen` (or `root` itself).
pub(crate) fn deepest_child_at(root: HWND, screen: POINT) -> HWND {
    let mut current = root;
    loop {
        let mut local = screen;
        // SAFETY: `current` is `root` or a child `ChildWindowFromPointEx`
        // just returned, both live; `local` is a valid, exclusively borrowed
        // `POINT`.
        if unsafe { ScreenToClient(current, &raw mut local) } == 0 {
            return current;
        }
        // SAFETY: as above; the flags are documented constants.
        let child = unsafe {
            ChildWindowFromPointEx(current, local, CWP_SKIPINVISIBLE | CWP_SKIPTRANSPARENT)
        };
        if child.is_null() || child == current {
            return current;
        }
        current = child;
    }
}

/// A wheel message's pointer position, which — unlike a button message's —
/// `WM_MOUSEWHEEL`/`WM_MOUSEHWHEEL` report in *screen* coordinates.
pub(crate) fn wheel_screen_point(message: &MSG) -> POINT {
    #[allow(clippy::cast_possible_wrap)]
    let point = POINT {
        x: i32::from(loword_signed(message.lParam) as i16),
        y: i32::from(hiword_signed(message.lParam) as i16),
    };
    point
}

fn interest(runtime: &Runtime, id: NodeId) -> InputInterest {
    runtime.renderer.snapshot.get(id).map(|node| node.input).unwrap_or_default()
}

/// The nearest node at or above `hwnd` whose interest satisfies `wants`.
pub(crate) fn interested_ancestor(
    runtime: &Runtime,
    hwnd: HWND,
    wants: impl Fn(InputInterest) -> bool,
) -> Option<NodeId> {
    let mut start = None;
    let mut current = hwnd;
    while !current.is_null() && current != runtime.window {
        if let Some(id) = runtime.renderer.registry.id_for_hwnd(current) {
            start = Some(id);
            break;
        }
        // SAFETY: `current` was just checked non-null; `GetParent` accepts
        // any window handle and returns null at the top of the chain.
        current = unsafe { GetParent(current) };
    }
    let mut id = start;
    while let Some(node) = id.and_then(|id| runtime.renderer.snapshot.get(id)) {
        if wants(node.input) {
            return Some(node.id);
        }
        id = node.parent;
    }
    None
}

/// `screen` in `id`'s local coordinates (the client area of the node's own
/// native window — for a container, its viewport).
fn local_point(runtime: &Runtime, id: NodeId, screen: POINT) -> Point {
    let Some(object) = runtime.renderer.registry.get(id) else {
        return Point::new(screen.x, screen.y);
    };
    let mut point = screen;
    // SAFETY: `object.hwnd()` is a live HWND owned by this window's
    // registry; `point` is a valid, exclusively borrowed `POINT`.
    let converted = unsafe { ScreenToClient(object.hwnd(), &raw mut point) } != 0;
    best_effort(converted, "ScreenToClient", "the sample is reported in screen coordinates");
    Point::new(point.x, point.y)
}

fn now(runtime: &Runtime) -> std::time::Duration {
    runtime.input.pointer.epoch.elapsed()
}

/// A mouse message's position in screen coordinates.
fn mouse_screen_point(message: &MSG) -> POINT {
    // `GET_X_LPARAM`/`GET_Y_LPARAM`: two signed 16-bit client coordinates
    // (negative while captured and outside the window).
    #[allow(clippy::cast_possible_wrap)]
    let mut point = POINT {
        x: i32::from(loword_signed(message.lParam) as i16),
        y: i32::from(hiword_signed(message.lParam) as i16),
    };
    // SAFETY: `message.hwnd` is the live window the message was addressed
    // to; `point` is a valid, exclusively borrowed `POINT`.
    let converted = unsafe { ClientToScreen(message.hwnd, &raw mut point) } != 0;
    best_effort(converted, "ClientToScreen", "the sample keeps client coordinates");
    point
}

fn mouse_buttons(wparam: WPARAM) -> PointerButtons {
    const MK_LBUTTON: usize = 0x0001;
    const MK_RBUTTON: usize = 0x0002;
    const MK_MBUTTON: usize = 0x0010;
    const MK_XBUTTON1: usize = 0x0020;
    const MK_XBUTTON2: usize = 0x0040;
    let mut buttons = PointerButtons::none();
    for (mask, button) in [
        (MK_LBUTTON, PointerButton::Primary),
        (MK_RBUTTON, PointerButton::Secondary),
        (MK_MBUTTON, PointerButton::Middle),
        (MK_XBUTTON1, PointerButton::Back),
        (MK_XBUTTON2, PointerButton::Forward),
    ] {
        if wparam & mask != 0 {
            buttons = buttons.with(button);
        }
    }
    buttons
}

/// Classifies a mouse message as a pointer phase and the button it
/// concerns, or `None` if it is not a button/move message.
fn mouse_phase(message: &MSG) -> Option<(PointerPhase, Option<PointerButton>)> {
    let x_button =
        || if hiword(message.wParam) == 1 { PointerButton::Back } else { PointerButton::Forward };
    Some(match message.message {
        WM_MOUSEMOVE => (PointerPhase::Move, None),
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => (PointerPhase::Down, Some(PointerButton::Primary)),
        WM_LBUTTONUP => (PointerPhase::Up, Some(PointerButton::Primary)),
        WM_RBUTTONDOWN | WM_RBUTTONDBLCLK => (PointerPhase::Down, Some(PointerButton::Secondary)),
        WM_RBUTTONUP => (PointerPhase::Up, Some(PointerButton::Secondary)),
        WM_MBUTTONDOWN | WM_MBUTTONDBLCLK => (PointerPhase::Down, Some(PointerButton::Middle)),
        WM_MBUTTONUP => (PointerPhase::Up, Some(PointerButton::Middle)),
        WM_XBUTTONDOWN | WM_XBUTTONDBLCLK => (PointerPhase::Down, Some(x_button())),
        WM_XBUTTONUP => (PointerPhase::Up, Some(x_button())),
        _ => return None,
    })
}

/// Whether a mouse message was synthesized by Windows from touch or pen
/// input that `WM_POINTER*` already delivered.
fn promoted_from_touch_or_pen() -> bool {
    // SAFETY: `GetMessageExtraInfo` takes no arguments and reads a
    // per-thread value set by the last `GetMessageW`/`PeekMessageW`, which
    // is the message this loop is currently pre-dispatching.
    let extra = unsafe { GetMessageExtraInfo() };
    // Reinterpreting the `LPARAM` bit pattern is exactly the documented
    // check (`(GetMessageExtraInfo() & 0xFFFFFF00) == 0xFF515700`).
    #[allow(clippy::cast_sign_loss)]
    let bits = extra as usize;
    bits & SIGNATURE_MASK == MI_WP_SIGNATURE
}

/// Handles one mouse button/move message. Never consumes it.
pub(crate) fn mouse_message(runtime: &mut Runtime, message: &MSG) {
    let Some((phase, button)) = mouse_phase(message) else {
        return;
    };
    if promoted_from_touch_or_pen() {
        return;
    }
    let screen = mouse_screen_point(message);
    if phase == PointerPhase::Move {
        update_hover(runtime, message.hwnd);
    }
    let target = runtime.input.pointer.captured(MOUSE_POINTER_ID).or_else(|| {
        interested_ancestor(runtime, message.hwnd, |i| i.wants_pointer() || i.wants_gestures())
    });
    let Some(target) = target else {
        return;
    };
    let mut sample = PointerEvent::new(
        MOUSE_POINTER_ID,
        PointerKind::Mouse,
        local_point(runtime, target, screen),
        now(runtime),
    )
    .with_buttons(mouse_buttons(message.wParam))
    .with_modifiers(modifiers());
    if let Some(button) = button {
        sample = sample.with_button(button);
    }

    // A primary press on a gesture-interested node holds the mouse for the
    // rest of the press, so the recognizer always sees the matching release
    // even if it happens outside the window.
    if phase == PointerPhase::Down
        && button == Some(PointerButton::Primary)
        && interest(runtime, target).wants_gestures()
        && runtime.input.pointer.captured(MOUSE_POINTER_ID).is_none()
    {
        capture(runtime, MOUSE_POINTER_ID, target, true);
    }

    deliver(runtime, target, phase, &sample);

    if phase == PointerPhase::Up && mouse_buttons(message.wParam).is_empty() {
        end_implicit_capture(runtime, MOUSE_POINTER_ID);
    }
}

/// `WM_MOUSELEAVE`: the pointer left the native window it was tracked on.
/// If it also left this top-level window altogether, hover ends; otherwise
/// the next `WM_MOUSEMOVE` over the new window settles it.
pub(crate) fn mouse_left(runtime: &mut Runtime) {
    let mut cursor = POINT { x: 0, y: 0 };
    // SAFETY: `cursor` is a valid, exclusively borrowed `POINT`.
    let located = unsafe { GetCursorPos(&raw mut cursor) } != 0;
    // SAFETY: `WindowFromPoint` takes a plain `POINT`.
    let under = if located { unsafe { WindowFromPoint(cursor) } } else { std::ptr::null_mut() };
    if under.is_null() || root_window(under) != runtime.window {
        set_hover(runtime, None);
    }
}

fn update_hover(runtime: &mut Runtime, hwnd: HWND) {
    let next = interested_ancestor(runtime, hwnd, InputInterest::wants_pointer);
    set_hover(runtime, next);
}

fn set_hover(runtime: &mut Runtime, next: Option<NodeId>) {
    let previous = runtime.input.pointer.hover;
    if previous == next {
        return;
    }
    runtime.input.pointer.hover = next;
    if let Some(previous) = previous {
        if !runtime.dispatch_or_quit(Event::PointerLeave { target: previous }) {
            return;
        }
    }
    if let Some(next) = next {
        runtime.dispatch_or_quit(Event::PointerEnter { target: next });
    }
}

/// Handles `WM_POINTERDOWN`/`UPDATE`/`UP` for touch and pen. Mouse-type
/// pointers are left to the mouse path above (see the module docs).
pub(crate) fn pointer_message(runtime: &mut Runtime, message: &MSG) {
    let phase = match message.message {
        WM_POINTERDOWN => PointerPhase::Down,
        WM_POINTERUPDATE => PointerPhase::Move,
        WM_POINTERUP => PointerPhase::Up,
        _ => return,
    };
    let pointer_id = u32::from(loword(message.wParam));
    let mut info = POINTER_INFO::default();
    // SAFETY: `info` is a valid, exclusively borrowed `POINTER_INFO`;
    // `pointer_id` came from this message's `wParam`, which is what
    // `GetPointerInfo` is documented to accept while the message is being
    // processed.
    if unsafe { GetPointerInfo(pointer_id, &raw mut info) } == 0 {
        return;
    }
    let pressure = (info.pointerType == PT_PEN).then(|| {
        let mut pen = POINTER_PEN_INFO::default();
        // SAFETY: as for `GetPointerInfo` above.
        let known = unsafe { GetPointerPenInfo(pointer_id, &raw mut pen) } != 0;
        // Pen pressure is reported in 0..=1024.
        #[allow(clippy::cast_precision_loss)]
        let pressure = pen.pressure as f32 / 1024.0;
        known.then_some(pressure)
    });
    touch_or_pen(runtime, message.hwnd, pointer_id, phase, &info, pressure.flatten());
}

/// Delivers one touch or pen sample Windows described with `info`, which
/// arrived addressed to `hwnd`. Split from [`pointer_message`] so the
/// targeting, capture, and gesture logic can be driven by a test with a
/// `POINTER_INFO` of its own; only the `GetPointerInfo` call is not.
pub(crate) fn touch_or_pen(
    runtime: &mut Runtime,
    hwnd: HWND,
    pointer_id: u32,
    phase: PointerPhase,
    info: &POINTER_INFO,
    pressure: Option<f32>,
) {
    let kind = match info.pointerType {
        PT_TOUCH => PointerKind::Touch,
        PT_PEN => PointerKind::Pen,
        _ => return,
    };
    let phase = if phase == PointerPhase::Up && info.pointerFlags & POINTER_FLAG_CANCELED != 0 {
        PointerPhase::Cancel
    } else {
        phase
    };

    let target = runtime.input.pointer.captured(pointer_id).or_else(|| {
        interested_ancestor(runtime, hwnd, |i| i.wants_pointer() || i.wants_gestures())
    });
    let Some(target) = target else {
        return;
    };
    if phase == PointerPhase::Down && runtime.input.pointer.captured(pointer_id).is_none() {
        // Touch and pen contacts are implicitly captured for their whole
        // lifetime on every platform this framework targets; Windows does
        // the same for the native window, and this mirrors it at the node
        // level.
        runtime.input.pointer.captures.insert(pointer_id, Capture { node: target, implicit: true });
    }

    let mut sample = PointerEvent::new(
        pointer_id,
        kind,
        local_point(runtime, target, info.ptPixelLocation),
        now(runtime),
    )
    .with_modifiers(modifiers());
    if phase == PointerPhase::Down || phase == PointerPhase::Up {
        sample = sample.with_button(PointerButton::Primary);
    }
    if phase != PointerPhase::Up && phase != PointerPhase::Cancel {
        sample = sample.with_buttons(PointerButtons::none().with(PointerButton::Primary));
    }
    if let Some(pressure) = pressure {
        sample = sample.with_pressure(pressure);
    }

    deliver(runtime, target, phase, &sample);
    if matches!(phase, PointerPhase::Up | PointerPhase::Cancel) {
        runtime.input.pointer.captures.remove(&pointer_id);
    }
}

/// `WM_POINTERCAPTURECHANGED`: a touch or pen contact was taken away from
/// the window it was implicitly captured to (a system edge gesture, another
/// window capturing it). The node it was routed to gets a cancel.
pub(crate) fn pointer_capture_changed(runtime: &mut Runtime, message: &MSG) {
    let pointer_id = u32::from(loword(message.wParam));
    if pointer_id == MOUSE_POINTER_ID {
        return;
    }
    let Some(capture) = runtime.input.pointer.captures.remove(&pointer_id) else {
        return;
    };
    let mut info = POINTER_INFO::default();
    // SAFETY: as in `pointer_message`; a failure (the pointer is already
    // gone) leaves `info` zeroed, which the fallbacks below handle.
    let known = unsafe { GetPointerInfo(pointer_id, &raw mut info) } != 0;
    let kind =
        if known && info.pointerType == PT_PEN { PointerKind::Pen } else { PointerKind::Touch };
    let position = if known {
        local_point(runtime, capture.node, info.ptPixelLocation)
    } else {
        Point::new(0, 0)
    };
    let sample = PointerEvent::new(pointer_id, kind, position, now(runtime));
    deliver(runtime, capture.node, PointerPhase::Cancel, &sample);
}

/// Delivers one sample to `target`: as a pointer event if it wants
/// pointers, and to its gesture recognizer if it wants gestures.
fn deliver(runtime: &mut Runtime, target: NodeId, phase: PointerPhase, sample: &PointerEvent) {
    let wants = interest(runtime, target);
    // Input on a canvas says which of its drawn regions it landed in, tested
    // at the center of the pixel the sample is in.
    let region = runtime.renderer.snapshot.get(target).and_then(|node| {
        let list = node.draw_list.as_ref()?;
        let position = sample.position();
        #[allow(clippy::cast_precision_loss, reason = "a pixel coordinate inside one window")]
        let (x, y) = (position.x as f32 + 0.5, position.y as f32 + 0.5);
        list.hit_test(x, y)
    });
    let region_sample;
    let sample = if region.is_some() {
        region_sample = sample.clone().with_region(region);
        &region_sample
    } else {
        sample
    };
    if wants.wants_pointer() {
        let event = match phase {
            PointerPhase::Down => Event::PointerDown { target, pointer: sample.clone() },
            PointerPhase::Move => Event::PointerMove { target, pointer: sample.clone() },
            PointerPhase::Up => Event::PointerUp { target, pointer: sample.clone() },
            PointerPhase::Cancel => Event::PointerCancel { target, pointer: sample.clone() },
        };
        if !runtime.dispatch_or_quit(event) {
            return;
        }
    }
    if wants.wants_gestures() {
        let gestures =
            runtime.input.pointer.recognizers.entry(target).or_default().handle(phase, sample);
        // Gesture arbitration (`rustnative_core::arbitrate`): inside a
        // scrolling container, a pan belongs to the container unless the
        // node's policy claims it.
        let pan_winner = if inside_scroll_container(runtime, target) {
            rustnative_core::arbitrate(
                rustnative_core::GestureConflict::ScrollVsPan,
                wants.policy(),
            )
        } else {
            rustnative_core::Winner::Framework
        };
        for gesture in gestures {
            if matches!(gesture, rustnative_core::Gesture::Pan { .. })
                && !pan_winner.framework_reports()
            {
                continue;
            }
            if !runtime.dispatch_or_quit(Event::Gesture { target, gesture }) {
                return;
            }
        }
        arm_long_press_timer(runtime);
    }
}

/// Keeps the long-press timer pointed at the earliest pending deadline of
/// any recognizer in this window, or stops it when none is pending.
fn arm_long_press_timer(runtime: &Runtime) {
    let now = runtime.input.pointer.epoch.elapsed();
    let next = runtime
        .input
        .pointer
        .recognizers
        .values()
        .filter_map(GestureRecognizer::next_deadline)
        .min();
    match next {
        Some(deadline) => {
            let wait = deadline.saturating_sub(now).as_millis();
            // A long-press threshold is well under a second; `max(1)`
            // keeps an already-due deadline from being `SetTimer(0)`, which
            // Windows clamps to its minimum anyway.
            let wait = u32::try_from(wait).unwrap_or(u32::MAX).max(1);
            // SAFETY: `runtime.window` is this runtime's live top-level HWND;
            // a null callback means `WM_TIMER` is posted to it.
            let armed = unsafe { SetTimer(runtime.window, LONG_PRESS_TIMER_ID, wait, None) } != 0;
            best_effort(
                armed,
                "SetTimer(long press)",
                "a long press is recognized on the next input",
            );
        }
        None => {
            // SAFETY: as above; killing a timer that is not set is a
            // documented, harmless failure.
            ignored_by_contract(unsafe { KillTimer(runtime.window, LONG_PRESS_TIMER_ID) });
        }
    }
}

/// `WM_TIMER` for [`LONG_PRESS_TIMER_ID`].
pub(crate) fn long_press_timer(runtime: &mut Runtime) {
    let now = runtime.input.pointer.epoch.elapsed();
    let fired: Vec<_> = runtime
        .input
        .pointer
        .recognizers
        .iter_mut()
        .flat_map(|(node, recognizer)| {
            recognizer.tick(now).into_iter().map(|gesture| (*node, gesture)).collect::<Vec<_>>()
        })
        .collect();
    for (target, gesture) in fired {
        if !runtime.dispatch_or_quit(Event::Gesture { target, gesture }) {
            return;
        }
    }
    arm_long_press_timer(runtime);
}

/// Whether `target` sits inside a container that scrolls.
fn inside_scroll_container(runtime: &Runtime, target: NodeId) -> bool {
    let snapshot = runtime.renderer.snapshot();
    let mut current = snapshot.get(target).and_then(|node| node.parent);
    while let Some(id) = current {
        let Some(node) = snapshot.get(id) else { return false };
        let overflow = node
            .column_style
            .map(|style| style.overflow)
            .or(node.row_style.map(|style| style.overflow));
        if overflow == Some(rustnative_core::Overflow::Scroll) {
            return true;
        }
        current = node.parent;
    }
    false
}

/// A wheel message: delivered to the innermost wheel-interested node under
/// the pointer, unless a scrollable container is nearer, in which case that
/// container scrolls (as it did before this milestone). Returns whether the
/// wheel was handled here and must not also reach the native control.
pub(crate) fn wheel(runtime: &mut Runtime, message: &MSG, under: HWND) -> bool {
    let Some(target) = interested_ancestor(runtime, under, InputInterest::wants_wheel) else {
        return false;
    };
    if runtime.renderer.scrollable_ancestor_below(under, target).is_some() {
        return false;
    }
    // The high word of `wParam` is the signed wheel delta in `WHEEL_DELTA`
    // units — the same 1/120-notch resolution `WheelDelta::Lines` uses.
    #[allow(clippy::cast_possible_wrap)]
    let raw = i32::from(hiword(message.wParam) as i16);
    // Windows: positive `WM_MOUSEWHEEL` means rotated away from the user
    // (content moves up); positive `WM_MOUSEHWHEEL` means tilted right.
    // `WheelDelta` uses reading direction (positive y = scroll down), so
    // the vertical sign flips and the horizontal one does not. Windows has
    // no pixel-granular wheel message; precision touchpads report
    // fractional notches in these same units, which `Lines` carries
    // losslessly.
    let delta = if message.message == WM_MOUSEHWHEEL {
        WheelDelta::Lines { x: raw, y: 0 }
    } else {
        WheelDelta::Lines { x: 0, y: -raw }
    };
    runtime.dispatch_or_quit(Event::Wheel { target, delta });
    true
}

fn capture(runtime: &mut Runtime, pointer_id: u32, node: NodeId, implicit: bool) {
    runtime.input.pointer.captures.insert(pointer_id, Capture { node, implicit });
    if pointer_id == MOUSE_POINTER_ID {
        // SAFETY: `runtime.window` is this runtime's live top-level HWND.
        // `SetCapture` returns the previous capture window, not a status.
        let _previous = unsafe { SetCapture(runtime.window) };
    }
}

fn release(runtime: &mut Runtime, pointer_id: u32) {
    if runtime.input.pointer.captures.remove(&pointer_id).is_none() {
        return;
    }
    // SAFETY: `GetCapture` takes no arguments.
    if pointer_id == MOUSE_POINTER_ID && unsafe { GetCapture() } == runtime.window {
        // SAFETY: releases this thread's capture, which the check above
        // established is ours.
        let released = unsafe { ReleaseCapture() } != 0;
        best_effort(released, "ReleaseCapture", "the capture ends at the next button release");
    }
}

fn end_implicit_capture(runtime: &mut Runtime, pointer_id: u32) {
    if runtime.input.pointer.captures.get(&pointer_id).is_some_and(|capture| capture.implicit) {
        release(runtime, pointer_id);
    }
}

/// Applies a component's deferred capture request.
pub(crate) fn apply_request(runtime: &mut Runtime, request: InputRequest) {
    match request {
        InputRequest::CapturePointer { node, pointer_id } => {
            if runtime.renderer.snapshot.contains(node) {
                capture(runtime, pointer_id, node, false);
            }
        }
        InputRequest::ReleasePointer { node, pointer_id }
            if runtime.input.pointer.captured(pointer_id) == Some(node) =>
        {
            release(runtime, pointer_id);
        }
        // A release for a capture the node does not hold, drag feedback
        // (handled by `native::input`), and any future request kind.
        _ => {}
    }
}

/// [`WM_FRAMEWORK_CAPTURE_LOST`]: if this window believes it holds the
/// mouse but Windows says otherwise, the capture was taken away (another
/// window called `SetCapture`, a modal loop began, the window was
/// deactivated) and the capturing node is told with a cancel.
pub(crate) fn capture_lost(runtime: &mut Runtime) {
    let Some(capture) = runtime.input.pointer.captures.get(&MOUSE_POINTER_ID).copied() else {
        return;
    };
    // SAFETY: `GetCapture` takes no arguments.
    if unsafe { GetCapture() } == runtime.window {
        return;
    }
    runtime.input.pointer.captures.remove(&MOUSE_POINTER_ID);
    let mut cursor = POINT { x: 0, y: 0 };
    // SAFETY: `cursor` is a valid, exclusively borrowed `POINT`.
    let _ = unsafe { GetCursorPos(&raw mut cursor) };
    let sample = PointerEvent::new(
        MOUSE_POINTER_ID,
        PointerKind::Mouse,
        local_point(runtime, capture.node, cursor),
        now(runtime),
    );
    deliver(runtime, capture.node, PointerPhase::Cancel, &sample);
}

/// Drops pointer bookkeeping for nodes a render removed, releasing a mouse
/// capture a removed node held.
pub(crate) fn prune(runtime: &mut Runtime) {
    let removed: Vec<u32> = runtime
        .input
        .pointer
        .captures
        .iter()
        .filter(|(_, capture)| !runtime.renderer.snapshot.contains(capture.node))
        .map(|(pointer, _)| *pointer)
        .collect();
    for pointer in removed {
        release(runtime, pointer);
    }
    let snapshot = &runtime.renderer.snapshot;
    runtime.input.pointer.recognizers.retain(|id, _| snapshot.contains(*id));
    if runtime.input.pointer.hover.is_some_and(|id| !snapshot.contains(id)) {
        runtime.input.pointer.hover = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(kind: u32, wparam: WPARAM) -> MSG {
        MSG { message: kind, wParam: wparam, ..MSG::default() }
    }

    #[test]
    fn every_mouse_button_message_maps_to_its_phase_and_button() {
        assert_eq!(mouse_phase(&message(WM_MOUSEMOVE, 0)), Some((PointerPhase::Move, None)));
        assert_eq!(
            mouse_phase(&message(WM_RBUTTONUP, 0)),
            Some((PointerPhase::Up, Some(PointerButton::Secondary)))
        );
        assert_eq!(
            mouse_phase(&message(WM_XBUTTONDOWN, 1 << 16)),
            Some((PointerPhase::Down, Some(PointerButton::Back)))
        );
        assert_eq!(
            mouse_phase(&message(WM_XBUTTONUP, 2 << 16)),
            Some((PointerPhase::Up, Some(PointerButton::Forward)))
        );
        assert_eq!(
            mouse_phase(&message(WM_LBUTTONDBLCLK, 0)),
            Some((PointerPhase::Down, Some(PointerButton::Primary))),
            "a double-click's second press is still a press"
        );
        assert_eq!(mouse_phase(&message(WM_MOUSEHWHEEL, 0)), None);
    }

    #[test]
    fn held_buttons_are_decoded_from_the_mk_flags() {
        let buttons = mouse_buttons(0x0001 | 0x0010 | 0x0040);
        assert!(buttons.contains(PointerButton::Primary));
        assert!(buttons.contains(PointerButton::Middle));
        assert!(buttons.contains(PointerButton::Forward));
        assert!(!buttons.contains(PointerButton::Secondary));
    }
}
