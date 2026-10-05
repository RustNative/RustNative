//! The running animations of one window.
//!
//! A [`Timeline`] is pure logic over time: it is told when frames happen
//! and answers with the values each animated property should have. It
//! never reads a clock, touches a component, or knows what a native object
//! is — which is what lets a backend drive it from real frames and a test
//! drive it from a [`super::ManualFrameClock`] and get identical behavior.
//!
//! One property of one node has at most one animation at a time: starting
//! another *retargets* the running one from wherever it currently is, so an
//! interrupted animation never jumps. For a spring that also means the
//! motion keeps its direction, because the retarget starts from the
//! in-flight value rather than from the old endpoint.

use std::collections::BTreeMap;
use std::time::Duration;

use super::{
    AnimatedProperty, AnimatedValue, Animation, AnimationId, AnimationOwner, Fill,
    MotionPreference, ReducedMotion, Repeat, Transition,
};
use crate::identity::NodeId;

/// One property's value for this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The node to apply it to.
    pub node: NodeId,
    /// The property that changed.
    pub property: AnimatedProperty,
    /// The value, or `None` meaning "this animation is over: put the
    /// property back where the rendered tree says it belongs".
    pub value: Option<AnimatedValue>,
}

/// An animation that ended, and who owned it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finished {
    /// The node it animated.
    pub node: NodeId,
    /// The property it was driving.
    pub property: AnimatedProperty,
    /// Which animation.
    pub animation: AnimationId,
    /// Who started it.
    pub owner: AnimationOwner,
}

/// What one [`Timeline::tick`] produced.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TickOutput {
    /// Values to apply, in a stable order.
    pub frames: Vec<Frame>,
    /// Animations that ended on this frame.
    pub finished: Vec<Finished>,
}

#[derive(Debug, Clone)]
struct Track {
    id: AnimationId,
    owner: AnimationOwner,
    from: AnimatedValue,
    to: AnimatedValue,
    /// Where the property goes when the animation ends without
    /// [`Fill::Forwards`]: `None` means "whatever the rendered tree says".
    rest: Option<AnimatedValue>,
    transition: Transition,
    /// When motion begins — the start time plus any delay.
    start: Duration,
    repeat: Repeat,
    autoreverse: bool,
    fill: Fill,
    last: Option<AnimatedValue>,
}

/// The running animations of one window. See the module documentation.
///
/// # Example
///
/// ```
/// use std::time::Duration;
///
/// use rustnative_core::{
///     AnimatedProperty, AnimatedValue, AnimationOwner, Easing, NodeId, Point, Timeline,
///     Transition,
/// };
///
/// let node = NodeId::from_key("panel");
/// let mut timeline = Timeline::new();
/// // The node moved: animate from where it is drawn to where layout put it.
/// timeline.transition(
///     node,
///     AnimatedProperty::Position,
///     AnimatedValue::Offset(Point::new(0, 0)),
///     AnimatedValue::Offset(Point::new(0, 100)),
///     Transition::new(Duration::from_millis(100)).easing(Easing::Linear),
///     Duration::ZERO,
/// );
///
/// let half = timeline.tick(Duration::from_millis(50));
/// assert_eq!(half.frames[0].value, Some(AnimatedValue::Offset(Point::new(0, 50))));
/// assert!(half.finished.is_empty());
///
/// // Finishing hands the property back to the rendered tree, which is
/// // where it was animating to in the first place.
/// let end = timeline.tick(Duration::from_millis(100));
/// assert_eq!(end.frames[0].value, None);
/// assert_eq!(end.finished.len(), 1);
/// assert!(!timeline.is_active());
/// ```
#[derive(Debug, Default)]
pub struct Timeline {
    tracks: BTreeMap<(NodeId, AnimatedProperty), Track>,
    next_id: u64,
    motion: MotionPreference,
}

impl Timeline {
    /// An empty timeline.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets whether the person has asked for reduced motion; applies to
    /// animations started afterwards.
    pub fn set_motion_preference(&mut self, motion: MotionPreference) {
        self.motion = motion;
    }

    /// The current motion preference.
    #[must_use]
    pub const fn motion_preference(&self) -> MotionPreference {
        self.motion
    }

