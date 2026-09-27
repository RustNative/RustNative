# The server application model

`PLAN.md` Milestone 49. `rustnative-server` is built on the same contracts
as the client:

- Routes are `rustnative_core::Route` patterns.
- Work runs in a `RequestScope`, cancelled when the response is sent, just
  as a component's tasks are cancelled when it unmounts.
- Typed server functions and server-only components are defined in
  `rustnative-core`, so the Windows application calls the server with the
  same types it answers with.
- The application is a `tower::Service` over `http` types (`C37`). It is
  served by `hyper`, or mounted inside an existing service.

The example is three crates:

- `examples/notes-shared`: the definitions and the view.
- `examples/server-demo`: the server.
- `examples/server-client`: the Windows client.

## Routes and handlers

```rust
async fn user(Path(id): Path<u32>, State(db): State<Db>) -> Result<Json<User>, ServerError> { … }

ServerApp::new()
    .state(db)
    .route("/users/:id", get(user).signed_in::<Principal>())
```

- A handler is an `async fn`.
- Its parameters are extractors:
  - `Path`, `Query`, `Json`, and `Form`;
  - `State`, `Session`, `Principal`, and `RequestScope`;
  - `CsrfToken` and `CspNonce`.
- Its result is anything that implements `IntoResponse`.
- A parameter that does not parse is a `400` before the handler runs.
- Every route says who may use it: `.public()`, `.signed_in::<P>()`, or
  `.authorized::<P, Policy>()`. A route without one does not compile.
- Error pages are HTML or JSON, depending on the request's `Accept`.

## Security

Secure by default. See `docs/server/security-checklist.md`, which lists
each protection and the test that holds it.

## Authentication

| Piece | What it is |
|---|---|
| `Sessions` | Sessions in an AES-256-GCM sealed cookie |
| `password::hash` / `verify` | Argon2id |
| `TokenSigner` | HMAC-signed bearer tokens, for native clients |
| `Passkeys` | WebAuthn registration and sign-in, ES256 with `none` attestation (`C52-2`) |
| `OAuthClient` | Authorization code with PKCE, over the core `HttpService` |
| `Authentication::<P>` | Finds the principal from the session or a token |
| `Policy<P>` | A named rule that routes and the admin surface require |

## Data

| Piece | What it is |
|---|---|
| `Db` | A SQLite connection pool (bundled SQLite). `run` moves work to the blocking pool. |
| `Db::transaction(scope, …)` | Commits only while the request is still live |
| `query!("SELECT …", args)` | Checked against `migrations/*.up.sql` at compile time. The row type is inferred. |
| `Migrations` | Up and down migrations, applied in their own transactions, with `pending` as the dry run |
| `schema::diff`, `squash` | The next migration from `schema.toml`, with rename detection (`C38`) |
| `RowPolicies` | Row-level read policies with a fixture harness (`C39`) |

The CLI commands:

```sh
rustnative db diff add_email            # asks about possible renames
rustnative db diff add_email --rename users.name:display_name
rustnative db migrate app.db --dry-run
rustnative db migrate app.db
rustnative db rollback app.db --to 0001_users
rustnative db squash baseline
```

## Jobs

`Jobs` is a queue in the database:

- It survives a restart. Running jobs are queued again.
- An idempotency key makes an enqueue happen at most once.
- A failed job retries with exponential backoff, then is kept as `dead`
  with its error.
- `Schedule` supports cron-style recurring work.
- `ServerApp::inspection` answers the Milestone 44 protocol's `jobs`
  request at `POST /__inspect`, to a policy's principals.

## Server functions and server-only components

- **Server functions.** A `ServerFn` in a shared crate has a path, an input
  type, and an output type.
  - The server serves it with `server_fn` or `server_fn_with`.
  - A client calls it with `rustnative_core::server_fn::call` over any
    `HttpService`: `WinHttp` on Windows, or `InProcess` in tests.
- **Server-only components** (`C05`). A `ServerComponentDef` has a name
  and its props, which must be serializable.
  - Its rendering exists only in the server (`server_component`).
  - Its output is a `rustnative_core::wire::WireNode`. The client turns it
    back into a `Node` and merges it with the ordinary reconciler.
  - Server-only code is out of the client's reach: the shared crate does
    not depend on the server crate, so reaching for it is a compile error.

## The API schema

- Server functions carry `ApiSchema` types. `openapi()` serves an
  OpenAPI 3.1 document at `/openapi.json`.
- `breaking_changes(published, current)` is the contract check. The notes
  server's test fails the build if `api/v1.json` would break.
- `typescript_client` generates a typed client for other languages.

## Web output

| Piece | What it is |
|---|---|
| `Head` | Title, description, canonical address, social card, and JSON-LD, with `validate()` for the build (`C41-1`) |
| `Sitemap` | A sitemap of the site's pages |
| `render::page` | A component tree as a document, styled under the strict CSP. The notes server serves the Windows client's own view this way. |

## Push

- **Web Push** (`Vapid`): the payload is encrypted with `aes128gcm`, and
  the request carries a VAPID ES256 token.
- **WNS, APNs, and FCM**: request builders for each service's documented
  format (`C54-1`).

## Configuration and operations

- **Configuration.** `Config` layers defaults, a TOML file, the
  environment, and a secrets directory, then validates the result into the
  application's own type. There is no global. `Secret<T>` never prints.
- **Health and metrics.** `/healthz`, `/readyz` (with `readiness` checks),
  and `/metrics` in Prometheus text. The `health` and `metrics` features
  control them.
- **Report** (`C40`). `ServerApp::report()` lists what was configured and
  why. The notes server prints it at start.

## Mounting

`ServerApp::prefix("/app")`, then `into_service()`, gives a
`tower::Service`. An existing hyper or tower service can hand it `/app/*`
on its own listener, as `it_mounts_inside_an_existing_service` shows.

## Owed

- **Web track: delivered.** Browser interactivity for server-rendered pages
  (islands), the web client half of server functions, and the serverless
  shapes (a function and an edge module, `serverless::run`) are built:
  `docs/web.md`, `docs/web/deploy.md`. `examples/web-notes` is this
  milestone's done-when in every shape.
- **Live push.** Live sending to WNS, APNs, and FCM needs credentials.
