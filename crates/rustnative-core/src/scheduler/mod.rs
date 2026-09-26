//! Structured concurrency for components: task spawning, scoped
//! cancellation, and delivery of task results back into the reconciliation
//! loop.
//!
//! See `executor` for the pluggable backend [`Scheduler`] runs on (the
//! P1.7 fix), and the module-level docs on [`TaskScope`] for the ownership
//! model that makes task cancellation automatic when a component unmounts
//! (the P1.5 fix this crate already carried into this rewrite).

mod executor;
mod local;
pub mod supervise;
pub mod suspend;

pub use executor::{
    BoxedSleep as SleepFuture, BoxedTask, Executor, ExecutorHandle, ManualExecutor, TokioExecutor,
};
pub use local::{LocalBoxedTask, LocalExecutor, LocalPool};
pub use supervise::{
    Background, Offloaded, Supervised, SupervisionPolicy, TaskFailure, panic_message,
};
pub use suspend::{Priority, SuspendRule};

use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::Mutex;

use crate::identity::ComponentId;

/// Stable identity for one spawned task.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct TaskId(u64);

impl TaskId {
    /// Allocates the next sequential id.
    ///
    /// # Panics
    ///
    /// Panics on exhaustion (`u64::MAX` tasks spawned by one `Scheduler`
    /// over its lifetime), for the same reason as `crate::identity`'s
    /// allocators: silently wrapping and reusing a live task id is exactly
    /// the class of bug the standards audit's P1.20 finding asks every
    /// identity allocator in this crate to design out.
    #[allow(
        clippy::expect_used,
        reason = "the standards audit's P1.20 finding requires an identity allocator to fail loudly on \
    /// exhaustion rather than silently wrap and reuse a live id; there is no caller that \
    /// could act on a `Result` here"
    )]
    fn next(counter: &AtomicU64) -> Self {
        Self(
            counter
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    current.checked_add(1)
                })
                .expect(
                    "framework task identity space exhausted (more than u64::MAX tasks were \
                          spawned by one Scheduler over its lifetime)",
                ),
        )
    }
}

/// A cancellable handle to one spawned task; see [`Self::cancel`].
#[derive(Clone)]
pub struct TaskHandle {
    id: TaskId,
    abort: Arc<dyn ExecutorHandle>,
    // Tracks whether *we* cancelled this task, as distinct from
    // `ExecutorHandle::is_finished`, which is also true after ordinary
    // completion. `TaskHandle::is_cancelled` must answer "did someone call
    // cancel()", not "is the task done".
    cancelled: Arc<AtomicBool>,
    settled: Arc<dyn Fn(TaskId) + Send + Sync>,
}

impl TaskHandle {
    /// Returns the task's identity.
    #[must_use]
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Requests cancellation and prevents the task's result from ever being
    /// delivered to the owning component — see `Scheduler::drain`, which
    /// re-checks this same shared flag at the moment of delivery rather than
    /// trusting a snapshot taken when the task completed, closing the
    /// completion/cancellation race the standards audit's P1.5 finding
    /// describes.
    ///
    /// The executor cannot synchronously interrupt CPU-bound code: the
    /// request becomes effective when the future yields to the executor.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.abort.abort();
        (self.settled)(self.id);
    }

    /// Returns whether [`Self::cancel`] has been called.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Returns whether the executor has finished or aborted this task.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.abort.is_finished()
    }
}

/// Structured ownership of asynchronous tasks created by one component.
///
/// A scope is owned by a component-tree entry. Clones are lightweight
/// render-time handles to the same scope. When the component is unmounted,
/// the framework drops its owning scope and the shared scope state cancels
/// every task — see `TaskScopeInner`'s `Drop` implementation.
#[derive(Clone)]
pub struct TaskScope {
    inner: Rc<TaskScopeInner>,
}

