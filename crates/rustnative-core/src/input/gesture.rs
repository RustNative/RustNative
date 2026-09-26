//! Portable gesture recognition from a pointer stream.
//!
//! Recognition lives here, not in each backend, for two reasons. First,
//! every platform would otherwise grow its own slightly different notion
//! of how far a finger may drift before a tap becomes a pan. Second, on
//! Windows specifically, the system gesture messages (`WM_GESTURE`) and the
//! pointer messages (`WM_POINTER*`) are mutually exclusive for a window, so
//! a backend that wants raw pointers — which this framework does — cannot
//! also have the OS recognize gestures for it.
//!
//! One [`GestureRecognizer`] tracks one target node. It is fed
//! [`PointerPhase`]s with [`PointerEvent`]s in that node's local
//! coordinates and a monotonic timestamp, and it is *told* the time via
//! [`GestureRecognizer::tick`] rather than reading a clock — which is what
//! makes long-press recognition deterministic under test. A backend asks
//! [`GestureRecognizer::next_deadline`] when to call `tick` next.

use std::collections::BTreeMap;
use std::time::Duration;

use super::Scalar;
use super::pointer::PointerEvent;
use crate::layout::Point;

/// Where a continuous gesture is in its lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GesturePhase {
    /// The gesture was just recognized.
    Began,
    /// The gesture continued.
    Changed,
    /// The gesture finished normally.
    Ended,
    /// The gesture was interrupted (capture lost, window deactivated).
    Cancelled,
}

/// A recognized gesture, in the target node's local coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gesture {
    /// A short press and release without significant movement.
    Tap {
        /// Where the press happened.
        position: Point,
    },
    /// A press held without significant movement past the long-press
    /// threshold. Delivered while the pointer is still down.
    LongPress {
        /// Where the press happened.
        position: Point,
    },
    /// A single-pointer drag.
    Pan {
        /// Lifecycle phase.
        phase: GesturePhase,
        /// Movement since the previous `Pan` event.
        delta: Point,
        /// Movement since the pan began (measured from the original press).
        total: Point,
    },
    /// A two-pointer pinch.
    Pinch {
        /// Lifecycle phase.
        phase: GesturePhase,
        /// Current distance between the two pointers divided by their
        /// distance when the pinch began.
        scale: Scalar,
        /// Midpoint between the two pointers.
        center: Point,
    },
}

/// How a pointer sample relates to its contact's lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerPhase {
    /// The contact began (button pressed, finger touched).
    Down,
    /// The contact moved.
    Move,
    /// The contact ended normally.
    Up,
    /// The contact was taken away (capture lost).
    Cancel,
}

/// Thresholds for gesture recognition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GestureConfig {
    /// How far (in logical pixels, per axis) a pointer may move and still
    /// count as stationary for tap/long-press purposes.
    pub slop: i32,
    /// The longest press that still counts as a tap.
    pub tap_timeout: Duration,
    /// How long a stationary press must be held to become a long press.
    pub long_press: Duration,
}

