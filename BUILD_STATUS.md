# Build Status — RustNative

## Current work: Milestones 39–58 on the Windows backend

Scope, as decided on 2026-09-23 and recorded in
`docs/superpowers/plans/2026-09-23-milestones-39-58.md`: every remaining
milestone, every tier, with the exception of backends other than Windows
(Milestones 33–38 and Web milestones A–K). Where a milestone item can only be
*realized* on one of those backends, its portable contract and its test double
are built here and the realization is listed below as **owed** by the backend
milestone that will do it. The shipped backends are Windows and the headless
reference backend (Milestone 45); a "done when" that names several backends is
met on those two, and its other half is listed as owed.

**Crate rename (2026-09-27).** The crates are now named `rustnative-*`, to
match the project name. Before this, they used the working name
`framework-*` (for example `framework-core` and `framework-windows`), and
their Rust paths used `framework_*`. Every entry in this file now uses the new
names, including entries for milestones built before the rename, so crate
names in commits before `c6366fc` will not match. Generic uses of the word are
unchanged, such as "framework-managed" and the CLI's `--framework-path` flag.
Projects created by `rustnative new` now depend on the `rustnative-*` crates.
One markup expansion golden was re-blessed: the longer crate path moves one
call past the pretty-printer's line width.

<!-- milestone entries, newest first -->

### Milestone 52 — The project around the framework — complete for Windows (device end owed)

With this entry, every milestone and tier in `PLAN.md` as it stood on
2026-09-26 is built on the Windows backend, except the other backends
themselves. The following work is owed, and each milestone's own entry names
the parts it owes:

- Milestones 33–38: macOS, Linux, Android, iOS, embedded, and terminal.
- The Web track's milestones A–K.
- **Milestones 59–66**, added to `PLAN.md` on 2026-09-27 as the ecosystem
  analysis was completed: 59 the toolchain and service layer, 60 the visual
  pipeline, 61 interaction depth, 62 media, capture, and files, 63 trust and
  compliance, 64 product operations, 65 engineering discipline, and 66 reach.
  Nothing of them is built. They come from the concept catalogue's last five
  documents (`docs/ecosystem-analysis/concepts-toolchain.md`,
  `concepts-surface.md`, `concepts-product.md`, `concepts-discipline.md`, and
  `concepts-reach.md`).

  On Milestone 59 specifically:
  Milestones 43, 50, and 52 built the parts of that layer listed in their own
  entries; what 59 adds — the release train and supported-host matrix, project
  diagnostics beyond toolchain installation, build profiles and managed
  credentials, store submission, update channels with fingerprint gating and an
  embedded fallback, repository-declared pipelines, native-module authoring
  with autolinking, universal route derivation, generated launch assets, the
  playground, device pairing, the eject/diff path, and artifact attribution —
  is not.

**Built.**

- **Stability.**
  - `docs/policy/stability.md` sets out semantic versioning across the
    crates, a deprecation window of two minor releases, the MSRV policy,
    and the rule that a breaking change ships with a codemod.
  - `rustnative upgrade --from <version> [--dry-run]` runs the codemods.
    They are syntax-aware and rewrite exactly the affected spans in place,
    leaving formatting and comments untouched. Where a codemod cannot be
    certain, it reports the position instead of guessing.
  - The first codemod converts `EdgeInsets` from `left`/`right` to
    `start`/`end` (Milestone 39). It is tested against a corpus in
    `crates/rustnative/tests/codemod-corpus/`.
- **Capability packages (`C71`).**
  - `rustnative_core::package` adds `CapabilityPackage` and
    `PackageManifest` (backends, framework version range, grants), with
    Cargo-compatible version matching.
  - `Services::install(&package, backend)` checks all three, builds the
    service from a scope holding only the declared grants, and keeps it
    under the package's name. Nothing is registered globally.
  - `rustnative add <path|name>` reads the package's
    `[package.metadata.rustnative]`, refuses an incompatible package with
    the reason, and adds the dependency.
  - `rustnative search` reads the index, `docs/packages/index.json` or
    `--index`.
  - `examples/package-battery` is a third-party-style package: a portable
    `BatteryService`, with Windows code (`GetSystemPowerStatus`) and
    headless code.
- **Feature kits (`C57-3`).** `rustnative generate kit auth|admin|commerce`
  writes working code on the server model.
  - `auth`: Argon2id passwords, sealed sessions, CSRF on every change,
    and the first account as administrator.
  - `admin`: the generated admin surface, for administrators only.
  - `commerce`: a catalogue, and a checkout that validates each receipt on
    the server before recording an entitlement.
  - `examples/kits` compiles and tests the templates themselves.
- **`rustnative describe --json`.** It lists:
  - 24 markup elements, each attribute with the builder method it calls;
  - 818 utility classes, with the properties each one sets, and the
    variants;
  - 46 capabilities (`Capability::ALL`, which inspection now reports
    against too), 41 events (`EVENT_NAMES`), the services, the component
    contract, and the layout semantics.
  - `docs/api/framework.json` is its committed output.
- **Documents.**
  - Measured core costs: `docs/performance/core-costs.md`.
  - The change-detection contract (`C03-1`):
    `docs/policy/change-detection.md`.
  - The embedded non-duplication policy: `docs/policy/embedded.md`.
  - Guides: an index mapping each task to its guide and a runnable
    example (`docs/guides/README.md`), and a first-application guide in
    both syntaxes and both style spellings.
  - An API reference note: `docs/api/README.md`.
  - New entries in the rejected-concepts list (`C72`).
- **The span example.** `examples/span` is one thermostat application that
  runs in a desktop window and at a device's 240×320 screen.

**Verified.** Full gate.
- **Codemods.** The corpus cases rewrite exactly as expected, with the
  expected number of places reported by hand. A second upgrade changes
  nothing more. End to end, `upgrade` with `--dry-run` writes nothing,
  and then rewrites in place.
- **Packages.**
  - The package installs only on the backends it declares, and nothing
    leaks into other `Services`.
  - Its `Cargo.toml` metadata matches its manifest.
  - `add` accepts the sample, refuses an Android-only package with the
    reason, and does not add a dependency twice. `search` finds the
    sample.
- **Kits.**
  - Sign-up, with validation and a unique name, then sign-out and sign-in,
    with the same answer for a wrong password as for an unknown name.
  - The admin surface answers 200 for the first account and 403 for the
    second.
  - A checkout is validated, an unknown product is refused, and
    entitlements are kept per buyer.
  - `generate kit` refuses a project without the server model, and
    refuses `admin` before `auth`.
- **Describe.**
  - The committed description is current, or a test fails.
  - The event list equals the `Event` enum, parsed from its source.
- **Core costs.**
  - Heap per node is checked against the document on every run: 1241 B
    for the tree and 2367 B for its snapshot.
  - The stack each chain depth needs was measured by a child-process
    binary search, down to 4 KiB: 67 KiB at depth 100 and 579 KiB at depth
    1,000.
  - Worst-case times were recorded in a release build.
- **The span example:** the same behaviour, and every control fits, at
  both screen sizes.
- **Doc parity:** the new guides show every tree in both syntaxes and
  both spellings.

**Not verified / owed.**
- **The device end of the span, and the embedded obligations** (Milestone
  37): the executor adapter on hardware, `embedded-hal` peripherals,
  `probe-rs`, board metadata (`C73`), and the RTOS adapter (`C79`).
- **A published crates.io index and a hosted API reference.** The index
  and the reference are generated and committed, not hosted.
- **Codemods for `.rsx` files and `rsx!` bodies.** The first codemod
  touches Rust syntax only. No markup attribute changed in that release.

### Milestone 57 — Surfaces beyond the main window, and product services — complete for Windows

**Built.** Guide: `docs/surfaces.md`.

- **Surfaces.** `rustnative_core::surfaces::Surfaces`, in every
  application's services, asks for the tray icon and its menu, a jump
  list, taskbar progress, and notifications. They answer with the new
  `Event::SurfaceAction`, routed to the window's root component like a
  menu choice.
  - **Tray:** `Shell_NotifyIconW` on the primary window, with version-4
    callbacks.
  - **Menu:** `TrackPopupMenu`; a choice arrives as its item's id.
  - **Clicks:** the icon arrives as `"activate"`, and the notification
    balloon as `"notification"`.
  - **Jump list:** `ICustomDestinationList` tasks with titles.
  - **Taskbar progress:** `ITaskbarList3`.
  - The icon is removed with its window.
  - `Capability::Surface(TrayExtra | JumpList | TaskbarProgress)` is now
    advertised. The remaining surfaces are answered as unavailable, with
    the reasons in the guide.
- **Product services** (`rustnative_core::product`).
  - **`SecureStorage`:** stated traits (hardware backing, biometric
    gating), an in-memory store, and `WindowsSecureStorage` (Credential
    Manager, with DPAPI sealing for the user).
  - **`Flags`:** typed flags with compiled defaults, remote
    configuration, local overrides (`RUSTNATIVE_FLAGS`), a cache for
    offline use, a version counter, and `refresh` over `HttpService`.
  - **`PushService` and `CommerceService`:** the contracts, with
    `FakeStore` for development.
  - `WindowsPush` and `WindowsStore` answer `Unavailable` with the reason:
    WNS and Store billing need Store-associated package identity.
  - `Services` carries the surfaces, flags, secure storage, push, and
    commerce.
- **The example.** `examples/product-services` uses a tray menu, a jump
  list, export progress on the taskbar, a notification whose click
  returns, a token in secure storage, and a remotely toggled layout, with
  no native code of its own.

**Verified.** Full gate.
- **A native harness test:**
  - the shell knows the tray icon after the application asks for it;
  - the icon click, the notification click, and a menu choice each arrive
    as `SurfaceAction`;
  - the shell accepts the jump list (and an empty one);
  - progress clears;
  - the icon is gone after the window closes.
- **Secure storage:** a secret round-trips, the credential holds only its
  DPAPI-sealed form, and deletion is idempotent.
- **Unavailable services:** `WindowsPush` and `WindowsStore` state why.
- **Capabilities:** the platform advertises the three realized surfaces
  and refuses the rest.
- **The example, on the headless backend:**
  - the surface requests, in order;
  - the tray action does what the button does;
  - the return from the notification;
  - the token persists across a restart;
  - the remote flag switches the layout.
- **Flags:** a doctest.

**Not verified / owed.**
- **Needs MSIX package identity:** multi-button toasts, WNS push,
  Store billing, Windows 11 widgets, and share targets. Real-store receipt
  validation on the server comes with Store billing.
- **The mobile reference application in the plan's done-when:** a widget,
  a share extension, push with actions, and a purchase. It is owed with
  Milestones 35 and 36.
- **Hardware-backed and biometric-gated secure storage** (Windows Hello
  key credentials).

### Milestone 51 — Observability, security, and compliance — complete for Windows and the server

**Built.** Guide: `docs/observability.md`. Also written:
`docs/security/threat-model-windows.md`,
`docs/security/threat-model-server.md`,
`docs/security/certification-posture.md`, and `docs/telemetry-policy.md`.

- **`rustnative-observe`** follows OpenTelemetry's model (`C70`).
  - Spans, structured logs tied to spans, semantic conventions, counters
    and histograms, and output in the Prometheus text format.
  - Exporters write to stdout, a file, OTLP/HTTP (batched), or memory.
  - `TracedHttp` sends `traceparent`, and the server middleware continues
    that trace, so one trace spans the client and the server.
  - The OTLP exporter holds and sends nothing until the person consents
    (`Telemetry` is off by default, and the choice persists).
- **Crash capture on Windows** (`rustnative_windows::crash`).
  - Handles panics and unhandled exceptions.
  - Writes a minidump (`MiniDumpWriteDump`) and a JSON report.
  - The report carries the message, a backtrace symbolicated in process,
    the versions, and the UI tree as last rendered (wire JSON, kept after
    each render while capture is on).
  - `rustnative crash list|show` reads the reports.
- **Grants enforced at each call** (`C68`): a scoped `HttpService`
  refuses origins outside its grant when each request is made, so
  refused requests never leave.
- **The isolated worker** (`C67`, `rustnative_windows::isolated`).
  - Runs with a low-integrity token.
  - A job object caps its memory, withholds UI access, and kills it when
    its owner drops it.
  - It talks to the owner in typed JSON messages over pipes.
- **Power loss** (`C80`). `FileStateStore` files now lead with a magic
  number and an FNV-1a checksum, so a torn file or a flipped bit reads as
  nothing stored. Files in the old format are still read.
- **Industrial services.**
  - `rustnative_core::industrial::{PrintService, SerialService}` are the
    contracts.
  - `WindowsPrinting` prints through the spooler and can print to a file.
  - `WindowsSerial` opens COM ports through the communications API.
  - `Capability::Printing` and `Capability::SerialPorts` report them.
- **Accelerators as capability answers** (`C89`).
  `Capability::Accelerator(Gpu | Npu)` is answered from the machine: DXGI
  hardware adapters for a GPU, and DXCore machine-learning adapters for an
  NPU (DXCore is loaded at run time).
- **`rustnative compliance`** generates, from the build:
  - a CycloneDX 1.5 SBOM;
  - licenses and a dependency inventory;
  - privacy and permission manifests;
  - an accessibility report;
  - requirement traceability, from `// req: ID` annotations.
- **Threat models** for Windows applications and for servers, a
  telemetry policy, and a certification posture.

**Verified.** Full gate.
- **Tracing:**
  - one trace across an in-process client and server, with correct
    parenting and no query string in span names;
  - `traceparent` parsing rejects garbage;
  - consent: a span recorded before consent is never sent, and one
    recorded after is sent;
  - Prometheus output.
- **Crash capture:** real child processes that panic, and one that
  dereferences null, each leave a report and a non-empty dump. The panic's
  report carries the tree and a symbolicated backtrace.
- **The isolated worker:** a round trip over its channel works; a write to
  a medium-integrity folder is denied; dropping the worker kills its
  process.
- **Grants:** a request to an origin outside the grant is refused and
  never reaches the service; a look-alike host is refused.
- **Power loss:**
  - every truncation of a state file reads as nothing stored;
  - a flipped bit is caught, and legacy files still read;
  - a process killed mid-save 12 times always leaves a whole value;
  - on its first run, the truncation test found that the first design (a
    trailing checksum) let a file cut short pass as a legacy file, and the
    checksum was moved to the front.
- **Printing:** to a file through the XPS Document Writer where it is
  installed.
- **Serial:** a missing port is an error.
- **Capabilities:** the platform's accelerator answers match what the
  machine reports.
- **Compliance:** the SBOM and the traceability mapping.

**Not verified / owed.**
- **Embedded obligations**: power-aware scheduling, the watchdog, the
  bounded-allocation mode, pools and high-water reports (`C77`),
  supervised or unprivileged domains (`C78`), and partition layouts
  (`C80-2`). These belong with the embedded backend milestones.
- **Symbolication on other targets.**
- **Uploading crash reports**: they stay on the machine, and uploading is
  the application's choice.
- **Integration contracts**: device messaging and provisioning beyond
  Milestone 55's MQTT, inference kept off the frame path, and node-graph
  transports.
- **Web security headers** were delivered by Milestone 49. Permissions
  policy on web targets is owed with the Web track.
- **A real power cut**: the power-loss test kills a process, which is not
  a power cut. The checksum covers the difference, and a power cut on
  hardware is owed.
- **Printing to paper** was not checked, since no printer is attached.

### Milestone 50 — Deployment, updates, and fleet operations — complete for Windows and the long-lived server

**Built.** Guides: `docs/deploy.md` and `docs/deploy/update-rules.md`.

- **The adapter contract.** `rustnative_server::deploy::DeploymentAdapter`
  covers `deploy`, `promote`, `rollback`, `status`, and `limits`, the
  per-request host limits.
- **A long-lived server.**
  - `LocalAdapter` keeps immutable revisions behind `TrafficSplitter`, a
    reverse proxy.
  - Clients are assigned to revisions by weighted, stable client buckets.
  - A request can preview a named revision with `x-revision`.
  - Promotion is by percentage, and rollback takes one command.
  - The splitter replaces `x-forwarded-for` with the client's real address.
  - `rustnative deploy local start|add|promote|rollback|status` drives the
    adapter through its loopback control API.
- **Infrastructure export.** `rustnative deploy export
  container|compose|kubernetes|systemd|all` writes each description:
  - a distroless, non-root Dockerfile;
  - Kubernetes manifests with probes on `/healthz` and `/readyz`, a
    read-only root file system, and only the declared resources as secrets;
  - a hardened systemd unit.
  - `--build` builds the image when Docker is installed.
- **Single artifact.** `rustnative_build::embed_assets` compiles a
  directory into the binary, each file named by its content hash.
  `ServerApp::assets` serves them as immutable. `assets::stylesheet` and
  `assets::script` write links with subresource-integrity digests, and
  scripts also carry the CSP nonce.
- **Response caching.**
  - `MethodRouter::cached(tags, ttl)` works on public `GET` routes.
    Responses that set a cookie are never stored.
  - `x-cache: hit|miss` reports each lookup.
  - `ResponseCache::invalidate(tag)` supports incremental regeneration.
  - `cache_inspection::<P, Policy>()` serves the queryable cache state.
- **Desktop updates (`rustnative_windows::update`).**
  - Ed25519-signed manifests, checked against the key pinned in
    `[update] public-key`.
  - Staged rollout by a stable installation bucket, and version pinning.
  - SHA-256-verified packages unpacked side by side, with safe paths only.
  - Atomic activation, and a `Launcher` that rolls back a version failing
    twice before `interactive`.
  - Model payloads (`C89`) are compatibility-checked before activation.
- **Update CLI.**
  - `rustnative update keygen` draws the key from the operating system's
    random source.
  - `rustnative update manifest` refuses a key that `rustnative.toml` does
    not trust.
- **MSIX.**
  - `[package] capabilities` go into the generated manifest (`C63`), with
    device capabilities last.
  - `rustnative package windows --appinstaller <url>` writes the
    `.appinstaller` file that App Installer updates from.
- **Build cache.** `rustnative build windows --cache` builds through
  `sccache` when it is installed (`C64`).
- **Written update rules** for each host.

**Verified.** Full gate.
- **Traffic splitting** across two real revision servers:
  - a preview is reachable only by `x-revision`;
  - at 30% promotion, each of 200 clients stays on its revision;
  - 100% promotion, then rollback;
  - redeploying an existing revision name is refused;
  - the control API drives the same adapter.
- **The response cache:** a miss, then a hit, then regeneration after the
  tag is invalidated, with the cache's state listed.
