//! The tree's side of Milestone 47: containing a failed subtree at its
//! boundary, store selections, and values provided to a subtree by type.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;

use super::{ComponentTree, RenderCause};
use crate::component::boundary::{BoundaryState, Failure};
use crate::component::context::AnimationRequest;
use crate::identity::ComponentId;
use crate::scheduler::{SupervisionPolicy, panic_message};
use crate::state::{Selection, Store};

impl ComponentTree {
    pub(crate) fn register_boundary(&mut self, id: ComponentId, state: Rc<RefCell<BoundaryState>>) {
        self.boundaries.insert(id, state);
    }

    fn is_descendant(&self, id: ComponentId, ancestor: ComponentId) -> bool {
        let mut current = self.parents.get(&id).copied();
        while let Some(parent) = current {
            if parent == ancestor {
                return true;
            }
            current = self.parents.get(&parent).copied();
        }
        false
    }

    /// Removes every component below `boundary`, records the failure on
    /// it, and returns its policy.
    pub(crate) fn contain(&mut self, boundary: ComponentId, message: String) -> SupervisionPolicy {
        let doomed = self
            .parents
            .keys()
            .copied()
            .filter(|id| self.is_descendant(*id, boundary))
            .collect::<Vec<_>>();
        for id in doomed {
            // A component that was rendering when the panic unwound is not
            // in the map any more; its entry, and with it its task scope,
            // was dropped by the unwind.
            if let Some(mut entry) = self.components.remove(&id) {
                entry.task_scope.cancel_all();
                Self::dispose_effects(&mut entry);
                let _ = catch_unwind(AssertUnwindSafe(|| entry.component.unmounted()));
            }
            self.paths.remove(&id);
            self.forget(id);
            self.pending_children.remove(&id);
            self.pending_effects.remove(&id);
            self.animation_requests.borrow_mut().push_back(AnimationRequest::CancelOwner(id));
        }
        if let Some(entry) = self.components.get_mut(&boundary) {
            entry.children.clear();
        }
        if let Some(children) = self.pending_children.get_mut(&boundary) {
            children.clear();
        }
        // The panic hook recorded this panic for a development run's error
        // dialog; it was contained, so there is nothing to show.
        let _ = crate::dev::take_panic();

        let component = self.paths.get(&boundary).cloned().unwrap_or_default();
        let Some(state) = self.boundaries.get(&boundary).cloned() else {
            return SupervisionPolicy::Escalate;
        };
        let mut state = state.borrow_mut();
        state.failures = state.failures.saturating_add(1);
        state.retry = state.policy.restart_delay(state.failures);
        let failure = Failure { component, message, attempt: state.failures };
        eprintln!(
            "rustnative-core: contained a failure in {}: {}",
            failure.component, failure.message
        );
        state.failure = Some(failure.clone());
        // Kept for whoever reads them, but bounded: an application that
        // never asks must not accumulate every failure of a long run.
        if self.failures.len() == 64 {
            self.failures.remove(0);
        }
        self.failures.push(failure);
        state.policy
    }

    /// Contains a panic raised by component `id` outside rendering, at the
    /// nearest boundary above it that contains failures; with none, the
    /// panic continues to the application's panic policy.
    pub(crate) fn fail(&mut self, id: ComponentId, payload: Box<dyn Any + Send>) {
        let mut current = self.parents.get(&id).copied();
        while let Some(ancestor) = current {
            let contains = self
                .boundaries
                .get(&ancestor)
                .is_some_and(|state| state.borrow().policy != SupervisionPolicy::Escalate);
            if contains {
                self.contain(ancestor, panic_message(&*payload));
                self.mark_dirty(ancestor, RenderCause::Failure);
                return;
            }
            current = self.parents.get(&ancestor).copied();
        }
        resume_unwind(payload);
    }

    /// The failures contained since the last call, oldest first.
    pub fn take_failures(&mut self) -> Vec<Failure> {
        std::mem::take(&mut self.failures)
    }

    pub(crate) fn record_selection(&mut self, id: ComponentId, selection: Selection) {
        self.selections.entry(id).or_default().push(selection);
    }

