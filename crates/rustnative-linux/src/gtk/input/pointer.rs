//! Pointer, touch, pen, hover, and wheel input for nodes that declared
//! interest in it (`rustnative_core::InputInterest`).
//!
//! One `GtkEventControllerLegacy` per window, in the capture phase, sees
//! every pointer event before any widget does. The sample is hit-tested
//! against GTK's own picking (`gtk_widget_pick`), walked up to the nearest
//! node that wants it, and delivered — as pointer events, to its gesture
//! recognizer, or both. Nothing is consumed except a wheel a node took, so
//! GTK's own widgets under the pointer behave exactly as they would
//! without the framework.
//!
//! | Portable | GDK |
//! |---|---|
//! | mouse, pointer id 0 | `GDK_SOURCE_MOUSE`, `_TOUCHPAD`, `_TRACKPOINT` button and motion events |
//! | touch, one pointer id per contact | touch events, one id per `GdkEventSequence` |
//! | pen, pointer id 1, with pressure | `GDK_SOURCE_PEN` events and their pressure axis |
//! | wheel lines (1/120 notch) | discrete scroll, and smooth scroll in wheel units |
//! | wheel pixels | smooth scroll in surface units (touchpads) |

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use rustnative_core::{
    Event, Gesture, GestureConflict, InputInterest, InputRequest, KeyModifiers, NodeId, Point,
    PointerButton, PointerButtons, PointerEvent, PointerKind, PointerPhase, WheelDelta, WindowId,
    Winner,
};

use super::super::backend::{Work, answer, post};
use super::super::layout_widget::RnLayout;
use super::super::registry::{WindowRegistry, WindowRuntime};
use super::keys;

/// Which node a contact is routed to, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Capture {
    pub(crate) node: NodeId,
    /// Taken by the backend for one press (every touch contact, and a
    /// press on a gesture-interested node), ending with it; otherwise
    /// requested by a component, ending when it releases.
    pub(crate) implicit: bool,
}

/// The mouse's pointer id.
pub(crate) const MOUSE: u32 = 0;
/// A pen's pointer id (a desktop has one pen at a time).
pub(crate) const PEN: u32 = 1;
/// Touch contacts are numbered from here.
const FIRST_TOUCH: u32 = 16;

/// What a GDK event says, in the window root's coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Raw {
    /// A contact sample.
    Sample {
        phase: PointerPhase,
        pointer: u32,
        sequence: Option<usize>,
        kind: PointerKind,
        x: f64,
        y: f64,
        button: Option<PointerButton>,
        buttons: PointerButtons,
        modifiers: KeyModifiers,
        pressure: Option<f64>,
    },
    /// The pointer left the window.
    Left,
    /// A scroll at a point.
    Scroll { x: f64, y: f64, delta: WheelDelta },
}

/// The legacy controller's handler.
pub(crate) fn legacy_event(
    window: WindowId,
    native: &gtk::Native,
    root: &RnLayout,
    event: &gdk::Event,
) -> glib::Propagation {
    let Some(raw) = translate(native, root, event) else {
        return glib::Propagation::Proceed;
    };
    answer(window, |registry| registry.pointer(window, raw)).unwrap_or(glib::Propagation::Proceed)
}

fn kind_of(event: &gdk::Event) -> PointerKind {
    match event.device().map(|device| device.source()) {
        Some(gdk::InputSource::Touchscreen) => PointerKind::Touch,
        Some(gdk::InputSource::Pen | gdk::InputSource::TabletPad) => PointerKind::Pen,
        _ => PointerKind::Mouse,
    }
}

fn release_mask(number: u32) -> gdk::ModifierType {
    match number {
        1 => gdk::ModifierType::BUTTON1_MASK,
        2 => gdk::ModifierType::BUTTON2_MASK,
        3 => gdk::ModifierType::BUTTON3_MASK,
        8 => gdk::ModifierType::BUTTON4_MASK,
        9 => gdk::ModifierType::BUTTON5_MASK,
        _ => gdk::ModifierType::empty(),
    }
}

fn button_of(number: u32) -> Option<PointerButton> {
    match number {
        1 => Some(PointerButton::Primary),
        2 => Some(PointerButton::Middle),
        3 => Some(PointerButton::Secondary),
        8 => Some(PointerButton::Back),
        9 => Some(PointerButton::Forward),
        _ => None,
    }
}

