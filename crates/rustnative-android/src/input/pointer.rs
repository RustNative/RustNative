//! Pointer, touch, pen, hover, and wheel input for nodes that declared
//! interest in it (`rustnative_core::InputInterest`).
//!
//! The window's root `RnLayout` offers every `MotionEvent` to the framework
//! before any view sees it (`RnLayout.dispatchTouchEvent` and
//! `dispatchGenericMotionEvent`). Each sample is hit-tested against the
//! view hierarchy (`RnViews.hitTest`), walked up to the nearest node that
//! wants it, and delivered — as pointer events, to its gesture recognizer,
//! or both. Nothing is claimed except a wheel a node took, so the views
//! under the finger behave exactly as they would without the framework; a
//! framework pan that wins arbitration inside a scroll container stops the
//! container intercepting (`requestDisallowInterceptTouchEvent`).
//!
//! | Portable | Android |
//! |---|---|
//! | mouse, pointer id 0 | `TOOL_TYPE_MOUSE`, with `getButtonState` |
//! | pen, pointer id 1, with pressure | `TOOL_TYPE_STYLUS` and `TOOL_TYPE_ERASER` |
//! | touch, pointer id 16 + Android's id | `TOOL_TYPE_FINGER` (and `UNKNOWN`) |
//! | hover enter/leave | `ACTION_HOVER_MOVE`/`HOVER_EXIT` (mouse and pen) |
//! | wheel lines (1/120 notch) | `ACTION_SCROLL`'s `AXIS_VSCROLL`/`AXIS_HSCROLL`, sign flipped to reading direction |

use std::collections::HashMap;
use std::time::Duration;

use rustnative_core::{
    KeyModifiers, NodeId, PointerButton, PointerButtons, PointerKind, PointerPhase, WheelDelta,
};

use super::PointerFrame;

/// Floats per pointer in a frame.
pub(crate) const VALUES: usize = 7;

// `MotionEvent` actions.
const ACTION_DOWN: i32 = 0;
const ACTION_UP: i32 = 1;
const ACTION_MOVE: i32 = 2;
const ACTION_CANCEL: i32 = 3;
const ACTION_POINTER_DOWN: i32 = 5;
const ACTION_POINTER_UP: i32 = 6;
const ACTION_HOVER_MOVE: i32 = 7;
const ACTION_SCROLL: i32 = 8;
const ACTION_HOVER_ENTER: i32 = 9;
const ACTION_HOVER_EXIT: i32 = 10;

// `MotionEvent` tool types.
const TOOL_TYPE_STYLUS: i32 = 2;
const TOOL_TYPE_MOUSE: i32 = 3;
const TOOL_TYPE_ERASER: i32 = 4;

// `MotionEvent` button state.
const BUTTON_PRIMARY: i32 = 1;
const BUTTON_SECONDARY: i32 = 2;
const BUTTON_TERTIARY: i32 = 4;
const BUTTON_BACK: i32 = 8;
const BUTTON_FORWARD: i32 = 16;
const BUTTON_STYLUS_PRIMARY: i32 = 32;
const BUTTON_STYLUS_SECONDARY: i32 = 64;

/// The mouse's pointer id.
pub(crate) const MOUSE: u32 = 0;
/// A pen's pointer id.
pub(crate) const PEN: u32 = 1;
/// Touch contacts are numbered from here.
const FIRST_TOUCH: u32 = 16;

/// One sample of a frame, in the root's pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Raw {
    /// A contact sample.
    Sample {
        phase: PointerPhase,
        pointer: u32,
        kind: PointerKind,
        x: f32,
        y: f32,
        button: Option<PointerButton>,
        buttons: PointerButtons,
        modifiers: KeyModifiers,
        pressure: Option<f32>,
        /// A hover sample: no contact.
        hover: bool,
    },
    /// The pointer left the window.
    Left,
    /// A wheel at a point.
    Scroll { x: f32, y: f32, delta: WheelDelta },
}

