//! Spans, logs, and trace context.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use rustnative_core::{HttpRequest, HttpResponse, HttpService, ServiceError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::export::Exporter;

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| u64::try_from(since.as_nanos()).unwrap_or(u64::MAX))
}

fn random_hex(bytes: usize) -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    while out.len() < bytes * 2 {
        let value = std::collections::hash_map::RandomState::new().build_hasher().finish();
        let _ = write!(out, "{value:016x}");
    }
    out.truncate(bytes * 2);
    out
}

/// Where a span sits in its trace (W3C Trace Context).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanContext {
    /// The trace, 32 hex digits.
    pub trace_id: String,
    /// The span, 16 hex digits.
    pub span_id: String,
}

impl SpanContext {
    /// The `traceparent` header value.
    #[must_use]
    pub fn traceparent(&self) -> String {
        format!("00-{}-{}-01", self.trace_id, self.span_id)
    }

    /// Reads a `traceparent` header value.
    #[must_use]
    pub fn from_traceparent(value: &str) -> Option<Self> {
        let parts: Vec<&str> = value.trim().split('-').collect();
        let valid = |part: &str, length: usize| {
            part.len() == length && part.bytes().all(|byte| byte.is_ascii_hexdigit())
        };
        if parts.len() != 4
            || !valid(parts[1], 32)
            || !valid(parts[2], 16)
            || parts[1].bytes().all(|byte| byte == b'0')
        {
            return None;
        }
        Some(Self {
            trace_id: parts[1].to_ascii_lowercase(),
            span_id: parts[2].to_ascii_lowercase(),
        })
    }
}

/// A span's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    /// Work inside the process.
    Internal,
    /// An outgoing request.
    Client,
    /// An incoming request.
    Server,
}

/// A finished span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpanRecord {
    /// The trace.
    pub trace_id: String,
    /// The span.
    pub span_id: String,
    /// The parent span, if any.
    pub parent_id: Option<String>,
    /// What it measured.
    pub name: String,
    /// Its kind.
    pub kind: SpanKind,
    /// Start, nanoseconds since the epoch.
    pub start_ns: u64,
    /// End.
    pub end_ns: u64,
    /// Its attributes (semantic conventions: [`crate::conventions`]).
    pub attributes: BTreeMap<String, Value>,
    /// An error, if it failed.
    pub error: Option<String>,
}

/// A log's severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Detail.
    Debug,
    /// Normal operation.
    Info,
    /// Something to look at.
    Warn,
    /// Something failed.
    Error,
}

/// A structured log record, tied to a span when there is one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogRecord {
    /// When, nanoseconds since the epoch.
    pub time_ns: u64,
    /// Severity.
    pub level: Level,
    /// The message.
    pub body: String,
    /// Structured fields.
    pub attributes: BTreeMap<String, Value>,
    /// The trace it belongs to.
    pub trace_id: Option<String>,
    /// The span it belongs to.
    pub span_id: Option<String>,
}

/// Emits spans and logs to exporters.
#[derive(Clone)]
pub struct Tracer {
    inner: Arc<TracerInner>,
}

struct TracerInner {
    service: String,
    exporters: Vec<Box<dyn Exporter>>,
}

impl Tracer {
    /// A tracer for `service`, sending to `exporters`.
    #[must_use]
    pub fn new(service: &str, exporters: Vec<Box<dyn Exporter>>) -> Self {
        Self { inner: Arc::new(TracerInner { service: service.to_owned(), exporters }) }
    }

    /// The service name.
    #[must_use]
    pub fn service(&self) -> &str {
        &self.inner.service
    }

    /// Starts a span, a child of `parent` when given.
    #[must_use]
    pub fn span(&self, name: &str, parent: Option<&SpanContext>) -> ActiveSpan {
        self.span_of_kind(name, SpanKind::Internal, parent)
    }

    /// Starts a span of `kind`.
    #[must_use]
    pub fn span_of_kind(
        &self,
        name: &str,
        kind: SpanKind,
        parent: Option<&SpanContext>,
    ) -> ActiveSpan {
        let context = SpanContext {
            trace_id: parent.map_or_else(|| random_hex(16), |parent| parent.trace_id.clone()),
            span_id: random_hex(8),
        };
        ActiveSpan {
            tracer: self.clone(),
            record: Mutex::new(Some(SpanRecord {
                trace_id: context.trace_id.clone(),
                span_id: context.span_id.clone(),
                parent_id: parent.map(|parent| parent.span_id.clone()),
                name: name.to_owned(),
                kind,
                start_ns: now_ns(),
                end_ns: 0,
                attributes: BTreeMap::new(),
                error: None,
            })),
            context,
        }
    }

