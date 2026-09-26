//! The tree's side of Milestone 54: deferrable messages delivered in time
//! slices, suspension of hidden subtrees, deferred values, and the report
//! of components whose props can never be skipped.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::{ComponentTree, RenderCause};
use crate::identity::{ComponentId, NodeId};
use crate::scheduler::TaskHandle;
use crate::state::Store;

impl ComponentTree {
    /// How long one slice of deferrable work may take before the tree
    /// yields to the host (default: 4 ms, a quarter of a 60 Hz frame).
    pub fn set_render_budget(&mut self, budget: Duration) {
        self.render_budget = budget;
    }

    /// Whether deferrable messages are waiting. A host that sees this after
    /// a pump schedules another pump soon, rather than waiting for input.
    #[must_use]
    pub fn has_deferred_work(&self) -> bool {
        !self.deferred_messages.is_empty()
    }

    /// Delivers deferrable messages until the render budget is spent.
    /// Returns whether any was delivered.
    pub(super) fn deliver_deferred_slice(&mut self) -> bool {
        if self.deferred_messages.is_empty() {
            return false;
        }
        let started = Instant::now();
        let mut delivered = false;
        while let Some(queued) = self.deferred_messages.pop_front() {
            let target = queued.target;
            let Some(mut entry) = self.components.remove(&target) else { continue };
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let accepted = entry.component.message_any(queued.message);
                if accepted {
                    entry.component.updated();
                }
                accepted
            }));
            self.components.insert(target, entry);
            match outcome {
                Ok(true) => {
                    self.mark_dirty(target, RenderCause::Message);
                    delivered = true;
                }
                Ok(false) => {}
                Err(payload) => {
                    self.fail(target, payload);
                    delivered = true;
                }
            }
            if started.elapsed() >= self.render_budget {
                break;
            }
        }
        delivered
    }

    /// Delivers one slice of deferrable messages — as many as fit the
    /// render budget — and renders. Returns whether anything changed.
    ///
    /// A host calls this when no input is waiting, and again (after
    /// handling any input that arrived) while [`Self::has_deferred_work`]:
    /// so deferrable work never delays the response to input, and each
    /// slice commits whole — a frame never shows half of one.
    pub fn pump_deferred(&mut self) -> bool {
        if !self.deliver_deferred_slice() {
            return false;
        }
        let _ = self.render_pass(false);
        true
    }

    /// Marks the whole window as in the background (minimized) or not:
    /// every component's scope is suspended while it is.
    pub fn set_backgrounded(&mut self, backgrounded: bool) {
        if self.backgrounded != backgrounded {
            self.backgrounded = backgrounded;
            self.update_visibility();
        }
    }

    /// Suspends the task scopes of components whose output is hidden, and
    /// resumes those shown again (see [`crate::scheduler::suspend`]).
    pub(super) fn update_visibility(&mut self) {
        let mut hidden_nodes: HashMap<NodeId, bool> = HashMap::new();
        let mut shown: HashSet<ComponentId> = HashSet::new();
        let mut authored: HashSet<ComponentId> = HashSet::new();
        if let Some(root) = &self.root_view {
            root.visit(&mut |node, parent, _| {
                let hidden = node.is_hidden()
                    || parent.and_then(|p| hidden_nodes.get(&p).copied()).unwrap_or(false);
                hidden_nodes.insert(node.id(), hidden);
                if let Some((owner, _)) = self.node_owners.get(&node.id()) {
                    authored.insert(*owner);
                    if !hidden {
                        shown.insert(*owner);
                    }
                }
            });
        }
        // A component that authors no node of its own shares its parent's
        // visibility.
        let ids = self.components.keys().copied().collect::<Vec<_>>();
        let visible = |id: ComponentId| {
            let mut current = id;
            loop {
                if authored.contains(&current) {
                    return shown.contains(&current);
                }
                match self.parents.get(&current) {
                    Some(parent) => current = *parent,
                    None => return true,
                }
            }
        };
        let hidden = ids
            .iter()
            .copied()
            .filter(|id| self.backgrounded || !visible(*id))
            .collect::<HashSet<_>>();
        for id in &ids {
            let Some(entry) = self.components.get(id) else { continue };
            let now_hidden = hidden.contains(id);
            if now_hidden != self.hidden.contains(id) {
                if now_hidden {
                    entry.task_scope.suspend();
                } else {
                    entry.task_scope.resume();
                }
            }
        }
        self.hidden = hidden;
    }

    /// The components whose tasks are suspended because they are hidden.
    #[must_use]
    pub fn suspended_components(&self) -> Vec<String> {
        let mut paths =
            self.hidden.iter().filter_map(|id| self.paths.get(id).cloned()).collect::<Vec<_>>();
        paths.sort();
        paths
    }

    /// The components whose props type compared unequal to its own clone —
    /// they re-render whenever their parent does, because their props can
    /// never be found equal (`C04-1`). Checked in debug builds.
    #[must_use]
    pub fn unskippable_components(&self) -> Vec<&'static str> {
        self.unskippable.iter().copied().collect()
    }
}