- **Embedded assets:** served immutable, and linked with integrity.
- **Container descriptions:** each asks only for what is declared.
- **The Windows updater**, running real executables:
  - it refuses a tampered manifest, a wrong package, a pinned
    installation, and a rollout this installation is not in yet;
  - a version that fails at start is rolled back on its second failure;
  - a package cannot write outside its version, via `..`, `\`, or a drive
    prefix;
  - a model payload is taken only by compatible application versions.
- **The CLI:** a manifest from `rustnative update manifest` is accepted by
  the updater; an untrusted key is refused; `deploy export all` writes
  every file.
- **MSIX output:** capabilities and the `.appinstaller` content are
  checked.

**Not verified / owed.**
- **Other targets.**
  - Adapters for static hosts, per-request functions, and edge/WASM, with
    their local emulators, and the web loading path (`C42`): Web
    milestones J and K.
  - Mobile over-the-air updates and store asset packs: Milestones 35–36.
  - Firmware A/B updates and multi-image signing (`C81`): Milestone 37.
  - Embedded-Linux images.
- **Remote build and signing.**
- **Resource bindings** beyond `[resources]` names.
- **A per-client rate limit at the proxy.**
- **Container builds and cache runs:** Docker and `sccache` are not
  installed on this machine, so the image build and the cached build were
  not run here.

### Milestone 56 — Durable and event-driven execution — complete (local implementation)

**Built.** A new crate, `rustnative-durable`. Guide: `docs/durable.md`.

- **Durable workflows**
  - `step` results are journaled in SQLite, and a replay returns the
    recorded result.
  - `transactional_step` commits a step's effect together with its journal
    entry, so the effect happens exactly once.
  - Steps get idempotency keys for effects outside the database.
  - Durable timers (`sleep`), signals and approvals, and compensation that
    runs in reverse when a workflow fails.
  - `version` markers let in-flight executions keep their behaviour.
  - Time and randomness are recorded (`now`, `random`).
  - A replay that diverges fails and names both steps.
  - The `WorkflowEngine` contract, with `LocalEngine` as its
    implementation.
- **Event handlers**
  - A standard `EventEnvelope`, and batches that report partial failures.
  - Failed events are retried with exponential backoff, then moved to
    dead letters.
  - Events are deduplicated by id.
  - Each invocation runs in a task scope bounded by that invocation.
- **Stateful actors.** `LocalActorSystem` runs one instance per id, which
  handles one message at a time. Each actor has private durable storage
  that survives eviction, and durable alarms.
- **Supervision.** `supervise` restarts a failing or panicking worker
  according to its `SupervisionPolicy`, and records the history.
- **Operations across the boundary.** `Operations` runs on the server
  (`/_ops/:id`, `/_ops/:id/cancel`). Any client over `HttpService` can call
  `follow` and `cancel`.
- **Example.** `examples/workflow-crash` is killed after a step and in the
  middle of another, then completes with each step executed exactly once.

**Verified.** Full gate.
- **The crash example's test** runs the real binary four times:
  1. Aborted after `reserve`.
  2. Aborted inside `charge`, before it commits.
  3. Run to the end. Each effect row appears exactly once.
  4. Run again. It adds nothing.
- **The crate's tests** cover:
  - timers, signals, approval, and compensation (one refund, for the
    failed order only);
  - versioning, and detection of a diverging replay;
  - a batch with a poison event (only it is retried, then dead-lettered),
    and suppression of duplicates;
  - an actor-backed collaborative session: 20 concurrent appends with no
    lost update, then eviction, restart from storage, and an alarm;
  - supervised restarts after an error and after a panic;
  - a client following and cancelling server operations.

**Not verified / owed.**
- The edge adapter for actors, and serverless adapters for event handlers
  (Web milestone K).
- An adapter for a hosted workflow engine.
- A direct call to the standard library's clock inside a workflow still
  compiles. The type system cannot forbid it, so divergence detection and
  the documented `clippy.toml` are the guard, as `docs/durable.md` says.

### Milestone 55 — Reconciliation beyond the screen — complete (Windows scope)

**Built.** A new crate, `rustnative-sync`. Guide: `docs/sync.md`.

- **The sync service**
  - `SyncedCollection` reads and writes locally first. Writes made offline
    stay pending until they are accepted.
  - `SyncServer` declares a conflict policy per collection:
    `ServerAuthority`, `LastWriterWins` (by hybrid logical clock), or
    `Merge(fn)`.
  - A `Filter` limits what a replica pulls. `subscribe` and the HTTP long
    poll push changes from the server.
  - Schema versioning: the server upgrades an older client's writes, and
    tells a client below the minimum `NeedsUpgrade`.
  - Transports: `InMemory` and `HttpSync` (over `HttpService`, so through
    `WinHttp`). The `server` feature adds `rustnative-server` endpoints.
- **Replicated types**
  - `GCounter`, `PnCounter`, `LwwRegister`, `OrSet`, `LwwMap`, and `Rga`
    (text and lists).
  - Proptest checks that each merge is commutative, associative, and
    idempotent.
  - Each type round-trips through JSON.
- **Server-interactive mode**
  - `LiveServer` runs a `ComponentTree` per session on its own thread.
    Trees travel as `WireNode`s over WebSocket. `LiveClient` and
    `RemoteView` reconcile them on the client.
  - Events are numbered and acknowledged. After a reconnect they are
    resent, and each is applied once.
  - Typing is echoed locally at once.
  - A session survives a reconnect within the grace period. After that,
    the client falls back to its state snapshot.
  - `drain` moves clients to a new instance with their state.
  - Wire keys now carry the owning component (`owner~key`), so events find
    child components' nodes (`wire::node_id`).
- **Render mode per subtree.** `Subtree` has four modes: `Static`,
  `ServerInteractive`, `ClientInteractive`, and `Auto`. `Auto` hands over
  to a registered client module and carries the server session's state.
- **Channels and presence.** `Channel` and `Presence` are contracts.
  `LocalHub` implements both, including expiry when heartbeats stop.
- **Device desired state**
  - `Twin`, `DeviceAgent`, and `Actuate`. Catching up applies only the
    newest desired state, not every one that was missed.
  - `DevicePolicy` settles changes made on the device itself.
  - `DeviceModel` maps state to LwM2M/IPSO resource paths.
- **Messaging**
  - `Broker` supports QoS 0, 1, and 2, retained values, a last will, and
    persistent sessions. `LocalBus` connects to it in process.
  - MQTT 3.1.1: a codec, a TCP broker, and a client. The client
    acknowledges messages and redelivers unacknowledged ones.
- **Examples**
  - `collab-notes`: offline edits on two devices converge.
  - `live-counter`: the count survives a reconnect and a deploy.
  - `device-desired`: over MQTT, the device converges after being offline.

**Found and fixed.**
- **`Rga` did not survive JSON.** It used non-string map keys, so it could
  not be serialized. `SyncedCollection::put` then treated the failure as a
  deletion, and the data was lost. `Rga` now serializes as a list of
  elements. `put` returns an error instead of writing.
- **Events were lost on a dying connection.** They are now resent until
  acknowledged and deduplicated by sequence number.
- **Session ids could collide.** Two server instances in one process could
  issue the same session id. Ids now include a random part per instance.

**Verified.** Full gate.
- 23 `rustnative-sync` tests:
  - CRDT properties;
  - sync policies, filters, schema versions, push, and presence;
  - device convergence;
  - bus and MQTT guarantees over TCP;
  - live mode: reconnect, deploy, typing echo, and the server-to-client
    switch.
- Each example's own tests.
- The live tests passed four times in a row without flakiness.

**Not verified / owed.**
- **Browser client.** The browser client for server-interactive mode and
  the browser render modes are owed with Web milestone H.
- **Windows smoke test.** The examples' `main` programs have no Windows
  smoke test. They use the same components the headless tests drive.
- **Device commissioning** is delegated to existing stacks.
- **MQTT 5 and TLS** for MQTT.

### Milestone 49 — The server application model — complete (Windows scope)

**Built.** The new `rustnative-server` crate, with `rustnative-server-macros`
for `query!`. Guides: `docs/server.md` and
`docs/server/security-checklist.md`.

- **The application model on the client's contracts**
  - Routes are `rustnative_core::Route` patterns.
  - Handlers are `async fn`s with typed extractors: `Path`, `Query`,
    `Json`, `Form`, `State`, `Session`, `Principal`, `RequestScope`,
    `CsrfToken`, and `CspNonce`.
  - Middleware runs before and after the handler.
  - Error pages are HTML or JSON, depending on `Accept`.
  - Each request gets a `RequestScope` that is cancelled when the response
    is sent. A transaction tied to the scope commits only while the request
    is still live.
  - The application is a `tower::Service` (`C37`). It is served by hyper,
    or mounted inside an existing service with `prefix`.
- **Authentication and authorization**
  - Sessions are sealed with AES-256-GCM. Passwords use Argon2id. Bearer
    tokens are HMAC-signed.
  - Passkeys: WebAuthn ES256 registration and assertion, with a minimal
    CBOR reader (`C52-2`).
  - Federation: OAuth authorization code with PKCE.
  - Access is part of each route's type (`public`, `signed_in`, or
    `authorized::<Policy>`). A route that states none does not compile.
- **Data**
  - A pooled, bundled SQLite (`rusqlite` 0.40, without its cache, so no
    duplicate `hashbrown`).
  - `query!` checks SQL against the migrations at compile time and infers
    the row type.
  - Migrations have up and down steps and a dry run.
  - `schema::diff` generates migrations from `schema.toml`, with rename
    detection and squashing (`C38`). The CLI is `rustnative db diff`,
    `migrate`, `rollback`, and `squash`.
  - Row policies come with a fixture harness (`C39`).
- **Jobs**
  - A durable queue with idempotency keys, exponential backoff, and dead
    jobs.
  - Cron schedules.
  - An inspection endpoint that answers the new `jobs` request in the
    Milestone 44 protocol.
- **Secure defaults**, each with a test (see the checklist):
  - CSRF double-submit;
  - a CSP with a per-response nonce;
  - security headers;
  - `__Host-` cookies;
  - rate limiting and a body limit;
  - escaping `Html`;
  - errors that hide internal detail.
- **Admin surface**: generated from the model and gated by a policy.
- **Typed server functions**: `rustnative_core::server_fn`. The server
  serves them with `server_fn`, and a client calls them over any
  `HttpService`.
- **Server-only components** (`C05`): `ServerComponentDef`, with the tree
  payload in the new `rustnative_core::wire` format. The client merges it
  with the ordinary reconciler.
- **The API schema** (`C35`):
  - `ApiSchema` types;
  - an OpenAPI 3.1 document at `/openapi.json`;
  - a `breaking_changes` contract check;
  - a generated TypeScript client.
- **Configuration**: layered (defaults, file, environment, secrets
  directory) into a typed value, with no global. `Secret` never prints.
- **Operations**: `/healthz`, `/readyz`, `/metrics`, and the startup report
  (`C40`).
- **Web output**: `Head`, `Sitemap`, and `render::page`, which serves a
  component tree as a page under the strict CSP (`C41-1`).
- **Push**: Web Push (aes128gcm with VAPID), plus WNS, APNs, and FCM request
  builders (`C54-1`).
- **Examples**
  - `examples/notes-shared`: the definitions and the shared view.
  - `examples/server-demo`: the server.
  - `examples/server-client`: the Windows client.

**Found and fixed.**
- Route parameters were iterated in alphabetical order, so a
  `Path<(String, String)>` read `/admin/:table/:id` backwards.
  `Route::parameter_names` now gives the pattern's order.
- A route parameter that looked numeric could not be extracted as a
  `String`. Text is now tried as well.

**Verified.** Full gate.
- 26 `rustnative-server` tests and its doc tests.
- The CLI `db` test.
- A wire round-trip of every syntax-equivalence case the format carries.
- The notes server:
  - sign-in, notes, and jobs;
  - the page;
  - the admin surface;
  - the published contract `api/v1.json`.
- The client:
  - headless, against the in-process server: sign in, write, and see the
    server-rendered summary;
  - over a real socket through `WinHttp`.
- A compile-fail test shows shared code cannot reach the server crate.

**Not verified / owed.**
- The Web track: browser interactivity for server pages, the serverless
  deployment shape, and the web client half of server functions (Web
  milestones H and K).
- Live WNS, APNs, and FCM sends need credentials.
- Passkey attestation formats other than `none`.

### Milestone 48 — Components, tokens, and visualization — complete (Windows scope)

**Built.** Guides: `docs/components.md`, `docs/tokens.md`,
`docs/idioms/windows.md`, `docs/guides/hybrid-rendering.md`, and
`docs/guides/host-content-controls.md`.

- **Native controls**: checkbox, radio, toggle, slider, progress, select,
  list box, date picker, spinner, separator, link, multiline text, and
  image. Each has an `rsx!` element. On Windows each is the system control,
  and its changes arrive as `Toggled`, `ValueChanged`, `SelectionChanged`,
  or `DateChanged`.
- **`rustnative-components`**: 18 composite components. Each has builder and
  markup forms, uses stores for binding and commands for actions, and has
  its accessibility role asserted.
  - The headless behaviour layer (`C19`) covers list selection, tabs,
    menus, trees, grid navigation, combobox, and date entry.
  - The per-host idiom table is `C23`.
  - `AdaptiveNavigation` (bottom bar, rail, or sidebar) and
    `CommandPalette` are `C22-3` and `C20-3`.
  - Charts are drawn on the draw-list path. Each has a data table as its
    accessible form, readable by keyboard (`X-VIZ-1`).
- **Token pipeline**: `rustnative tokens import` turns a W3C Design Tokens
  file into the Milestone 58 `@theme` block.
  - A role marked with `rustnative.host` follows `HostPalette`. On Windows
    that is `DwmGetColorizationColor` and `GetSysColor`, re-read on
    settings and colorization changes.
  - Brand values apply as given.
- **Grid** (`C18-1`): `Node::grid` with fixed, auto, and fraction tracks,
  gaps, and spans. `LayoutStyle::grid` places a child. It has a markup
  `<Grid tracks=…>` element and a `grid` attribute.
- **Matched geometry** (`C25`): `Node::with_shared_id`. A node arriving in
  place of one with the same identity moves and resizes from the old one's
  rectangle (`matched_geometry`). Windows animates it with the timeline, and
  it respects reduced motion.
- **Lists** (`C26`): `rustnative_data::list::Projection` gives non-copying
  filter, sort, and group views. `diff_keys` reports removals, insertions,
  and minimal moves. `SectionedView` has list, grid, or carousel sections
  on a virtual list.
- **Documents** (`C27`): `DocumentController` handles open, save, save as
  (atomic), revert, autosave, dirty state, undo, and redo. It is bound to
  the standard commands, reports external changes, and follows the Windows
  title convention. `RecentDocuments` keeps the recent list.
- **Host content** (`C28`): `HostContent` and `host_content` build a
  capability-guarded foreign node, or the application's fallback. On
  Windows, media is `MCIWnd`, the camera preview is `avicap32`, and web
  content is not offered.
- **Text profiles**: `TextProfile` and `Script` declare which scripts a
  target renders, and check strings against them.
- **Example**: `examples/gallery` is built only from the library and
  `tokens.json`.

**Found and fixed.**
- **Node identity.** Nodes a parent passed to a child component in its
  props (a card's body of badges) were scoped a second time. Only their
  local keys survived, so two badges collided. The duplicate identity then
  made the headless window render nothing. Already-scoped nodes now keep
  their identity. Regression test: `rustnative-core/tests/composition.rs`.
- **CLI manifest.** `rustnative.toml` refused a `[style]` table in the CLI,
  although the build reads it. `rustnative tokens import` could not run in
  a project that declared its style file.
- **Markup props.** A component element's omitted props are now defaulted.

**Verified.** Full gate.
- Conformance cases for every new element and attribute in all three
  spellings: builder, `rsx!`, and `.rsx`.
- Grid layout test, plus matched-geometry, list, document, text-profile,
  and host-content unit and doc tests.
- The library's headless tests, including sectioned layouts.
- Gallery headless goldens for three pages, plus the token-set theme.
- Windows tests:
  - native controls;
  - library components realized as host controls in the host accent;
  - a shared-identity node growing from 40 to 200 px over its transition;
  - media as `MCIWndClass`, and web content as the fallback.

**Not verified / owed.**
- Web content needs WebView2 (`Capability::WebContent` is not offered).
- System media transport controls and picture-in-picture are WinRT APIs.
- `SysLink` falls back to clickable text without a Common Controls 6
  manifest.
- Rendering on the other backends is owed with those backends.

### Milestone 54 — Responsiveness under load — complete (Windows scope)

**Built.** `docs/responsiveness.md`.

- **Priorities**: `Priority` is Immediate, Normal, or Deferrable, set with
  `Callback::send_with`. Deferrable messages wait for
  `ComponentTree::pump_deferred`, which delivers a budgeted slice
  (`set_render_budget`, 4 ms) and commits it whole. Windows runs it only
  while `GetQueueStatus(QS_INPUT)` reports no input, and asks again after
  input; headless runs it when nothing more urgent is left.
- **Deferred values**: `ComponentContext::deferred(key, input, compute)`
  computes off the UI thread. A superseded computation is cancelled, and
  never started if superseded before it runs. `Deferred { current,
  pending }` keeps the previous result on screen while the new one is
  prepared (`C02`).
- **Pure components**: `PureComponent` renders from `&Props` only, so a
  mutating render does not compile (`C03`). It is composed with
  `context.pure` and skipped on equal props.
- **Suspendable scopes**: `SuspendRule` is Complete, Cancel, or Defer, used
  through `TaskScope::spawn_with`, `suspend`, and `resume`. Effects share
  their component's suspension. Components whose output is hidden (a
  `hidden` node, such as navigation or tabs) or whose window is minimized
  are suspended and resumed automatically (`suspended_components`). Stream
  collection pauses with its scope (`C75`, `C76`).
- **Skipping**: `unskippable_components` reports props types that are
  unequal to their own clone (`C04-1`).
- **Reference application**: `examples/filter-demo`, 200 000 rows with a
  virtual list, a deferred filter, and a clock that stops while its screen
  is hidden.
- **Bench**: a `filter` scenario (Windows, keystrokes posted to the real
  edit control) and a headless keystroke key, with budgets in
  `budgets/*.toml`.

**Found and fixed.**
- A hidden child that re-rendered alone lost its parent's `hidden` flag,
  and its item index, when spliced into the reused parent output. It
  reappeared, which also affected navigation stacks. The splice now keeps
  both, and a regression test covers it.
- Filter rows keyed by data were recreated on every result, costing a
  keystroke that landed just after up to 55 ms. They are now keyed by
  position and re-texted.

**Verified.** Full gate.
- `rustnative bench --check` passes for both targets.
  - Windows: `filter_input_latency_ms` 0.69 (budget 16),
    `filter_input_latency_max_ms` 30.5 (budget 30, limit 60),
    `filter_results_ms` 3.6.
  - Headless: `filter_input_latency_ms` 0.12 (budget 4).
- Core tests cover slices and ordering, supersession (one computation for
  three keystrokes), a hidden screen with no ticks and resuming,
  Cancel-rule cancellation, minimized suspension, pure skipping, the
  unskippable report, and the splice regression.
- The example's headless tests cover keystrokes within budget, the stale
  view shown while updating, a screenful of rows realized, and the clock
  stopped while hidden.
- `native::responsiveness_integration` shows the Windows ticker stopping
  when hidden and resuming when shown, and zero messages of any kind over
  two seconds of idle.

**Not verified / owed.**
- The worst keystroke waits for a filter result's realization, which
  takes about 30 ms and sits at the budget line.
- A query's interval revalidation still runs while its only observers are
  hidden.
- The embedded static-priority executor and idle-current measurement are
  owed with Milestone 37.

### Milestone 47 — State, resilience, and data — complete (Windows scope)

**Built.** `docs/data.md`.

- **Core** (`rustnative-core`):
  - `Store<T>` shared state, provided to a subtree (`provide_scoped`,
    `scoped`) and read by slice (`select`). A component re-renders only when
    its slice changes by value (`C08`). Updates are ordered, and nested
    updates are queued. `Derived<I, O>` caches values. `Store::inspectable`
    feeds `rustnative inspect stores`.
  - Error boundaries (`context.boundary`) contain a panic in render, update,
    or message delivery. The subtree is removed as on unmount, a fallback is
    shown, and the failure is reported to `take_failures` and the inspection
    trace (`TraceKind::Failure`). `SupervisionPolicy` is isolate, restart
    with backoff, or escalate (`C17`). `TaskScope::spawn_supervised` applies
    the same policies to tasks.
  - `collect` for streams (`C12`), `prepare` for off-thread preparation
    (`C09-2`), and `Background` for message-less UI-thread work and
    offloaded work.
  - `NavigationStack::saved_state` with a 64 KiB budget (`C14`), and
    `CertificatePins`.
- **Data layer** (`rustnative-data`, a new crate):
  - `QueryClient` with exhaustive `QueryState` (`C29`), deduplication, stale
    and retention lifetimes, stale-while-revalidate, a `Revalidate` policy,
    retries with seeded jitter, hierarchical invalidation, cancellation when
    unobserved, structural sharing, infinite queries, prefetch, and
    `BatchLoader` (`C30`).
  - Mutations with optimistic updates and rollback, a durable offline queue
    replayed in order on reconnect and at start, and a `ConflictPolicy`.
  - Forms: `Schema`, typed `Field`s, `Changeset`, `FieldErrors`, constraint
    mapping, and `Form` submission (`C36`).
  - `Migrations` and `VersionedStore`, with a dry run.
  - `History` (undo and redo), `StateMachine` with state-scoped work and
    Mermaid output (`C10`), and `Operation` (`C87`).
  - `BackgroundWork` with network, power, and deadline constraints.
  - `LocalTable` live queries and `PagingSource` (`C31`).
  - `HttpClient` interceptors (auth refresh, retry, logging, ETag cache) and
    typed `Endpoint`s (`C34`).
  - `ImageLoader` with decode, downscale, disk cache, and retention.
- **Windows**:
  - `WinHttp`, the platform's `HttpService`. It enforces pins: it sends the
    headers, checks the certificate's SHA-256 through `BCryptHash`, and only
    then writes the body. It refuses plain HTTP and redirects for pinned
    hosts.
  - `WindowsConditions` (`GetSystemPowerStatus`, `WinINet`) and
    `WicDecoder` (WIC decode with a Fant-downscaling scaler).
- **Example**: `examples/data-demo`, a task board over an in-process server.

**Verified.** Full gate.

- Core tests cover slice-only re-renders, stores updated by background
  work, render and handler failures contained, restart with backoff on
  virtual time, manual retry, an uncontained panic still reaching the
  application, supervised task restarts, and stream delivery.
- `rustnative-data` has 22 tests: dedup, freshness, retention, retries,
  failure and empty states, invalidation, identity kept, optimistic
  rollback, the durable offline queue, merge conflicts, pagination,
  batching, forms, migrations, paging gaps, live queries, the interceptor
  chain, constraints, pre-emption, state-scoped cancellation, and decoding.
- On Windows:
  - a real WinHTTP round trip to a local server, and WIC decoding;
  - `native::data_integration`: a fetched query lands in a native control,
    an optimistic item shows and rolls back, and a handler panic is
    contained with a native fallback while the window stays open, then
    rebuilt by "Try again".
- The example's headless tests cover:
  - one request for two observers, and pagination;
  - optimistic add confirmed by the server;
  - offline queueing across a restart, sent in order;
  - the weather widget contained and restarted with backoff, and retried by
    hand, with the board untouched.

**Not verified / owed.**
- Certificate-pin mismatch against a live TLS server. The tests cover the
  digest, the policy, and the plain-HTTP refusal, but no TLS endpoint is in
  the test environment.
- Suspension of hidden subtrees' streams and queries is Milestone 54.
- Request scope and server-side schema validation are Milestone 49.
- The progress component is Milestone 48.
- The image memory budget is by time, not bytes.
- Mobile background schedulers are owed with those backends.

### Milestone 46 — Internationalization and localization — complete (Windows scope)

**Built.** `docs/i18n.md`.

- **Catalogues** (`rustnative-i18n`, a new platform-free crate): a documented
  Fluent subset.
  - It covers messages, `{ $var }`, `{ -term }`, selectors with exact
    numbers, plural categories, and gender or text keys, and comments as
    translator context.
  - It has CLDR cardinal rules for en, de, fr, ar, he, pl, ru, ja, zh, and
    ko, tested on CLDR's own samples (`VENDORED.md`).
  - Fallback goes locale → language → source.
- **Typed messages**: `rustnative_build::compile_messages` checks the
  translations against the source (a translation reading a variable its
  source never passes fails the build). It generates one function per
  message whose parameters are exactly its variables (`i64` for plurals,
  text otherwise), plus the embedded `catalogues()`.
  `rustnative_core::messages_mod!()` declares them.
- **Resolution** (`rustnative_core::i18n`): `Message` is data, and becomes
  text during the render of the component showing it, against that
  component's `LOCALE`.
  - Showing a message records the locale read, so a switch re-renders only
    message readers (`C15`).
  - `Application::set_catalogues` / inspection `SetCatalogue` replace
    catalogues live; the dev loop uses them for `.ftl` saves.
  - The pseudo-locales are `en-XA` and `ar-XB`, and
    `RUSTNATIVE_I18N_SHOW_KEYS` shows each message's key.
- **Host formatting**: the `LocaleService` contract. `WindowsLocale` covers
  numbers, currency, dates, times, collation, and casing through NLS, with
  the person's overrides honored. `InvariantLocale` is deterministic for
  headless and tests.
- **Routes**: `Router::localized`, `alternates`, and `locale_of` (`C41-2`).
- **Tooling**: `rustnative i18n extract|merge|show|lint`, reading builder
  calls, `.rsx`, and `rsx!` alike. `[i18n]` in `rustnative.toml` sets the
  source locale and the allow-list.
- **Layout suite**: the reference screen's strings are now a catalogue, and
  its pseudo variant resolves through the real message path.

**Found and fixed.** A gender selector (`[feminine] … *[other]`) was typed
as a plural, because `other` is also a plural category. It is now a plural
only when every key is a plural category or a number.

**Verified.** Full gate. The example's headless tests cover:
- runtime switching;
- Polish few and many forms;
- Arabic few and zero forms and the feminine form;
- RTL mirroring, measured by position;
- the pseudo-locale.

The native test (`native::i18n_integration`) shows the same controls taking
Arabic text with its plural, and the subtree gaining `WS_EX_LAYOUTRTL`. NLS
formatting is tested for de, fr, en-NZ, and tr casing. The CLI workflow test
runs on a copy of the example.

**Not verified / owed.** HTML language alternates wait for Web H. No
translator has reviewed the Arabic and Polish strings; they are the
example's own.

### Milestone 43 — The developer loop — complete (Windows scope)

**Built.** `docs/developer-loop.md`.

- **`rustnative dev windows`** watches the project and handles each save in
  one of two ways, printing which:
  - A token-only `app.css` change is pushed live through inspection's new
    `SetStyleFile`: the theme is re-resolved in place, with no rebuild.
    Utility, `inline`-theme, and token-name changes are rebuilds.
  - Anything else is rebuilt, restarted, and restored:
    1. a failed build leaves the running application as it was;
    2. otherwise its state is snapshotted (inspection `Components`);
    3. it is closed as a person would close it (the new `Quit`: flushed,
       placement saved);
    4. a copy of the new build is started (Windows will not overwrite a
       running executable);
    5. the state is restored field by field through `Component::edit`.
- **Remote host**: `rustnative dev-agent`, with a CSPRNG token and a
  two-phase deploy, so a refusal sends nothing. It writes only into its own
  folder and hands back the application's inspection endpoint.
- **Budgets**: `dev_loop_restart_ms` (6.9 s on `examples/hello-label`, of
  which the build is 5.7 s; two state fields restored). `first_run_s` covers
  three commands, new to interactive.
- **Previews and catalogue** (`C55`): `rustnative_core::preview`, covering
  `Preview`, `PreviewMatrix` (48 configurations when full), `PreviewFrame`,
  and `Catalogue`.
  - `rustnative preview` opens it natively.
  - `rustnative_headless::preview_goldens` makes every preview a golden test
    (`C55-3`), in the templates' `tests/previews.rs`.
- **Templates**: the application is a library (with `previews()`) and the
  executable is a thin shell, so an application change recompiles one crate.
- **`rustnative generate`** (`C57-1`) covers components, screens (routes
  wired into `router()`), and services. Each comes with its preview and its
  test, in either syntax, verified by building and testing both generated
  projects.
- **Editor assistance** in `.rsx` and inside `rsx!` alike:
  - attribute completion and hover, with go-to-definition to the builder
    method;
  - class-string completion (variants kept) and hover naming properties,
    tokens, and resolved values;
  - diagnostics narrowed to the class or attribute they name;
  - the structural editing requests (`C56`), as format-preserving
    workspace edits (`markup_edit`).
- **Development services** (`C58`):
  - `[resources]` are provisioned locally;
  - `rustnative test --watch`;
  - a development run's panic dialog gives the source position, in the
    `.rsx` file via the source map (`rustnative_core::dev`).
- **`rustnative doctor --install [--dry-run]`** (`C90`) adds what `rustup`
  can.

**Found and fixed.**
- The style-sheet parser panicked on an unterminated block (a half-typed
  `app.css`). It is now an error, and every prefix of a real file is tested.
- A relative `--framework-path` was written relative to the new project. It
  is now resolved from where the command runs.
- The inspection and agent tokens now come from the OS CSPRNG, after a
  security review of the first version.

**Verified.** Full gate. `rustnative dev --once` on hello-label restores its
state; this is an ignored test that the interactive CI pass runs. Both
templates, with generated items, build and pass their tests.

**Not verified / owed.**
- The device loop, board quickstarts, and development builds on devices are
  owed with Milestones 35–37. Only the Windows remote host is verified, over
  loopback.
- Dynamic-library reload (`--hot`) is not built. It would duplicate the
  framework's process-wide state across a `cdylib` boundary unless the
  framework itself is shared. Restart-with-state is the loop instead.
- Live locale catalogue pushes wait for Milestone 46.
- `generate server-resource` waits for Milestone 49.

### Milestone 42 — Budgets — complete (Windows scope)

**Built.**
- **Budget files**: `budgets/windows.toml` and `budgets/headless.toml`, with
  every key defined in `budgets/SCHEMA.md`. Each key declares its budget
  (`max`) and its noise (`tolerance`); the measured values are kept in
  comments.
- **Startup phase model** (`C62`, `rustnative_core::perf`): process start,
  runtime ready, first frame, first content, interactive.
  - Windows marks each phase, with the process start taken from
    `GetProcessTimes`. The headless backend marks them too.
  - `RUSTNATIVE_STARTUP_TRACE=1` prints the phases.
  - `RUSTNATIVE_EXIT_AT=interactive` quits at interactive: the scripted
    startup.
  - Frame times and realization times are recorded for the harness.
- **Harness**: `examples/bench-app` runs the scenarios: startup, interaction
  (`BM_CLICK` posted to the real button, timed until the change is realized),
  animation frame times, the core on 1k nodes, the markup/style compile
  steps, and headless launch and input.
  - `rustnative bench --target windows|headless [--check] [--low-end]
    [--build-times]` takes medians and writes `target/budget-report.json`.
  - `--check` fails on a regression beyond tolerance, and on any key that is
    unbudgeted or unmeasured.
  - CI job `budgets` runs it on Windows (with build times) and on headless
    pinned to one core.
- **PGO** (`C62-2`): `rustnative build windows --release --pgo` builds
  instrumented, runs the scripted startup, merges the profiles with
  `llvm-profdata`, and rebuilds with `-Cprofile-use`. Without the
  `llvm-tools` component it fails at once with the `rustup` command to run.
- **No claim without a number**: `rustnative-conformance/tests/doc_claims.rs`
  rejects performance adjectives in README/docs that do not cite `budgets/`.
  README's "Performance budgets" section quotes the file.

**Fixes the budgets found.**
- **Click-to-realized** on a 40-row form was about 29 ms. Two causes:
  - Every relayout re-measured every label through a new device context and
    `DrawTextW` (20 ms). The Windows measurer now caches measurements, which
    are pure for a font, text, width, and scale.
  - Every relayout moved every window (4–10 ms). The renderer now moves only
    windows whose rectangle changed. It forgets positions on a direction
    flip and on node removal.

  The median is now 7–13 ms.
- **Fonts and brushes** were created per control (about 0.3 ms and two GDI
  handles each). Styles now share them through a reference-counted pool,
  freed with the last user. The GDI leak gate still holds.
- **Tried and reverted**: batching the first layout's moves with
  `DeferWindowPos` made no measurable difference.
- **Recorded, not fixed**: the primary window's first `ShowWindow` costs
  about 130 ms of startup (DWM, the OS's work). Process start to runtime
  ready (about 50 ms) is loader and runtime initialization.

**Verified.** Full gate. `rustnative bench --check` passes on both targets on
the reference machine. The scripted startup exits at interactive, and the PGO
missing-tool path is exercised.

**Not verified / owed.**
- The optimized half of the PGO build is not run here, because `llvm-tools`
  is not installed on this machine (it is a download the person decides on).
- The CI runners' numbers are unknown until CI runs. The tolerances reflect
  this desktop's noise (±50–100% on microbenchmarks); a runner that differs
  will need its own calibration.
- Web metrics (`C42-4`), edge and server keys, embedded RAM/flash and
  boot-to-first-frame (`C83-2`), and the device matrix are owed with their
  backends.

### Milestone 44 — Inspection and diagnostics — complete (Windows scope)

**Built.** `docs/inspection.md`.

- **One protocol** (`rustnative_core::inspect`), answered by
  `Application::inspect`; the backend supplies what only it knows through
  `InspectBackend`. It covers:
  - the declarative tree, with each node's component and the classes as
    written;
  - components with state, which `Component::inspect` shows and
    `Component::edit` makes editable, through a message;
  - realized host objects and the mapping to nodes;
  - layout explanations;
  - style provenance by precedence level (`C18-2`: class, declaration, typed
    override, component default, token, state variant). `DeclarationSet` now
    records the class or declaration each declaration came from;
  - the event/task trace with every component's render-or-skip reason
    (`C04-2`);
  - tasks, host-object lifetimes, capabilities and refusals (style table and
    unit mapping included), and mappers (`C24-2`);
  - state history, the overlay, and recording.
- **Transport**: token-authenticated, versioned line-delimited JSON over TCP.
  - It is loopback by default, or an explicit address for a remote machine.
  - It turns on with `RUSTNATIVE_INSPECT` and publishes an endpoint file.
  - Requests are answered on the UI thread, woken through the scheduler waker.
- **Client**: `rustnative inspect` (tree, components, realized, state, set,
  explain, style, trace, tasks, lifetimes, caps, mappers, history, overlay,
  record, stop, to-test).
- **Overlay** (layout, events, frame cost) is a `DrawList` built by the core.
  - On Windows it is a layered, click-through, topmost canvas popup drawn
    through the canvas path.
  - On headless it is `HeadlessApp::overlay()`.
- **Record, replay, and time travel** (`C61`):
  - Recordings hold input, virtual-time stamps, and the HTTP exchanges
    (`RecordingHttp`). Nodes are named root-relative, so a recording replays
    across binaries.
  - Redaction by node key, and secret headers are never recorded.
  - Replay is deterministic on headless (`HeadlessApp::replay`) and runs on any
    `Application` (`Recording::replay`).
  - State history can be stepped back through.
  - `Recording::to_test` produces a generated test.
    `rustnative-headless/tests/replayed_session.rs` is one, checked in and run.
- **Reduced form**: `inspect::compact`, deferred formatting (message ids and
  arguments, host-side format table), tested round trip.
- **Backends**: Windows (`native::inspect`, polled on the primary window's
  wake; answers from the registry's window classes, `HWND`s, and Win32
  rectangles; a lifetime log added to the registry) and headless
  (`HeadlessInspect`).

**Fixes this milestone made.** The style explanation first reported
typography that a class copied from the theme (a class setting only the
weight) as a typed override. It now reads typed overrides from the
as-authored tree. A second `enable_inspection` started a second listener; it
now returns the running one.

**Verified.** Full gate (fmt, clippy, tests, docs, MSRV, deny). One test
failed once under the full workspace run:
`rustnative/tests/packaging.rs::the_portable_zip_is_reproducible_and_its_checksums_verify`
found two package runs' archives differed. It passed alone and in a full run
of its own binary, so it is recorded as flaky under load and watched, not
fixed. The protocol is verified in these places:
- in the core (`rustnative-core/tests/inspection.rs`: tree, state editing,
  layout and style explanations, trace, history, redacted recording and replay,
  overlay, transport with and without the token);
- on headless (`rustnative-headless/tests/inspection.rs`: realized objects,
  capabilities, lifetimes, overlay; a session with HTTP recorded and replayed
  without the server; the generated test);
- on Windows over the real transport (`native::inspect_integration`: answers
  from the native objects, an edit reaching the control, the overlay popup's
  styles and draw list, hiding it);
- the CLI against a live application (`rustnative/tests/inspect.rs`).

**Not verified / owed.** These are owed with their backends (36, 37):
- the terminal and embedded reduced forms: `compact` exists, but no probe or
  serial transport does;
- emitting the trace to embedded trace formats and debugger
  kernel-awareness (`C92`);
- device sensor recordings (`C88`).

Also not done:
- Only HTTP responses are recorded (storage and clipboard are not).
- On Windows only the primary window answers `realized` and `lifetimes`.
- Scroll offsets are not applied to the overlay's rectangles.

### Milestone 41 — Guarantees and conformance suites — complete (Windows scope)

**Built.** `docs/guarantees.md` lists every guarantee with its named test on
each shipped backend.

- **Shared suites** (`rustnative-conformance`): `host::ConformanceHost` /
  `Driver`, implemented for the headless backend (`HeadlessHost`) and for
  Windows over the native harness (`native::guarantees_integration`); suites
  for the transient fast path (typing renders only the owning component,
  scrolling renders nothing), batching (one render per message, never a
  partial set), scope-bound cancellation, and native-object lifetime. Style
  equivalence moved here beside syntax equivalence.
- **Scope-bound cancellation as a property**: random mount/unmount/time
  sequences (`tests/cancellation_property.rs`, proptest).
- **Layout conformance**: `rustnative_core::localization::pseudo_localize`
  (+40 %, accented, bracketed), the reference screen
  (`rustnative_conformance::reference`), and the suite at text scales
  1.0/1.5/2.0 × pseudo × mirrored — no clipping, no overlap, targets ≥ 24×24
  inside their container, all reachable by Tab — on headless and, with the
  system's font metrics, on Windows.
- **Windows-only guarantees**: the GDI/USER leak gate (100 cycles of a styled
  subtree), modal-loop conformance (menu tracking and the size loop keep
  animations and tasks running), fidelity (system control classes, focus cues
  on keyboard traversal, high contrast, text scale), and text through the
  system's stack (Arabic, mixed bidi, Devanagari, emoji ZWJ, Thai, CJK).
- **Fixes the suites found** (recorded in `docs/guarantees.md`): wrapping
  labels were measured at one line (the column now measures at the width it
  assigns); Windows measured text in the default window font rather than the
  drawing font (`IntrinsicMeasurer::measure_styled`); the text scale did not
  reach Windows fonts and high contrast did not reach colours (now applied to
  every realized style, on existing objects — font sizes from declarations
  are specified at the default text size so they are not scaled twice);
  keyboard traversal left focus cues hidden (`WM_CHANGEUISTATE`); scheduled
  work stalled in host modal loops; a test harness's windows outlived their
  runtimes (the harness now tears them down).
- `docs/conformance/windows-fidelity.md`,
  `docs/conformance/windows-screen-reader-pass.md` (the checklist),
  `docs/comparison/methodology.md`, `examples/reference-app`.

**Verified.** Full gate; the Windows visual golden was re-blessed after the
measurement fix (controls now sized for the Segoe UI font that draws them —
reviewed by eye).

**Not verified / owed.** A recorded Narrator pass is owed to a person — no
pass is claimed. The OLE drag-and-drop modal loop is not exercised (it needs a
physical button held). The comparison's self-drawing and embedded-engine
columns are not measured. The deferred backends owe their columns.

### Milestone 40 — Interoperability and incremental adoption — complete (Windows scope)

**Built.**

- **Library-only mode** (`crates/rustnative-interop`): the `.ril` interface
  description (services, constructors, methods, events; `bool`, integers,
  `f64`, UTF-8 `string`; `[thread = owner|any]`), generators for a C header,
  C# 5 P/Invoke bindings with an `IDisposable` wrapper per service, and the
  Rust implementation shims (a trait per service, its events, and an
  `export_<library>!` macro), over a runtime that contains every failure as a
  status code with a message — panics, wrong thread, stale handle, invalid
  UTF-8, and re-entrant calls (`BUSY`). Owner-only services may hold `!Send`
  models. `rustnative bindgen <file.ril> --lang c|csharp|rust`.
- **Embedding inward** (`WindowsPlatform::embed` → `EmbeddedRoot`): the
  primary window realized as a `WS_CHILD` of a host window, with
  `set_bounds`, `handle_message` for the host's loop, and teardown that
  destroys the framework's windows only. **Guest-runtime mode**
  (`start_external` → `ExternalLoop`): the framework's windows under the
  host's `main` and loop. Under a host's loop the framework records quit
  requests (`finished`, `error`) instead of posting `WM_QUIT`; the scheduler
  wake is also handled by the window procedure; the framework root is found
  by class, not `GA_ROOT`.
- **Embedding outward** (`Node::foreign` / `<Foreign kind="…">`,
  `SurfaceContent`, `register_foreign`, `ForeignControl`, `Ownership`):
  factory-made objects sized by `IntrinsicMeasurer::measure_foreign`, laid
  out and clipped by layout, destroyed when owned or hidden and handed back
  when borrowed (also when the window closes), never subclassed or pooled.
- **The rendering-surface hand-off**: `docs/interop/surface-handoff.md`, and
  `WM_DPICHANGED` handled — the window takes the suggested rectangle and every
  surface is re-reported with its new scale factor.
- **The adoption ladder** (`docs/interop/adoption-ladder.md`):
  `examples/adoption-library`, `examples/adoption-subtree`,
  `examples/adoption-foreign`.

**Verified.** Full gate. A C program (MSVC, `/W4 /WX`) and a C# program (the
.NET Framework compiler, `/warnaserror`) drive the library DLL through the
generated bindings; a `windows-sys`-only Win32 program embeds a subtree,
clicks into it, resizes, and drops it; a Rust Native program adopts a month
calendar and a date picker at their factories' sizes and removes them;
guest-runtime close posts no `WM_QUIT`; the DPI hand-off test; the syntax
equivalence and compile-failure suites cover `<Foreign>`; `bindgen` CLI test.

**Not verified / owed.** The web rung — a component exported as a web custom
element (`C43`) — is owed by Web milestone B. Embedding into the other hosts
(a view, a widget, a document node) is owed by Milestones 33–38. A foreign
object's keyboard traversal is the framework's only when its node declares
itself focusable. The headless backend measures foreign nodes at zero unless
their layout fixes a size.

### Milestone 58 — The style spellings — complete (Windows scope)

**Built.**

- **`crates/rustnative-style`**: the declaration model (`StyleProperty` —
  every property names one typed field — `StyleValue` with token
  references, `Condition`, `DeclarationSet` in a `static`), the value parser
  (lengths in px/rem/em, `calc()`, `var()`), colours (`#hex`, `rgb()`,
  `hsl()`, `oklch()`, `color-mix()`; one gamut rule: linear sRGB, clamp,
  encode), the Tailwind CSS **v4.1.13** utility vocabulary and variants over
  the vendored default theme (MIT, pinned in `VENDORED.md` with its hash),
  the `app.css` parser (`@theme [inline]`, namespace reset, `@utility`,
  `@apply`, `@custom-variant`; `@import "tailwindcss"`, `@plugin`, `@config`,
  selectors, and `@media` refused with the reason), the run-time
  `TokenTable`, and the capability tables and unit mappings of both shipped
  backends.
- **Macros**: `classes!` and `styles!` (re-exported by `rustnative-core`),
  reading the crate's `app.css` (or `rustnative.toml [style] file`) with
  rebuild tracking; an unknown class, a computed class, a layout property
  under a state variant, and — for `target_os = "windows"` — a property the
  Windows table marks unavailable are compile errors at the string. Markup:
  `class="…"` and `style="…"` lower to them; `style={expr}` stays typed.
  **Documented deviation**: the builder spelling is
  `.with_class(classes!("…"))`, not `.with_class("…")`, because only the
  macro can make an unknown class a compile error (2.14's first rule).
- **Core**: `Node::{with_class, with_declarations, with_state_style}`,
  `StateStyles` (layered in `Theme::resolve`, so a backend's hover/press
  repaint uses them unchanged), `VisualStyle::shadow`, `Theme` tokens
  (`with_tokens`, `with_token`), `keys::WINDOW_WIDTH` and `keys::POINTER`,
  and resolution in `ComponentTree` after every render and on every
  environment change — per node, in the environment of the component that
  rendered it (so `provide_env` scopes `dark:`), with `rem` following the
  text scale. `Platform::style_capabilities` / `unit_mapping`.
- **Build and CLI**: `rustnative_build::compile_styles()` →
  `rustnative_core::app_theme!()`; both project templates ship `app.css`, a
  project utility, and `set_theme(app_theme!())`; `rustnative expand
  --classes/--styles` prints each declaration and what it resolves to.
- **Windows**: the table (colours, fonts, layout, opacity realized; border
  colour and radius approximated on containers — a one-pixel frame and a
  `SetWindowRgn` rounded region — native controls keep their system shape;
  font generics mapped to Segoe UI/Cambria/Consolas; shadow unavailable), the
  unit mapping, and a live scheme switch from `WM_SETTINGCHANGE` restyling
  existing controls. `hello-label` styles its counter panel with classes and
  an `app.css` utility and gains a System/Dark/Light scheme toggle.

**Verified.** Full gate. `rustnative-core/tests/style_equivalence.rs` resolves
every property in the utility, declaration, and typed spellings to equal
nodes (a property without a case fails the suite), and shows state,
scheme, width, direction, motion, pointer, text-scale, token-switch, and
provided-environment conditions. `native::style_integration` reads the
Windows table back from the native objects (font face/weight/size via
`WM_GETFONT`, colours via `WM_CTLCOLORSTATIC`, container border and rounded
region) across a scheme and a token switch on the same HWNDs.
Compile-failure cases in `rustnative-conformance/tests/style_ui*`; CLI tests
for `expand --classes` and an `app.css` mistake reported at
`app.css:line:col`; the documentation-parity check now also requires both
style spellings.

**Not verified / owed.** Container queries and relational variants are
deferred by the plan; a `Literal::subspan` span inside the class string needs
an unstable API, so errors point at the string and name the class. The
deferred backends owe their own capability tables and unit mappings
(Milestones 33–38, Web). The unavailable-property check keys to
`target_os = "windows"`; a second backend on the same OS will need a
backend-selecting cfg. The Windows unit mapping is recorded as it is, not as intended:
layout works in device pixels (one logical pixel is one device pixel), and
scaling layout by `GetDpiForWindow / 96` is **owed** — `rem` does follow the
text-scale setting.

### Milestone 53 — The markup syntax — complete

**Built.**

- **`crates/rustnative-markup`**: the one parser (`syn`), the element table
  (every node kind, its required and optional attributes, the universal
  modifiers), the lowering to builder calls with spans preserved
  (`quote_spanned`), the formatter, and the `.rsx` compiler with its
  source map. Expansion goldens pin the lowering of every element.
- **Carrier 1, `rsx!`** (`crates/rustnative-macros`, re-exported as
  `rustnative_core::rsx` behind the default `markup` feature): elements,
  components (`child_with_props`), `if`/`else`, `match`, `for`, braces for
  any Rust expression, fragments, `IntoChildren`.
- **Carrier 2, `.rsx` files**: `rustnative_build::compile_rsx` compiles
  `src/**/*.rsx` into `OUT_DIR/rsx/…` from `build.rs`, with
  `rerun-if-changed` per file; `rsx_mod!` includes one. `rustnative
  build`/`check`/`test`/`run` run Cargo with JSON diagnostics and remap every
  span in a lowered file back to the `.rsx` line and column
  (`crates/rustnative/src/diagnostics.rs`).
- **Tooling**: `rustnative expand`, `rustnative fmt [--check]` (both
  carriers), `rustnative lsp` (a proxy that presents `.rsx` documents to the
  Rust language server as their lowered files and maps positions and
  diagnostics back; markup completion and hover), and `rustnative new
  --syntax builder|markup` (required; both templates build).
- **Conformance** (`crates/rustnative-conformance`): the equivalence suite
  (every node kind and modifier, builder vs `rsx!` vs `.rsx`), the
  compile-failure suite (trybuild; unknown element, unknown/missing/duplicate
  attribute, mismatched tag, wrong type, and the rest, through both
  carriers), and the documentation-parity check: every section of
  `README.md`/`PLAN.md`/guides and every runnable doc example that builds a
  tree shows both syntaxes.

**Verified.** Full gate. The CLI tests build and check a generated markup
project, report a type error written in `app.rsx` at its `app.rsx:line:col`
with the `.rsx` line quoted, and drive the LSP proxy end to end.

**Not verified / owed.** The LSP proxy is tested against a scripted echo
server, not a live rust-analyzer; editor assistance *inside* an `rsx!` in a
`.rs` file is Milestone 43's. Plain `cargo build` reports positions in the
lowered file (stated in the plan).

### Milestone 39 — Portable-surface obligations — complete (Windows scope)

**Built.** Every item of the milestone, with its proving test listed in
`docs/conformance/new-backend-checklist.md`:

- **typed environment** (`rustnative_core::environment`): `EnvKey`, window
  and per-subtree values (`provide_env`), upward preferences, and the
  framework keys (locale, direction, text scale, size class, colour scheme,
  reduced motion, contrast, posture, safe area, window mode). Windows feeds
  them from the host (`native::host_traits`) at start and on every
  `WM_SETTINGCHANGE`;
- **the invalidation contract** (`docs/invalidation.md`): rendering is now
  incremental — dirty components and readers of changed values render, and
  everything else is reused and spliced into its parent's output — with a
  render log recording each render's `RenderCause`. This is the change with
  the widest reach in the milestone; every existing core, headless, and
  Windows test passes under it unchanged;
- **right-to-left in the layout model**: `EdgeInsets` fields are now
  `start`/`end`, `LayoutStyle::direction` overrides per subtree,
  `LayoutResult::physical_rects` mirrors for hosts without their own
  mirroring (headless), and Windows mirrors natively with
  `WS_EX_LAYOUTRTL` on the logical rectangles (`rendering::direction`),
  switchable at run time on the same native objects;
- **commands** (`rustnative_core::command`): declared per render, bound by
  `Node::with_command`, `MenuItem::command`, and shortcuts, routed through
  the focus chain; disabled everywhere at once. Windows takes shortcuts
  before a key reaches the control and updates bound menu items (state and
  `\tCtrl+S` labels) on `WM_INITMENUPOPUP`;
- **adaptive layout**: window size classes on resize, and
  `ComponentContext::container_classes` for a container's own class,
  reported by both backends after layout; **safe area** honoured by the
  headless backend (zero on Windows);
- **permissions** (`PermissionState`, `PermissionService`,
  `WindowsPermissions` over the consent store), **gesture arbitration**
  (`arbitrate`, applied to scroll-vs-pan on Windows), **thread affinity**
  (compile-time `!Send` proof; `UiThread`; `ThreadAffinity`), the
  **escape-hatch contract** (`NativeHandle<Unchecked|Live>`,
  `rustnative_windows::native_handle`), the **teardown policy** (Windows
  releases capture, unclips the cursor, cancels IME composition on a
  terminating panic and on exit), **per-property mappers**
  (`rustnative_windows::register_mapper`), **cursors**
  (`Node::with_cursor`, `WM_SETCURSOR`), the **surface vocabulary**
  (`Capability::Surface`), **capability grants' shape** (`GrantSet`,
  `Services::scoped`, `Granted`), the Windows **ownership module**, and the
  audit, platform-group, typestate, and permission-mapping documents.

Windows now advertises `Cursors`, `Hover`, `CommandShortcuts`,
`RightToLeft`, `HostTraits`, `SystemAppearance`, and `Permissions`.

**Verified.** Core: `tests/invalidation.rs` (7), `tests/commands.rs` (2),
`tests/affinity.rs`, and unit tests for environment, commands, permissions,
arbitration, grants, handles, affinity. Headless:
`tests/portable_surface.rs` (4). Windows (real windows on this machine):
right-to-left mirroring and switching back, commands through F5 and menu
state, declared cursor, teardown after a deliberate panic with capture and a
clipped cursor, native handle validation and staleness, a replacing text
mapper, host traits reaching the environment, permission states read from
the consent store.

**Not verified / owed.** The `WM_SETTINGCHANGE` path was exercised by its
parts (reading traits, applying them, rendering) rather than by changing the
machine's settings during a test. Hover and cursor were driven by sending
the messages Windows sends, not by moving the physical pointer. Real safe
areas, hinges, host gesture recognizers, and `NotAsked`/`Limited`/`Denied`
permission states belong to the mobile and web backends (33, 35, 36, Web D/E).

### Milestone 45 — Test infrastructure — complete (Windows scope)

**Built.** `crates/rustnative-headless`, a real backend (`HeadlessPlatform`
implements `Platform` and advertises only what its model realizes) that
realizes the tree through the same `TreeSnapshot`/`TreeDiff` a native backend
consumes, lays it out with deterministic metrics (`HeadlessMeasurer`: 8 px per
character, 32 px lines, a text scale for Milestone 41), and drives it with
`HeadlessApp`: `click`, `type_text`, `set_text`, `press`, `tab`, `select_tab`,
`scroll`, `resize`, `open_url`, `lifecycle`, `advance`, `settle`. Every
interaction goes through hit-testing and host focus rules to the same events
`rustnative-windows` produces — a click on a covered or disabled control is an
error, Tab skips disabled and hidden controls, Enter activates a focused
button, typing reports one `TextChanged` per keystroke, a virtual list reports
a range change on its first frame and then only when scrolling leaves the
range (the Windows backend's rule). Time is a `ManualExecutor`; persisted state
is flushed after the same 1.5 s debounce Windows uses.

The accessibility query API (`Query::role(..).name(..)`, `Query::label`,
`Query::text`, `Query::key`) fails with a listing of what the tree contains.
Goldens (`assert_golden!`, blessed with `RUSTNATIVE_BLESS=1`) describe the
realized tree; on Windows, `native::capture` captures a real window through
`PrintWindow` and compares it with a reviewed BMP (per-channel tolerance 12,
at most 1% of pixels). Exhaustive mode fails a test that leaves a task pending,
an HTTP expectation unmet (`MockHttp`), an unexpected request, or an input or
animation request unconsumed. The lifecycle suite tests collisions: process
death with unflushed state, a polite termination, a deep link arriving during
restoration, a configuration change during a load, low memory while
suspended, and kill-and-restore per destination.

Core additions it needed: `Application::with_executor`,
`Application::set_theme`/`ComponentTree::set_theme` (a theme change is a
re-render and diff — the headless test asserts it creates no objects),
`Lifecycle::LowMemory` (flushes first), and `NodeId::local_key`/`owner` for
diagnostics. Windows: `native::memory_watch` turns the low-memory resource
notification into `Lifecycle::LowMemory` on the primary window, one event per
episode.

**Verified.** 30 headless tests (4 unit, 10 interaction, 6 lifecycle
collisions, 4 exhaustive, 6 doc tests); Windows: the visual golden
(`tests/goldens/windows/counter.bmp`, blessed on this machine: Windows 10
19045, 96 DPI), the low-memory flush-then-notify path, and the watcher's
start/stop. CI gains a `headless` job, run twice — normally and pinned to one
core with one test thread as the low-end profile.

**Not verified / owed.** The real low-memory notification was not provoked
(the test posts the message the watcher posts). The visual golden holds on
this machine class; another DPI or font set needs its own blessing. Owed by
Milestone 37: the host-side device simulator and the hardware-in-the-loop
runner; by Milestones 35–37: the device and emulator matrix. Preview goldens
(`C55-3`) arrive with previews in Milestone 43.

### Core work shared by the remaining targets — complete

`Clock` (`SystemClock`, `ManualClock`, `Services::clock`, `Executor::now`); the
single-threaded executor seam (`LocalExecutor`, `LocalPool`,
`ComponentContext::spawn_local`, polled by `pump_tasks`, with a wake that fires
before the backend installs its waker remembered and delivered); and
`rustnative-types`, the `no_std` crate for geometry, `Color`, and `Scalar`,
built for `thumbv7em-none-eabihf` here and in CI. Verified by unit tests, three
integration tests (a `!Send` task delivered on virtual time; cancelled with its
owner and never resumed; `now` and `sleep` on one clock) and a Windows test in
which a `!Send` task spawned during the first render runs through the real
message loop.

---

## Previous: Milestone 32 — Packaging and deployment (Windows) — complete.

### What was built

`crates/rustnative-build`, run from an application's `build.rs`: it reads
`rustnative.toml` and writes the `.ico` (a PNG wrapped into a PNG-compressed icon
entry — six bytes of header, sixteen of directory, then the PNG, so no
image library), the application manifest, and the `.rc` carrying both plus
`VERSIONINFO`, compiles them with the SDK's `rc.exe`, and links the result.
A machine without the SDK gets a Cargo warning, not a failed build.

`rustnative package windows --format zip|msix|all [--sign <pfx> --password-env VAR]`:
a reproducible portable zip (sorted entries, fixed timestamps and
attributes, stored entries, `SHA256SUMS`), and an MSIX whose
`AppxManifest.xml` — identity, publisher, version, logos, and one
`uap:Protocol` per URL scheme from Milestone 30 — comes from the same
`rustnative.toml` the running application's state store and single-instance mutex
use, packed with `makeappx` and optionally signed with `signtool`.

The example now carries its own `rustnative.toml` and `build.rs`, so the
framework's own application is built the way it tells others to build
theirs.

Dependencies: `crc32fast` and `sha2` for the zip's checksums; `toml` moved
from 0.9 to 1.x in both new crates, which also resolved a duplicate
`winnow` that `cargo clippy`'s `multiple_crate_versions` flagged.

### What it found

1. **The icon could be built without an image crate.** An `.ico` may hold a
   PNG directly since Vista, so "convert the icon" is a header and the
   file's own bytes — no decoding, no resizing, no dependency.
2. **`makeappx` needs logos that exist, not logos that are right.** A
   package with no images does not build, so `rustnative` writes a placeholder PNG
   (assembled byte by byte, with correct CRCs and a stored deflate block)
   when the project has no PNG icon. It is a placeholder, and says so.

### Verified, and how

Fifteen unit tests in `rustnative-build` cover the ICO writer (including a
256-pixel icon, which an `.ico` records as zero), the manifest's contents
and its XML escaping, the `.rc` text and its quoting, and SDK discovery.
Six integration tests in `rustnative` build a generated project for real and then
read the results back the way Windows does: `GetFileVersionInfoW` /
`VerQueryValueW` find the `ProductVersion` `rustnative.toml` declared,
`FindResourceW` finds the `RT_MANIFEST` resource, two packaging runs
produce a byte-identical zip whose `SHA256SUMS` matches an independently
computed hash of the executable, `makeappx unpack` round-trips the MSIX and
its manifest (identity, version, executable, `runFullTrust`, logos), and
signing with a missing certificate is refused before anything is built.

```text
cargo fmt --all -- --check                                        clean
cargo clippy --workspace --all-targets --all-features -D warnings clean
cargo test --workspace                                            passing; 3 ignored (M25, unchanged)
cargo doc --workspace --no-deps (RUSTDOCFLAGS=-D warnings)        clean
cargo +1.85 check --workspace --all-targets                       clean
cargo deny check                                                  clean
```

### Not verified on this machine

**Signing with a real certificate.** No certificate was created, imported,
or used: this machine's certificate store is untouched. What is tested is
the argument construction (SHA-256, the password read from an environment
variable and never from the command line) and the refusals; `signtool`
itself was not run against a `.pfx`. The same applies to *installing* the
MSIX, which requires a signature from a trusted certificate.

The resource path was exercised with the SDK present; the "no SDK" branch
(a warning, and a build that continues) was read rather than run.

### Platform availability

Nothing outside Windows has been run at all, because nothing outside Windows
has a backend yet. Two of the planned targets are also blocked on hardware
rather than on work: this project has no macOS machine and no iOS device, so
Milestones 33 and 36 cannot be built or verified here. They remain fully
planned and fully specified (`PLAN.md`, 2.13 and section 8); the build order
follows what can be verified, and neither milestone will be called complete
on reasoning alone. The same rule covers every other target: a backend
advertises a `Capability` only once it realizes it, and this file records
what was run and on what.

### Syntax availability

`PLAN.md` 2.9 states that the declarative tree has two equal spellings. One of
them exists: the builder syntax is what Milestones 1–32 were built and verified
in, and what every test, example, and doc example in this repository currently
compiles.

The markup syntax — `.rsx` files and the `rsx!` macro alike — is **specified
and not implemented.** There is no `rustnative-markup` or `rustnative-macros`
crate, no `markup` feature, no `rustnative_build::compile_rsx()`, no `.rsx`
tooling in `rustnative` (`fmt`, `expand`, `lsp`, source-mapped diagnostics),
and no equivalence suite yet; Milestone 53 is that work. The markup in
`README.md` and `PLAN.md` is the specification those tests will be written
against, not code that compiles today, and it is recorded here rather than left
to be inferred from a missing crate.

The same rule that governs platforms governs this: nothing claims the markup
syntax is available until it runs, and when it does, "runs" will mean the
equivalence suite passing over every node kind and modifier through both
carriers, and a `.rsx` diagnostic reported at its source position — not a
demo compiling.

### Style availability

`PLAN.md` 2.14 states that the resolved style has two spellings. One of them
exists: `Theme`, `ComponentStyle`, `VisualStyle`, the state variants, and
`TreeSnapshot::from_node_with_theme` are implemented and verified on
Windows/Win32 (Milestone 21), and every style in this repository is written that
way.

The second spelling — the declaration vocabulary, the utility classes, and the
`app.css` theme file — is **specified and not implemented.** There is no
`rustnative-style` crate, no `with_class`/`class` attribute, no
`rustnative_build::compile_styles()`, no `app.css` handling, and no per-backend
style capability table or unit mapping; Milestone 58 is that work. The
`bg-primary rounded-lg p-4` examples in `README.md` and `PLAN.md` are the
specification its tests will be written against, not code that compiles today.

The Windows backend's style support is likewise narrower than the vocabulary
being planned, and narrower than `VisualStyle` itself: foreground and background
colours and fonts are realized through `WM_CTLCOLOR*`, `WM_SETFONT`, and
`WM_ERASEBKGND`, while the border colour and corner radius a `VisualStyle` can
carry are not applied by this backend, and shadows, gradients, and transforms do
not exist in the model yet. Which of those become realized, approximated, or
unavailable is Milestone 58's capability table, and it will be recorded here per
backend as it is answered rather than assumed from this plan.

---

## Previous: Milestone 31 — Developer CLI and project tooling — complete.

### What was built

`crates/rustnative` (package `rustnative-cli`, binary `rustnative`): `new`, `build`, `run`,
`check`, `test`, and `doctor`. `rustnative.toml` (`config`) describes a project;
`project` creates one from templates and finds the one you are standing in;
`platform` knows every platform on the roadmap and which has a backend;
`toolchain` runs Cargo and locates the MSVC build tools and the Windows SDK;
`doctor` reports both, as a table or as JSON. Errors carry exit codes: 2
usage, 3 no backend for that platform, 4 a missing toolchain, 1 otherwise.

Dependencies: `clap`, `serde`, `serde_json`, `toml` — all already in this
workspace's lockfile through existing dev-dependencies, all MIT/Apache-2.0.

### What it found

1. **`trailing_var_arg` alone does not pass flags through.** `rustnative test
   --offline` was parsed as `rustnative`'s own flag and refused; the pass-through
   argument also needs `allow_hyphen_values`. The test that runs
   `rustnative test --offline` caught it.
2. **A speculative `target_triple` had no caller.** It was written to
   return `None` for every platform (the only backend is the host's), which
   clippy correctly read as a method that ignores its receiver. Removed;
   cross-compiled platforms can add it when they exist.

### Verified, and how

Eleven integration tests run the real binary in real folders: a generated
project **compiles** (`cargo check --offline` against this workspace) and
**builds to an executable** (`rustnative build windows`, and the `.exe` is there);
`rustnative.toml` is generated valid and found from a subfolder; each of the six
platforms without a backend is refused by name with its milestone and exit
code 3; an unknown platform is a usage error (2); a broken `rustnative.toml` names
the field (`app.version`); running outside a project says `rustnative new`;
`doctor --json` is valid JSON that reports this machine's real rustc,
Cargo, MSVC tools, and SDK paths; `doctor`'s table names the milestones;
`rustnative test` passes both filters and flags through to Cargo; and creating a
project over an existing folder is refused. Seventeen unit tests cover
config validation (every invalid field named, unknown fields refused),
template filling, SDK version comparison, and the exit-code mapping.

```text
cargo fmt --all -- --check                                        clean
cargo clippy --workspace --all-targets --all-features -D warnings clean
cargo test --workspace                                            passing; 3 ignored (M25, unchanged)
cargo doc --workspace --no-deps (RUSTDOCFLAGS=-D warnings)        clean
cargo +1.85 check --workspace --all-targets                       clean
cargo deny check                                                  clean
```

### Not verified on this machine

A machine *without* the MSVC tools or the Windows SDK: `doctor` reports
what it finds, and the "missing" branches were exercised only by reading
them, not by uninstalling a toolchain. `rustnative new` against published crates
(`rustnative-core = "0.1"`) cannot be resolved until the crates are
published, so the generated-project tests use `--framework-path`.

---

## Previous: Milestone 30 — Persistence and navigation — complete.

### What was built

Core: `navigation` (`Route`, `Router`, `url_path`, `NavigationStack`,
`NavigationCommand`, `Navigator`), `persistence` (`StateStore`,
`MemoryStateStore`, `Persisted`, and the per-window write buffer),
`Lifecycle`, `Node::hidden`, `Node::tab_bar`, `Event::{TabSelected,
DeepLink, Lifecycle}`, component **key paths** in the component tree, and
`Application::{flush_state, has_unsaved_state, lifecycle, open_url}`. serde
is now a normal dependency of the core (the old optional `serde` feature is
kept, empty, so manifests that name it still build).

Windows: hidden nodes realized with `ShowWindow` and skipped by Tab;
`SysTabControl32` tab bars with controlled selection; `FileStateStore`;
`native::lifecycle` (idle flush timer, `WM_QUERYENDSESSION`,
`WM_ENDSESSION`, `WM_POWERBROADCAST`, final `Terminating` after the loop,
primary-window placement save/restore); `native::single_instance` (named
mutex, message-only listener, `WM_COPYDATA` handoff, launch URL from the
command line); `WindowsPlatform::with_app_id`. New capabilities:
`StatePersistence`, `DeepLinks`, `Lifecycle` — and `CustomDrawing` /
`NativeSurfaces`, which Milestone 29 realized but did not advertise.

### Deviations from the plan, and why

- **No `ComponentContext::navigator()`.** Navigation commands travel through
  the existing child-to-parent `Callback`, which already delivers after the
  requesting event and needs no new runtime machinery. A `Navigator` wraps
  one.
- **No separate `TabHost` type.** A tab host is a tab bar plus pages marked
  `hidden` — a pattern three lines long, shown in the README and example,
  rather than a type that would hide it.
- **Deep-link scheme registration** is packaging's job (Milestone 32).

### What it found

1. **Hiding did nothing on a window's first render.** `IsWindowVisible`
   answers "no" for every child of a window not yet shown, so the check
   "already hidden?" skipped hiding exactly when a hidden page is first
   created. The tab-bar test caught the second page visible; visibility is
   now decided by the window's own `WS_VISIBLE` bit.
2. **An inserted method stole an `#[allow]`.** New `ComponentTree` methods
   were placed between an existing function's attributes and its
   signature, so the `expect_used` exemption silently moved to the wrong
   function; clippy caught it, and the methods now sit above the doc
   comment.
