//! The application: routes, state, middleware, and the security pipeline,
//! as a `tower::Service` — served on its own listener or mounted inside an
//! existing service.

use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Instant;

use bytes::Bytes;
use http::{HeaderValue, Method, StatusCode, header};
use http_body_util::{BodyExt, Full, Limited};
use rustnative_core::Route;

use crate::handler::{Guarded, MethodRouter};
use crate::request::{RequestContext, TypeMap};
use crate::response::{IntoResponse, Response, ServerError};
use crate::scope::{RequestScope, ScopeGuard};
use crate::security::{
    CSRF_COOKIE, Cookie, CspNonce, CsrfToken, RateLimiter, Security, check_csrf, random_token,
    secure_headers,
};

/// Runs before routing: may attach values (a session, a principal) or
/// refuse the request.
pub type Before = Arc<dyn Fn(&mut RequestContext) -> Result<(), ServerError> + Send + Sync>;

/// Runs after the handler: may change the response (set a cookie).
pub type After = Arc<dyn Fn(&RequestContext, &mut Response) + Send + Sync>;

/// A route as the application lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteInfo {
    /// Its pattern.
    pub pattern: String,
    /// Its methods.
    pub methods: Vec<Method>,
    /// Who may use it (`public`, or a policy's name).
    pub access: &'static str,
}

/// A server application; see the crate documentation.
pub struct ServerApp {
    routes: Vec<(Route, String, MethodRouter<Guarded>)>,
    state: TypeMap,
    security: Security,
    prefix: String,
    before: Vec<Before>,
    after: Vec<After>,
    readiness: Vec<(String, Arc<dyn Fn() -> bool + Send + Sync>)>,
    report: Vec<String>,
    operations: Vec<crate::openapi::Operation>,
    openapi: Option<(String, String)>,
    extra: Vec<(String, Arc<dyn Fn() -> Response + Send + Sync>)>,
    cache: crate::cache::ResponseCache,
}

