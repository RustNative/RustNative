//! Portable advanced-input payloads: pointers (mouse, touch, pen), wheels,
//! gestures, IME composition, clipboard actions, drag-and-drop, and
//! gamepads.
//!
//! # Semantic in the core, native in the backend
//!
//! Everything in this module is platform-free data plus the two pieces of
//! input *logic* that do not depend on a platform at all and so should
//! exist exactly once: [`GestureRecognizer`], which turns a portable pointer
//! stream into taps, long presses, pans, and pinches, and
//! [`GamepadPoller`], which turns successive controller snapshots into
//! discrete button/axis changes. A backend's job is only to produce
//! [`PointerEvent`]s and [`GamepadState`]s from its native APIs; it never
//! re-implements recognition.
//!
//! # Opt-in delivery
//!
//! A pointer device can produce hundreds of events a second. Delivering
//! each one to every component would turn "the mouse moved" into a
//! rerender storm, which `PLAN.md` §2.9 rules out. A node therefore
//! declares what it wants through [`InputInterest`]
//! (`Node::with_input`), and a backend delivers pointer, wheel, gesture,
//! drop, and gamepad events only to nodes that asked — walking up from the
//! node under the pointer to the nearest interested ancestor. Ordinary
//! clicks, focus, keys, and text keep flowing exactly as before.

mod arbitration;
mod cursor;
mod drag;
mod gamepad;
mod gesture;
mod ime;
mod pointer;

pub use arbitration::{GestureConflict, GesturePolicy, Winner, arbitrate};
pub use cursor::Cursor;
pub use drag::{DragData, DropEffect};
pub use gamepad::{
    GamepadAxis, GamepadButton, GamepadInput, GamepadPoller, GamepadSource, GamepadState,
};
pub use gesture::{Gesture, GestureConfig, GesturePhase, GestureRecognizer, PointerPhase};
pub use ime::{ClipboardAction, Composition};
pub use pointer::{PointerButton, PointerButtons, PointerEvent, PointerKind, WheelDelta};

pub use rustnative_types::Scalar;

/// Which advanced-input streams a node wants delivered to it.
///
/// Every flag defaults to off; see the module documentation for why
/// delivery is opt-in. Keys, text, focus, and clicks are not gated by this
/// type — they were always delivered and still are.
///
/// # Example
///
/// ```
/// use rustnative_core::{InputInterest, Node};
///
/// let canvas = Node::column("canvas", []).with_input(InputInterest::new().pointer().wheel());
/// assert!(canvas.input().wants_pointer());
/// assert!(!canvas.input().wants_drop());
///
/// // The same node in markup:
/// let markup = rustnative_core::rsx! { <Column key="canvas" input={InputInterest::new().pointer().wheel()}></Column> };
/// assert_eq!(markup, canvas);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[allow(clippy::struct_excessive_bools, reason = "independent opt-in flags, not a state machine")]
pub struct InputInterest {
    pointer: bool,
    wheel: bool,
    gestures: bool,
    drop_target: bool,
    gamepad: bool,
    policy: GesturePolicy,
}

impl InputInterest {
    /// No advanced input.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pointer: false,
            wheel: false,
            gestures: false,
            drop_target: false,
            gamepad: false,
            policy: GesturePolicy::Exclusive,
        }
    }

    /// Pointer down/move/up/cancel and enter/leave.
    #[must_use]
    pub const fn pointer(mut self) -> Self {
        self.pointer = true;
        self
    }

    /// Wheel and trackpad scroll deltas.
    #[must_use]
    pub const fn wheel(mut self) -> Self {
        self.wheel = true;
        self
    }

    /// Recognized gestures (tap, long press, pan, pinch).
    #[must_use]
    pub const fn gestures(mut self) -> Self {
        self.gestures = true;
        self
    }

    /// Drag-and-drop: this node accepts drops.
    #[must_use]
    pub const fn drop_target(mut self) -> Self {
        self.drop_target = true;
        self
    }

    /// Game controller input.
    #[must_use]
    pub const fn gamepad(mut self) -> Self {
        self.gamepad = true;
        self
    }

    /// Sets how this node's gestures relate to the host's own handling of
    /// the same input (see [`GesturePolicy`] and [`arbitrate`]).
    #[must_use]
    pub const fn gesture_policy(mut self, policy: GesturePolicy) -> Self {
        self.policy = policy;
        self
    }

    /// This node's gesture policy.
    #[must_use]
    pub const fn policy(self) -> GesturePolicy {
        self.policy
    }

    /// Whether pointer events are wanted.
    #[must_use]
    pub const fn wants_pointer(self) -> bool {
        self.pointer
    }

    /// Whether wheel events are wanted.
    #[must_use]
    pub const fn wants_wheel(self) -> bool {
        self.wheel
    }

    /// Whether gesture events are wanted.
    #[must_use]
    pub const fn wants_gestures(self) -> bool {
        self.gestures
    }

    /// Whether this node accepts drops.
    #[must_use]
    pub const fn wants_drop(self) -> bool {
        self.drop_target
    }

    /// Whether gamepad events are wanted.
    #[must_use]
    pub const fn wants_gamepad(self) -> bool {
        self.gamepad
    }

    /// Whether any advanced input at all is wanted.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        !(self.pointer || self.wheel || self.gestures || self.drop_target || self.gamepad)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_equality_is_total_and_normalizes_the_awkward_values() {
        assert_eq!(Scalar::new(f32::NAN), Scalar::ZERO);
        assert_eq!(Scalar::new(-0.0), Scalar::ZERO);
        assert_eq!(Scalar::new(0.5), Scalar::new(0.5));
        assert_ne!(Scalar::new(0.5), Scalar::new(0.25));
    }

    #[test]
    fn input_interest_defaults_to_nothing_and_builds_up() {
        assert!(InputInterest::new().is_empty());
        let interest = InputInterest::new().pointer().drop_target();
        assert!(interest.wants_pointer() && interest.wants_drop());
        assert!(!interest.wants_wheel() && !interest.wants_gestures() && !interest.wants_gamepad());
        assert!(!interest.is_empty());
    }
}
