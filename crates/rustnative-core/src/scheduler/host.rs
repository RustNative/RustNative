//! An executor for hosts that own their loop and have no thread pool: a
//! browser page running a WebAssembly subtree, a sandboxed request on an
//! edge runtime, a server rendering one request on one thread.
//!
//! [`super::TokioExecutor`] needs threads to run on, which is exactly what
//! those hosts do not have (or must not reach for — a per-request host that
//! lazily creates a process-wide runtime pays for it on every cold start).
//! [`HostExecutor`] runs nothing by itself: the host drives it, through
//! [`crate::ComponentTree::pump_tasks`] (which calls
//! [`super::Executor::run_ready`]) whenever it is woken, and it measures
//! time only through the [`Clock`] the host gives it — so a target whose
//! standard library has no clock (`wasm32-unknown-unknown`) never reaches
//! one.
//!
//! A task's waker and a delay that needs a timer both call the host's wake
//! callback: a browser host answers the first with a microtask and the
//! second with `setTimeout` for [`HostExecutor::next_deadline`]; a server
//! host blocks its request thread until either happens.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use super::executor::{BoxedSleep, BoxedTask, Executor, ExecutorHandle};
use crate::clock::Clock;

type Wake_ = Arc<dyn Fn() + Send + Sync>;

struct Entry {
    future: Mutex<Option<BoxedTask>>,
    ready: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    aborted: Arc<AtomicBool>,
}

struct Handle {
    finished: Arc<AtomicBool>,
    aborted: Arc<AtomicBool>,
    wake: Option<Wake_>,
}

impl ExecutorHandle for Handle {
    fn abort(&self) {
        self.aborted.store(true, Ordering::Release);
        if let Some(wake) = &self.wake {
            wake();
        }
    }
    fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire) || self.aborted.load(Ordering::Acquire)
    }
}

struct TaskWaker {
    ready: Arc<AtomicBool>,
    wake: Option<Wake_>,
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.ready.store(true, Ordering::Release);
        if let Some(wake) = &self.wake {
            wake();
        }
    }
}

struct Inner {
    clock: Arc<dyn Clock>,
    tasks: Mutex<Vec<Arc<Entry>>>,
    timers: Mutex<Vec<(Duration, Waker)>>,
    wake: Mutex<Option<Wake_>>,
}

/// A single-threaded executor driven by its host, for hosts with no thread
/// pool (a browser's WebAssembly subtree, a sandboxed request): it runs
/// nothing by itself — [`crate::ComponentTree::pump_tasks`] polls it through
/// [`Executor::run_ready`] — and measures time only through the [`Clock`] it
/// is given, so a target with no system clock never reaches one. Its wake
/// callback ([`Self::set_wake`]) tells the host when to pump: soon for a
/// ready task, at [`Self::next_deadline`] for a delay.
///
/// # Example
///
/// ```
/// use std::sync::Arc;
/// use std::sync::atomic::{AtomicBool, Ordering};
/// use std::time::Duration;
///
/// use rustnative_core::{Executor, HostExecutor, ManualClock};
///
/// let clock = ManualClock::new();
/// let executor = HostExecutor::new(Arc::new(clock.clone()));
/// let done = Arc::new(AtomicBool::new(false));
/// let flag = Arc::clone(&done);
/// let delay = executor.sleep(Duration::from_millis(40));
/// executor.spawn(Box::pin(async move {
///     delay.await;
///     flag.store(true, Ordering::SeqCst);
/// }));
///
/// executor.run_ready();
/// assert_eq!(executor.next_deadline(), Some(Duration::from_millis(40)));
/// clock.advance(Duration::from_millis(40));
/// executor.run_ready();
/// assert!(done.load(Ordering::SeqCst));
/// ```
#[derive(Clone)]
pub struct HostExecutor {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for HostExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostExecutor").field("pending", &self.pending_task_count()).finish()
    }
}