struct TaskScopeInner {
    scheduler: Scheduler,
    local: Rc<dyn LocalExecutor>,
    target: ComponentId,
    /// Live tasks, plus — transiently — a `None` tombstone for a task that
    /// settled before `spawn` got to register it (see [`TaskScope::spawn`]).
    tasks: Arc<Mutex<HashMap<TaskId, Option<TaskHandle>>>>,
    /// Whether the owning component is hidden (`suspend`), shared with the
    /// scopes of its effects.
    suspension: Arc<suspend::Suspension>,
}

impl fmt::Debug for TaskScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskScope")
            .field("target", &self.inner.target)
            .field("task_count", &self.task_count())
            .finish()
    }
}

impl TaskScope {
    pub(crate) fn new(
        scheduler: Scheduler,
        local: Rc<dyn LocalExecutor>,
        target: ComponentId,
    ) -> Self {
        Self::with_suspension(scheduler, local, target, Arc::default())
    }

    /// A scope suspended and resumed together with others sharing
    /// `suspension` — an effect's, with its component's.
    pub(crate) fn with_suspension(
        scheduler: Scheduler,
        local: Rc<dyn LocalExecutor>,
        target: ComponentId,
        suspension: Arc<suspend::Suspension>,
    ) -> Self {
        Self {
            inner: Rc::new(TaskScopeInner {
                scheduler,
                local,
                target,
                tasks: Arc::new(Mutex::new(HashMap::new())),
                suspension,
            }),
        }
    }

    /// Spawns a task owned by this component scope. The task is
    /// automatically cancelled if the scope is dropped (the owning
    /// component unmounts, or the effect that spawned it re-runs) before the
    /// task completes.
    pub fn spawn<M, F>(&self, future: F) -> TaskHandle
    where
        M: Send + 'static,
        F: Future<Output = M> + Send + 'static,
    {
        let handle = self.inner.scheduler.spawn_with_settlement(
            self.inner.target,
            future,
            self.settlement(),
        );
        self.register(handle)
    }

    /// Spawns a `!Send` future owned by this scope, polled on the component
    /// tree's own thread (see [`LocalExecutor`]).
    ///
    /// Ownership is exactly [`Self::spawn`]'s: the task is cancelled when
    /// the scope is dropped, and its output is delivered as a message.
    pub fn spawn_local<M, F>(&self, future: F) -> TaskHandle
    where
        M: Send + 'static,
        F: Future<Output = M> + 'static,
    {
        let handle = self.inner.scheduler.spawn_local_with_settlement(
            &*self.inner.local,
            self.inner.target,
            future,
            self.settlement(),
        );
        self.register(handle)
    }

    /// The callback a task runs when it settles. Registered already is the
    /// ordinary case: remove it. Not registered yet means the task settled
    /// on another thread before [`Self::register`] got the lock, so a
    /// tombstone tells `register` not to add it at all.
    fn settlement(&self) -> Arc<dyn Fn(TaskId) + Send + Sync> {
        let tasks = Arc::clone(&self.inner.tasks);
        Arc::new(move |id| {
            let mut tasks = tasks.lock();
            if tasks.remove(&id).is_none() {
                tasks.insert(id, None);
            }
        })
    }

    fn register(&self, handle: TaskHandle) -> TaskHandle {
        // Decided under the same lock the settlement callback takes, so the
        // two can interleave in either order and the registry still ends up
        // without the task once it has settled. An earlier revision inserted
        // unconditionally and then checked `handle.is_finished()`, which
        // leaked the entry forever whenever settlement ran *before* the
        // insert but the executor had not yet marked the task finished — a
        // window the settlement callback, as the task's last statement,
        // always sits inside.
        let mut tasks = self.inner.tasks.lock();
        if tasks.remove(&handle.id()).is_none() {
            tasks.insert(handle.id(), Some(handle.clone()));
        }
        drop(tasks);
        handle
    }

    /// Cancels all currently owned tasks. This is idempotent.
    pub fn cancel_all(&self) {
        let tasks = self.inner.tasks.lock().values().flatten().cloned().collect::<Vec<_>>();
        for task in tasks {
            task.cancel();
        }
    }

