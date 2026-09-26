//! Observability (`PLAN.md` Milestone 51): structured logs, metrics, and
//! traces in an OpenTelemetry-shaped model (`C70`), exported as JSON to
//! stdout, a file, or an OTLP/HTTP collector — and one trace across the
//! client and the server, through the `traceparent` header.
//!
//! ```
//! use rustnative_observe::{Level, Telemetry, Tracer, memory::MemoryExporter};
//!
//! let exporter = MemoryExporter::default();
//! let tracer = Tracer::new("notes", vec![Box::new(exporter.clone())]);
//! {
//!     let span = tracer.span("render", None).attribute("rustnative.component", "NoteList");
//!     tracer.log(Level::Info, "rendered", &[("rows", 12.into())], Some(span.context()));
//! }
//! assert_eq!(exporter.spans()[0].name, "render");
//! assert_eq!(exporter.logs()[0].trace_id.as_ref(), Some(&exporter.spans()[0].trace_id));
//! # let _ = Telemetry::default();
//! ```
//!
//! **Consent.** Data that leaves the machine is opt-in: [`Telemetry`]
//! starts off, remembers the person's choice, and an OTLP exporter sends
//! nothing without it (`docs/telemetry-policy.md`).

pub mod conventions;
pub mod export;
pub mod memory;
pub mod metrics;
#[cfg(feature = "server")]
pub mod server;
pub mod trace;

pub use export::{Exporter, FileExporter, OtlpExporter, StdoutExporter};
pub use metrics::{Counter, Histogram, Metrics};
pub use trace::{
    ActiveSpan, Level, LogRecord, SpanContext, SpanKind, SpanRecord, Telemetry, TracedHttp, Tracer,
};
