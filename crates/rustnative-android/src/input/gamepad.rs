//! Game controllers: Android delivers a controller's buttons as `KeyEvent`s
//! and its sticks, triggers, and hat as `MotionEvent` axes
//! (`SOURCE_GAMEPAD`, `SOURCE_JOYSTICK`). Each controller's state is kept
//! here and diffed by the portable `GamepadPoller` into `Event::Gamepad`,
//! delivered to the nodes that declared gamepad interest.
//!
//! | Portable | Android |
//! |---|---|
//! | `South`, `East`, `West`, `North` | `KEYCODE_BUTTON_A`, `_B`, `_X`, `_Y` |
//! | shoulders, sticks, back, start | `KEYCODE_BUTTON_L1`, `_R1`, `_THUMBL`, `_THUMBR`, `_SELECT`, `_START` |
//! | d-pad | `KEYCODE_DPAD_*` from a gamepad, or the hat axes |
//! | `LeftX`/`LeftY`, `RightX`/`RightY` | `AXIS_X`/`AXIS_Y`, `AXIS_Z`/`AXIS_RZ` |
//! | triggers | `AXIS_LTRIGGER`/`AXIS_RTRIGGER` (or `AXIS_BRAKE`/`AXIS_GAS`) |
//!
//! A controller is a slot (0–3) for as long as it stays connected.

use std::collections::HashMap;

use rustnative_core::{
    GamepadAxis, GamepadButton, GamepadInput, GamepadPoller, GamepadSource, GamepadState,
};

/// The slots a controller can occupy.
const SLOTS: u32 = 4;

/// What each connected controller reports.
#[derive(Debug, Default)]
pub(crate) struct Pads {
    states: HashMap<u32, GamepadState>,
    devices: HashMap<i32, u32>,
}

impl GamepadSource for Pads {
    fn slots(&self) -> u32 {
        SLOTS
    }
    fn poll(&mut self, slot: u32) -> Option<GamepadState> {
        self.states.get(&slot).cloned()
    }
}

/// One window's controllers.
pub(crate) struct Gamepads {
    poller: GamepadPoller<Pads>,
}

impl Default for Gamepads {
    fn default() -> Self {
        Self { poller: GamepadPoller::new(Pads::default()) }
    }
}

impl std::fmt::Debug for Gamepads {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gamepads").finish_non_exhaustive()
    }
}

/// The portable button for a gamepad key code.
pub(crate) const fn button(key_code: i32) -> Option<GamepadButton> {
    Some(match key_code {
        96 => GamepadButton::South,  // BUTTON_A
        97 => GamepadButton::East,   // BUTTON_B
        99 => GamepadButton::West,   // BUTTON_X
        100 => GamepadButton::North, // BUTTON_Y
        102 => GamepadButton::LeftShoulder,
        103 => GamepadButton::RightShoulder,
        106 => GamepadButton::LeftStick,
        107 => GamepadButton::RightStick,
        109 => GamepadButton::Back, // BUTTON_SELECT
        108 => GamepadButton::Start,
        19 => GamepadButton::DPadUp,
        20 => GamepadButton::DPadDown,
        21 => GamepadButton::DPadLeft,
        22 => GamepadButton::DPadRight,
        _ => return None,
    })
}

impl Gamepads {
    fn slot(&mut self, device: i32) -> u32 {
        let pads = self.poller.source_mut();
        if let Some(slot) = pads.devices.get(&device) {
            return *slot;
        }
        let slot = (0..SLOTS)
            .find(|slot| !pads.devices.values().any(|taken| taken == slot))
            .unwrap_or(SLOTS - 1);
        pads.devices.insert(device, slot);
        pads.states.insert(slot, GamepadState::new());
        slot
    }

    /// A controller's button changed; returns what that produced.
    pub(crate) fn key(
        &mut self,
        device: i32,
        key_code: i32,
        pressed: bool,
    ) -> Vec<(u32, GamepadInput)> {
        let Some(button) = button(key_code) else { return Vec::new() };
        let slot = self.slot(device);
        let pads = self.poller.source_mut();
        let state = pads.states.remove(&slot).unwrap_or_default();
        pads.states.insert(slot, with_button(state, button, pressed));
        self.poller.poll()
    }

