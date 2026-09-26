//! Event and render tracing, state history, and the per-application
//! inspection state they live in.

use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};

use super::overlay::OverlayMode;
use super::record::Recorder;
use super::server::InspectServer;
use super::{HistoryEntry, HttpTape};

/// How many trace entries, and history entries, are kept.
const KEPT: usize = 512;

/// One component that rendered, and why (`C04-2`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderInfo {
    /// The component's key path.
    pub component: String,
    /// Why it rendered: `initial`, `event`, `message`, `props`,
    /// `environment(<key>)`, `preference(<key>)`, `theme`, or `forced`.
    pub cause: String,
}

/// What one render pass did: every component either rendered, with its
/// cause, or was skipped because nothing it depends on changed.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PassInfo {
    /// The components that rendered.
    pub rendered: Vec<RenderInfo>,
    /// The components whose previous output was reused.
    pub skipped: Vec<String>,
}

/// What one trace entry records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TraceKind {
    /// An event dispatched to a window.
    Event {
        /// The event.
        event: String,
        /// Whether a component handled it.
        handled: bool,
        /// The render pass it caused, if any.
        pass: Option<PassInfo>,
    },
    /// Completed tasks delivered their results.
    Tasks {
        /// The render pass they caused.
        pass: PassInfo,
    },
    /// The inspector edited a component's state.
    Edit {
        /// The component's key path.
        component: String,
        /// The field.
        field: String,
        /// The render pass it caused.
        pass: PassInfo,
    },
    /// An error boundary contained a failure (`PLAN.md` Milestone 47).
    Failure {
        /// The boundary's key path.
        component: String,
        /// The panic's message.
        message: String,
        /// How many failures the boundary has contained.
        attempt: u32,
    },
}

/// One traced happening.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceEntry {
    /// Its sequence number, increasing from 1.
    pub seq: u64,
    /// The window it happened in.
    pub window: u64,
    /// How long handling it took, in microseconds — the frame cost the
    /// overlay graphs.
    pub micros: u64,
    /// What happened.
    pub kind: TraceKind,
}

/// One application's inspection state. Tracing and history cost nothing
/// until something turns inspection on: a server, an overlay, or a
/// recording.
#[derive(Default)]
pub(crate) struct Inspection {
    pub(crate) enabled: bool,
    pub(crate) seq: u64,
    pub(crate) trace: VecDeque<TraceEntry>,
    pub(crate) history: VecDeque<HistoryEntry>,
    pub(crate) server: Option<InspectServer>,
    pub(crate) overlay: Option<OverlayMode>,
    pub(crate) recorder: Option<Recorder>,
    pub(crate) http: Option<HttpTape>,
    pub(crate) quit: bool,
}

impl std::fmt::Debug for Inspection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inspection")
            .field("enabled", &self.enabled)
            .field("trace", &self.trace.len())
            .field("overlay", &self.overlay)
            .field("recording", &self.recorder.is_some())
            .finish_non_exhaustive()
    }
}

impl Inspection {
    /// Whether anything is watching.
    pub(crate) const fn active(&self) -> bool {
        self.enabled || self.overlay.is_some() || self.recorder.is_some()
    }

    /// Appends an entry, returning its sequence number.
    pub(crate) fn push(&mut self, window: u64, micros: u64, kind: TraceKind) -> u64 {
        self.seq += 1;
        if self.trace.len() == KEPT {
            self.trace.pop_front();
        }
        self.trace.push_back(TraceEntry { seq: self.seq, window, micros, kind });
        self.seq
    }

    /// Records inspectable components' state after change `seq`, when it
    /// differs from the last recorded.
    pub(crate) fn remember(
        &mut self,
        seq: u64,
        cause: String,
        states: BTreeMap<String, serde_json::Value>,
    ) {
        if states.is_empty() || self.history.back().is_some_and(|last| last.states == states) {
            return;
        }
        if self.history.len() == KEPT {
            self.history.pop_front();
        }
        self.history.push_back(HistoryEntry { seq, cause, states });
    }
}