3. **`Callback` could not be cloned for non-`Clone` messages.** Its derived
   `Clone` required `M: Clone`, though cloning a callback copies an
   endpoint, never a message. It is now implemented by hand.

### Verified, and how

Core: route tables and a property test that any parameters survive
`build` then `matches`; navigation stacks keep entry identity across
push/pop/replace/reset and round-trip through serde; black-box tests that a
pushed screen leaves the one below mounted with its state (mount counts),
that a deep link is routed by the root, that persisted state survives a
whole `Application` being dropped and rebuilt, that it follows component
keys rather than positions, and that `Suspending` flushes before the
component hears it; hidden subtrees are absent from the accessibility tree.

Windows: `FileStateStore` round-trips, survives a forged half-written
temporary file (reads the committed value, deletes the debris on reopen),
detects a forged hash collision, and stores long keys with reserved
characters. Six integration tests in real windows: choosing a tab by
mouse reports `TabSelected` and swaps the visible page without destroying
the hidden one; Tab traversal never focuses a hidden page's button;
`WM_QUERYENDSESSION` writes buffered state before the component hears
`Terminating`; the idle-flush timer writes buffered state; a second claim
of the same app id sees the first, and a `WM_COPYDATA` handoff delivers
`DeepLink` to the running window; and the primary window reopens exactly
where it was closed.

