# Web archetypes

Read [`foundations.md`](foundations.md) first. This document analyses concrete
web archetypes as *paths through* the root choices analysed there, climbing
from substrate to ecosystem in the order of [`method-and-stack.md`](method-and-stack.md).

RustNative's Web track is `PLAN.md` Web milestones A–K, with three deployment
modes from one tree. This is what those milestones are measured against, and
what they are missing.

Tags: `[W]` web-specific, `[X]` cross-cutting.

---

## W1 — The client-side component library with tree diffing

The archetype every other web archetype is a reaction to. Analysed first for
that reason.

**Root (L0–L2).** Dynamic interpreted substrate (F0.1), single-threaded host
loop the framework does not own, realization into the host's document tree
(F2.4). Every consequence below follows from those three.

**Semantics and model (L3–L4).** Full re-render with keyed tree diffing (F3.1);
layout and text delegated wholly to the engine (F3.2, F3.3), which is why this
archetype never had to solve them; local component state with explicit setters
(F4.1); effects with hand-declared dependency lists (F4.2). Error containment
exists as subtree boundaries — this archetype invented the mainstream version
of it.

**Integration (L5–L6).** Accessibility, input methods, bidirectional text, and
locale formatting all come from the engine, so the archetype's own L5 surface
is thin by luck rather than by design. L6 is explicitly *not* its problem:
routing, data, forms, and persistence are third-party (see W9).

**Loop and ship (L7–L8).** The best loop in the industry — sub-second
state-preserving replacement, mature inspectors showing tree, props, state, and
render causes, and profilers that attribute cost per component. Shipping is
someone else's tool.

**Project (L9).** The largest component ecosystem in software, the largest
talent pool, and incremental adoption into existing pages as its original
growth mechanism.

**Strengths.** Composition proved at every scale; renderer pluggability that
validates the layered architecture RustNative also uses; ecosystem and hiring;
mountable inside anything.

**Weaknesses.** Update cost proportional to tree size, so memoization and
referential-stability discipline become daily work; dependency-list effects are
a permanent bug class; it is one third of a framework and the rest is an
assembly (the seam-count asymmetry in its purest form); a runtime ships before
any application code runs.

**Opportunities.** Invalidation by ownership rather than by diffing the world —
we already have stable identity, transient-state rules, and scope-bound
effects, and none of it is currently *provable*. Effect correctness by
construction is a guarantee they can only recommend. Batteries in one versioned
whole rather than eight.

**Threats.** The ecosystem gap is the single largest threat in this document,
and familiarity is a genuine technical advantage: a model developers already
know ships working code faster even when it is worse.

**Concepts introduced here.** `C01` interruptible rendering and priority lanes;
`C02` visibility- and lifecycle-aware work; `C03` change-detection strategy;
`C15` environment down, preferences up. Each is analysed on its own merits,
independently of this archetype, in [`concepts-core.md`](concepts-core.md).

**What we must ship.**

- `W-CL-1` `[X]` Render-cause tracing and a documented invalidation contract
  with over-invalidation as a test failure (= `X-L3-1`, `X-L3-2`).
- `W-CL-2` `[X]` Mountable inside a foreign host tree — a RustNative subtree
  inside an existing document or native view hierarchy, so adoption is
  incremental (= `X-INTEROP-1`).
- `W-CL-3` `[X]` Subtree error boundaries with fallback and retry (= `X-L4-3`).

---

## W2 — Fine-grained reactive and compiler-driven UI

**Root (L0–L2).** Same substrate and same document host as W1. The difference
is entirely at L3, which is what makes this archetype the cleanest natural
experiment in the whole analysis.

**Semantics and model (L3–L4).** Reactive signal graphs or build-time emitted
update code (F3.1, variants 3 and 4) instead of diffing. State cells are wired
to the nodes that depend on them, so update cost is proportional to the change.
Effects track dependencies automatically rather than by hand.

**Integration (L5–L6).** Thin, and for the same reason as W1: the engine does
it. L6 remains third-party, though these communities ship more of it in-house
than W1 does.

**Loop and ship (L7–L8).** Good loops; inspection is weaker, because the thing
being inspected is either a subscription graph or generated code rather than a
tree that matches the source.

**Project (L9).** Smaller ecosystems, smaller talent pools, excellent
benchmark reputations.

**Strengths.** Update cost proportional to the change; near-zero shipped
runtime in the compiler variant; no memoization ceremony; benchmark numbers
that set the expectation everyone else is judged against.

**Weaknesses.** Tracking rules leak into the mental model — when a read is
tracked, when it is not, where a subscription is created — replacing one subtle
bug class with another. Compiler variants debug in generated output. Host
fidelity, accessibility, and internationalization are nobody's job.