/// A value computed off the UI thread from an input, where the component
/// keeps showing the previous result while a newer one is prepared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deferred<T> {
    /// The most recent result, if any has been computed.
    pub current: Option<T>,
    /// Whether a newer result is being computed — what a component shows
    /// as "stale" or "updating".
    pub pending: bool,
}

struct DeferredSlot<I, O: 'static> {
    input: RefCell<Option<I>>,
    requested: Cell<u64>,
    result: Store<(Option<O>, u64)>,
    task: RefCell<Option<TaskHandle>>,
}

impl<I, O> Drop for DeferredSlot<I, O> {
    fn drop(&mut self) {
        if let Some(task) = self.task.borrow_mut().take() {
            task.cancel();
        }
    }
}

impl<M: Send + 'static> crate::ComponentContext<'_, M> {
    /// The result of `compute(input)`, computed off the UI thread and kept
    /// under `key` (`C02`).
    ///
    /// When `input` changes (by value), the computation for the previous
    /// input is cancelled — superseded work is discarded — and a new one
    /// starts. Until it finishes, [`Deferred::current`] is the previous
    /// result and [`Deferred::pending`] is `true`, so the component keeps
    /// showing content and can show that it is updating. The component
    /// re-renders when the new result arrives; an input change never waits
    /// for it, so typing stays as fast as the rest of the render.
    ///
    /// Choose an `O` that is cheap to clone (an `Arc` of a large result):
    /// the component receives a copy on every render.
    ///
    /// # Panics
    ///
    /// If `key` was used by this component for a deferred value of other
    /// types.
    pub fn deferred<I, O>(
        &mut self,
        key: &str,
        input: I,
        compute: impl FnOnce(I) -> O + Send + 'static,
    ) -> Deferred<O>
    where
        I: PartialEq + Clone + Send + 'static,
        O: Clone + PartialEq + Send + 'static,
    {
        let owner = self.parent;
        let slot = self
            .tree
            .deferreds
            .entry((owner, key.to_owned()))
            .or_insert_with(|| {
                Rc::new(DeferredSlot::<I, O> {
                    input: RefCell::new(None),
                    requested: Cell::new(0),
                    result: Store::new(format!("deferred {key}"), (None, 0)),
                    task: RefCell::new(None),
                }) as Rc<dyn Any>
            })
            .clone()
            .downcast::<DeferredSlot<I, O>>()
            .unwrap_or_else(|_| panic!("the deferred value {key:?} changed type"));
        let changed = slot.input.borrow().as_ref() != Some(&input);
        if changed {
            *slot.input.borrow_mut() = Some(input.clone());
            let generation = slot.requested.get() + 1;
            slot.requested.set(generation);
            if let Some(previous) = slot.task.borrow_mut().take() {
                previous.cancel();
            }
            let background = self.background();
            let runner = background.clone();
            let result = slot.result.clone();
            // Offloaded when the task first runs, not now: a computation
            // superseded before then never starts.
            let handle = background.spawn_local(async move {
                let output = runner.offload(async move { compute(input) }).await;
                result.set((Some(output), generation));
            });
            *slot.task.borrow_mut() = Some(handle);
        }
        let requested = slot.requested.get();
        let (current, done) =
            self.select(&slot.result, |(output, generation)| (output.clone(), *generation));
        Deferred { current, pending: done != requested }
    }
}