/// The buttons held in `state`.
pub(crate) fn buttons_of(state: gdk::ModifierType) -> PointerButtons {
    let mut buttons = PointerButtons::none();
    for (mask, button) in [
        (gdk::ModifierType::BUTTON1_MASK, PointerButton::Primary),
        (gdk::ModifierType::BUTTON2_MASK, PointerButton::Middle),
        (gdk::ModifierType::BUTTON3_MASK, PointerButton::Secondary),
        (gdk::ModifierType::BUTTON4_MASK, PointerButton::Back),
        (gdk::ModifierType::BUTTON5_MASK, PointerButton::Forward),
    ] {
        if state.contains(mask) {
            buttons = buttons.with(button);
        }
    }
    buttons
}

/// A scroll event's delta, in reading direction (positive y scrolls down).
pub(crate) fn wheel_delta(
    direction: gdk::ScrollDirection,
    deltas: (f64, f64),
    wheel_units: bool,
) -> Option<WheelDelta> {
    let notch = WheelDelta::DETENT;
    Some(match direction {
        gdk::ScrollDirection::Up => WheelDelta::Lines { x: 0, y: -notch },
        gdk::ScrollDirection::Down => WheelDelta::Lines { x: 0, y: notch },
        gdk::ScrollDirection::Left => WheelDelta::Lines { x: -notch, y: 0 },
        gdk::ScrollDirection::Right => WheelDelta::Lines { x: notch, y: 0 },
        gdk::ScrollDirection::Smooth => {
            let (dx, dy) = deltas;
            #[allow(
                clippy::cast_possible_truncation,
                reason = "a scroll delta, rounded; far inside i32's range"
            )]
            let scaled = |value: f64, by: f64| (value * by).round() as i32;
            if wheel_units {
                WheelDelta::Lines {
                    x: scaled(dx, f64::from(notch)),
                    y: scaled(dy, f64::from(notch)),
                }
            } else {
                WheelDelta::Pixels { x: scaled(dx, 1.0), y: scaled(dy, 1.0) }
            }
        }
        _ => return None,
    })
}

fn translate(native: &gtk::Native, root: &RnLayout, event: &gdk::Event) -> Option<Raw> {
    let kind = kind_of(event);
    let at = || -> Option<(f64, f64)> {
        let (x, y) = event.position()?;
        let (surface_x, surface_y) = native.surface_transform();
        #[allow(clippy::cast_possible_truncation, reason = "widget coordinates are f32 in GTK")]
        let point = gtk::graphene::Point::new((x - surface_x) as f32, (y - surface_y) as f32);
        let local = native.compute_point(root, &point)?;
        Some((f64::from(local.x()), f64::from(local.y())))
    };
    let modifier = event.modifier_state();
    let sample = |phase: PointerPhase,
                  button: Option<PointerButton>,
                  pointer: u32,
                  sequence: Option<usize>| {
        let (x, y) = at()?;
        Some(Raw::Sample {
            phase,
            pointer,
            sequence,
            kind,
            x,
            y,
            button,
            buttons: buttons_of(modifier),
            modifiers: keys::modifiers(modifier),
            pressure: event.axis(gdk::AxisUse::Pressure),
        })
    };
    let mouse_or_pen = if kind == PointerKind::Pen { PEN } else { MOUSE };
    let sequence = || Some(event.event_sequence().as_ptr() as usize);
    match event.event_type() {
        gdk::EventType::ButtonPress => {
            let button = event
                .downcast_ref::<gdk::ButtonEvent>()
                .and_then(|event| button_of(event.button()));
            sample(PointerPhase::Down, button, mouse_or_pen, None)
        }
        gdk::EventType::ButtonRelease => {
            let number =
                event.downcast_ref::<gdk::ButtonEvent>().map_or(0, gdk::ButtonEvent::button);
            let (x, y) = at()?;
            // GDK reports the state before the event; the portable sample
            // carries the buttons still held after it.
            let held = modifier - release_mask(number);
            Some(Raw::Sample {
                phase: PointerPhase::Up,
                pointer: mouse_or_pen,
                sequence: None,
                kind,
                x,
                y,
                button: button_of(number),
                buttons: buttons_of(held),
                modifiers: keys::modifiers(modifier),
                pressure: event.axis(gdk::AxisUse::Pressure),
            })
        }
        gdk::EventType::MotionNotify => sample(PointerPhase::Move, None, mouse_or_pen, None),
        gdk::EventType::TouchBegin => {
            sample(PointerPhase::Down, Some(PointerButton::Primary), 0, sequence())
        }
        gdk::EventType::TouchUpdate => sample(PointerPhase::Move, None, 0, sequence()),
        gdk::EventType::TouchEnd => {
            sample(PointerPhase::Up, Some(PointerButton::Primary), 0, sequence())
        }
        gdk::EventType::TouchCancel => sample(PointerPhase::Cancel, None, 0, sequence()),
        gdk::EventType::LeaveNotify => Some(Raw::Left),
        gdk::EventType::Scroll => {
            let scroll = event.downcast_ref::<gdk::ScrollEvent>()?;
            let (x, y) = at()?;
            let wheel_units = scroll.unit() == gdk::ScrollUnit::Wheel;
            let delta = wheel_delta(scroll.direction(), scroll.deltas(), wheel_units)?;
            Some(Raw::Scroll { x, y, delta })
        }
        _ => None,
    }
}

