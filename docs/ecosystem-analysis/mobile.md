# Mobile archetypes — Android and iOS

Read [`foundations.md`](foundations.md) first. Bottom-up analysis, root choices
to ecosystem, per [`method-and-stack.md`](method-and-stack.md).

RustNative's mobile targets are `PLAN.md` Milestones 35 (Android) and 36
(iOS). Mobile differs from desktop at the root in three ways that shape every
archetype below: **the host owns the process lifecycle** and may destroy it at
any time; **capabilities are permission-gated at runtime**; and **distribution
is gated by a review process with its own rules about code loading, size, and
privacy**. Tags: `[M]` mobile, `[X]` cross-cutting.

---

## M1 — The first-party declarative native UI stack

**Root (L0–L2).** The vendor's own current UI framework: host-language
substrate, host-owned loop, and a retained host view hierarchy that the
declarative layer produces and updates (F2.1). The declarative layer is a
description that is diffed or reactively bound onto that hierarchy.

**Semantics and model (L3–L4).** Identity derived from structure and explicit
keys; constraint or measure/arrange layout; host text stack; and an
observation-based state model (F4.1) with lifecycle-scoped coroutines or
publishers for asynchronous work — structurally close to what `PLAN.md`
Milestones 18–19 specify.

**Integration (L5–L6).** The deepest of any archetype: accessibility, dynamic
text size, right-to-left mirroring, input methods, haptics, system theming,
permissions, background execution, push, deep links, and share — all
first-party. L6 is well covered by companion libraries from the same vendor:
persistence with migrations, navigation, background work scheduling, and
dependency injection.

**Loop and ship (L7–L8).** Live previews with state preservation, on-device
debugging, memory and energy profilers tied to the OS, and first-party
packaging, signing, staged rollout, and crash reporting with symbolication.

**Project (L9).** Vendor documentation and a very large ecosystem, with the
same vendor-churn risk as its desktop counterpart and an aggressive deprecation
cadence driven by annual OS releases.

**Strengths.** Perfect fidelity and accessibility; immediate access to new OS
capabilities; excellent tooling; the review process is designed around it.

**Weaknesses.** One platform only; annual OS churn; language lock-in; and a
declarative layer whose performance characteristics are opaque enough that
teams profile rather than reason.

**Opportunities for RustNative.** Our L2 and L4 choices put us structurally
alongside this archetype — real host views, scope-bound async, stable identity
— while being portable, which it is not. Its companion libraries are also the
clearest specification of what L6 must contain on mobile: persistence with
migrations, scheduled background work, navigation with deep links, and image
loading with caching. Two of those four have no equivalent in our plan.

**Threats.** For a single-platform product, this is the correct choice and will
remain so. Our comparison target is the cross-platform archetypes, not this
one — but this one sets the fidelity bar we are measured against.

**Concepts introduced here.** `C01` interruptible rendering and priority lanes;
`C02` visibility- and lifecycle-aware work; `C04` positional memoization and
skipping; `C09` transactional snapshot state; `C12` reactive streams; `C13`
navigation and URL as typed state; `C14` state scoped to the right lifetime;
`C15` environment down, preferences up; `C22` adaptive layout; `C25`
shared-element transitions; `C31` live queries, database as source of truth;
`C49` surfaces beyond the main window; `C55` live previews and catalogues;
`C60` semantics-based testing; `C62` profile-guided startup and startup
tracing. Each is analysed on its own merits, independently of this archetype,
in [`concepts-app.md`](concepts-app.md),
[`concepts-core.md`](concepts-core.md),
[`concepts-delivery.md`](concepts-delivery.md).

**What we must ship.**

- `M-FP-1` `[M]` Fidelity conformance per mobile backend against the host's own
  applications: dynamic text size, right-to-left mirroring, dark mode, reduced
  motion, haptics, scroll physics, and system gesture areas.
- `M-FP-2` `[M]` Scheduled and constrained background work as a portable
  service contract (network-availability, charging, and deadline constraints),
  because both hosts impose it and neither can be worked around.
- `M-FP-3` `[M]` Image and asset loading with decode, downscale, memory and
  disk caching, and cancellation tied to component lifetime — a requirement on
  every mobile screen that exists.

---

## M2 — The assembled first-party native stack

