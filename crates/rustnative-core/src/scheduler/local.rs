//! The single-threaded executor seam: tasks that never leave the UI thread.
//!
//! [`super::Executor`] runs `Send` futures, which is right for work that
//! belongs on a thread pool and wrong for two kinds of host this framework
//! targets: a browser, whose DOM and most Web APIs exist only on the main
//! thread, and a single-core embedded loop with no threads to send to. Both
//! need futures that are `!Send` — ones holding an `Rc`, a host object, or a
//! main-thread-only binding — to be first-class component tasks with the
//! same ownership, cancellation, and message delivery as any other.
//!
//! [`LocalExecutor`] is that seam, and [`LocalPool`] is the implementation
//! every component tree carries: it lives on the tree's own thread, is
//! polled when the tree pumps its tasks, and wakes the host through the
//! same waker the thread-pool executor uses, so a backend that already
//! pumps tasks on wake needs no change to run local ones. A host with its
//! own main-thread executor (a browser's microtask queue, say) drives the
//! pool from it by calling [`crate::ComponentTree::pump_tasks`] when woken.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Wake, Waker};

use super::ExecutorHandle;

/// A boxed, type-erased, `!Send` unit future — what
/// [`LocalExecutor::spawn_local`] accepts.
pub type LocalBoxedTask = Pin<Box<dyn Future<Output = ()>>>;

/// An executor for futures that must stay on the thread that created them.
pub trait LocalExecutor {
    /// Takes ownership of `task`, to be polled on this executor's thread.
    fn spawn_local(&self, task: LocalBoxedTask) -> Box<dyn ExecutorHandle>;

    /// Polls every ready task until none can make progress. A host calls
    /// this from its own loop; [`crate::ComponentTree::pump_tasks`] calls it
    /// before delivering completed results.
    fn run_until_stalled(&self);

    /// The number of tasks spawned and not yet finished or aborted.
    fn pending_task_count(&self) -> usize;
}

struct LocalEntry {
    future: RefCell<Option<LocalBoxedTask>>,
    ready: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    aborted: Arc<AtomicBool>,
}

struct LocalHandle {
    finished: Arc<AtomicBool>,
    aborted: Arc<AtomicBool>,
    wake_host: Arc<dyn Fn() + Send + Sync>,
}

impl ExecutorHandle for LocalHandle {
    fn abort(&self) {
        self.aborted.store(true, Ordering::Release);
        // The pool drops the future on its next pass; waking the host makes
        // that pass happen promptly rather than at the next unrelated input.
        (self.wake_host)();
    }
    fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire) || self.aborted.load(Ordering::Acquire)
    }
}

struct LocalWaker {
    ready: Arc<AtomicBool>,
    wake_host: Arc<dyn Fn() + Send + Sync>,
}

impl Wake for LocalWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.ready.store(true, Ordering::Release);
        (self.wake_host)();
    }
}

/// The default [`LocalExecutor`]: a run-until-stalled pool polled on the
/// component tree's own thread.
///
/// The pool never runs anything by itself. A task makes progress when the
/// thread that owns the pool calls [`LocalExecutor::run_until_stalled`] —
/// which is what makes it a correct executor for a UI thread: a task is
/// only ever polled between messages, never in the middle of one.
///
/// # Example
///
/// ```
/// use std::cell::Cell;
/// use std::rc::Rc;
/// use std::sync::Arc;
///
/// use rustnative_core::{LocalExecutor, LocalPool};
///
/// let pool = LocalPool::new(Arc::new(|| {}));
/// // An `Rc` is not `Send`: this future could not run on a thread pool.
/// let count = Rc::new(Cell::new(0));
/// let seen = Rc::clone(&count);
/// pool.spawn_local(Box::pin(async move { seen.set(seen.get() + 1) }));
///
/// assert_eq!(count.get(), 0, "nothing runs until the owning thread asks");
/// pool.run_until_stalled();
/// assert_eq!(count.get(), 1);
/// assert_eq!(pool.pending_task_count(), 0);
/// ```
#[derive(Clone)]
pub struct LocalPool {
    tasks: Rc<RefCell<Vec<Rc<LocalEntry>>>>,
    wake_host: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for LocalPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalPool").field("pending", &self.pending_task_count()).finish()
    }
}

