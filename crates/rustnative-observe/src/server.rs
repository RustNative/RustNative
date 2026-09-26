//! Tracing a `rustnative-server` application: every request is a server
//! span, continuing the caller's trace when it sent `traceparent`.

use std::sync::{Arc, Mutex, PoisonError};

use rustnative_server::RequestContext;
use rustnative_server::app::{After, Before};

use crate::trace::{ActiveSpan, SpanContext, SpanKind, Tracer};

/// The request's span context, for handlers that start child spans.
#[derive(Clone)]
pub struct RequestSpan(pub SpanContext);

impl rustnative_server::FromRequest for RequestSpan {
    fn from_request(request: &RequestContext) -> Result<Self, rustnative_server::ServerError> {
        request
            .value::<Self>()
            .cloned()
            .ok_or_else(|| rustnative_server::ServerError::internal("tracing is not set up"))
    }
}

/// The span in progress for a request (ended by the after-middleware).
struct Open(Arc<Mutex<Option<ActiveSpan>>>);

/// The middleware pair that traces requests with `tracer`.
#[must_use]
pub fn middleware(tracer: Tracer) -> (Before, After) {
    let before: Before = Arc::new(move |request: &mut RequestContext| {
        let parent = request.header("traceparent").and_then(SpanContext::from_traceparent);
        let span = tracer
            .span_of_kind(
                &format!("{} {}", request.method(), request.path()),
                SpanKind::Server,
                parent.as_ref(),
            )
            .attribute(crate::conventions::HTTP_METHOD, request.method().as_str().to_owned())
            .attribute(crate::conventions::URL_PATH, request.path().to_owned());
        request.insert(RequestSpan(span.context().clone()));
        request.insert(Open(Arc::new(Mutex::new(Some(span)))));
        Ok(())
    });
    let after: After =
        Arc::new(|request: &RequestContext, response: &mut rustnative_server::Response| {
            if let Some(Open(open)) = request.value::<Open>() {
                if let Some(span) = open.lock().unwrap_or_else(PoisonError::into_inner).take() {
                    span.set(crate::conventions::HTTP_STATUS, response.status().as_u16());
                    if response.status().is_server_error() {
                        span.fail(response.status().to_string());
                    }
                }
            }
        });
    (before, after)
}
