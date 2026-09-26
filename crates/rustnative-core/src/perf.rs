//! What the budgets measure (`PLAN.md` Milestone 42): the startup phase
//! model (`C62`), frame times, and when the last change was realized.
//!
//! These are process-wide, because a process starts once: a backend marks
//! each phase as it reaches it, the first time only, and says when the
//! process started (which only the host knows). Frame times and the last
//! realization are recorded only while something asked for them.
//!
//! With `RUSTNATIVE_STARTUP_TRACE=1` in the environment the phases are
//! printed to standard error when the application becomes interactive.
//!
//! ```
//! use rustnative_core::perf::{self, StartupPhase};
//!
//! perf::mark(StartupPhase::RuntimeReady);
//! let trace = perf::startup_trace();
//! assert!(trace.get(StartupPhase::RuntimeReady).is_some());
//! ```

use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// A phase of starting up, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupPhase {
    /// The process was created.
    ProcessStart,
    /// The runtime is ready: the backend has initialized and can create
    /// windows.
    RuntimeReady,
    /// The first window exists with its tree realized.
    FirstFrame,
    /// The first window has painted: the application's content is on
    /// screen.
    FirstContent,
    /// The first time, after its content is on screen, the application has
    /// nothing left to do: it responds to input immediately.
    Interactive,
}

impl StartupPhase {
    /// Every phase, in order.
    pub const ALL: [Self; 5] = [
        Self::ProcessStart,
        Self::RuntimeReady,
        Self::FirstFrame,
        Self::FirstContent,
        Self::Interactive,
    ];

    /// The phase's name in budget files (`first_frame`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ProcessStart => "process_start",
            Self::RuntimeReady => "runtime_ready",
            Self::FirstFrame => "first_frame",
            Self::FirstContent => "first_content",
            Self::Interactive => "interactive",
        }
    }
}

/// When each phase was reached, measured from the process's start.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StartupTrace {
    /// The phases reached, in order, each with its time since the process
    /// started.
    pub phases: Vec<(StartupPhase, Duration)>,
}

impl StartupTrace {
    /// When `phase` was reached, if it was.
    #[must_use]
    pub fn get(&self, phase: StartupPhase) -> Option<Duration> {
        self.phases.iter().find(|(reached, _)| *reached == phase).map(|(_, at)| *at)
    }
}

impl fmt::Display for StartupTrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut previous = Duration::ZERO;
        for (phase, at) in &self.phases {
            writeln!(
                f,
                "{:<14} {:>9.3} ms  (+{:.3} ms)",
                phase.name(),
                at.as_secs_f64() * 1e3,
                at.saturating_sub(previous).as_secs_f64() * 1e3
            )?;
            previous = *at;
        }
        Ok(())
    }
}

struct State {
    /// The first moment this module was touched, and how long before it
    /// the process started.
    origin: Option<(Instant, Duration)>,
    marks: Vec<(StartupPhase, Instant)>,
    frames: Option<Vec<Instant>>,
    realized: Option<Instant>,
}

static STATE: Mutex<State> =
    Mutex::new(State { origin: None, marks: Vec::new(), frames: None, realized: None });

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut state = STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.origin.is_none() {
        state.origin = Some((Instant::now(), Duration::ZERO));
    }
    f(&mut state)
}

/// Records that the process started `before` ago — what only the host can
/// say (on Windows, `GetProcessTimes`). Without it, the process's start is
/// taken to be the first time this module was used.
pub fn set_process_start(before: Duration) {
    with(|state| state.origin = Some((Instant::now(), before)));
}

/// Marks `phase` as reached now, the first time only. Returns whether this
/// was the first time.
#[allow(clippy::must_use_candidate, reason = "a backend marks a phase and moves on")]
pub fn mark(phase: StartupPhase) -> bool {
    let first = with(|state| {
        if state.marks.iter().any(|(reached, _)| *reached == phase) {
            return false;
        }
        state.marks.push((phase, Instant::now()));
        true
    });
    if first
        && phase == StartupPhase::Interactive
        && std::env::var("RUSTNATIVE_STARTUP_TRACE").is_ok_and(|value| value == "1")
    {
        eprint!("rustnative startup:\n{}", startup_trace());
    }
    first
}

/// Whether `phase` has been reached.
#[must_use]
pub fn reached(phase: StartupPhase) -> bool {
    with(|state| state.marks.iter().any(|(reached, _)| *reached == phase))
}

/// The phases reached so far, from the process's start.
#[must_use]
pub fn startup_trace() -> StartupTrace {
    with(|state| {
        let (origin, before) = state.origin.unwrap_or((Instant::now(), Duration::ZERO));
        let since_start = |at: Instant| at.saturating_duration_since(origin) + before;
        let mut phases = vec![(StartupPhase::ProcessStart, Duration::ZERO)];
        phases.extend(state.marks.iter().map(|(phase, at)| (*phase, since_start(*at))));
        phases.sort_by_key(|(phase, _)| *phase);
        StartupTrace { phases }
    })
}

/// Starts recording frame times (discarding any recorded).
pub fn record_frames() {
    with(|state| state.frames = Some(Vec::new()));
}

/// Records that a frame was produced now, if frames are being recorded.
/// A backend calls this for every animation frame.
pub fn frame() {
    with(|state| {
        if let Some(frames) = state.frames.as_mut() {
            frames.push(Instant::now());
        }
    });
}

/// The intervals between recorded frames, and stops recording.
#[must_use]
pub fn take_frame_times() -> Vec<Duration> {
    with(|state| {
        let frames = state.frames.take().unwrap_or_default();
        frames.windows(2).map(|pair| pair[1].saturating_duration_since(pair[0])).collect()
    })
}

/// Records that a change was realized now. A backend calls this after each
/// render reaches its host objects, so input latency — synthetic input to
/// realized change — can be measured through the real input path.
pub fn realized() {
    with(|state| state.realized = Some(Instant::now()));
}

/// When a change was last realized.
#[must_use]
pub fn last_realized() -> Option<Instant> {
    with(|state| state.realized)
}

/// `values` at percentile `p` (0–100), nearest rank; `None` when empty.
#[must_use]
pub fn percentile(values: &[Duration], p: u8) -> Option<Duration> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let last = sorted.len().checked_sub(1)?;
    let rank = (last * usize::from(p.min(100))).div_ceil(100);
    sorted.get(rank).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_are_marked_once_in_order_from_the_process_start() {
        set_process_start(Duration::from_millis(40));
        assert!(mark(StartupPhase::FirstFrame));
        assert!(!mark(StartupPhase::FirstFrame), "only the first time counts");
        mark(StartupPhase::RuntimeReady);
        let trace = startup_trace();
        let names: Vec<_> = trace.phases.iter().map(|(phase, _)| *phase).collect();
        assert_eq!(names[0], StartupPhase::ProcessStart);
        assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(trace.get(StartupPhase::RuntimeReady).unwrap() >= Duration::from_millis(40));
        assert!(trace.to_string().contains("first_frame"));
    }

    #[test]
    fn percentiles_use_the_nearest_rank() {
        let values: Vec<_> = (1..=100).map(Duration::from_millis).collect();
        assert_eq!(percentile(&values, 50), Some(Duration::from_millis(51)));
        assert_eq!(percentile(&values, 99), Some(Duration::from_millis(100)));
        assert_eq!(percentile(&values, 100), Some(Duration::from_millis(100)));
        assert_eq!(percentile(&[], 50), None);
    }
}
