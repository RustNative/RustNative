# Concepts III — the engineering loop, build and packaging, isolation and security, ecosystem (L7–L9)

Continues [`concepts-core.md`](concepts-core.md) (format, inclusion rule,
scoring) and [`concepts-app.md`](concepts-app.md). Requirement identifiers are
`Cnn-k`.

These concepts matter disproportionately for this project: `foundations.md`
F7 established that the engineering loop is the one layer where our substrate
starts behind, and these are the mechanisms other frameworks used to get ahead
there.

---

# Part A — The engineering loop

## C55 — Live previews and component catalogues

**Introduced by.** M1, D1 (preview canvases), W14 (isolated component
catalogues), D3 and D5 (designer previews).

**Mechanism.** A component is rendered *in isolation*, outside the running
application, from a declared preview: a function that constructs it with
sample state. Previews can be multiplied across configurations — light and
dark, every locale, every text scale, several size classes, right-to-left,
high contrast — and viewed side by side, updating as the code changes. A
catalogue collects every component's previews into a browsable, documented
library that designers and engineers share, and doubles as the input for
visual regression tests.

**Strengths.** Fastest possible feedback for UI work, with no navigation to the
screen being built; edge cases (long strings, empty states, errors) made
visible; a living design system; accessibility and localization regressions
caught while writing, not after release.

**Weaknesses.** Previews rot unless they are also tests; preview-only code paths
diverge from real ones; heavy preview infrastructure slows builds.

**Opportunities.** Previews are a direct consumer of pieces this plan already
requires: the headless backend (`X-L7-5`) renders a component without a host,
the environment (`C15`) supplies configuration, pseudo-localization
(`X-L5-4`) supplies stress text, and golden tests (`X-L7-8`) turn every preview
into a regression test so previews cannot rot. Both authoring syntaxes (2.9)
need the same preview support.

**Threats.** UI developers from every major archetype now consider isolated
previews basic; a compiled framework without them is judged against the slow
rebuild loop alone.

**Position.** **Absent.**

**Requirements.**

- `C55-1` `[X]` A preview declaration usable from both syntaxes, rendering a
  component in isolation across a declared configuration matrix (theme, locale
  including pseudo-locales, text scale, size class, direction, contrast).
- `C55-2` `[X]` A catalogue browser shipped with the CLI, rendering previews
  on the real backend of the development machine and on the headless backend.
- `C55-3` `[X]` Every preview is also a golden test, so a preview that stops
  rendering or changes unexpectedly fails CI.

## C56 — Visual designers and round-trip authoring

**Introduced by.** D1, D3, D5, E8's modern members, and D2's historical RAD
tools.

**Mechanism.** A visual editor manipulates the UI description directly —
placing, configuring, and binding elements — and writes the same source the
developer edits by hand, so either can be used at any time without a one-way
export. The best implementations use the language server protocol so the
designer, the text editor, and the compiler agree on one model.

**Strengths.** Designers contribute directly; layout discovery is faster;
onboarding is easier.

