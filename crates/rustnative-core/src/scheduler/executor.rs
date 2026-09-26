//! A pluggable async execution backend.
//!
//! The standards audit's P1.7 finding is that `rustnative-core` originally
//! hard-wired every [`super::Scheduler`] to one process-global,
//! `OnceLock`-backed, always-exactly-two-worker-thread Tokio runtime: every
//! `Application` in a process shared one executor whether or not that was
//! appropriate, tests shared global runtime state with each other and with
//! any real application in the same process, and there was no way for an
//! embedding host to supply its own executor.
//!
//! [`Executor`] is the seam that fixes this: [`super::Scheduler`] spawns
//! through a `dyn Executor` it is handed at construction rather than a
//! hard-coded free function. [`TokioExecutor`] is the default
//! implementation and preserves the exact previous behavior (via
//! [`TokioExecutor::shared`], the same process-wide runtime that always
//! existed — this is a seam being introduced, not a behavior change for
//! existing callers), but a host can now construct its own `TokioExecutor`
//! with a different worker-count policy (see [`TokioExecutor::dedicated`]),
//! or provide an entirely different [`Executor`] implementation.
//!
//! # Time is part of the executor contract (P1.18)
//!
//! An earlier version of this seam spawned tasks through [`Executor`] but
//! still read delays directly from a hard-coded Tokio timer, so a component
//! that awaited a delay could never be tested deterministically no matter
//! which executor its `Scheduler` used. [`Executor::sleep`] closes that
//! gap: [`super::Scheduler::sleep`] (and, in turn,
//! `ComponentContext::sleep`/`EffectContext::sleep`) delegate to *that*
//! scheduler's own executor, not a global runtime. [`ManualExecutor`] is
//! the deterministic backend this unlocks — see its docs — and is the
//! recommended executor for any test that spawns tasks or awaits delays and
//! wants to assert on their outcome without racing a real clock or a real
//! thread pool.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock, PoisonError};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

/// A boxed, type-erased unit-returning future — what [`Executor::spawn`]
/// accepts. Callers (see `Scheduler::spawn_with_settlement`) box their own
/// `Output = M` future together with the bookkeeping (pushing the result to
/// the completion queue, waking the host) into one `Output = ()` future
/// before handing it to an executor, so the executor abstraction itself
/// never needs to know about message types.
pub type BoxedTask = Pin<Box<dyn Future<Output = ()> + Send>>;

/// A boxed, type-erased delay future — what [`Executor::sleep`] returns.
pub type BoxedSleep = Pin<Box<dyn Future<Output = ()> + Send>>;

/// An opaque handle to one spawned task, letting its owner request
/// cancellation and observe completion without knowing which [`Executor`]
/// implementation is running it.
pub trait ExecutorHandle: Send + Sync {
    /// Requests that the task stop running as soon as possible.
    fn abort(&self);
    /// Returns whether the task has finished running or been aborted.
    fn is_finished(&self) -> bool;
}

/// A pluggable async execution backend.
///
/// Implementations must run a spawned future to completion (or abortion)
/// without blocking the caller of [`Self::spawn`], and must be safe to share
/// across every component/window in one [`super::Scheduler`]. Time
/// (`Self::sleep`) is part of the contract deliberately: a backend that
/// controls both is one whose tests can control both, which is the whole
/// point of [`ManualExecutor`].
pub trait Executor: Send + Sync + 'static {
    /// Runs `future` to completion (or abortion) without blocking the
    /// caller, returning a handle to observe/cancel it.
    fn spawn(&self, future: BoxedTask) -> Box<dyn ExecutorHandle>;

    /// Returns a future that resolves after (at least) `duration` has
    /// elapsed according to this executor's own notion of time. A real
    /// backend measures wall-clock time; [`ManualExecutor`] measures a
    /// virtual clock that only moves when explicitly advanced.
    fn sleep(&self, duration: Duration) -> BoxedSleep;

    /// The current time by this executor's own clock: the same clock
    /// [`Self::sleep`] measures. The default is the process's monotonic
    /// clock ([`crate::SystemClock`]); [`ManualExecutor`] reports its
    /// virtual time, so timestamps and delays move together in a test.
    fn now(&self) -> Duration {
        crate::clock::process_epoch().elapsed()
    }
}