impl LocalPool {
    /// A pool that calls `wake_host` whenever one of its tasks becomes
    /// ready, so the host knows to call [`LocalExecutor::run_until_stalled`].
    #[must_use]
    pub fn new(wake_host: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { tasks: Rc::new(RefCell::new(Vec::new())), wake_host }
    }
}

impl LocalExecutor for LocalPool {
    fn spawn_local(&self, task: LocalBoxedTask) -> Box<dyn ExecutorHandle> {
        let finished = Arc::new(AtomicBool::new(false));
        let aborted = Arc::new(AtomicBool::new(false));
        self.tasks.borrow_mut().push(Rc::new(LocalEntry {
            future: RefCell::new(Some(task)),
            ready: Arc::new(AtomicBool::new(true)),
            finished: Arc::clone(&finished),
            aborted: Arc::clone(&aborted),
        }));
        (self.wake_host)();
        Box::new(LocalHandle { finished, aborted, wake_host: Arc::clone(&self.wake_host) })
    }

    fn run_until_stalled(&self) {
        loop {
            // A snapshot, so a task that spawns another while being polled
            // does not re-borrow the list; the new task is picked up by the
            // next sweep of this same call.
            let entries: Vec<Rc<LocalEntry>> = self.tasks.borrow().clone();
            let mut progressed = false;
            for entry in &entries {
                if entry.finished.load(Ordering::Acquire) {
                    continue;
                }
                if entry.aborted.load(Ordering::Acquire) {
                    entry.finished.store(true, Ordering::Release);
                    entry.future.borrow_mut().take();
                    progressed = true;
                    continue;
                }
                if !entry.ready.swap(false, Ordering::AcqRel) {
                    continue;
                }
                let taken = entry.future.borrow_mut().take();
                let Some(mut future) = taken else { continue };
                let waker = Waker::from(Arc::new(LocalWaker {
                    ready: Arc::clone(&entry.ready),
                    wake_host: Arc::clone(&self.wake_host),
                }));
                let mut cx = Context::from_waker(&waker);
                match future.as_mut().poll(&mut cx) {
                    Poll::Ready(()) => entry.finished.store(true, Ordering::Release),
                    Poll::Pending => *entry.future.borrow_mut() = Some(future),
                }
                progressed = true;
            }
            self.tasks.borrow_mut().retain(|entry| !entry.finished.load(Ordering::Acquire));
            if !progressed {
                break;
            }
        }
    }

    fn pending_task_count(&self) -> usize {
        self.tasks.borrow().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn a_ready_task_wakes_the_host_and_runs_when_polled() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&wakes);
        let pool = LocalPool::new(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        let ran = Rc::new(Cell::new(false));
        let flag = Rc::clone(&ran);
        pool.spawn_local(Box::pin(async move { flag.set(true) }));
        assert_eq!(wakes.load(Ordering::SeqCst), 1, "spawning asks the host for a pass");
        pool.run_until_stalled();
        assert!(ran.get());
    }

    #[test]
    fn an_aborted_task_is_dropped_without_running() {
        let pool = LocalPool::new(Arc::new(|| {}));
        let ran = Rc::new(Cell::new(false));
        let flag = Rc::clone(&ran);
        let handle = pool.spawn_local(Box::pin(async move { flag.set(true) }));
        handle.abort();
        pool.run_until_stalled();
        assert!(!ran.get());
        assert!(handle.is_finished());
        assert_eq!(pool.pending_task_count(), 0);
    }

    #[test]
    fn a_task_spawned_while_polling_runs_in_the_same_pass() {
        let pool = LocalPool::new(Arc::new(|| {}));
        let ran = Rc::new(Cell::new(false));
        let inner_pool = pool.clone();
        let flag = Rc::clone(&ran);
        pool.spawn_local(Box::pin(async move {
            inner_pool.spawn_local(Box::pin(async move { flag.set(true) }));
        }));
        pool.run_until_stalled();
        assert!(ran.get());
    }
}