fn kind_of(tool: i32) -> PointerKind {
    match tool {
        TOOL_TYPE_MOUSE => PointerKind::Mouse,
        TOOL_TYPE_STYLUS | TOOL_TYPE_ERASER => PointerKind::Pen,
        _ => PointerKind::Touch,
    }
}

/// The buttons held in Android's button state.
pub(crate) fn buttons_of(state: i32) -> PointerButtons {
    let mut buttons = PointerButtons::none();
    for (mask, button) in [
        (BUTTON_PRIMARY | BUTTON_STYLUS_PRIMARY, PointerButton::Primary),
        (BUTTON_SECONDARY | BUTTON_STYLUS_SECONDARY, PointerButton::Secondary),
        (BUTTON_TERTIARY, PointerButton::Middle),
        (BUTTON_BACK, PointerButton::Back),
        (BUTTON_FORWARD, PointerButton::Forward),
    ] {
        if state & mask != 0 {
            buttons = buttons.with(button);
        }
    }
    buttons
}

/// The samples one frame stands for.
pub(crate) fn translate(frame: &PointerFrame, previous_buttons: i32) -> Vec<Raw> {
    let count = frame.ids.len().min(frame.tools.len()).min(frame.values.len() / VALUES);
    let modifiers = super::keys::modifiers(frame.meta);
    let pointer_at = |index: usize| -> Option<Raw> {
        let id = *frame.ids.get(index)?;
        let tool = *frame.tools.get(index)?;
        let at = index * VALUES;
        let kind = kind_of(tool);
        let pointer = match kind {
            PointerKind::Mouse => MOUSE,
            PointerKind::Pen => PEN,
            PointerKind::Touch => FIRST_TOUCH + u32::try_from(id).unwrap_or(0),
        };
        Some(Raw::Sample {
            phase: PointerPhase::Move,
            pointer,
            kind,
            x: frame.values[at],
            y: frame.values[at + 1],
            button: None,
            buttons: if kind == PointerKind::Mouse {
                buttons_of(frame.buttons)
            } else {
                PointerButtons::none()
            },
            modifiers,
            pressure: (kind == PointerKind::Pen).then_some(frame.values[at + 2].clamp(0.0, 1.0)),
            hover: false,
        })
    };
    let with = |raw: Raw, phase: PointerPhase, button: Option<PointerButton>, hover: bool| match raw
    {
        Raw::Sample { pointer, kind, x, y, buttons, modifiers, pressure, .. } => {
            let buttons = match (kind, phase, button) {
                // A finger or pen tip holds the primary button while down.
                (
                    PointerKind::Touch | PointerKind::Pen,
                    PointerPhase::Down | PointerPhase::Move,
                    _,
                ) if !hover => buttons.with(PointerButton::Primary),
                _ => buttons,
            };
            Raw::Sample { phase, pointer, kind, x, y, button, buttons, modifiers, pressure, hover }
        }
        other => other,
    };
    let index = usize::try_from(frame.action_index).unwrap_or(0);
    match frame.action {
        ACTION_DOWN | ACTION_POINTER_DOWN => {
            let Some(raw) = pointer_at(index) else { return Vec::new() };
            let button = match raw {
                Raw::Sample { kind: PointerKind::Mouse, .. } => {
                    pressed_button(frame.buttons & !previous_buttons)
                        .or(Some(PointerButton::Primary))
                }
                _ => Some(PointerButton::Primary),
            };
            vec![with(raw, PointerPhase::Down, button, false)]
        }
        ACTION_UP | ACTION_POINTER_UP => {
            let Some(raw) = pointer_at(index) else { return Vec::new() };
            let button = match raw {
                Raw::Sample { kind: PointerKind::Mouse, .. } => {
                    pressed_button(previous_buttons & !frame.buttons)
                        .or(Some(PointerButton::Primary))
                }
                _ => Some(PointerButton::Primary),
            };
            vec![with(raw, PointerPhase::Up, button, false)]
        }
        ACTION_MOVE => (0..count)
            .filter_map(pointer_at)
            .map(|raw| with(raw, PointerPhase::Move, None, false))
            .collect(),
        ACTION_CANCEL => (0..count)
            .filter_map(pointer_at)
            .map(|raw| with(raw, PointerPhase::Cancel, None, false))
            .collect(),
        ACTION_HOVER_ENTER | ACTION_HOVER_MOVE => (0..count)
            .filter_map(pointer_at)
            .map(|raw| with(raw, PointerPhase::Move, None, true))
            .collect(),
        ACTION_HOVER_EXIT => vec![Raw::Left],
        ACTION_SCROLL => {
            let Some(at) = (count > 0).then_some(0) else { return Vec::new() };
            let base = at * VALUES;
            let (horizontal, vertical) = (frame.values[base + 5], frame.values[base + 6]);
            #[allow(
                clippy::cast_possible_truncation,
                reason = "a wheel delta in notches times the detent, rounded"
            )]
            let lines = |value: f32| {
                (value * f32::from(i16::try_from(WheelDelta::DETENT).unwrap_or(120))).round() as i32
            };
            // Android's positive vertical axis is the wheel rolled away
            // (content moves down); the portable delta is in reading
            // direction (positive scrolls down).
            vec![Raw::Scroll {
                x: frame.values[base],
                y: frame.values[base + 1],
                delta: WheelDelta::Lines { x: lines(horizontal), y: -lines(vertical) },
            }]
        }
        _ => Vec::new(),
    }
}