**Opportunities.** Ownership makes precise dependency tracking explicit rather
than magic: dependencies can be values a developer can read. Macros and
monomorphization give build-time specialization inside the language's own
toolchain, and the client JavaScript those macros emit carries source maps
back to the Rust it came from, so stack traces point at source instead of at
emitted code.
And the transient-state rule (`PLAN.md` 2.10) already gives us the
change-proportional path for exactly the updates that matter most — it is
currently an implementation practice rather than a contract.

**Threats.** Their published numbers define the bar a diff-and-patch
architecture is measured against, fairly or not.

**Concepts introduced here.** `C03` change-detection strategy; `C04` positional
memoization and skipping; `C08` derived state and selectors. Each is analysed
on its own merits, independently of this archetype, in
[`concepts-core.md`](concepts-core.md).

**What we must ship.**

- `W-FG-1` `[X]` Targeted update paths for high-frequency state (text entry,
  scroll offset, animated values, list windows) that mutate host objects with
  no tree pass, formalized as a contract with tests (`PLAN.md` 2.10 promoted).
- `W-FG-2` `[X]` Public, reproducible benchmarks with per-target CI budgets:
  startup, update cost, input latency, memory, artifact size (= `X-L0-1`).

---

## W3 — The full-stack meta-framework

**Root (L0–L2).** W1's substrate and realization, extended across the network:
the same dynamic runtime executes on a server or in a per-request host, and the
document tree is produced there and re-attached in the browser. The
consequential root fact is that *the same code runs in two places with no typed
boundary between them* — the substrate has no mechanism to check what crosses.

**Semantics and model (L3–L4).** W1's diffing plus hydration: the client
rebuilds enough of the tree to attach behaviour to server-produced markup.
Hydration mismatch is the permanent defect class this creates. Routes come from
the file tree or a route table, with nested layouts and per-route data loaders;
mutations are typed actions by convention, checked at runtime.

**Integration (L5–L6).** This is the archetype's real achievement: L6 is
*owned* — routing, data loading, mutations, forms, caching, and revalidation
are framework concerns rather than library choices. L5 remains the engine's.

**Loop and ship (L7–L8).** Excellent loop. Shipping is the differentiator:
build outputs static pages, server-rendered pages, and per-request functions
from one source, and deployment adapters retarget without touching application
code.

**Project (L9).** Enormous, with heavy version churn: architectural revisions
every few releases, and migration as the loudest recurring complaint.

**Strengths.** One mental model across the network boundary — the largest
productivity win of the decade and the reason this archetype displaced W1 as
the default. Per-route rendering strategy. Streaming with partial hydration.
Deployment adapters that make hosting reversible. Conventions that give a new
developer routing, data, error pages, and deployment on day one.

**Weaknesses.** The network seam is untyped in practice — serialized through a
dynamic format, asserted by convention or a generator, failing at runtime.
Hydration is a structural cost, and its mismatches are managed rather than
eliminated. Rendering-strategy interactions (caching layers, revalidation
rules, streaming boundaries, per-route runtimes) produce behaviour that is hard
to reason about locally and hard to reproduce off one hosting platform. Vendor
gravity: the best capabilities are one platform's implementation, and
self-hosting is second-class. Footprint: a runtime on every client and a
language runtime with a memory floor on every server instance — multiplied by
concurrency, which is exactly what per-request hosts do.

**Opportunities.** A typed seam end to end: a server-only function called from
a component as an ordinary compiled call, one definition, checked by the
compiler, with the browser side emitted from that definition rather than
written by hand — unreachable for them without changing substrate.
Server/client mismatch deleted as a category, because one tree definition
produces both the server's markup and the client code that attaches to it;
`PLAN.md` Web milestone K's equivalence test should be promoted from a test to
a stated guarantee. Identical behaviour off-platform,
with every rendering strategy runnable locally. Footprint as a headline number.
A stability policy as a direct answer to their loudest complaint.

**Threats.** Their productivity story does not depend on their runtime, so it
keeps improving while we build backends. Their component ecosystem is
immediately usable. Their hosting makes deployment a two-minute operation, and
harder deployment loses evaluations regardless of merit.

**Concepts introduced here.** `C05` server components; `C06` partial
prerendering; `C07` per-component render modes; `C13` navigation and URL as
typed state; `C25` shared-element transitions; `C29` data-state contracts and
colocated data needs; `C41` web metadata and discoverability; `C42` resource
loading and user-centric metrics; `C58` dev services, continuous tests, error
overlays. Each is analysed on its own merits, independently of this archetype,
in [`concepts-app.md`](concepts-app.md),
[`concepts-core.md`](concepts-core.md),
[`concepts-delivery.md`](concepts-delivery.md).

**What we must ship.**

- `W-MF-1` `[W]` Typed server functions: one definition, compile-time checked
  at both call sites, no generated glue, documented wire format and versioning
  (extends Web milestone H).
- `W-MF-2` `[W]` Per-route rendering strategy — static, server-rendered,
  streamed, client-only — declared in the Milestone 30 route table, with no
  second routing model.
