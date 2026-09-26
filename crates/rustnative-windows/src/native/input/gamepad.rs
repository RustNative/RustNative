//! Game controllers through `XInput`, polled on a window timer.
//!
//! `XInput` has no event API: a controller's state can only be sampled. The
//! diffing that turns samples into events is portable and lives in
//! `rustnative_core::GamepadPoller`; this module supplies the samples
//! ([`XInputSource`]) and decides *when* to take them:
//!
//! - never, unless some node in this window declared gamepad interest (so an
//!   application that does not use controllers never wakes for them);
//! - every 16 ms while a controller is connected;
//! - once a second while none is, because Microsoft documents that
//!   querying an empty slot is comparatively expensive and should not be
//!   done every frame.
//!
//! Events are delivered only while the window is its thread's active
//! window: a background window reacting to the controller the person is
//! using for something else would be a bug, not a feature.

use rustnative_core::{
    Event, GamepadAxis, GamepadButton, GamepadPoller, GamepadSource, GamepadState,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
use windows_sys::Win32::UI::Input::XboxController::{
    XINPUT_GAMEPAD, XINPUT_GAMEPAD_A, XINPUT_GAMEPAD_B, XINPUT_GAMEPAD_BACK,
    XINPUT_GAMEPAD_DPAD_DOWN, XINPUT_GAMEPAD_DPAD_LEFT, XINPUT_GAMEPAD_DPAD_RIGHT,
    XINPUT_GAMEPAD_DPAD_UP, XINPUT_GAMEPAD_LEFT_SHOULDER, XINPUT_GAMEPAD_LEFT_THUMB,
    XINPUT_GAMEPAD_RIGHT_SHOULDER, XINPUT_GAMEPAD_RIGHT_THUMB, XINPUT_GAMEPAD_START,
    XINPUT_GAMEPAD_X, XINPUT_GAMEPAD_Y, XINPUT_STATE, XInputGetState,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

use super::super::runtime::Runtime;
use super::super::win32::{best_effort, ignored_by_contract};

/// `SetTimer` id for gamepad polling on a top-level window.
pub(crate) const GAMEPAD_TIMER_ID: usize = 0x4652_0002;
const CONNECTED_INTERVAL_MS: u32 = 16;
const IDLE_INTERVAL_MS: u32 = 1_000;

/// `XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE` etc. from `XInput.h`.
const LEFT_THUMB_DEADZONE: f32 = 7849.0;
const RIGHT_THUMB_DEADZONE: f32 = 8689.0;
const TRIGGER_THRESHOLD: f32 = 30.0;

/// The system's `XInput` controllers (four slots).
#[derive(Debug, Default)]
pub(crate) struct XInputSource;

impl GamepadSource for XInputSource {
    fn slots(&self) -> u32 {
        4
    }

    fn poll(&mut self, slot: u32) -> Option<GamepadState> {
        let mut state = XINPUT_STATE::default();
        // SAFETY: `state` is a valid, exclusively borrowed `XINPUT_STATE`;
        // `slot` is within `XInput`'s four user indices.
        let result = unsafe { XInputGetState(slot, &raw mut state) };
        // `ERROR_SUCCESS`; anything else (typically
        // `ERROR_DEVICE_NOT_CONNECTED`) means no controller in this slot.
        (result == 0).then(|| convert(&state.Gamepad))
    }
}

/// Maps a stick axis reading to `-1.0..=1.0`, zeroing the dead zone and
/// rescaling the rest so the output still reaches the full range just past
/// it (a plain cut-off would jump from 0 straight to ~0.24).
fn stick(value: i16, dead_zone: f32) -> f32 {
    let value = f32::from(value);
    let magnitude = value.abs();
    if magnitude <= dead_zone {
        return 0.0;
    }
    let scaled = ((magnitude - dead_zone) / (32767.0 - dead_zone)).min(1.0);
    scaled.copysign(value)
}

fn trigger(value: u8) -> f32 {
    let value = f32::from(value);
    if value <= TRIGGER_THRESHOLD {
        0.0
    } else {
        ((value - TRIGGER_THRESHOLD) / (255.0 - TRIGGER_THRESHOLD)).min(1.0)
    }
}

/// Converts one `XInput` sample into the portable snapshot.
pub(crate) fn convert(pad: &XINPUT_GAMEPAD) -> GamepadState {
    let mut state = GamepadState::new();
    for (mask, button) in [
        (XINPUT_GAMEPAD_A, GamepadButton::South),
        (XINPUT_GAMEPAD_B, GamepadButton::East),
        (XINPUT_GAMEPAD_X, GamepadButton::West),
        (XINPUT_GAMEPAD_Y, GamepadButton::North),
        (XINPUT_GAMEPAD_LEFT_SHOULDER, GamepadButton::LeftShoulder),
        (XINPUT_GAMEPAD_RIGHT_SHOULDER, GamepadButton::RightShoulder),
        (XINPUT_GAMEPAD_BACK, GamepadButton::Back),
        (XINPUT_GAMEPAD_START, GamepadButton::Start),
        (XINPUT_GAMEPAD_LEFT_THUMB, GamepadButton::LeftStick),
        (XINPUT_GAMEPAD_RIGHT_THUMB, GamepadButton::RightStick),
        (XINPUT_GAMEPAD_DPAD_UP, GamepadButton::DPadUp),
        (XINPUT_GAMEPAD_DPAD_DOWN, GamepadButton::DPadDown),
        (XINPUT_GAMEPAD_DPAD_LEFT, GamepadButton::DPadLeft),
        (XINPUT_GAMEPAD_DPAD_RIGHT, GamepadButton::DPadRight),
    ] {
        if pad.wButtons & mask != 0 {
            state = state.with_button(button);
        }
    }
    state
        .with_axis(GamepadAxis::LeftX, stick(pad.sThumbLX, LEFT_THUMB_DEADZONE))
        .with_axis(GamepadAxis::LeftY, stick(pad.sThumbLY, LEFT_THUMB_DEADZONE))
        .with_axis(GamepadAxis::RightX, stick(pad.sThumbRX, RIGHT_THUMB_DEADZONE))
        .with_axis(GamepadAxis::RightY, stick(pad.sThumbRY, RIGHT_THUMB_DEADZONE))
        .with_axis(GamepadAxis::LeftTrigger, trigger(pad.bLeftTrigger))
        .with_axis(GamepadAxis::RightTrigger, trigger(pad.bRightTrigger))
}

/// One window's controller polling.
pub(crate) struct GamepadInputState {
    poller: GamepadPoller<Box<dyn GamepadSource>>,
    interval: Option<u32>,
}

impl std::fmt::Debug for GamepadInputState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GamepadInputState")
            .field("interval", &self.interval)
            .finish_non_exhaustive()
    }
}

