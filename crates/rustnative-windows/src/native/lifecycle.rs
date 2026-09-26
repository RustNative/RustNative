//! The application's life as Windows reports it, and the persisted state
//! that has to be on disk before each step of it.
//!
//! | Windows says | The application hears | State |
//! |---|---|---|
//! | `WM_QUERYENDSESSION` (sign-out, shutdown) | `Lifecycle::Terminating` | flushed first |
//! | `WM_POWERBROADCAST` / `PBT_APMSUSPEND` | `Lifecycle::Suspending` | flushed first |
//! | `WM_POWERBROADCAST` / `PBT_APMRESUMEAUTOMATIC` | `Lifecycle::Resuming` | — |
//! | the low-memory resource notification (`native::memory_watch`) | `Lifecycle::LowMemory` | flushed first |
//! | the last window closed (the loop ends) | `Lifecycle::Terminating` | flushed first |
//! | a burst of writes went quiet for [`IDLE_FLUSH`] | nothing | flushed |
//!
//! The idle flush is what bounds how much a crash can lose: a component
//! that saved something a second and a half ago has it on disk, without
//! every keystroke costing a disk write.
//!
//! The primary window's placement (position, size, maximized) is saved in
//! the same store when it closes and restored when it next opens — on the
//! monitor it was on, if that monitor is still there.

use rustnative_core::{Lifecycle, StateStore, WindowId};
use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromRect};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowPlacement, KillTimer, PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND, SW_SHOWMAXIMIZED,
    SW_SHOWNORMAL, SetTimer, SetWindowPlacement, WINDOWPLACEMENT,
};

use super::runtime::Runtime;
use super::win32::{best_effort, ignored_by_contract};

/// The timer that flushes persisted state once writes stop.
pub(crate) const FLUSH_TIMER_ID: usize = 0x4652_0003;

/// How long writes must stop before they are flushed.
const IDLE_FLUSH: u32 = 1500;

/// The key the primary window's placement is saved under.
const PLACEMENT_KEY: &str = "rust-native/window-placement/primary";

/// Arms (or re-arms) the idle flush if anything is waiting to be written.
///
/// Called after every dispatch and task pump; re-arming on each one is what
/// makes it a debounce — the flush happens once things go quiet.
pub(crate) fn after_dispatch(runtime: &Runtime) {
    if !runtime.with_application(|application| application.has_unsaved_state()) {
        return;
    }
    // SAFETY: `runtime.window` is live; a null callback posts `WM_TIMER`.
    let armed = unsafe { SetTimer(runtime.window, FLUSH_TIMER_ID, IDLE_FLUSH, None) } != 0;
    best_effort(armed, "SetTimer(state flush)", "state is flushed at the next lifecycle point");
}

/// The idle flush timer fired.
pub(crate) fn idle_flush(runtime: &Runtime) {
    // SAFETY: `runtime.window` is live; the timer is this module's.
    ignored_by_contract(unsafe { KillTimer(runtime.window, FLUSH_TIMER_ID) });
    let flushed = runtime.with_application(rustnative_core::Application::flush_state);
    best_effort(flushed.is_ok(), "flush_state(idle)", "the writes stay buffered and are retried");
}

/// Reports a lifecycle step to the application, which flushes first.
pub(crate) fn notify(runtime: &mut Runtime, lifecycle: Lifecycle) {
    let flushed = runtime.with_application(rustnative_core::Application::flush_state);
    best_effort(flushed.is_ok(), "flush_state(lifecycle)", "the writes stay buffered");
    // The flush is done above rather than by `Application::lifecycle` so the
    // event goes through `Runtime::dispatch`, which re-renders and applies
    // what the component did in answer.
    runtime.dispatch_or_quit(rustnative_core::Event::Lifecycle(lifecycle));
}

/// `WM_POWERBROADCAST`: the machine is going to sleep or waking up.
pub(crate) fn power_broadcast(runtime: &mut Runtime, event: u32) {
    match event {
        PBT_APMSUSPEND => notify(runtime, Lifecycle::Suspending),
        PBT_APMRESUMEAUTOMATIC => notify(runtime, Lifecycle::Resuming),
        _ => {}
    }
}