**Weaknesses.** Round-tripping is the hard part: generated code that becomes
the source of truth and merges badly (F3.5's sixth variant) is the common
failure.

**Opportunities.** Markup lowers to builder calls through one parser and one
set of diagnostics (Milestone 53), which is exactly the single model a
round-tripping designer needs. A designer is therefore *a client of the markup
tooling*, not a new representation.

**Threats.** Medium: designer tooling is a real differentiator in embedded
(E8) and enterprise (D3) markets.

**Position.** **Absent.**

**Requirements.**

- `C56-1` `[X]` The markup language server (Milestone 53) exposes a structural
  editing API — insert, move, set attribute — that preserves formatting and
  comments, so a visual designer can be built on it without its own model.
- `C56-2` `[X]` A stated non-goal: no designer that writes a separate file
  format or generated code as the source of truth.

## C57 — Scaffolding generators, templates, and codemods

**Introduced by.** W7, W9, W14's framework CLIs, and several W-archetypes'
automated upgrade tools.

**Mechanism.** The CLI generates the parts of an application that follow
conventions — a resource with model, migration, handlers, forms, and tests; a
component with its preview and test; a new screen wired into navigation. Starter
templates and first-party feature kits (authentication, billing, admin) create
working applications, not empty ones. For upgrades, *codemods* — programmatic
source transformations — rewrite application code across breaking changes, and
the upgrade command runs them.

**Strengths.** Consistency across a codebase; fast starts; upgrades that do not
stall on manual rewriting; convention teaching by example.

**Weaknesses.** Generated code that nobody understands; templates that go stale.

**Opportunities.** `rustnative new` exists (Milestone 31). Generators for
components, screens, services, and server resources extend it; codemods are
the mechanism behind the automated migration promised in `W-MF-8`, and Rust's
syntax-tree tooling makes them reliable.

**Threats.** Upgrade pain is the loudest complaint about the incumbent web
archetype; a stability promise without codemods is a promise kept by users'
labour.

**Position.** **Partial** — project creation exists; generators and codemods do
not.

**Requirements.**

- `C57-1` `[X]` `rustnative generate` for components (with preview and test),
  screens (wired into navigation), services, and server resources, in both
  syntaxes.
- `C57-2` `[X]` Codemods shipped with every breaking release and run by
  `rustnative upgrade`, tested against a corpus of example applications.
- `C57-3` `[X]` Feature kits — authentication, commerce, administration — as
  templates that produce working, tested code.

## C58 — Development services, continuous testing, and error overlays

**Introduced by.** W9's newer members, W3, and W14's build tools.

**Mechanism.** In development mode the framework *provisions dependencies
automatically* — starting a database, message broker, or identity provider in
a container when the application needs one and none is configured. Tests run
continuously in the background and report on each save, only for affected
code. Build and runtime errors appear as an overlay in the running application,
pointing at the source, instead of only in a terminal.

**Strengths.** A new developer runs the application with zero setup; test
feedback without context switching; errors seen where the developer is looking.

**Weaknesses.** Container dependencies on developer machines; background test
runs consume resources.

**Opportunities.** Resource bindings (`C48`) state exactly which services an
application needs, so development services can be derived rather than
configured. The in-application overlay path (`D-IM-1`) is the natural carrier
for an error overlay on every backend, including devices.

**Threats.** Setup friction ends evaluations in the first hour (`M-BR-4`).

**Position.** **Absent.**

**Requirements.**

- `C58-1` `[X]` Development services derived from declared bindings, provisioned
  locally when absent, and torn down with the development session.
- `C58-2` `[X]` A continuous test mode running affected tests on save.
- `C58-3` `[X]` Build and runtime errors surfaced as an in-application overlay
  on every backend, linking to source positions (including `.rsx` positions).

## C59 — Development builds

**Introduced by.** The bridged mobile archetype (M3) and its managed toolchain
layer (M8).

**Mechanism.** A *development build* is the application's native shell,
compiled once with all of its native modules, plus a development client that
loads application code, tools, and state from the development machine. Native
code changes require a new development build; everything else updates live.

**Strengths.** Separates the slow part (native compilation, signing,
installation) from the fast loop; one development build is shared by a team.

**Weaknesses.** Two kinds of change to understand; development builds go stale
when native dependencies change.

**Opportunities.** For a compiled framework the split is different — all
application code is native — but the idea transfers to *what* is rebuilt: a
development build containing the backend, the runtime, and a loader for the
application as a dynamic library (`X-L7-2`) makes the on-device loop
(`X-L7-1`) re-send one library instead of reinstalling the application.

**Threats.** On-device iteration speed is where mobile developers compare
frameworks most directly.

**Position.** **Absent.**

**Requirements.**

- `C59-1` `[M]` `[E]` A development build per device target that loads the
  application crate as a separately rebuilt unit, so an application change is
  delivered to the device without reinstalling or re-signing.

## C60 — Semantics-based testing

**Introduced by.** M1 (tests that query a semantics tree), W14's testing
libraries (query by role and accessible name), and platform UI-automation
frameworks.

**Mechanism.** Tests find elements the way users and assistive technologies
do — by role, accessible name, label, and state — rather than by
implementation details such as identifiers or tree position. The same
semantics tree that powers accessibility powers the test API. A test that
cannot find a button by its accessible name has found an accessibility bug.

**Strengths.** Tests survive refactoring; accessibility is tested implicitly by
every UI test; tests read like user stories.

**Weaknesses.** Elements without good semantics are hard to target — which is
the point, but it slows adoption in codebases with poor accessibility.

**Opportunities.** The portable accessibility tree (Milestone 26) is exactly
the semantics tree required, and it exists on every backend by design; making
it the test query surface costs little and turns the whole test suite into an
accessibility check.

**Threats.** Without it, tests couple to implementation and break on every
refactor, and teams stop writing them.

**Position.** **Absent** as a test API.

**Requirements.**

- `C60-1` `[X]` A test query API over the portable accessibility tree — by
  role, accessible name, label, state — used by component, interaction, and
  end-to-end tests on every backend including the headless one.
- `C60-2` `[X]` A test failure message that, when an element cannot be found by
  semantics, lists what the semantics tree does contain.

## C61 — Record, replay, and time-travel debugging

**Introduced by.** W14's state containers (time travel), E11 (recording and
replaying message streams), and reverse debuggers.

