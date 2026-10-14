//! Test doubles for the service contracts, with expectations.
//!
//! Every service a component reaches goes through [`rustnative_core::Services`],
//! so a test replaces one by registering a double. The doubles here also
//! *record*, which is what exhaustive mode ([`crate::HeadlessApp::exhaustive`])
//! checks at the end of a test: an HTTP request nobody expected, or an
//! expected one that never happened, fails the test rather than passing
//! silently.

use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;
use rustnative_core::{HttpRequest, HttpResponse, HttpService, Method, ServiceError};

/// One scripted HTTP exchange.
#[derive(Debug, Clone)]
struct Expectation {
    method: Method,
    url: String,
    response: Result<HttpResponse, ServiceError>,
}

#[derive(Debug, Default)]
struct MockState {
    expected: VecDeque<Expectation>,
    requests: Vec<HttpRequest>,
    unexpected: Vec<String>,
}

/// An [`HttpService`] that answers scripted requests in order and records
/// every request it receives.
///
/// ```
/// use rustnative_core::{HttpRequest, HttpResponse, HttpService, Method};
/// use rustnative_headless::MockHttp;
///
/// let http = MockHttp::new();
/// http.expect(Method::Get, "https://example.test/items", HttpResponse::new(200, vec![], b"[]".to_vec()));
/// # let runtime = std::thread::spawn(move || {
/// let response = futures_lite_block_on(http.execute(HttpRequest::get("https://example.test/items")));
/// # assert_eq!(response.unwrap().status(), 200);
/// # assert!(http.violations().is_empty());
/// # });
/// # runtime.join().unwrap();
/// # fn futures_lite_block_on<F: std::future::Future>(f: F) -> F::Output {
/// #     use std::task::{Context, Poll, Waker};
/// #     let mut f = std::pin::pin!(f);
/// #     let mut cx = Context::from_waker(Waker::noop());
/// #     loop { if let Poll::Ready(v) = f.as_mut().poll(&mut cx) { return v; } }
/// # }
/// ```
#[derive(Debug, Clone, Default)]
pub struct MockHttp {
    state: Arc<Mutex<MockState>>,
}

impl MockHttp {
    /// A mock with no expectations: every request it receives is a
    /// violation.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Expects `method url` next and answers it with `response`.
    pub fn expect(&self, method: Method, url: impl Into<String>, response: HttpResponse) {
        self.state.lock().expected.push_back(Expectation {
            method,
            url: url.into(),
            response: Ok(response),
        });
    }

    /// Expects `method url` next and fails it with `error`, as a network
    /// failure would.
    pub fn expect_failure(&self, method: Method, url: impl Into<String>, error: ServiceError) {
        self.state.lock().expected.push_back(Expectation {
            method,
            url: url.into(),
            response: Err(error),
        });
    }

    /// Every request received, in order.
    #[must_use]
    pub fn requests(&self) -> Vec<HttpRequest> {
        self.state.lock().requests.clone()
    }

    /// What went wrong: unexpected requests, and expectations never met.
    #[must_use]
    pub fn violations(&self) -> Vec<String> {
        let state = self.state.lock();
        let mut violations = state.unexpected.clone();
        violations.extend(state.expected.iter().map(|expectation| {
            format!(
                "expected {} {} was never requested",
                expectation.method.as_str(),
                expectation.url
            )
        }));
        violations
    }
}

#[async_trait::async_trait]
impl HttpService for MockHttp {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ServiceError> {
        let mut state = self.state.lock();
        state.requests.push(request.clone());
        let matches = state
            .expected
            .front()
            .is_some_and(|next| next.method == *request.method() && next.url == request.url());
        if matches {
            if let Some(next) = state.expected.pop_front() {
                return next.response;
            }
        }
        let description = format!("unexpected {} {}", request.method().as_str(), request.url());
        state.unexpected.push(description.clone());
        Err(ServiceError::new(description))
    }
}

/// A share sheet as a host that presents it as a popover anchored to its
/// source (iPadOS) answers: a request with no anchor node is refused, and
/// every request is recorded for the test to read.
#[derive(Debug, Clone, Default)]
pub struct AnchoredShare {
    shared: std::sync::Arc<std::sync::Mutex<Vec<rustnative_core::services::ShareRequest>>>,
}

impl AnchoredShare {
    /// A share sheet that has shown nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The requests it showed, oldest first.
    #[must_use]
    pub fn shown(&self) -> Vec<rustnative_core::services::ShareRequest> {
        self.shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
}

#[async_trait::async_trait]
impl rustnative_core::services::ShareService for AnchoredShare {
    async fn share(
        &self,
        request: rustnative_core::services::ShareRequest,
    ) -> Result<(), rustnative_core::ServiceError> {
        if request.anchor.is_none() {
            return Err(rustnative_core::ServiceError::new(
                "this host presents the share sheet from its source: the request names no anchor node",
            ));
        }
        self.shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(request);
        Ok(())
    }
}
