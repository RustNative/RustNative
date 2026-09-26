//! An HTTP client with an interceptor chain and declared endpoints
//! (`PLAN.md` Milestone 47, `C34`).
//!
//! [`HttpClient`] wraps any [`HttpService`] — the host's (on Windows,
//! `rustnative_windows::WinHttp`, which enforces certificate pins) or a
//! test's — and passes every request through its [`Interceptor`]s in
//! order. The standard ones:
//!
//! - [`BearerAuth`]: adds the access token, and on a 401 refreshes it once
//!   and tries again;
//! - [`Retry`]: tries idempotent requests again after a transport failure or
//!   a 5xx;
//! - [`Logging`]: records each exchange;
//! - [`ResponseCache`]: revalidates `GET`s with their `ETag` and answers a
//!   304 from the cache.
//!
//! An [`Endpoint`] declares one operation of an API — method, path, and
//! the request and response types — and turns it into a typed call.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

use parking_lot::Mutex;
use rustnative_core::{HttpRequest, HttpResponse, HttpService, Method, ServiceError};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// A boxed, `Send` future — what a token refresh returns.
pub type SendFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// One link of an [`HttpClient`]'s chain.
#[async_trait::async_trait]
pub trait Interceptor: Send + Sync {
    /// Handles `request`, passing it on with `next.run(request)` (as often
    /// as it likes) or answering it itself.
    async fn intercept(
        &self,
        request: HttpRequest,
        next: Next<'_>,
    ) -> Result<HttpResponse, ServiceError>;
}

/// The rest of the chain after an interceptor.
#[derive(Clone, Copy)]
pub struct Next<'a> {
    chain: &'a [Arc<dyn Interceptor>],
    service: &'a dyn HttpService,
}

impl fmt::Debug for Next<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Next").field("remaining", &self.chain.len()).finish_non_exhaustive()
    }
}

impl Next<'_> {
    /// Passes `request` to the next interceptor, or to the service after the
    /// last.
    ///
    /// # Errors
    ///
    /// Whatever the rest of the chain, or the service, fails with.
    pub async fn run(self, request: HttpRequest) -> Result<HttpResponse, ServiceError> {
        match self.chain.split_first() {
            Some((first, rest)) => {
                first.intercept(request, Next { chain: rest, service: self.service }).await
            }
            None => self.service.execute(request).await,
        }
    }
}

/// An [`HttpService`] with interceptors; see the [module
/// documentation](self). It is itself an `HttpService`, so it composes.
#[derive(Clone)]
pub struct HttpClient {
    service: Arc<dyn HttpService>,
    chain: Vec<Arc<dyn Interceptor>>,
    base: String,
}

impl fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpClient")
            .field("base", &self.base)
            .field("interceptors", &self.chain.len())
            .finish_non_exhaustive()
    }
}

impl HttpClient {
    /// A client sending through `service`, with URLs relative to `base`.
    pub fn new(service: Arc<dyn HttpService>, base: impl Into<String>) -> Self {
        Self { service, chain: Vec::new(), base: base.into() }
    }

    /// Adds `interceptor` after the ones already added: the first added
    /// sees a request first and its response last.
    #[must_use]
    pub fn with(mut self, interceptor: impl Interceptor + 'static) -> Self {
        self.chain.push(Arc::new(interceptor));
        self
    }

    /// `path` resolved against the base URL.
    #[must_use]
    pub fn url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            path.to_owned()
        } else {
            format!("{}/{}", self.base.trim_end_matches('/'), path.trim_start_matches('/'))
        }
    }
}

#[async_trait::async_trait]
impl HttpService for HttpClient {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ServiceError> {
        Next { chain: &self.chain, service: &*self.service }.run(request).await
    }
}

fn with_header(request: &HttpRequest, name: &str, value: &str) -> HttpRequest {
    let mut copy = HttpRequest::new(request.method().clone(), request.url());
    for (header, existing) in request.headers() {
        if !header.eq_ignore_ascii_case(name) {
            copy = copy.header(header.clone(), existing.clone());
        }
    }
    copy.header(name, value).body(request.body_bytes().to_vec())
}

/// Adds a bearer token, and refreshes it once when the server answers 401.
pub struct BearerAuth {
    token: Mutex<String>,
    refresh: Box<dyn Fn() -> SendFuture<Result<String, ServiceError>> + Send + Sync>,
}

impl fmt::Debug for BearerAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BearerAuth").finish_non_exhaustive()
    }
}

impl BearerAuth {
    /// Starts with `token`; `refresh` gets a new one.
    pub fn new(
        token: impl Into<String>,
        refresh: impl Fn() -> SendFuture<Result<String, ServiceError>> + Send + Sync + 'static,
    ) -> Self {
        Self { token: Mutex::new(token.into()), refresh: Box::new(refresh) }
    }
}

#[async_trait::async_trait]
impl Interceptor for BearerAuth {
    async fn intercept(
        &self,
        request: HttpRequest,
        next: Next<'_>,
    ) -> Result<HttpResponse, ServiceError> {
        let token = self.token.lock().clone();
        let response =
            next.run(with_header(&request, "Authorization", &format!("Bearer {token}"))).await?;
        if response.status() != 401 {
            return Ok(response);
        }
        let fresh = (self.refresh)().await?;
        self.token.lock().clone_from(&fresh);
        next.run(with_header(&request, "Authorization", &format!("Bearer {fresh}"))).await
    }
}

/// Tries idempotent requests again after a transport failure or a 5xx.
#[derive(Debug, Clone, Copy)]
pub struct Retry {
    /// How many times to try again.
    pub attempts: u32,
}

