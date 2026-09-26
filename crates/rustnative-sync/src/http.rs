//! Sync over HTTP: the client adapter over the core `HttpService` (so the
//! Windows application syncs through `WinHttp`), and — with the `server`
//! feature — the endpoints on a `rustnative_server` application:
//! `POST /_sync/push`, `POST /_sync/pull`, and `GET /_sync/wait?since=N`,
//! a long poll that answers as soon as anything newer is accepted (server
//! push without a socket).

use rustnative_core::{HttpRequest, HttpService, Method};

use crate::replica::{Pull, PullReply, Push, PushReply, SyncError, SyncTransport};

/// The client adapter.
pub struct HttpSync {
    http: std::sync::Arc<dyn HttpService>,
    base: String,
    headers: Vec<(String, String)>,
}

impl HttpSync {
    /// Syncs with the server at `base` through `http`, sending `headers`
    /// (`Authorization: Bearer …`) with each request.
    #[must_use]
    pub fn new(
        http: std::sync::Arc<dyn HttpService>,
        base: &str,
        headers: Vec<(String, String)>,
    ) -> Self {
        Self { http, base: base.trim_end_matches('/').to_owned(), headers }
    }

    async fn post<T: serde::Serialize, R: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<R, SyncError> {
        let body =
            serde_json::to_vec(body).map_err(|error| SyncError::Decode(error.to_string()))?;
        let mut request = HttpRequest::new(Method::Post, format!("{}{path}", self.base))
            .header("content-type", "application/json")
            .body(body);
        for (name, value) in &self.headers {
            request = request.header(name.clone(), value.clone());
        }
        let response = self
            .http
            .execute(request)
            .await
            .map_err(|error| SyncError::Unreachable(error.to_string()))?;
        if !(200..300).contains(&response.status()) {
            return Err(SyncError::Unreachable(format!(
                "the server answered {}",
                response.status()
            )));
        }
        serde_json::from_slice(response.body_bytes())
            .map_err(|error| SyncError::Decode(error.to_string()))
    }

    /// Waits (up to the server's poll window) for anything newer than
    /// `since`; returns the latest sequence.
    ///
    /// # Errors
    ///
    /// The server is unreachable.
    pub async fn wait(&self, since: u64) -> Result<u64, SyncError> {
        let mut request = HttpRequest::get(format!("{}/_sync/wait?since={since}", self.base));
        for (name, value) in &self.headers {
            request = request.header(name.clone(), value.clone());
        }
        let response = self
            .http
            .execute(request)
            .await
            .map_err(|error| SyncError::Unreachable(error.to_string()))?;
        serde_json::from_slice(response.body_bytes())
            .map_err(|error| SyncError::Decode(error.to_string()))
    }
}

#[async_trait::async_trait]
impl SyncTransport for HttpSync {
    async fn push(&self, push: Push) -> Result<PushReply, SyncError> {
        self.post("/_sync/push", &push).await
    }

    async fn pull(&self, pull: Pull) -> Result<PullReply, SyncError> {
        self.post("/_sync/pull", &pull).await
    }
}

/// The server endpoints.
#[cfg(feature = "server")]
pub mod server {
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    use rustnative_server::handler::{Guarded, MethodRouter};
    use rustnative_server::{Json, Query, ServerApp, get, post};
    use serde::Deserialize;

    use crate::replica::{Pull, Push, SyncServer};

    #[derive(Deserialize)]
    struct Since {
        since: u64,
    }

    /// Adds the sync endpoints to `app`, each guarded by `guard`
    /// (`|router| router.signed_in::<User>()`).
    #[must_use]
    pub fn mount(
        app: ServerApp,
        sync: &Arc<Mutex<SyncServer>>,
        guard: impl Fn(MethodRouter) -> MethodRouter<Guarded>,
    ) -> ServerApp {
        let (pushing, pulling, waiting) = (Arc::clone(sync), Arc::clone(sync), Arc::clone(sync));
        app.route(
            "/_sync/push",
            guard(post(move |Json(push): Json<Push>| {
                let sync = Arc::clone(&pushing);
                async move { Json(sync.lock().unwrap_or_else(PoisonError::into_inner).push(push)) }
            })),
        )
        .route(
            "/_sync/pull",
            guard(post(move |Json(pull): Json<Pull>| {
                let sync = Arc::clone(&pulling);
                async move { Json(sync.lock().unwrap_or_else(PoisonError::into_inner).pull(&pull)) }
            })),
        )
        .route(
            "/_sync/wait",
            guard(get(move |Query(since): Query<Since>| {
                let sync = Arc::clone(&waiting);
                async move {
                    let mut changes =
                        sync.lock().unwrap_or_else(PoisonError::into_inner).subscribe();
                    let latest = |sync: &Arc<Mutex<SyncServer>>| {
                        sync.lock().unwrap_or_else(PoisonError::into_inner).sequence()
                    };
                    if latest(&sync) > since.since {
                        return Json(latest(&sync));
                    }
                    let _ = tokio::time::timeout(Duration::from_secs(25), changes.recv()).await;
                    Json(latest(&sync))
                }
            })),
        )
    }
}