**Mechanism.** Every input event and message is recorded with timing. A
recording can be replayed deterministically to reproduce a bug exactly, stepped
backwards and forwards through state history, attached to a bug report, and
turned into a regression test.

**Strengths.** Reproducing the unreproducible; bug reports that contain the bug;
regression tests from real sessions.

**Weaknesses.** Determinism is required — any unrecorded source of
non-determinism breaks replay; recordings can contain sensitive data.

**Opportunities.** Determinism is unusually attainable here: messages are the
only way state changes, the clock and executor are replaceable (`X-L1-5`,
`X-L1-3`), and services are overridable (`C11-2`). A message log plus recorded
service responses is a complete, replayable recording — something frameworks
with ambient mutation cannot offer.

**Threats.** Low as a gap; high as a differentiator missed.

**Position.** **Absent.**

**Requirements.**

- `C61-1` `[X]` Recording of input, messages, and service responses, with
  redaction rules, and deterministic replay on the headless backend and the
  originating backend.
- `C61-2` `[X]` Step-backwards through state history in the inspector.
- `C61-3` `[X]` Conversion of a recording into a regression test.

## C62 — Profile-guided startup optimization and startup tracing

**Introduced by.** M1 (shipped startup profiles), D3 and W9 (ahead-of-time and
profile-guided compilation), W3 (startup tracing).

**Mechanism.** The critical startup path is recorded as a profile, shipped with
the application, and used to pre-compile, order, or preload exactly the code
and data startup needs. Startup is traced phase by phase — process start,
runtime initialization, first frame, first meaningful content, interactive —
so regressions are attributable.

**Strengths.** Measurable startup improvements without code changes; startup
regressions caught and explained.

**Weaknesses.** Profiles go stale; tooling complexity.

**Opportunities.** Profile-guided optimization is available in the Rust
toolchain; the framework's contribution is the phase model and the trace, and
wiring profile generation into release builds.

**Threats.** Startup time is one of the few numbers users perceive directly.

**Position.** **Absent.**

**Requirements.**

- `C62-1` `[X]` A startup phase model with tracing on every backend — process
  start, runtime ready, first frame, first content, interactive — with each
  phase budgeted in Milestone 42.
- `C62-2` `[X]` Optional profile-guided release builds driven by `rustnative`,
  with the profile generated from a scripted startup run.

---

# Part B — Build and packaging

## C63 — Continuous native generation

**Introduced by.** M8 (the managed toolchain layer around M3).

**Mechanism.** The platform-native project folders — manifests, entitlements,
build scripts, resources — are *generated* from declarative project
configuration every time, and never edited by hand. Libraries that need native
changes contribute *configuration plugins* that modify the generated projects
programmatically. Upgrading the framework regenerates the native projects
rather than asking the developer to merge template changes.

**Strengths.** Native project drift and upgrade merge conflicts disappear;
libraries can declare their native requirements; one source of truth for
application metadata.

**Weaknesses.** Everything must be expressible through configuration or a
plugin; escape to hand-edited native projects is a one-way door.

**Opportunities.** `rustnative.toml` already generates the Windows manifest and
packaging (Milestone 32), and every later backend plans to generate
`Info.plist`, manifests, and entitlements from it. Stating the rule — native
project files are build outputs — and adding a typed plugin hook for
capability packages (`X-ECO-1`) to contribute native configuration makes this a
guarantee rather than a habit.

**Threats.** Without it, every capability package ships instructions to "add
this to your manifest", and upgrades become manual merges.

**Position.** **Partial** — generation from `rustnative.toml` on Windows; not
stated as a rule; no plugin hook.

**Requirements.**