```text
cargo fmt --all -- --check                                        clean
cargo clippy --workspace --all-targets --all-features -D warnings clean
cargo test --workspace                                            passing; 3 ignored (M25, unchanged)
cargo doc --workspace --no-deps (RUSTDOCFLAGS=-D warnings)        clean
cargo +1.85 check --workspace --all-targets                       clean
cargo deny check                                                  clean
```

### Not verified on this machine

A real sign-out, shutdown, or sleep (the tests send the messages Windows
would); a genuine second process (the handoff is exercised in one process
through the same mutex, listener window, and `WM_COPYDATA`); restoring a
placement whose monitor has since been disconnected (the
`MonitorFromRect` guard is in place but no monitor was unplugged); and a
screen reader's view of hidden pages beyond the accessibility tree's own
test.

---

## Previous: Milestone 29 — Graphics / custom rendering escape hatch — complete.

### What was built

`rustnative_core::graphics`: `DrawList` (retained, cheaply cloned, compared by
pointer first), `DrawCommand`, `Paint`, `Path`, `ImageData`, `Transform2D`,
`Vec2`/`RectF` (all on `Scalar`, so the node tree stays `Eq`), hit testing
that honours transforms and clips, and `SurfaceId`. Two new node kinds,
`Node::canvas` and `Node::native_surface`; `PointerEvent::region`;
`Event::SurfaceResized`.