impl Default for GestureConfig {
    /// Values in line with the major platforms' own defaults (Android's
    /// `ViewConfiguration`, Windows' drag rectangle and double-click time).
    fn default() -> Self {
        Self {
            slop: 8,
            tap_timeout: Duration::from_millis(300),
            long_press: Duration::from_millis(500),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Idle,
    Pressed {
        id: u32,
        start: Point,
        since: Duration,
    },
    LongPressed {
        id: u32,
    },
    Panning {
        id: u32,
        start: Point,
        last: Point,
    },
    Pinching {
        a: u32,
        b: u32,
        initial: f64,
        last_scale: Scalar,
    },
    /// A multi-pointer gesture ended while some pointers remain down; no
    /// new gesture starts until every pointer is released, so lifting one
    /// finger of a pinch never turns into an accidental pan.
    Exhausted,
}

/// Turns one node's pointer stream into [`Gesture`]s. See the module
/// documentation.
///
/// # Example
///
/// ```
/// use std::time::Duration;
///
/// use rustnative_core::{
///     Gesture, GestureRecognizer, Point, PointerEvent, PointerKind, PointerPhase,
/// };
///
/// let mut recognizer = GestureRecognizer::default();
/// let at = |x, ms| PointerEvent::new(0, PointerKind::Touch, Point::new(x, 10), Duration::from_millis(ms));
///
/// assert!(recognizer.handle(PointerPhase::Down, &at(10, 0)).is_empty());
/// let gestures = recognizer.handle(PointerPhase::Up, &at(11, 80));
/// assert_eq!(gestures, vec![Gesture::Tap { position: Point::new(10, 10) }]);
/// ```
#[derive(Debug, Clone)]
pub struct GestureRecognizer {
    config: GestureConfig,
    pointers: BTreeMap<u32, Point>,
    state: State,
}

impl Default for GestureRecognizer {
    fn default() -> Self {
        Self::new(GestureConfig::default())
    }
}

impl GestureRecognizer {
    /// A recognizer with `config`'s thresholds.
    #[must_use]
    pub fn new(config: GestureConfig) -> Self {
        Self { config, pointers: BTreeMap::new(), state: State::Idle }
    }

    /// Feeds one pointer sample, returning any gestures it completes or
    /// advances.
    pub fn handle(&mut self, phase: PointerPhase, event: &PointerEvent) -> Vec<Gesture> {
        let id = event.pointer_id();
        let position = event.position();
        match phase {
            PointerPhase::Down => {
                self.pointers.insert(id, position);
                self.pointer_down(id, position, event.timestamp())
            }
            PointerPhase::Move => {
                if !self.pointers.contains_key(&id) {
                    // Hover movement with no contact down.
                    return Vec::new();
                }
                self.pointers.insert(id, position);
                self.pointer_moved(id, position)
            }
            PointerPhase::Up | PointerPhase::Cancel => {
                if self.pointers.remove(&id).is_none() {
                    return Vec::new();
                }
                let cancelled = phase == PointerPhase::Cancel;
                self.pointer_released(id, position, event.timestamp(), cancelled)
            }
        }
    }

    /// Advances time to `now` (on the same clock as the pointer
    /// timestamps), recognizing a long press whose threshold has passed.
    pub fn tick(&mut self, now: Duration) -> Vec<Gesture> {
        if let State::Pressed { id, start, since } = self.state {
            if now.saturating_sub(since) >= self.config.long_press {
                self.state = State::LongPressed { id };
                return vec![Gesture::LongPress { position: start }];
            }
        }
        Vec::new()
    }

    /// When [`Self::tick`] next needs to be called for a pending long
    /// press to be recognized on time, or `None` if nothing is pending.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Duration> {
        match self.state {
            State::Pressed { since, .. } => Some(since.saturating_add(self.config.long_press)),
            _ => None,
        }
    }

    /// Whether any pointer is currently down on this recognizer's node.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.pointers.is_empty()
    }

    fn pointer_down(&mut self, id: u32, position: Point, now: Duration) -> Vec<Gesture> {
        match self.state {
            State::Idle => {
                self.state = State::Pressed { id, start: position, since: now };
                Vec::new()
            }
            State::Pressed { .. } | State::LongPressed { .. } => self.begin_pinch(Vec::new()),
            State::Panning { start, last, .. } => {
                let ended = Gesture::Pan {
                    phase: GesturePhase::Ended,
                    delta: Point::new(0, 0),
                    total: difference(last, start),
                };
                self.begin_pinch(vec![ended])
            }
            // A third finger during a pinch, or any finger while waiting
            // for release, does not start anything new.
            State::Pinching { .. } | State::Exhausted => Vec::new(),
        }
    }

    fn begin_pinch(&mut self, mut gestures: Vec<Gesture>) -> Vec<Gesture> {
        let mut ids = self.pointers.keys().copied();
        let (Some(a), Some(b)) = (ids.next(), ids.next()) else {
            return gestures;
        };
        let initial = distance(self.pointers[&a], self.pointers[&b]);
        if initial < 1.0 {
            // Two contacts on the same pixel have no meaningful scale.
            self.state = State::Exhausted;
            return gestures;
        }
        self.state = State::Pinching { a, b, initial, last_scale: Scalar::ONE };
        gestures.push(Gesture::Pinch {
            phase: GesturePhase::Began,
            scale: Scalar::ONE,
            center: midpoint(self.pointers[&a], self.pointers[&b]),
        });
        gestures
    }

    fn pointer_moved(&mut self, id: u32, position: Point) -> Vec<Gesture> {
        match self.state {
            State::Pressed { id: pressed, start, .. } if pressed == id => {
                if !self.beyond_slop(start, position) {
                    return Vec::new();
                }
                self.state = State::Panning { id, start, last: position };
                let moved = difference(position, start);
                vec![Gesture::Pan { phase: GesturePhase::Began, delta: moved, total: moved }]
            }
            State::Panning { id: panning, start, last } if panning == id => {
                if position == last {
                    return Vec::new();
                }
                self.state = State::Panning { id, start, last: position };
                vec![Gesture::Pan {
                    phase: GesturePhase::Changed,
                    delta: difference(position, last),
                    total: difference(position, start),
                }]
            }
            State::Pinching { a, b, initial, last_scale } if id == a || id == b => {
                let (pa, pb) = (self.pointers[&a], self.pointers[&b]);
                let scale = scale_of(distance(pa, pb), initial);
                if scale == last_scale {
                    return Vec::new();
                }
                self.state = State::Pinching { a, b, initial, last_scale: scale };
                vec![Gesture::Pinch {
                    phase: GesturePhase::Changed,
                    scale,
                    center: midpoint(pa, pb),
                }]
            }
            _ => Vec::new(),
        }
    }