- `C63-1` `[X]` Native project files and manifests are generated build outputs
  on every backend, never hand-edited, regenerated on upgrade.
- `C63-2` `[X]` A typed configuration-plugin hook through which capability
  packages declare native requirements (permissions, entitlements, manifest
  entries, native dependencies).

## C64 — Remote builds, build caching, and task graphs

**Introduced by.** M8's cloud build services, E7's shared-state cache, and
monorepo task runners in W14.

**Mechanism.** Builds for hosts the developer's machine cannot build for (or
cannot sign for) run in the cloud; build outputs are cached by content hash and
shared between developers and CI, so nobody rebuilds what someone else already
built; a task graph runs only what changed across a multi-package repository.

**Strengths.** Faster CI and local builds; builds for platforms without local
hardware; reproducibility through content addressing.

**Weaknesses.** Cache correctness depends on complete input declaration;
infrastructure cost.

**Opportunities.** Build time is our substrate's tax (`X-L0-3`). Shared caching
of compiled dependencies is available in the Rust ecosystem and should be the
documented default for teams; remote builds address the hardware constraint in
2.13 for *building* (though not for verifying).

**Threats.** Clean-build times are a first impression.

**Position.** **Absent.**

**Requirements.**

- `C64-1` `[X]` A documented, supported shared build cache for local and CI
  builds, with its effect measured in the build-time budget.
- `C64-2` `[X]` Remote build and signing for targets that cannot be built
  locally, driven by `rustnative`, with the 2.13 rule preserved: a remote build
  is not a verification.

## C65 — Hierarchical code sharing between platform groups

**Introduced by.** M4.

**Mechanism.** Shared code is organized as a hierarchy of source sets: common
to everything, then common to a *group* (all Apple hosts, all mobile hosts,
all desktop hosts, all hosts with a browser), then per platform. A declaration
in a higher set can require an implementation in each lower set (an
*expected/actual* pair), checked by the compiler.

**Strengths.** Real code reuse between related hosts without pretending they
are identical; the compiler proves every platform supplied its part.

**Weaknesses.** Build configuration complexity; group boundaries are sometimes
arbitrary.

**Opportunities.** Milestone 36 already intends to share Objective-C interop and
Core Text with Milestone 33 "wherever the two platforms genuinely agree", and
the terminal and embedded backends share the draw-list path. The Apple hosts
are also the plan's one nested group: iOS and iPadOS (Milestones 36 and 70)
share all of UIKit, macOS shares only the layer beneath it, so the Apple group
holds a UIKit group inside it. Making the groups
explicit crates — an Apple-shared crate, a draw-list-host crate — with trait
contracts that each member implements is the Rust expression of this concept,
and it is Tier 0 because it determines crate structure before the second
backend is written.

**Threats.** Without explicit grouping, shared code is copied between backends
and diverges.

**Position.** **Absent.**

**Requirements.**

- `C65-1` `[X]` Explicit shared crates for platform groups (Apple hosts,
  draw-list hosts, and any other group with genuine agreement), each defining
  trait contracts its members implement, decided before the second backend.

## C66 — Generated bindings from interface descriptions

**Introduced by.** The Linux toolkit lineage's introspection data (D-level
toolkits with annotated C interfaces), M4's language exports, and D5.

**Mechanism.** A library's interface is annotated — ownership transfer,
nullability, array lengths, lifetimes of callbacks — and a machine-readable
description is generated from it. Bindings for many languages are then
generated automatically from that description, correctly, instead of written
by hand for each.

**Strengths.** Every language gets complete, correct bindings; ownership rules
are data, not documentation.

**Weaknesses.** Annotation discipline; generated APIs can be unidiomatic.

**Opportunities.** Library-only mode (`M-MP-1`) requires generated typed
interfaces per host language; generating them from one annotated description of
the exposed Rust interface, with ownership explicit, is the correct mechanism —
and the same description feeds the machine-readable framework description
(`X-ECO-2`).

**Threats.** Hand-written bindings for each host language will lag and leak.

**Position.** **Absent.**

**Requirements.**

- `C66-1` `[X]` One interface description for the library-only surface, with
  ownership and threading annotated, from which bindings for each supported
  host language are generated and tested.

---

# Part C — Isolation and security

## C67 — Multi-process architecture

**Introduced by.** D7a, D7b, and the browser engines they embed; M1's extension
processes.

