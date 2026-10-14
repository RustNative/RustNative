//! Terminal restoration (`PLAN.md` Milestone 38: "a crashed application
//! must not leave a terminal unusable"; Milestone 39's teardown policy).
//!
//! Restoration is armed by [`crate::term::open`] *before* the terminal is
//! changed and the first escape sequence is written, and it runs, once, on
//! whichever of these comes first:
//!
//! | Path | How |
//! |---|---|
//! | normal exit | the terminal's `Drop` |
//! | a panic, caught or not | a panic hook installed once and chained to the hook before it, so the report prints on the restored screen |
//! | `SIGTERM`, `SIGINT`, `SIGQUIT`, `SIGHUP` | the loop ends cleanly (flushing state) and drops the terminal; a second signal while it is stuck restores from the signal handler and exits with `128 + signal` |
//! | `SIGTSTP` | restored, stopped, and retaken on `SIGCONT` with a full redraw |
//! | Ctrl+Break, the console closing, logoff, shutdown (Windows) | the control handler tells the loop, waits up to three seconds for it, then restores itself |
//!
//! What is restored: the `termios` mode (or the console's input and output
//! modes and code pages), and every mode the session set — the alternate
//! screen, cursor visibility and shape, autowrap, mouse reporting, focus
//! reporting, bracketed paste, the kitty keyboard protocol, a frame left
//! open by synchronized output, the SGR state, and the window title.

use std::sync::Once;

use rustnative_core::{Restoration, TeardownPolicy};

static HOOK: Once = Once::new();

/// Installs the panic hook that restores the terminal, chained to the hook
/// already installed (the core's crash capture, the test harness's, or the
/// default). Done once per process.
pub(crate) fn install_panic_hook() {
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            previous(info);
        }));
    });
}

/// Restores the terminal now, if a session holds it — what an application
/// calls before it hands the terminal to another program itself. Safe to
/// call any number of times, from any thread.
pub fn restore() {
    crate::term::restore_now();
}

/// What this backend restores, on normal exit and on panic (Milestone 39):
/// the terminal itself, the cursor, mouse reporting (the terminal's form of
/// pointer capture), every timer and frame it started, the signal and
/// console handlers it registered, and persisted state.
#[must_use]
pub fn teardown_policy() -> TeardownPolicy {
    TeardownPolicy::new([
        Restoration::Terminal,
        Restoration::Cursor,
        Restoration::PointerCapture,
        Restoration::Timers,
        Restoration::Registrations,
        Restoration::PersistedState,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_policy_restores_the_terminal() {
        let policy = teardown_policy();
        assert!(policy.restores(Restoration::Terminal));
        assert!(policy.restores(Restoration::PersistedState));
        assert!(
            !policy.restores(Restoration::FullScreen),
            "a terminal has no full-screen mode to leave"
        );
        // With no session, restoring is a no-op, however often.
        restore();
        restore();
    }
}