impl HostExecutor {
    /// An executor measuring time with `clock`.
    #[must_use]
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            inner: Arc::new(Inner {
                clock,
                tasks: Mutex::new(Vec::new()),
                timers: Mutex::new(Vec::new()),
                wake: Mutex::new(None),
            }),
        }
    }

    /// Calls `wake` whenever a task becomes ready to poll or a delay is
    /// armed, so the host knows to call [`Executor::run_ready`] (soon, or at
    /// [`Self::next_deadline`]).
    pub fn set_wake(&self, wake: Arc<dyn Fn() + Send + Sync>) {
        *self.inner.wake.lock().unwrap_or_else(PoisonError::into_inner) = Some(wake);
    }

    fn wake(&self) -> Option<Wake_> {
        self.inner.wake.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// The earliest time, by this executor's clock, at which an armed delay
    /// is due — what a host sets its timer for — or `None` if no delay is
    /// armed.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Duration> {
        self.inner
            .timers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(deadline, _)| *deadline)
            .min()
    }

    /// Drops every task and armed delay: the host is done with them (a
    /// request has been answered, a page closed). A task's future is
    /// dropped here even when it holds this executor (a delay it awaits
    /// does), which would otherwise keep it alive forever.
    pub fn shutdown(&self) {
        let tasks =
            std::mem::take(&mut *self.inner.tasks.lock().unwrap_or_else(PoisonError::into_inner));
        let timers =
            std::mem::take(&mut *self.inner.timers.lock().unwrap_or_else(PoisonError::into_inner));
        // Dropped with no lock held: a future's drop may reach the executor.
        for entry in &tasks {
            entry.aborted.store(true, Ordering::Release);
            let future = entry.future.lock().unwrap_or_else(PoisonError::into_inner).take();
            drop(future);
        }
        drop(timers);
    }

    /// Tasks spawned and not yet finished or aborted.
    #[must_use]
    pub fn pending_task_count(&self) -> usize {
        self.inner.tasks.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    /// Whether any task is ready to be polled now.
    #[must_use]
    pub fn has_ready_work(&self) -> bool {
        let now = self.inner.clock.now();
        let timers_due = self
            .inner
            .timers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|(deadline, _)| *deadline <= now);
        timers_due
            || self
                .inner
                .tasks
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .any(|entry| entry.ready.load(Ordering::Acquire))
    }

    fn fire_due_timers(&self) {
        let now = self.inner.clock.now();
        let mut due = Vec::new();
        self.inner.timers.lock().unwrap_or_else(PoisonError::into_inner).retain(
            |(deadline, waker)| {
                if *deadline <= now {
                    due.push(waker.clone());
                    false
                } else {
                    true
                }
            },
        );
        for waker in due {
            waker.wake();
        }
    }

    fn poll_ready_tasks(&self) {
        let wake = self.wake();
        loop {
            let entries: Vec<Arc<Entry>> =
                self.inner.tasks.lock().unwrap_or_else(PoisonError::into_inner).clone();
            let mut progressed = false;
            for entry in &entries {
                if entry.finished.load(Ordering::Acquire) {
                    continue;
                }
                if entry.aborted.load(Ordering::Acquire) {
                    entry.finished.store(true, Ordering::Release);
                    *entry.future.lock().unwrap_or_else(PoisonError::into_inner) = None;
                    progressed = true;
                    continue;
                }
                if !entry.ready.swap(false, Ordering::AcqRel) {
                    continue;
                }
                let taken = entry.future.lock().unwrap_or_else(PoisonError::into_inner).take();
                let Some(mut future) = taken else { continue };
                let waker = Waker::from(Arc::new(TaskWaker {
                    ready: Arc::clone(&entry.ready),
                    wake: wake.clone(),
                }));
                let mut cx = Context::from_waker(&waker);
                match future.as_mut().poll(&mut cx) {
                    Poll::Ready(()) => entry.finished.store(true, Ordering::Release),
                    Poll::Pending => {
                        *entry.future.lock().unwrap_or_else(PoisonError::into_inner) = Some(future);
                    }
                }
                progressed = true;
            }
            self.inner
                .tasks
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retain(|entry| !entry.finished.load(Ordering::Acquire));
            if !progressed {
                break;
            }
        }
    }
}

