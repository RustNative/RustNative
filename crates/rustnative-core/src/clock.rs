//! The host clock: the one place framework code reads "now".
//!
//! Time has two consumers in this crate that must agree: the scheduler,
//! whose delays already run on the executor's own notion of time
//! ([`crate::Executor::sleep`]), and everything that *compares* times — a
//! cache's freshness lifetime, a debounce, a retry backoff, a startup
//! phase. The second group used to have nothing to read but the operating
//! system's clock, which a test cannot move. [`Clock`] is the seam: code
//! that needs the current time asks the [`crate::Services`] it was given
//! for [`crate::Services::clock`], a backend supplies [`SystemClock`], and
//! a test supplies [`ManualClock`] — or a [`crate::ManualExecutor`], which
//! is a clock too, so one object moves both delays and timestamps.
//!
//! A clock reports a [`Duration`] since an arbitrary, fixed origin rather
//! than a wall-clock date: every consumer compares instants, and a
//! monotonic origin is what a date cannot promise.
//!
//! # Example
//!
//! ```
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! use rustnative_core::{Clock, ManualClock, Services};
//!
//! let clock = ManualClock::new();
//! let services = Services::default().with_clock(Arc::new(clock.clone()));
//!
//! let start = services.clock().now();
//! clock.advance(Duration::from_millis(1_500));
//! assert_eq!(services.clock().now() - start, Duration::from_millis(1_500));
//! ```

use std::sync::Mutex;
use std::sync::{Arc, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// A source of monotonic time.
pub trait Clock: Send + Sync + 'static {
    /// The time elapsed since this clock's fixed origin.
    fn now(&self) -> Duration;
}

/// The process's monotonic clock, measured from the first time any
/// [`SystemClock`] was read in this process.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

pub(crate) fn process_epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        process_epoch().elapsed()
    }
}

/// A clock that moves only when told to — for tests.
#[derive(Debug, Clone, Default)]
pub struct ManualClock {
    now: Arc<Mutex<Duration>>,
}

impl ManualClock {
    /// A clock starting at zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Moves the clock forward by `duration`.
    pub fn advance(&self, duration: Duration) {
        let mut now = self.now.lock().unwrap_or_else(PoisonError::into_inner);
        *now = now.saturating_add(duration);
    }

    /// Sets the clock to `now`, which may be earlier than the current value
    /// — a test of how code copes with a clock that was reset.
    pub fn set(&self, now: Duration) {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner) = now;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Clock for crate::ManualExecutor {
    fn now(&self) -> Duration {
        crate::ManualExecutor::now(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_clock_never_goes_backwards() {
        let clock = SystemClock;
        let first = clock.now();
        let second = clock.now();
        assert!(second >= first);
    }

    #[test]
    fn a_manual_clock_moves_only_when_told() {
        let clock = ManualClock::new();
        assert_eq!(clock.now(), Duration::ZERO);
        clock.advance(Duration::from_secs(2));
        assert_eq!(clock.now(), Duration::from_secs(2));
        let shared = clock.clone();
        shared.advance(Duration::from_secs(1));
        assert_eq!(clock.now(), Duration::from_secs(3), "clones share one clock");
        clock.set(Duration::from_secs(1));
        assert_eq!(clock.now(), Duration::from_secs(1));
    }

    #[test]
    fn a_manual_executor_is_a_clock_that_moves_with_its_delays() {
        let executor = crate::ManualExecutor::new();
        executor.advance(Duration::from_millis(250));
        assert_eq!(Clock::now(&executor), Duration::from_millis(250));
    }
}
