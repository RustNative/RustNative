//! Game-controller input: a portable state snapshot, and the diffing that
//! turns successive snapshots into discrete events.
//!
//! Every native controller API this framework targets (`XInput`,
//! `GameController.framework`, Android's `InputDevice`, the Web Gamepad
//! API) is *polled* for a full state snapshot rather than delivering
//! per-button events. Turning snapshots into "button A went down" is
//! therefore the same logic everywhere, and lives here once, in
//! [`GamepadPoller`], tested against a fake [`GamepadSource`] instead of a
//! physical controller.

use std::collections::BTreeSet;

use super::Scalar;

/// A controller button, named by position rather than by any one vendor's
/// labels (Xbox "A" / `PlayStation` "Cross" / Nintendo "B" are all
/// [`Self::South`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GamepadButton {
    /// Bottom face button.
    South,
    /// Right face button.
    East,
    /// Left face button.
    West,
    /// Top face button.
    North,
    /// Left shoulder/bumper.
    LeftShoulder,
    /// Right shoulder/bumper.
    RightShoulder,
    /// The "back"/"select"/"view" button.
    Back,
    /// The "start"/"menu" button.
    Start,
    /// Left stick click.
    LeftStick,
    /// Right stick click.
    RightStick,
    /// D-pad up.
    DPadUp,
    /// D-pad down.
    DPadDown,
    /// D-pad left.
    DPadLeft,
    /// D-pad right.
    DPadRight,
}

/// A controller axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GamepadAxis {
    /// Left stick horizontal, `-1.0` (left) to `1.0` (right).
    LeftX,
    /// Left stick vertical, `-1.0` (down) to `1.0` (up).
    LeftY,
    /// Right stick horizontal.
    RightX,
    /// Right stick vertical.
    RightY,
    /// Left trigger, `0.0` (released) to `1.0` (fully pressed).
    LeftTrigger,
    /// Right trigger.
    RightTrigger,
}

impl GamepadAxis {
    const ALL: [Self; 6] = [
        Self::LeftX,
        Self::LeftY,
        Self::RightX,
        Self::RightY,
        Self::LeftTrigger,
        Self::RightTrigger,
    ];

    const fn index(self) -> usize {
        match self {
            Self::LeftX => 0,
            Self::LeftY => 1,
            Self::RightX => 2,
            Self::RightY => 3,
            Self::LeftTrigger => 4,
            Self::RightTrigger => 5,
        }
    }
}

/// One discrete change on one controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamepadInput {
    /// The controller was plugged in / became available.
    Connected,
    /// The controller was unplugged / became unavailable.
    Disconnected,
    /// A button changed state.
    Button {
        /// Which button.
        button: GamepadButton,
        /// Whether it is now held.
        pressed: bool,
    },
    /// An axis moved.
    Axis {
        /// Which axis.
        axis: GamepadAxis,
        /// Its new value (see [`GamepadAxis`] for each axis' range).
        value: Scalar,
    },
}

/// A complete snapshot of one controller.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GamepadState {
    buttons: BTreeSet<GamepadButton>,
    axes: [Scalar; 6],
}

impl GamepadState {
    /// A snapshot with nothing pressed and every axis centered.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `self` with `button` held.
    #[must_use]
    pub fn with_button(mut self, button: GamepadButton) -> Self {
        self.buttons.insert(button);
        self
    }

    /// `self` with `axis` at `value`, clamped into that axis' range.
    #[must_use]
    pub fn with_axis(mut self, axis: GamepadAxis, value: f32) -> Self {
        let clamped = match axis {
            GamepadAxis::LeftTrigger | GamepadAxis::RightTrigger => value.clamp(0.0, 1.0),
            _ => value.clamp(-1.0, 1.0),
        };
        self.axes[axis.index()] = Scalar::new(clamped);
        self
    }

    /// Whether `button` is held.
    #[must_use]
    pub fn is_pressed(&self, button: GamepadButton) -> bool {
        self.buttons.contains(&button)
    }

    /// `axis`' current value.
    #[must_use]
    pub fn axis(&self, axis: GamepadAxis) -> f32 {
        self.axes[axis.index()].get()
    }

    /// The events that turn `previous` into `next`, ignoring axis motion
    /// smaller than `axis_epsilon` (sensor noise on a resting stick would
    /// otherwise produce a continuous event stream).
    ///
    /// `None` means "not connected", so a `None -> Some` transition yields
    /// [`GamepadInput::Connected`] followed by the new state's non-resting
    /// buttons and axes, and `Some -> None` yields
    /// [`GamepadInput::Disconnected`] alone.
    #[must_use]
    pub fn diff(
        previous: Option<&Self>,
        next: Option<&Self>,
        axis_epsilon: f32,
    ) -> Vec<GamepadInput> {
        let resting = Self::new();
        let (previous, next, mut events) = match (previous, next) {
            (None, None) => return Vec::new(),
            (Some(_), None) => return vec![GamepadInput::Disconnected],
            (None, Some(next)) => (&resting, next, vec![GamepadInput::Connected]),
            (Some(previous), Some(next)) => (previous, next, Vec::new()),
        };
        for button in previous.buttons.symmetric_difference(&next.buttons) {
            events
                .push(GamepadInput::Button { button: *button, pressed: next.is_pressed(*button) });
        }
        for axis in GamepadAxis::ALL {
            let (before, after) = (previous.axis(axis), next.axis(axis));
            let settled = after == 0.0 && before != 0.0;
            if (after - before).abs() >= axis_epsilon || settled {
                events.push(GamepadInput::Axis { axis, value: Scalar::new(after) });
            }
        }
        events
    }
}

