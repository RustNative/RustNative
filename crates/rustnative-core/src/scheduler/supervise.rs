//! Supervision, streams, and message-less background work (`PLAN.md`
//! Milestone 47: `C09-2`, `C12`, `C17`).
//!
//! # Supervision (`C17`)
//!
//! A [`SupervisionPolicy`] says what happens when supervised work fails by
//! panicking: [isolate](SupervisionPolicy::Isolate) it, [restart it with
//! backoff](SupervisionPolicy::RestartWithBackoff), or
//! [escalate](SupervisionPolicy::Escalate) to whoever supervises the
//! supervisor. The same policies apply to a subtree
//! ([`ComponentContext::boundary`](crate::ComponentContext::boundary)) and to
//! a task ([`TaskScope::spawn_supervised`]). A failure never cancels a
//! sibling: tasks in one scope, and components under one parent, fail
//! alone.
//!
//! # Streams (`C12`)
//!
//! [`TaskScope::collect`] delivers every item of a stream to the component
//! as a message, for as long as the component is mounted.
//!
//! - **Hot or cold** is the stream's own nature, not the framework's. The
//!   framework's streams are hot: a `tokio` channel receiver, a store's
//!   changes, an operation's progress. None replays what was sent before
//!   collection began.
//! - **Backpressure:** collection pulls as fast as the stream yields and
//!   queues each item for the next task pump. A producer that must not run
//!   ahead of the UI sends through a bounded channel, which backpressures
//!   the producer rather than the UI.
//! - **Lifetime:** collection is a task of the component's scope, so it
//!   ends when the component unmounts, and pauses while the component is
//!   hidden ([`crate::scheduler::suspend`]): a hot stream's producer is
//!   then backpressured by its channel, not drained into a hidden screen.
//!
//! # Prepare, then apply (`C09-2`)
//!
//! [`TaskScope::prepare`] runs a computation off the UI thread and delivers
//! its result, moved, as one message: the component applies it in one
//! update, so the UI never shows a half-applied state and never copies the
//! prepared value.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use parking_lot::Mutex;

use super::{
    CompletedTask, ExecutorHandle, LocalExecutor, Scheduler, TaskHandle, TaskId, TaskScope,
};

/// What happens when supervised work fails; see the [module
/// documentation](crate::scheduler::supervise).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SupervisionPolicy {
    /// Contain the failure and stop. A boundary shows its fallback until it
    /// is retried by hand; a task reports its failure as its result.
    #[default]
    Isolate,
    /// Contain the failure and start again after a delay that doubles from
    /// `initial` up to `max`, at most `attempts` times; then behave as
    /// [`Self::Isolate`].
    RestartWithBackoff {
        /// The first delay.
        initial: Duration,
        /// The longest delay.
        max: Duration,
        /// How many restarts are allowed.
        attempts: u32,
    },
    /// Do not contain the failure: pass it to the enclosing supervisor (the
    /// nearest enclosing boundary; for a task, the executor).
    Escalate,
}

impl SupervisionPolicy {
    /// The delay before restart `attempt` (counting from 1), or `None` when
    /// this policy does not restart then.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use rustnative_core::SupervisionPolicy;
    ///
    /// let policy = SupervisionPolicy::RestartWithBackoff {
    ///     initial: Duration::from_millis(10),
    ///     max: Duration::from_millis(40),
    ///     attempts: 3,
    /// };
    /// assert_eq!(policy.restart_delay(1), Some(Duration::from_millis(10)));
    /// assert_eq!(policy.restart_delay(2), Some(Duration::from_millis(20)));
    /// assert_eq!(policy.restart_delay(3), Some(Duration::from_millis(40)));
    /// assert_eq!(policy.restart_delay(4), None);
    /// ```
    #[must_use]
    pub fn restart_delay(self, attempt: u32) -> Option<Duration> {
        match self {
            Self::RestartWithBackoff { initial, max, attempts } if attempt <= attempts => {
                let factor = 1_u32.checked_shl(attempt.saturating_sub(1)).unwrap_or(u32::MAX);
                Some(initial.saturating_mul(factor).min(max))
            }
            _ => None,
        }
    }
}

/// Supervised work that failed and was not restarted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskFailure {
    /// The panic's message.
    pub message: String,
    /// How many times the work ran.
    pub attempts: u32,
}

/// The message a [`TaskScope::spawn_supervised`] task delivers.
pub type Supervised<M> = Result<M, TaskFailure>;

