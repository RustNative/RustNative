//! Hybrid logical clocks: timestamps that follow wall time, never go
//! backwards, and order every event across replicas — the basis of
//! last-writer-wins.
//!
//! Wall time is the system's by default; [`HlcClock::with_clock`] reads it
//! from a `rustnative_core::Clock` instead (`Services::clock`, or a
//! `ManualClock` in a test), so a replica's stamps are as controllable as
//! every other time the framework reads.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// A replica's identity.
pub type ReplicaId = u64;

/// A hybrid logical timestamp: wall milliseconds, a counter for events in
/// the same millisecond, and the replica (so two stamps are never equal).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct Hlc {
    /// Milliseconds since the Unix epoch.
    pub wall: u64,
    /// Events within the millisecond.
    pub counter: u32,
    /// The replica that made it.
    pub replica: ReplicaId,
}

/// A replica's hybrid logical clock (named apart from
/// `rustnative_core::Clock`, the monotonic source it can read from).
pub struct HlcClock {
    replica: ReplicaId,
    last: std::sync::Mutex<Hlc>,
    offset: AtomicU64,
    /// A core clock and the Unix time its zero stands for; `None` reads the
    /// system's wall clock.
    source: Option<(Arc<dyn rustnative_core::Clock>, Duration)>,
}

impl std::fmt::Debug for HlcClock {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HlcClock")
            .field("replica", &self.replica)
            .field("last", &self.last)
            .field("from_core_clock", &self.source.is_some())
            .finish_non_exhaustive()
    }
}

impl HlcClock {
    /// A clock for `replica`.
    #[must_use]
    pub fn new(replica: ReplicaId) -> Self {
        Self {
            replica,
            last: std::sync::Mutex::new(Hlc { wall: 0, counter: 0, replica }),
            offset: AtomicU64::new(0),
            source: None,
        }
    }

    /// A clock for `replica` reading `clock`, whose zero is Unix time
    /// `epoch` — `Services::clock` with the wall time read once at start,
    /// or a `ManualClock` that a test advances.
    #[must_use]
    pub fn with_clock(
        replica: ReplicaId,
        clock: Arc<dyn rustnative_core::Clock>,
        epoch: Duration,
    ) -> Self {
        Self { source: Some((clock, epoch)), ..Self::new(replica) }
    }

    /// This clock's replica.
    #[must_use]
    pub const fn replica(&self) -> ReplicaId {
        self.replica
    }

    /// Skews this clock's wall time by `ms` (a test's way to make one
    /// device's clock run ahead).
    pub fn skew(&self, ms: u64) {
        self.offset.store(ms, Ordering::SeqCst);
    }

    fn physical(&self) -> u64 {
        let since = match &self.source {
            Some((clock, epoch)) => epoch.saturating_add(clock.now()),
            None => SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default(),
        };
        let now = u64::try_from(since.as_millis()).unwrap_or(u64::MAX);
        now.saturating_add(self.offset.load(Ordering::SeqCst))
    }

    /// A stamp for a local event.
    pub fn now(&self) -> Hlc {
        let physical = self.physical();
        let mut last = self.last.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *last = if physical > last.wall {
            Hlc { wall: physical, counter: 0, replica: self.replica }
        } else {
            Hlc { wall: last.wall, counter: last.counter.saturating_add(1), replica: self.replica }
        };
        *last
    }

    /// Advances past a stamp received from another replica, so the next
    /// local stamp orders after it.
    pub fn observe(&self, remote: Hlc) {
        let mut last = self.last.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if remote.wall > last.wall || (remote.wall == last.wall && remote.counter > last.counter) {
            *last = Hlc { wall: remote.wall, counter: remote.counter, replica: self.replica };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_core_clock_drives_the_wall_time() {
        let manual = rustnative_core::ManualClock::new();
        let clock = HlcClock::with_clock(3, Arc::new(manual.clone()), Duration::from_secs(1_000));
        assert_eq!(clock.now(), Hlc { wall: 1_000_000, counter: 0, replica: 3 });
        assert_eq!(clock.now().counter, 1, "the same millisecond counts up");
        manual.advance(Duration::from_millis(5));
        assert_eq!(clock.now(), Hlc { wall: 1_000_005, counter: 0, replica: 3 });
        manual.set(Duration::ZERO);
        assert!(clock.now().wall >= 1_000_005, "a clock set back never goes backwards");
    }
}
