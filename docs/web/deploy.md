# Deploying a web application: static, server, function, edge

`PLAN.md` Web milestone K. One application, unchanged, deploys in four
shapes. What differs is how long an instance lives and what its host allows.

| Shape | Build | An instance lives | Run it locally |
|---|---|---|---|
| **Static** (client-side) | `rustnative build web --mode client` → `target/web/client` | — (files) | `rustnative serve static target/web/client` |
| **Server** | `rustnative build web --mode server`, `rustnative package web` | as long as the process | `rustnative run web`, `rustnative dev web` |
| **Function** (AWS Lambda's runtime API) | `rustnative build web --mode serverless --host lambda` → `target/web/lambda/bootstrap` | one invocation at a time, reused while warm | `rustnative serve lambda <binary>` |
| **Edge** (WAGI: CGI over WASI) | `rustnative build web --mode serverless --host wagi` → `target/web/wagi/<name>.wasm` | one request | `rustnative serve wagi <module>` |

`examples/web-notes` is the application in all four
(`crates/rustnative/tests/serverless.rs`): signed in, backed by SQLite,
indexing each new note in a background job, and — in the serverless shapes —
reaching its data through the server's data service over HTTP, because
SQLite is not available in an edge sandbox.

## The serverless entry point

```rust
fn main() -> std::process::ExitCode {
    rustnative_server::serverless::run(my_app::app())
}
```

`run` reads the environment. With `AWS_LAMBDA_RUNTIME_API` it takes
invocations from the function runtime; with `REQUEST_METHOD` it answers one
WAGI request. Both call the same `AppService::handle` as the server does, so
routing, security headers, sessions, CSRF, server functions, and pages
behave identically.

- **Function** (`serverless::lambda`): API Gateway v2 and function-URL
  events become requests; cookies, base64 bodies, and multi-value headers
  round-trip. An invocation whose deadline passed before it started is
  refused as `DeadlineExceeded`. Other events (a queue's batch) go to an
  `EventHook`. For SQS use `rustnative_durable::events::sqs_batch(handler)`,
  which answers with `batchItemFailures`, so only the failed records are
  redelivered (`C44`).
- **Edge** (`serverless::wagi`): the request comes from the CGI variables
  and standard input, and the response goes to standard output.

### Stateless by construction

- Each invocation runs on a runtime of its own, which is dropped at the
  response. Work the invocation spawned and left running is cancelled then
  (`tests/serverless.rs::work_an_invocation_spawned_does_not_outlive_it`).
- Pages render on the invocation's thread. A streamed page is answered whole,
  because neither shape streams.
- An instance never rendered the page that asks for a client module, so the
  application declares each client component its pages use:
  `ServerApp::client::<T>()`. Without the declaration, the edge shape answers
  404 for the module. The equivalence test caught this.
- Sessions are sealed in a cookie with one key (`NOTES_SECRET_KEY` in the
  example). Any instance of any shape reads them; nothing is kept in an
  instance.
- Configuration and secrets come from the environment per invocation. In the
  edge shape the host sets them (`serve wagi --env NAME=VALUE`).
- **Images** that pages register while rendering (`ImageStore`) have the
  same problem as modules. In the serverless shapes, serve images from the
  static export's files or a content network rather than through the
  application.

### Host limits as capabilities (`W-SL-2`)

What the host allows arrives in the request as `HostLimits`, so a page can
adapt. A long report can stream only its shell, or refuse a large upload
early.

```rust
let limits = rustnative_web::request(context).map(|r| r.limits().clone());
```

| | deadline | memory | file system | response | payload |
|---|---|---|---|---|---|
| function | the invocation's remaining time | `AWS_LAMBDA_FUNCTION_MEMORY_SIZE` | `/tmp`, this instance only | 6 MB | 6 MB |
| edge | `RUSTNATIVE_LIMIT_DEADLINE_MS` | `RUSTNATIVE_LIMIT_MEMORY_BYTES` | none | `RUSTNATIVE_LIMIT_RESPONSE_BYTES` | `RUSTNATIVE_LIMIT_PAYLOAD_BYTES` |
| server | none | none | read-write | none | the body limit |

### Outbound HTTP and storage

- `serverless::outbound::send` makes a plain HTTP request. Natively it goes
  over a socket. On the edge it goes through the host's `rn_http` import,
  and only to hosts the operator allows (`--allow-http host:port`), as an
  edge platform's outbound allow-list works.
- `serverless::kv` is the edge host's key-value store (`rn_kv`: `get`,
  `set`, `delete`, `list`).

These two imports are this framework's own, and so is the emulator that
provides them. A provider's WAGI executor offers neither; there, a module
that uses them needs the provider's HTTP and key-value interfaces bridged
to the same four operations. That bridge is owed. A module that uses
neither runs on any WAGI host as it is.

## The emulators

`rustnative serve lambda <binary>` runs the function behind an emulated
runtime API, with an HTTP front that turns each request into an API
Gateway v2 event:

- `--memory-mb` (reported, not enforced);
- `--timeout`: an invocation not answered in time is `504`, and the instance
  is replaced;
- `POST /2015-03-31/functions/function/invocations` invokes the function with
  a raw event, which is how a queue batch is tested.

`rustnative serve wagi <module>` instantiates the module per request, in
an interpreter (`wasmi`), and enforces what an edge host enforces:

- **CPU in fuel.** `--fuel` sets the budget for every route, and
  `--route-fuel PREFIX=FUEL` sets one for a prefix. Out of fuel is `503`,
  `x-rn-limit: fuel`. Every response reports what it used in `x-rn-fuel`.
- **Memory.** `--memory-mb` is a ceiling on linear memory. A request that
  grows past it is `503`, `x-rn-limit: memory`.
- **Response size.** A response over `--response-kb` is `502`.
- **The deadline** bounds the module's sleeps.
- **Actors.** `--actors /actors/` runs requests for one actor id one at a
  time.

A route over its fuel budget has two ways forward:

- Serve it partially prerendered (`.strategy(Strategy::Streamed).partial()`).
  The shell is cached, and the expensive boundary is filled from the
  long-lived server or a function.
- Move the route to a function, where CPU is billed by time rather than
  capped.

`budgets/serverless.toml` holds the sign-in route's budget. `rustnative
bench --target serverless` measures both shapes' artifact size, cold and warm
latency, and memory.

## Edge actors (`C46`)

The actor contract (`rustnative_durable::Actor`: an identity, one message
at a time, private storage, alarms) runs in two places:

- the local actor system (SQLite);
- an edge host: `rustnative_durable::edge::serve::<A>("/actors/")` in a
  `wasm32-wasip1` module. Each request is an instance, the host routes one
  actor id to one instance at a time, storage is `rn_kv`
  (`actor/{id}/{key}`, `alarm/{id}`), and the host's scheduler calls
  `POST /actors/{id}/alarm`.

`examples/edge-actors` runs one collaborative document under both:
`tests/local.rs`, and `crates/rustnative/tests/serverless.rs` on the edge.

The emulator has no alarm scheduler of its own. On a provider, its
per-object alarm calls the same path.

## Deploying: preview, promotion, rollback

`rustnative deploy local start --target static|function|edge|server` puts
the traffic splitter (`docs/deploy.md`) in front of revisions. Each revision
is `serve static`, `serve lambda`, or `serve wagi` (or a server) on its own
port. A revision is previewed by name (`x-revision`), promoted by
percentage, and rolled back without a restart. `GET /target` on the control
port states the target's limits
(`tests/serverless.rs::a_function_deployment_previews_promotes_and_rolls_back`).

For a provider:

- `rustnative deploy export sam` writes `deploy/template.yaml`: an AWS SAM
  template that runs the function on `provided.al2023` behind an HTTP API.
  Its `live` alias shifts traffic as a canary (`Canary10Percent5Minutes`),
  which is the provider's own preview, promotion, and rollback. Secrets are
  `NoEcho` parameters.
- `rustnative deploy export spin` writes `deploy/spin.toml`: a Spin manifest
  that runs the module under the WAGI executor on every route, with outbound
  hosts and secrets as variables.
- The static export's `_headers` is what static hosts such as Netlify and
  Cloudflare Pages read.

The function runs on Linux, so its binary must be built for Linux
(`--target x86_64-unknown-linux-musl`, or on a Linux machine). The template
expects `bootstrap` in `target/web/lambda`.

## Equivalence (`W-MF-5`)

One application, in the same state, loaded in Edge from the static export,
the server, the Lambda emulator, and the edge emulator, has an identical
page DOM in each, before and after the same interactions
(`crates/rustnative/tests/serverless.rs::the_ui_is_the_same_in_every_mode`).
The same client component as generated JavaScript and as WebAssembly has an
identical DOM after the same events
(`crates/rustnative-server/tests/wasm_browser.rs`).
