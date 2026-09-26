//! Undo and redo (`PLAN.md` Milestone 47): a state history over ordered
//! updates.

use std::time::Duration;

/// A value with its past and its undone future.
///
/// Every [`Self::record`] is one step to undo. Typing is not one step per
/// keystroke: [`Self::record_coalesced`] folds a run of changes in one
/// group, made within a window of each other, into a single step.
///
/// Bind it to the standard commands (`CommandId::UNDO`, `CommandId::REDO`)
/// so the menu, the shortcut, and the toolbar all reach it:
///
/// ```
/// use std::time::Duration;
///
/// use rustnative_data::History;
///
/// let mut text = History::new(String::new());
/// let window = Duration::from_millis(500);
/// text.record_coalesced("h".to_owned(), "typing", Duration::from_millis(0), window);
/// text.record_coalesced("hi".to_owned(), "typing", Duration::from_millis(100), window);
/// text.record("hi!".to_owned());
///
/// assert!(text.undo());
/// assert_eq!(text.present(), "hi");
/// assert!(text.undo(), "the typing was one step");
/// assert_eq!(text.present(), "");
/// assert!(!text.undo());
/// assert!(text.redo());
/// assert_eq!(text.present(), "hi");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct History<T> {
    past: Vec<T>,
    present: T,
    future: Vec<T>,
    limit: usize,
    group: Option<(String, Duration)>,
}

impl<T: Clone> History<T> {
    /// A history starting at `present`, keeping up to 100 steps.
    pub fn new(present: T) -> Self {
        Self { past: Vec::new(), present, future: Vec::new(), limit: 100, group: None }
    }

    /// Keeps at most `limit` steps to undo; older ones are forgotten.
    #[must_use]
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit.max(1);
        self
    }

    /// The current value.
    pub fn present(&self) -> &T {
        &self.present
    }

    /// Makes `value` current, as one step to undo. Anything undone is
    /// forgotten.
    pub fn record(&mut self, value: T) {
        self.group = None;
        self.push(value);
    }

    /// Makes `value` current, folded into the previous step when that was in
    /// the same `group` less than `window` before `now`.
    pub fn record_coalesced(&mut self, value: T, group: &str, now: Duration, window: Duration) {
        let joins = self
            .group
            .as_ref()
            .is_some_and(|(last, at)| last == group && now.saturating_sub(*at) < window);
        self.group = Some((group.to_owned(), now));
        if joins {
            self.present = value;
            self.future.clear();
        } else {
            self.push(value);
        }
    }

    fn push(&mut self, value: T) {
        let previous = std::mem::replace(&mut self.present, value);
        self.past.push(previous);
        if self.past.len() > self.limit {
            self.past.remove(0);
        }
        self.future.clear();
    }

    /// Returns to the previous step; `false` when there is none.
    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.past.pop() else { return false };
        self.group = None;
        self.future.push(std::mem::replace(&mut self.present, previous));
        true
    }

    /// Re-applies the step last undone; `false` when there is none.
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.future.pop() else { return false };
        self.group = None;
        self.past.push(std::mem::replace(&mut self.present, next));
        true
    }

    /// Whether there is a step to undo — the Undo command's enabled state.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    /// Whether there is a step to redo.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }
}