fn pressed_button(changed: i32) -> Option<PointerButton> {
    [
        (BUTTON_PRIMARY, PointerButton::Primary),
        (BUTTON_SECONDARY, PointerButton::Secondary),
        (BUTTON_TERTIARY, PointerButton::Middle),
        (BUTTON_BACK, PointerButton::Back),
        (BUTTON_FORWARD, PointerButton::Forward),
    ]
    .into_iter()
    .find(|(mask, _)| changed & mask != 0)
    .map(|(_, button)| button)
}

/// Which node a contact is routed to, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Capture {
    pub(crate) node: NodeId,
    /// Taken by the backend for one press, ending with it; otherwise
    /// requested by a component, ending when it releases.
    pub(crate) implicit: bool,
}

/// One window's pointer state.
#[derive(Debug, Default)]
pub(crate) struct PointerState {
    pub(crate) captures: HashMap<u32, Capture>,
    pub(crate) recognizers: HashMap<NodeId, rustnative_core::GestureRecognizer>,
    pub(crate) hover: Option<NodeId>,
    /// The button state of the last frame (to tell which button changed).
    pub(crate) buttons: i32,
    /// The long-press timer's token, while armed.
    pub(crate) long_press: Option<i64>,
    /// The first event's time (ms of uptime), the portable clock's zero.
    pub(crate) epoch: Option<i64>,
}

impl PointerState {
    /// The portable timestamp of an event at `time` (ms of uptime).
    pub(crate) fn timestamp(&mut self, time: i64) -> Duration {
        let epoch = *self.epoch.get_or_insert(time);
        Duration::from_millis(u64::try_from(time.saturating_sub(epoch)).unwrap_or(0))
    }
}

#[cfg(target_os = "android")]
pub(crate) use platform::{apply_requests, long_press, offer};

#[cfg(target_os = "android")]
mod platform {
    use rustnative_core::{
        Event, Gesture, GestureConflict, InputInterest, InputRequest, NodeId, Point, PointerEvent,
        PointerKind, PointerPhase, WindowId, Winner,
    };

    use super::{Capture, Raw, translate};
    use crate::Error;
    use crate::input::PointerFrame;
    use crate::jni_host::{Arg, Class, call_static};
    use crate::registry::WindowRegistry;