/// A native controller API, polled for snapshots.
pub trait GamepadSource {
    /// How many controller slots this source exposes (`XInput` has four).
    fn slots(&self) -> u32;
    /// The current state of `slot`, or `None` if nothing is connected.
    fn poll(&mut self, slot: u32) -> Option<GamepadState>;
}

impl<S: GamepadSource + ?Sized> GamepadSource for Box<S> {
    fn slots(&self) -> u32 {
        (**self).slots()
    }
    fn poll(&mut self, slot: u32) -> Option<GamepadState> {
        (**self).poll(slot)
    }
}

/// Polls a [`GamepadSource`] and reports what changed since the last poll.
#[derive(Debug)]
pub struct GamepadPoller<S> {
    source: S,
    last: Vec<Option<GamepadState>>,
    axis_epsilon: f32,
}

impl<S: GamepadSource> GamepadPoller<S> {
    /// The default axis-noise threshold: 1% of full deflection.
    pub const DEFAULT_AXIS_EPSILON: f32 = 0.01;

    /// A poller over `source` with the default axis threshold.
    pub fn new(source: S) -> Self {
        Self { source, last: Vec::new(), axis_epsilon: Self::DEFAULT_AXIS_EPSILON }
    }

    /// Polls every slot once, returning `(slot, change)` pairs in slot
    /// order.
    pub fn poll(&mut self) -> Vec<(u32, GamepadInput)> {
        let slots = self.source.slots();
        let slot_count = usize::try_from(slots).unwrap_or(usize::MAX);
        self.last.resize(slot_count, None);
        let mut changes = Vec::new();
        for (slot, last) in (0..slots).zip(self.last.iter_mut()) {
            let next = self.source.poll(slot);
            for input in GamepadState::diff(last.as_ref(), next.as_ref(), self.axis_epsilon) {
                changes.push((slot, input));
            }
            *last = next;
        }
        changes
    }

    /// Whether any slot was connected at the last poll.
    #[must_use]
    pub fn any_connected(&self) -> bool {
        self.last.iter().any(Option::is_some)
    }

    /// The underlying source.
    pub fn source_mut(&mut self) -> &mut S {
        &mut self.source
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        slots: Vec<Option<GamepadState>>,
    }

    impl GamepadSource for Fake {
        fn slots(&self) -> u32 {
            u32::try_from(self.slots.len()).unwrap()
        }
        fn poll(&mut self, slot: u32) -> Option<GamepadState> {
            self.slots[slot as usize].clone()
        }
    }

    #[test]
    fn connecting_reports_connected_then_non_resting_input() {
        let state = GamepadState::new()
            .with_button(GamepadButton::South)
            .with_axis(GamepadAxis::LeftX, 0.5);
        let events = GamepadState::diff(None, Some(&state), 0.01);
        assert_eq!(
            events,
            vec![
                GamepadInput::Connected,
                GamepadInput::Button { button: GamepadButton::South, pressed: true },
                GamepadInput::Axis { axis: GamepadAxis::LeftX, value: Scalar::new(0.5) },
            ]
        );
        assert_eq!(GamepadState::diff(Some(&state), None, 0.01), vec![GamepadInput::Disconnected]);
    }

    #[test]
    fn axis_noise_below_epsilon_is_ignored_but_returning_to_rest_is_not() {
        let a = GamepadState::new().with_axis(GamepadAxis::RightY, 0.004);
        let b = GamepadState::new().with_axis(GamepadAxis::RightY, 0.009);
        assert!(GamepadState::diff(Some(&a), Some(&b), 0.01).is_empty());
        let rest = GamepadState::new();
        assert_eq!(
            GamepadState::diff(Some(&b), Some(&rest), 0.01),
            vec![GamepadInput::Axis { axis: GamepadAxis::RightY, value: Scalar::ZERO }],
            "a stick settling back to exactly zero must be reported, however small the move"
        );
    }

    #[test]
    fn axes_are_clamped_to_their_documented_ranges() {
        let state = GamepadState::new()
            .with_axis(GamepadAxis::LeftTrigger, -3.0)
            .with_axis(GamepadAxis::LeftX, 7.0);
        assert!((state.axis(GamepadAxis::LeftTrigger) - 0.0).abs() < f32::EPSILON);
        assert!((state.axis(GamepadAxis::LeftX) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_poller_reports_each_change_once_per_slot() {
        let mut poller = GamepadPoller::new(Fake { slots: vec![None, None] });
        assert!(poller.poll().is_empty());
        assert!(!poller.any_connected());

        poller.source_mut().slots[1] = Some(GamepadState::new().with_button(GamepadButton::Start));
        assert_eq!(
            poller.poll(),
            vec![
                (1, GamepadInput::Connected),
                (1, GamepadInput::Button { button: GamepadButton::Start, pressed: true }),
            ]
        );
        assert!(poller.poll().is_empty(), "an unchanged state produces nothing");

        poller.source_mut().slots[1] = Some(GamepadState::new());
        assert_eq!(
            poller.poll(),
            vec![(1, GamepadInput::Button { button: GamepadButton::Start, pressed: false })]
        );
        poller.source_mut().slots[1] = None;
        assert_eq!(poller.poll(), vec![(1, GamepadInput::Disconnected)]);
    }
}
