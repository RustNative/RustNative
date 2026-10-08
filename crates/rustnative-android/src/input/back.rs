//! System back and predictive back on the navigation model (`PLAN.md`
//! Milestones 30 and 35): the back command
//! (`rustnative_core::command::standard::BACK`).
//!
//! The activity claims the system's back gesture exactly while a component
//! declares `BACK` enabled (`Application::handles_back`) — through
//! `OnBackInvokedDispatcher` on API 33+, with `OnBackAnimationCallback`'s
//! progress on 34+, and `onBackPressed` before 33. With nothing to go back
//! to, the claim is withdrawn and the system's own back (leaving the
//! application, with its own predictive animation) happens instead.

use rustnative_core::WindowId;
use rustnative_core::command::standard::BACK;

use crate::Error;
use crate::jni_host::{Arg, call};
use crate::protocol;
use crate::registry::WindowRegistry;

/// Claims or releases the system back gesture to match whether the window
/// handles back now.
pub(crate) fn sync_back(registry: &mut WindowRegistry, window: WindowId) -> Result<(), Error> {
    let focused = registry.windows.get(&window).and_then(|runtime| runtime.input.focused);
    let handles =
        registry.with_application(|application| application.handles_back(window, focused));
    let Some(runtime) = registry.windows.get_mut(&window) else { return Ok(()) };
    if runtime.back_claimed == handles {
        return Ok(());
    }
    if let Some(activity) = &runtime.activity {
        call(activity, "setBackHandled", "(Z)V", &[Arg::Bool(handles)])?;
        runtime.back_claimed = handles;
    }
    Ok(())
}

/// A phase of the system's back gesture.
pub(crate) fn back(
    registry: &mut WindowRegistry,
    window: WindowId,
    phase: i32,
    progress: f32,
    edge: i32,
) -> Result<(), Error> {
    let focused = registry.windows.get(&window).and_then(|runtime| runtime.input.focused);
    let changed = if phase == protocol::BACK_INVOKED {
        registry.with_application(|application| application.invoke_command(window, BACK, focused))
    } else {
        let Some(phase) = super::phases::portable_phase(phase, progress, edge) else {
            return Ok(());
        };
        registry.with_application(|application| application.back_progress(window, phase, focused))
    };
    if changed {
        registry.render(window)?;
    }
    registry.after_change(window)
}
