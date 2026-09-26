//! Hybrid logical clocks: timestamps that follow wall time, never go
//! backwards, and order every event across replicas — the basis of
//! last-writer-wins.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

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

/// A replica's clock.
#[derive(Debug)]
pub struct Clock {
    replica: ReplicaId,
    last: std::sync::Mutex<Hlc>,
    offset: AtomicU64,
}

impl Clock {
    /// A clock for `replica`.
    #[must_use]
    pub fn new(replica: ReplicaId) -> Self {
        Self {
            replica,
            last: std::sync::Mutex::new(Hlc { wall: 0, counter: 0, replica }),
            offset: AtomicU64::new(0),
        }
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
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX));
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
