//! What an application does when a component panics.
//!
//! # Why this is a policy rather than a fixed behavior
//!
//! The standards audit's P2.33 finding is that this framework had two
//! inconsistent halves of an answer: the Windows backend catches panics at
//! its `WNDPROC` boundary (correct — unwinding across an `extern "system"`
//! frame is undefined behavior), while the core could still panic straight
//! through an `assert!` or `expect()`; and the caught half had exactly one
//! hard-coded response, terminate the application.
//!
//! > The production architecture should have a consistent application-level
//! > error/panic policy. [...] The policy should be configurable by the
//! > host.
//!
//! It should be configurable because the right answer is genuinely
//! different per application, and a framework cannot know which:
//!
//! - An editor with unsaved work would rather keep the window open on a
//!   panic in one panel than take the document down with it.
//! - A kiosk or a safety-adjacent tool wants the opposite: a component in an
//!   unknown state is a reason to stop, loudly and immediately.
//! - A test harness wants to observe the panic and keep going.
//!
//! # What a panic does and does not mean here
//!
//! A caught panic leaves the panicking component's own state arbitrary —
//! `update` may have mutated half its fields before unwinding. Every policy
//! other than [`PanicPolicy::Terminate`] is therefore a deliberate trade:
//! the application keeps running with one component whose invariants may not
//! hold. [`PanicPolicy::CloseWindow`] is the middle ground that bounds the
//! damage to a window whose whole tree can be discarded, which is why it is
//! the one this crate recommends for applications that do not want to exit.
//!
//! This is a policy about *panics*, which are bugs. Ordinary recoverable
//! failures are `Result`s — see [`crate::component::RenderError`] for where
//! this crate draws that line.

use std::fmt;

use crate::identity::WindowId;

/// How an application responds to a component panic caught at a platform
/// boundary.
///
/// # Example
///
/// ```
/// use rustnative_core::{PanicAction, PanicPolicy, PanicReport, WindowId};
///
/// let report = PanicReport {
///     message: "index out of bounds".to_owned(),
///     window: WindowId::PRIMARY,
/// };
///
/// // The default ends the application, whatever else is open.
/// assert_eq!(PanicPolicy::default(), PanicPolicy::Terminate);
/// assert_eq!(PanicPolicy::Terminate.resolve(&report, true), PanicAction::Terminate);
///
/// // `CloseWindow` bounds the damage to one window...
/// assert_eq!(
///     PanicPolicy::CloseWindow.resolve(&report, true),
///     PanicAction::CloseWindow(WindowId::PRIMARY),
/// );
/// // ...but will not close the last one and leave a loop with nothing to show.
/// assert_eq!(PanicPolicy::CloseWindow.resolve(&report, false), PanicAction::Terminate);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PanicPolicy {
    /// End the application, surfacing the panic as the failure of
    /// `Platform::run`.
    ///
    /// The default, because it is the only response that cannot leave an
    /// application running on state whose invariants a panic already
    /// disproved. An application that has weighed that trade opts into a
    /// different policy explicitly.
    #[default]
    Terminate,
    /// Close the window whose callback panicked and keep the application
    /// running, provided another window remains open.
    ///
    /// Bounds the damage to one window's component tree, which is discarded
    /// wholesale rather than resumed. If the panicking window is the last
    /// one open there is nothing left to run, so this degrades to
    /// [`Self::Terminate`].
    CloseWindow,
    /// Report the panic and continue with the component tree as it stands.
    ///
    /// The weakest policy: the panicking component keeps running with
    /// whatever state it had when it unwound. Appropriate for a development
    /// build or a test harness that wants to observe a panic without losing
    /// the session, and rarely appropriate for a shipped application.
    ReportAndContinue,
}

/// A component panic, as handed to the application's [`PanicPolicy`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanicReport {
    /// The panic's own message, where it carried one.
    pub message: String,
    /// The window whose callback the panic escaped from.
    pub window: WindowId,
}

impl fmt::Display for PanicReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "component panic in window {}: {}", self.window.get(), self.message)
    }
}

/// What a platform backend should actually do, once a [`PanicPolicy`] has
/// been applied to a specific [`PanicReport`].
///
/// A backend matches on this rather than on the policy itself, because the
/// decision depends on application state the policy alone does not capture
/// — whether closing this window would leave any window open at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PanicAction {
    /// Stop the event loop and surface the panic as an error.
    Terminate,
    /// Close this one window; the event loop continues.
    CloseWindow(WindowId),
    /// Do nothing beyond having caught the panic.
    Continue,
}

impl PanicPolicy {
    /// Resolves this policy against a specific panic.
    ///
    /// `other_windows_remain` is what stops [`Self::CloseWindow`] from
    /// closing the last window and leaving an application with a running
    /// event loop and nothing to show.
    #[must_use]
    pub const fn resolve(self, report: &PanicReport, other_windows_remain: bool) -> PanicAction {
        match self {
            Self::Terminate => PanicAction::Terminate,
            Self::CloseWindow => {
                if other_windows_remain {
                    PanicAction::CloseWindow(report.window)
                } else {
                    PanicAction::Terminate
                }
            }
            Self::ReportAndContinue => PanicAction::Continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> PanicReport {
        PanicReport { message: "boom".to_owned(), window: WindowId::PRIMARY }
    }

    #[test]
    fn the_default_policy_terminates() {
        assert_eq!(PanicPolicy::default(), PanicPolicy::Terminate);
        assert_eq!(PanicPolicy::default().resolve(&report(), true), PanicAction::Terminate);
    }

    #[test]
    fn close_window_closes_only_the_panicking_window() {
        assert_eq!(
            PanicPolicy::CloseWindow.resolve(&report(), true),
            PanicAction::CloseWindow(WindowId::PRIMARY)
        );
    }

    #[test]
    fn close_window_terminates_rather_than_closing_the_last_window() {
        assert_eq!(
            PanicPolicy::CloseWindow.resolve(&report(), false),
            PanicAction::Terminate,
            "an application with no windows left has nothing to continue with"
        );
    }

    #[test]
    fn report_and_continue_does_nothing_either_way() {
        assert_eq!(PanicPolicy::ReportAndContinue.resolve(&report(), true), PanicAction::Continue);
        assert_eq!(PanicPolicy::ReportAndContinue.resolve(&report(), false), PanicAction::Continue);
    }

    #[test]
    fn a_report_renders_the_window_and_the_message() {
        let rendered = report().to_string();
        assert!(rendered.contains("boom"));
        assert!(rendered.contains("window 0"));
    }
}