impl Default for ServerApp {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerApp {
    /// An application with the strict security defaults and no routes.
    #[must_use]
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            state: TypeMap::default(),
            security: Security::default(),
            prefix: String::new(),
            before: Vec::new(),
            after: Vec::new(),
            readiness: Vec::new(),
            report: Vec::new(),
            operations: Vec::new(),
            openapi: None,
            extra: Vec::new(),
            cache: crate::cache::ResponseCache::new(),
        }
    }

    /// The response cache (to invalidate tags when data changes).
    #[must_use]
    pub fn response_cache(&self) -> crate::cache::ResponseCache {
        self.cache.clone()
    }

    /// Serves the cache's state at `GET /__cache` to principals `Pol`
    /// allows.
    #[must_use]
    pub fn cache_inspection<P, Pol>(self) -> Self
    where
        P: Clone + Send + Sync + 'static,
        Pol: crate::auth::Policy<P>,
    {
        let cache = self.cache.clone();
        self.route(
            "/__cache",
            crate::get(move || {
                let cache = cache.clone();
                async move { crate::response::Json(cache.entries()) }
            })
            .authorized::<P, Pol>(),
        )
    }

    /// Describes an operation for the API schema.
    #[must_use]
    pub fn describe(mut self, operation: crate::openapi::Operation) -> Self {
        self.operations.push(operation);
        self
    }

    /// Serves the API schema at `/openapi.json`.
    #[must_use]
    pub fn openapi(mut self, title: &str, version: &str) -> Self {
        self.openapi = Some((title.to_owned(), version.to_owned()));
        self
    }

    /// The API schema document.
    #[must_use]
    pub fn openapi_document(&self, title: &str, version: &str) -> serde_json::Value {
        crate::openapi::document(title, version, &self.routes(), &self.operations)
    }

    /// Serves a fixed public resource at `path` (a sitemap, `robots.txt`),
    /// built when requested.
    #[must_use]
    pub fn resource(
        mut self,
        path: &str,
        build: impl Fn() -> Response + Send + Sync + 'static,
    ) -> Self {
        self.extra.push((path.to_owned(), Arc::new(build)));
        self
    }

    /// Adds a route. It must say who may use it (see [`crate::handler`]).
    ///
    /// # Panics
    ///
    /// If `pattern` is not a valid route pattern — a mistake in the
    /// source, found when the application is built.
    #[must_use]
    #[allow(clippy::panic, reason = "an invalid pattern is a source mistake, caught at startup")]
    pub fn route(mut self, pattern: &str, router: MethodRouter<Guarded>) -> Self {
        let route = Route::parse(pattern)
            .unwrap_or_else(|error| panic!("route pattern {pattern:?}: {error}"));
        self.routes.push((route, pattern.to_owned(), router));
        self
    }

    /// Registers application state, which handlers take as `State<T>`.
    #[must_use]
    pub fn state<T: Clone + Send + Sync + 'static>(mut self, value: T) -> Self {
        self.state.insert(value);
        self
    }

    /// Replaces the security settings.
    #[must_use]
    pub fn security(mut self, security: Security) -> Self {
        self.security = security;
        self
    }

    /// Serves the application below `prefix` (`/app`), for mounting inside
    /// another service; paths outside it are `404`.
    #[must_use]
    pub fn prefix(mut self, prefix: &str) -> Self {
        prefix.trim_end_matches('/').clone_into(&mut self.prefix);
        self
    }

    /// Adds middleware that runs before routing.
    #[must_use]
    pub fn before(mut self, middleware: Before) -> Self {
        self.before.push(middleware);
        self
    }

    /// Adds middleware that runs after the handler.
    #[must_use]
    pub fn after(mut self, middleware: After) -> Self {
        self.after.push(middleware);
        self
    }

    /// Adds a readiness check `/readyz` runs.
    #[must_use]
    pub fn readiness(
        mut self,
        name: &str,
        check: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Self {
        self.readiness.push((name.to_owned(), Arc::new(check)));
        self
    }

    /// Records a line for the startup report (see [`Self::report`]).
    #[must_use]
    pub fn note(mut self, line: impl Into<String>) -> Self {
        self.report.push(line.into());
        self
    }

    /// The routes, for the API schema and the admin surface.
    #[must_use]
    pub fn routes(&self) -> Vec<RouteInfo> {
        self.routes
            .iter()
            .map(|(_, pattern, router)| RouteInfo {
                pattern: pattern.clone(),
                methods: router.methods().into_iter().cloned().collect(),
                access: router.access,
            })
            .collect()
    }

    /// What was configured, and why (`C40`): each feature-driven default,
    /// whether it is on, and what turned it on.
    #[must_use]
    pub fn report(&self) -> Vec<String> {
        let security = &self.security;
        let mut lines = vec![
            format!("body limit: {} bytes (default 1 MiB)", security.body_limit),
            match security.rate_limit {
                Some((count, window)) => format!("rate limit: {count} per {}s per client", window.as_secs()),
                None => "rate limit: off (turned off by the application)".into(),
            },
            format!("request forgery protection: {}", if security.csrf { "on" } else { "off" }),
            "security headers: content security policy with a nonce, nosniff, frame denial, referrer and permissions policy".into(),
            format!("strict transport security: {}", if security.hsts { "on" } else { "off" }),
            format!(
                "health: {}",
                if cfg!(feature = "health") { "/healthz and /readyz (feature `health`)" } else { "off (feature `health` not enabled)" }
            ),
            format!(
                "metrics: {}",
                if cfg!(feature = "metrics") { "/metrics (feature `metrics`)" } else { "off (feature `metrics` not enabled)" }
            ),
            format!("routes: {}", self.routes.len()),
        ];
        lines.extend(self.report.iter().cloned());
        lines
    }

    /// The application as a service, to serve or to mount.
    #[must_use]
    pub fn into_service(self) -> AppService {
        AppService(Arc::new(Inner {
            app: self,
            limiter: RateLimiter::default(),
            requests: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            micros: AtomicU64::new(0),
        }))
    }
}

struct Inner {
    app: ServerApp,
    limiter: RateLimiter,
    requests: AtomicU64,
    errors: AtomicU64,
    micros: AtomicU64,
}

/// A running application: a `tower::Service` over `http` requests.
#[derive(Clone)]
pub struct AppService(Arc<Inner>);

impl AppService {
    /// Answers `request`, whose body is already read. `client` is the
    /// peer's address when known (it keys the rate limit).
    pub async fn handle(
        &self,
        request: http::Request<Bytes>,
        client: Option<SocketAddr>,
    ) -> Response {
        let started = Instant::now();
        let response = self.answer(request, client).await;
        let inner = &self.0;
        inner.requests.fetch_add(1, Ordering::Relaxed);
        if response.status().is_server_error() {
            inner.errors.fetch_add(1, Ordering::Relaxed);
        }
        let micros = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        inner.micros.fetch_add(micros, Ordering::Relaxed);
        response
    }