#[async_trait::async_trait]
impl Interceptor for Retry {
    async fn intercept(
        &self,
        request: HttpRequest,
        next: Next<'_>,
    ) -> Result<HttpResponse, ServiceError> {
        let idempotent =
            matches!(request.method(), Method::Get | Method::Put | Method::Delete | Method::Head);
        let mut tries = 0;
        loop {
            let outcome = next.run(request.clone()).await;
            let failed = match &outcome {
                Ok(response) => response.status() >= 500,
                Err(_) => true,
            };
            if !failed || !idempotent || tries >= self.attempts {
                return outcome;
            }
            tries += 1;
        }
    }
}

/// Records every exchange as `METHOD url -> status`.
#[derive(Debug, Clone, Default)]
pub struct Logging {
    log: Arc<Mutex<Vec<String>>>,
}

impl Logging {
    /// An empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Everything recorded so far.
    #[must_use]
    pub fn entries(&self) -> Vec<String> {
        self.log.lock().clone()
    }
}

#[async_trait::async_trait]
impl Interceptor for Logging {
    async fn intercept(
        &self,
        request: HttpRequest,
        next: Next<'_>,
    ) -> Result<HttpResponse, ServiceError> {
        let line = format!("{} {}", request.method(), request.url());
        let outcome = next.run(request).await;
        let result = match &outcome {
            Ok(response) => response.status().to_string(),
            Err(error) => format!("error: {error}"),
        };
        self.log.lock().push(format!("{line} -> {result}"));
        outcome
    }
}

/// Revalidates `GET`s with the `ETag` of the cached response, and answers
/// a 304 with the cached response.
#[derive(Debug, Clone, Default)]
pub struct ResponseCache {
    entries: Arc<Mutex<HashMap<String, (String, HttpResponse)>>>,
}

impl ResponseCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl Interceptor for ResponseCache {
    async fn intercept(
        &self,
        request: HttpRequest,
        next: Next<'_>,
    ) -> Result<HttpResponse, ServiceError> {
        if !matches!(request.method(), Method::Get) {
            return next.run(request).await;
        }
        let url = request.url().to_owned();
        let cached = self.entries.lock().get(&url).cloned();
        let request = match &cached {
            Some((etag, _)) => with_header(&request, "If-None-Match", etag),
            None => request,
        };
        let response = next.run(request).await?;
        if response.status() == 304 {
            if let Some((_, cached)) = cached {
                return Ok(cached);
            }
        }
        let etag = response
            .headers()
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("etag"))
            .map(|(_, value)| value.clone());
        if let (Some(etag), 200) = (etag, response.status()) {
            self.entries.lock().insert(url, (etag, response.clone()));
        }
        Ok(response)
    }
}

/// One declared operation of an API: a typed call (`C34`).
///
/// The path may hold `{name}` placeholders, filled from the parameters.
///
/// ```
/// use rustnative_data::Endpoint;
///
/// #[derive(serde::Deserialize)]
/// struct Todo { title: String }
///
/// const TODO: Endpoint<(), Todo> = Endpoint::get("todos/{id}");
/// assert_eq!(TODO.path(&[("id", "7")]), "todos/7");
/// ```
pub struct Endpoint<Req, Res> {
    method: fn() -> Method,
    path: &'static str,
    _types: PhantomData<fn(Req) -> Res>,
}

impl<Req, Res> fmt::Debug for Endpoint<Req, Res> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", (self.method)(), self.path)
    }
}

impl<Req, Res> Endpoint<Req, Res> {
    /// A `GET` of `path`.
    #[must_use]
    pub const fn get(path: &'static str) -> Self {
        Self { method: || Method::Get, path, _types: PhantomData }
    }

    /// A `POST` to `path`.
    #[must_use]
    pub const fn post(path: &'static str) -> Self {
        Self { method: || Method::Post, path, _types: PhantomData }
    }

    /// A `PUT` to `path`.
    #[must_use]
    pub const fn put(path: &'static str) -> Self {
        Self { method: || Method::Put, path, _types: PhantomData }
    }

    /// A `DELETE` of `path`.
    #[must_use]
    pub const fn delete(path: &'static str) -> Self {
        Self { method: || Method::Delete, path, _types: PhantomData }
    }

    /// The path with its placeholders filled.
    #[must_use]
    pub fn path(&self, parameters: &[(&str, &str)]) -> String {
        parameters.iter().fold(self.path.to_owned(), |path, (name, value)| {
            path.replace(&format!("{{{name}}}"), value)
        })
    }
}

impl<Req: Serialize, Res: DeserializeOwned> Endpoint<Req, Res> {
    /// Calls the endpoint through `client`.
    ///
    /// # Errors
    ///
    /// The transport failed, the server answered other than 2xx, or the
    /// body was not a `Res`.
    pub async fn call(
        &self,
        client: &HttpClient,
        parameters: &[(&str, &str)],
        body: Option<&Req>,
    ) -> Result<Res, ServiceError> {
        let mut request = HttpRequest::new((self.method)(), client.url(&self.path(parameters)))
            .header("Accept", "application/json");
        if let Some(body) = body {
            let bytes =
                serde_json::to_vec(body).map_err(|error| ServiceError::new(error.to_string()))?;
            request = request.header("Content-Type", "application/json").body(bytes);
        }
        let response = client.execute(request).await?;
        if !(200..300).contains(&response.status()) {
            return Err(ServiceError::new(format!("{self:?} answered {}", response.status())));
        }
        let body = if response.body_bytes().is_empty() {
            b"null".as_slice()
        } else {
            response.body_bytes()
        };
        serde_json::from_slice(body).map_err(|error| ServiceError::new(error.to_string()))
    }
}