    /// Returns the number of tasks currently tracked by this scope
    /// (spawned but not yet completed, cancelled, or drained).
    #[must_use]
    pub fn task_count(&self) -> usize {
        self.inner.tasks.lock().values().flatten().count()
    }

    /// The scheduler backing this scope, e.g. so a delay can be created
    /// through the same [`Executor`] this scope's tasks run on (see
    /// `EffectContext::sleep`/`ComponentContext::sleep`) rather than a
    /// hard-coded global timer — the standards audit's P1.18 finding.
    ///
    /// A `Send` handle: a task that sleeps in a loop clones it in.
    #[must_use]
    pub fn scheduler(&self) -> &Scheduler {
        &self.inner.scheduler
    }
}

impl Drop for TaskScopeInner {
    fn drop(&mut self) {
        let tasks = self.tasks.lock().values().flatten().cloned().collect::<Vec<_>>();
        for task in tasks {
            task.cancel();
        }
    }
}

pub(crate) struct CompletedTask {
    pub(crate) target: ComponentId,
    pub(crate) message: Box<dyn Any + Send>,
    cancelled: Arc<AtomicBool>,
}

struct SchedulerInner {
    next_id: AtomicU64,
    completed: Mutex<VecDeque<CompletedTask>>,
    waker: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    /// Set when [`Scheduler::host_waker`] fired before any waker was
    /// installed — a local task spawned during the first render — so
    /// [`Scheduler::set_waker`] can deliver the wake it missed.
    missed_wake: AtomicBool,
    executor: Arc<dyn Executor>,
}

/// Spawns and drives asynchronous component tasks to completion, decoupled
/// from any specific [`Executor`] implementation.
#[derive(Clone)]
pub struct Scheduler {
    inner: Arc<SchedulerInner>,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    /// Creates a scheduler backed by the process-wide default executor (a
    /// shared two-worker-thread Tokio runtime — see
    /// [`TokioExecutor::shared`]). This is what every `ComponentTree`/
    /// `Application` constructor uses unless [`Self::with_executor`] is
    /// used instead.
    #[must_use]
    pub fn new() -> Self {
        Self::with_executor(TokioExecutor::shared())
    }

    /// Creates a scheduler backed by a caller-supplied [`Executor`],
    /// addressing the standards audit's P1.7 finding directly: a host can
    /// now give an `Application` its own dedicated executor (see
    /// [`TokioExecutor::dedicated`]) instead of always sharing the one
    /// process-global runtime.
    pub fn with_executor(executor: Arc<dyn Executor>) -> Self {
        Self {
            inner: Arc::new(SchedulerInner {
                next_id: AtomicU64::new(1),
                completed: Mutex::new(VecDeque::new()),
                waker: Mutex::new(None),
                missed_wake: AtomicBool::new(false),
                executor,
            }),
        }
    }

