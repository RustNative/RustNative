//! Request scopes: work tied to a request's lifetime, as a component's
//! tasks are tied to its (Milestone 18). When the response is sent, or the
//! client goes away, everything spawned in the scope is cancelled.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use tokio::task::AbortHandle;

/// A request's task scope; cloning shares it.
#[derive(Debug, Clone, Default)]
pub struct RequestScope {
    tasks: Arc<Mutex<Vec<AbortHandle>>>,
    cancelled: Arc<AtomicBool>,
}

impl RequestScope {
    /// Spawns `work` on the runtime, cancelled when the request ends.
    pub fn spawn<F>(&self, work: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let handle = tokio::spawn(work).abort_handle();
        self.tasks.lock().unwrap_or_else(PoisonError::into_inner).push(handle);
    }

    /// Whether the request has ended.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Ends the scope: cancels everything spawned in it, and a
    /// transaction tied to it rolls back. The server does this when the
    /// response is sent.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        for task in self.tasks.lock().unwrap_or_else(PoisonError::into_inner).drain(..) {
            task.abort();
        }
    }
}

/// Cancels the scope when dropped: when the response has been produced, or
/// the future answering the request was dropped because the client left.
pub(crate) struct ScopeGuard(pub(crate) RequestScope);

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