**Root (L0–L2).** The previous generation of the same vendor stacks:
imperative view controllers and view hierarchies, with the application
assembling third-party libraries for networking, image loading, dependency
injection, reactive streams, and architecture.

**Semantics and model (L3–L4).** Manual view lifecycle, manual state
synchronization, and an architecture pattern chosen per team. Reactive stream
libraries were adopted precisely to tame the resulting state-propagation
problem, and unidirectional-message architectures (F4.1, variant 2) grew here
as the disciplined answer.

**Integration (L5–L6).** Same host access as M1; L6 assembled per application.

**Strengths.** Total control; the largest body of production code and
StackOverflow-era knowledge; libraries that are individually excellent.

**Weaknesses.** The seam-count asymmetry at full strength; state
synchronization bugs as the defining defect class; and very high per-screen
cost.

**Opportunities for RustNative.** This archetype is the strongest evidence for
our L4 position: the industry's own correction was to move toward
unidirectional data flow, immutable descriptions, and scope-bound asynchronous
work — which is what `PLAN.md` sections 2.8 and 2.10–2.12 already specify. It
is also a large migration population, and migration means embedding: a
RustNative subtree inside an existing native screen is how a team would adopt
us without a rewrite.

**Threats.** None directly; the archetype is in maintenance.

**Concepts introduced here.** `C10` statecharts; `C11` reducer-effect
architecture with exhaustive tests; `C12` reactive streams; `C13` navigation
and URL as typed state; `C26` model/view, proxies, identity snapshots; `C34`
declarative HTTP clients. Each is analysed on its own merits, independently of
this archetype, in [`concepts-app.md`](concepts-app.md),
[`concepts-core.md`](concepts-core.md).

**What we must ship.**

- `M-AS-1` `[M]` Host-view embedding on both mobile backends: our tree realized
  into a caller-supplied host view, participating in the host's lifecycle
  callbacks, so adoption can be screen by screen (= `X-INTEROP-1`).
- `M-AS-2` `[M]` The reverse: a host view adopted as a leaf of our tree, laid
  out and clipped by our layout model.

---

## M3 — The bridged cross-platform native-widget framework

**Root (L0–L2).** A dynamic runtime (F0.1) executing application code, driving
*host* native views (F2.1) across a boundary (F0.5). Historically that boundary
was an asynchronous serialized bridge; the modern generation replaced it with
synchronous typed interop precisely because the bridge was the ceiling — the
clearest real-world confirmation of F0.5's analysis in this entire document.

**Semantics and model (L3–L4).** Tree diffing (F3.1) over a portable layout
implementation, with the resulting geometry applied to host views — the same
division of labour `PLAN.md` 2.11 specifies. State and effects follow W1.

**Integration (L5–L6).** Good but not host-level: accessibility works through
mapped properties and lags host semantics; system conventions are approximated
where the abstraction flattens them. L6 is strong in practice thanks to a large
ecosystem of native modules, but those modules are per-platform code the
application team ends up maintaining.

**Loop and ship (L7–L8).** The strongest loop in mobile: sub-second updates
with state preserved, on-device. Also the archetype that made **over-the-air
updates** normal — shipping application code updates without a store review,
within each host's rules — which is a genuine product capability the native
archetypes lack.

**Project (L9).** Very large ecosystem, web-adjacent talent pool, and managed
toolchains that remove most of the native build complexity.

**Strengths.** Real host views, so fidelity is far better than self-drawing
archetypes; one codebase across both mobile hosts and often the web; the
iteration loop; over-the-air updates; and an ecosystem for every device
capability.

**Weaknesses.** A dynamic runtime shipped in the application, with its startup,
memory, and size cost; the interop boundary as a correctness and performance
hazard even after modernization; native modules that require per-platform
expertise anyway — the exact expertise the archetype promised to remove;
version upgrades that are notoriously painful; and untyped boundaries between
application code and native modules.

**Opportunities for RustNative.** This is our closest architectural analogue
and therefore our sharpest comparison: same realization strategy, same portable
layout, same host-view reuse — with a compiled substrate instead of a shipped
runtime, and a typed boundary instead of a serialized one. The concrete
openings are its three weakest points: startup and size, upgrade pain (answered
by a stability policy, `W-MF-8`), and the native-module boundary (answered by a
typed escape hatch, `X-L0-6`). Its over-the-air update capability is a genuine
gap in our plan and must be matched where each host permits it.