    fn pointer_released(
        &mut self,
        id: u32,
        position: Point,
        now: Duration,
        cancelled: bool,
    ) -> Vec<Gesture> {
        let end_phase = if cancelled { GesturePhase::Cancelled } else { GesturePhase::Ended };
        let gestures = match self.state {
            State::Pressed { id: pressed, start, since } if pressed == id => {
                let quick = now.saturating_sub(since) <= self.config.tap_timeout;
                if !cancelled && quick && !self.beyond_slop(start, position) {
                    vec![Gesture::Tap { position: start }]
                } else {
                    Vec::new()
                }
            }
            State::Panning { id: panning, start, last } if panning == id => {
                vec![Gesture::Pan {
                    phase: end_phase,
                    delta: difference(position, last),
                    total: difference(position, start),
                }]
            }
            State::Pinching { a, b, last_scale, .. } if id == a || id == b => {
                let other = if id == a { b } else { a };
                let center = self.pointers.get(&other).map_or(position, |p| midpoint(*p, position));
                vec![Gesture::Pinch { phase: end_phase, scale: last_scale, center }]
            }
            _ => Vec::new(),
        };
        // Whatever was happening is over for this contact. With pointers
        // still down, wait for all of them to lift before recognizing
        // anything new.
        let involved = match self.state {
            State::Pressed { id: p, .. }
            | State::LongPressed { id: p }
            | State::Panning { id: p, .. } => p == id,
            State::Pinching { a, b, .. } => id == a || id == b,
            State::Idle | State::Exhausted => true,
        };
        if involved {
            self.state = if self.pointers.is_empty() { State::Idle } else { State::Exhausted };
        }
        gestures
    }

    fn beyond_slop(&self, start: Point, position: Point) -> bool {
        let moved = difference(position, start);
        moved.x.saturating_abs() > self.config.slop || moved.y.saturating_abs() > self.config.slop
    }
}

fn difference(a: Point, b: Point) -> Point {
    Point::new(a.x.saturating_sub(b.x), a.y.saturating_sub(b.y))
}

fn distance(a: Point, b: Point) -> f64 {
    let dx = f64::from(a.x) - f64::from(b.x);
    let dy = f64::from(a.y) - f64::from(b.y);
    dx.hypot(dy)
}