    /// A controller's axes moved (`values`: x, y, z, rz, left trigger, right
    /// trigger, hat x, hat y); returns what that produced.
    pub(crate) fn axes(&mut self, device: i32, values: &[f32]) -> Vec<(u32, GamepadInput)> {
        let slot = self.slot(device);
        let value = |index: usize| values.get(index).copied().unwrap_or(0.0);
        let pads = self.poller.source_mut();
        let mut state = pads.states.remove(&slot).unwrap_or_default();
        for (axis, index) in [
            (GamepadAxis::LeftX, 0),
            (GamepadAxis::LeftY, 1),
            (GamepadAxis::RightX, 2),
            (GamepadAxis::RightY, 3),
            (GamepadAxis::LeftTrigger, 4),
            (GamepadAxis::RightTrigger, 5),
        ] {
            state = state.with_axis(axis, value(index));
        }
        let (hat_x, hat_y) = (value(6), value(7));
        state = with_button(state, GamepadButton::DPadLeft, hat_x < -0.5);
        state = with_button(state, GamepadButton::DPadRight, hat_x > 0.5);
        state = with_button(state, GamepadButton::DPadUp, hat_y < -0.5);
        state = with_button(state, GamepadButton::DPadDown, hat_y > 0.5);
        pads.states.insert(slot, state);
        self.poller.poll()
    }
}

/// `state` with `button` pressed or released.
fn with_button(state: GamepadState, button: GamepadButton, pressed: bool) -> GamepadState {
    if state.is_pressed(button) == pressed {
        return state;
    }
    // `GamepadState` adds buttons; a release is a state rebuilt without it.
    let mut next = GamepadState::new();
    for other in [
        GamepadButton::South,
        GamepadButton::East,
        GamepadButton::West,
        GamepadButton::North,
        GamepadButton::LeftShoulder,
        GamepadButton::RightShoulder,
        GamepadButton::Back,
        GamepadButton::Start,
        GamepadButton::LeftStick,
        GamepadButton::RightStick,
        GamepadButton::DPadUp,
        GamepadButton::DPadDown,
        GamepadButton::DPadLeft,
        GamepadButton::DPadRight,
    ] {
        if other == button {
            if pressed {
                next = next.with_button(other);
            }
        } else if state.is_pressed(other) {
            next = next.with_button(other);
        }
    }
    for axis in [
        GamepadAxis::LeftX,
        GamepadAxis::LeftY,
        GamepadAxis::RightX,
        GamepadAxis::RightY,
        GamepadAxis::LeftTrigger,
        GamepadAxis::RightTrigger,
    ] {
        next = next.with_axis(axis, state.axis(axis));
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_and_axes_become_portable_input() {
        let mut pads = Gamepads::default();
        let pressed = pads.key(7, 96, true);
        assert!(
            pressed.iter().any(|(slot, input)| *slot == 0
                && matches!(
                    input,
                    GamepadInput::Button { button: GamepadButton::South, pressed: true }
                )),
            "{pressed:?}"
        );
        let released = pads.key(7, 96, false);
        assert!(
            released.iter().any(|(_, input)| matches!(
                input,
                GamepadInput::Button { button: GamepadButton::South, pressed: false }
            )),
            "{released:?}"
        );
        let moved = pads.axes(7, &[0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        assert!(
            moved.iter().any(|(_, input)| matches!(
                input,
                GamepadInput::Axis { axis: GamepadAxis::LeftX, .. }
            )),
            "{moved:?}"
        );
        assert!(
            moved.iter().any(|(_, input)| matches!(
                input,
                GamepadInput::Button { button: GamepadButton::DPadDown, pressed: true }
            )),
            "the hat is the d-pad: {moved:?}"
        );
        // A second controller takes the next slot.
        assert!(pads.key(9, 97, true).iter().all(|(slot, _)| *slot == 1));
        assert_eq!(button(50), None);
    }
}
