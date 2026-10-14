//! The terminal itself: the seam between the session and the host's I/O.
//!
//! A [`Terminal`] is what the session loop talks to. Two kinds implement
//! it: the process's own terminal ([`open`] — `termios` on Unix, the
//! console API on Windows) and the synthetic terminal the tests drive
//! (`crate::harness`). Everything above this seam is the same for both.

use std::io;
use std::sync::Arc;
use std::time::Duration;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::ProcessTerminal;
#[cfg(windows)]
pub use windows::ProcessTerminal;

#[cfg(unix)]
pub(crate) use unix::restore_now;
#[cfg(windows)]
pub(crate) use windows::restore_now;

/// Something the host told the process, other than input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Signal {
    /// Asked to end politely (`SIGTERM`, `SIGINT` from outside, `SIGQUIT`,
    /// Ctrl+Break, logoff, shutdown).
    Terminate,
    /// The terminal went away (`SIGHUP`, the console window closed): end
    /// now; nobody sees the screen.
    Hangup,
    /// Asked to stop (`SIGTSTP` from outside): restore the terminal and
    /// stop until continued.
    Suspend,
    /// Continued after a stop (`SIGCONT`): retake the terminal and redraw.
    Continue,
}

/// What [`Terminal::wait`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Readiness {
    /// Input bytes, in order.
    pub input: Vec<u8>,
    /// Whether the terminal's size changed.
    pub resized: bool,
    /// Signals delivered.
    pub signals: Vec<Signal>,
    /// Whether [`Terminal::waker`] was called.
    pub woken: bool,
}

impl Readiness {
    /// Whether nothing happened (the wait timed out).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.input.is_empty() && !self.resized && self.signals.is_empty() && !self.woken
    }
}

/// Wakes a waiting terminal from any thread.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// A terminal the session runs on.
pub trait Terminal {
    /// Columns and rows.
    fn size(&self) -> (u16, u16);

    /// Writes `bytes` and flushes them.
    ///
    /// # Errors
    ///
    /// The terminal is gone.
    fn write(&mut self, bytes: &[u8]) -> io::Result<()>;

    /// Waits up to `timeout` (`None`: indefinitely) for input, a resize, a
    /// signal, or a wake, and returns what happened.
    ///
    /// # Errors
    ///
    /// The terminal is gone.
    fn wait(&mut self, timeout: Option<Duration>) -> io::Result<Readiness>;

    /// A handle that ends a [`Self::wait`] early, callable from any thread.
    fn waker(&self) -> Waker;

    /// Gives the terminal back to the shell (cooked mode, main screen) and
    /// stops the process until it is continued; returns once it is, with
    /// the terminal taken again. A no-op where the host has no job control.
    ///
    /// # Errors
    ///
    /// The terminal could not be retaken.
    fn suspend(&mut self) -> io::Result<()> {
        Ok(())
    }

    /// Whether this is the Windows console in virtual-terminal mode (for
    /// colour detection).
    fn is_windows_console(&self) -> bool {
        false
    }

    /// The terminal's name for diagnostics (`TERM`, or the console).
    fn name(&self) -> String {
        std::env::var("TERM").unwrap_or_else(|_| "unknown".to_owned())
    }
}

/// Opens the process's own terminal in raw mode, with restoration armed
/// (`crate::guard`): the controlling terminal on Unix (`/dev/tty`, so it
/// works with standard input and output redirected), the console on Windows
/// (`CONIN$`/`CONOUT$`).
///
/// # Errors
///
/// There is no terminal (the process runs detached, or under a service
/// manager), or it refuses raw mode or virtual-terminal processing.
pub fn open() -> io::Result<ProcessTerminal> {
    ProcessTerminal::open()
}