On Windows, `native::graphics`: one Direct2D translation (`d2d`) used by
both the canvas window and the pixel tests; the canvas window class, which
keeps its draw list and render target on the window so painting never
re-enters the runtime, and rebuilds a lost device on the next paint; the
surface window class and a per-thread `SurfaceId` table; and the public
`rustnative_windows::native_surface` returning a `SurfaceHandle` that
implements `raw-window-handle` 0.6.

**Dependency changes.** `raw-window-handle` 0.6 (MIT/Apache/Zlib) and
`windows-numerics` 0.3 are new. `windows`/`windows-core` are pinned to 0.62
instead of `>=0.60, <=0.62`: Direct2D's transform type lives in
`windows-numerics`, which `windows` does not re-export, so the two must be
the exact pair `windows` 0.62 uses — a range could resolve to a `windows`
whose `Matrix3x2` is a different type.

### What it found

1. **A clip rotated twice.** A clip is a Direct2D layer with a geometric
   mask, and the first version passed the current transform as the layer's
   `maskTransform` — but Direct2D already carries the mask through the
   world transform in force at the push. The rotated-clip pixel test saw a
   square turned 90 degrees instead of 45. The mask transform is identity.
2. **`PushAxisAlignedClip` would have been wrong for rotation.** It clips to
   the bounding box of a rotated rectangle, which disagrees with the hit
   test; clips are layers with geometric masks instead, and the same pixel
   test pins that a bounding-box corner is outside a rotated clip.

### Verified, and how

Nine pixel tests draw through the production translation into a WIC bitmap
and read the pixels back: exact fill coverage, a half-covered antialiased
edge rendering neutral grey, clips (axis-aligned and rotated), transforms
scoped to their push, an opacity layer compositing overlapping shapes once,
a filled path, an image scaled into its rectangle, and text rasterized where
it was placed. Four integration tests drive real windows: a changed draw
list reaches the same canvas window with one render and no device rebuild;
a discarded device is rebuilt on the next paint; a pointer press on each
half of a canvas reports hit region 1 and 2; and a native surface is laid
out at its declared size, reported once, handed out as a Win32
`raw-window-handle` for exactly that window, and unavailable once removed.
Core tests cover transform composition and inversion, hit testing under
transforms, clips, and stacking, draw-list equality, image validation and
premultiplication, and that a new drawing updates only its canvas without
invalidating layout.

```text
cargo fmt --all -- --check                                        clean
cargo clippy --workspace --all-targets --all-features -D warnings clean
cargo test --workspace                                            passing; 3 ignored (M25, unchanged)
cargo doc --workspace --no-deps (RUSTDOCFLAGS=-D warnings)        clean
cargo +1.85 check --workspace --all-targets                       clean
cargo deny check                                                  clean
```

### Not verified on this machine

A real GPU swapchain attached to a native surface (wgpu is not a dependency
of this workspace; the test proves the handle is the right window, not that
a particular graphics API accepts it). A genuine device loss: the recovery
path is exercised by discarding the render target, which is exactly what
the `D2DERR_RECREATE_TARGET` branch does, but no driver reset was induced.

### Deliberately not built

Shader-backed drawing inside a `DrawList` — that is what a native surface
is for. Hit regions are not automatically accessibility elements: a region
has an id but no name, and an unnamed element is noise to a screen reader;
a canvas describes its content with Milestone 26's `VirtualElement`s, which
already become UI Automation fragments.

---

## Previous: Milestone 28 — Virtualized lists and large data sets — complete.

### What was built

`rustnative_core::virtualization`: `VirtualListStyle` (item count, `ItemExtent`
fixed or estimated, overscan, axis), `ExtentCache` (arithmetic for fixed
items, a Fenwick tree for measured ones), `VirtualRange::compute`, and
`ScrollAnchor`. `Node::virtual_list`/`virtual_list_with_layout` build a
scrollable column or row carrying the declaration; `Node::with_item_index`
tags each realized row. The layout engine places rows at their item offsets,
reports a content size covering every item, and reports what estimated rows
measured (`LayoutResult::measured_items`). Snapshots give virtual-list items
their position in the whole list for accessibility.

On Windows: `rendering::virtual_list` (per-list caches, ranges, anchors),
`rendering::pool` (recycling native windows between a removal and an
insertion in the same render), and `native::virtual_list`, which reports
range changes to components in a bounded loop so a range change answered by
new rows never nests one dispatch inside another.

**API change.** `LayoutEngine::layout_result_with` took an unused
`_scroll_offsets` map "for API symmetry"; it now takes the virtual lists'
`ExtentCache`s, which layout genuinely needs. Scroll offsets remain a
viewport transform, not a layout input.

### What it found

1. **A virtual list sized itself to its data.** A `Fill` child is given its
   preferred size plus a share of the free space, and a container's preferred
   size was the sum of its children — so the realized rows made the list
   taller, which made the viewport taller, which realized more rows: the
   first native run realized 11,087 of them. A virtual list now reports no
   preferred size of its own; it is sized by its parent, as a list that is
   shorter than its content has to be.
2. **Estimated rows ignored their own declared size.** Intrinsic measurement
   of a container ignores its `Fixed` height (containers are sized by their
   parents), so an estimated row that declared 50 px measured 0. A row's
   declared main-axis size is now what it measures as.