    /// Whether anything is animating.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.tracks.is_empty()
    }

    /// How many animations are running.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    /// Whether nothing is running.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// The animation running on a property, if any — what a cancellation
    /// request names it by.
    #[must_use]
    pub fn running_id(&self, node: NodeId, property: AnimatedProperty) -> Option<AnimationId> {
        self.tracks.get(&(node, property)).map(|track| track.id)
    }

    /// The value this timeline last produced for a property, if it is
    /// animating.
    #[must_use]
    pub fn current(&self, node: NodeId, property: AnimatedProperty) -> Option<AnimatedValue> {
        self.tracks.get(&(node, property)).and_then(|track| track.last)
    }

    /// Animates `property` from `from` to `to` because a render changed it.
    ///
    /// `from` is what is on screen now; an animation already running for
    /// this property is retargeted from its *current* value instead, so
    /// the motion stays continuous. Returns `None` when there is nothing to
    /// animate (the value did not actually change).
    pub fn transition(
        &mut self,
        node: NodeId,
        property: AnimatedProperty,
        from: AnimatedValue,
        to: AnimatedValue,
        transition: Transition,
        now: Duration,
    ) -> Option<AnimationId> {
        let from = self.retarget_from(node, property, from, now);
        if from == to {
            self.tracks.remove(&(node, property));
            return None;
        }
        let id = self.allocate();
        let reduced = self.motion == MotionPreference::Reduced;
        self.tracks.insert(
            (node, property),
            Track {
                id,
                owner: AnimationOwner::Transition(node),
                from,
                to,
                // A transition ends by handing the property back: its
                // target *is* what the rendered tree says, so the backend
                // stops overriding rather than pinning the same value
                // forever.
                rest: None,
                transition: if reduced { instant(transition) } else { transition },
                start: now.saturating_add(if reduced {
                    Duration::ZERO
                } else {
                    transition.start_delay()
                }),
                repeat: Repeat::Once,
                autoreverse: false,
                fill: Fill::None,
                last: Some(from),
            },
        );
        Some(id)
    }

    /// Starts an explicit animation on `node`, owned by `owner`.
    ///
    /// `current` is the property's present value, used when the animation
    /// does not say where to start from and as what it returns to when it
    /// ends without [`Fill::Forwards`].
    pub fn start(
        &mut self,
        node: NodeId,
        animation: &Animation,
        owner: AnimationOwner,
        current: Option<AnimatedValue>,
        now: Duration,
    ) -> AnimationId {
        let property = animation.property();
        let running = self.current(node, property);
        let from = animation
            .start_value()
            .or(running)
            .or(current)
            .unwrap_or_else(|| animation.end_value());
        let id = self.allocate();
        let skipped = self.motion == MotionPreference::Reduced
            && animation.reduced_behavior() == ReducedMotion::Skip;
        let transition = animation.transition();
        self.tracks.insert(
            (node, property),
            Track {
                id,
                owner,
                from,
                to: animation.end_value(),
                rest: property.constant_rest().or(current),
                transition: if skipped { instant(transition) } else { transition },
                start: now.saturating_add(if skipped {
                    Duration::ZERO
                } else {
                    transition.start_delay()
                }),
                // A skipped animation runs its one instant iteration.
                repeat: if skipped { Repeat::Once } else { animation.repetitions() },
                autoreverse: animation.is_autoreversed(),
                fill: animation.fill_mode(),
                last: Some(from),
            },
        );
        id
    }

    /// Stops one animation, returning the frame that puts its property
    /// back where the rendered tree says (or leaves it, for
    /// [`Fill::Forwards`]).
    pub fn cancel(&mut self, animation: AnimationId) -> Option<Frame> {
        let key =
            *self.tracks.iter().find(|(_, track)| track.id == animation).map(|(key, _)| key)?;
        let track = self.tracks.remove(&key)?;
        Some(rest_frame(key.0, key.1, &track))
    }

    /// Stops every animation `owner` started — how a component's
    /// animations end when it unmounts.
    pub fn cancel_owner(&mut self, owner: AnimationOwner) -> Vec<Frame> {
        let keys: Vec<_> = self
            .tracks
            .iter()
            .filter(|(_, track)| track.owner == owner)
            .map(|(key, _)| *key)
            .collect();
        keys.into_iter()
            .filter_map(|key| {
                self.tracks.remove(&key).map(|track| rest_frame(key.0, key.1, &track))
            })
            .collect()
    }

    /// Drops every animation on `node` without producing frames — for a
    /// node that no longer exists, where there is nothing left to restore.
    pub fn forget(&mut self, node: NodeId) {
        self.tracks.retain(|(animated, _), _| *animated != node);
    }

    /// Advances to `now`, producing this frame's values.
    pub fn tick(&mut self, now: Duration) -> TickOutput {
        let mut output = TickOutput::default();
        let mut completed = Vec::new();
        for (key, track) in &mut self.tracks {
            let (value, done) = evaluate(track, now);
            let changed = track.last != Some(value);
            track.last = Some(value);
            if done {
                completed.push(*key);
                let final_value = match track.fill {
                    // The value it actually ended on, which is not always
                    // its target: an even number of autoreversed cycles
                    // ends back where it started.
                    Fill::Forwards => Some(value),
                    Fill::None => track.rest,
                };
                output.frames.push(Frame { node: key.0, property: key.1, value: final_value });
                output.finished.push(Finished {
                    node: key.0,
                    property: key.1,
                    animation: track.id,
                    owner: track.owner,
                });
            } else if changed {
                output.frames.push(Frame { node: key.0, property: key.1, value: Some(value) });
            }
        }
        for key in completed {
            self.tracks.remove(&key);
        }
        output
    }

    fn allocate(&mut self) -> AnimationId {
        self.next_id = self.next_id.wrapping_add(1);
        AnimationId::new(self.next_id)
    }

    /// Where a new animation for this property should start from: the
    /// running animation's current value, if there is one.
    fn retarget_from(
        &mut self,
        node: NodeId,
        property: AnimatedProperty,
        fallback: AnimatedValue,
        now: Duration,
    ) -> AnimatedValue {
        match self.tracks.get(&(node, property)) {
            Some(track) => evaluate(track, now).0,
            None => fallback,
        }
    }
}