**Threats.** Its ecosystem breadth and its loop are both far ahead of ours, and
its managed toolchain makes the first hour of a project dramatically easier —
which is when most evaluations end.

**Concepts introduced here.** `C59` development builds; `C63` continuous native
generation. Each is analysed on its own merits, independently of this
archetype, in [`concepts-delivery.md`](concepts-delivery.md).

**What we must ship.**

- `M-BR-1` `[M]` Over-the-air update support within each host's rules: signed
  update payloads, staged rollout, rollback, version pinning, and a documented
  statement of what each host permits and forbids.
- `M-BR-2` `[M]` Startup, memory, and installed-size budgets per mobile
  backend, measured on a low-end reference device in CI (= `X-L0-1`).
- `M-BR-3` `[M]` A typed native-module contract: per-platform code written
  against a generated, checked interface rather than a hand-marshalled bridge.
- `M-BR-4` `[X]` First-hour experience parity: project creation, device run,
  and live reload in three commands or fewer, documented as a measured target.

---

## M4 — Shared-language multiplatform

**Root (L0–L2).** A compiled substrate targeting each host's own toolchain, so
shared code becomes a native library on each platform, with *no* runtime
bridge. Two variants: share logic only and write UI per platform, or share a
declarative UI layer that draws its own widgets (F2.2) on some targets.

**Semantics and model (L3–L4).** Logic sharing is clean because there is no
boundary to cross. The UI-sharing variant inherits F2.2's ceiling on the
platforms where it draws.

**Integration (L5–L6).** Excellent when UI is per-platform — full host
fidelity, because the UI *is* first-party. Weaker in the shared-UI variant on
platforms where it draws its own widgets.

**Loop and ship (L7–L8).** Native toolchains, native packaging, native
debugging on each side; the shared layer's loop is a compile cycle.

**Strengths.** No runtime tax; logic shared with full type safety across the
boundary; incremental adoption designed in — a shared module can be added to an
existing application without restructuring it; and the per-platform-UI variant
gives up nothing on fidelity.

**Weaknesses.** In the logic-only variant, the UI is written twice, which is
the cost the whole cross-platform category exists to avoid. In the shared-UI
variant, the fidelity advantage evaporates on the platforms that draw.
Tooling maturity varies by target, and the compiled-per-platform model makes
some debugging scenarios awkward.

**Opportunities for RustNative.** This archetype validates our substrate
strategy and shows the adoption pattern that works: *shared logic as a native
library first, UI later*. RustNative can offer that pattern directly — a
RustNative core compiled into an existing native application with no UI at all
— which is a far lower-risk first step than "rewrite your screen". It is also
the archetype that proves we can have both: shared model *and* host-native UI,
because our realization is host-native on every target rather than drawn on
some.

**Threats.** It is the most technically aligned competitor and is growing; its
logic-sharing story is already strong and its UI story is improving.

**Concepts introduced here.** `C65` hierarchical platform-group sharing; `C66`
generated bindings from interface descriptions. Each is analysed on its own
merits, independently of this archetype, in
[`concepts-delivery.md`](concepts-delivery.md).

**What we must ship.**

- `M-MP-1` `[M]` A library-only integration mode: application model, state,
  scheduler, services, and data layer compiled into an existing native
  application, exposed through a generated, typed interface per host language,
  with no UI dependency.
- `M-MP-2` `[X]` A documented adoption ladder — library, then embedded subtree,
  then full application — with a worked example at each rung.

---

## M5 — Hybrid web containers

**Root (L0–L2).** A web application inside the host's webview (F2.4), with host
capabilities exposed through a plugin bridge.

**Strengths.** Web codebase reused on mobile; huge plugin ecosystem for device
capabilities; a very fast first release; over-the-air updates by nature.

**Weaknesses.** It is a web page in a native frame: scroll physics, gestures,
transitions, focus and keyboard behaviour, and accessibility semantics are the
engine's, not the host's; performance on low-end devices is poor; store review
scrutinizes these applications more heavily; and every capability depends on a
community-maintained plugin.