/// The primary window's placement, as saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Placement {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    maximized: bool,
}

fn store(runtime: &Runtime) -> Option<std::sync::Arc<dyn StateStore>> {
    runtime.with_application(|application| application.services().state_store().cloned())
}

/// Saves the primary window's placement; called as it closes.
pub(crate) fn save_placement(runtime: &Runtime) {
    // An embedded root's placement is the host's to keep.
    if runtime.window_id != WindowId::PRIMARY || runtime.embedded {
        return;
    }
    let Some(store) = store(runtime) else {
        return;
    };
    let mut placement = WINDOWPLACEMENT {
        length: u32::try_from(size_of::<WINDOWPLACEMENT>()).unwrap_or(u32::MAX),
        ..WINDOWPLACEMENT::default()
    };
    // SAFETY: `runtime.window` is live (this runs before it is destroyed);
    // `placement` is initialized with its length and exclusively borrowed.
    if unsafe { GetWindowPlacement(runtime.window, &raw mut placement) } == 0 {
        return;
    }
    let normal = placement.rcNormalPosition;
    let saved = Placement {
        left: normal.left,
        top: normal.top,
        right: normal.right,
        bottom: normal.bottom,
        maximized: placement.showCmd == SW_SHOWMAXIMIZED as u32,
    };
    if let Ok(bytes) = serde_json::to_vec(&saved) {
        best_effort(
            store.save(PLACEMENT_KEY, &bytes).is_ok(),
            "save(window placement)",
            "the window opens at its default place next time",
        );
    }
}

/// Restores the primary window's saved placement, if it has one and it is
/// still on a connected monitor. Returns whether it did (and so showed the
/// window).
pub(crate) fn restore_placement(runtime: &Runtime) -> bool {
    if runtime.window_id != WindowId::PRIMARY {
        return false;
    }
    let Some(saved) = store(runtime)
        .and_then(|store| store.load(PLACEMENT_KEY).ok().flatten())
        .and_then(|bytes| serde_json::from_slice::<Placement>(&bytes).ok())
    else {
        return false;
    };
    let normal =
        RECT { left: saved.left, top: saved.top, right: saved.right, bottom: saved.bottom };
    // A window saved on a monitor that has since been disconnected would
    // come back somewhere nobody can see it; the default place is better.
    // SAFETY: `normal` is a plain value borrowed for the call.
    if unsafe { MonitorFromRect(&raw const normal, MONITOR_DEFAULTTONULL) }.is_null()
        || normal.right <= normal.left
        || normal.bottom <= normal.top
    {
        return false;
    }
    let placement = WINDOWPLACEMENT {
        length: u32::try_from(size_of::<WINDOWPLACEMENT>()).unwrap_or(u32::MAX),
        flags: 0,
        showCmd: if saved.maximized { SW_SHOWMAXIMIZED as u32 } else { SW_SHOWNORMAL as u32 },
        ptMinPosition: POINT { x: -1, y: -1 },
        ptMaxPosition: POINT { x: -1, y: -1 },
        rcNormalPosition: normal,
    };
    // SAFETY: `runtime.window` is live; `placement` is fully initialized
    // and borrowed for the call.
    unsafe { SetWindowPlacement(runtime.window, &raw const placement) != 0 }
}

/// The saved placement, for tests.
#[cfg(test)]
pub(crate) fn saved_placement(store: &dyn StateStore) -> Option<(RECT, bool)> {
    let saved: Placement = serde_json::from_slice(&store.load(PLACEMENT_KEY).ok()??).ok()?;
    Some((
        RECT { left: saved.left, top: saved.top, right: saved.right, bottom: saved.bottom },
        saved.maximized,
    ))
}

/// Whether `hwnd` is the window lifecycle notifications are reported for:
/// the primary one, so a multi-window application hears each step once.
pub(crate) fn is_primary(runtime: &Runtime) -> bool {
    runtime.window_id == WindowId::PRIMARY
}