/// A copy of `transition` that completes immediately, keeping nothing but
/// the fact that it happened — how reduced motion is honored.
fn instant(transition: Transition) -> Transition {
    Transition::new(Duration::ZERO).easing(transition.easing_curve())
}

fn rest_frame(node: NodeId, property: AnimatedProperty, track: &Track) -> Frame {
    Frame {
        node,
        property,
        value: match track.fill {
            Fill::Forwards => track.last.or(Some(track.to)),
            Fill::None => track.rest,
        },
    }
}

/// This track's value at `now`, and whether it has finished.
fn evaluate(track: &Track, now: Duration) -> (AnimatedValue, bool) {
    if now < track.start {
        // Still in its delay: hold at the starting value.
        return (track.from, false);
    }
    let elapsed = now.saturating_sub(track.start);
    let easing = track.transition.easing_curve();
    let (progress, done) = if easing.is_spring() {
        // A spring settles on its own; repetition does not apply to it.
        easing.spring_progress(elapsed)
    } else {
        timed_progress(track, elapsed)
    };
    let value = track.from.interpolate(track.to, progress).unwrap_or(track.to);
    (value, done)
}

/// Progress for a duration-based curve, accounting for repeats and
/// autoreversal.
///
/// Counted in whole milliseconds rather than floating-point seconds: which
/// iteration an animation is in, and whether it is done, are integer
/// questions, and answering them in integers keeps a long-running repeat
/// from drifting.
fn timed_progress(track: &Track, elapsed: Duration) -> (f32, bool) {
    let duration = track.transition.duration_hint().as_millis();
    if duration == 0 {
        return (1.0, true);
    }
    let elapsed = elapsed.as_millis();
    let iteration = elapsed / duration;
    let total = match track.repeat {
        Repeat::Once => Some(1),
        Repeat::Times(times) => Some(u128::from(times.max(1))),
        Repeat::Forever => None,
    };
    let curve = track.transition.easing_curve();
    if total.is_some_and(|total| iteration >= total) {
        // An even number of autoreversed iterations ends back at the start.
        let ends_at_start = track.autoreverse && total.is_some_and(|total| total % 2 == 0);
        let fraction = if ends_at_start { 0.0 } else { 1.0 };
        return (curve.progress(fraction), true);
    }
    let mut fraction = fraction_of(elapsed % duration, duration);
    if track.autoreverse && iteration % 2 == 1 {
        fraction = 1.0 - fraction;
    }
    (curve.progress(fraction), false)
}

/// How far `position` is through `duration`, both in milliseconds.
#[allow(
    clippy::cast_precision_loss,
    reason = "a position within one iteration, so both values are that iteration's duration at               most — milliseconds far below f32's exact-integer range for any real animation"
)]
fn fraction_of(position: u128, duration: u128) -> f32 {
    position as f32 / duration as f32
}

#[cfg(test)]
mod tests {
    use super::super::Easing;
    use super::*;
    use crate::layout::Point;
    use crate::style::Color;

    fn node() -> NodeId {
        NodeId::from_key("animated")
    }

    fn offset(x: i32) -> AnimatedValue {
        AnimatedValue::Offset(Point::new(x, 0))
    }

