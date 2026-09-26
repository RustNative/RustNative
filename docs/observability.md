# Observability, security, and compliance

`PLAN.md` Milestone 51. This guide covers:

- diagnosing a running application: traces, logs, metrics, and crash
  reports;
- limiting what its parts can reach: grants and the isolated worker;
- generating the evidence a reviewer asks for.

## Traces, logs, and metrics

`rustnative-observe` follows OpenTelemetry's model (`C70`) and its semantic
conventions (`rustnative_observe::conventions`).

```rust
let tracer = Tracer::new("notes", vec![Box::new(StdoutExporter)]);
let span = tracer.span("open notes", None).attribute(conventions::COMPONENT, "NoteList");
tracer.log(Level::Info, "loaded", &[("rows", 12.into())], Some(span.context()));
```

- **One trace across the boundary.** `TracedHttp` wraps any
  `HttpService`. It traces each request as a client span and sends its
  context as `traceparent`. On the server,
  `rustnative_observe::server::middleware(tracer)` makes each request a
  server span that continues that trace. A handler takes `RequestSpan` to
  start child spans.
- **Exporters.**
  - `StdoutExporter` and `FileExporter` write one JSON object per line.
  - `OtlpExporter` sends batches to an OpenTelemetry collector over
    OTLP/HTTP.
  - `MemoryExporter` is for tests.
- **Metrics.** `Metrics` holds counters and histograms, rendered in the
  Prometheus text format.

## Consent

Nothing leaves the machine unless the person agrees
(`docs/telemetry-policy.md`).

- **Off by default.** `Telemetry` starts off, and the application keeps
  the person's choice with `to_state` and `from_state`.
- **Enforced by the exporter.** `OtlpExporter` neither batches nor sends
  anything until consent is given, so a span recorded before the person
  agreed is never sent.

## Crash reports

On Windows, `rustnative_windows::crash::install(app_id, version)` writes a
report to `%LOCALAPPDATA%\<id>\crashes\` for a panic or an unhandled
exception. Each report holds:

- a minidump;
- the message;
- a backtrace, symbolicated in process when the PDB is beside the
  executable;
- the versions;
- **the UI tree as it stood**: the last rendered tree, as wire JSON.

`rustnative crash list` and `rustnative crash show <id>` read the reports.
Reports stay on the machine. Sending them is the application's choice,
under the same consent.

## Grants

`Services::scoped(grants)` gives one part of an application (a
third-party package, a plug-in) only what its `GrantSet` covers (`C68`).

- **Checked at every request.** HTTP is checked against the granted
  origins each time a request is made, not only when the service is
  handed out. A URL built at run time gets no further than a literal one.
- **Refused requests never leave.** They fail with "not granted".

## The isolated worker

`rustnative_windows::isolated::IsolatedWorker` runs untrusted or fragile
work in a sandboxed child process (`C67`).

- **The channel.** Messages are typed, one JSON message per line over
  pipes.
- **Low integrity.** The worker cannot write the person's files or
  settings.
- **A job object.** The job caps the worker's memory and denies it the
  clipboard, other windows, and system settings. It also ends the worker
  when its owner drops it.
- **Crashes.** A crash shows as the end of the channel. Restart the
  worker with `rustnative_durable::supervise`.

## Power loss

`FileStateStore` replaces files atomically with write-through. Each file
also carries a checksum, so a file torn by power loss reads as nothing
stored, never as a shorter value (`C80`). A test kills a process in the
middle of saving, and every value left behind is whole.

## Industrial services

`rustnative_core::industrial` defines `PrintService` and `SerialService`.
Windows implements them with the spooler (`WindowsPrinting`, which can also
print to a file through a document writer) and COM ports
(`WindowsSerial`).

## Accelerators

`Capability::Accelerator(AcceleratorKind::Gpu | Npu)` is answered from the
machine (`C89`):

- **GPU:** DXGI lists a hardware adapter.
- **NPU:** DXCore lists a machine-learning adapter that is not also a
  graphics adapter.

## Compliance evidence

`rustnative compliance` writes the evidence into `target/compliance/`,
generated from the build:

| File | Contents |
|---|---|
| `sbom.cdx.json` | CycloneDX 1.5 SBOM |
| `licenses.md` | Licenses |
| `dependency-inventory.json` | Dependency inventory |
| `privacy-manifest.json`, `permissions.md` | Privacy and permission manifests |
| `accessibility-report.json` | Accessibility results |
| `traceability.json` | Requirement traceability: each `// req: ID` comment above a test, mapped to that test |

See `docs/security/` for the threat models and the certification posture.