- `W-MF-3` `[W]` Streaming HTML with pending-representation boundaries: the
  shell flushes before slow subtrees resolve.
- `W-MF-4` `[W]` Selective attachment of client code decided by the framework
  from the tree, not by annotation.
- `W-MF-5` `[W]` Cross-mode rendered-output equivalence enforced in CI and
  published as a guarantee.
- `W-MF-6` `[W]` Deployment adapters as a stable documented contract — one
  static host, one long-lived server, one function runtime, one edge/WASM
  runtime — each with a local emulator.
- `W-MF-7` `[X]` Incremental regeneration and response caching with
  inspectable invalidation: cache state queryable, never inferred.
- `W-MF-8` `[X]` A published stability policy with a support window and
  automated migration for every breaking change.

---

## W4 — Islands and content-first progressive enhancement

**Root (L0–L2).** Same substrate; the distinguishing choice is at L2/L3 —
markup is the default output and interactivity is opt-in per region, with
multiple UI technologies allowed to coexist as islands.

**Semantics and model (L3–L4).** Two models side by side: a static page model
and an island model. Cross-island state must leave the component model and go
through an ad-hoc channel.

**Integration (L5–L6).** Strong content pipeline — typed front matter,
collections, generated indexes. L6 application concerns are thin by design.

**Loop and ship (L7–L8).** Best-in-class first-load metrics, which map
directly to search ranking and conversion; static output deploys anywhere.

**Strengths.** Minimal client payload by default, which is the correct default
for most pages; excellent authoring integration; interoperable islands.

**Weaknesses.** Awkward shared state; weak for dense application-shaped
products; a boundary the developer must maintain by hand.

**Opportunities.** Islands *without* the boundary: selective attachment derived
from the tree (`W-MF-4`) produces the same shipped-byte outcome while shared
state stays ordinary component state, because there is only one model. That is
a strictly better position and should be stated as one. Content-first authoring
should also be first-class so our own documentation surfaces are not worse than
our application surfaces.

**Threats.** For content sites their defaults are hard to beat — and content
sites are how many teams first try a framework.

**Concepts introduced here.** `C41` web metadata and discoverability; `C42`
resource loading and user-centric metrics. Each is analysed on its own merits,
independently of this archetype, in [`concepts-app.md`](concepts-app.md).

**What we must ship.**

- `W-IS-1` `[W]` Zero client payload for fully static routes: a route with no
  interactive subtree ships no JavaScript.
- `W-IS-2` `[W]` A typed structured-content pipeline rendered through the same
  component model.
- `W-IS-3` `[W]` A documented progressive-enhancement baseline: forms,
  navigation, and submissions that work before and without the client runtime.

---

## W5 — Resumable, zero-hydration execution

**Root (L0–L2).** Same substrate; the distinguishing choice is at L1 —
execution state is serialized into the markup so the client *resumes* instead
of replaying, and handlers are fetched on first interaction.

**Semantics and model (L3–L4).** Serialization constrains the programming
model: what may be captured in a closure becomes a framework-level rule.

**Strengths.** Startup cost decoupled from application size — structurally the
hardest problem in W1 and W3. Lazy loading at interaction granularity.

**Weaknesses.** Latency moves to first interaction, which is sometimes worse.
Small ecosystem, high conceptual overhead, and debugging tools that must
explain a non-obvious execution model.

**Opportunities.** Client JavaScript generated only from the client subset,
and only for interactive subtrees, is small by construction, so the *outcome*
resumability buys is available without constraining closures in the rest of
the application — provided that code is split, and WebAssembly is shipped only
for the subtrees that opt into it. The requirement to extract is
code splitting at route and interaction granularity, and it is measurable.

**Threats.** If artifact size is ignored, generated client code can be
*larger* than a well-split hand-written bundle and their argument lands
against us.

**Concepts introduced here.** `C07` per-component render modes; `C42` resource
loading and user-centric metrics. Each is analysed on its own merits,
independently of this archetype, in [`concepts-app.md`](concepts-app.md),
[`concepts-core.md`](concepts-core.md).

**What we must ship.**

- `W-RS-1` `[W]` Client module splitting by route with lazily fetched subtrees
  and a per-route size budget enforced in CI.
- `W-RS-2` `[W]` A startup budget independent of total application size,
  measured on a throttled low-end device profile.

---

## W6 — Hypermedia and attribute-driven enhancement

**Root (L0–L2).** The minimal path: no build step, no component model, no
client state. Markup fragments returned by the server replace regions of the
document (F3.1, variant 5).

**Semantics and model (L3–L4).** State lives on the server. Local state, focus,
and scroll position must be preserved explicitly or they are lost with the
fragment.

**Strengths.** Radically low complexity; no build pipeline; no client/server
state synchronization problem; pages that keep working for years untouched.
Pairs naturally with W7.

