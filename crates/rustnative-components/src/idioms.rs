//! The per-host idiom table (`PLAN.md` Milestone 48, `C23`): behaviour that
//! differs by host even for the same control — the order of a dialog's
//! buttons, how it is dismissed, where a destructive action goes. The
//! whole table for Windows is `docs/idioms/windows.md`.
//!
//! Components read the host's idioms from the environment
//! ([`IDIOMS`]), so a backend (or a test) provides another host's.

use rustnative_core::environment::EnvKey;

/// How a host lays out and dismisses things.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Idioms {
    /// Whether the confirming button comes before the cancelling one
    /// (Windows: OK, Cancel) or after (macOS, GNOME: Cancel, OK).
    pub confirm_first: bool,
    /// Whether Escape cancels a dialog.
    pub escape_dismisses: bool,
    /// Whether a destructive action is styled apart from the others.
    pub mark_destructive: bool,
}

impl Idioms {
    /// Windows: OK then Cancel, Escape cancels.
    #[must_use]
    pub const fn windows() -> Self {
        Self { confirm_first: true, escape_dismisses: true, mark_destructive: true }
    }

    /// macOS and GNOME: Cancel then OK, Escape cancels.
    #[must_use]
    pub const fn macos() -> Self {
        Self { confirm_first: false, escape_dismisses: true, mark_destructive: true }
    }
}

impl Default for Idioms {
    /// Windows' — the backend this framework ships today.
    fn default() -> Self {
        Self::windows()
    }
}

/// The environment key components read their host's idioms from.
pub static IDIOMS: EnvKey<Idioms> = EnvKey::new("rustnative.components.idioms", Idioms::default);
