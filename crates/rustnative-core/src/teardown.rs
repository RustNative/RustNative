//! What must be put back when an application stops — normally or by a
//! panic (`PLAN.md` Milestone 39).
//!
//! A process that exits while holding the mouse capture, a clipped cursor,
//! a full-screen mode, or an open IME composition leaves the host in a
//! state the person has to repair. A terminal left in raw mode is the
//! famous case (Milestone 38); every host has its own. The policy is a
//! checklist a backend implements and a test verifies by panicking on
//! purpose: [`TeardownPolicy::STANDARD`] lists what every backend restores,
//! and a backend's documentation lists how.

/// One piece of host state a backend restores at teardown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Restoration {
    /// Release any pointer capture.
    PointerCapture,
    /// Unclip the cursor and restore its shape and visibility.
    Cursor,
    /// Leave full-screen or exclusive modes.
    FullScreen,
    /// Cancel an open input-method composition.
    Ime,
    /// Stop every timer and frame callback the backend started.
    Timers,
    /// Revoke registrations with the host (drop targets, hot keys, tray
    /// icons, clipboard listeners).
    Registrations,
    /// Flush persisted state.
    PersistedState,
    /// Return a terminal to cooked mode and the main screen (Milestone 38).
    Terminal,
    /// Put hardware outputs in their declared safe state (Milestone 37).
    Hardware,
}

/// The restorations a backend performs, on normal exit and on panic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeardownPolicy {
    restorations: Vec<Restoration>,
}

impl TeardownPolicy {
    /// What every windowed backend restores.
    pub const STANDARD: &'static [Restoration] = &[
        Restoration::PointerCapture,
        Restoration::Cursor,
        Restoration::FullScreen,
        Restoration::Ime,
        Restoration::Timers,
        Restoration::Registrations,
        Restoration::PersistedState,
    ];

    /// A policy restoring `restorations`.
    #[must_use]
    pub fn new(restorations: impl IntoIterator<Item = Restoration>) -> Self {
        Self { restorations: restorations.into_iter().collect() }
    }

    /// The standard windowed policy.
    #[must_use]
    pub fn standard() -> Self {
        Self::new(Self::STANDARD.iter().copied())
    }

    /// Whether this policy restores `restoration`.
    #[must_use]
    pub fn restores(&self, restoration: Restoration) -> bool {
        self.restorations.contains(&restoration)
    }

    /// Every restoration, in order.
    #[must_use]
    pub fn restorations(&self) -> &[Restoration] {
        &self.restorations
    }
}