impl Executor for HostExecutor {
    fn spawn(&self, future: BoxedTask) -> Box<dyn ExecutorHandle> {
        let finished = Arc::new(AtomicBool::new(false));
        let aborted = Arc::new(AtomicBool::new(false));
        self.inner.tasks.lock().unwrap_or_else(PoisonError::into_inner).push(Arc::new(Entry {
            future: Mutex::new(Some(future)),
            ready: Arc::new(AtomicBool::new(true)),
            finished: Arc::clone(&finished),
            aborted: Arc::clone(&aborted),
        }));
        let wake = self.wake();
        if let Some(wake) = &wake {
            wake();
        }
        Box::new(Handle { finished, aborted, wake })
    }

    fn sleep(&self, duration: Duration) -> BoxedSleep {
        Box::pin(HostSleep { executor: self.clone(), duration, deadline: None })
    }

    fn now(&self) -> Duration {
        self.inner.clock.now()
    }

    fn run_ready(&self) {
        // A timer firing wakes a task; a task finishing may arm another
        // timer that is already due (a zero delay). Settle both.
        loop {
            self.fire_due_timers();
            self.poll_ready_tasks();
            if !self.has_ready_work() {
                break;
            }
        }
    }
}

struct HostSleep {
    executor: HostExecutor,
    duration: Duration,
    deadline: Option<Duration>,
}

impl Future for HostSleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let now = self.executor.inner.clock.now();
        let duration = self.duration;
        let deadline = *self.deadline.get_or_insert_with(|| now.saturating_add(duration));
        if now >= deadline {
            return Poll::Ready(());
        }
        self.executor
            .inner
            .timers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((deadline, cx.waker().clone()));
        if let Some(wake) = self.executor.wake() {
            wake();
        }
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn nothing_runs_until_the_host_asks() {
        let executor = HostExecutor::new(Arc::new(ManualClock::new()));
        let runs = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&runs);
        executor.spawn(Box::pin(async move {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        assert_eq!(runs.load(Ordering::SeqCst), 0);
        executor.run_ready();
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(executor.pending_task_count(), 0);
    }

    #[test]
    fn spawning_and_arming_a_delay_wake_the_host() {
        let executor = HostExecutor::new(Arc::new(ManualClock::new()));
        let wakes = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&wakes);
        executor.set_wake(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        let delay = executor.sleep(Duration::from_secs(1));
        executor.spawn(Box::pin(delay));
        assert_eq!(wakes.load(Ordering::SeqCst), 1, "the spawn");
        executor.run_ready();
        assert_eq!(wakes.load(Ordering::SeqCst), 2, "the armed delay");
        assert_eq!(executor.next_deadline(), Some(Duration::from_secs(1)));
    }

    #[test]
    fn shutdown_drops_a_task_that_holds_the_executor() {
        struct Flag(Arc<AtomicBool>);
        impl Drop for Flag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let executor = HostExecutor::new(Arc::new(ManualClock::new()));
        let dropped = Arc::new(AtomicBool::new(false));
        let flag = Flag(Arc::clone(&dropped));
        let delay = executor.sleep(Duration::from_secs(3600));
        executor.spawn(Box::pin(async move {
            delay.await;
            drop(flag);
        }));
        executor.run_ready();
        assert!(!dropped.load(Ordering::SeqCst));
        executor.shutdown();
        assert!(dropped.load(Ordering::SeqCst));
        assert_eq!(executor.pending_task_count(), 0);
        assert_eq!(executor.next_deadline(), None);
    }

    #[test]
    fn an_aborted_task_never_runs() {
        let executor = HostExecutor::new(Arc::new(ManualClock::new()));
        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        let handle = executor.spawn(Box::pin(async move { flag.store(true, Ordering::SeqCst) }));
        handle.abort();
        executor.run_ready();
        assert!(!ran.load(Ordering::SeqCst));
        assert!(handle.is_finished());
    }
}
