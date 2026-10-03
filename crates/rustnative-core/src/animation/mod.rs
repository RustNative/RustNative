//! Time-based animation: what can animate, how it moves, and the timeline
//! that turns declarations into per-frame values.
//!
//! # Animation is not a rerender
//!
//! `PLAN.md`'s animation milestone states the constraint this module is
//! built around: *"Animations must not turn into full component rerenders
//! on every frame. Frame updates should target only the native properties
//! that need to change."*
//!
//! So an animation never touches component state. A [`Timeline`] holds the
//! running animations for one window, is advanced by a frame signal, and
//! produces [`Frame`]s — "this node's opacity is now 0.4" — that a backend
//! applies directly to native objects. The declarative tree is untouched
//! from the first frame to the last.
//!
//! # Two ways to animate
//!
//! - **Transitions** are declarative: a node says *how* a property should
//!   move ([`Node::with_transition`](crate::Node::with_transition)), and
//!   whenever a render changes that property the backend animates from the
//!   value currently on screen to the new one. Interrupting an in-flight
//!   transition retargets it from where it is now, so a value never jumps.
//! - **Explicit animations** are imperative: a component asks for one
//!   through [`ComponentContext::animations`](crate::ComponentContext::animations)
//!   and can cancel it by node and property. They belong to the component
//!   that started them and are cancelled when it unmounts, exactly like
//!   tasks.
//!
//! # Time comes from outside
//!
//! [`Timeline::tick`] is *given* the current time rather than reading a
//! clock, and a [`FrameClock`] supplies it at runtime. That is what makes
//! every behavior here — easing curves, spring settling, retargeting,
//! repeats — testable without waiting for real frames (see
//! [`ManualFrameClock`]).
//!
//! # Reduced motion
//!
//! A person who has asked their system for less motion gets it: with
//! [`MotionPreference::Reduced`], a transition completes on its first
//! frame, and an explicit animation does whatever its
//! [`ReducedMotion`] says.

mod easing;
mod interpolate;
mod matched;
mod overrides;
mod timeline;

pub use matched::{MatchedGeometry, matched_geometry};
pub use overrides::{AnimatedOverrides, NodeOverrides};

pub use easing::Easing;
pub use timeline::{Finished, Frame, TickOutput, Timeline};

use std::time::Duration;

use crate::identity::{ComponentId, NodeId};
use crate::input::Scalar;
use crate::layout::{Point, Size};
use crate::style::Color;

/// A native property an animation can drive.
///
/// Deliberately a small, closed set: each one is something a platform can
/// change on a realized object without re-laying-out or rerendering
/// anything. Layout, text, and structure are not animatable — those are
/// what a render is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum AnimatedProperty {
    /// The node's position within its parent, as layout computed it.
    /// Transitioning this animates a node that layout moved.
    Position,
    /// The node's size, as layout computed it.
    Size,
    /// An offset *added* to the node's laid-out position, resting at
    /// `(0, 0)`. This is what an explicit animation moves; it never fights
    /// with layout.
    Translation,
    /// Opacity, from `0.0` (invisible) to `1.0`.
    Opacity,
    /// The node's background color.
    Background,
    /// The node's foreground (text) color.
    Foreground,
}

impl AnimatedProperty {
    /// The value this property rests at when nothing is animating it, for
    /// the properties whose rest value is a constant rather than something
    /// layout or the theme decides.
    #[must_use]
    pub const fn constant_rest(self) -> Option<AnimatedValue> {
        match self {
            Self::Translation => Some(AnimatedValue::Offset(Point::new(0, 0))),
            _ => None,
        }
    }
}

/// A value an animation interpolates.
///
/// A [`Frame`] only ever carries the variant its property uses; a mismatch
/// (an opacity animated toward a color) cannot interpolate and is reported
/// as such rather than guessed at — see [`AnimatedValue::interpolate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnimatedValue {
    /// A position or translation, in logical pixels.
    Offset(Point),
    /// A size, in logical pixels.
    Size(Size),
    /// A unitless number (opacity).
    Scalar(Scalar),
    /// A color.
    Color(Color),
}

impl AnimatedValue {
    /// Opacity as a value.
    #[must_use]
    pub fn opacity(value: f32) -> Self {
        Self::Scalar(Scalar::new(value.clamp(0.0, 1.0)))
    }

    /// `self` moved `progress` of the way toward `other` (`0.0` is `self`,
    /// `1.0` is `other`), or `None` if the two are different kinds of
    /// value.
    ///
    /// `progress` may fall outside `0.0..=1.0` — a spring overshoots, and
    /// so do some easing curves — and the result extrapolates accordingly,
    /// except that opacity stays within its own range.
    #[must_use]
    pub fn interpolate(self, other: Self, progress: f32) -> Option<Self> {
        interpolate::between(self, other, progress)
    }
}