**Mechanism.** Privileged work (filesystem, network, native APIs) runs in one
process; untrusted or crash-prone content (rendering web content, running
plugins, decoding media) runs in separate, sandboxed processes with restricted
permissions, communicating over typed IPC. A crash in a content process is
recoverable without losing the application.

**Strengths.** Security through isolation; crash containment; per-process
resource limits.

**Weaknesses.** Memory overhead; IPC complexity and latency; startup cost.

**Opportunities.** Most RustNative applications do not need it — which is the
footprint argument against D7a — but applications that host untrusted content
(plugins, user scripts, documents from the network) do. Offering an *optional*
isolated worker process with a typed message boundary, reusing the message
model, gives the benefit where needed without imposing the cost everywhere.

**Threats.** Low as a default; high for plugin-hosting applications that would
otherwise run untrusted code in-process.

**Position.** **Absent.**

**Requirements.**

- `C67-1` `[D]` An optional isolated worker process with a typed message
  boundary, host sandbox restrictions applied per backend, and crash recovery
  reported to the supervising scope (`C17`).

## C68 — Capability as authorization

**Introduced by.** D7b, the browser permission model, and M1's
permission-gated services.

**Mechanism.** A *permission* layer that decides whether a particular part of
the application may use a capability that the host *has*: per-window and
per-plugin allowlists of commands, scopes on filesystem paths and network
origins, and an isolation pattern that intercepts every privileged call for
validation. This is distinct from, and complementary to, *availability*.

**Strengths.** Least privilege inside the application; a compromised or buggy
component cannot reach more than it was granted; auditable security posture.

**Weaknesses.** Configuration burden; permission errors confuse developers.

**Opportunities.** `PLAN.md` 2.5's capabilities answer *"does this host have
it?"*. Nothing answers *"may this part of the application use it?"*. Adding the
second question to the same model — a capability handle obtainable only through
a grant, with scopes — is cheap now and nearly impossible to retrofit once
applications depend on ambient access. It also underpins the plugin ecosystem
(`X-ECO-1`): a third-party package should receive only the capabilities it
declares.

**Threats.** The first security review of a plugin-capable application built on
the framework will ask exactly this question.

**Position.** **Absent.**

**Requirements.**

- `C68-1` `[X]` Capability grants distinct from capability availability:
  services obtainable only through a grant, grants scoped (paths, origins,
  windows, packages), and third-party packages receiving only the grants they
  declare — shape decided in Milestone 39, enforcement in Milestone 51.

## C69 — Web platform security primitives

**Introduced by.** W3, W7, and the web platform itself.

**Mechanism.** A strict content security policy with per-response nonces;
typed restrictions on dangerous sinks so only sanitized values reach them;
subresource integrity for anything loaded from elsewhere; cross-origin
isolation headers that enable powerful features safely; and permissions
policies restricting which browser features a page may use.

**Strengths.** Whole classes of injection removed; defence in depth enforced by
the browser.

**Weaknesses.** Strict policies break third-party scripts; nonce plumbing
through rendering.

**Opportunities.** A framework that generates every script tag and every sink
write can apply these by default — nonces, integrity hashes, and typed sink
values — which is far easier than retrofitting them onto hand-written markup.

**Threats.** Security headers are among the first things automated scanners
flag.

**Position.** **Absent** (partly covered by `W-SF-5`).

**Requirements.**

- `C69-1` `[W]` Strict content security policy with per-response nonces,
  subresource integrity, typed restrictions on dangerous sinks, cross-origin
  isolation, and permissions policy, all on by default.

## C70 — Vendor-neutral observability

**Introduced by.** W9, W16's runtime lineage (built-in telemetry events and live
dashboards), and the industry's vendor-neutral tracing standard.

**Mechanism.** Libraries emit traces, metrics, and logs through a
vendor-neutral API with standard semantic conventions; the application chooses
an exporter. Frameworks instrument themselves — every request, query, job, and
render — so observability works before the application adds anything. Live
dashboards show the running system from inside.

**Strengths.** No lock-in to a monitoring vendor; consistent data across
services; zero-effort baseline instrumentation.

**Weaknesses.** Volume and cost; conventions change.

