//! Observability (`PLAN.md` Milestone 51): one trace spans the client and
//! the server; exporters honour consent; metrics render for Prometheus.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::sync::{Arc, Mutex};

use rustnative_core::{HttpRequest, HttpResponse, HttpService, ServiceError};
use rustnative_observe::memory::MemoryExporter;
use rustnative_observe::{
    Level, Metrics, OtlpExporter, SpanContext, SpanKind, Telemetry, TracedHttp, Tracer,
};
use rustnative_server::local::InProcess;
use rustnative_server::{ServerApp, get};

async fn hello() -> &'static str {
    "hello"
}

#[tokio::test]
async fn one_trace_spans_the_client_and_the_server() {
    let exporter = MemoryExporter::default();
    let server_tracer = Tracer::new("server", vec![Box::new(exporter.clone())]);
    let (before, after) = rustnative_observe::server::middleware(server_tracer);
    let service = ServerApp::new()
        .before(before)
        .after(after)
        .route("/hello", get(hello).public())
        .into_service();

    let client_tracer = Tracer::new("client", vec![Box::new(exporter.clone())]);
    let http = TracedHttp::new(InProcess(service), client_tracer.clone());
    {
        let screen = client_tracer.span("open notes", None);
        http.within(Some(screen.context().clone()));
        let response =
            http.execute(HttpRequest::get("https://api.example.com/hello?secret=1")).await.unwrap();
        assert_eq!(response.body_bytes(), b"hello");
        client_tracer.log(Level::Info, "loaded", &[("count", 1.into())], Some(screen.context()));
    }

    let spans = exporter.spans();
    let find = |kind: SpanKind| spans.iter().find(|span| span.kind == kind).unwrap();
    let (client, server) = (find(SpanKind::Client), find(SpanKind::Server));
    let screen = spans.iter().find(|span| span.name == "open notes").unwrap();
    assert_eq!(client.trace_id, screen.trace_id);
    assert_eq!(server.trace_id, client.trace_id, "one trace across the boundary");
    assert_eq!(server.parent_id.as_deref(), Some(client.span_id.as_str()));
    assert_eq!(client.name, "Get /hello", "no query string in a span name");
    assert_eq!(server.attributes["http.response.status_code"], 200);
    assert_eq!(exporter.logs()[0].trace_id.as_deref(), Some(screen.trace_id.as_str()));
}

#[test]
fn traceparent_round_trips_and_rejects_garbage() {
    let context = SpanContext {
        trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".into(),
        span_id: "00f067aa0ba902b7".into(),
    };
    assert_eq!(SpanContext::from_traceparent(&context.traceparent()), Some(context));
    assert_eq!(
        SpanContext::from_traceparent("00-00000000000000000000000000000000-00f067aa0ba902b7-01"),
        None
    );
    assert_eq!(SpanContext::from_traceparent("nonsense"), None);
}

struct Collector(Arc<Mutex<Vec<String>>>);

#[async_trait::async_trait]
impl HttpService for Collector {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ServiceError> {
        self.0.lock().unwrap().push(String::from_utf8(request.body_bytes().to_vec()).unwrap());
        Ok(HttpResponse::new(200, Vec::new(), Vec::new()))
    }
}

#[tokio::test]
async fn nothing_leaves_without_consent() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let telemetry = Telemetry::from_state(None);
    let otlp = Arc::new(OtlpExporter::new(
        Arc::new(Collector(Arc::clone(&sent))),
        "https://collector:4318",
        telemetry.clone(),
        10,
    ));
    let tracer = Tracer::new("app", vec![Box::new(Arc::clone(&otlp))]);
    drop(tracer.span("before consent", None));
    otlp.flush("app").await.unwrap();
    assert!(sent.lock().unwrap().is_empty(), "off by default");

    telemetry.set_consent(true);
    drop(tracer.span("after consent", None));
    otlp.flush("app").await.unwrap();
    let body = sent.lock().unwrap()[0].clone();
    assert!(
        body.contains("resourceSpans")
            && body.contains("after consent")
            && !body.contains("before consent"),
        "{body}"
    );
    assert!(Telemetry::from_state(Some(&telemetry.to_state())).consented());
}

#[test]
fn metrics_render_for_prometheus() {
    let metrics = Metrics::new();
    metrics.counter("renders_total").add(3);
    let latency = metrics.histogram("render_ms", &[1.0, 5.0, 16.0]);
    for value in [0.5, 4.0, 30.0] {
        latency.record(value);
    }
    let text = metrics.prometheus();
    assert!(text.contains("renders_total 3"));
    assert!(text.contains("render_ms_bucket{le=\"5\"} 2"));
    assert!(text.contains("render_ms_count 3"));
}