impl Default for GamepadInputState {
    fn default() -> Self {
        Self::with_source(Box::new(XInputSource))
    }
}

impl GamepadInputState {
    /// Polls `source` instead of `XInput` — how the native tests stand in
    /// for a physical controller.
    pub(crate) fn with_source(source: Box<dyn GamepadSource>) -> Self {
        Self { poller: GamepadPoller::new(source), interval: None }
    }

    /// The current polling interval, or `None` if not polling.
    #[cfg(test)]
    pub(crate) fn interval(&self) -> Option<u32> {
        self.interval
    }
}

fn set_interval(runtime: &mut Runtime, next: Option<u32>) {
    if runtime.input.gamepad.interval == next {
        return;
    }
    runtime.input.gamepad.interval = next;
    match next {
        Some(interval) => {
            // SAFETY: `runtime.window` is this runtime's live top-level
            // HWND; re-setting an existing timer id replaces its interval.
            let armed = unsafe { SetTimer(runtime.window, GAMEPAD_TIMER_ID, interval, None) } != 0;
            best_effort(armed, "SetTimer(gamepad)", "controller input is not polled");
        }
        None => {
            // SAFETY: as above.
            ignored_by_contract(unsafe { KillTimer(runtime.window, GAMEPAD_TIMER_ID) });
        }
    }
}

fn first_interested(runtime: &Runtime) -> Option<rustnative_core::NodeId> {
    runtime
        .renderer
        .snapshot
        .ordered_nodes()
        .into_iter()
        .find(|node| node.input.wants_gamepad())
        .map(|node| node.id)
}

/// Starts or stops polling to match whether this window currently has a
/// gamepad-interested node. Called after every render.
pub(crate) fn sync_timer(runtime: &mut Runtime) {
    let wanted = first_interested(runtime).is_some();
    let next = match (wanted, runtime.input.gamepad.interval) {
        (false, _) => None,
        (true, Some(current)) => Some(current),
        (true, None) => Some(CONNECTED_INTERVAL_MS),
    };
    set_interval(runtime, next);
}

/// `WM_TIMER` for [`GAMEPAD_TIMER_ID`].
pub(crate) fn poll(runtime: &mut Runtime) {
    let changes = runtime.input.gamepad.poller.poll();
    let interval = if runtime.input.gamepad.poller.any_connected() {
        CONNECTED_INTERVAL_MS
    } else {
        IDLE_INTERVAL_MS
    };
    set_interval(runtime, Some(interval));
    // SAFETY: `GetActiveWindow` takes no arguments.
    if unsafe { GetActiveWindow() } != runtime.window {
        return;
    }
    let Some(target) = first_interested(runtime) else {
        return;
    };
    for (gamepad, input) in changes {
        if !runtime.dispatch_or_quit(Event::Gamepad { target, gamepad, input }) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stick_readings_inside_the_dead_zone_are_zero_and_the_rest_reaches_full_range() {
        assert!(stick(7000, LEFT_THUMB_DEADZONE).abs() < f32::EPSILON);
        assert!(stick(-7000, LEFT_THUMB_DEADZONE).abs() < f32::EPSILON);
        assert!((stick(i16::MAX, LEFT_THUMB_DEADZONE) - 1.0).abs() < 1e-6);
        assert!((stick(i16::MIN, LEFT_THUMB_DEADZONE) + 1.0).abs() < 1e-6, "clamped at -1");
        let just_past = stick(8000, LEFT_THUMB_DEADZONE);
        assert!(just_past > 0.0 && just_past < 0.01, "no jump at the dead-zone edge");
    }

    #[test]
    fn a_raw_sample_converts_buttons_and_axes() {
        let pad = XINPUT_GAMEPAD {
            wButtons: XINPUT_GAMEPAD_A | XINPUT_GAMEPAD_DPAD_LEFT,
            bLeftTrigger: 255,
            bRightTrigger: 10,
            sThumbLX: i16::MAX,
            sThumbLY: 0,
            sThumbRX: 0,
            sThumbRY: i16::MIN,
        };
        let state = convert(&pad);
        assert!(state.is_pressed(GamepadButton::South));
        assert!(state.is_pressed(GamepadButton::DPadLeft));
        assert!(!state.is_pressed(GamepadButton::East));
        assert!((state.axis(GamepadAxis::LeftTrigger) - 1.0).abs() < 1e-6);
        assert!(state.axis(GamepadAxis::RightTrigger).abs() < f32::EPSILON, "below threshold");
        assert!((state.axis(GamepadAxis::LeftX) - 1.0).abs() < 1e-6);
        assert!((state.axis(GamepadAxis::RightY) + 1.0).abs() < 1e-6);
    }
}