/// The nearest node at or above `start` whose interest satisfies `wants`.
fn interested(
    runtime: &WindowRuntime,
    start: Option<NodeId>,
    wants: impl Fn(InputInterest) -> bool,
) -> Option<NodeId> {
    let snapshot = &runtime.renderer.snapshot;
    let mut current = start;
    while let Some(node) = current.and_then(|id| snapshot.get(id)) {
        if wants(node.input) {
            return Some(node.id);
        }
        current = node.parent;
    }
    None
}

/// The node realized by the deepest widget under `(x, y)` in the root.
fn node_at(runtime: &WindowRuntime, x: f64, y: f64) -> Option<NodeId> {
    let widget = runtime.root.pick(x, y, gtk::PickFlags::DEFAULT)?;
    runtime.renderer.registry.node_for_widget(&widget)
}

/// `(x, y)` in the root, in `node`'s widget's coordinates.
fn local(runtime: &WindowRuntime, node: NodeId, x: f64, y: f64) -> Point {
    #[allow(clippy::cast_possible_truncation, reason = "widget coordinates are f32 in GTK")]
    let point = gtk::graphene::Point::new(x as f32, y as f32);
    let converted = runtime
        .renderer
        .widget(node)
        .and_then(|widget| runtime.root.compute_point(widget, &point))
        .unwrap_or(point);
    #[allow(clippy::cast_possible_truncation, reason = "a pixel coordinate inside one window")]
    Point::new(converted.x().round() as i32, converted.y().round() as i32)
}

/// Whether `node` sits inside a container that scrolls.
fn inside_scroll_container(runtime: &WindowRuntime, node: NodeId) -> bool {
    let snapshot = &runtime.renderer.snapshot;
    let mut current = snapshot.get(node).and_then(|node| node.parent);
    while let Some(parent) = current.and_then(|id| snapshot.get(id)) {
        if super::super::rendering::controls::scrolls(parent) {
            return true;
        }
        current = parent.parent;
    }
    false
}

/// A scroll container strictly between `from` (walking up) and `stop`.
fn scroll_container_below(runtime: &WindowRuntime, from: Option<NodeId>, stop: NodeId) -> bool {
    let snapshot = &runtime.renderer.snapshot;
    let mut current = from;
    while let Some(node) = current.and_then(|id| snapshot.get(id)) {
        if node.id == stop {
            return false;
        }
        if super::super::rendering::controls::scrolls(node) {
            return true;
        }
        current = node.parent;
    }
    false
}