**Weaknesses.** A network latency floor on every interaction; offline and
optimistic UI essentially out of reach; complex interactions degrade into
unstructured imperative code; no type safety on the client path.

**Opportunities.** Take the deployment simplicity, not the model: a
server-rendered RustNative application returning *typed* tree fragments that
patch the realized tree keeps "the server owns state" while removing the
untyped payload and the latency floor for local interactions. The real appeal
is the absent build step — a single binary with embedded assets and no asset
pipeline to operate is achievable and should be an explicit mode.

**Threats.** The simplicity argument is genuinely strong for applications that
are mostly forms and lists — which is most internal software.

**Concepts introduced here.** `C33` persistent-connection server UI, channels,
presence; `C07` per-component render modes. Each is analysed on its own merits,
independently of this archetype, in [`concepts-app.md`](concepts-app.md),
[`concepts-core.md`](concepts-core.md).

**What we must ship.**

- `W-HM-1` `[W]` A server-driven mode: typed fragment updates applied to the
  realized tree with no bespoke client code, preserving focus, selection, and
  scroll.
- `W-HM-2` `[W]` Single-artifact deployment: server binary with embedded
  assets, no separate asset pipeline required.

---

## W7 — The batteries-included server-full framework

**Root (L0–L2).** Interpreted or managed substrate (F0.1/F0.2), a process per
worker, and no UI realization at all beyond templates. The substrate choice is
what caps its throughput and floors its memory — and what makes it a poor fit
for per-request hosts, which is where a growing share of deployment is going.

**Semantics and model (L3–L4).** Convention over configuration: the framework
knows where code lives and what it means. Dynamic typing or deep reflection
means errors surface in production, which the ecosystem answers with large test
suites.

**Integration (L5–L6).** The most complete L6 in this entire analysis: object/
relational mapping, migrations, authentication and authorization, background
jobs, mail, caching, file storage, validation, localization, an administrative
interface, and a test framework — versioned as one product. Localization is
first-class here, which is worth noting against our own absence of it.

**Loop and ship (L7–L8).** Good loop, mature operational tooling, mature
security defaults on by default: request forgery protection, session handling,
parameter filtering, password hashing, injection-safe query construction.

**Project (L9).** Deep documentation and training material accumulated over
many years; a plugin ecosystem covering payments, search, admin, and identity.

**Strengths.** Time from empty directory to deployed application is the best of
any archetype, and that decides most projects. One upgrade unit. Conventions
that carry teams. A generated admin interface — a feature most teams need and
nobody enjoys building.

**Weaknesses.** Runtime cost and throughput ceilings that force horizontal
scaling early; production-surfaced type errors; convention rigidity that makes
leaving the happy path disproportionately painful; a weak front-end story that
is either limited templates or a bolted-on client framework and its seam; cold
start and process weight that rule out per-request hosts.

**Opportunities.** **This is the archetype RustNative under-serves most.**
`PLAN.md`'s Web track covers rendering and deployment thoroughly and says
almost nothing about the application backend — no authentication, no data
layer, no jobs, no mail, no admin. A framework that renders beautifully and
answers none of those is not competing for the same projects. The
compiled-language version of this archetype barely exists: its niche is
occupied by minimal microframeworks (W8) that deliberately refuse to provide
it. Owning it is the largest uncontested opening in this document.
Compile-time-checked queries beat the archetype at its own strongest
convenience. Its security defaults are a documented checklist, not research.

**Threats.** Building this well is a multi-year effort; building it badly is
worse than not building it. Each missing third-party integration is a lost
evaluation.

**Concepts introduced here.** `C36` changesets; `C37` middleware and typed
extractors; `C38` generated migrations; `C40` auto-configuration; `C57`
generators, templates, codemods. Each is analysed on its own merits,
independently of this archetype, in [`concepts-app.md`](concepts-app.md),
[`concepts-delivery.md`](concepts-delivery.md).

**What we must ship.**

- `W-SF-1` `[W]` A server application model: typed request handling,
  middleware, sessions, and error pages built on the *same* component,
  scheduler, and service contracts as the client — not a separate framework.
- `W-SF-2` `[W]` Authentication and authorization contracts: session and token
  strategies, password and passkey handling, provider federation, and a policy
  model expressed in the type system.
- `W-SF-3` `[W]` A data layer with compile-time-checked queries, migrations
  with up/down and dry run, pooling, and transaction scoping tied to request
  lifetime the way task scopes are tied to component lifetime.
- `W-SF-4` `[W]` Background jobs and scheduled work: durable queues, retries
  with backoff, idempotency keys, and an inspection interface.
- `W-SF-5` `[W]` Secure-by-default request handling: request forgery
  protection, strict content security policy, secure cookie defaults, rate
  limiting, request-size limits, and escaping that cannot be accidentally
  bypassed.