struct TokioHandle {
    abort: tokio::task::AbortHandle,
}

impl ExecutorHandle for TokioHandle {
    fn abort(&self) {
        self.abort.abort();
    }
    fn is_finished(&self) -> bool {
        self.abort.is_finished()
    }
}

/// The default [`Executor`], backed by a Tokio multi-thread runtime.
#[derive(Clone)]
pub struct TokioExecutor {
    runtime: Arc<tokio::runtime::Runtime>,
}

impl TokioExecutor {
    /// Builds a new, independently owned Tokio runtime with `worker_threads`
    /// workers. Use this when a host wants an executor whose lifetime and
    /// resource usage is scoped to one `Application` (or one test) rather
    /// than shared process-wide — see [`Self::shared`] for the default the
    /// rest of this crate uses when nothing else is specified.
    ///
    /// # Panics
    ///
    /// Panics if the underlying Tokio runtime fails to start (for example,
    /// if the process cannot spawn threads), matching this crate's existing
    /// policy of treating executor startup failure as unrecoverable rather
    /// than something every `Scheduler::new()` call site should have to
    /// handle.
    #[must_use]
    #[allow(
        clippy::expect_used,
        reason = "documented in this method's own `# Panics` \n    /// section: a host that cannot start a thread pool has no working executor, and \n    /// returning a `Result` would push that unrecoverable case onto every call site"
    )]
    pub fn dedicated(worker_threads: usize) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(worker_threads.max(1))
            .thread_name("framework-async")
            .enable_time()
            .build()
            .expect("failed to start a dedicated framework async runtime");
        Self { runtime: Arc::new(runtime) }
    }

    /// Returns the process-wide default executor: a two-worker-thread Tokio
    /// runtime, created lazily on first use and shared by every
    /// [`super::Scheduler`] that does not request its own via
    /// [`super::Scheduler::with_executor`]. This is the same runtime shape
    /// `rustnative-core` has always defaulted to; what changed is that it is
    /// now an opt-in default behind [`Executor`] rather than the only
    /// executor `Scheduler` could ever use.
    pub fn shared() -> Arc<Self> {
        static SHARED: OnceLock<Arc<TokioExecutor>> = OnceLock::new();
        Arc::clone(SHARED.get_or_init(|| Arc::new(Self::dedicated(2))))
    }

    /// Grants access to the underlying `tokio::runtime::Runtime`. Used by
    /// this crate's synchronous test helper (`block_on_for_test`) to drive
    /// an `async fn` from a non-`async` `#[test]` function.
    #[cfg(test)]
    pub(super) fn runtime(&self) -> &tokio::runtime::Runtime {
        &self.runtime
    }
}

impl Executor for TokioExecutor {
    fn spawn(&self, future: BoxedTask) -> Box<dyn ExecutorHandle> {
        let join_handle = self.runtime.spawn(future);
        Box::new(TokioHandle { abort: join_handle.abort_handle() })
    }

    fn sleep(&self, duration: Duration) -> BoxedSleep {
        // `tokio::time::sleep` reads the ambient runtime from a thread-local
        // at construction time, so it must be built while a runtime context
        // is entered. `Runtime::enter` just sets that thread-local for the
        // duration of the closure; it does not block or run anything, and
        // the guard is dropped immediately after — the future itself is
        // polled later, by whichever thread eventually drives it (normally
        // one of this same runtime's own workers, which already have the
        // context entered).
        let _guard = self.runtime.enter();
        Box::pin(tokio::time::sleep(duration))
    }
}

// ---------------------------------------------------------------------
// ManualExecutor — deterministic, virtual-time backend for tests (P1.18)
// ---------------------------------------------------------------------

struct TaskEntry {
    future: StdMutex<Option<BoxedTask>>,
    ready: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    aborted: Arc<AtomicBool>,
}

struct ManualHandle {
    finished: Arc<AtomicBool>,
    aborted: Arc<AtomicBool>,
}