impl WindowRegistry {
    /// Routes one pointer event. Returns whether GTK should also handle it.
    pub(crate) fn pointer(
        &mut self,
        window: WindowId,
        raw: Raw,
    ) -> Result<glib::Propagation, crate::Error> {
        let Some(runtime) = self.windows.get_mut(&window) else {
            return Ok(glib::Propagation::Proceed);
        };
        match raw {
            Raw::Left => {
                self.set_hover(window, None)?;
                Ok(glib::Propagation::Proceed)
            }
            Raw::Scroll { x, y, delta } => {
                let under = node_at(runtime, x, y);
                let Some(target) = interested(runtime, under, InputInterest::wants_wheel) else {
                    return Ok(glib::Propagation::Proceed);
                };
                // A nearer scroll container scrolls, as it would without the
                // framework.
                if scroll_container_below(runtime, under, target) {
                    return Ok(glib::Propagation::Proceed);
                }
                self.dispatch(window, Event::Wheel { target, delta })?;
                Ok(glib::Propagation::Stop)
            }
            Raw::Sample {
                phase,
                pointer,
                sequence,
                kind,
                x,
                y,
                button,
                buttons,
                modifiers,
                pressure,
            } => {
                let pointer = match sequence {
                    Some(sequence) => {
                        let next = &mut runtime.input.next_sequence_id;
                        *runtime.input.sequences.entry(sequence).or_insert_with(|| {
                            let id = FIRST_TOUCH + *next;
                            *next += 1;
                            id
                        })
                    }
                    None => pointer,
                };
                let under = node_at(runtime, x, y);
                if phase == PointerPhase::Move && kind != PointerKind::Touch {
                    let hover = interested(runtime, under, InputInterest::wants_pointer);
                    self.set_hover(window, hover)?;
                }
                let Some(runtime) = self.windows.get_mut(&window) else {
                    return Ok(glib::Propagation::Proceed);
                };
                let target =
                    runtime.input.captures.get(&pointer).map(|capture| capture.node).or_else(
                        || {
                            interested(runtime, under, |interest| {
                                interest.wants_pointer() || interest.wants_gestures()
                            })
                        },
                    );
                let Some(target) = target else {
                    if matches!(phase, PointerPhase::Up | PointerPhase::Cancel) {
                        forget_sequence(runtime, sequence);
                    }
                    return Ok(glib::Propagation::Proceed);
                };
                let wants = runtime
                    .renderer
                    .snapshot
                    .get(target)
                    .map(|node| node.input)
                    .unwrap_or_default();
                if phase == PointerPhase::Down
                    && !runtime.input.captures.contains_key(&pointer)
                    && (kind == PointerKind::Touch
                        || (button == Some(PointerButton::Primary) && wants.wants_gestures()))
                {
                    runtime
                        .input
                        .captures
                        .insert(pointer, Capture { node: target, implicit: true });
                }
                let mut sample = PointerEvent::new(
                    pointer,
                    kind,
                    local(runtime, target, x, y),
                    runtime.input.epoch.elapsed(),
                )
                .with_buttons(buttons)
                .with_modifiers(modifiers);
                if let Some(button) = button {
                    sample = sample.with_button(button);
                }
                if let Some(pressure) = pressure {
                    #[allow(clippy::cast_possible_truncation, reason = "a pressure in 0–1")]
                    let pressure = pressure as f32;
                    sample = sample.with_pressure(pressure);
                }
                self.deliver(window, target, phase, sample)?;
                if let Some(runtime) = self.windows.get_mut(&window) {
                    let ended = match phase {
                        PointerPhase::Up => kind == PointerKind::Touch || buttons.is_empty(),
                        PointerPhase::Cancel => true,
                        PointerPhase::Down | PointerPhase::Move => false,
                    };
                    if ended {
                        if runtime
                            .input
                            .captures
                            .get(&pointer)
                            .is_some_and(|capture| capture.implicit)
                        {
                            runtime.input.captures.remove(&pointer);
                        }
                        forget_sequence(runtime, sequence);
                    }
                }
                Ok(glib::Propagation::Proceed)
            }
        }
    }

    fn set_hover(&mut self, window: WindowId, next: Option<NodeId>) -> Result<(), crate::Error> {
        let Some(runtime) = self.windows.get_mut(&window) else { return Ok(()) };
        let previous = runtime.input.hover;
        if previous == next {
            return Ok(());
        }
        runtime.input.hover = next;
        if let Some(previous) = previous {
            self.dispatch(window, Event::PointerLeave { target: previous })?;
        }
        if let Some(next) = next {
            self.dispatch(window, Event::PointerEnter { target: next })?;
        }
        Ok(())
    }

    /// Delivers one sample: as a pointer event if the node wants pointers,
    /// and to its gesture recognizer if it wants gestures.
    fn deliver(
        &mut self,
        window: WindowId,
        target: NodeId,
        phase: PointerPhase,
        sample: PointerEvent,
    ) -> Result<(), crate::Error> {
        let Some(runtime) = self.windows.get(&window) else { return Ok(()) };
        let wants =
            runtime.renderer.snapshot.get(target).map(|node| node.input).unwrap_or_default();
        // Input on a canvas says which of its drawn regions it landed in,
        // tested at the center of the pixel the sample is in.
        let region = runtime.renderer.snapshot.get(target).and_then(|node| {
            let list = node.draw_list.as_ref()?;
            let position = sample.position();
            #[allow(clippy::cast_precision_loss, reason = "a pixel coordinate inside one window")]
            let (x, y) = (position.x as f32 + 0.5, position.y as f32 + 0.5);
            list.hit_test(x, y)
        });
        let sample = if region.is_some() { sample.with_region(region) } else { sample };
        let pan_winner = if inside_scroll_container(runtime, target) {
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
            self.dispatch(window, event)?;
        }
        if wants.wants_gestures() {
            let gestures = match self.windows.get_mut(&window) {
                Some(runtime) => {
                    runtime.input.recognizers.entry(target).or_default().handle(phase, &sample)
                }
                None => return Ok(()),
            };
            for gesture in gestures {
                // Gesture arbitration: inside a scroll container, a pan
                // belongs to the container unless the node's policy claims
                // it.
                if matches!(gesture, Gesture::Pan { .. }) && !pan_winner.framework_reports() {
                    continue;
                }
                self.dispatch(window, Event::Gesture { target, gesture })?;
            }
            self.arm_long_press(window);
        }
        Ok(())
    }