    /// Emits a log record.
    pub fn log(
        &self,
        level: Level,
        body: &str,
        fields: &[(&str, Value)],
        span: Option<&SpanContext>,
    ) {
        let record = LogRecord {
            time_ns: now_ns(),
            level,
            body: body.to_owned(),
            attributes: fields
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
            trace_id: span.map(|span| span.trace_id.clone()),
            span_id: span.map(|span| span.span_id.clone()),
        };
        for exporter in &self.inner.exporters {
            exporter.log(&self.inner.service, &record);
        }
    }

    fn finish(&self, record: &SpanRecord) {
        for exporter in &self.inner.exporters {
            exporter.span(&self.inner.service, record);
        }
    }
}

/// A span in progress; it ends (and is exported) when dropped.
pub struct ActiveSpan {
    tracer: Tracer,
    record: Mutex<Option<SpanRecord>>,
    context: SpanContext,
}

impl ActiveSpan {
    /// Its context, for children and for propagation.
    #[must_use]
    pub const fn context(&self) -> &SpanContext {
        &self.context
    }

    /// Adds an attribute.
    #[must_use]
    pub fn attribute(self, key: &str, value: impl Into<Value>) -> Self {
        self.set(key, value);
        self
    }

    /// Sets an attribute.
    pub fn set(&self, key: &str, value: impl Into<Value>) {
        if let Some(record) = self.record.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
            record.attributes.insert(key.to_owned(), value.into());
        }
    }

    /// Marks it failed.
    pub fn fail(&self, error: impl Into<String>) {
        if let Some(record) = self.record.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
            record.error = Some(error.into());
        }
    }
}

impl Drop for ActiveSpan {
    fn drop(&mut self) {
        if let Some(mut record) = self.record.lock().unwrap_or_else(PoisonError::into_inner).take()
        {
            record.end_ns = now_ns();
            self.tracer.finish(&record);
        }
    }
}

/// An `HttpService` that traces every request as a client span and sends
/// its context as `traceparent`, so the server's span joins the trace.
pub struct TracedHttp<S: HttpService> {
    inner: S,
    tracer: Tracer,
    parent: Mutex<Option<SpanContext>>,
}

impl<S: HttpService> TracedHttp<S> {
    /// Wraps `inner`.
    pub const fn new(inner: S, tracer: Tracer) -> Self {
        Self { inner, tracer, parent: Mutex::new(None) }
    }

    /// Makes the next requests children of `parent`.
    pub fn within(&self, parent: Option<SpanContext>) {
        *self.parent.lock().unwrap_or_else(PoisonError::into_inner) = parent;
    }
}

#[async_trait::async_trait]
impl<S: HttpService> HttpService for TracedHttp<S> {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ServiceError> {
        let parent = self.parent.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let span = self
            .tracer
            .span_of_kind(
                &format!("{:?} {}", request.method(), crate::conventions::path_of(request.url())),
                SpanKind::Client,
                parent.as_ref(),
            )
            .attribute(
                crate::conventions::HTTP_METHOD,
                format!("{:?}", request.method()).to_uppercase(),
            )
            .attribute(crate::conventions::URL_FULL, request.url().to_owned());
        let request = request.header("traceparent", span.context().traceparent());
        let result = self.inner.execute(request).await;
        match &result {
            Ok(response) => span.set(crate::conventions::HTTP_STATUS, response.status()),
            Err(error) => span.fail(error.to_string()),
        }
        result
    }
}

/// Whether the person has agreed to send telemetry off the machine.
/// It starts off; the choice is kept in the state store by the caller
/// ([`Telemetry::to_state`] / [`Telemetry::from_state`]).
#[derive(Debug, Clone, Default)]
pub struct Telemetry {
    consent: Arc<AtomicBool>,
}

impl Telemetry {
    /// Records the person's choice.
    pub fn set_consent(&self, consent: bool) {
        self.consent.store(consent, Ordering::SeqCst);
    }

    /// Whether they agreed.
    #[must_use]
    pub fn consented(&self) -> bool {
        self.consent.load(Ordering::SeqCst)
    }

    /// The choice, to persist.
    #[must_use]
    pub fn to_state(&self) -> Value {
        Value::Bool(self.consented())
    }

    /// The persisted choice (absent: no).
    #[must_use]
    pub fn from_state(state: Option<&Value>) -> Self {
        let telemetry = Self::default();
        telemetry.set_consent(state.and_then(Value::as_bool).unwrap_or(false));
        telemetry
    }
}