    #[allow(clippy::too_many_lines, reason = "the request pipeline, one step after another")]
    async fn answer(&self, request: http::Request<Bytes>, client: Option<SocketAddr>) -> Response {
        let app = &self.0.app;
        let (parts, body) = request.into_parts();
        let wants_json =
            parts.headers.get(header::ACCEPT).and_then(|value| value.to_str().ok()).is_some_and(
                |accept| accept.contains("application/json") && !accept.contains("text/html"),
            );
        let fail = |error: ServerError| error.render(wants_json);

        let Some(path) = parts.uri.path().strip_prefix(app.prefix.as_str()) else {
            return fail(ServerError::not_found());
        };
        let path = if path.is_empty() { "/".to_owned() } else { path.to_owned() };
        if !path.starts_with('/') {
            return fail(ServerError::not_found());
        }

        if let Some(limit) = app.security.rate_limit {
            let key =
                client.map_or_else(|| "unknown".to_owned(), |address| address.ip().to_string());
            if let Err(wait) = self.0.limiter.take(&key, limit) {
                let mut response =
                    fail(ServerError::new(StatusCode::TOO_MANY_REQUESTS, "Too many requests"));
                if let Ok(value) = HeaderValue::from_str(&wait.as_secs().max(1).to_string()) {
                    response.headers_mut().insert(header::RETRY_AFTER, value);
                }
                return response;
            }
        }
        if body.len() > app.security.body_limit {
            return fail(ServerError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "The request is too large",
            ));
        }

        if let Some(response) = self.operational(&path) {
            return response;
        }

        let Some((params, order, router)) = app.routes.iter().find_map(|(route, _, router)| {
            Some((
                route.matches(&path)?,
                route.parameter_names().into_iter().map(str::to_owned).collect(),
                router,
            ))
        }) else {
            return fail(ServerError::not_found());
        };
        let Some(handler) = router.handler(&parts.method) else {
            let mut response =
                fail(ServerError::new(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed"));
            let allow =
                router.methods().iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
            if let Ok(value) = HeaderValue::from_str(&allow) {
                response.headers_mut().insert(header::ALLOW, value);
            }
            return response;
        };

        let scope = RequestScope::default();
        let mut context = RequestContext {
            method: parts.method.clone(),
            path,
            query: parts.uri.query().unwrap_or_default().to_owned(),
            headers: parts.headers,
            body,
            params,
            param_order: order,
            state: Arc::new(app.state.clone()),
            values: TypeMap::default(),
            scope: scope.clone(),
            client,
        };
        let existing = context.cookie(CSRF_COOKIE).map(str::to_owned);
        let issued = existing.is_none();
        let csrf = existing.unwrap_or_else(|| random_token(24));
        let nonce = random_token(16);
        context.insert(CsrfToken(csrf.clone()));
        context.insert(CspNonce(nonce.clone()));

        for middleware in &app.before {
            if let Err(error) = middleware(&mut context) {
                return fail(error);
            }
        }
        if let Some(gate) = &router.gate {
            if let Err(error) = gate(&mut context) {
                return fail(error);
            }
        }
        if app.security.csrf && !router.csrf_exempt {
            if let Err(error) = check_csrf(&context) {
                return fail(error);
            }
        }

        let head = context.method == Method::HEAD;
        // Cached public GETs: the handler's output is reused as it was.
        let cache_key = (context.method == Method::GET && router.access == "public")
            .then_some(router.cache)
            .flatten()
            .map(|(tags, ttl)| (format!("{}?{}", context.path, context.query), tags, ttl));
        let after_context = RequestContext {
            method: context.method.clone(),
            path: context.path.clone(),
            query: context.query.clone(),
            headers: context.headers.clone(),
            body: Bytes::new(),
            params: context.params.clone(),
            param_order: context.param_order.clone(),
            state: Arc::clone(&context.state),
            values: context.values.clone(),
            scope: scope.clone(),
            client,
        };
        let guard = ScopeGuard(scope);
        let cached = cache_key.as_ref().and_then(|(key, _, _)| app.cache.get(key));
        let hit = cached.is_some();
        let mut response = match cached {
            Some(response) => response,
            None => handler(context).await,
        };
        drop(guard);
        if let Some((key, tags, ttl)) = cache_key {
            if !hit {
                app.cache.put(key, &response, tags, ttl);
            }
            response
                .headers_mut()
                .insert("x-cache", HeaderValue::from_static(if hit { "hit" } else { "miss" }));
        }

        if let Some(error) = response.extensions().get::<ServerError>().cloned() {
            response = error.render(wants_json);
        }
        for middleware in &app.after {
            middleware(&after_context, &mut response);
        }
        secure_headers(response.headers_mut(), &app.security, &nonce);
        if issued {
            let cookie = Cookie::new(CSRF_COOKIE, csrf).readable_by_script().strict();
            if let Ok(value) = HeaderValue::from_str(&cookie.to_string()) {
                response.headers_mut().append(header::SET_COOKIE, value);
            }
        }
        if head {
            *response.body_mut() = Bytes::new();
        }
        response
    }

    /// `/healthz`, `/readyz`, and `/metrics`.
    fn operational(&self, path: &str) -> Option<Response> {
        let inner = &self.0;
        if let Some((_, build)) = inner.app.extra.iter().find(|(candidate, _)| candidate == path) {
            return Some(build());
        }
        if path == "/openapi.json" {
            if let Some((title, version)) = &inner.app.openapi {
                let document = inner.app.openapi_document(title, version);
                return Some(crate::response::Json(document).into_response());
            }
        }
        match path {
            "/healthz" if cfg!(feature = "health") => Some("ok".into_response()),
            "/readyz" if cfg!(feature = "health") => {
                let failing = inner
                    .app
                    .readiness
                    .iter()
                    .filter(|(_, check)| !check())
                    .map(|(name, _)| name.clone())
                    .collect::<Vec<_>>();
                Some(if failing.is_empty() {
                    "ready".into_response()
                } else {
                    (StatusCode::SERVICE_UNAVAILABLE, format!("not ready: {}", failing.join(", ")))
                        .into_response()
                })
            }
            "/metrics" if cfg!(feature = "metrics") => {
                let requests = inner.requests.load(Ordering::Relaxed);
                let errors = inner.errors.load(Ordering::Relaxed);
                let micros = inner.micros.load(Ordering::Relaxed);
                #[allow(clippy::cast_precision_loss, reason = "a metric, not an account")]
                let seconds = micros as f64 / 1_000_000.0;
                let text = format!(
                    "# TYPE http_requests_total counter\nhttp_requests_total {requests}\n\
                     # TYPE http_server_errors_total counter\nhttp_server_errors_total {errors}\n\
                     # TYPE http_request_duration_seconds_sum counter\nhttp_request_duration_seconds_sum {seconds}\n"
                );
                let mut response = text.into_response();
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("text/plain; version=0.0.4"),
                );
                Some(response)
            }
            _ => None,
        }
    }