/// The text of a panic payload.
#[must_use]
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with a non-string payload".to_owned())
}

/// A future that turns a panic while polling into an `Err`.
struct CatchUnwind<F> {
    future: Pin<Box<F>>,
}

impl<F: Future> Future for CatchUnwind<F> {
    type Output = std::thread::Result<F::Output>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let future = self.future.as_mut();
        match catch_unwind(AssertUnwindSafe(move || future.poll(cx))) {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(value)) => Poll::Ready(Ok(value)),
            Err(payload) => Poll::Ready(Err(payload)),
        }
    }
}

/// Polls a stream to its next item without an extension trait.
struct Next<'a, S> {
    stream: &'a mut Pin<Box<S>>,
}

impl<S: futures_core::Stream> Future for Next<'_, S> {
    type Output = Option<S::Item>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.stream.as_mut().poll_next(cx)
    }
}

impl TaskScope {
    /// Spawns work made by `make`, supervised by `policy`: when it panics it
    /// is restarted or isolated as the policy says, and the component
    /// receives `deliver(Ok(output))` or, once it gives up,
    /// `deliver(Err(TaskFailure))`.
    /// Under [`SupervisionPolicy::Escalate`] a panic is not contained.
    pub fn spawn_supervised<T, M, F, Make>(
        &self,
        policy: SupervisionPolicy,
        make: Make,
        deliver: impl FnOnce(Supervised<T>) -> M + Send + 'static,
    ) -> TaskHandle
    where
        T: Send + 'static,
        M: Send + 'static,
        F: Future<Output = T> + Send + 'static,
        Make: Fn() -> F + Send + 'static,
    {
        let scheduler = self.scheduler().clone();
        let work = async move {
            let mut attempt = 0_u32;
            loop {
                attempt += 1;
                match (CatchUnwind { future: Box::pin(make()) }).await {
                    Ok(value) => return Ok(value),
                    Err(payload) => {
                        if policy == SupervisionPolicy::Escalate {
                            resume_unwind(payload);
                        }
                        match policy.restart_delay(attempt) {
                            Some(delay) => scheduler.sleep(delay).await,
                            None => {
                                return Err(TaskFailure {
                                    message: panic_message(&*payload),
                                    attempts: attempt,
                                });
                            }
                        }
                    }
                }
            }
        };
        self.spawn(async move { deliver(work.await) })
    }

    /// Delivers every item of `stream`, mapped by `map`, as a message; see
    /// the [module documentation](crate::scheduler::supervise) for its semantics.
    pub fn collect<S, M>(
        &self,
        stream: S,
        map: impl Fn(S::Item) -> M + Send + 'static,
    ) -> TaskHandle
    where
        S: futures_core::Stream + Send + 'static,
        M: Send + 'static,
    {
        let settled = self.settlement();
        let suspension = std::sync::Arc::clone(&self.inner.suspension);
        let handle = self.scheduler().spawn_posting(
            self.inner.target,
            move |post| super::suspend::Gated {
                // Paused while the component is hidden: a stream is pulled
                // only while its items can be shown.
                future: Box::pin(async move {
                    let mut stream = Box::pin(stream);
                    while let Some(item) = (Next { stream: &mut stream }).await {
                        post.post(map(item));
                    }
                }),
                suspension,
            },
            settled,
        );
        self.register(handle)
    }

    /// Runs `prepare` off the UI thread and delivers its result as one
    /// message, for the component to apply atomically (`C09-2`).
    pub fn prepare<M>(&self, prepare: impl FnOnce() -> M + Send + 'static) -> TaskHandle
    where
        M: Send + 'static,
    {
        self.spawn(async move { prepare() })
    }

    /// A handle for message-less work bound to nothing but itself — what a
    /// data layer uses to fetch and cache on behalf of many components.
    #[must_use]
    pub fn background(&self) -> Background {
        Background { scheduler: self.scheduler().clone(), local: Rc::clone(&self.inner.local) }
    }
}

/// Delivers messages from one running task (see [`TaskScope::collect`]).
pub(crate) struct Poster {
    scheduler: Scheduler,
    target: crate::identity::ComponentId,
    cancelled: Arc<AtomicBool>,
}

impl Poster {
    fn post<M: Send + 'static>(&self, message: M) {
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        self.scheduler.inner.completed.lock().push_back(CompletedTask {
            target: self.target,
            message: Box::new(message),
            cancelled: Arc::clone(&self.cancelled),
        });
        if let Some(waker) = self.scheduler.inner.waker.lock().clone() {
            waker();
        }
    }
}