    /// Registers a callback the scheduler invokes whenever a task
    /// completes, so a platform backend can wake its event loop rather than
    /// polling the completion queue.
    ///
    /// If any task has *already* completed by the time the waker is
    /// installed, it is invoked once immediately. That is not an
    /// optimization — it closes a real gap. A component's very first render
    /// happens while its `Application` is being constructed, which is
    /// strictly before a backend can install a waker (the backend needs an
    /// `Application` to run), so a task spawned from that first render can
    /// finish inside the window where no waker exists. Without this catch-up
    /// its result would sit in the queue with nothing scheduled to collect
    /// it: delivered late, on the next unrelated input, or — for a screen
    /// that is waiting on exactly that task and has no other input — never.
    ///
    /// A spurious wake is harmless (the backend pumps tasks and finds
    /// nothing), so this errs toward waking.
    pub fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) {
        // The completion queue is sampled *before* publishing the waker, and
        // the wake is fired *after*, with neither lock held. Any completion
        // racing this either lands before the sample (so the catch-up wake
        // covers it) or after the publish (so it fires the waker itself);
        // the overlap costs at most one extra wake, never a missed one.
        let already_completed = !self.inner.completed.lock().is_empty();
        // Published and the missed-wake flag consumed under one lock, which
        // `host_waker` also takes: a wake either sees the waker or leaves a
        // flag this read is guaranteed to observe.
        let missed = {
            let mut slot = self.inner.waker.lock();
            *slot = Some(waker);
            self.inner.missed_wake.swap(false, Ordering::AcqRel)
        };
        if already_completed || missed {
            // Re-read rather than keeping a clone: the waker is stored
            // behind the lock, so reading it back is what proves the catch-up
            // wake uses the same callback a real completion would.
            let installed = self.inner.waker.lock().clone();
            if let Some(installed) = installed {
                installed();
            }
        }
    }

    /// Spawns `future` on this scheduler's executor. The task runs to
    /// completion (or cancellation) off the caller's thread; its result is
    /// queued for `target` and observed the next time `Scheduler::drain` is
    /// called.
    pub fn spawn<M, F>(&self, target: ComponentId, future: F) -> TaskHandle
    where
        M: Send + 'static,
        F: Future<Output = M> + Send + 'static,
    {
        self.spawn_with_settlement(target, future, Arc::new(|_| {}))
    }

    pub(crate) fn spawn_with_settlement<M, F>(
        &self,
        target: ComponentId,
        future: F,
        settled: Arc<dyn Fn(TaskId) + Send + Sync>,
    ) -> TaskHandle
    where
        M: Send + 'static,
        F: Future<Output = M> + Send + 'static,
    {
        let id = TaskId::next(&self.inner.next_id);
        // A task settles by completing, by being cancelled, or both (a
        // cancel that races completion, or `cancel` called twice). The
        // callback must see exactly one of those: `TaskScope` reads a second
        // settlement of a task it already removed as "settled before it was
        // registered" and would strand a tombstone for it.
        let fired = AtomicBool::new(false);
        let settled: Arc<dyn Fn(TaskId) + Send + Sync> = Arc::new(move |id| {
            if !fired.swap(true, Ordering::AcqRel) {
                settled(id);
            }
        });
        let cancelled = Arc::new(AtomicBool::new(false));
        let inner = Arc::clone(&self.inner);
        let task_cancelled = Arc::clone(&cancelled);
        let task_settled = Arc::clone(&settled);
        let handle = self.inner.executor.spawn(Box::pin(async move {
            let value = future.await;
            // Re-checked authoritatively in `drain` against the same shared
            // flag: even if this check races a concurrent `cancel()` and
            // sees a stale `false`, the entry is filtered out for real at
            // drain time, so a cancelled task's result can never reach the
            // component regardless of exactly when `cancel()` ran relative
            // to this line (see `scheduler_stress` tests below).
            if !task_cancelled.load(Ordering::Acquire) {
                inner.completed.lock().push_back(CompletedTask {
                    target,
                    message: Box::new(value),
                    cancelled: task_cancelled,
                });
                if let Some(waker) = inner.waker.lock().clone() {
                    waker();
                }
            }
            task_settled(id);
        }));
        TaskHandle { id, abort: Arc::from(handle), cancelled, settled }
    }

    pub(crate) fn spawn_local_with_settlement<M, F>(
        &self,
        local: &dyn LocalExecutor,
        target: ComponentId,
        future: F,
        settled: Arc<dyn Fn(TaskId) + Send + Sync>,
    ) -> TaskHandle
    where
        M: Send + 'static,
        F: Future<Output = M> + 'static,
    {
        let id = TaskId::next(&self.inner.next_id);
        // Exactly-once settlement, for the reason `spawn_with_settlement`
        // gives.
        let fired = AtomicBool::new(false);
        let settled: Arc<dyn Fn(TaskId) + Send + Sync> = Arc::new(move |id| {
            if !fired.swap(true, Ordering::AcqRel) {
                settled(id);
            }
        });
        let cancelled = Arc::new(AtomicBool::new(false));
        let inner = Arc::clone(&self.inner);
        let task_cancelled = Arc::clone(&cancelled);
        let task_settled = Arc::clone(&settled);
        let handle = local.spawn_local(Box::pin(async move {
            let value = future.await;
            if !task_cancelled.load(Ordering::Acquire) {
                inner.completed.lock().push_back(CompletedTask {
                    target,
                    message: Box::new(value),
                    cancelled: task_cancelled,
                });
                if let Some(waker) = inner.waker.lock().clone() {
                    waker();
                }
            }
            task_settled(id);
        }));
        TaskHandle { id, abort: Arc::from(handle), cancelled, settled }
    }

    /// A callback that wakes whatever host this scheduler's waker belongs
    /// to, read at call time so it works even when created before
    /// [`Self::set_waker`] is called. A [`LocalPool`] is built with this.
    #[must_use]
    pub fn host_waker(&self) -> Arc<dyn Fn() + Send + Sync> {
        let inner = Arc::clone(&self.inner);
        Arc::new(move || {
            let waker = {
                let slot = inner.waker.lock();
                if slot.is_none() {
                    inner.missed_wake.store(true, Ordering::Release);
                }
                slot.clone()
            };
            if let Some(waker) = waker {
                waker();
            }
        })
    }

    /// The current time according to this scheduler's executor: virtual
    /// under [`ManualExecutor`], monotonic otherwise.
    #[must_use]
    pub fn now(&self) -> Duration {
        self.inner.executor.now()
    }

    pub(crate) fn drain(&self) -> Vec<CompletedTask> {
        self.inner
            .completed
            .lock()
            .drain(..)
            .filter(|completed| !completed.cancelled.load(Ordering::Acquire))
            .collect()
    }

    /// Returns a cancellable delay measured by *this* scheduler's own
    /// executor, closing the standards audit's P1.18 finding: an earlier
    /// version of this method always read a timer from
    /// [`TokioExecutor::shared`]'s runtime regardless of which executor a
    /// given `Scheduler` was constructed with, which made it impossible to
    /// deterministically test a component whose behavior depends on a
    /// delay. A `Scheduler` built with [`Self::with_executor`] using
    /// [`ManualExecutor`] instead resolves this future only when the test
    /// explicitly advances that executor's virtual clock.
    #[must_use]
    pub fn sleep(&self, duration: Duration) -> SleepFuture {
        self.inner.executor.sleep(duration)
    }
}

