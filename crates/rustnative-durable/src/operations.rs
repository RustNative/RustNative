//! Long-running operations across the client/server boundary (`C87`). The
//! server runs the work, and the client watches its progress and can
//! cancel it. The states are the same as in
//! `rustnative_data::OperationState`.
//!
//! - **Server:** an [`Operations`] registry mounted on a `rustnative-server`
//!   application, at `GET /_ops/:id` and `POST /_ops/:id/cancel`.
//! - **Client:** [`follow`] and [`cancel`], over the core `HttpService`, so
//!   the Windows application can watch a server job.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rustnative_core::{HttpRequest, HttpService, Method};
use rustnative_server::handler::{Guarded, MethodRouter};
use rustnative_server::{Json, Path, ServerApp, ServerError, get, post};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Where an operation is, as it travels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RemoteState {
    /// Working toward `goal`.
    Running {
        /// What it is doing.
        goal: String,
        /// The latest progress report.
        progress: Option<Value>,
    },
    /// Finished with a result.
    Succeeded {
        /// The result.
        result: Value,
    },
    /// Finished without one.
    Failed {
        /// Why.
        error: String,
    },
    /// Stopped before finishing.
    Cancelled,
}

impl RemoteState {
    /// Whether it is over.
    #[must_use]
    pub const fn is_finished(&self) -> bool {
        !matches!(self, Self::Running { .. })
    }
}

/// What the work gets: a way to report progress, and a way to see a
/// cancellation.
#[derive(Clone)]
pub struct Reporter {
    state: Arc<Mutex<RemoteState>>,
    cancelled: Arc<AtomicBool>,
}

impl Reporter {
    /// Reports progress.
    pub fn report(&self, progress: impl Serialize) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let RemoteState::Running { progress: current, .. } = &mut *state {
            *current = serde_json::to_value(progress).ok();
        }
    }

    /// Whether the client asked to cancel. The work stops at its next check,
    /// and its task is also aborted.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

struct Entry {
    state: Arc<Mutex<RemoteState>>,
    cancelled: Arc<AtomicBool>,
    task: tokio::task::AbortHandle,
}

/// The server's running operations.
#[derive(Clone, Default)]
pub struct Operations {
    entries: Arc<Mutex<HashMap<u64, Entry>>>,
    next: Arc<AtomicU64>,
}

impl Operations {
    /// No operations yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts `work` toward `goal`. Returns its id for the client.
    pub fn start<F, Fut>(&self, goal: &str, work: F) -> u64
    where
        F: FnOnce(Reporter) -> Fut,
        Fut: Future<Output = Result<Value, String>> + Send + 'static,
    {
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let state =
            Arc::new(Mutex::new(RemoteState::Running { goal: goal.to_owned(), progress: None }));
        let cancelled = Arc::new(AtomicBool::new(false));
        let reporter = Reporter { state: Arc::clone(&state), cancelled: Arc::clone(&cancelled) };
        let future = work(reporter);
        let finished = Arc::clone(&state);
        let task = tokio::spawn(async move {
            let outcome = future.await;
            let mut state = finished.lock().unwrap_or_else(PoisonError::into_inner);
            if !state.is_finished() {
                *state = match outcome {
                    Ok(result) => RemoteState::Succeeded { result },
                    Err(error) => RemoteState::Failed { error },
                };
            }
        })
        .abort_handle();
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, Entry { state, cancelled, task });
        id
    }

    /// Where operation `id` is.
    #[must_use]
    pub fn state(&self, id: u64) -> Option<RemoteState> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .map(|entry| entry.state.lock().unwrap_or_else(PoisonError::into_inner).clone())
    }

    /// Cancels operation `id`.
    pub fn cancel(&self, id: u64) {
        if let Some(entry) = self.entries.lock().unwrap_or_else(PoisonError::into_inner).get(&id) {
            entry.cancelled.store(true, Ordering::SeqCst);
            entry.task.abort();
            let mut state = entry.state.lock().unwrap_or_else(PoisonError::into_inner);
            if !state.is_finished() {
                *state = RemoteState::Cancelled;
            }
        }
    }

    /// Mounts `GET /_ops/:id` and `POST /_ops/:id/cancel` on `app`, each
    /// guarded by `guard`.
    #[must_use]
    pub fn mount(
        &self,
        app: ServerApp,
        guard: impl Fn(MethodRouter) -> MethodRouter<Guarded>,
    ) -> ServerApp {
        let (reading, cancelling) = (self.clone(), self.clone());
        app.route(
            "/_ops/:id",
            guard(get(move |Path(id): Path<u64>| {
                let operations = reading.clone();
                async move { operations.state(id).map(Json).ok_or_else(ServerError::not_found) }
            })),
        )
        .route(
            "/_ops/:id/cancel",
            guard(post(move |Path(id): Path<u64>| {
                let operations = cancelling.clone();
                async move {
                    operations.cancel(id);
                    Json(operations.state(id))
                }
            })),
        )
    }
}

fn with_headers(mut request: HttpRequest, headers: &[(&str, &str)]) -> HttpRequest {
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request
}

async fn state_of(
    http: &dyn HttpService,
    base: &str,
    id: u64,
    headers: &[(&str, &str)],
) -> Result<RemoteState, String> {
    let request = with_headers(
        HttpRequest::get(format!("{}/_ops/{id}", base.trim_end_matches('/'))),
        headers,
    );
    let response = http.execute(request).await.map_err(|error| error.to_string())?;
    serde_json::from_slice(response.body_bytes()).map_err(|error| error.to_string())
}

/// Follows operation `id` on the server at `base` until it is over. Calls
/// `on_state` with each new state, and returns the last one.
///
/// # Errors
///
/// The server is unreachable.
pub async fn follow(
    http: &dyn HttpService,
    base: &str,
    id: u64,
    headers: &[(&str, &str)],
    poll: Duration,
    mut on_state: impl FnMut(&RemoteState),
) -> Result<RemoteState, String> {
    let mut last: Option<RemoteState> = None;
    loop {
        let state = state_of(http, base, id, headers).await?;
        if last.as_ref() != Some(&state) {
            on_state(&state);
            last = Some(state.clone());
        }
        if state.is_finished() {
            return Ok(state);
        }
        tokio::time::sleep(poll).await;
    }
}

/// Asks the server to cancel operation `id`.
///
/// # Errors
///
/// The server is unreachable.
pub async fn cancel(
    http: &dyn HttpService,
    base: &str,
    id: u64,
    headers: &[(&str, &str)],
) -> Result<(), String> {
    let request = with_headers(
        HttpRequest::new(Method::Post, format!("{}/_ops/{id}/cancel", base.trim_end_matches('/'))),
        headers,
    );
    http.execute(request).await.map(|_| ()).map_err(|error| error.to_string())
}