- `W-SF-6` `[W]` A generated administrative surface derived from the schema and
  the policy model.
- `W-SF-7` `[X]` One validation and forms model shared by client and server:
  one schema, one set of error messages, checked at compile time.

---

## W8 — The minimal server microframework

**Root (L0–L2).** Varies — this archetype exists on every substrate, and its
compiled members are our closest neighbours: single binary, low memory, high
throughput, no UI layer at all.

**Semantics and model (L3–L4).** Routing, middleware, handlers. Nothing else,
deliberately.

**Integration (L5–L6).** None provided; composition of small libraries is the
philosophy rather than an omission.

**Strengths.** Small, legible, auditable, replaceable; no opinions to fight;
excellent for services and APIs where W7's batteries are dead weight.

**Weaknesses.** Every application re-solves auth, migrations, jobs, validation,
and observability differently and usually worse; no upgrade unit; no front-end
story.

**Opportunities.** Interoperate rather than compete: existing services of this
shape are what a RustNative application will most often need to sit beside or
inside, and mounting our render path as a handler inside one is an adoption
path worth more than a benchmark win. And the `W-SF-*` pieces should be usable
independently, so this archetype's users can adopt one at a time.

**Threats.** For API-only work we offer nothing they do not, and should not
pretend otherwise.

**Concepts introduced here.** `C35` API contracts from types; `C37` middleware
and typed extractors; `C71` plugin encapsulation and registries. Each is
analysed on its own merits, independently of this archetype, in
[`concepts-app.md`](concepts-app.md),
[`concepts-delivery.md`](concepts-delivery.md).

**What we must ship.**

- `W-MS-1` `[W]` A handler-level integration contract: a RustNative render path
  mounted inside an existing compiled-language HTTP service, sharing its
  listener, middleware, and configuration.

---

## W9 — The enterprise typed server platform

**Root (L0–L2).** Managed VM with ahead-of-time compilation (F0.2) — the
substrate that is actively closing the startup and footprint gap, which makes
this the most credible substrate competitor on the server.

**Semantics and model (L3–L4).** Modules, dependency injection (increasingly
resolved at compile time), declarative configuration, strong typing.

**Integration (L5–L6).** Deep operational maturity: health checks, metrics,
tracing, configuration management, secret handling, and a serious security and
compliance posture — which is what actually decides regulated procurements.

**Strengths.** Structure that survives large teams and long tenure;
compile-time wiring and ahead-of-time compilation delivering fast startup on a
managed runtime; excellent profilers and debuggers; operational and compliance
maturity.

**Weaknesses.** Ceremony and indirection; slow local iteration; a learning
curve measured in months; a runtime baseline that is trimmed, not removed;
ahead-of-time mode in tension with its own reflection-dependent ecosystem.

**Opportunities.** Rust gives compile-time wiring with no container at all —
`Services` and the capability contracts are the seed. What is missing is
configuration, health, and operational surface, which is a checklist rather
than research.

**Threats.** In regulated environments their compliance evidence and vendor
support decide the purchase, and technical superiority does not substitute.

**Concepts introduced here.** `C16` scoped dependency injection; `C40`
auto-configuration; `C58` dev services, continuous tests, error overlays; `C62`
profile-guided startup and startup tracing; `C70` vendor-neutral observability.
Each is analysed on its own merits, independently of this archetype, in
[`concepts-app.md`](concepts-app.md), [`concepts-core.md`](concepts-core.md),
[`concepts-delivery.md`](concepts-delivery.md).

**What we must ship.**

- `W-EP-1` `[X]` Layered configuration (defaults, files, environment, secrets)
  with typed access, startup validation, and no global mutable state.
- `W-EP-2` `[X]` Operational surface: liveness and readiness, structured
  logging, metrics, and tracing that spans the client/server boundary.
- `W-EP-3` `[X]` Compliance evidence generated from the build: dependency
  inventory and licences, accessibility conformance, privacy and permission
  manifests per target.

---

## W10 — Per-request function platforms

**Root (L0–L2).** A host that creates a process or sandbox per request and
destroys it afterwards. Cold start is the defining property, and it is a
substrate consequence — which is why this archetype punishes F0.1 and F0.2
hardest and rewards F0.4 most.

**Semantics and model (L3–L4).** Nothing durable between invocations; every
durable concern goes to a service. Execution deadline, memory ceiling, payload
and response limits are hard.

**Integration (L5–L6).** Platform-integrated identity, queues, storage, and
events — which is the actual lock-in, not the function body.

**Loop and ship (L7–L8).** Imperfect local reproduction, so behaviour differs
between development and production; platform-mediated, often thin
observability; no servers to operate and scale-to-zero economics.

**Strengths.** Operational simplicity, per-request failure isolation, and cost
that follows usage.

**Weaknesses.** Cold start; statelessness pushing latency into services;
development/production divergence; platform lock-in through surrounding
services.