    /// Serves the application on `listener` until `shutdown` completes;
    /// requests in flight finish first.
    ///
    /// # Errors
    ///
    /// Accepting a connection failed.
    pub async fn serve(
        self,
        listener: tokio::net::TcpListener,
        shutdown: impl Future<Output = ()>,
    ) -> std::io::Result<()> {
        let mut shutdown = std::pin::pin!(shutdown);
        loop {
            let (stream, peer) = tokio::select! {
                accepted = listener.accept() => accepted?,
                () = &mut shutdown => return Ok(()),
            };
            let service = self.clone();
            tokio::spawn(async move {
                let io = hyper_util::rt::TokioIo::new(stream);
                let answer = hyper::service::service_fn(
                    move |request: http::Request<hyper::body::Incoming>| {
                        let service = service.clone();
                        async move {
                            Ok::<_, Infallible>(
                                service.collect_and_handle(request, Some(peer)).await,
                            )
                        }
                    },
                );
                let _ =
                    hyper::server::conn::http1::Builder::new().serve_connection(io, answer).await;
            });
        }
    }

    async fn collect_and_handle<B>(
        &self,
        request: http::Request<B>,
        client: Option<SocketAddr>,
    ) -> http::Response<Full<Bytes>>
    where
        B: http_body::Body + Send,
        B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        let (parts, body) = request.into_parts();
        let limit = self.0.app.security.body_limit;
        let response = match Limited::new(body, limit).collect().await {
            Ok(collected) => {
                self.handle(http::Request::from_parts(parts, collected.to_bytes()), client).await
            }
            Err(_) => ServerError::new(StatusCode::PAYLOAD_TOO_LARGE, "The request is too large")
                .render(false),
        };
        response.map(Full::new)
    }
}

impl<B> tower_service::Service<http::Request<B>> for AppService
where
    B: http_body::Body + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    type Response = http::Response<Full<Bytes>>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: http::Request<B>) -> Self::Future {
        let service = self.clone();
        let client = request.extensions().get::<SocketAddr>().copied();
        Box::pin(async move { Ok(service.collect_and_handle(request, client).await) })
    }
}
