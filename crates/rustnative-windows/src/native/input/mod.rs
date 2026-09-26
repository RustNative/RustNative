//! Native input: keyboard translation and focus (Milestone 11), and the
//! advanced input system (Milestone 25) — pointers, capture, wheels,
//! gestures, IME composition, clipboard events, drag-and-drop, and game
//! controllers.
//!
//! Each concern is its own module and owns its own slice of per-window
//! state, gathered in [`InputState`] on the window's `Runtime`. None of them
//! knows about rendering beyond reading the realized tree (to find which
//! node is under a native window and what input it declared), and none of
//! them mutates component state: everything reaches a component as an
//! `Event` through `Runtime::dispatch`.

pub(crate) mod clipboard;
pub(crate) mod drop_target;
mod focus;
pub(crate) mod gamepad;
pub(crate) mod ime;
mod keys;
pub(crate) mod pointer;

pub(crate) use focus::{focus_next, focused_node, set_hovered, set_pressed, sync_focus};
pub(crate) use keys::{key_code, modifiers};

use rustnative_core::InputRequest;

use super::runtime::Runtime;

/// One window's advanced-input bookkeeping.
#[derive(Debug, Default)]
pub(crate) struct InputState {
    pub(crate) pointer: pointer::PointerState,
    pub(crate) ime: ime::ImeState,
    pub(crate) drag: drop_target::DragState,
    pub(crate) gamepad: gamepad::GamepadInputState,
}

/// Applies the input requests components made during the dispatch that
/// just finished.
pub(crate) fn apply_requests(runtime: &mut Runtime, requests: Vec<InputRequest>) {
    for request in requests {
        match request {
            InputRequest::SetDropEffect(effect) => runtime.input.drag.effect = effect,
            other => pointer::apply_request(runtime, other),
        }
    }
}

/// Reconciles input state with a freshly rendered tree: forgets nodes that
/// no longer exist and starts or stops controller polling.
pub(crate) fn after_render(runtime: &mut Runtime) {
    pointer::prune(runtime);
    gamepad::sync_timer(runtime);
}
