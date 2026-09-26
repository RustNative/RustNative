//! Suspendable task scopes and update priorities (`PLAN.md` Milestone 54,
//! `C01`–`C03`).
//!
//! # Suspension
//!
//! A component whose subtree is hidden — a screen under another on a
//! navigation stack, a non-selected tab, a collapsed panel, a minimized
//! window — keeps its state but should not keep working. The framework
//! suspends its task scope, and the scopes of its effects, while it is
//! hidden, and resumes them when it is shown. What that means for a task is
//! stated when it is spawned, by its [`SuspendRule`]:
//!
//! - [`SuspendRule::Complete`] (the default for [`TaskScope::spawn`]): runs
//!   on, and its result is delivered, hidden or not — right for a request
//!   whose answer the screen needs when it comes back.
//! - [`SuspendRule::Cancel`]: cancelled when the scope is suspended — right
//!   for work only worth doing while seen.
//! - [`SuspendRule::Defer`]: paused — not polled at all — while suspended,
//!   and continued on resume. A periodic task (a clock, a poll) spawned
//!   with this rule does no work while its screen is hidden.
//!
//! # Priorities
//!
//! A message carries a [`Priority`]. Input feedback is
//! [`Priority::Immediate`]; ordinary messages are [`Priority::Normal`];
//! [`Priority::Deferrable`] ones — the results of expensive work a person
//! is not waiting on keystroke by keystroke — are delivered only after
//! everything more urgent, a budgeted slice at a time
//! (`ComponentTree::pump_deferred`), which a host runs only while no input
//! waits — so they never delay the response to input, and each slice
//! commits whole.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Waker};

use parking_lot::Mutex;

use super::{TaskHandle, TaskScope};

/// What happens to a task while its scope is suspended; see the [module
/// documentation](crate::scheduler::suspend).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SuspendRule {
    /// It runs on and its result is delivered.
    #[default]
    Complete,
    /// It is cancelled.
    Cancel,
    /// It is paused, and continues on resume.
    Defer,
}

/// How urgently a message must be delivered; see the [module
/// documentation](crate::scheduler::suspend).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub enum Priority {
    /// Input feedback: delivered before anything else.
    Immediate,
    /// Delivered in order with other ordinary messages.
    #[default]
    Normal,
    /// Delivered only when nothing more urgent is waiting, a slice at a
    /// time.
    Deferrable,
}

/// The suspension state a component's scope shares with its effects'.
#[derive(Default)]
pub(crate) struct Suspension {
    suspended: AtomicBool,
    parked: Mutex<Vec<Waker>>,
    cancel_on_suspend: Mutex<Vec<TaskHandle>>,
}

impl Suspension {
    fn suspend(&self) {
        {
            let _parked = self.parked.lock();
            self.suspended.store(true, Ordering::Release);
        }
        for task in self.cancel_on_suspend.lock().drain(..) {
            task.cancel();
        }
    }

    fn resume(&self) {
        let wakers = {
            let mut parked = self.parked.lock();
            self.suspended.store(false, Ordering::Release);
            std::mem::take(&mut *parked)
        };
        for waker in wakers {
            waker.wake();
        }
    }

    fn is_suspended(&self) -> bool {
        self.suspended.load(Ordering::Acquire)
    }
}

/// A future that is not polled while its scope is suspended.
pub(super) struct Gated<F> {
    pub(super) future: Pin<Box<F>>,
    pub(super) suspension: Arc<Suspension>,
}

impl<F: Future> Future for Gated<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        {
            // Checked and parked under one lock, so a resume between the two
            // cannot be missed.
            let mut parked = self.suspension.parked.lock();
            if self.suspension.suspended.load(Ordering::Acquire) {
                parked.push(cx.waker().clone());
                return Poll::Pending;
            }
        }
        self.future.as_mut().poll(cx)
    }
}

impl TaskScope {
    pub(crate) fn suspension(&self) -> &Arc<Suspension> {
        &self.inner.suspension
    }

    /// Suspends the scope: [`SuspendRule::Cancel`] tasks are cancelled and
    /// [`SuspendRule::Defer`] tasks pause.
    pub fn suspend(&self) {
        self.inner.suspension.suspend();
    }

    /// Resumes the scope: paused tasks continue.
    pub fn resume(&self) {
        self.inner.suspension.resume();
    }

    /// Whether the scope is suspended.
    #[must_use]
    pub fn is_suspended(&self) -> bool {
        self.inner.suspension.is_suspended()
    }

    /// Spawns `future` under `rule`; see [`SuspendRule`].
    pub fn spawn_with<M, F>(&self, rule: SuspendRule, future: F) -> TaskHandle
    where
        M: Send + 'static,
        F: Future<Output = M> + Send + 'static,
    {
        match rule {
            SuspendRule::Complete => self.spawn(future),
            SuspendRule::Defer => self.spawn(Gated {
                future: Box::pin(future),
                suspension: Arc::clone(&self.inner.suspension),
            }),
            SuspendRule::Cancel => {
                let handle = self.spawn(future);
                let mut cancellable = self.inner.suspension.cancel_on_suspend.lock();
                cancellable.retain(|task| !task.is_finished() && !task.is_cancelled());
                cancellable.push(handle.clone());
                if self.is_suspended() {
                    handle.cancel();
                }
                handle
            }
        }
    }
}
