//! The activity and process lifecycle on the portable lifecycle and
//! state-restoration contracts (`PLAN.md` Milestone 35;
//! `docs/android/lifecycle.md`).
//!
//! | Android | Portable |
//! |---|---|
//! | primary `onResume` | `Lifecycle::Resuming` |
//! | primary `onPause` | state flushed, then `Lifecycle::Suspending` |
//! | `onSaveInstanceState` | state flushed (the process may be killed next) |
//! | primary `onDestroy`, finishing | state flushed, `Lifecycle::Terminating`; the application ends |
//! | another window's `onDestroy`, finishing | the window closes (`Application::close_window`) |
//! | `onDestroy`, not finishing (recreation) | the window's views go; the window, its components, and their state stay for the next activity |
//! | `onConfigurationChanged` | host traits re-read and the window relaid out (no recreation: the manifest handles every change in place) |
//! | `onTrimMemory(RUNNING_LOW, RUNNING_CRITICAL, MODERATE, COMPLETE)`, `onLowMemory` | `Lifecycle::LowMemory` |
//! | `onMultiWindowModeChanged` | `keys::WINDOW_MODE` |
//! | the window extensions' folding features | `keys::POSTURE` |
//!
//! Process death needs nothing here: the next process starts the
//! application's `main` again, whose state store restores what was flushed.

// `ComponentCallbacks2` trim levels.
const TRIM_MEMORY_RUNNING_LOW: i32 = 10;
const TRIM_MEMORY_RUNNING_CRITICAL: i32 = 15;
const TRIM_MEMORY_MODERATE: i32 = 60;
const TRIM_MEMORY_COMPLETE: i32 = 80;

/// Whether a trim level asks the application to give memory back.
pub(crate) const fn is_low_memory(level: i32) -> bool {
    matches!(
        level,
        TRIM_MEMORY_RUNNING_LOW
            | TRIM_MEMORY_RUNNING_CRITICAL
            | TRIM_MEMORY_MODERATE
            | TRIM_MEMORY_COMPLETE
    )
}

#[cfg(target_os = "android")]
pub(crate) use platform::step;

#[cfg(target_os = "android")]
mod platform {
    use rustnative_core::{Lifecycle, WindowId};

    use crate::Error;
    use crate::backend::Flow;
    use crate::protocol;
    use crate::registry::WindowRegistry;

    /// One lifecycle step of `window`'s activity.
    pub(crate) fn step(
        registry: &mut WindowRegistry,
        window: WindowId,
        what: i32,
        argument: i32,
    ) -> Result<Flow, Error> {
        let primary = window == WindowId::PRIMARY;
        match what {
            protocol::LC_START => {
                if let Some(runtime) = registry.windows.get_mut(&window) {
                    runtime.started = true;
                }
            }
            protocol::LC_STOP => {
                if let Some(runtime) = registry.windows.get_mut(&window) {
                    runtime.started = false;
                }
            }
            protocol::LC_RESUME if primary => registry.lifecycle(Lifecycle::Resuming)?,
            protocol::LC_PAUSE if primary => registry.lifecycle(Lifecycle::Suspending)?,
            protocol::LC_SAVE => {
                if let Err(error) =
                    registry.with_application(rustnative_core::Application::flush_state)
                {
                    crate::log::warn(&format!("state was not flushed before saving: {error}"));
                }
            }
            protocol::LC_DESTROY => registry.activity_gone(window),
            protocol::LC_DESTROY_FINISHING if primary => {
                registry.lifecycle(Lifecycle::Terminating)?;
                registry.close(window);
            }
            protocol::LC_DESTROY_FINISHING => {
                registry.with_application(|application| application.close_window(window));
                registry.close(window);
                registry.sync()?;
            }
            protocol::LC_CONFIGURATION => {
                registry.refresh_host_traits()?;
                registry.relayout(window)?;
            }
            protocol::LC_TRIM if super::is_low_memory(argument) => {
                registry.lifecycle(Lifecycle::LowMemory)?;
            }
            protocol::LC_LOW_MEMORY => registry.lifecycle(Lifecycle::LowMemory)?,
            protocol::LC_POSTURE => {
                crate::environment::window_changed(registry, window);
                registry.render(window)?;
            }
            protocol::LC_MULTI_WINDOW => {
                registry.traits.multi_window = argument != 0;
                crate::environment::window_changed(registry, window);
                registry.render(window)?;
            }
            _ => {}
        }
        Ok(registry.flow())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_pressure_levels_are_low_memory() {
        assert!(super::is_low_memory(10));
        assert!(super::is_low_memory(80));
        // UI_HIDDEN and BACKGROUND are not pressure: suspension covered them.
        assert!(!super::is_low_memory(20));
        assert!(!super::is_low_memory(40));
    }
}
