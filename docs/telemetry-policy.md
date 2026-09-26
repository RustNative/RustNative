# Telemetry policy

`PLAN.md` Milestone 51.

- **Off by default.** Nothing leaves the person's machine until they agree.
  `rustnative_observe::Telemetry` starts without consent, and the
  application stores the person's choice (`to_state` / `from_state`).
- **What may be sent, with consent:** spans (names, durations, status, and
  the attributes the semantic conventions define), metrics, and crash
  reports. Span names carry routes and paths, never query strings.
- **What is never sent:** secrets (`Secret<T>` serializes as
  `[redacted]`), request and response bodies, text the person typed, and
  file contents. The same redaction rules apply to inspection recordings
  (Milestone 44).
- **Where it goes:** an OpenTelemetry collector the application's publisher
  names (`OtlpExporter`); there is no framework-operated endpoint.
- **Withdrawing consent** stops sending at once; batched data not yet sent
  is dropped.