**Opportunities.** `X-OBS-1` requires tracing across the client/server boundary;
using the vendor-neutral standard and its conventions makes that interoperable
with whatever the organization already runs. Framework-emitted spans for
renders, tasks, and service calls come from instrumentation points the
inspection protocol (`X-L7-3`) already needs.

**Threats.** A proprietary telemetry format would be rejected by operations
teams.

**Position.** **Absent.**

**Requirements.**

- `C70-1` `[X]` Framework instrumentation — renders, tasks, service calls,
  requests, jobs — emitted through the vendor-neutral tracing and metrics
  standard with its semantic conventions, exporter chosen by the application,
  and a client-to-server trace context propagated by the HTTP service.

---

# Part D — Ecosystem

## C71 — Plugin encapsulation and registries

**Introduced by.** W8 (encapsulated plugin contexts), M3 and M5 (native-module
ecosystems), E9 and E2 (board and package registries).

**Mechanism.** Plugins register into an encapsulated context — decorations,
routes, services visible only to their own subtree unless explicitly
exported — so plugins cannot collide. Registries index packages with
compatibility metadata (framework version, supported hosts, required
capabilities), quality signals, and search.

**Strengths.** Safe composition of third-party code; discoverability; clear
compatibility information.

**Weaknesses.** Registry curation and trust are ongoing costs.

**Opportunities.** Rust's package ecosystem already provides distribution; what
a framework registry adds is *compatibility metadata*: which backends a
capability package implements, which capability grants it requires (`C68`),
and which framework versions it supports — generated from the package itself.

**Threats.** Without compatibility metadata, users discover at build time — or
on a device — that a package does not support their target.

**Position.** **Absent.**

**Requirements.**

- `C71-1` `[X]` Capability packages declare supported backends, required
  grants, and framework version range in machine-readable metadata checked by
  `rustnative`, with a compatibility index generated from it.
- `C71-2` `[X]` Package-contributed services and routes scoped to what the
  package declares, with no ambient global registration.

## C72 — Explicit non-goals from the concept survey

Some widely adopted concepts are rejected, and the reasons are recorded so they
are not re-litigated without new information.

- **Asynchronous-interception change detection** (`C03`) — rejected: checks far
  more than changed, makes performance unpredictable, and its originator spent
  years migrating away from it.
- **Designer-generated code as source of truth** (`C56`) — rejected: F3.5's
  ceiling; round-trip through markup instead.
- **A proprietary flow-based visual programming environment** (`C86`) —
  rejected as a framework feature: out of scope for an application framework;
  integration with existing tools through service contracts instead.
- **A bundled browser engine for UI** (D7a) — rejected by 2.2; host content
  controls (`C28`) cover embedded web content where an application needs it.
- **Runtime reflection-based dependency injection** (`C16`) — rejected:
  compile-time wiring through services and the environment gives the same
  outcome without runtime failure modes.
- **A visual designer that writes its own format or generated code as the
  source of truth** (`C56-2`) — rejected: a designer, where one exists,
  edits the same `.rsx` markup the compiler reads, never a separate
  document or a code-behind the developer must not touch.
- **Embedding a Chromium runtime to host web content on Windows**
  (Milestone 48) — declined: host web content through the system's own
  component where the platform offers one without a redistributable, and
  answer `Capability::WebContent` honestly as unavailable where it does not.
- **Faking an unavailable host service** (Milestones 51, 57) — rejected: a
  push, billing, or widget service the host withholds from an unpackaged
  application answers `Unavailable` with the reason, never a stub that
  appears to work. `FakeStore` exists for tests and is named as such.
- **A framework-maintained package registry service** (Milestone 52) —
  declined: packages are ordinary crates; an index is a JSON file an
  organization or the project publishes, and `rustnative add` reads the
  crate's own metadata.

---

## Part summary

The engineering-loop concepts here — previews (`C55`), development services and
overlays (`C58`), development builds (`C59`), semantics-based testing
(`C60`), and record/replay (`C61`) — are the concrete content that Milestones
43–45 were missing. Three of them exploit root choices directly: previews ride
on the headless backend, semantics tests ride on the portable accessibility
tree, and deterministic replay rides on message-only state changes and
replaceable clock and executor. Those three are places where a compiled,
message-driven framework can end up *ahead* in the loop rather than merely
catching up.

Two concepts are Tier 0 because they fix structure before the second backend:
hierarchical platform-group crates (`C65`) and the shape of capability grants
(`C68`).