/// Whether the person asked their system for less motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MotionPreference {
    /// Animate normally.
    #[default]
    Full,
    /// Minimize motion (Windows' "Show animations", macOS' "Reduce
    /// motion", `prefers-reduced-motion`).
    Reduced,
}

/// What an explicit animation does when motion is reduced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ReducedMotion {
    /// Jump straight to the end: the outcome without the movement. The
    /// right default — most animation is decoration over a state change
    /// that still has to happen.
    #[default]
    Skip,
    /// Run anyway. For animation that *is* the content (a progress
    /// indicator, a media transport), where skipping would lose meaning.
    Run,
}

/// How many times an animation repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Repeat {
    /// Once.
    #[default]
    Once,
    /// A fixed number of times (`0` behaves as [`Self::Once`]).
    Times(u32),
    /// Until cancelled.
    Forever,
}

/// What a finished animation leaves behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Fill {
    /// The property returns to its resting value — what layout, the theme,
    /// or the node declares. The CSS default, and the safe one: an
    /// animation cannot permanently disagree with the rendered tree.
    #[default]
    None,
    /// The property stays at the animation's final value until something
    /// else changes it.
    Forwards,
}

/// How a property moves, for a transition or an explicit animation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    duration: Duration,
    easing: Easing,
    delay: Duration,
}

impl Transition {
    /// A transition over `duration`, eased in and out, with no delay.
    #[must_use]
    pub const fn new(duration: Duration) -> Self {
        Self { duration, easing: Easing::EaseInOut, delay: Duration::ZERO }
    }

    /// A spring, which settles when it stops moving rather than after a
    /// fixed duration (see [`Easing::Spring`]).
    #[must_use]
    pub fn spring(stiffness: f32, damping: f32, mass: f32) -> Self {
        Self {
            duration: Duration::ZERO,
            easing: Easing::spring(stiffness, damping, mass),
            delay: Duration::ZERO,
        }
    }

    /// Sets the easing curve.
    #[must_use]
    pub const fn easing(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }

    /// Waits `delay` before starting.
    #[must_use]
    pub const fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// The duration (zero for a spring, which settles on its own).
    #[must_use]
    pub const fn duration_hint(&self) -> Duration {
        self.duration
    }

    /// The easing curve.
    #[must_use]
    pub const fn easing_curve(&self) -> Easing {
        self.easing
    }

    /// The delay before it starts.
    #[must_use]
    pub const fn start_delay(&self) -> Duration {
        self.delay
    }
}

impl Default for Transition {
    /// 200 ms, eased in and out — a short, unobtrusive default.
    fn default() -> Self {
        Self::new(Duration::from_millis(200))
    }
}

/// An explicit animation a component asks for.
///
/// # Example
///
/// ```
/// use std::time::Duration;
///
/// use rustnative_core::{AnimatedProperty, AnimatedValue, Animation, Point, Transition};
///
/// // Slide in from the left, and stay where the layout puts it.
/// let slide = Animation::new(
///     AnimatedProperty::Translation,
///     AnimatedValue::Offset(Point::new(0, 0)),
///     Transition::new(Duration::from_millis(180)),
/// )
/// .from(AnimatedValue::Offset(Point::new(-40, 0)));
/// assert_eq!(slide.property(), AnimatedProperty::Translation);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Animation {
    property: AnimatedProperty,
    from: Option<AnimatedValue>,
    to: AnimatedValue,
    transition: Transition,
    repeat: Repeat,
    fill: Fill,
    reduced: ReducedMotion,
    autoreverse: bool,
}

impl Animation {
    /// An animation of `property` toward `to`, starting from wherever the
    /// property is now, moving as `transition` says.
    #[must_use]
    pub const fn new(
        property: AnimatedProperty,
        to: AnimatedValue,
        transition: Transition,
    ) -> Self {
        Self {
            property,
            from: None,
            to,
            transition,
            repeat: Repeat::Once,
            fill: Fill::None,
            reduced: ReducedMotion::Skip,
            autoreverse: false,
        }
    }

    /// Starts from `from` instead of the property's current value.
    #[must_use]
    pub const fn from(mut self, from: AnimatedValue) -> Self {
        self.from = Some(from);
        self
    }

    /// Repeats (see [`Repeat`]).
    #[must_use]
    pub const fn repeat(mut self, repeat: Repeat) -> Self {
        self.repeat = repeat;
        self
    }

