//! Supervision for long-lived server and device processes. When a worker
//! fails or panics, its [`SupervisionPolicy`] (from Milestone 47) restarts
//! it with backoff, and its history can be read afterwards.

use std::future::Future;

use futures_util::FutureExt;
use rustnative_core::SupervisionPolicy;

/// What happened to a supervised worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerEvent {
    /// It started (the attempt, from 1).
    Started(u32),
    /// It failed, with why.
    Failed(String),
    /// It finished on its own.
    Finished,
    /// The policy gave up.
    GaveUp,
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic".to_owned())
}

/// Runs the worker that `make()` returns, under `policy`. A failure (an
/// `Err` or a panic) restarts it after the policy's delay, until the policy
/// gives up. Returns the worker's history.
pub async fn supervise<F, Fut>(policy: SupervisionPolicy, make: F) -> Vec<WorkerEvent>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<(), String>> + Send,
{
    let mut history = Vec::new();
    let mut attempt = 0;
    loop {
        attempt += 1;
        history.push(WorkerEvent::Started(attempt));
        let error = match std::panic::AssertUnwindSafe(make()).catch_unwind().await {
            Ok(Ok(())) => {
                history.push(WorkerEvent::Finished);
                break;
            }
            Ok(Err(error)) => error,
            Err(panic) => panic_text(&*panic),
        };
        history.push(WorkerEvent::Failed(error));
        if let Some(delay) = policy.restart_delay(attempt) {
            tokio::time::sleep(delay).await;
        } else {
            history.push(WorkerEvent::GaveUp);
            break;
        }
    }
    history
}
