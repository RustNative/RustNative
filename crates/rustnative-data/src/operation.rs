//! Long-running operations (`PLAN.md` Milestone 47, `C87`): a goal, a
//! stream of progress, cancellation, pre-emption by a newer goal, and a
//! result — observable by any component, and shown by the standard
//! progress components (Milestone 48).

use std::cell::RefCell;
use std::fmt;
use std::future::Future;
use std::rc::Rc;

use rustnative_core::{Background, Store, TaskHandle};

/// Where an operation is.
#[derive(Debug, Clone, PartialEq)]
pub enum OperationState<P, R> {
    /// Not started.
    Idle,
    /// Working toward `goal`; `progress` is the latest report.
    Running {
        /// What it is doing.
        goal: String,
        /// How far it has got.
        progress: Option<P>,
    },
    /// Finished with a result.
    Succeeded(R),
    /// Finished without one.
    Failed(String),
    /// Stopped before finishing.
    Cancelled,
}

/// Reports an operation's progress.
pub struct Progress<P: 'static, R: 'static> {
    store: Store<OperationState<P, R>>,
}

impl<P, R> fmt::Debug for Progress<P, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Progress").finish_non_exhaustive()
    }
}

impl<P: 'static, R: 'static> Progress<P, R> {
    /// Reports `progress`.
    pub fn report(&self, progress: P) {
        self.store.update(move |state| {
            if let OperationState::Running { progress: current, .. } = state {
                *current = Some(progress);
            }
        });
    }
}

/// A long-running operation; see the [module documentation](self).
///
/// Starting a new goal while one runs pre-empts it: the old work is
/// cancelled and the new one starts.
pub struct Operation<P: 'static, R: 'static> {
    store: Store<OperationState<P, R>>,
    task: Rc<RefCell<Option<TaskHandle>>>,
    background: Background,
}

impl<P, R> Clone for Operation<P, R> {
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
            task: Rc::clone(&self.task),
            background: self.background.clone(),
        }
    }
}

impl<P, R> fmt::Debug for Operation<P, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Operation").field("store", &self.store).finish_non_exhaustive()
    }
}

impl<P: 'static, R: 'static> Operation<P, R> {
    /// An idle operation whose work runs on `background`.
    pub fn new(name: &str, background: Background) -> Self {
        Self {
            store: Store::new(name, OperationState::Idle),
            task: Rc::new(RefCell::new(None)),
            background,
        }
    }

    /// Its state, for [`rustnative_core::ComponentContext::select`].
    #[must_use]
    pub fn store(&self) -> &Store<OperationState<P, R>> {
        &self.store
    }

    /// Starts working toward `goal`, pre-empting any running work. `work`
    /// reports progress through the [`Progress`] it is given.
    pub fn start<Fut>(&self, goal: impl Into<String>, work: impl FnOnce(Progress<P, R>) -> Fut)
    where
        Fut: Future<Output = Result<R, String>> + 'static,
    {
        if let Some(previous) = self.task.borrow_mut().take() {
            previous.cancel();
        }
        let goal = goal.into();
        self.store.set(OperationState::Running { goal, progress: None });
        let running = work(Progress { store: self.store.clone() });
        let store = self.store.clone();
        let task = Rc::clone(&self.task);
        let handle = self.background.spawn_local(async move {
            let outcome = running.await;
            task.borrow_mut().take();
            store.set(match outcome {
                Ok(result) => OperationState::Succeeded(result),
                Err(message) => OperationState::Failed(message),
            });
        });
        *self.task.borrow_mut() = Some(handle);
    }

    /// Stops the running work, if any.
    pub fn cancel(&self) {
        if let Some(task) = self.task.borrow_mut().take() {
            task.cancel();
            self.store.set(OperationState::Cancelled);
        }
    }
}