**Opportunities for RustNative.** The clearest fidelity contrast available on
mobile, and the population most likely to be feeling the pain of it. The
transferable lesson is the *plugin ecosystem shape*: a third-party capability
package that works across both mobile hosts, installable without touching the
framework. That is the `X-ECO-*` requirement, and we have no equivalent.

**Threats.** For content-shaped applications this archetype ships fast enough
that quality concerns lose the argument.

**Concepts introduced here.** `C23` platform-adaptive components; `C71` plugin
encapsulation and registries. Each is analysed on its own merits, independently
of this archetype, in [`concepts-app.md`](concepts-app.md),
[`concepts-delivery.md`](concepts-delivery.md).

**What we must ship.**

- `M-HY-1` `[X]` A third-party capability package contract: a community-authored
  service or control implementing a portable contract with per-backend code,
  discoverable and versioned, with no framework fork required
  (= `X-ECO-1`).

---

## M6 — Native-scripting bridges

**Root (L0–L2).** A dynamic runtime with reflective access to the host's entire
API surface, so any native class is callable from script without writing a
module.

**Strengths.** No per-capability plugin needed; full host API reach; host-native
views.

**Weaknesses.** Reflection is slow and fragile across OS versions; no type
checking across the boundary; smaller ecosystem; the model is hard to secure
and hard to optimize.

**Opportunities for RustNative.** It demonstrates real demand for *full* host
API reach without waiting for the framework to wrap it. Our answer is the
escape hatch (`PLAN.md` 2.6) plus host-binding generation, and it should be a
first-class, documented workflow rather than an advanced-user note.

**Threats.** Low.

**Concepts introduced here.** `C66` generated bindings from interface
descriptions. Each is analysed on its own merits, independently of this
archetype, in [`concepts-delivery.md`](concepts-delivery.md).

**What we must ship.**

- `M-NS-1` `[M]` A documented workflow for calling an unwrapped host API from
  application code — generated bindings, ownership and threading rules, and a
  stated support boundary.

---

## M7 — Engine-based application stacks

**Root (L0–L2).** A game engine's runtime and renderer (F2.2 at its most
extreme), with UI drawn into the engine's scene.

**Strengths.** Unmatched for graphics-heavy, animation-heavy, and 3D
applications; one codebase everywhere; excellent asset and content tooling.

**Weaknesses.** Enormous artifacts and memory; no host accessibility at all;
system conventions absent; input methods minimal; a licensing and runtime-cost
model that has repeatedly surprised its users.

**Opportunities for RustNative.** Not a competitor for application software,
but it defines the ceiling of the visual-richness axis, and it is the reason
`D-GX-1` (surface handoff) matters on mobile too: an application with an
engine-rendered core and a native UI shell is a real and underserved shape.

**Threats.** None in this category.

**What we must ship.**

- `M-EN-1` `[M]` Surface handoff on mobile backends: a tree node owning a
  host-native rendering surface with documented lifetime, resize, and
  present semantics (mobile half of `D-GX-1`).

---

## M8 — The managed toolchain and service layer

The archetype this analysis initially under-read. It was first treated as a
wrapper around M3; on a closer reading it is the most complete claim to the
layer *between a repository and an installed, updatable application* that any
framework family has made — and that is the layer where a framework is most
often chosen or abandoned for reasons having nothing to do with its runtime.

**Root (L0–L2).** It has no runtime of its own and realizes nothing. Its root
choice is organizational rather than technical: **the native project is a build
output, not source** (`C63`). Everything else follows. If manifests,
entitlements, and build files are generated, then a library can contribute to
them programmatically, an upgrade can regenerate them, a service can build them
without a developer's machine, and a fingerprint over their inputs can decide
what an installed build is compatible with.

**Semantics and model (L3–L4).** Inherited from the framework it wraps. Its own
additions are a curated, version-locked library set and a universal router
(`C100`) whose routes serve native navigation and web URLs from one table.

**Integration (L5–L6).** A first-party module set covering the common device
capabilities with one API and one release cadence, plus *autolinking* and a
declarative native-module authoring layer (`C99`): adding a dependency that
contains native code requires no project edit, and writing one requires a
declaration rather than hand-written marshalling.

**Loop and ship (L7–L8).** The defining band, and the reason the archetype
exists:

- **development builds** (`C59`) split slow native compilation from the fast
  loop, and **pairing** (`C103`) attaches devices by scanning a code, over the
  local network or a relay, several at once;
- **cloud builds with named profiles and managed credentials** (`C95`, `C64`)
  let a developer produce a signed store build without holding a signing key or
  owning the platform's hardware;
- **store submission is a command** (`C96`), with metadata and phased release
  declared in the repository;
- **updates go to channels, gated by a fingerprint of native inputs** (`C97`),
  with an embedded fallback and adoption reporting — the mechanism that makes
  shipping application code outside the store safe rather than merely possible;
- **pipelines are declared in the repository** (`C98`) as framework-aware jobs;
- **launch assets are generated** from one source (`C101`), and bundle
  composition is inspectable (`C105`).

**Project (L9).** A **versioned release train** (`C93`): one platform version
pinning the library set, the supported host versions, and the toolchain, with a
cadence, a deprecation window, an upgrade command, and a **diagnostics command**
(`C94`) that explains why a project cannot build. A **browser playground**
(`C102`) makes every documentation example runnable from a link, and the
**escape ladder** (`C104`) lets a team materialize the generated native project
and still see — and adopt — what generation would have produced.

**Strengths.** It removes the entire class of work mobile developers dislike
and that has nothing to do with their product: project files, signing,
credentials, submission, compatibility, device setup, upgrade merges. The first
hour is a scanned code and a running application; the release process is a
command; an upgrade is one version number. Compatibility is *computed* rather
than remembered. It became the recommended way to start with the framework it
wraps, which is the strongest available evidence that this layer, not the UI
layer, converts evaluations into adoptions.

**Weaknesses.** Anything not expressible as configuration or a plugin requires
leaving the generated path, and leaving it used to be permanent — the
correction is `C104`, and the fact that a correction was needed is the lesson.
The hosted halves — builds, updates, submission, playground — are a commercial
dependency, with cost per build minute and a trust decision about key custody.
The release train holds back anything that ships faster than it. Its
fingerprinting guarantee is only as complete as its list of native inputs. And
all of it sits on a dynamic-runtime substrate (F0.1) whose costs it cannot
remove: it makes shipping pleasant, not startup fast.

**Opportunities for RustNative.** Three, in order of value.

1. *The layer is already ours to take on desktop.* Milestones 43, 50, and 52
   shipped the developer loop with previews and generators, signed staged
   updates with rollback, deployment adapters with revisions and traffic
   splitting, the stability policy with codemods, capability packages with
   grants, and a machine-readable framework description — on Windows. That is a
   larger fraction of this archetype's L7–L9 surface than any other framework
   family has built outside it. What is missing is not the idea but the specific
   mechanisms catalogued in [`concepts-toolchain.md`](concepts-toolchain.md):
   profiles and credentials, submission, channels and fingerprint gating,
   pipelines, the release train, diagnostics, pairing, the playground,
   generated launch assets, bundle attribution, and the escape ladder.
2. *Our fingerprint can be better than theirs* (`C97-1`). Compatibility gating
   is only as good as the knowledge of what affects the native side. Ours is
   already declarative — the crate graph, `rustnative.toml`, generated manifests
   and entitlements, capability packages and their grants — so the hash is
   computable at build time and checkable at update time, and the guarantee is
   as complete as the declaration rather than as complete as a maintained list.
3. *Key custody can be a choice rather than a product* (`C95-2`). The archetype
   ties managed credentials to its hosted service. A credential contract with
   local, vault, and remote-service backings gives the same developer
   experience to organizations that cannot hand a signing identity to a vendor
   — which is most regulated ones.

**Threats.** This archetype sets the bar for the first hour and for the release
process, and both are judged before any runtime characteristic is measured. Its
hosted services are genuinely convenient, and a framework that answers "how do
I ship this?" with a document of manual steps loses to one that answers with a
command. Most of its mechanisms are also *independent of its substrate*, so
none of our root-layer advantages diminish them: we cannot out-compile this
problem.

