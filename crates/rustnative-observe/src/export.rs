//! Exporters: where spans and logs go.

use std::io::Write as _;
use std::sync::{Arc, Mutex, PoisonError};

use rustnative_core::{HttpRequest, HttpService, Method};
use serde_json::{Value, json};

use crate::trace::{LogRecord, SpanRecord, Telemetry};

/// A destination for telemetry.
pub trait Exporter: Send + Sync {
    /// A finished span.
    fn span(&self, service: &str, span: &SpanRecord);
    /// A log record.
    fn log(&self, service: &str, log: &LogRecord);
}

/// A shared exporter: keep an `Arc` to flush an [`OtlpExporter`] the tracer
/// also writes to.
impl<E: Exporter + ?Sized> Exporter for Arc<E> {
    fn span(&self, service: &str, span: &SpanRecord) {
        (**self).span(service, span);
    }
    fn log(&self, service: &str, log: &LogRecord) {
        (**self).log(service, log);
    }
}

/// One JSON object per line on standard output.
#[derive(Debug, Clone, Copy, Default)]
pub struct StdoutExporter;

impl Exporter for StdoutExporter {
    fn span(&self, service: &str, span: &SpanRecord) {
        println!("{}", json!({ "service": service, "span": span }));
    }
    fn log(&self, service: &str, log: &LogRecord) {
        println!("{}", json!({ "service": service, "log": log }));
    }
}

/// One JSON object per line, appended to a file.
pub struct FileExporter {
    file: Mutex<std::fs::File>,
}

impl FileExporter {
    /// Appends to `path`.
    ///
    /// # Errors
    ///
    /// The file cannot be opened.
    pub fn open(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self { file: Mutex::new(file) })
    }

    fn write(&self, value: &Value) {
        let mut file = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = writeln!(file, "{value}");
    }
}

impl Exporter for FileExporter {
    fn span(&self, service: &str, span: &SpanRecord) {
        self.write(&json!({ "service": service, "span": span }));
    }
    fn log(&self, service: &str, log: &LogRecord) {
        self.write(&json!({ "service": service, "log": log }));
    }
}

fn attributes(values: &std::collections::BTreeMap<String, Value>) -> Vec<Value> {
    values
        .iter()
        .map(|(key, value)| {
            let any = match value {
                Value::String(text) => json!({ "stringValue": text }),
                Value::Bool(flag) => json!({ "boolValue": flag }),
                Value::Number(number) if number.is_i64() || number.is_u64() => {
                    json!({ "intValue": number.to_string() })
                }
                Value::Number(number) => json!({ "doubleValue": number }),
                other => json!({ "stringValue": other.to_string() }),
            };
            json!({ "key": key, "value": any })
        })
        .collect()
}

/// The OTLP/HTTP JSON body for spans (`/v1/traces`).
#[must_use]
pub fn otlp_traces(service: &str, spans: &[SpanRecord]) -> Value {
    let spans: Vec<Value> = spans
        .iter()
        .map(|span| {
            json!({
                "traceId": span.trace_id,
                "spanId": span.span_id,
                "parentSpanId": span.parent_id.clone().unwrap_or_default(),
                "name": span.name,
                "kind": match span.kind { crate::SpanKind::Internal => 1, crate::SpanKind::Server => 2, crate::SpanKind::Client => 3 },
                "startTimeUnixNano": span.start_ns.to_string(),
                "endTimeUnixNano": span.end_ns.to_string(),
                "attributes": attributes(&span.attributes),
                "status": if span.error.is_some() { json!({ "code": 2, "message": span.error }) } else { json!({ "code": 1 }) },
            })
        })
        .collect();
    json!({ "resourceSpans": [{
        "resource": { "attributes": [{ "key": "service.name", "value": { "stringValue": service } }] },
        "scopeSpans": [{ "scope": { "name": "rustnative" }, "spans": spans }],
    }] })
}

/// Sends spans to an OpenTelemetry collector over OTLP/HTTP JSON, in
/// batches, and only with the person's consent ([`Telemetry`]).
pub struct OtlpExporter {
    http: Arc<dyn HttpService>,
    endpoint: String,
    telemetry: Telemetry,
    batch: Mutex<Vec<SpanRecord>>,
    size: usize,
}

impl OtlpExporter {
    /// Exports to `endpoint` (`https://collector:4318`) through `http`,
    /// `size` spans at a time.
    #[must_use]
    pub fn new(
        http: Arc<dyn HttpService>,
        endpoint: &str,
        telemetry: Telemetry,
        size: usize,
    ) -> Self {
        Self {
            http,
            endpoint: endpoint.trim_end_matches('/').to_owned(),
            telemetry,
            batch: Mutex::new(Vec::new()),
            size: size.max(1),
        }
    }

    /// Sends what is batched (call on shutdown).
    ///
    /// # Errors
    ///
    /// The collector could not be reached.
    pub async fn flush(&self, service: &str) -> Result<(), String> {
        let spans = std::mem::take(&mut *self.batch.lock().unwrap_or_else(PoisonError::into_inner));
        if spans.is_empty() || !self.telemetry.consented() {
            return Ok(());
        }
        let body = otlp_traces(service, &spans).to_string();
        let request = HttpRequest::new(Method::Post, format!("{}/v1/traces", self.endpoint))
            .header("content-type", "application/json")
            .body(body.into_bytes());
        self.http.execute(request).await.map(|_| ()).map_err(|error| error.to_string())
    }

    /// Whether a batch is full.
    #[must_use]
    pub fn ready(&self) -> bool {
        self.batch.lock().unwrap_or_else(PoisonError::into_inner).len() >= self.size
    }
}

impl Exporter for OtlpExporter {
    fn span(&self, _: &str, span: &SpanRecord) {
        if self.telemetry.consented() {
            self.batch.lock().unwrap_or_else(PoisonError::into_inner).push(span.clone());
        }
    }
    fn log(&self, _: &str, _: &LogRecord) {}
}