impl ExecutorHandle for ManualHandle {
    fn abort(&self) {
        self.aborted.store(true, Ordering::Release);
    }
    fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire) || self.aborted.load(Ordering::Acquire)
    }
}

struct ManualWaker(Arc<AtomicBool>);

impl Wake for ManualWaker {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::Release);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.store(true, Ordering::Release);
    }
}

struct ClockState {
    now: Duration,
    sleepers: Vec<(Duration, Waker)>,
}

struct ManualInner {
    tasks: StdMutex<Vec<Arc<TaskEntry>>>,
    clock: StdMutex<ClockState>,
}

impl Default for ManualInner {
    fn default() -> Self {
        Self {
            tasks: StdMutex::new(Vec::new()),
            clock: StdMutex::new(ClockState { now: Duration::ZERO, sleepers: Vec::new() }),
        }
    }
}

/// A deterministic, single-threaded, virtual-time [`Executor`] for tests.
///
/// Nothing runs implicitly: [`Self::spawn`]ed futures only make progress
/// when the test calls [`Self::run_until_stalled`], and [`Self::sleep`]
/// futures only resolve when the test calls [`Self::advance`] (which also
/// drives [`Self::run_until_stalled`] afterward) past their deadline. This
/// gives component/effect/task-scope tests exactly the two properties a
/// real thread pool and a real clock cannot: no flaky timing-dependent
/// sleeps in the test suite, and the ability to assert "nothing has
/// happened yet" as a real, checkable state rather than a race.
///
/// Finished and aborted tasks are pruned from the internal registry at the
/// end of every [`Self::run_until_stalled`] pass, so a long-running test
/// that spawns many short-lived tasks does not retain them forever — the
/// same bounded-registry property [`super::TaskScope`] itself provides for
/// production use (P1.5).
///
/// # Example
///
/// Testing time-dependent behavior without racing a real clock:
///
/// ```
/// use std::sync::Arc;
/// use std::sync::atomic::{AtomicBool, Ordering};
/// use std::time::Duration;
///
/// use rustnative_core::{Executor, ManualExecutor, Scheduler};
///
/// let executor = ManualExecutor::new();
/// let scheduler = Scheduler::with_executor(Arc::new(executor.clone()));
///
/// let fired = Arc::new(AtomicBool::new(false));
/// let flag = Arc::clone(&fired);
/// let delay = scheduler.sleep(Duration::from_secs(30));
/// executor.spawn(Box::pin(async move {
///     delay.await;
///     flag.store(true, Ordering::SeqCst);
/// }));
///
/// // Nothing runs until asked to.
/// executor.run_until_stalled();
/// assert!(!fired.load(Ordering::SeqCst), "the delay has not elapsed yet");
///
/// // Time only moves when the test moves it, so this is deterministic
/// // rather than a 30-second wait or a hopeful sleep.
/// executor.advance(Duration::from_secs(30));
/// executor.run_until_stalled();
/// assert!(fired.load(Ordering::SeqCst));
/// ```
#[derive(Clone, Default)]
pub struct ManualExecutor {
    inner: Arc<ManualInner>,
}

impl ManualExecutor {
    /// Creates an executor whose virtual clock starts at zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The executor's current virtual time, starting at zero and moving
    /// only when [`Self::advance`] is called.
    #[must_use]
    pub fn now(&self) -> Duration {
        self.inner.clock.lock().unwrap_or_else(PoisonError::into_inner).now
    }