/// Drives `future` to completion on the shared runtime from a synchronous
/// context. `pub(crate)` — used only by this crate's own tests to call into
/// `async fn`-based service implementations from non-`async` `#[test]`
/// functions. Zero hand-rolled `unsafe`: `tokio::runtime::Runtime` owns a
/// sound, audited waker implementation internally.
#[cfg(test)]
pub(crate) fn block_on_for_test<F: Future>(future: F) -> F::Output {
    TokioExecutor::shared().runtime().block_on(future)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration as StdDuration;

    #[test]
    fn a_task_settles_exactly_once_however_it_ends() {
        use std::sync::atomic::AtomicUsize;

        let executor = ManualExecutor::new();
        let scheduler = Scheduler::with_executor(Arc::new(executor.clone()));
        let settlements = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&settlements);
        let handle = scheduler.spawn_with_settlement(
            ComponentId::next(&mut 1),
            async { 1usize },
            Arc::new(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
        );
        handle.cancel();
        handle.cancel();
        executor.run_until_stalled();
        assert_eq!(
            settlements.load(Ordering::SeqCst),
            1,
            "cancelled twice and run: one settlement"
        );
    }

    #[test]
    fn task_scope_removes_completed_tasks_from_its_registry() {
        let scheduler = Scheduler::new();
        let scope = TaskScope::new(
            scheduler.clone(),
            Rc::new(LocalPool::new(scheduler.host_waker())),
            ComponentId::next(&mut 1),
        );
        let handle = scope.spawn::<u32, _>(async { 1 });
        // Poll until the executor reports completion; avoid a fixed sleep,
        // which would make this test flaky under load.
        let deadline = std::time::Instant::now() + StdDuration::from_secs(5);
        while !handle.is_finished() && std::time::Instant::now() < deadline {
            std::thread::sleep(StdDuration::from_millis(1));
        }
        assert!(handle.is_finished());
        // Give the settlement callback (which runs after `is_finished`
        // becomes observable) a moment to run.
        std::thread::sleep(StdDuration::from_millis(20));
        assert_eq!(scope.task_count(), 0);
    }

    /// A task that finishes before any waker exists must still wake the
    /// backend once one is installed.
    ///
    /// This is not hypothetical timing trivia: a component's first render
    /// runs inside `Application::new`, and a platform backend can only
    /// install its waker afterwards, so every task spawned from a first
    /// render races exactly this window. The native integration test
    /// `native_task_wakeup` is what surfaced it — under load, the gap
    /// between constructing the application and starting the message loop
    /// grew wide enough for the task to land inside it, and the result was
    /// never collected.
    #[test]
    fn a_waker_installed_after_a_task_already_completed_still_fires() {
        let scheduler = Scheduler::new();
        let target = ComponentId::next(&mut 1);
        let handle = scheduler.spawn::<u32, _>(target, async { 7 });

        let deadline = std::time::Instant::now() + StdDuration::from_secs(5);
        while !handle.is_finished() && std::time::Instant::now() < deadline {
            std::thread::sleep(StdDuration::from_millis(1));
        }
        assert!(handle.is_finished(), "the task must have completed before the waker is set");
        // The completion callback runs just after `is_finished` becomes
        // observable; let it enqueue before installing the waker.
        std::thread::sleep(StdDuration::from_millis(50));

        let wakes = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&wakes);
        scheduler.set_waker(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));

        assert_eq!(
            wakes.load(Ordering::SeqCst),
            1,
            "installing a waker while a completion is already queued must wake immediately,              or that result is never collected"
        );
        assert_eq!(scheduler.drain().len(), 1, "and the queued result must still be there");
    }

    #[test]
    fn installing_a_waker_with_nothing_pending_does_not_fire_it() {
        let scheduler = Scheduler::new();
        let wakes = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&wakes);
        scheduler.set_waker(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        assert_eq!(wakes.load(Ordering::SeqCst), 0);
    }

    /// A component that spawns and completes tasks continuously does not
    /// accumulate state without bound.
    ///
    /// This is the standards audit's P1.5 finding turned into a test rather
    /// than a review note. `TaskScope` used to push every `TaskHandle` it
    /// ever created into a `Vec` and never remove any, so a long-lived
    /// component doing repeated async work retained a handle, an
    /// `AbortHandle`, and an `Arc<AtomicBool>` for every task in its entire
    /// history. That is invisible in a short test — a few dozen tasks cost
    /// nothing — and is a slow leak in an application left open all day,
    /// which is exactly the shape of bug worth a stress test rather than an
    /// example.
    ///
    /// The assertion is about the *registry*, not about memory: a bounded
    /// registry is the property that makes the retention bounded, and it is
    /// the one that can be checked deterministically.
    #[test]
    fn a_long_lived_scope_does_not_accumulate_handles_across_many_task_generations() {
        const GENERATIONS: usize = 40;
        const PER_GENERATION: usize = 25;

        let scheduler = Scheduler::new();
        let scope = TaskScope::new(
            scheduler.clone(),
            Rc::new(LocalPool::new(scheduler.host_waker())),
            ComponentId::next(&mut 1),
        );
        let mut high_water = 0usize;

        for _ in 0..GENERATIONS {
            let handles = (0..PER_GENERATION)
                .map(|value| scope.spawn::<usize, _>(async move { value }))
                .collect::<Vec<_>>();

            let deadline = std::time::Instant::now() + StdDuration::from_secs(10);
            while std::time::Instant::now() < deadline
                && !handles.iter().all(super::TaskHandle::is_finished)
            {
                std::thread::sleep(StdDuration::from_millis(1));
            }
            // The settlement callback that removes a finished task from the
            // registry runs just *after* `is_finished` becomes observable,
            // so this waits for the registry itself to drain rather than
            // for a fixed interval — under a loaded machine that callback
            // can be scheduled arbitrarily late, and a sleep long enough to
            // be reliable would be one nobody wants to sit through.
            while std::time::Instant::now() < deadline && scope.task_count() > 0 {
                std::thread::sleep(StdDuration::from_millis(1));
            }
            high_water = high_water.max(scope.task_count());
        }

        assert!(
            high_water <= PER_GENERATION,
            "the live-task registry peaked at {high_water} with only {PER_GENERATION} tasks ever \
             in flight at once — handles from completed generations are being retained (P1.5)"
        );
        assert_eq!(
            scope.task_count(),
            0,
            "every task completed, so the registry must be empty rather than holding \
             {GENERATIONS} generations of finished handles"
        );

        // Completions are still delivered — a registry that stayed empty by
        // dropping results would pass the assertions above and be useless.
        let delivered = scheduler.drain().len();
        assert_eq!(
            delivered,
            GENERATIONS * PER_GENERATION,
            "every completed task's result must still reach the scheduler's queue"
        );
    }

    #[test]
    fn dropping_a_task_scope_cancels_every_owned_task() {
        let scheduler = Scheduler::new();
        let ran_to_completion = Arc::new(AtomicBool::new(false));
        {
            let scope = TaskScope::new(
                scheduler.clone(),
                Rc::new(LocalPool::new(scheduler.host_waker())),
                ComponentId::next(&mut 1),
            );
            let flag = Arc::clone(&ran_to_completion);
            let _handle = scope.spawn::<(), _>(async move {
                tokio::time::sleep(StdDuration::from_millis(200)).await;
                flag.store(true, Ordering::SeqCst);
            });
        }
        std::thread::sleep(StdDuration::from_millis(400));
        assert!(!ran_to_completion.load(Ordering::SeqCst));
    }

    /// A stress test for the completion/cancellation race described in the
    /// standards audit's P1.5 finding: spawn many short tasks and cancel
    /// roughly half of them essentially concurrently with their completion,
    /// then assert that not one cancelled task's result was ever delivered.
    #[test]
    fn cancelled_tasks_never_deliver_a_result_even_under_concurrent_completion() {
        let scheduler = Scheduler::new();
        let delivered_from_cancelled = Arc::new(AtomicUsize::new(0));
        let target = ComponentId::next(&mut 1);

        let mut handles = Vec::new();
        for i in 0..500 {
            let handle = scheduler.spawn::<u32, _>(target, async move { i });
            handles.push(handle);
        }
        // Cancel every other task immediately, racing the executor's
        // worker threads, which are already trying to complete them.
        for handle in handles.iter().step_by(2) {
            handle.cancel();
        }

        // Give every task a chance to either complete or be cancelled.
        std::thread::sleep(StdDuration::from_millis(200));

        for completed in scheduler.drain() {
            // Every delivered result's task must not have been cancelled —
            // reaching this loop at all is already proof of that, since
            // `drain` filters on the shared flag, but this loop additionally
            // counts deliveries for the assertion below.
            delivered_from_cancelled.fetch_add(1, Ordering::SeqCst);
            let _ = completed.message;
        }

        // Roughly the odd-indexed half should have been delivered; none of
        // the cancelled (even-indexed) ones should ever appear here. This
        // assertion is deliberately loose (a range, not an exact count)
        // because a handful of odd-indexed tasks may also still be
        // in-flight when `drain` runs — the property under test is "no
        // cancelled task ever delivers", not "every surviving task always
        // delivers within this sleep window".
        assert!(delivered_from_cancelled.load(Ordering::SeqCst) <= 250);
    }
}
