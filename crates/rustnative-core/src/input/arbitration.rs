//! Gesture arbitration: how the framework's recognizers coexist with the
//! host's (`PLAN.md` Milestone 39).
//!
//! A pointer drag inside a scrolling list could be the list scrolling or
//! the node under it panning; a long press on text could select a word or
//! open the application's own menu. Users perceive a wrong answer as a bug,
//! and each host answers these conflicts its own way, so the answer is a
//! declared policy per node ([`GesturePolicy`], on [`crate::InputInterest`])
//! and a fixed table ([`arbitrate`]) every backend consults — not a
//! per-application workaround.
//!
//! | Conflict | `Exclusive` | `DeferToHost` | `Simultaneous` |
//! |---|---|---|---|
//! | scroll vs. pan | framework pans, container does not scroll | container scrolls, no pan | both |
//! | text selection vs. drag | framework drags | host selects | host selects, drag reported |
//! | system edge vs. pan | host | host | host |
//! | long press vs. context menu | framework's long press | host's menu | both |
//!
//! The system-edge row has no framework winner on purpose: an edge swipe
//! that opens the host's own UI (the notification centre, the app switcher)
//! belongs to the person's host, and an application cannot and should not
//! capture it.

/// How a node's gestures relate to the host's own handling of the same
/// input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GesturePolicy {
    /// The node's gestures win every conflict the host allows it to win.
    #[default]
    Exclusive,
    /// The host's behaviour wins; the node's recognizer yields.
    DeferToHost,
    /// Both happen.
    Simultaneous,
}

/// A conflict between a framework gesture and a host behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GestureConflict {
    /// A pan on a node inside a scrolling container.
    ScrollVsPan,
    /// A drag starting on selectable text.
    TextSelectionVsDrag,
    /// A pan that starts at a screen edge the host reserves.
    SystemEdgeVsPan,
    /// A long press where the host shows a context menu.
    LongPressVsContextMenu,
}

/// Who gets the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Winner {
    /// The framework's recognizer; the host's behaviour is suppressed.
    Framework,
    /// The host's behaviour; the framework's gesture is not reported.
    Host,
    /// Both.
    Both,
}

impl Winner {
    /// Whether the framework's gesture is reported.
    #[must_use]
    pub const fn framework_reports(self) -> bool {
        matches!(self, Self::Framework | Self::Both)
    }

    /// Whether the host's behaviour happens.
    #[must_use]
    pub const fn host_acts(self) -> bool {
        matches!(self, Self::Host | Self::Both)
    }
}

/// The arbitration table in the module documentation.
#[must_use]
pub const fn arbitrate(conflict: GestureConflict, policy: GesturePolicy) -> Winner {
    match (conflict, policy) {
        (GestureConflict::SystemEdgeVsPan, _) | (_, GesturePolicy::DeferToHost) => Winner::Host,
        (_, GesturePolicy::Exclusive) => Winner::Framework,
        (_, GesturePolicy::Simultaneous) => Winner::Both,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_what_the_documentation_says() {
        use GestureConflict::*;
        use GesturePolicy::*;
        assert_eq!(arbitrate(ScrollVsPan, Exclusive), Winner::Framework);
        assert_eq!(arbitrate(ScrollVsPan, DeferToHost), Winner::Host);
        assert_eq!(arbitrate(ScrollVsPan, Simultaneous), Winner::Both);
        for policy in [Exclusive, DeferToHost, Simultaneous] {
            assert_eq!(arbitrate(SystemEdgeVsPan, policy), Winner::Host, "the edge is the host's");
        }
        assert_eq!(arbitrate(LongPressVsContextMenu, DeferToHost), Winner::Host);
        assert!(arbitrate(TextSelectionVsDrag, Simultaneous).host_acts());
        assert!(arbitrate(TextSelectionVsDrag, Simultaneous).framework_reports());
    }
}