**Opportunities.** Their defining constraint is our defining strength. A
compiled, single-threaded request path with no lazily created process-wide
runtime, a measured cold start and a measured memory ceiling, is decisive —
and `PLAN.md` Web milestone K already specifies the discipline. It should be
promoted to a published, CI-enforced budget. Response-bounded task scopes are a
correctness guarantee they leave to developer care and we already have the
mechanism for. A faithful local emulator attacks their worst daily annoyance.

**Threats.** Competing on the function body alone wins little when the lock-in
is the surrounding services.

**Concepts introduced here.** `C44` event-driven triggers; `C45` durable
execution; `C47` cold-start mitigation, revisions, traffic splitting; `C48`
resource bindings. Each is analysed on its own merits, independently of this
archetype, in [`concepts-app.md`](concepts-app.md).

**What we must ship.**

- `W-SL-1` `[W]` Cold-start, memory, and artifact-size budgets for the
  serverless path measured in CI on every commit, with the numbers published.
- `W-SL-2` `[W]` Host-limit capabilities: execution deadline, memory ceiling,
  filesystem availability and durability, payload and response limits — as
  queryable answers.
- `W-SL-3` `[W]` A local emulator for both runtime shapes (native per-request
  binary, sandboxed WASM) enforcing the same limits locally.
- `W-SL-4` `[W]` Request-scoped task scopes that cancel at response, with a
  test proving no work outlives the response.

---

## W11 — Edge and isolate runtimes

**Root (L0–L2).** Sandboxed modules — frequently WebAssembly — started per
request at the network edge, with single-digit-millisecond startup, a
restricted API surface, and strict CPU-time limits. WASM is a *native output*
for our substrate and a compatibility layer for theirs.

**Semantics and model (L3–L4).** Same as W10, with tighter CPU budgets and a
smaller API surface; heavy render work is infeasible.

**Integration (L5–L6).** Edge storage and cache primitives with distinctive
consistency, TTL, and placement semantics that differ per platform.

**Strengths.** Startup cost near zero; global distribution without operating
regions; a WASM-first posture that suits a compiled Rust framework exactly.

**Weaknesses.** Restricted APIs (no arbitrary filesystem, limited networking,
no long-lived compute); strict CPU budgets; runtime divergence between edge and
server paths; hard debugging and harder reproduction.

**Opportunities.** This is the deployment shape where RustNative should be
measurably best: same tree, same router, same typed server functions, inside
the CPU budget. The capability model (`PLAN.md` 2.5) is the right way to
express edge restrictions, and streaming rendering under a CPU budget is
exactly what Web milestones H and K are designed for.

**Threats.** Abstracting over per-platform storage semantics risks the
lowest-common-denominator outcome `PLAN.md` section 1 forbids.

**Concepts introduced here.** `C46` single-instance edge actors; `C47`
cold-start mitigation, revisions, traffic splitting; `C48` resource bindings.
Each is analysed on its own merits, independently of this archetype, in
[`concepts-app.md`](concepts-app.md).

**What we must ship.**

- `W-ED-1` `[W]` A first-class edge/WASM host adapter with restrictions
  expressed as capabilities rather than compilation failures.
- `W-ED-2` `[W]` A per-route CPU-time budget for the render path and a
  documented strategy for routes that exceed it.
- `W-ED-3` `[W]` Edge storage and cache contracts that expose each platform's
  distinctive semantics rather than flattening them.

---

## W12 — Deployment and infrastructure tooling

**Root.** Not an application substrate at all — a separate language, tool, and
mental model that turns code into infrastructure. Its root property is that it
is *outside* the application, which is the source of both its power and its
drift.

**Strengths.** Infrastructure as reviewable versioned code; per-branch preview
environments, which changed how teams review work; typed programmatic
definitions with reusable constructs; local emulation of the deployed
environment; log streaming, secrets, and rollback as first-class commands.

**Weaknesses.** State-file and drift management as a recurring hazard;
deployment loops measured in minutes; shallow cross-provider abstraction; a
second language and model to maintain.

**Opportunities.** `rustnative` already owns build and packaging; extending it
to deploy keeps one tool and one manifest, and infrastructure requirements can
be *derived* from what the application declares — routes, services,
capabilities, storage — rather than written twice. Preview environments and
one-command rollback are cheap once adapters exist.

**Threats.** Teams already own their deployment tooling and reject frameworks
that insist on their own; generating artifacts those tools consume is the
pragmatic position.

**Concepts introduced here.** `C45` durable execution; `C48` resource bindings;
`C64` remote builds and build caching. Each is analysed on its own merits,
independently of this archetype, in [`concepts-app.md`](concepts-app.md),
[`concepts-delivery.md`](concepts-delivery.md).

**What we must ship.**