    /// Points the long-press timer at the earliest pending deadline of any
    /// recognizer in the window.
    fn arm_long_press(&mut self, window: WindowId) {
        let Some(runtime) = self.windows.get_mut(&window) else { return };
        if let Some(source) = runtime.input.long_press.take() {
            source.remove();
        }
        let now = runtime.input.epoch.elapsed();
        let next = runtime
            .input
            .recognizers
            .values()
            .filter_map(rustnative_core::GestureRecognizer::next_deadline)
            .min();
        if let Some(deadline) = next {
            let wait = deadline.saturating_sub(now).max(std::time::Duration::from_millis(1));
            runtime.input.long_press = Some(glib::timeout_add_local_once(wait, move || {
                // The source has fired; forgetting its id is the backend's job.
                post(Work::LongPress(window));
            }));
        }
    }

    /// The long-press timer fired.
    pub(crate) fn long_press(&mut self, window: WindowId) -> Result<(), crate::Error> {
        let Some(runtime) = self.windows.get_mut(&window) else { return Ok(()) };
        runtime.input.long_press = None;
        let now = runtime.input.epoch.elapsed();
        let fired: Vec<(NodeId, Gesture)> = runtime
            .input
            .recognizers
            .iter_mut()
            .flat_map(|(node, recognizer)| {
                recognizer.tick(now).into_iter().map(|gesture| (*node, gesture)).collect::<Vec<_>>()
            })
            .collect();
        for (target, gesture) in fired {
            self.dispatch(window, Event::Gesture { target, gesture })?;
        }
        self.arm_long_press(window);
        Ok(())
    }
}

fn forget_sequence(runtime: &mut WindowRuntime, sequence: Option<usize>) {
    if let Some(sequence) = sequence {
        runtime.input.sequences.remove(&sequence);
    }
}

/// Applies a component's deferred capture request.
pub(crate) fn apply_request(runtime: &mut WindowRuntime, request: InputRequest) {
    match request {
        InputRequest::CapturePointer { node, pointer_id }
            if runtime.renderer.snapshot.contains(node) =>
        {
            runtime.input.captures.insert(pointer_id, Capture { node, implicit: false });
        }
        InputRequest::ReleasePointer { node, pointer_id }
            if runtime
                .input
                .captures
                .get(&pointer_id)
                .is_some_and(|capture| capture.node == node) =>
        {
            runtime.input.captures.remove(&pointer_id);
        }
        _ => {}
    }
}

/// Forgets input state of nodes a render removed.
pub(crate) fn prune(runtime: &mut WindowRuntime) {
    let snapshot = &runtime.renderer.snapshot;
    runtime.input.captures.retain(|_, capture| snapshot.contains(capture.node));
    runtime.input.recognizers.retain(|id, _| snapshot.contains(*id));
    if runtime.input.hover.is_some_and(|id| !snapshot.contains(id)) {
        runtime.input.hover = None;
    }
    if runtime.input.focused.is_some_and(|id| !snapshot.contains(id)) {
        runtime.input.focused = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_directions_are_in_reading_direction() {
        let notch = WheelDelta::DETENT;
        assert_eq!(
            wheel_delta(gdk::ScrollDirection::Down, (0.0, 0.0), true),
            Some(WheelDelta::Lines { x: 0, y: notch })
        );
        assert_eq!(
            wheel_delta(gdk::ScrollDirection::Up, (0.0, 0.0), true),
            Some(WheelDelta::Lines { x: 0, y: -notch })
        );
        assert_eq!(
            wheel_delta(gdk::ScrollDirection::Smooth, (0.0, 0.5), true),
            Some(WheelDelta::Lines { x: 0, y: notch / 2 })
        );
        assert_eq!(
            wheel_delta(gdk::ScrollDirection::Smooth, (3.0, -12.4), false),
            Some(WheelDelta::Pixels { x: 3, y: -12 })
        );
    }

    #[test]
    fn button_numbers_and_masks_map() {
        assert_eq!(button_of(1), Some(PointerButton::Primary));
        assert_eq!(button_of(3), Some(PointerButton::Secondary));
        assert_eq!(button_of(8), Some(PointerButton::Back));
        let held = buttons_of(gdk::ModifierType::BUTTON1_MASK | gdk::ModifierType::BUTTON3_MASK);
        assert_eq!(
            held,
            PointerButtons::none().with(PointerButton::Primary).with(PointerButton::Secondary)
        );
    }
}