    /// The number of tasks still tracked (not yet finished or aborted).
    /// Exposed so tests can assert a task scope leaves no residue behind,
    /// mirroring the guarantee `TaskScope` itself makes in production.
    #[must_use]
    pub fn pending_task_count(&self) -> usize {
        self.inner.tasks.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    /// Polls every ready task to a fixed point: repeatedly sweeps the task
    /// list, polling any task whose waker has fired since its last poll,
    /// until a full sweep makes no further progress. Newly spawned tasks
    /// (including ones spawned *during* this call, from inside another
    /// task's poll) are picked up by the same call because the sweep
    /// re-reads the task list every pass.
    pub fn run_until_stalled(&self) {
        loop {
            let entries: Vec<Arc<TaskEntry>> =
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
                let waker = Waker::from(Arc::new(ManualWaker(Arc::clone(&entry.ready))));
                let mut cx = Context::from_waker(&waker);
                match future.as_mut().poll(&mut cx) {
                    Poll::Ready(()) => {
                        entry.finished.store(true, Ordering::Release);
                    }
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

    /// Advances the virtual clock by `duration`, wakes every [`Self::sleep`]
    /// future whose deadline has now passed, and drives them (and anything
    /// they in turn wake) to the next stall point via
    /// [`Self::run_until_stalled`].
    pub fn advance(&self, duration: Duration) {
        let mut due: Vec<(Duration, Waker)> = {
            let mut clock = self.inner.clock.lock().unwrap_or_else(PoisonError::into_inner);
            clock.now = clock.now.saturating_add(duration);
            let now = clock.now;
            let mut due = Vec::new();
            clock.sleepers.retain(|(deadline, waker)| {
                if *deadline <= now {
                    due.push((*deadline, waker.clone()));
                    false
                } else {
                    true
                }
            });
            due
        };
        // Wake and drain one deadline at a time, earliest first, rather than
        // waking everything due and sweeping once: `run_until_stalled`'s
        // sweep order otherwise reflects spawn order, not wake order, which
        // would let a later deadline's continuation run before an earlier
        // one's just because it was spawned first. Draining after each wake
        // matches the order a real clock would deliver these in, including
        // for any follow-up task a woken continuation itself spawns.
        due.sort_by_key(|(deadline, _)| *deadline);
        for (_, waker) in due {
            waker.wake();
            self.run_until_stalled();
        }
        // Nothing was due; still drive anything that was already ready.
        self.run_until_stalled();
    }
}

impl Executor for ManualExecutor {
    fn spawn(&self, future: BoxedTask) -> Box<dyn ExecutorHandle> {
        let ready = Arc::new(AtomicBool::new(true));
        let finished = Arc::new(AtomicBool::new(false));
        let aborted = Arc::new(AtomicBool::new(false));
        let entry = Arc::new(TaskEntry {
            future: StdMutex::new(Some(future)),
            ready,
            finished: Arc::clone(&finished),
            aborted: Arc::clone(&aborted),
        });
        self.inner.tasks.lock().unwrap_or_else(PoisonError::into_inner).push(entry);
        Box::new(ManualHandle { finished, aborted })
    }

    fn now(&self) -> Duration {
        ManualExecutor::now(self)
    }

    fn sleep(&self, duration: Duration) -> BoxedSleep {
        Box::pin(ManualSleep { inner: Arc::clone(&self.inner), duration, deadline: None })
    }
}

struct ManualSleep {
    inner: Arc<ManualInner>,
    duration: Duration,
    deadline: Option<Duration>,
}

impl Future for ManualSleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        // All fields are `Unpin` (`Arc`, `Duration`, `Option<Duration>`), so
        // projecting to a plain `&mut Self` is sound without pin-project.
        let this = self.get_mut();
        let mut clock = this.inner.clock.lock().unwrap_or_else(PoisonError::into_inner);
        let deadline =
            *this.deadline.get_or_insert_with(|| clock.now.saturating_add(this.duration));
        if clock.now >= deadline {
            Poll::Ready(())
        } else {
            clock.sleepers.push((deadline, cx.waker().clone()));
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    /// Polls `condition` until it holds or a generous deadline passes,
    /// returning whether it held.
    ///
    /// Every "did the executor get there yet?" assertion in this module goes
    /// through this instead of sleeping a fixed interval and hoping. The
    /// deadline is long enough that only a real hang reaches it, and the
    /// poll interval is short enough that a passing test costs milliseconds.
    fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            if condition() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        condition()
    }

    use super::*;
    use std::sync::atomic::Ordering as AtomicOrdering;

    #[test]
    fn dedicated_executor_is_independent_of_the_shared_one() {
        let dedicated = TokioExecutor::dedicated(1);
        let ran = Arc::new(AtomicBool::new(false));
        let ran_clone = Arc::clone(&ran);
        let handle = dedicated.spawn(Box::pin(async move {
            ran_clone.store(true, AtomicOrdering::SeqCst);
        }));
        assert!(
            wait_until(|| handle.is_finished()),
            "a task spawned on a dedicated executor must run there"
        );
        assert!(ran.load(AtomicOrdering::SeqCst));
    }

    #[test]
    fn abort_prevents_further_progress_of_a_pending_task() {
        let executor = TokioExecutor::dedicated(1);
        let handle = executor.spawn(Box::pin(async move {
            tokio::time::sleep(Duration::from_secs(60)).await;
        }));
        handle.abort();
        // Polled to a deadline rather than asserted after a fixed sleep.
        // Abort is asynchronous — it unparks the worker, which then drops
        // the future — so a fixed wait is a bet on scheduler latency, and
        // that bet loses on a loaded machine (this test failed exactly that
        // way once a task-retention stress test started running alongside
        // it). A deadline is both faster in the common case and not a race.
        assert!(
            wait_until(|| handle.is_finished()),
            "an aborted task must stop making progress rather than running its 60s sleep"
        );
    }

    #[test]
    fn manual_executor_does_not_run_a_spawned_task_until_polled() {
        let executor = ManualExecutor::new();
        let ran = Arc::new(AtomicBool::new(false));
        let ran_clone = Arc::clone(&ran);
        let handle = executor.spawn(Box::pin(async move {
            ran_clone.store(true, AtomicOrdering::SeqCst);
        }));
        assert!(!ran.load(AtomicOrdering::SeqCst), "nothing should run before run_until_stalled");
        executor.run_until_stalled();
        assert!(ran.load(AtomicOrdering::SeqCst));
        assert!(handle.is_finished());
    }

    #[test]
    fn manual_executor_sleep_only_resolves_after_advancing_past_its_deadline() {
        let executor = ManualExecutor::new();
        let woke = Arc::new(AtomicBool::new(false));
        let woke_clone = Arc::clone(&woke);
        let sleep_duration = Duration::from_millis(100);
        let spawn_executor = executor.clone();
        executor.spawn(Box::pin(async move {
            Executor::sleep(&spawn_executor, sleep_duration).await;
            woke_clone.store(true, AtomicOrdering::SeqCst);
        }));
        executor.run_until_stalled();
        assert!(!woke.load(AtomicOrdering::SeqCst), "sleep must not resolve before its deadline");

        executor.advance(Duration::from_millis(50));
        assert!(!woke.load(AtomicOrdering::SeqCst), "sleep must not resolve early");

        executor.advance(Duration::from_millis(50));
        assert!(
            woke.load(AtomicOrdering::SeqCst),
            "sleep must resolve once virtual time reaches it"
        );
    }

    #[test]
    fn manual_executor_prunes_finished_tasks() {
        let executor = ManualExecutor::new();
        for _ in 0..8 {
            executor.spawn(Box::pin(async {}));
        }
        executor.run_until_stalled();
        assert_eq!(executor.pending_task_count(), 0, "finished tasks must not be retained");
    }

    #[test]
    fn manual_executor_advance_wakes_multiple_sleepers_in_deadline_order() {
        let executor = ManualExecutor::new();
        let order = Arc::new(StdMutex::new(Vec::<u32>::new()));
        for (id, millis) in [(1u32, 30u64), (2, 10), (3, 20)] {
            let spawn_executor = executor.clone();
            let order = Arc::clone(&order);
            executor.spawn(Box::pin(async move {
                Executor::sleep(&spawn_executor, Duration::from_millis(millis)).await;
                order.lock().unwrap_or_else(PoisonError::into_inner).push(id);
            }));
        }
        executor.run_until_stalled();
        executor.advance(Duration::from_millis(30));
        assert_eq!(
            *order.lock().unwrap_or_else(PoisonError::into_inner),
            vec![2, 3, 1],
            "sleepers must resolve in deadline order regardless of spawn order"
        );
    }
}