- `W-DP-1` `[W]` `rustnative deploy` with a documented adapter contract, plus
  export of standard infrastructure descriptions so existing pipelines consume
  our output instead of being replaced.
- `W-DP-2` `[W]` Preview deployments, staged rollout, and one-command rollback
  in the adapter contract.
- `W-DP-3` `[X]` Typed configuration and per-invocation secret reads, with a
  build-time check that no secret is embedded in an artifact.

---

## W13 — Self-hosted function platforms

**Root.** Function-as-a-service on container orchestration the team owns:
scale-to-zero workloads, event routing, container-native packaging.

**Strengths.** No vendor lock-in; regulatory and data-residency fit; works with
existing pipelines.

**Weaknesses.** Full operational burden returns; worse cold start than managed
platforms; thinner tooling.

**Opportunities.** Container output is a small addition once the per-request
path exists, and it unlocks regulated and on-premises buyers. A small compiled
image matters more here than anywhere else, because image pull time *is*
scale-up latency.

**Threats.** Low volume — this is an adapter, not a workstream.

**Concepts introduced here.** `C44` event-driven triggers; `C47` cold-start
mitigation, revisions, traffic splitting. Each is analysed on its own merits,
independently of this archetype, in [`concepts-app.md`](concepts-app.md).

**What we must ship.**

- `W-SH-1` `[W]` Container image output from `rustnative` with a minimal base
  and a declared image-size budget, usable by any orchestrator.

---

## W14 — The supporting ecosystem

The libraries that exist because every archetype above left the same holes:
client state containers, asynchronous data caches, styling systems and
component kits, build tools, and visualization libraries.

**Strengths.** Asynchronous data caches are the most valuable of these —
caching, deduplication, background refetching, pagination, optimistic updates,
and invalidation as one coherent model; teams frequently choose a stack
*because* of this layer. State containers give predictable updates and
time-travel debugging. Utility styling and component kits collapse the
design-to-code gap. Modern build tools set the iteration-speed expectation.
Visualization and 3D libraries cover a need no UI framework provides.

**Weaknesses.** Each is a seam with its own error, async, and time model;
integration burden falls on the application and the combinations are
effectively untested; styling systems drift from host conventions, which is
acceptable on the web and wrong everywhere else.

**Opportunities.** Absorb the data cache into the framework — RustNative has
resources and services but no caching, deduplication, invalidation, pagination,
or optimistic updates, and this is the clearest missing L6 capability in the
whole plan. It is portable to every target, not a web feature. A design-token
pipeline into the existing theme system maps a design system to host-native
appearance per target. Absorbing the utility styling vocabulary is the same kind
of move and is cheaper than it looks, because the half of it that transfers is a
declaration parser over a resolution model we already have (`foundations.md`
F3.4); doing it inside the framework is also what lets the vocabulary be honest
per host, which no library sitting above a framework can be. A component library
covering what every application needs. Custom drawing (Milestone 29) is already
the correct substrate for charting.

**Threats.** These libraries are the strongest single reason to stay on an
existing stack. Choosing which few to match — data caching, design tokens, and
the styling vocabulary teams already type — matters more than breadth.

**Concepts introduced here.** `C08` derived state and selectors; `C11`
reducer-effect architecture with exhaustive tests; `C12` reactive streams;
`C19` lookless and headless controls; `C22` adaptive layout; `C30`
stale-while-revalidate, specified; `C35` API contracts from types; `C43`
micro-frontends and custom elements; `C55` live previews and catalogues; `C60`
semantics-based testing; `C61` record, replay, time travel; `C64` remote builds
and build caching. Each is analysed on its own merits, independently of this
archetype, in [`concepts-app.md`](concepts-app.md),
[`concepts-core.md`](concepts-core.md),
[`concepts-delivery.md`](concepts-delivery.md).

**What we must ship.**

- `X-DATA-1` `[X]` An asynchronous data layer in `rustnative-core`: typed
  queries keyed by identity, declared cache lifetimes, deduplication,
  background revalidation, retries with backoff, pagination and infinite
  scrolling, optimistic updates with rollback, and invalidation that composes
  with the component lifecycle.
- `X-DATA-2` `[X]` Offline-capable mutation queueing with a conflict policy, on
  every target with durable storage.
- `X-UI-1` `[X]` A component library covering the controls applications
  actually need, realized natively per backend, with documented accessibility
  semantics for each.
- `X-UI-2` `[X]` A design-token pipeline feeding the theme system, with
  per-host mapping and a documented token schema (= `X-L3-7`).
- `X-VIZ-1` `[X]` Charting and visualization components on the draw-list path,
  with an accessible alternative for every visual encoding.

---

## W15 — Backend-as-a-service platforms

**Root (L0–L2).** Not an application framework but a *hosted backend*: a
managed database, authentication, file storage, realtime subscriptions, and
per-request functions, reached directly from client code through generated
APIs. The root choice is that the security boundary is moved from an
application server into the data layer itself.