    fn linear(millis: u64) -> Transition {
        Transition::new(Duration::from_millis(millis)).easing(Easing::Linear)
    }

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    fn start_linear(timeline: &mut Timeline, to: i32, at: u64) {
        timeline.transition(
            node(),
            AnimatedProperty::Translation,
            offset(0),
            offset(to),
            linear(100),
            ms(at),
        );
    }

    #[test]
    fn a_transition_to_the_same_value_does_not_animate() {
        let mut timeline = Timeline::new();
        let id = timeline.transition(
            node(),
            AnimatedProperty::Position,
            offset(5),
            offset(5),
            linear(100),
            Duration::ZERO,
        );
        assert!(id.is_none());
        assert!(!timeline.is_active());
    }

    #[test]
    fn a_delay_holds_the_starting_value_until_it_elapses() {
        let mut timeline = Timeline::new();
        timeline.transition(
            node(),
            AnimatedProperty::Translation,
            offset(0),
            offset(100),
            linear(100).delay(ms(50)),
            Duration::ZERO,
        );
        assert!(timeline.tick(ms(25)).frames.is_empty(), "no movement during the delay");
        assert_eq!(timeline.tick(ms(100)).frames[0].value, Some(offset(50)));
    }

    #[test]
    fn interrupting_a_transition_retargets_from_the_current_value() {
        let mut timeline = Timeline::new();
        start_linear(&mut timeline, 100, 0);
        let midway = timeline.tick(ms(50)).frames[0].value.unwrap();
        assert_eq!(midway, offset(50));

        // A new target arrives mid-flight, claiming to start from 0 again.
        start_linear(&mut timeline, 0, 50);
        let next = timeline.tick(ms(50)).frames;
        assert!(
            next.is_empty() || next[0].value == Some(offset(50)),
            "the new animation must begin where the old one actually was"
        );
        // From 50 toward 0 over 100 ms: a quarter of the way at t = 75.
        assert_eq!(timeline.tick(ms(75)).frames[0].value, Some(offset(38)), "and move from there");
    }

    #[test]
    fn a_finished_transition_reports_its_target_once_and_stops() {
        let mut timeline = Timeline::new();
        start_linear(&mut timeline, 10, 0);
        let end = timeline.tick(ms(100));
        assert_eq!(
            end.frames,
            vec![Frame {
                node: node(),
                property: AnimatedProperty::Translation,
                // `None`: the transition is over, and the property is the
                // rendered tree's again.
                value: None,
            }]
        );
        assert_eq!(end.finished.len(), 1);
        assert!(!timeline.is_active());
        assert!(timeline.tick(ms(200)).frames.is_empty(), "a finished animation is silent");
    }

    #[test]
    fn an_explicit_animation_returns_to_rest_unless_it_fills_forwards() {
        let mut timeline = Timeline::new();
        let animation = Animation::new(AnimatedProperty::Translation, offset(20), linear(100));
        timeline.start(
            node(),
            &animation,
            AnimationOwner::Transition(node()),
            None,
            Duration::ZERO,
        );
        let end = timeline.tick(ms(100));
        assert_eq!(
            end.frames[0].value,
            Some(offset(0)),
            "translation rests at the origin, which is where the layout puts the node"
        );

        let forwards = animation.fill(Fill::Forwards);
        timeline.start(node(), &forwards, AnimationOwner::Transition(node()), None, Duration::ZERO);
        assert_eq!(timeline.tick(ms(100)).frames[0].value, Some(offset(20)));
    }

    #[test]
    fn repeats_and_autoreversal_run_the_expected_number_of_cycles() {
        let mut timeline = Timeline::new();
        let animation = Animation::new(AnimatedProperty::Translation, offset(100), linear(100))
            .from(offset(0))
            .repeat(Repeat::Times(2))
            .autoreverse(true)
            .fill(Fill::Forwards);
        timeline.start(
            node(),
            &animation,
            AnimationOwner::Transition(node()),
            None,
            Duration::ZERO,
        );

        assert_eq!(timeline.tick(ms(50)).frames[0].value, Some(offset(50)), "out");
        // Halfway through the reversed second cycle the value is 50 again;
        // an unchanged value produces no frame, which is the point — a
        // frame only ever exists because something moved.
        assert!(timeline.tick(ms(150)).frames.is_empty(), "no frame for an unchanged value");
        assert_eq!(
            timeline.current(node(), AnimatedProperty::Translation),
            Some(offset(50)),
            "and back"
        );
        let end = timeline.tick(ms(200));
        assert_eq!(end.finished.len(), 1);
        assert_eq!(
            end.frames[0].value,
            Some(offset(0)),
            "two autoreversed cycles end where they began"
        );
    }