3. **A scheduler task could stay registered forever** (pre-existing, found
   because this milestone's larger test binary shifted timing). `TaskScope`
   inserted a task after spawning it and removed it again only if
   `is_finished()` was already true — but the settlement callback that
   removes a finished task runs as the task's *last statement*, before the
   executor marks it finished, so a task that settled before the insert was
   never removed. Registration and settlement now decide under one lock,
   with a tombstone for "settled before registered", and a task settles
   exactly once however it ends (a cancel racing completion used to settle
   twice). `a_task_settles_exactly_once_however_it_ends` pins the second;
   the registry test now waits on the registry itself instead of a fixed
   20 ms sleep.

### Verified, and how

Five integration tests (`native::virtual_list_integration`) drive a real
window through the production message loop, scrolling with posted
`WM_MOUSEWHEEL` messages aimed inside the list: 100,000 items realize exactly
12 native rows for a 210 px viewport; a scroll inside the range renders
nothing and crossing one renders exactly once; rows that stay in range keep
their `HWND`, and rows that scroll in are realized on the windows of rows
that scrolled out (only rows beyond the old range's size are created);
inserting ten items above the viewport leaves the anchored row at the same
screen position (`GetWindowRect`); and estimated rows are measured and move
every later offset. Core has unit tests for extents, ranges, anchors, and
virtual layout, and property tests that Fenwick offsets equal naive prefix
sums, that `index_at` finds the containing item, that a range always covers
its viewport within overscan, that ranges are monotone in the offset, and
that anchoring survives any number of insertions above. Benchmarks
`virtual_list/range_100k` and `layout/virtual_10k` are in
`core_benchmarks.rs`.

```text
cargo fmt --all -- --check                                        clean
cargo clippy --workspace --all-targets --all-features -D warnings clean
cargo test --workspace                                            passing; 3 ignored (M25, unchanged)
cargo doc --workspace --no-deps (RUSTDOCFLAGS=-D warnings)        clean
cargo +1.85 check --workspace --all-targets                       clean
cargo deny check                                                  clean
```

### Not verified on this machine

Scrolling with a physical wheel or touchpad (the tests post the same
message a wheel produces), and a screen reader announcing a row's position.
Recycling is limited to rows of a virtual list, by design: anywhere else a
reused window would change which node it belongs to for no measured benefit.

---

## Previous: Milestone 27 — Animations and transitions — complete.

### What was built

`rustnative_core::animation`: animatable properties and typed animated values,
`Transition` (duration and easing curve, or a mass/stiffness/damping spring,
either with a start delay), `Animation` (from/to, repeat, autoreverse, fill,
reduced-motion behaviour), and `Timeline`, the platform-free evaluator that
turns elapsed time into per-property frames and carries velocity across a
retarget. Nodes declare transitions (`Node::with_transition`) and opacity;
components request and cancel animations through `ComponentContext`.

On Windows, `native::animation`: a process-wide frame driver thread paced by
`DwmFlush`, per-window frame handling, and `native::rendering::animated`, the
per-node overrides a frame writes so geometry, opacity, and colours change
without touching the rendered tree. Opacity is realized with `WS_EX_LAYERED`
and `SetLayeredWindowAttributes`, geometry with `SetWindowPos`.

### What it found

1. **The overdamped spring started at the wrong end.** The two-root solution
   was written with its coefficients transposed, so an overdamped spring
   began at `+1` — the far side of the target — and swung across it. It is now
   solved from `x(0) = -1, v(0) = 0` with the weights that follow;
   `an_overdamped_spring_never_overshoots` pins it, and needed a 2000-step
   window, because the correct spring settles slowly, as an overdamped one
   should.
2. **A transition jumped, then animated.** A transition is driven by the
   rendered value changing, so by the time the backend noticed, the native
   object had already been moved to the new value: the animation then ran
   from the target to the target. The renderer now pins the *previous* value
   as the animation's start when it collects the transition
   (`pin_transition_start`), before applying the new one.
3. **A finished transition left its property pinned.** Holding the final
   value as an override meant a later relayout could not move the node. A
   transition now completes with `Frame { value: None }`: the override is
   dropped and the node returns to exactly what the tree says.
4. **`Fill::Forwards` held the wrong value.** It held the animation's `to`,
   which is wrong for an autoreversed animation that ends back at its start.
   It now holds the value actually evaluated at completion.

### Verified, and how

Six integration tests (`native::animation_integration`) drive a real window
through the production message loop with a `ManualFrameClock` installed and
frame messages delivered by hand, so timing is deterministic rather than
wall-clock: a transition animates a node **without a single rerender**;
frames stop — and the driver goes idle — once everything settles; an explicit
animation reaches its target and reports `Event::AnimationFinished`; opacity
makes the window layered and steps its alpha; reduced motion arrives at the
target immediately; and animations end when their node or window does. The
core timeline has its own unit tests, plus property tests over interruption
and completion.

```text
cargo fmt --all -- --check                                        clean
cargo clippy --workspace --all-targets --all-features -D warnings clean
cargo test --workspace                                            passing; 3 ignored (M25, unchanged)
cargo doc --workspace --no-deps (RUSTDOCFLAGS=-D warnings)        clean
cargo +1.85 check --workspace --all-targets                       clean
cargo deny check                                                  clean
```

Layered child windows need a manifest declaring a supported OS, which a
`cargo test` binary does not get by default.
`crates/rustnative-windows/build.rs` embeds `tests.manifest` into the test
executables, so the opacity test exercises the real code path instead of
silently proving nothing.

### Not verified on this machine

Frame *pacing* against a real display: the tests drive a manual clock, so
they prove what each frame computes and that frames stop, not that `DwmFlush`
lands them on the compositor's rhythm. Reduced motion was exercised by
setting the preference directly and by a synthesized `WM_SETTINGCHANGE`, not
by toggling the setting in Windows' own settings UI.

---

## Previous: Milestone 26 — Full accessibility bridge — complete.

### What was built

`rustnative-core::accessibility`: the full portable model the milestone lists
(roles, names, descriptions, values and ranges, states, actions,
relationships, focus, virtualized children) plus live regions, automation
ids, and virtual elements, and `AccessibilityTree`, the portable projection
(flattened structure, resolved relationships, computed names). On Windows,
`native::uia`: real UI Automation server-side providers for every realized
node, fragments for virtual elements, seven control patterns routed to the
component as `Event::AccessibilityAction`, and property/structure/live-region
events.

### What it found

1. **The runtime lookup trusted any window's `GWLP_USERDATA`.** The message
   loop resolves the root window of *every* message it sees, and the UI
   thread also owns windows this crate never created — the system IME
   window, OLE's hidden window (since Milestone 25), UI Automation's own.
   `RuntimeSlot::get` read their slot as a `*mut Runtime`. It now checks the
   window's class atom first; `a_foreign_windows_slot_is_never_read_as_a_runtime`
   pins it.
2. **COM objects this crate implemented were agile.** The `windows` crate's
   `#[implement]` makes objects agile by default (`IAgileObject` plus the
   free-threaded marshaler), so a caller in another apartment calls them
   directly on its own thread. The providers — and Milestone 25's drop
   target — reach the window's `Runtime`, which belongs to the UI thread
   alone. The first UIA tests "passed" with calls arriving on UI Automation's
   threads. Every such object is now `Agile = false`, so COM marshals calls
   into the UI thread's apartment, and `with_runtime` refuses (and in debug
   builds asserts) when called on a thread that does not own the window, so
   the mistake cannot come back silently.
   `a_runtime_is_never_resolved_from_a_thread_that_does_not_own_the_window`
   pins the guard.
3. **Every container exposed a meaningless pane.** A container is two
   windows (a viewport and an inner content window that makes scrolling a
   window move), and the inner one appeared to assistive technology as an
   unlabeled child. With virtual elements it was worse: a client asking for
   the canvas's children got the pane. The inner window now reports itself
   as neither a control nor a content element, so the control view presents
   a node's real children directly.

### Verified, and how

The four UIA tests (`native::uia_integration`) run a real `IUIAutomation`
client in a multithreaded apartment on another thread while the test thread
pumps the production message loop — the arrangement a screen reader in
another process produces. They read names, descriptions, control types,
automation ids, LabeledBy, and set position; toggle a custom check box and
set a custom slider through their patterns and read back the state the
*component* rendered; navigate, invoke, and outlive a canvas' virtual
elements (a removed element answers `UIA_E_ELEMENTNOTAVAILABLE`); and
receive a live-region announcement through a registered event handler.

```text
cargo fmt --all -- --check                                        clean
cargo clippy --workspace --all-targets --all-features -D warnings clean
cargo test --workspace                                            passing; 3 ignored (M25, unchanged)
cargo doc --workspace --no-deps (RUSTDOCFLAGS=-D warnings)        clean
cargo +1.85 check --workspace --all-targets                       clean
cargo deny check                                                  clean
```

### Not verified on this machine

Narrator and NVDA themselves were not driven: the client used is UI
Automation's own, which is what they are built on, but a screen reader's
speech output is not something a test here can observe. The macOS, iOS,
Android, and Linux accessibility bridges belong to backends that do not exist
yet.

---

## Previous: Milestone 25 — Advanced input system — complete.

### What was built

`rustnative-core::input` holds the portable half: pointer, wheel, gesture,
IME, clipboard, drag, and gamepad payloads; per-node `InputInterest` so
high-frequency streams are opt-in; `GestureRecognizer` and `GamepadPoller`,
the two pieces of input *logic* that do not depend on a platform and so exist
once; and `InputRequests` for deferred pointer capture and drag feedback.
`rustnative-windows::native::input` realizes it: mouse and hover,
`WM_POINTER*` touch and pen with implicit per-contact capture, top-level
mouse capture with lost capture reported as a cancel, IMM32 composition,
clipboard shortcuts and change notifications, an OLE `IDropTarget` per
window, and `XInput` polling that runs only while a node asks.

### What it found

1. **`WindowsClipboard::write_text` had never worked.** It opened the
   clipboard with a NULL owner, and Microsoft documents that `EmptyClipboard`
   then leaves the clipboard ownerless, which makes the following
   `SetClipboardData` fail. Every write through the shipped service returned
   "SetClipboardData failed". Nothing had ever written the clipboard for
   real until this milestone's paste test did. Fixed with a throwaway
   message-only owner window per write; `services::clipboard::tests::
   written_text_reads_back` guards it.
2. **A COM object outlived its apartment.** The accessibility annotator kept
   its `IAccPropServices` in a `thread_local!`, released at thread exit —
   after the thread's COM apartment was torn down. It never crashed before
   only because COM was never initialized on the UI thread, so the object
   was never created and annotation silently did nothing. Initializing OLE
   for drag-and-drop made it real, and one native test crashed the process
   with `STATUS_ACCESS_VIOLATION`. The service is now owned by the
   per-window annotator and released with the window. (Side effect worth
   stating plainly: accessible-name annotations on standard controls now
   actually apply at runtime, where before they were a no-op.)
3. **Wheel targeting asked the desktop, not the window.** Scroll-container
   wheel handling used `GetCursorPos` + `WindowFromPoint`, answering with
   whatever window is topmost on the desktop. It now hit-tests within the
   window the message was delivered to, from the screen point
   `WM_MOUSEWHEEL` itself carries.

### Reentrancy, deliberately avoided

`WM_CAPTURECHANGED` (sent from inside `SetCapture`/`ReleaseCapture`) and
`WM_CLIPBOARDUPDATE` (sent cross-thread, deliverable during any
message-processing Win32 call) can both arrive while the window's `Runtime`
is already borrowed. Their handlers only re-post a private message and
return; the posted message is handled on an unnested turn of the loop.

An existing instance of the same hazard was noticed and **not** fixed in this
milestone, because it predates it and changing it alters established event
timing: `SetWindowTextW` on a native `EDIT` sends `EN_CHANGE` synchronously,
which the container forwards to the root `window_proc`, which resolves the
same `Runtime` that `Runtime::dispatch` is still borrowing. It is recorded
under "Known remaining work" below.

### Verified, and how

```text
cargo fmt --all -- --check                                        clean
cargo clippy --workspace --all-targets --all-features -D warnings clean
cargo test --workspace                                            passing; 3 ignored (below)
cargo doc --workspace --no-deps (RUSTDOCFLAGS=-D warnings)        clean
cargo +1.85 check --workspace --all-targets                       clean
cargo deny check                                                  clean
cargo run -p hello-label                                          starts and stays up
```

Native tests added (`native::input_integration`), all against real windows
through the production loop: pointer delivery in node-local coordinates to
interested nodes only (with a mouse tap recognized); capture routing outside
the node, native release, and a lost capture becoming `PointerCancel`; wheel
delivery to interested nodes vs. container scrolling with **zero renders**;
Ctrl+C and `KeyUp`; IME start/commit/cancel on a focusable container; a real
Shell data object for two real files driven through the registered
`IDropTarget` exactly as OLE drives it; controller polling that is opt-in,
backs off when nothing is connected, and is ignored by an inactive window;
and two touch contacts described by real `POINTER_INFO`s that stay
implicitly captured and pinch.

### Not verified on this machine, and why

This development shell runs inside a job object that denies clipboard access
(`OpenClipboard` fails with `ERROR_ACCESS_DENIED` for every process it
starts, PowerShell's `Set-Clipboard` included) and whose `SetCursorPos` does
not produce mouse messages. Three tests need exactly those and are
`#[ignore]`d with that reason rather than weakened into passing:

- `native_pointer_hover_follows_the_real_cursor`
- `native_clipboard_paste_and_change_notification`
- `services::clipboard::tests::written_text_reads_back`

CI runs them (`cargo test --workspace -- --ignored` on `windows-latest`),
and so can anyone on an ordinary desktop session. Also not exercised with
real hardware here: a physical touch screen, pen, or game controller (the
tests stand in for the *device* with a real `POINTER_INFO` or a controller
source; everything after that is production code), and an installed CJK IME
(composition was driven with the IMM32 messages themselves, whose result
strings are empty without an IME).

### Known remaining work (carried forward)

- **Nested `EN_CHANGE` during `SetWindowTextW`** re-resolves a `Runtime`
  that is still mutably borrowed further up the stack (see above).

---

## Previous: second standards-audit remediation pass — complete.

An earlier revision of this document claimed every `Audit.md` finding was
already closed. That claim was wrong, and this pass began by checking it
rather than trusting it: the workspace was built, tested, linted, and read
against the audit finding by finding on a real Windows machine with a real
toolchain. Most findings genuinely were closed. Twelve were not, and several
of those were closed only on paper — the code had moved in the right
direction without the finding's actual requirement being met.

That is worth stating plainly, because a status document that overstates
completion is worse than one that admits a gap: the gap stays, and nobody
looks for it again.

### What this pass found still open, and closed

| Finding | What was actually still missing |
|---|---|
| **P0.2** | `Runtime` still held bare `*mut Application`/`*mut WindowRegistry`, dereferenced at ~12 sites. The audit's recommended `NativeWindowContext::from_hwnd()` step did not exist. |
| **P1.14** | `build_native_menu` wrapped its `HMENU` in `OwnedMenu` only on the success path, so any failure inside `append_menu_item` leaked the whole partially built menu tree. A real resource bug. |
| **P1.16** | The Windows backend consumed exactly one field of the portable accessibility model (`is_focusable`). Role, name, and description reached no Win32 API at all — and every leaf node defaulted to `AccessibilityRole::None`, so the model carried nothing to consume. |
| **P1.17** | No native message-loop test existed. The twelve scenarios the audit names by name were all absent. |
| **P1.21** | `Renderer` was still the god object the audit describes: diff application, HWND creation, styling, accessibility, scrolling, layout, and positioning in one type. |
| **P2.23** | `rustnative-windows` had no `deny(missing_docs)`; only `rustnative-core` did. |
| **P2.28** | `style_override` and `visual_style` were two fields of the same type, distinguished only by name and a comment. |
| **P2.30** | `WM_ERASEBKGND` still created and destroyed a brush on every repaint. |
| **P2.31** | No Win32 return-value classification existed; ignored returns were indistinguishable from unconsidered ones. |
| **P2.32** | `Error::WindowsApi` was still exactly the flat `{ operation, code }` the audit criticized. |
| **P2.33** | A caught component panic had one hard-coded response: terminate. |
| **P2.40** | The `pedantic`/`cargo` groups were on, but none of the individually chosen restriction lints the audit also asks for. |
| Phase 7 | Benchmarks measured one tree size each rather than the 10/100/1k/10k sweep the audit asks for; four of the named properties had no property test; there was no long-running task stress test. |
| Phase 5 #26 | "Add examples to major public APIs" — `cargo test` reported zero doc-tests, so there were none at all. There are now 16, on the crate roots and on `Component`, `Application`, `ComponentContext`, `Node`, `TreeSnapshot`, `LayoutEngine`, `Theme`, `Services`, `MenuBar`, `ManualExecutor`, and `PanicPolicy`. |

All of the above are now closed. See the commit history from
`Close P0.2, P1.14, P1.16, P1.21, P2.30, P2.31 from Audit.md` onward for the
per-finding detail.

### Traceability

Every finding, where it is addressed, and what would fail if it regressed.
This table exists because the previous pass's completion claim could not be
checked: "closed" was an assertion with nothing behind it. Each row below
names a file and a test, so the claim is falsifiable.

| Finding | Addressed in | Verified by |
|---|---|---|
| **P0.1** identity | `core/identity.rs` (interning table) | `distinct_keys_never_collide` (proptest) |
| **P0.2** raw-pointer model | `windows/native/context.rs`, `native/user_data.rs` | `native::context::tests`, `native_window_creation_reentrancy` |
| **P0.3** dialog COM threading | `windows/services/mod.rs` (`run_sta`) | dedicated STA thread + paired `CoUninitialize` |
| **P1.4** notifications | `windows/services/notifications.rs` | persistent message-only host, `NIM_MODIFY` |
| **P1.5** task retention | `core/scheduler/mod.rs` (`TaskScope`) | `a_long_lived_scope_does_not_accumulate_handles_across_many_task_generations` |
| **P1.6** cancellation semantics | `core/scheduler/mod.rs` docs | `cancelled_tasks_never_deliver_a_result_even_under_concurrent_completion` |
| **P1.7** injectable executor | `core/scheduler/executor.rs` (`Executor`) | `dedicated_executor_is_independent_of_the_shared_one` |
| **P1.8** effect dependencies | `core/component/effects.rs` (`EffectDependencies`) | `dependencies_use_partial_eq_not_hashing` |
| **P1.9** stale event routing | `core/component/tree.rs` (`dispatch`) | `dispatch_to_unknown_target_is_not_delivered_to_root`, `native_message_after_object_removal_is_rejected` |
| **P1.10** structured render errors | `core/component/error.rs` (`RenderError`) | `component::tree::tests` |
| **P1.11** tree topology indexing | `core/reconcile/snapshot.rs` (children/depth indexes) | `children_of_and_ordered_nodes_use_the_precomputed_index`, `scaling/*` benches |
| **P1.12** layout invalidation | `core/reconcile/diff.rs` (`is_layout_relevant_change`) | `tree_diff_emits_update_when_only_visual_style_changes` |
| **P1.13** menu id aliasing | `windows/native/menu.rs` (`checked_add`) | `building_a_menu_assigns_one_command_id_per_selectable_item` |
| **P1.14** menu RAII | `windows/native/menu.rs` (`OwnedMenu`) | `a_failure_partway_through_building_does_not_leak_the_partial_menu` |
| **P1.15** modern file dialogs | `windows/services/dialogs.rs` (`IFileOpenDialog`) | owner-window resolution via `native::window_handles` |
| **P1.16** accessibility | `windows/native/rendering/accessibility.rs`, `core/node.rs` defaults | `rendering::accessibility::tests`, `native_focus_traversal` |
| **P1.17** native tests | `windows/native/harness.rs`, `native/integration.rs` | 19 scenarios against real windows |
| **P1.18** deterministic scheduler | `core/scheduler/executor.rs` (`ManualExecutor`) | `manual_executor_sleep_only_resolves_after_advancing_past_its_deadline` |
| **P1.19** layout overflow | `core/layout/{engine,measure}.rs` (saturating) | `measurement_never_panics_on_pathologically_large_input`, `layout_geometry_is_always_non_negative` |
| **P1.20** identity wraparound | `core/identity.rs`, `core/scheduler/mod.rs` (`checked_add`) | documented exhaustion panics, `#[allow]` with reason |
| **P1.21** responsibility boundaries | `core/` module split, `windows/native/rendering/` split | every module's own tests compile against a narrow surface |
| **P2.22** monolith split | `core/` and `windows/` module maps | — |
| **P2.23** documented public API | `#![deny(missing_docs)]` in both crate roots | the lint; `RUSTDOCFLAGS: -D warnings` in CI |
| **P2.24** field exposure | `VisualStyle`/`Theme`/`WindowState`/`TreeDiff` accessors | — |
| **P2.25** snapshot mutability | `core/reconcile/snapshot.rs` (documented exception) | — |
| **P2.26** `ServiceFuture` | removed; rationale in `core/services/mod.rs` | — |
| **P2.27** `async-trait` isolation | confined to `services/` in both crates | `grep async_trait` finds nothing outside `services/` |
| **P2.28** style phases | `core/style/phase.rs` (`StyleOverride`/`ResolvedStyle`) | `an_unthemed_snapshot_leaves_the_resolved_style_at_its_resting_default` |
| **P2.29** reverse HWND lookup | `windows/native/registry.rs` (`by_hwnd`) | `id_for_hwnd_is_a_real_reverse_lookup` |
| **P2.30** erase-background brush | `windows/native/rendering/styling.rs` (cache) | `the_background_brush_cache_survives_repeated_erases_without_exhausting_gdi` |
| **P2.31** Win32 result handling | `windows/native/win32.rs` | applied at every call site; `best_effort` asserts in debug |
| **P2.32** error context | `windows/error.rs` (`NativeContext`, `Win32Category`) | `display_names_the_window_and_node_but_never_the_raw_handle` |
| **P2.33** panic policy | `core/panic.rs`, honored in `native/message_loop.rs` | three `native_panic_policy_*` tests |
| **P2.34** property tests | `core/tests/property_tests.rs` | 8 properties |
| **P2.35** benchmarks | `core/benches/core_benchmarks.rs` | 11 benchmarks incl. 10/100/1k/10k sweeps |
| **P2.36** docs match reality | `README.md`, this file | — |
| **P2.37** toolchain components | `rust-toolchain.toml` | `cargo-audit` installed in CI, not as a rustup component |
| **P2.38** MSRV + stable | `.github/workflows/ci.yml` | `cargo +1.85 check` passes locally too |
| **P2.39** CI | `.github/workflows/ci.yml` | fmt, clippy, test, doc, deny, audit, MSRV, Windows |
| **P2.40** lint policy | `Cargo.toml` `[workspace.lints]`, `clippy.toml` | `clippy -D warnings` clean with restriction lints on |

### Bugs the new tests found

The point of P1.17 was never the test count. Within minutes of the native
integration suite existing, it found four real defects that every prior
review pass had missed — three of them in code that had been read,
documented, and signed off:

1. **Every caught component panic discarded its message.**
   `panic_payload_message(&payload)` coerced the `Box<dyn Any + Send>`
   *itself* to `&dyn Any` rather than dereferencing it, so both
   `downcast_ref` calls missed and the boundary reported "component panicked
   with a non-string payload" for every panic. The one piece of diagnostic
   information a caught panic carries was being thrown away.

2. **A task completing before a backend installed its waker was never
   collected.** A component's first render runs inside `Application::new`,
   strictly before a platform backend can install a waker, so any task
   spawned there and finishing in that window left a result in the queue with
   nothing scheduled to pick it up — delivered on the next unrelated input,
   or never. `Scheduler::set_waker` now fires once immediately if a
   completion is already pending.

3. **`TreeSnapshot::from_node_with_theme` resolved against the wrong field.**
   It read `visual_style` where it meant `style_override`, and was correct
   only because `from_node` happened to initialize both to the same value.
   Splitting the two phases into distinct types (P2.28) turned that latent
   confusion into a compile error.

4. **A declared layout minimum could be silently overridden.**
   `LayoutEngine` applied the available-space cap *after* the constraint
   clamp, so a child declaring `min_width: 150` inside a narrower parent came
   out narrower than 150. Found by the constraints property test on its
   seventh generated case.

Two smaller ones: focus traversal started from the framework's cached focus
rather than the live native focus, so the first Tab after a programmatic
`SetFocus` did nothing; and two executor tests asserted after a fixed 20 ms
sleep, which failed as soon as a stress test ran alongside them.

### Where the bar sits now

```text
cargo fmt --all -- --check                                        clean
cargo clippy --workspace --all-targets --all-features -D warnings clean
cargo test --workspace                                            168 passing
cargo doc --workspace --no-deps                                   clean
```

Verified on real Windows (Windows 10, `rustc` 1.98.0, `x86_64-pc-windows-msvc`),
not cross-compiled and not under Wine. `rustnative-windows`'s `native` module
— the `#[cfg(windows)]`-gated part that a Linux pipeline silently excludes —
is compiled, linted, and *executed* by that run, including 19 tests that
create real top-level windows and drive them through the production message
loop.

### Known remaining work

Genuinely open, and deliberately so:

- **A full UI Automation provider.** The accessibility bridge annotates
  standard controls; custom, non-`HWND`-backed semantic nodes will need a
  real provider. The audit explicitly scopes this as future work.
- **The `windows-cross-wine` CI job is commented out.** It did not work
  reliably on GitHub Actions. The real `windows-latest` job covers what
  matters; `tools/windows-cross-test.sh` still works locally.
- **Drag-and-drop, system share, and system-appearance notifications**
  remain portable contracts only, and are not advertised as supported
  capabilities, so no application can depend on them by accident.
- **Native menus are static at window creation**, matching the maturity of a
  window's title and size.

---

The sections below are the historical record of earlier passes, kept as
written at the time. Where they claim completeness, read the table above
first.

## Completeness pass (milestones 1–24)

A full audit of milestones 1–24 against a "production framework" bar found that most of the architecture was already solid (real native HWNDs, full tree diffing, working Tab/Shift+Tab traversal, structured async task scopes with cancellation), but several milestones had gaps between what was documented as "implemented" and what actually took effect end-to-end. All of the following were closed in this pass:

- **Milestone 21 (theme + styling) had no realization at all.** `Theme`/`VisualStyle` resolution existed and was unit-tested in isolation, but nothing ever called it outside of tests: `TreeSnapshot` only ever carried a node's raw style *override*, and the Windows backend never read font or color data — no `WM_SETFONT`, no `WM_CTLCOLOR*`, no background paint. Fixed: `TreeSnapshot::from_node_with_theme` resolves every node's style (theme default merged with override, in its `Normal`/`Disabled` state) before it reaches the backend, mirroring how layout geometry is already fully resolved in the core; the Windows backend now realizes it as native fonts (`CreateFontIndirectW` + `WM_SETFONT`) and colors (`WM_CTLCOLORSTATIC`/`WM_CTLCOLOREDIT`/`WM_CTLCOLORBTN` for controls, `WM_ERASEBKGND` for containers), with GDI resources owned and freed per node.
- **No `disabled` concept existed anywhere**, so the theme's `Disabled` state variant was unreachable. Fixed: `Node::disabled(bool)`/`is_disabled()`, threaded through `TreeNode`, realized as `EnableWindow` on Windows, and excluded from Tab-order focus traversal.
- **Live interaction style variants were declared but not realized.** Fixed: the Windows renderer retains each node's unresolved style override and applies hover, pressed, and focus variants as transient native repaint state via `TrackMouseEvent`/`WM_MOUSELEAVE`, mouse-button messages, and focus synchronization; these changes never force a component rerender.
- **Milestone 23 ("native dialogs, menus, and system integration") was only portable contracts.** The Windows backend had no file-dialog implementation, `notify()` was a hardcoded stub error, and there was no menu system — not even a data model. Fixed: `WindowsFileDialogs` (open/save/folder-pick — originally via `GetOpenFileNameW`/`GetSaveFileNameW`/`SHBrowseForFolderW`, since modernized to `IFileOpenDialog`/`IFileSaveDialog`; see the standards-audit pass's P1.15 below), real toast-style notifications via `Shell_NotifyIconW`, and a full `MenuBar`/`MenuItem` model in the core realized as native `HMENU`s with `WM_COMMAND` routing to a new `Event::MenuAction`.
- **Milestone 24 ("multi-window support") only let you open windows before the platform's event loop started.** There was no way for a running component to open or close a window in response to an event (e.g., a menu action or button click), since `Application::open_window`/`close_window` aren't reachable from `ComponentContext`. Fixed: a deferred `WindowCommand` queue (`ComponentContext::windows()` → `WindowRequests::open`/`close`) applied by `Application` after each dispatch/task-pump, and a Windows-side `WindowRegistry` that keeps native HWNDs in sync with `Application::window_ids()` as they change at runtime — including a specific, documented design to avoid a use-after-free when a window is asked to close itself mid-dispatch (its teardown is deferred via a posted `WM_CLOSE`, never synchronous).

`Capability::Menus` was added, and `WindowsPlatform::capabilities()` now advertises `FileDialogs`, `Notifications`, and `Menus` alongside the previously-advertised capabilities. `examples/hello-label` now exercises all of the above (a disabled button past count 5, a native menu bar with a checkable-style toggle and a runtime-opened settings window, both routed through `Event::MenuAction`).

**Known, deliberately out-of-scope remainder**, consistent with the framework's existing platform-independence philosophy: drag-and-drop, system share, and system-appearance-change notifications remain portable contracts/flags only (not advertised as supported capabilities, so no app can be misled into depending on them); native menus are static at window creation (matching the existing maturity level of a `Window`'s title and size, which also are not reactively updatable after the window opens).

## Standards audit remediation pass

`Audit.md` (a from-scratch senior-engineer review against a "would this pass
review at a top-tier systems team" bar) found 3 P0s, 20 P1s, and 15 P2s.
Every P0, every P1, and every P2 in that document is now closed, including
every item in its Phase 1–3 roadmap — the two items (P0.2, P1.15's owner-
window support, and P2.23's `missing_docs` enforcement) that earlier status
updates once recorded as deferred were all revisited and closed for real,
the first two once this pass developed a way to actually compile and run
`rustnative-windows`'s Windows-only code in this environment (see P1.17
below), and the third as a self-contained mechanical documentation pass
verified by the crate's own `deny(missing_docs)` lint. A prior pass had
already closed a meaningful subset before this one started (engineering
controls — `deny.toml`, `clippy.toml`,
`rustfmt.toml`, `SECURITY.md`/`CONTRIBUTING.md`, an accurate
`rust-toolchain.toml` — plus task-scope bounded retention, effect-dependency
exact equality instead of hashing, stale-event rejection, indexed
`TreeSnapshot` children/depth lookups, selective layout invalidation,
checked identity allocators, RAII menu resources with checked command-id
allocation, and a dedicated STA thread for native dialogs instead of the
shared blocking pool — but, notably, *not* an actual CI workflow: this
repository had none until this pass, see P1.17 below). This pass picked up
from there and closed every remaining P0/P1/P2 finding and every roadmap
item in `Audit.md`, once the compile/test breakthrough below made
`rustnative-windows`'s Windows-only surface reachable by the same tools
(`cargo build`/`test`/`clippy`/`fmt`) as `rustnative-core` already was.

**P0.1 — node identity could theoretically collide (`rustnative-core`,
fully fixed).** `NodeId::from_key` was an FNV-1a hash of the key string:
compact, but a many-to-one mapping, so *some* pair of distinct keys
colliding was mathematically inevitable even though any one collision was
astronomically unlikely. Replaced with a true interning table
(`crate::identity`) keyed by exact string equality: distinct keys are now
*structurally* guaranteed distinct ids, not just very-probably distinct.
Covered by both a unit test and a `proptest` property test generating
hundreds of randomized key sets per run.

**P1.7/P1.18 — the async scheduler was hard-wired to one process-global
Tokio runtime, and delays could not be tested deterministically (fully
fixed).** Introduced `crate::scheduler::Executor`, a pluggable backend
trait; `Scheduler::new()` still defaults to the same shared two-worker
runtime as before (`TokioExecutor::shared`, so no behavior changed for
existing callers), but `Scheduler::with_executor` now lets a host supply
its own (`TokioExecutor::dedicated(n)` for an independently-owned runtime,
or any other `Executor` impl). Time was initially left as a known, separate
gap (`SleepFuture` still read the shared runtime's timer regardless of
which executor a `Scheduler` used) — that gap is now closed too:
`Executor` gained a `sleep` method alongside `spawn`, `Scheduler::sleep`
delegates to it, and `ComponentContext::sleep`/`EffectContext::sleep`
route through their owning component's scope's scheduler instead of a
free-standing constructor. `ManualExecutor` is the new deterministic,
virtual-time backend this unlocks: nothing runs until `run_until_stalled`
is called, and delays only resolve once `advance(duration)` moves the
virtual clock (in deadline order, even across multiple concurrently
pending sleeps) — so a component/effect test that spawns a task or awaits
a delay no longer needs to race a real clock or a real thread pool to
assert on the outcome. Covered by six new unit tests in
`scheduler::executor::tests`.

**P1.10 — user-triggerable composition mistakes could panic (fully
fixed).** A component using the same node key twice in one render, or
calling `ComponentContext::effect` with a duplicate key, used to trip a
release-mode-silent `debug_assert!`/`assert!`. Both are now a structured
`RenderError` (`crate::component::RenderError`), surfaced through
`ComponentTree::render`'s `Result` and inspectable afterward via
`last_render_error()`, while the render pass still completes a
structurally consistent tree rather than aborting. Internal
impossible-states (a bookkeeping map missing an entry the runtime itself
just inserted) deliberately remain `debug_assert!`/`expect!` — the line
this pass draws between the two is documented directly on `RenderError`.

**P1.4 — the Windows tray-notification icon used an undocumented `hWnd =
NULL` identity, added and immediately deleted per call (fixed).**
`rustnative-windows` now creates one persistent, hidden, message-only host
window on first use (`NotificationHost`) and keeps it alive for the life
of the process; the icon is added once and updated in place via
`NIM_MODIFY` for every subsequent notification instead of being torn down
and recreated (which also fixes a visible taskbar-icon flicker the old
add/delete-per-call design had). Verified against `windows-sys`'s actual
`HWND_MESSAGE`/`NIM_MODIFY` constants via the crate's published docs
before writing the code, per the same discipline as the rest of this
crate's Win32 surface (see the local-verification limitation below).

**P0.2 — a single `GWLP_USERDATA` slot was cast to two different types
across three files with the invariant re-explained by hand at each call
site (fully fixed).** Top-level windows store a `*mut Runtime` there;
container/control windows store a cached `COLORREF` there — a disjoint use
of the same untyped Win32 slot, previously accessed through seven-plus
individual `GetWindowLongPtrW`/`SetWindowLongPtrW` calls spread across
`message_loop.rs`, `container.rs`, and `renderer.rs`. Centralized behind
two narrow typed accessors in a new `native::user_data` module —
`RuntimeSlot::{get, set}` and `BackgroundColorSlot::{get, set}` — so the
invariant is documented and can be audited or changed in exactly one place,
and every call site outside that module is now a plain function call
instead of a bespoke `unsafe` cast. This is now genuinely compiler-verified
rather than reviewed by hand only: it compiles cleanly as real
`x86_64-pc-windows-gnu` code (see `tools/windows-cross-test.sh` under
"Local verification" below), and its one cross-cutting behavioral
assumption (`GetWindowLongPtrW` on a null `HWND` is well-defined and
returns `0` rather than crashing, which `RuntimeSlot::get` relies on when
called against a possibly-null ancestor window) was additionally confirmed
against a real Win32 implementation via a MinGW/Wine ground-truth probe.

**P1.17 — no way to ever compile or run the native Win32 backend existed
(fixed).** `rustnative-windows`'s `native` module is `#[cfg(windows)]`-gated,
so every local development and review pass — including every one before
this repository had a CI workflow at all — ran on Linux, where that module
is simply excluded from the build. No commit in this project's history had
ever actually had its Win32 logic compiled, let alone tested or run, by
anything. Three fixes landed together: (1) `.github/workflows/ci.yml` adds
a `windows-latest` job so this finally happens automatically on real
Windows going forward (unverified until its first real CI run, since this
environment cannot run GitHub Actions itself); (2)
`tools/windows-cross-test.sh` (see "Local verification" below) makes it
possible *right now, in this environment*, via `RUSTC_BOOTSTRAP=1 -Z
build-std` plus Wine — no more waiting for a Windows machine or a CI run to
find out whether a change to `native/` compiles; (3) using exactly that
pipeline, `native/` went from zero unit tests of its own to sixteen —
`native::user_data`, `native::registry`, and `native::measure` are now
covered against real `HWND`s (message-only windows, needing no display
driver), including a real GDI-handle-leak proof via `GetGuiResources`
before/after counts, not just data-structure-level assertions. This pass
also used the pipeline to find and fix a previously-invisible problem at
real scale — see "122 real clippy findings" below — and to implement P1.15
with actual compiler feedback rather than blind review. What remains
genuinely open: this is Wine, not Windows (see "Local verification" for
exactly where that distinction matters and does not substitute for the
real Windows CI job), and message-loop-level integration tests (creating a
full top-level window and driving its message loop, as opposed to the
narrower `native::registry`/`native::user_data`/`native::measure` unit
tests this pass added) remain future work.

 Split into ~20 focused modules matching the audit's
own suggested map: `identity`, `event`, `node`, `component/{context,
effects,error,tree}`, `reconcile/{snapshot,diff}`,
`layout/{geometry,constraints,measure,engine}`, `style/theme`,
`scheduler/{mod,executor}`, `services/{mod,memory}`, `capability`, `menu`,
`window`, `application`, `platform`. The public API is re-exported flat
from the crate root exactly as before (`rustnative_core::NodeId`,
`rustnative_core::Component`, ...), so no downstream call site — including
every one in `rustnative-windows` and the example — changed.
`rustnative-windows` was deliberately **not** similarly re-split in this
pass; see "What this pass deliberately did not do" below.

**Encapsulation (P2.24/25).** `VisualStyle`, `Theme`, `WindowState`, and
`TreeDiff`'s operation list moved from public fields to accessor methods
(`Theme` gained builder methods — `with_button`, `with_foreground`, etc. —
so a custom theme can still be constructed without the struct-update
syntax the public fields used to allow). `TreeNode` and the plain
geometry/color value types (`Rect`, `Point`, `Size`, `Color`,
`Typography`, ...) deliberately kept public fields: they have no
invariant a getter would protect and are read broadly by any backend by
design — see `crate::reconcile::snapshot`'s and `crate::layout::geometry`'s
module docs for the reasoning.

**Other P2s closed:** removed the unused, never-adopted `ServiceFuture`
type alias (P2.26); the workspace lint policy now runs
`clippy::pedantic`/`clippy::cargo` in addition to the previous minimal
set, with each `#[allow]` exception commented at its use site (P2.40);
added a `proptest`-based property test suite covering identity,
reconciliation round-tripping, and layout non-negativity under randomized
input (P2.34); added a `criterion` benchmark suite covering snapshot
construction, diffing, layout, component dispatch, and task scheduling
(P2.35); every public struct/enum/trait/associated type in `rustnative-core`
now has a doc comment (P2.23 — see "Known, deliberately incomplete" below
for the part of this that's still open).

## 122 real clippy findings, found and fixed for real

`cargo clippy --workspace --all-targets -- -D warnings` had, since this
project's `deny.toml`/`clippy.toml` policy was first written, only ever
actually been evaluated against `rustnative-core` and the tiny
`#[cfg(windows)]`-free surface of `rustnative-windows` — every claim in this
document (and every prior one) that "clippy is clean" was true only of
that subset, because clippy, like `cargo check`, silently excludes
`#[cfg(windows)]`-gated code on a non-Windows host. The first time this
pass ran clippy through `tools/windows-cross-test.sh`'s real
cross-compilation path — against the actual `x86_64-pc-windows-gnu` target,
with the workspace's real `-D warnings` policy — it returned **122
errors**, none of them previously known, spread across every file in
`native/` and `services/`.

These were fixed for real, not suppressed: `SetWindowLongPtrW`/`GetClientRect`/
`DrawTextW`-style implicit-reference-to-pointer calls became explicit
`&raw const`/`&raw mut`; ad hoc `WM_SIZE`/`WM_MOVE`/`WM_COMMAND`/
`WM_MOUSEWHEEL` bit-twiddling (`(x >> 16) & 0xffff) as u16 as i16`-style
chains, several of them subtly duplicated across files) was replaced with
shared, tested `loword`/`hiword`/`loword_signed`/`hiword_signed` helpers in
`native::util`; every genuinely lossless numeric conversion (`u8`/`u16` →
`u32`/`i32`) switched from `as` to `u32::from`/`i32::from`; a handful of
`unsafe` blocks (an `OwnedMenu::drop`, a notification-host `unsafe impl
Send`, a `RegisterClassW` call) were missing the safety-comment this
workspace's `undocumented_unsafe_blocks = "deny"` policy requires, and now
have one; and every remaining narrowing/sign-changing cast that really is
safe by construction (masked-to-16-bits message fields; struct sizes that
can never approach `u32::MAX`; a `COLORREF`'s 24 significant bits fitting
in an `isize` regardless of pointer width) got a `#[allow]` with a comment
explaining *why*, at the exact site, rather than a blanket suppression —
matching this workspace's existing P2.40 policy for every other
intentional lint exception. `clippy::multiple_crate_versions` firing on a
second `syn` major version pulled in transitively by the `windows` crate
(used only by `services::dialogs`, see P1.15 below) is recorded as an
accepted, externally-imposed exception in `clippy.toml`'s
`allowed-duplicate-crates`, rather than worked around by depending on an
older/newer `windows` release chosen only to dodge the lint.

**What this pass deliberately did not do, and why:**

- **`rustnative-windows`'s module split (P1.21/P2.22).** Unlike
  `rustnative-core`, this crate's `native` module can only be
  compiler-verified via the cross-compilation path in
  `tools/windows-cross-test.sh` (see "Local verification" below), not via
  plain `cargo check` on this host — a real but no longer absolute
  limitation. The crate is organized into the same kind of focused module
  tree `rustnative-core` uses: `native::{app, container, input, measure,
  menu, message_loop, registry, renderer, runtime, user_data,
  window_handles, util, test_support}`, plus `error`, `ffi`, `platform`,
  and `services::{clipboard, dialogs, notifications, system}` at the crate
  root. This pass added `native::user_data` and `native::test_support` to
  that tree (see P0.2 and P1.17 above), and a later pass in the same
  overall effort added `native::window_handles` (see P1.15's owner-window
  section above), otherwise leaving the module boundaries as they already
  were, since a wholesale reorganization of unsafe FFI code is a separate,
  larger piece of work from what this pass's compile-verification
  breakthrough makes newly safe to attempt.

## P1.15 — modern `IFileDialog`, with real owner-window support (fixed)

An earlier note in this document deferred this, reasoning that
`windows-sys` (this crate's dependency everywhere else) only exposes raw
COM vtables, and hand-indexing into one to call `IFileOpenDialog`/
`IFileSaveDialog` risked a silent, undefined-behavior-on-real-Windows
mistake with no compiler able to catch it in this environment. Both halves
of that reasoning held up under a real attempt this pass, and pointed to
the same fix: `windows-sys` genuinely has no COM interface definitions at
all for `IFileOpenDialog` (confirmed by their total absence from its own
source, not just from what this crate happens to enable), so implementing
this against raw `windows-sys` really would have meant re-deriving the
interface's GUID and vtable layout from documentation by hand. Instead,
`services::dialogs` now depends on the `windows` crate — Microsoft's own
higher-level, maintained bindings, which provide the actual generated
`IFileOpenDialog`/`IFileSaveDialog`/`IShellItem` methods rather than raw
vtable slots — scoped to just this one module via a `cfg(windows)`-only,
minimal-feature dependency (see `Cargo.toml`'s comment on it), not added
workspace-wide. `show_open_or_save`'s two functions and the previous
`SHBrowseForFolderW`-based folder picker were replaced with three
functions built on `IFileOpenDialog`/`IFileSaveDialog`, unified where
possible: `IFileOpenDialog` with `FOS_PICKFOLDERS` is Microsoft's own
documented modern replacement for `SHBrowseForFolderW`, so folder picking
now goes through the same modern interface family as file open/save
instead of a third, older API. This compiles, links, and passes clippy's
full `-D warnings` policy against the real `x86_64-pc-windows-gnu` target
via `tools/windows-cross-test.sh` — the same real verification this
document describes for everything else in `native/`.

**`Audit.md`'s Phase 3 roadmap item 15 — real owner/parent-window support —
is now also closed.** `FileDialogRequest` (the cross-platform request type
in `rustnative-core`, shared by every future platform backend) gained an
`owner: Option<WindowId>` field. `rustnative-windows` resolves it to a real
native `HWND` through a new module, `native::window_handles`: a small,
thread-safe `WindowId -> HWND` table for top-level windows, kept in sync by
`WindowRegistry` at window creation and by `window_proc`'s `WM_DESTROY`
handling at window teardown. This exists specifically because
`services::dialogs` runs each dialog on its own dedicated STA thread (see
P1.15's `run_sta` discussion elsewhere in this document), not the
message-loop thread that actually owns the window — so resolving a window
identity to its native handle from the dialog thread means reaching across
threads for it, which every other native handle in this crate deliberately
avoids needing to do. `dialogs.rs`'s `show_open`/`show_save`/
`show_pick_folder` now call `IFileDialog::Show` with that resolved handle
(or `None`, unchanged, for a request with no owner or one naming a window
this process doesn't currently recognize — an owner is a presentation
enhancement, not a correctness requirement, so a stale or absent owner
falls back to an unowned dialog rather than an error). Covered by three new
unit tests in `native::window_handles::tests`, run and passing under the
same real Windows/Wine verification as everything else in `native/` (see
"Local verification" below) — `rustnative-windows`'s native test suite is
now 19 tests, up from 16.

## `missing_docs` (P2.23, now fully enforced)

Every public item in `rustnative-core` — including individual struct
fields, enum variants, trait methods, and associated functions, not just
the types that contain them — now carries a doc comment, and
`#![deny(missing_docs)]` (not `warn`) is set in `lib.rs` so this cannot
silently regress. An earlier pass documented every type but left the lint
disabled entirely, deferring roughly 380 field/variant/method-level
warnings as a bounded, mechanical follow-up rather than fixing them or
half-enabling a lint that would fail `-D warnings` CI on an unrelated
backlog; this pass wrote that documentation (not filler — each comment
describes what the specific field/variant/method actually does) and
flipped the lint to `deny`. `cargo build -p rustnative-core --all-targets`
now reports zero `missing_docs` warnings.



```text
Application
  └── Window registry
       └── Framework-managed ComponentTree per window
       ├── keyed component identity
       ├── typed props
       ├── child → parent callbacks/messages
       ├── lifecycle
       ├── structured TaskScope per component
       ├── dependency-aware effects + cleanup
       ├── injected Services + capabilities
       ├── deferred window-open/close requests
       ├── theme/style resolution (resolved before reaching the backend)
       └── declarative Node tree (+ menu bar, disabled state)
            ↓
         TreeDiff
            ↓
         LayoutEngine
            ↓
         Windows native realization
         (controls, fonts/colors, native menus, dynamic window registry)
```

## Completed milestones

1. Framework foundation + native Windows label
2. Stable node identity + native object ownership + UI tree
3. Application state + reconciliation
4. Components + events + structural tree diffing
5. Native containers + hierarchical layout
6. Deterministic layout phase + coordinate-space contract
7. General layout model (`Auto`, `Fixed`, `Fill`, alignment, padding, margin, gap)
8. Intrinsic measurement + `Row` + layout invalidation
9. Constraints + text wrapping
10. Overflow + clipping + scrolling with native viewport/content-host architecture
11. Focus + keyboard input + accessibility semantics
12. Text input + controlled component state
13. Component composition + lifecycle
14. Framework-managed component tree
15. Typed component props + parent-to-child data flow
16. Child-to-parent callbacks + shared message channels
17. Async tasks + framework scheduler
18. Structured task scopes
19. Effects + reactive invalidation
20. Resource and service system
21. Theme + styling system, resolved and realized as native fonts/colors
22. Platform capability abstraction
23. Native dialogs, menus, and system integration — realized (file dialogs, notifications, native menu bar), not just contracts
24. Window lifecycle + multi-window support, including opening/closing windows at runtime
25. Advanced input: pointers/touch/pen with capture, wheels, portable gestures, IME, clipboard events, OLE drag-and-drop, XInput controllers
26. Full accessibility bridge: portable model and projection, UI Automation providers, patterns, events, virtual elements

## Structured task-scope guarantees

- Every managed component receives a persistent task scope.
- Rerenders do not recreate the scope.
- Prop changes do not recreate the scope.
- `ComponentContext::spawn()` uses the component-owned scope.
- `ComponentContext::task_scope()` exposes the same ownership boundary explicitly.
- Scope destruction cancels outstanding tasks.
- Component removal cancels the scope before `unmounted()`.
- Worker threads never mutate component state directly.
- Task completions return through the framework scheduler/event loop.
- Completions targeting removed components are ignored.

## Latest user-reported Windows verification

The user reported successful compilation/testing through the structured-task-scope milestone after the final scope fix. Before that final fix, the task-scope suite exposed the render-time task-scope lookup bug; the corrected implementation passes the intended architecture by carrying the component-owned `TaskScope` directly through `ComponentContext` instead of requiring a live map lookup during rendering.

The authoritative commands on Windows remain:

```powershell
cargo fmt --all
cargo check --workspace
cargo test --workspace
cargo audit
cargo run -p hello-label
```

## Local verification

`rustnative-core` — every module from this pass and the audit-remediation
pass above — was compiled, linted, and tested locally with a genuine
current-stable toolchain (`rustc`/`cargo` 1.91.1, obtained via this
environment's package manager, well past the crate's declared MSRV of
1.85), not the older, edition-downgraded 1.75 compiler prior passes were
limited to. `cargo test -p rustnative-core` passes (65 test functions: 57
unit tests across every module — including new ones in
`scheduler::executor::tests` covering `ManualExecutor`'s deterministic
scheduling and virtual-time behavior — 4 integration tests in
`component_lifecycle.rs`, and 4 `proptest` properties in
`property_tests.rs` — each of those 4 runs hundreds of independently
generated cases per invocation, so effective input coverage is well beyond
the function count alone), `cargo bench -p rustnative-core` runs its full
`criterion` suite successfully, `cargo clippy --workspace --all-targets
--all-features -- -D warnings` is clean under the strengthened
`pedantic`/`cargo` lint policy, and `cargo fmt --all -- --check` is clean.
`examples/hello-label` was also fully type-checked against the same
compiler and compiles cleanly.

The Windows-specific code under `crates/rustnative-windows/src/native/` was,
this pass, **actually compiled, linked into real PE32+ binaries, and
executed** for the first time in this project's history — not merely
type-checked on a non-Windows subset. Earlier passes (and the first attempt
in this one) concluded this was impossible: this environment's rustc ships
a prebuilt standard library only for its own host target, `rustup target
add x86_64-pc-windows-gnu` needs network access to
`static.rust-lang.org` (outside this environment's allowlist), and `-Z
build-std` looked like the obvious workaround except that it is a
nightly-only flag a stable-channel compiler rejects outright. That last
conclusion was wrong, or at least incomplete: `RUSTC_BOOTSTRAP=1` makes a
*stable* rustc and cargo accept nightly-gated flags, including `-Z
build-std`, as if running on nightly. Combined with fetching the exact
matching `library/` source for the installed rustc's own commit hash
(verified to match exactly, not just the version number) from
`rust-lang/rust` on GitHub — sparse-checked-out to avoid downloading the
full repository, plus its `library/backtrace` git submodule fetched
separately at its pinned commit, since a plain checkout doesn't pull
submodules — this successfully builds `core`/`alloc`/`std` from source for
`x86_64-pc-windows-gnu`. The only remaining gap, two small "Rust runtime
startup object" files (`rsbegin.o`/`rsend.o`) that bootstrap normally
builds as a special case rather than through a normal `cargo build`, is
closed by compiling `library/rtstartup/{rsbegin,rsend}.rs` directly with
`rustc --emit=obj` and placing the result in the sysroot's target `lib/`
directory. The whole procedure — including a hard check that the fetched
library source's commit actually matches the installed compiler's, so this
can never silently build against mismatched source — is now a reusable,
documented script: **`tools/windows-cross-test.sh`**.

Running it:

- Compiles `rustnative-core` and all of `rustnative-windows` — every file
  under `native/` and `services/`, including this session's `user_data.rs`
  centralization and the `IFileOpenDialog`/`IFileSaveDialog` rewrite of
  `services::dialogs` (P1.15) — as real `x86_64-pc-windows-gnu` code,
  against the real `windows-sys`/`windows` crates. This is qualitatively
  different from a host-target `cargo check`: on the host target, the
  *bodies* of every `#[cfg(windows)]`-gated function are stripped before
  type-checking even runs; cross-compiled for real, every `unsafe` FFI
  call, struct field access, and pointer cast in that module went through
  full type-checking, borrow-checking, and code generation.
- Links successfully via `x86_64-w64-mingw32-gcc`/`-ld` (from
  `gcc-mingw-w64-x86-64`) into real PE32+ binaries — confirmed with `file`.
- Runs `rustnative-core`'s full 57-test unit suite under Wine, all passing,
  as an actual Windows binary rather than a Linux one.
- Runs `rustnative-windows`'s 19-test suite under Wine, all passing — see
  "Native test coverage" below for what these actually exercise.
- Runs `cargo clippy` against this same real target with the workspace's
  full `-D warnings` policy — see "122 real clippy findings" above.
- With `--run-example` and an Xvfb virtual display available, actually
  launches `examples/hello-label` as a live Win32 GUI application under
  Wine: `CreateWindowExW` succeeds for real (confirmed by the absence of
  Wine's "no driver could be loaded" diagnostic once a display is
  present, versus its presence when none is), the menu bar realizes, and a
  screenshot (`import -window root`) shows real native controls — a menu
  bar with "File"/"View", a native button, and native labels reflecting
  the component tree's actual rendered state — positioned by this crate's
  actual layout engine and painted by actual Win32 child windows.

This is Wine, not Windows, and the two are not identical — differences in
DPI handling, theming (`visual styles`/`UxTheme`), IME behavior, shell COM
interfaces (`IFileOpenDialog`'s Wine implementation has historically lagged
real Windows more than plain `user32`/`gdi32`, which is one reason P1.15's
dialog code, while fully compiled and clippy-clean, has not been run
end-to-end interactively under Wine the way `hello-label` has), and other
areas Wine deliberately or incidentally diverges from real Windows are
exactly the kind of thing this technique cannot catch. Treat a pass here as
"compiles, links, and behaves plausibly under Wine", and the real Windows
CI job (`.github/workflows/ci.yml`, `test-windows`) as the actual release
gate — this does not replace that, but it closes nearly all of the gap
between "reasoned about carefully with no compiler" and "confirmed on real
Windows" that every previous pass's notes described as unclosable in this
environment.

### Native test coverage (P1.17, closed this pass)

Before this pass, `rustnative-windows` had exactly one unit test
(`platform::tests::capabilities_only_advertise_realized_backend_features`),
and it did not touch the `native` module at all — every other module
under `native/` had zero automated coverage of its own, correctness
resting entirely on manual review. This pass used the cross-compilation
pipeline above to add sixteen tests total, all running against real
`HWND`s created via a shared `native::test_support::TestWindow` helper
(message-only windows — `HWND_MESSAGE` as parent — which need no display
driver, so these tests run identically with or without Xvfb, including
under Wine's "null" graphics driver):

- `native::user_data`: `RuntimeSlot`/`BackgroundColorSlot` round-trip
  through a real `GWLP_USERDATA` slot on a real window, a null-`HWND`
  safety check pinned against the real linked `GetWindowLongPtrW` (not
  just the separate `tools/win_probes/null_hwnd.c` C probe), and an
  explicit test documenting that the two typed accessors are views over
  the same underlying storage.
- `native::registry`: insert/get round-tripping, duplicate-`NodeId`
  rejection (and that a rejected insert doesn't clobber the existing
  entry), the `id_for_hwnd` reverse lookup for both plain objects and a
  container's two HWNDs, `remove` clearing both directions, and — the
  strongest of these — a real proof via `IsWindow` that
  `NativeObjectRegistry::drop` actually destroys every `HWND` it still
  owns, not just that it compiles to do so.
- `native::measure`: real `GetDC`/`DrawTextW`-based measurement
  (monotonicity under more text, respecting a max-width constraint), and
  a real GDI-handle-leak proof for `ControlStyle::resolve`/`Drop` via
  `GetGuiResources(GR_GDIOBJECTS)` before/after counts — the same
  accounting Windows' own Task Manager uses, not a heuristic.

**What remains open:** message-loop-level integration tests (creating a
full top-level window via `native::runtime`/`native::app` and driving
`WM_COMMAND`/`WM_SIZE`/etc. through it, as opposed to the narrower
unit-level tests above) are still future work — this pass closed the "zero
coverage, and no way to add any" gap, not "full coverage of every
message-dispatch path."

**Confirmed on real Windows, and one real finding from doing so.** The
person using this project ran `cargo test --workspace` on an actual
Windows machine after this pass — the first time anything in this
repository's history had that happen. 72 of 73 tests passed unmodified,
including all 57 `rustnative-core` tests and 15 of 16 `rustnative-windows`
tests, which is itself a strong, independent confirmation that the
Wine-based verification this pass relied on throughout was not fooling
itself. The one failure,
`control_style_drop_frees_every_gdi_handle_it_created` (`before=0,
after=4`), was a real bug — but in the *test*, not in
`ControlStyle::drop`, whose logic (unconditionally freeing both fields
when non-null) is correct by inspection. `GetGuiResources(GR_GDIOBJECTS)`
is a process-wide counter, and `cargo test` runs tests concurrently across
threads by default; without serializing every GDI-object-creating test
against each other, a brush or font created by a *different*,
concurrently-running test between this test's "before" and "after"
snapshots produces exactly the same symptom as a real leak. Fixed with a
`static GDI_ACCOUNTING_LOCK: Mutex<()>` in `measure.rs`'s test module,
held for each GDI-creating test's entire before/during/after window (not
just around the allocating calls) — scoped to that one file since it is
the only one whose tests create real (non-null) GDI objects. Notably, this
raciness did not reproduce under Wine even across several runs in this
pass, which is itself worth recording: it suggests Wine's `GetGuiResources`
accounting is not sensitive enough to this class of interference to be a
reliable stand-in for it, one more concrete instance of the "Wine is not
Windows" limitation this document already calls out elsewhere, now with a
specific example rather than only the general caveat.

### MinGW/Wine struct-layout ground-truth verification

Separately from full-crate cross-compilation above, this pass also used
`x86_64-w64-mingw32-gcc` to compile small, standalone C programs against
real `<windows.h>` headers, run under Wine, to get real executed ground
truth (not documentation lookup) for specific behavioral assumptions this
crate's `unsafe` FFI code depends on. Used to confirm
`GetWindowLongPtrW(NULL, GWLP_USERDATA)` returns `0` (and sets
`ERROR_INVALID_WINDOW_HANDLE`) rather than crashing — the assumption
`native::message_loop::run_message_loop` and `RuntimeSlot::get` both rely
on when resolving a possibly-null ancestor window. The probe sources are
kept at `tools/win_probes/*.c` for reuse on any future struct-layout or
documented-edge-case question.

## Post-delivery fix: unbounded window creation on real Windows

The user ran the completeness-pass build on a real Windows 10 machine.
`cargo check --workspace` and `cargo test --workspace` passed, but
`cargo run -p hello-label` spun up new windows continuously until the

process crashed — exactly the class of bug flagged as a risk above, since
this specific code path had no compiler and no runtime available to catch
it here.

**Root cause:** `WindowRegistry::create_window` called
`self.runtimes.insert(id, runtime)` as its *last* step, after
`CreateWindowExW` and `ShowWindow`. `ShowWindow` (and, in principle,
`CreateWindowExW` itself) can synchronously deliver `WM_SIZE` to the new
window's own `window_proc` before either call returns. `Runtime::dispatch`
calls `sync()` at the end of every dispatch — including this one — and
`sync()` create-loop checks `!self.runtimes.contains_key(&id)`. Since the
window hadn't been inserted yet, it looked "missing" from its own
perspective and `sync()` created a second native window for the same
`WindowId`, which showed itself, which delivered its own synchronous
`WM_SIZE`, which created a third — recursing until the stack overflowed.
This is precisely the kind of native-callback reentrancy bug that cannot be
caught by `cargo check`/`cargo test`, only by running the real event loop.

**Fix:** two layers, both in `crates/rustnative-windows/src/lib.rs`:

1. `create_window` now inserts the new `Runtime` into `self.runtimes`
   immediately after `CreateWindowExW` succeeds — before `SetMenu`, the
   waker, `render()`, or `ShowWindow` — so any message synchronously
   delivered during those later calls sees the window as already present.
2. A `creating: HashSet<WindowId>` reentrancy guard on `WindowRegistry`,
   checked by both `sync()`'s create-loop and `create_window` itself, as
   defense-in-depth in case a message is ever delivered synchronously even
   earlier than that (from inside `CreateWindowExW` before it returns, which
   is not believed to happen without `WS_VISIBLE` but — again — was
   unverifiable here).

Traced by hand through the actual nested reentrant call stack that opening
both the primary and the auxiliary example window produces (each window's
own `ShowWindow` can trigger the other window's creation from partway
inside the first window's own creation call), which now terminates
correctly: every window is created exactly once, and `self.creating` and
`self.runtimes` end each `sync()` pass consistent with
`Application::window_ids()`.

`cargo test -p rustnative-core` still passes 37/37 and `cargo check -p
hello-label` still compiles cleanly after this fix; the fix itself remains
subject to the same Windows-target compile-check limitation described below
— it is reasoned through carefully, but a second real-Windows run is what
actually confirms it.



**Advanced input system**

Every remaining platform target is planned to the same depth, and none is implemented yet. The Web roadmap covers compile-time client JavaScript generation with opt-in WebAssembly subtrees, semantic DOM/CSS realization, browser events and accessibility, Web APIs/capabilities, Workers, routing/history, server rendering with client attachment, serverless/edge deployment, service workers/PWA, and browser packaging/testing/deployment; the terminal backend covers the cell grid, Unicode-width measurement, key and mouse protocols, and terminal restoration; macOS and iOS are specified in full and wait on hardware this project does not have yet.

Milestones 20–24 add service injection/mocks, theme tokens and style resolution,
capability discovery with a native escape hatch, portable system-integration
contracts, and independently owned multi-window component roots.

## Long-range roadmap

The complete roadmap is in `PLAN.md`. The remaining major stages are:

```text
Advanced input
        ↓
Full accessibility bridge
        ↓
Animations/transitions
        ↓
Virtualized lists
        ↓
Graphics/custom rendering escape hatch
        ↓
Persistence/navigation
        ↓
CLI/project tooling
        ↓
Packaging/deployment
        ↓
Shared core work for the remaining hosts
(no_std-capable subset, single-threaded executor seam, host clock)
        ↓
macOS / Linux / Android / iOS / Embedded / Terminal backends
        ↓
Web (Rust server + HTML/CSS/generated JS, opt-in WASM subtrees; client-side, server-rendered, serverless)
```

**Advanced input system**

Every remaining platform target is planned to the same depth, and none is implemented yet. The Web roadmap covers compile-time client JavaScript generation with opt-in WebAssembly subtrees, semantic DOM/CSS realization, browser events and accessibility, Web APIs/capabilities, Workers, routing/history, server rendering with client attachment, serverless/edge deployment, service workers/PWA, and browser packaging/testing/deployment; the terminal backend covers the cell grid, Unicode-width measurement, key and mouse protocols, and terminal restoration; macOS and iOS are specified in full and wait on hardware this project does not have yet.

Milestones 20–24 add service injection/mocks, theme tokens and style resolution,
capability discovery with a native escape hatch, portable system-integration
contracts, and independently owned multi-window component roots.

## Long-range roadmap

The complete roadmap is in `PLAN.md`. The remaining major stages are:

```text
Advanced input
        ↓
Full accessibility bridge
        ↓
Animations/transitions
        ↓
Virtualized lists
        ↓
Graphics/custom rendering escape hatch
        ↓
Persistence/navigation
        ↓
CLI/project tooling
        ↓
Packaging/deployment
        ↓
Shared core work for the remaining hosts
(no_std-capable subset, single-threaded executor seam, host clock)
        ↓
macOS / Linux / Android / iOS / Embedded / Terminal backends
        ↓
Web (Rust server + HTML/CSS/generated JS, opt-in WASM subtrees; client-side, server-rendered, serverless)
```
