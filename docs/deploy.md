# Deployment, updates, and fleet operations

`PLAN.md` Milestone 50. One application ships in several shapes:

- a Windows desktop package (a ZIP or MSIX);
- a long-lived server;
- a container description.

After it is deployed, it updates.

## The adapter contract

`rustnative_server::deploy::DeploymentAdapter` covers:

- `deploy` of an immutable `Revision`, which starts as a preview with no
  traffic;
- `promote` to a percentage of traffic;
- `rollback`;
- `status`;
- `limits`: what the host allows each request (deadline, memory, file
  system, payload).

Request scopes end with the response on every target, as Milestone 49
already guarantees.

## A long-lived server

```sh
rustnative deploy local start --port 8080 --control 8081   # the proxy
rustnative deploy local add r1 127.0.0.1:9001              # revision r1 takes all traffic
rustnative deploy local add r2 127.0.0.1:9002              # r2: a preview (header x-revision: r2)
rustnative deploy local promote r2 --percent 10            # 10% of clients
rustnative deploy local promote r2 --percent 100
rustnative deploy local rollback                           # back to r1
rustnative deploy local status
```

- **The proxy.** The `TrafficSplitter` is a reverse proxy built on
  `rustnative-server`. It sends each client to a revision by a stable hash
  of the client's address, so a client stays on its revision. It marks
  each response with `x-served-by`.
- **Moving traffic.** Promotions and rollbacks move traffic without
  restarting anything.
- **Revisions are immutable.** A name used once cannot be deployed again.
- **Client addresses.** Behind the proxy, every request reaches a revision
  from the proxy's address. The proxy sends the client's address in
  `x-forwarded-for`, replacing any the client sent, and a revision believes
  it because the proxy is a trusted one: loopback is trusted by default
  (`Security::trusted_proxies`), so each revision's rate limit counts
  clients, not the proxy. Behind another proxy or load balancer, add its
  addresses to `trusted_proxies`.

## Container and infrastructure descriptions

```sh
rustnative deploy export container    # Dockerfile (distroless, non-root, /data a volume)
rustnative deploy export compose      # read-only root, the state on a named volume
rustnative deploy export kubernetes   # Deployment + PersistentVolumeClaim + Service, probes on /healthz and /readyz
rustnative deploy export systemd      # the system read-only to the service, its state in StateDirectory
```

- **Output.** The descriptions are written to `deploy/`.
- **Least privilege.** Each description asks only for what the
  application declared: the port, the environment variables in
  `rustnative.toml` `[resources]`, a read-only root file system, and a
  non-root user.
- **One writable place.** The application's state — the SQLite database,
  the durable job queue, workflows, actors (`db` is a default feature) —
  lives in one directory each description makes writable: `/data` on a
  volume in a container, `/var/lib/<name>` (`StateDirectory`) under
  systemd. It is the working directory, so a relative path such as
  `app.db` lands there, and `RUSTNATIVE_DATA_DIR` names it
  (`rustnative_server::data_dir()`).
- **One instance.** That state belongs to one instance, so the
  descriptions run one (`replicas: 1`, a `Recreate` strategy for the
  volume). Move the state to a database the instances share before
  scaling out; the response cache, rate limiter, and server-interactive
  sessions are per instance too.
- **Pinned images.** Images are tagged with `rustnative.toml`'s version,
  never `latest`, so a revision is what its name says.
- **Building the image.** This needs Docker. `rustnative deploy export
  container --build` runs `docker build` when Docker is installed, and
  says so when it is not.

## Single artifact

A build script calls `rustnative_build::embed_assets("assets")`, and the
server serves the result with `ServerApp::assets(ASSETS)`.

- **Names.** Each file is named by its content hash and served with
  `cache-control: immutable`.
- **Integrity.** `assets::stylesheet` and `assets::script` write the link
  with its subresource-integrity digest, and the script also carries the
  response's CSP nonce.
- **One file.** The binary is the whole deployment.

## Response caching

A route declares `get(page).public().cached(&["notes"], ttl)`, and
`ServerApp::response_cache().invalidate("notes")` regenerates every page
tagged `notes`. `cache_inspection::<P, Policy>()` serves the cache's state
at `/__cache`.

- **Public only.** Only public routes are cached.
- **No personal data.** A response that sets a cookie, or says it must not
  be shared (`Cache-Control: no-store` or `private`), is never stored. A
  rendered page is `no-store` — it carries its request's CSP nonce and
  request-forgery token — so a page route marked `cached` is rendered per
  request; a page's shareable part is cached as its prerendered shell
  (`Strategy::PartiallyPrerendered`). What the cache stores are the other
  public responses: JSON, feeds, generated assets.
- **Varies by language and format.** Entries are keyed by path, query,
  `Accept-Language`, and `Accept`, so one visitor's language is never served
  to another.
- **Bounded.** At most `ResponseCache::DEFAULT_CAPACITY` entries (or
  `ResponseCache::with_capacity`); past that, the entry nearest its expiry
  goes first.

## Desktop updates

See `docs/deploy/update-rules.md` for what each host permits.

In `rustnative.toml`:

```toml
[update]
public-key = "…"         # rustnative update keygen prints it
```

The CLI commands:

```sh
rustnative update keygen --out publisher.key
rustnative update manifest --version 1.1.0 --url https://…/demo-1.1.0.zip \
    --package target/package/demo-1.1.0.zip --rollout 10 --expires-in-days 30 \
    --key publisher.key > latest.json
```

`rustnative update manifest` refuses a key whose public half is not the
`[update] public-key` in `rustnative.toml`, so a release cannot be signed
with a key the installed application does not trust.

The manifest format is `rustnative_update`'s, the same on every host that
updates itself. What is published is an envelope — the manifest's JSON as a
string, and an Ed25519 signature over exactly those bytes — so nothing is
re-serialized to be verified, and a field a later release adds does not break
an older installation. A verified manifest is still refused when it names
another application id than the installation's, or when it is past its
`expires` time (default 30 days): a stale manifest served by a compromised
mirror stops being believed, so republish `latest.json` before it expires.
Versions compare numerically with pre-releases before their release
(`1.3.0-beta.2` < `1.3.0`).

An MSIX installed from a web page or a share updates through App Installer
instead: `rustnative package windows --format msix --appinstaller <url>`
also writes the `.appinstaller` file, which points App Installer at `<url>`
and asks it to check on every launch.

In the application: `Updater::check` reads the manifest, and `stage`
verifies the package and unpacks it beside the running version. `activate`
switches versions atomically. `Launcher` starts the current version and
rolls back one that fails twice before `interactive`.

## Generated native project files

`[package] capabilities = ["internetClient", "webcam"]` in
`rustnative.toml` goes into the MSIX manifest. A capability package
(Milestone 52) declares its own capabilities there the same way (`C63`).

## Build cache

`rustnative build windows --cache` builds through `sccache` when it is
installed (cargo's `build.rustc-wrapper`), so local and CI builds share
compiled crates. It says so and builds without the cache when `sccache` is
not installed (`C64`). Remote build and signing are owed. A remote build
would not count as a verification.

## Owed

- **Other targets: delivered for the Web.** Static-host, function, and edge
  targets (`deploy local start --target static|function|edge`), their
  emulators (`serve static`, `serve lambda`, `serve wagi`), and provider
  descriptions (`deploy export sam`, `deploy export spin`):
  `docs/web/deploy.md`. A real provider account is not exercised. Mobile
  store packs and firmware images are owed with Milestones 35–37 and 70.
- **Web loading path** (`C42`): delivered with Web milestone J
  (`docs/web.md`, `budgets/web.toml`).