    #[test]
    fn a_forever_animation_keeps_going() {
        let mut timeline = Timeline::new();
        let animation =
            Animation::new(AnimatedProperty::Opacity, AnimatedValue::opacity(1.0), linear(100))
                .from(AnimatedValue::opacity(0.0))
                .repeat(Repeat::Forever);
        timeline.start(
            node(),
            &animation,
            AnimationOwner::Transition(node()),
            None,
            Duration::ZERO,
        );
        for step in 1..20 {
            let output = timeline.tick(ms(step * 25));
            assert!(output.finished.is_empty(), "a forever animation never finishes on its own");
        }
        assert!(timeline.is_active());
    }

    #[test]
    fn cancelling_restores_the_resting_value_and_cancelling_an_owner_stops_all_of_theirs() {
        let mut timeline = Timeline::new();
        let owner = AnimationOwner::Component(crate::identity::ComponentId::ROOT);
        let slide = Animation::new(AnimatedProperty::Translation, offset(50), linear(100));
        let fade =
            Animation::new(AnimatedProperty::Opacity, AnimatedValue::opacity(0.0), linear(100))
                .from(AnimatedValue::opacity(1.0));
        let id = timeline.start(node(), &slide, owner, None, Duration::ZERO);
        timeline.start(node(), &fade, owner, Some(AnimatedValue::opacity(1.0)), Duration::ZERO);
        timeline.tick(ms(50));

        let frame = timeline.cancel(id).expect("the animation was running");
        assert_eq!(frame.value, Some(offset(0)), "translation goes back to the origin");
        assert_eq!(timeline.len(), 1);

        let frames = timeline.cancel_owner(owner);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].value, Some(AnimatedValue::opacity(1.0)), "back to where it started");
        assert!(!timeline.is_active());
    }

    #[test]
    fn forgetting_a_node_leaves_nothing_to_restore() {
        let mut timeline = Timeline::new();
        start_linear(&mut timeline, 10, 0);
        timeline.forget(node());
        assert!(!timeline.is_active());
        assert_eq!(timeline.tick(ms(50)).frames, []);
    }

    #[test]
    fn reduced_motion_completes_a_transition_on_its_first_frame() {
        let mut timeline = Timeline::new();
        timeline.set_motion_preference(MotionPreference::Reduced);
        timeline.transition(
            node(),
            AnimatedProperty::Background,
            AnimatedValue::Color(Color::rgb(0, 0, 0)),
            AnimatedValue::Color(Color::rgb(255, 255, 255)),
            linear(400).delay(ms(200)),
            Duration::ZERO,
        );
        let output = timeline.tick(Duration::ZERO);
        // Finished on its first frame, which — as for any finished
        // transition — hands the color back to the rendered tree, where it
        // already is.
        assert_eq!(output.frames[0].value, None);
        assert_eq!(output.finished.len(), 1, "no delay, no duration, no motion");
    }

    #[test]
    fn reduced_motion_still_runs_an_animation_that_asks_to() {
        let mut timeline = Timeline::new();
        timeline.set_motion_preference(MotionPreference::Reduced);
        let animation = Animation::new(AnimatedProperty::Translation, offset(100), linear(100))
            .from(offset(0))
            .reduced_motion(ReducedMotion::Run);
        timeline.start(
            node(),
            &animation,
            AnimationOwner::Transition(node()),
            None,
            Duration::ZERO,
        );
        assert_eq!(timeline.tick(ms(50)).frames[0].value, Some(offset(50)));
    }

    #[test]
    fn a_spring_transition_settles_without_a_duration() {
        let mut timeline = Timeline::new();
        timeline.transition(
            node(),
            AnimatedProperty::Translation,
            offset(0),
            offset(100),
            Transition::spring(180.0, 20.0, 1.0),
            Duration::ZERO,
        );
        let mut finished = None;
        let mut last_moving = None;
        for step in 1..400 {
            let output = timeline.tick(ms(step * 8));
            if let Some(frame) = output.frames.first() {
                if output.finished.is_empty() {
                    last_moving = frame.value;
                } else {
                    finished = Some(frame.value);
                    break;
                }
            }
        }
        assert_eq!(finished, Some(None), "a finished transition releases the property");
        assert!(
            matches!(last_moving, Some(AnimatedValue::Offset(point)) if point.x > 50),
            "and got most of the way there first: {last_moving:?}"
        );
        assert!(!timeline.is_active());
    }
}