    /// The node under `(x, y)` (root pixels) and the point in that node's
    /// view, in dp.
    fn hit(registry: &WindowRegistry, window: WindowId, x: f32, y: f32) -> Option<(NodeId, Point)> {
        let runtime = registry.windows.get(&window)?;
        let root = runtime.root.as_ref()?;
        let renderer = runtime.renderer.as_ref()?;
        let found = call_static(
            Class::Views,
            "hitTest",
            "(Landroid/view/View;FF)[I",
            &[Arg::Obj(root), Arg::Float(x), Arg::Float(y)],
        )
        .ok()?
        .ints();
        let (&tag, &local_x, &local_y) = (found.first()?, found.get(1)?, found.get(2)?);
        let node = renderer.registry.node_for_tag(tag)?;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_precision_loss,
            reason = "a pixel coordinate inside one window, over the density"
        )]
        let dp = |pixels: i32| (pixels as f32 / runtime.density).round() as i32;
        Some((node, Point::new(dp(local_x), dp(local_y))))
    }

    /// The local point of `(x, y)` in `node`'s view (for a captured
    /// contact that left it), in dp.
    fn local(registry: &WindowRegistry, window: WindowId, node: NodeId, x: f32, y: f32) -> Point {
        let Some(runtime) = registry.windows.get(&window) else { return Point::new(0, 0) };
        let Some(view) = runtime.renderer.as_ref().and_then(|renderer| renderer.view(node)) else {
            return Point::new(0, 0);
        };
        let Some(root) = runtime.root.as_ref() else { return Point::new(0, 0) };
        let at = call_static(
            Class::Views,
            "toLocal",
            "(Landroid/view/View;Landroid/view/View;FF)[I",
            &[Arg::Obj(root), Arg::Obj(view), Arg::Float(x), Arg::Float(y)],
        )
        .map(crate::jni_host::Ret::ints)
        .unwrap_or_default();
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_precision_loss,
            reason = "a pixel coordinate inside one window, over the density"
        )]
        let dp = |pixels: i32| (pixels as f32 / runtime.density).round() as i32;
        Point::new(dp(at.first().copied().unwrap_or(0)), dp(at.get(1).copied().unwrap_or(0)))
    }

    fn interested(
        registry: &WindowRegistry,
        window: WindowId,
        start: Option<NodeId>,
        wants: impl Fn(InputInterest) -> bool,
    ) -> Option<NodeId> {
        let snapshot = &registry.windows.get(&window)?.renderer.as_ref()?.snapshot;
        let mut current = start;
        while let Some(node) = current.and_then(|id| snapshot.get(id)) {
            if wants(node.input) {
                return Some(node.id);
            }
            current = node.parent;
        }
        None
    }

    fn snapshot(
        registry: &WindowRegistry,
        window: WindowId,
    ) -> Option<&rustnative_core::TreeSnapshot> {
        registry
            .windows
            .get(&window)
            .and_then(|runtime| runtime.renderer.as_ref())
            .map(|renderer| &renderer.snapshot)
    }

    /// A scroll container at or above `from`, strictly below `stop`.
    fn scroll_container_below(
        registry: &WindowRegistry,
        window: WindowId,
        from: Option<NodeId>,
        stop: NodeId,
    ) -> bool {
        let Some(snapshot) = snapshot(registry, window) else { return false };
        let mut current = from;
        while let Some(node) = current.and_then(|id| snapshot.get(id)) {
            if node.id == stop {
                return false;
            }
            if crate::rendering::controls::scrolls(node) {
                return true;
            }
            current = node.parent;
        }
        false
    }

    /// Whether `node` sits inside a container that scrolls.
    fn inside_scroll_container(registry: &WindowRegistry, window: WindowId, node: NodeId) -> bool {
        let Some(snapshot) = snapshot(registry, window) else { return false };
        let mut current = snapshot.get(node).and_then(|node| node.parent);
        while let Some(parent) = current.and_then(|id| snapshot.get(id)) {
            if crate::rendering::controls::scrolls(parent) {
                return true;
            }
            current = parent.parent;
        }
        false
    }

    /// Routes one frame. Returns whether the framework claims it.
    pub(crate) fn offer(
        registry: &mut WindowRegistry,
        window: WindowId,
        frame: &PointerFrame,
    ) -> Result<bool, Error> {
        let Some(runtime) = registry.windows.get_mut(&window) else { return Ok(false) };
        let previous = std::mem::replace(&mut runtime.input.pointer.buttons, frame.buttons);
        let timestamp = runtime.input.pointer.timestamp(frame.time);
        let mut claimed = false;
        for raw in translate(frame, previous) {
            claimed |= route(registry, window, raw, timestamp)?;
        }
        Ok(claimed)
    }

    fn route(
        registry: &mut WindowRegistry,
        window: WindowId,
        raw: Raw,
        timestamp: std::time::Duration,
    ) -> Result<bool, Error> {
        match raw {
            Raw::Left => {
                set_hover(registry, window, None)?;
                Ok(false)
            }
            Raw::Scroll { x, y, delta } => {
                let under = hit(registry, window, x, y).map(|(node, _)| node);
                let Some(target) = interested(registry, window, under, InputInterest::wants_wheel)
                else {
                    return Ok(false);
                };
                // A nearer scroll container scrolls, as it would without
                // the framework.
                if scroll_container_below(registry, window, under, target) {
                    return Ok(false);
                }
                registry.dispatch(window, Event::Wheel { target, delta })?;
                Ok(true)
            }
            Raw::Sample {
                phase,
                pointer,
                kind,
                x,
                y,
                button,
                buttons,
                modifiers,
                pressure,
                hover,
            } => {
                if let Some(runtime) = registry.windows.get_mut(&window) {
                    runtime.input.fine_pointer = kind != PointerKind::Touch;
                }
                let found = hit(registry, window, x, y);
                let under = found.map(|(node, _)| node);
                if phase == PointerPhase::Move && kind != PointerKind::Touch {
                    let next = interested(registry, window, under, InputInterest::wants_pointer);
                    set_hover(registry, window, next)?;
                }
                if hover {
                    // A hovering pen or mouse with no button: only hover.
                    let target = interested(registry, window, under, InputInterest::wants_pointer);
                    if let Some(target) = target {
                        let at = local(registry, window, target, x, y);
                        let sample = PointerEvent::new(pointer, kind, at, timestamp)
                            .with_modifiers(modifiers);
                        registry
                            .dispatch(window, Event::PointerMove { target, pointer: sample })?;
                    }
                    return Ok(false);
                }
                let captured = registry
                    .windows
                    .get(&window)
                    .and_then(|runtime| runtime.input.pointer.captures.get(&pointer).copied());
                let target = captured.map(|capture| capture.node).or_else(|| {
                    interested(registry, window, under, |interest| {
                        interest.wants_pointer() || interest.wants_gestures()
                    })
                });
                let Some(target) = target else { return Ok(false) };
                let wants = registry
                    .windows
                    .get(&window)
                    .and_then(|runtime| runtime.renderer.as_ref())
                    .and_then(|renderer| renderer.snapshot.get(target))
                    .map(|node| node.input)
                    .unwrap_or_default();
                if phase == PointerPhase::Down
                    && captured.is_none()
                    && (kind == PointerKind::Touch
                        || (button == Some(rustnative_core::PointerButton::Primary)
                            && wants.wants_gestures()))
                {
                    if let Some(runtime) = registry.windows.get_mut(&window) {
                        runtime
                            .input
                            .pointer
                            .captures
                            .insert(pointer, Capture { node: target, implicit: true });
                    }
                }
                let mut sample = PointerEvent::new(
                    pointer,
                    kind,
                    local(registry, window, target, x, y),
                    timestamp,
                )
                .with_buttons(buttons)
                .with_modifiers(modifiers);
                if let Some(button) = button {
                    sample = sample.with_button(button);
                }
                if let Some(pressure) = pressure {
                    sample = sample.with_pressure(pressure);
                }
                deliver(registry, window, target, phase, sample)?;
                if matches!(phase, PointerPhase::Up | PointerPhase::Cancel) {
                    if let Some(runtime) = registry.windows.get_mut(&window) {
                        if runtime
                            .input
                            .pointer
                            .captures
                            .get(&pointer)
                            .is_some_and(|capture| capture.implicit)
                        {
                            runtime.input.pointer.captures.remove(&pointer);
                        }
                    }
                }
                Ok(false)
            }
        }
    }

    fn set_hover(
        registry: &mut WindowRegistry,
        window: WindowId,
        next: Option<NodeId>,
    ) -> Result<(), Error> {
        let Some(runtime) = registry.windows.get_mut(&window) else { return Ok(()) };
        let previous = runtime.input.pointer.hover;
        if previous == next {
            return Ok(());
        }
        runtime.input.pointer.hover = next;
        if let Some(previous) = previous {
            registry.dispatch(window, Event::PointerLeave { target: previous })?;
        }
        if let Some(next) = next {
            registry.dispatch(window, Event::PointerEnter { target: next })?;
        }
        Ok(())
    }

    /// Delivers one sample: as a pointer event if the node wants pointers,
    /// and to its gesture recognizer if it wants gestures.
    fn deliver(
        registry: &mut WindowRegistry,
        window: WindowId,
        target: NodeId,
        phase: PointerPhase,
        sample: PointerEvent,
    ) -> Result<(), Error> {
        let Some(renderer) =
            registry.windows.get(&window).and_then(|runtime| runtime.renderer.as_ref())
        else {
            return Ok(());
        };
        let node = renderer.snapshot.get(target);
        let wants = node.map(|node| node.input).unwrap_or_default();
        let region = node.and_then(|node| {
            let list = node.draw_list.as_ref()?;
            let position = sample.position();
            #[allow(clippy::cast_precision_loss, reason = "a coordinate inside one window")]
            let (x, y) = (position.x as f32 + 0.5, position.y as f32 + 0.5);
            list.hit_test(x, y)
        });
        let sample = if region.is_some() { sample.with_region(region) } else { sample };
        let inside_scroll = inside_scroll_container(registry, window, target);
        let pan_winner = if inside_scroll {
            rustnative_core::arbitrate(GestureConflict::ScrollVsPan, wants.policy())
        } else {
            Winner::Framework
        };
        if wants.wants_pointer() {
            let event = match phase {
                PointerPhase::Down => Event::PointerDown { target, pointer: sample.clone() },
                PointerPhase::Move => Event::PointerMove { target, pointer: sample.clone() },
                PointerPhase::Up => Event::PointerUp { target, pointer: sample.clone() },
                PointerPhase::Cancel => Event::PointerCancel { target, pointer: sample.clone() },
            };
            registry.dispatch(window, event)?;
        }
        if wants.wants_gestures() {
            let gestures = match registry.windows.get_mut(&window) {
                Some(runtime) => runtime
                    .input
                    .pointer
                    .recognizers
                    .entry(target)
                    .or_default()
                    .handle(phase, &sample),
                None => return Ok(()),
            };
            for gesture in gestures {
                if matches!(gesture, Gesture::Pan { .. }) {
                    if !pan_winner.framework_reports() {
                        continue;
                    }
                    if inside_scroll {
                        // The framework's pan won: the scroll container
                        // must stop intercepting this sequence.
                        claim_sequence(registry, window, target);
                    }
                }
                registry.dispatch(window, Event::Gesture { target, gesture })?;
            }
            arm_long_press(registry, window)?;
        }
        Ok(())
    }

    fn claim_sequence(registry: &WindowRegistry, window: WindowId, target: NodeId) {
        let Some(view) = registry
            .windows
            .get(&window)
            .and_then(|runtime| runtime.renderer.as_ref())
            .and_then(|renderer| renderer.view(target))
        else {
            return;
        };
        let _ =
            call_static(Class::Views, "claimSequence", "(Landroid/view/View;)V", &[Arg::Obj(view)]);
    }

    /// Points the long-press timer at the earliest pending deadline.
    fn arm_long_press(registry: &mut WindowRegistry, window: WindowId) -> Result<(), Error> {
        let Some(runtime) = registry.windows.get_mut(&window) else { return Ok(()) };
        let now = runtime
            .input
            .pointer
            .epoch
            .map_or(std::time::Duration::ZERO, |_| current_timestamp(runtime));
        let next = runtime
            .input
            .pointer
            .recognizers
            .values()
            .filter_map(rustnative_core::GestureRecognizer::next_deadline)
            .min();
        runtime.input.pointer.long_press = None;
        if let Some(deadline) = next {
            let wait = deadline.saturating_sub(now).max(std::time::Duration::from_millis(1));
            let token = crate::timers::schedule(crate::timers::Timer::LongPress(window), wait)?;
            if let Some(runtime) = registry.windows.get_mut(&window) {
                runtime.input.pointer.long_press = Some(token);
            }
        }
        Ok(())
    }

    fn current_timestamp(runtime: &mut crate::registry::WindowRuntime) -> std::time::Duration {
        let uptime = call_static(Class::Bridge, "uptimeMillis", "()J", &[])
            .map_or(0, crate::jni_host::Ret::long);
        runtime.input.pointer.timestamp(uptime)
    }

    /// The long-press timer fired.
    pub(crate) fn long_press(
        registry: &mut WindowRegistry,
        window: WindowId,
        token: i64,
    ) -> Result<(), Error> {
        let Some(runtime) = registry.windows.get_mut(&window) else { return Ok(()) };
        if runtime.input.pointer.long_press != Some(token) {
            return Ok(());
        }
        runtime.input.pointer.long_press = None;
        let now = current_timestamp(runtime);
        let fired: Vec<(NodeId, Gesture)> = runtime
            .input
            .pointer
            .recognizers
            .iter_mut()
            .flat_map(|(node, recognizer)| {
                recognizer.tick(now).into_iter().map(|gesture| (*node, gesture)).collect::<Vec<_>>()
            })
            .collect();
        for (target, gesture) in fired {
            registry.dispatch(window, Event::Gesture { target, gesture })?;
        }
        arm_long_press(registry, window)
    }

    /// Applies components' deferred input requests.
    pub(crate) fn apply_requests(
        registry: &mut WindowRegistry,
        window: WindowId,
        requests: Vec<InputRequest>,
    ) -> Result<(), Error> {
        for request in requests {
            let Some(runtime) = registry.windows.get_mut(&window) else { return Ok(()) };
            let present = |node: NodeId| {
                runtime.renderer.as_ref().is_some_and(|renderer| renderer.snapshot.contains(node))
            };
            match request {
                InputRequest::CapturePointer { node, pointer_id } if present(node) => {
                    runtime
                        .input
                        .pointer
                        .captures
                        .insert(pointer_id, Capture { node, implicit: false });
                }
                InputRequest::ReleasePointer { node, pointer_id }
                    if runtime
                        .input
                        .pointer
                        .captures
                        .get(&pointer_id)
                        .is_some_and(|capture| capture.node == node) =>
                {
                    runtime.input.pointer.captures.remove(&pointer_id);
                }
                other => crate::input::drag::apply_request(registry, window, other)?,
            }
        }
        if let Some(runtime) = registry.windows.get_mut(&window) {
            if let Some(renderer) = runtime.renderer.as_ref() {
                let snapshot = &renderer.snapshot;
                runtime.input.pointer.captures.retain(|_, capture| snapshot.contains(capture.node));
                runtime.input.pointer.recognizers.retain(|id, _| snapshot.contains(*id));
                if runtime.input.pointer.hover.is_some_and(|id| !snapshot.contains(id)) {
                    runtime.input.pointer.hover = None;
                }
                if runtime.input.focused.is_some_and(|id| !snapshot.contains(id)) {
                    runtime.input.focused = None;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(
        action: i32,
        index: i32,
        pointers: &[(i32, i32, f32, f32)],
        buttons: i32,
    ) -> PointerFrame {
        let mut values = Vec::new();
        for (_, _, x, y) in pointers {
            values.extend([*x, *y, 0.5, 0.0, 0.0, 0.0, 0.0]);
        }
        PointerFrame {
            action,
            action_index: index,
            ids: pointers.iter().map(|pointer| pointer.0).collect(),
            tools: pointers.iter().map(|pointer| pointer.1).collect(),
            values,
            buttons,
            meta: 0,
            time: 0,
        }
    }

    #[test]
    fn fingers_are_touch_contacts_numbered_from_sixteen() {
        let down = translate(
            &frame(ACTION_POINTER_DOWN, 1, &[(0, 1, 1.0, 2.0), (3, 1, 30.0, 40.0)], 0),
            0,
        );
        let [Raw::Sample { phase, pointer, kind, x, buttons, button, .. }] = down[..] else {
            panic!("one sample: {down:?}")
        };
        assert_eq!((phase, pointer, kind), (PointerPhase::Down, 19, PointerKind::Touch));
        assert!((x - 30.0).abs() < f32::EPSILON);
        assert_eq!(button, Some(PointerButton::Primary));
        assert!(buttons.contains(PointerButton::Primary));
        let moved =
            translate(&frame(ACTION_MOVE, 0, &[(0, 1, 1.0, 2.0), (3, 1, 31.0, 41.0)], 0), 0);
        assert_eq!(moved.len(), 2);
    }

    #[test]
    fn a_pen_reports_its_pressure_and_a_mouse_its_buttons() {
        let pen = translate(&frame(ACTION_DOWN, 0, &[(0, 2, 5.0, 5.0)], 0), 0);
        assert!(
            matches!(pen[0], Raw::Sample { pointer: PEN, kind: PointerKind::Pen, pressure: Some(p), .. } if (p - 0.5).abs() < f32::EPSILON)
        );
        let right = translate(&frame(ACTION_DOWN, 0, &[(0, 3, 5.0, 5.0)], BUTTON_SECONDARY), 0);
        assert!(matches!(
            right[0],
            Raw::Sample { pointer: MOUSE, button: Some(PointerButton::Secondary), .. }
        ));
        let released = translate(&frame(ACTION_UP, 0, &[(0, 3, 5.0, 5.0)], 0), BUTTON_SECONDARY);
        assert!(matches!(
            released[0],
            Raw::Sample { phase: PointerPhase::Up, button: Some(PointerButton::Secondary), .. }
        ));
    }

    #[test]
    fn hover_and_the_wheel() {
        let hover = translate(&frame(ACTION_HOVER_MOVE, 0, &[(0, 3, 5.0, 5.0)], 0), 0);
        assert!(matches!(hover[0], Raw::Sample { hover: true, phase: PointerPhase::Move, .. }));
        assert_eq!(
            translate(&frame(ACTION_HOVER_EXIT, 0, &[(0, 3, 5.0, 5.0)], 0), 0),
            vec![Raw::Left]
        );
        let mut wheel = frame(ACTION_SCROLL, 0, &[(0, 3, 5.0, 6.0)], 0);
        wheel.values[6] = -1.0; // rolled toward the person: scroll down
        let notch = WheelDelta::DETENT;
        assert_eq!(
            translate(&wheel, 0),
            vec![Raw::Scroll { x: 5.0, y: 6.0, delta: WheelDelta::Lines { x: 0, y: notch } }]
        );
    }
}