    /// Plays every other repetition backwards.
    #[must_use]
    pub const fn autoreverse(mut self, autoreverse: bool) -> Self {
        self.autoreverse = autoreverse;
        self
    }

    /// What the finished animation leaves behind (see [`Fill`]).
    #[must_use]
    pub const fn fill(mut self, fill: Fill) -> Self {
        self.fill = fill;
        self
    }

    /// What to do when motion is reduced (see [`ReducedMotion`]).
    #[must_use]
    pub const fn reduced_motion(mut self, reduced: ReducedMotion) -> Self {
        self.reduced = reduced;
        self
    }

    /// The property this animates.
    #[must_use]
    pub const fn property(&self) -> AnimatedProperty {
        self.property
    }

    /// The declared starting value, if it does not start from the
    /// property's current value.
    #[must_use]
    pub const fn start_value(&self) -> Option<AnimatedValue> {
        self.from
    }

    /// The value it animates to.
    #[must_use]
    pub const fn end_value(&self) -> AnimatedValue {
        self.to
    }

    /// How it moves.
    #[must_use]
    pub const fn transition(&self) -> Transition {
        self.transition
    }

    /// How many times it repeats.
    #[must_use]
    pub const fn repetitions(&self) -> Repeat {
        self.repeat
    }

    /// Whether alternate repetitions play backwards.
    #[must_use]
    pub const fn is_autoreversed(&self) -> bool {
        self.autoreverse
    }

    /// What it leaves behind.
    #[must_use]
    pub const fn fill_mode(&self) -> Fill {
        self.fill
    }

    /// What it does when motion is reduced.
    #[must_use]
    pub const fn reduced_behavior(&self) -> ReducedMotion {
        self.reduced
    }
}

/// Identifies one running animation, for cancelling it and for reporting
/// that it finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AnimationId(u64);

impl AnimationId {
    /// The underlying value, for diagnostics and for a backend's own
    /// bookkeeping.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }
}

/// Who started an animation, so it can be cancelled when they go away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimationOwner {
    /// A component; the animation ends when it unmounts.
    Component(ComponentId),
    /// The backend itself — a transition, which lives as long as the node
    /// it animates.
    Transition(NodeId),
}

/// A source of frame times.
///
/// The backend's real clock reads a monotonic timer; [`ManualFrameClock`]
/// is told what time it is, which is what makes animation behavior
/// testable to the millisecond.
pub trait FrameClock: std::fmt::Debug {
    /// The time now, on a monotonic clock whose origin does not matter.
    fn now(&self) -> Duration;
}

/// A [`FrameClock`] that only moves when told to.
#[derive(Debug, Default, Clone)]
pub struct ManualFrameClock {
    now: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl ManualFrameClock {
    /// A clock at zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Moves the clock forward by `delta`.
    pub fn advance(&self, delta: Duration) {
        let millis = u64::try_from(delta.as_millis()).unwrap_or(u64::MAX);
        self.now.fetch_add(millis, std::sync::atomic::Ordering::Relaxed);
    }

    /// Sets the clock to `now`.
    pub fn set(&self, now: Duration) {
        let millis = u64::try_from(now.as_millis()).unwrap_or(u64::MAX);
        self.now.store(millis, std::sync::atomic::Ordering::Relaxed);
    }
}

impl FrameClock for ManualFrameClock {
    fn now(&self) -> Duration {
        Duration::from_millis(self.now.load(std::sync::atomic::Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manual_clock_only_moves_when_told_to() {
        let clock = ManualFrameClock::new();
        assert_eq!(clock.now(), Duration::ZERO);
        clock.advance(Duration::from_millis(16));
        clock.advance(Duration::from_millis(16));
        assert_eq!(clock.now(), Duration::from_millis(32));
        clock.set(Duration::from_secs(1));
        assert_eq!(clock.now(), Duration::from_secs(1));
    }

    #[test]
    fn translation_rests_at_the_origin_and_the_others_rest_where_the_tree_says() {
        assert_eq!(
            AnimatedProperty::Translation.constant_rest(),
            Some(AnimatedValue::Offset(Point::new(0, 0)))
        );
        assert_eq!(AnimatedProperty::Position.constant_rest(), None);
        assert_eq!(AnimatedProperty::Opacity.constant_rest(), None);
    }

    #[test]
    fn opacity_values_are_clamped_to_their_range() {
        assert_eq!(AnimatedValue::opacity(2.0), AnimatedValue::Scalar(Scalar::ONE));
        assert_eq!(AnimatedValue::opacity(-1.0), AnimatedValue::Scalar(Scalar::ZERO));
    }
}