impl Scheduler {
    pub(crate) fn spawn_posting<F>(
        &self,
        target: crate::identity::ComponentId,
        body: impl FnOnce(Poster) -> F,
        settled: Arc<dyn Fn(TaskId) + Send + Sync>,
    ) -> TaskHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let id = TaskId::next(&self.inner.next_id);
        let fired = AtomicBool::new(false);
        let settled: Arc<dyn Fn(TaskId) + Send + Sync> = Arc::new(move |id| {
            if !fired.swap(true, Ordering::AcqRel) {
                settled(id);
            }
        });
        let cancelled = Arc::new(AtomicBool::new(false));
        let poster = Poster { scheduler: self.clone(), target, cancelled: Arc::clone(&cancelled) };
        let future = body(poster);
        let task_settled = Arc::clone(&settled);
        let handle = self.inner.executor.spawn(Box::pin(async move {
            future.await;
            task_settled(id);
        }));
        TaskHandle { id, abort: Arc::from(handle), cancelled, settled }
    }
}

/// Message-less background work: futures on the UI thread that deliver
/// nothing to a component, and work offloaded to the executor whose result
/// is awaited. Obtained from [`TaskScope::background`] or
/// [`ComponentContext::background`](crate::ComponentContext::background).
///
/// Work spawned here is owned by its [`TaskHandle`], not by a component:
/// whoever spawns it decides when to cancel it.
#[derive(Clone)]
pub struct Background {
    scheduler: Scheduler,
    local: Rc<dyn LocalExecutor>,
}

impl std::fmt::Debug for Background {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Background").finish_non_exhaustive()
    }
}

/// A result being computed on the executor; see [`Background::offload`].
pub struct Offloaded<T> {
    slot: Arc<Mutex<(Option<T>, Option<Waker>)>>,
    handle: Box<dyn ExecutorHandle>,
}

impl<T> Future for Offloaded<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let mut slot = self.slot.lock();
        if let Some(value) = slot.0.take() {
            return Poll::Ready(value);
        }
        slot.1 = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl<T> Drop for Offloaded<T> {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

struct Detached;

impl ExecutorHandle for Detached {
    fn abort(&self) {}
    fn is_finished(&self) -> bool {
        true
    }
}

impl Background {
    /// Runs `future` on the UI thread, delivering nothing.
    pub fn spawn_local(&self, future: impl Future<Output = ()> + 'static) -> TaskHandle {
        let id = TaskId::next(&self.scheduler.inner.next_id);
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled);
        let handle = self.local.spawn_local(Box::pin(async move {
            if !flag.load(Ordering::Acquire) {
                future.await;
            }
        }));
        TaskHandle { id, abort: Arc::from(handle), cancelled, settled: Arc::new(|_| {}) }
    }

    /// Runs `future` on the executor; awaiting the result (from a
    /// [`Self::spawn_local`] future) yields its output. Dropping the result
    /// cancels the work.
    pub fn offload<T: Send + 'static>(
        &self,
        future: impl Future<Output = T> + Send + 'static,
    ) -> Offloaded<T> {
        let slot = Arc::new(Mutex::new((None, None::<Waker>)));
        let filled = Arc::clone(&slot);
        let handle = self.scheduler.inner.executor.spawn(Box::pin(async move {
            let value = future.await;
            let waker = {
                let mut slot = filled.lock();
                slot.0 = Some(value);
                slot.1.take()
            };
            if let Some(waker) = waker {
                waker.wake();
            }
        }));
        Offloaded { slot, handle }
    }

    /// A delay on the scheduler's clock (virtual under
    /// [`crate::ManualExecutor`]).
    pub fn sleep(&self, duration: Duration) -> super::SleepFuture {
        self.scheduler.sleep(duration)
    }

    /// The scheduler's current time.
    #[must_use]
    pub fn now(&self) -> Duration {
        self.scheduler.now()
    }

    /// A handle that does nothing, for a slot with no work in it.
    #[must_use]
    pub fn idle(&self) -> TaskHandle {
        TaskHandle {
            id: TaskId::next(&self.scheduler.inner.next_id),
            abort: Arc::new(Detached),
            cancelled: Arc::new(AtomicBool::new(false)),
            settled: Arc::new(|_| {}),
        }
    }
}