    /// Marks every component whose selected slice of a store changed.
    pub(super) fn mark_stale_selections(&mut self) {
        let epoch = crate::state::epoch();
        if epoch == self.seen_epoch {
            return;
        }
        self.seen_epoch = epoch;
        let stale = self
            .selections
            .iter()
            .filter(|(_, selections)| selections.iter().any(Selection::is_stale))
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in stale {
            if self.components.contains_key(&id) {
                self.mark_dirty(id, RenderCause::Store);
            }
        }
    }

    /// Whether a store changed since the last render pass looked.
    pub(super) fn stores_changed(&self) -> bool {
        crate::state::epoch() != self.seen_epoch
    }

    pub(crate) fn provide_scoped<T: Clone + 'static>(&mut self, id: ComponentId, value: T) {
        self.scoped.entry(id).or_default().insert(TypeId::of::<T>(), Rc::new(value));
    }

    pub(crate) fn provide_scoped_with<T: Clone + 'static>(
        &mut self,
        id: ComponentId,
        make: impl FnOnce() -> T,
    ) -> T {
        let values = self.scoped.entry(id).or_default();
        let value =
            values.entry(TypeId::of::<T>()).or_insert_with(|| Rc::new(make()) as Rc<dyn Any>);
        value.downcast_ref::<T>().cloned().unwrap_or_else(|| unreachable!("keyed by its type"))
    }

    pub(crate) fn scoped<T: Clone + 'static>(&self, id: ComponentId) -> Option<T> {
        let mut current = Some(id);
        while let Some(owner) = current {
            if let Some(value) =
                self.scoped.get(&owner).and_then(|values| values.get(&TypeId::of::<T>()))
            {
                return value.downcast_ref::<T>().cloned();
            }
            current = self.parents.get(&owner).copied();
        }
        None
    }
}

impl<M: Send + 'static> crate::ComponentContext<'_, M> {
    /// Reads a slice of `store`, chosen by `select`, and re-renders this
    /// component when — and only when — that slice changes by value (see
    /// [`crate::state`]).
    pub fn select<T: 'static, R>(
        &mut self,
        store: &Store<T>,
        select: impl Fn(&T) -> R + 'static,
    ) -> R
    where
        R: PartialEq + Clone + 'static,
    {
        let value = store.read(&select);
        self.tree.record_selection(self.parent, Selection::new(store, value.clone(), select));
        value
    }

    /// Provides `value` to this component's descendants, found by its type
    /// with [`Self::scoped`]. Providing again replaces it.
    ///
    /// This is how a store, or a service with a narrower lifetime than the
    /// application (`C16`), is scoped to a subtree: a window's root
    /// provides window-scoped services; a navigation destination's screen
    /// provides destination-scoped ones. The value is dropped when this
    /// component unmounts.
    pub fn provide_scoped<T: Clone + 'static>(&mut self, value: T) {
        self.tree.provide_scoped(self.parent, value);
    }

    /// Provides the value `make` constructs — once, on this component's
    /// first render — and returns it; later renders return the same value.
    pub fn provide_scoped_with<T: Clone + 'static>(&mut self, make: impl FnOnce() -> T) -> T {
        self.tree.provide_scoped_with(self.parent, make)
    }

    /// The nearest value of type `T` provided by this component or an
    /// ancestor.
    #[must_use]
    pub fn scoped<T: Clone + 'static>(&self) -> Option<T> {
        self.tree.scoped(self.parent)
    }

    /// This component's identity — stable for as long as it stays mounted,
    /// which is what a data layer keys its observers by.
    #[must_use]
    pub fn component_id(&self) -> ComponentId {
        self.parent
    }

    /// A handle for message-less background work; see
    /// [`crate::Background`].
    #[must_use]
    pub fn background(&self) -> crate::Background {
        self.task_scope.background()
    }

    /// Delivers every item of `stream`, mapped to a message, for as long as
    /// this component is mounted (`C12`; see [`crate::scheduler::supervise`]).
    pub fn collect<S>(
        &self,
        stream: S,
        map: impl Fn(S::Item) -> M + Send + 'static,
    ) -> crate::TaskHandle
    where
        S: futures_core::Stream + Send + 'static,
    {
        self.task_scope.collect(stream, map)
    }

    /// Computes `prepare` off the UI thread and delivers its result as one
    /// message (`C09-2`).
    pub fn prepare(&self, prepare: impl FnOnce() -> M + Send + 'static) -> crate::TaskHandle {
        self.task_scope.prepare(prepare)
    }
}