fn midpoint(a: Point, b: Point) -> Point {
    let mid = |p: i32, q: i32| {
        let sum = i64::from(p) + i64::from(q);
        // The mean of two `i32`s always fits in an `i32`.
        i32::try_from(sum / 2).unwrap_or(p)
    };
    Point::new(mid(a.x, b.x), mid(a.y, b.y))
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "a pinch scale is a ratio of two on-screen distances; any value it can take is \
              far inside f32's range, and f32 precision is all a scale factor needs"
)]
fn scale_of(current: f64, initial: f64) -> Scalar {
    // Quantized to 1/1000 so sub-pixel jitter in a stationary pinch does
    // not produce a stream of "changed" events that change nothing.
    let ratio = (current / initial * 1000.0).round() / 1000.0;
    Scalar::new(ratio as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::PointerKind;

    fn sample(id: u32, x: i32, y: i32, ms: u64) -> PointerEvent {
        PointerEvent::new(id, PointerKind::Touch, Point::new(x, y), Duration::from_millis(ms))
    }

    #[test]
    fn a_quick_stationary_press_is_a_tap() {
        let mut r = GestureRecognizer::default();
        r.handle(PointerPhase::Down, &sample(0, 5, 5, 0));
        r.handle(PointerPhase::Move, &sample(0, 8, 6, 30));
        let out = r.handle(PointerPhase::Up, &sample(0, 8, 6, 100));
        assert_eq!(out, vec![Gesture::Tap { position: Point::new(5, 5) }]);
        assert!(!r.is_active());
    }

    #[test]
    fn a_slow_press_is_not_a_tap_and_a_long_press_fires_on_tick() {
        let mut r = GestureRecognizer::default();
        r.handle(PointerPhase::Down, &sample(0, 5, 5, 0));
        assert_eq!(r.next_deadline(), Some(Duration::from_millis(500)));
        assert!(r.tick(Duration::from_millis(499)).is_empty());
        assert_eq!(
            r.tick(Duration::from_millis(500)),
            vec![Gesture::LongPress { position: Point::new(5, 5) }]
        );
        assert_eq!(r.next_deadline(), None, "a long press fires once");
        assert!(r.handle(PointerPhase::Up, &sample(0, 5, 5, 900)).is_empty());
    }

    #[test]
    fn moving_past_slop_becomes_a_pan_whose_deltas_sum_to_its_total() {
        let mut r = GestureRecognizer::default();
        r.handle(PointerPhase::Down, &sample(0, 0, 0, 0));
        assert!(r.handle(PointerPhase::Move, &sample(0, 4, 0, 10)).is_empty(), "within slop");
        let mut deltas = Point::new(0, 0);
        let mut last_total = Point::new(0, 0);
        for (step, x) in [20, 35, 60].into_iter().enumerate() {
            for gesture in r.handle(PointerPhase::Move, &sample(0, x, 3, 20 + step as u64)) {
                let Gesture::Pan { delta, total, phase } = gesture else { panic!("not a pan") };
                assert_eq!(phase == GesturePhase::Began, step == 0);
                deltas = Point::new(deltas.x + delta.x, deltas.y + delta.y);
                last_total = total;
            }
        }
        assert_eq!(deltas, last_total);
        let end = r.handle(PointerPhase::Up, &sample(0, 60, 3, 100));
        assert!(matches!(end[..], [Gesture::Pan { phase: GesturePhase::Ended, .. }]));
    }

    #[test]
    fn two_contacts_pinch_and_lifting_one_does_not_pan() {
        let mut r = GestureRecognizer::default();
        r.handle(PointerPhase::Down, &sample(1, 100, 100, 0));
        let began = r.handle(PointerPhase::Down, &sample(2, 200, 100, 5));
        assert_eq!(
            began,
            vec![Gesture::Pinch {
                phase: GesturePhase::Began,
                scale: Scalar::ONE,
                center: Point::new(150, 100)
            }]
        );
        let changed = r.handle(PointerPhase::Move, &sample(2, 300, 100, 20));
        assert_eq!(
            changed,
            vec![Gesture::Pinch {
                phase: GesturePhase::Changed,
                scale: Scalar::new(2.0),
                center: Point::new(200, 100)
            }]
        );
        let ended = r.handle(PointerPhase::Up, &sample(2, 300, 100, 30));
        assert!(matches!(ended[..], [Gesture::Pinch { phase: GesturePhase::Ended, .. }]));
        assert!(
            r.handle(PointerPhase::Move, &sample(1, 400, 400, 40)).is_empty(),
            "the remaining finger must not start a pan"
        );
        assert!(r.handle(PointerPhase::Up, &sample(1, 400, 400, 50)).is_empty());
        assert!(!r.is_active());
    }

    #[test]
    fn a_second_contact_during_a_pan_ends_the_pan_and_begins_a_pinch() {
        let mut r = GestureRecognizer::default();
        r.handle(PointerPhase::Down, &sample(1, 0, 0, 0));
        r.handle(PointerPhase::Move, &sample(1, 30, 0, 10));
        let out = r.handle(PointerPhase::Down, &sample(2, 130, 0, 20));
        assert!(matches!(
            out[..],
            [
                Gesture::Pan { phase: GesturePhase::Ended, .. },
                Gesture::Pinch { phase: GesturePhase::Began, .. }
            ]
        ));
    }

    #[test]
    fn cancelling_reports_cancelled_and_never_a_tap() {
        let mut r = GestureRecognizer::default();
        r.handle(PointerPhase::Down, &sample(0, 0, 0, 0));
        assert!(r.handle(PointerPhase::Cancel, &sample(0, 0, 0, 10)).is_empty());
        r.handle(PointerPhase::Down, &sample(0, 0, 0, 20));
        r.handle(PointerPhase::Move, &sample(0, 50, 0, 30));
        let out = r.handle(PointerPhase::Cancel, &sample(0, 50, 0, 40));
        assert!(matches!(out[..], [Gesture::Pan { phase: GesturePhase::Cancelled, .. }]));
    }

    #[test]
    fn hover_movement_and_stray_releases_are_ignored() {
        let mut r = GestureRecognizer::default();
        assert!(r.handle(PointerPhase::Move, &sample(0, 10, 10, 0)).is_empty());
        assert!(r.handle(PointerPhase::Up, &sample(0, 10, 10, 0)).is_empty());
        assert!(!r.is_active());
    }
}