**Concepts introduced here.** `C93` the versioned release train; `C94` project
and environment diagnostics; `C95` build profiles and managed credentials;
`C96` store submission as a command; `C97` update channels, runtime
compatibility, and native fingerprinting; `C98` pipelines defined in the
repository; `C99` autolinking and declarative native modules; `C100` one router
for every target; `C101` generated launch assets; `C102` the shareable
playground; `C103` device pairing for the development loop; `C104` the two-way
escape ladder for generated projects; `C105` artifact composition inspection;
and, from the earlier pass, `C59` development builds, `C63` continuous native
generation, and `C64` remote builds and caching. Each is analysed on its own
merits in [`concepts-toolchain.md`](concepts-toolchain.md) and
[`concepts-delivery.md`](concepts-delivery.md).

**What we must ship.** `C93-1`–`C93-4`, `C94-1`, `C94-2`, `C95-1`–`C95-3`,
`C96-1`, `C97-1`–`C97-4`, `C98-1`, `C98-2`, `C99-1`, `C99-2`, `C100-1`,
`C100-2`, `C101-1`, `C102-1`, `C102-2`, `C103-1`, `C104-1`, `C105-1`, plus
`C59-1`, `C63-1`, `C63-2`, `C64-1`, `C64-2`, and `M-BR-4`. Gathered as
Milestone 59 in [`gap-plan.md`](gap-plan.md), with the parts belonging to an
existing milestone routed there.

---

## The mobile-specific obligations every archetype must satisfy

These are host facts, not competitive choices. Any framework that misses one
fails in production regardless of how good its UI layer is.

**Process lifecycle and death.** The host may destroy the process at any time
and restore the user to where they were. This requires saved state, restoration
on cold start, and correct behaviour when restoration and deep-link entry
collide. `PLAN.md` Milestones 30 and 35 cover the mechanism; the *collision
cases* are what break in practice.

**Configuration changes.** Rotation, dark mode, text size, locale, and window
size changes arrive as host events and must not lose state or leak host
objects.

**Permissions.** Runtime, revocable, and sometimes partially granted. The
capability model must express not-yet-asked, denied, denied-permanently, and
limited-grant states rather than a boolean.

**Background execution and energy.** Both hosts strictly limit background work,
and both measure energy use. Work must be expressible as constrained scheduled
jobs, and the framework must not poll.

**Push notifications and deep links** as first-class entry points into the
navigation model, not as an afterthought bolted onto a running application.

**Store review, privacy, and size.** Privacy manifests and data-use
declarations, restrictions on downloading executable code, artifact-size
thresholds that affect install conversion, and required accessibility
statements. These are build-time artifacts and should be generated, not written
by hand (`W-EP-3`).

**Device fragmentation.** Screen sizes, notches and cutouts, safe areas,
foldables and hinges, multi-window and split-screen, variable refresh rates,
low-memory devices, and a long tail of OS versions. This is a test-matrix
obligation as much as a design one.

- `M-OB-1` `[M]` Permission states as a capability enum covering not-asked,
  granted, limited, denied, and permanently-denied, with a portable request
  flow and a documented per-host mapping.
- `M-OB-2` `[M]` Lifecycle conformance suite: process death and restoration,
  configuration changes, deep-link entry during restoration, and
  low-memory trim — run on emulator and on a physical device.
- `M-OB-3` `[M]` Safe-area, cutout, foldable, and split-screen handling as
  layout-model properties rather than per-application code.
- `M-OB-4` `[M]` A device matrix in CI including at least one low-end device
  profile, with the performance budgets from `M-BR-2` enforced on it.

---

## Summary: the mobile opening, ranked

1. **The bridged archetype's substrate tax (`M-BR-2`, `M-BR-3`).** Same
   realization strategy as ours, with a runtime and an untyped boundary we do
   not have. The most direct comparison available, and the easiest to make
   concrete with numbers.
2. **Adoption without rewriting (`M-MP-1`, `M-AS-1`, `M-MP-2`).** Library
   first, then embedded subtree, then application. The only realistic entry
   into an installed base this large, and the archetype that proves it works is
   already doing it.
3. **Over-the-air updates (`M-BR-1`).** A capability our plan lacks entirely
   and that mobile teams treat as non-negotiable.
4. **The mobile host obligations (`M-OB-*`, `M-FP-2`, `M-FP-3`).** Not
   differentiators — the price of being usable at all, and currently only
   partly specified.
5. **Fidelity against web containers and engines.** Real, but it only converts
   when the first hour is as easy as theirs (`M-BR-4`).