**Semantics and model (L3–L4).** The client holds the application logic;
the backend enforces row-level authorization policies on every query and
subscription (`C39`). Realtime change feeds push updates into client state.
The more ambitious members add offline persistence with background
synchronization (`C32`).

**Integration (L5–L6).** Very strong L6 for small teams: authentication,
storage, realtime, and sync are provided. L5 is whatever UI layer the client
uses.

**Loop and ship (L7–L8).** Local emulators for the whole backend, schema
migrations, generated client types, and hosted deployment. Observability and
cost controls are the platform's.

**Project (L9).** Large communities, generous free tiers, and heavy adoption by
small teams and prototypes.

**Strengths.** Time to a working product measured in days; security enforced
where the data lives; realtime and offline sync without writing a server.

**Weaknesses.** Authorization policies in the database are hard to test and
review; complex business logic eventually needs a real server; vendor coupling
at the data layer, which is the hardest layer to migrate; costs that scale
unpredictably.

**Opportunities.** Two, and they point in different directions. First,
*adopt the concept*: data-layer policies enforced for every query and
subscription are the right design for the server model's authorization
(`C39-1`). Second, *meet the users where they are*: a client-side adapter for
existing hosted backends lets a team adopt RustNative's UI without changing its
backend (`C39-2`) — the backend-side analogue of embedding.

**Threats.** For small teams, "no backend to write" beats "a better backend",
and this archetype plus any UI layer is the fastest route to a launched
product.

**What we must ship.** `C39-1`, `C39-2`, `C32-1` (see
[`concepts-app.md`](concepts-app.md)).

---

## W16 — Persistent-connection server-driven UI

**Root (L0–L2).** A long-lived server process *per connected client* holds the
UI state; the browser holds a thin client that sends events and applies diffs
over a persistent connection. The strongest members run on a runtime built
around lightweight isolated processes and supervision (`C17`), which is what
makes a process per user affordable and fault-isolated.

**Semantics and model (L3–L4).** Server-side components render; the framework
diffs and sends minimal changes; the client patches the document. State is
ordinary server-side state; there is no client state store and no API layer.
Publish/subscribe channels and presence tracking (`C33`) make multi-user
features trivial.

**Integration (L5–L6).** The server has direct access to the database, jobs,
and messaging. Optimistic client hooks cover latency-sensitive interactions.
Offline is impossible by construction.

**Loop and ship (L7–L8).** Very fast loops; live dashboards of the running
system; deployments must drain or migrate connections.

**Project (L9).** Smaller ecosystems with unusually high developer
satisfaction; the managed-runtime variants of the same idea have large
enterprise adoption.

**Strengths.** Rich interactivity with almost no client code; one language and
one state model; real-time collaboration by default; excellent fault isolation
in the process-per-client runtimes.

**Weaknesses.** A network round trip on every interaction not covered by
optimistic hooks; memory per connected client; no offline; reconnection and
deploy behaviour must be engineered carefully.

**Opportunities.** The server-driven mode (`W-HM-1`) already applies typed
fragments to the realized tree. A persistent-connection variant uses the same
reconciler on both ends, so the client patch is ordinary reconciliation, and
per-connection state is a component tree with scope-bound tasks — the lifetime
model already exists. Combined with per-subtree render modes (`C07`), a
component can start server-interactive for instant interactivity and move to
the client once its module arrives.

**Threats.** For internal tools and real-time applications this archetype is
among the most productive in the industry, which is exactly the market the
batteries-included server model (`W-SF-*`) targets.

**What we must ship.** `C33-1`, `C33-2`, `C33-3`, `C07-1`, `C17-1` (see the
concept documents).

---

## Summary: the web opening, ranked

1. **The server-full gap (`W-SF-*`).** The largest uncontested opportunity in
   this analysis: nobody offers a batteries-included application backend in a
   compiled, statically typed language with a native UI story attached.
2. **The typed network seam (`W-MF-1`) and the end of server/client mismatch
   (`W-MF-4`, `W-MF-5`).** Root-layer advantages the incumbent cannot copy
   without changing substrate.
3. **Per-request and edge footprint (`W-SL-*`, `W-ED-*`).** Their worst
   constraint is our best number, and the discipline is already planned.
4. **The data layer (`X-DATA-*`).** The most-used third-party layer in the
   ecosystem, absent from our plan, portable to every target.
5. **The engineering loop (`X-L7-*`).** Not optional: being slower here loses
   evaluations before anything above is examined.
6. **Reconciliation beyond the screen (`C32`, `C33`).** Local-first sync and
   persistent-connection server UI are both reconciliation problems — the
   framework's core competency — and both are how their archetypes win.
7. **Server concepts the substrate improves (`C05`, `C45`).** Server-only
   components with a type-checked boundary, and durable workflows with
   determinism enforced by type rather than by lint.
