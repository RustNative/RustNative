# Gap plan — from the matrix to the roadmap

[`parity-matrix.md`](parity-matrix.md) says where we stand. This document says
what to build, in what order, and why that order.

It does not restate the backend work already in `PLAN.md` sections 8 and 9.
Everything here is either missing from the plan entirely, or present only as a
bullet under "eventually" when the analysis says it decides adoption.

## The sequencing rule that governs everything below

**Anything that is a per-backend obligation must land before the second
backend exists.** A conformance suite, a capability shape, a layout property,
or an embedding contract costs once when there is one backend and N times when
there are N. This is the same argument `PLAN.md` 2.4 makes about widening
contracts rather than adding conditionals, applied to schedule instead of
structure.

Everything else is ordered by what it unblocks.

## Four tiers

```text
Tier 0  before the second backend      — per-backend obligations and the seams
Tier 1  alongside the backend work     — the loop, the guarantees, the
                                         budgets, the toolchain layer,
                                         interaction depth, and discipline
Tier 2  before any public release      — the application layer users expect,
                                         responsiveness, the visual pipeline,
                                         media and files, and trust
Tier 3  with and after the web track   — the server, deployment, ecosystem,
                                         reconciliation beyond the screen,
                                         durable execution, surfaces,
                                         product operations, and reach
Optional on demand                     — capabilities the existing mechanisms
                                         already cover another way; the
                                         framework is complete without them
```

Tiers are not strict phases: Tier 1 runs continuously the way `PLAN.md`
section 9 does. The optional band is not a tier at all: nothing depends on it,
and each item in it names the trigger that would make it worth building. Milestone numbers below are identities, not an order —
`PLAN.md` section 8 already establishes that convention.

---

# Tier 0 — Before the second backend

## Milestone 53 — The markup syntax

**Why.** `foundations.md` F3.5: a markup surface is how the largest population
of UI developers arrives at a framework at all, and it decays into a trap
unless its equality with the builder surface is enforced by a test. Carried as
a source dialect — the form that population already writes — it carries a
second obligation: the compile step must be invisible in use. It is Tier 0
because the authoring surface is what every later example, template, guide,
doc test, and conformance case is written in — retrofitting a second syntax
through that corpus later costs more than every other Tier 0 item, and grows
with each backend.

**Covers.** `X-L3-8`, `X-L3-9`, `X-L3-10`, `X-L3-11`.

**Scope.**

- `X-L3-8` **Capability equality by construction.** Markup is a compile-time
  front end that lowers to builder calls and nothing else; every node kind is
  an element, every builder method or style field an attribute, `..expr`
  reaches any `Node -> Node` function including an application's own
  extensions, and `{expr}` splices any builder expression into markup. An
  equivalence suite asserts both spellings of every node kind and modifier are
  equal values, with the markup compiled through both carriers.
- **One grammar, two carriers.** `.rsx` files, where an element is an
  expression written anywhere Rust accepts one, for code that is mostly UI;
  and `rsx!`, for markup in `.rs` files, crates without a build step, and
  runnable documentation. The `.rsx` compile step only wraps markup in
  `rsx!`, so parsing, lowering, and diagnostics exist once, and the no-bare-text
  rule keeps the grammar identical in both.
- Structural constructs markup is good at — conditional and repeated children,
  fragments, component elements with typed props (their context inferred from
  the enclosing function in `.rsx` files, named explicitly in `rsx!`) —
  without narrowing the builder form to match, or the reverse.
- `X-L3-9` Diagnostics at compiler quality, held by a compile-failure suite
  run through both carriers.
- `X-L3-10`, `X-L3-11` Tooling parity and an invisible compile step:
  diagnostics from `rustnative build`, `check`, and `test` reported at the
  `.rsx` source; a language-server proxy mapping positions both ways; a
  whole-file formatter; an expansion view; source-map round trips under test.
- Both syntaxes in every document, template, and doc example, side by side;
  project templates that require a choice rather than defaulting to one.

**Done when.** The equivalence suite covers every node kind and modifier
through both carriers, the diagnostics suite covers every listed error through
both carriers, a `.rsx` diagnostic lands at its source position in the CLI and
the editor, both templates build, and no documented example exists in only one
syntax.

## Milestone 58 — The style spellings

**Why.** `foundations.md` F3.4: the cascading styling model splits cleanly into
a declaration vocabulary — the most widely known way to express a style — and a
resolution mechanism this framework already has its own version of. Taking the
first without the second gives the familiar spelling at the cost of a parser;
taking both would put a second engine against our own per-node resolution on
every host. The utility archetype already made that split, which is why it, and
not a stylesheet, is what a host-native framework can carry. It is Tier 0 on two
counts: it is an authoring surface, so Milestone 53's corpus argument applies
unchanged, and the per-property capability table and unit mapping it defines are
per-backend obligations of exactly the kind this tier exists to land early.

**Covers.** `X-L3-12`, `X-L3-13`, `X-L3-14`, `X-L3-15`, `X-L3-16`, the
resolution half of `X-L3-7`; concept `C22-4`.

**Scope.**

- `X-L3-13` A declaration vocabulary in a `rustnative-style` crate — values,
  units, arithmetic, colour functions and spaces, token references — parsed at
  build time, with no selector, specificity, or cascade admitted at any point,
  and every property mapped to exactly one typed style or layout property.
- `X-L3-12` **Vocabulary equality by construction.** The utility layer is a
  compile-time front end emitting typed properties, so it cannot carry a style
  the typed spelling lacks; an equivalence suite asserts both spellings resolve
  to equal values, an unresolvable class is a compile error naming the property
  it expected, and an expansion command prints what a class string became.
  Compatibility target and version are stated rather than implied, and the
  default token set is vendored under its own licence and pinned.
- `X-L3-14` Token-valued declarations resolved at resolution time, so theme,
  colour-scheme, and palette changes re-apply to existing host objects without a
  rebuild or a tree pass — and so a design-token export and a hand-written theme
  are the same artifact (`X-L3-7`, with Milestone 48 supplying the pipeline).
- `X-L3-15`, `X-L3-16` Per-backend style capability tables and unit mappings,
  asserted by conformance (Milestone 41 thereafter): an unavailable property
  fails the build rather than vanishing, and the cell-quantized mapping is
  stated with its rounding rule rather than chosen in one backend's source.
- `C22-4` State, colour-scheme, and size-class variants bound to mechanisms that
  already exist — Milestone 21's state variants and Milestone 39's size classes
  — with the structural variants taken up as typed relationships in Milestone
  67, and container-relative and relational variants specified the same way in
  the optional Milestone 68, rather than implied.
- Both style spellings in every document, example, template, and component
  entry, on the same terms as the two syntaxes.

**Done when.** The equivalence suite covers every documented style property in
both spellings, an unknown class and an unrealizable property each fail the
build with a spanned diagnostic, a token change re-themes a running application
on Windows, and every backend's capability table and unit mapping is recorded.

## Milestone 39 — Portable-surface obligations

**Why.** Three archetype families fail in the same place: a portable API shaped
by the first host it was written for (`desktop.md` D6, D10; `mobile.md` M3).
The cost of fixing it rises with each backend.

**Covers.** `D-PT-1`, `D-PT-2`, `D-TW-1`, `X-L3-3`, `M-OB-1`, `M-OB-3`,
`X-L5-2`, `X-L4-4`, `X-L1-2`, `X-L0-5`, `X-L0-6`; concepts `C15-1`–`C15-3`,
`C20-1`–`C20-2`, `C21-1`, `C22-1`–`C22-2`, `C24-1`–`C24-2`, `C49-1`, `C65-1`,
`C68-1` (shape).

**Scope.**

- A desktop-class affordance audit of the portable API — window management,
  menus, shortcut maps, hover and cursors, drag-and-drop, multi-window state —
  each expressed as a capability rather than assumed present.
- A reverse audit: which parts of the current API encode a Windows assumption,
  resolved by widening the contract (2.4), not by conditionals.
- Right-to-left as a layout-model property: start/end throughout, mirroring
  applied by the core, with the backend applying host mirroring where it has
  it.
- Safe areas, display cutouts, foldable hinges, and split-screen as layout
  properties.
- Permission states as a capability enum (not-asked, granted, limited, denied,
  permanently-denied) with a portable request flow.
- A gesture arbitration contract describing how our recognizers coexist with a
  host's.
- Per-target panic and teardown policy, thread-affinity enforcement, one
  ownership module per backend, and the escape-hatch contract, all written down
  as the rules a new backend is held to.
- Seven structural decisions from the concept survey, each of which fixes how
  every later backend is written:
  - `C15` a typed, subtree-overridable **environment** carrying host traits
    (theme, locale, direction, text scale, size class, colour scheme, reduced
    motion, posture) and services, fed by every backend, plus upward
    preferences;
  - `C20` a portable **command** type bound by menus, toolbars, shortcuts, and
    host-level surfaces, routed through the focus chain per host convention;
  - `C22` **adaptive-layout** vocabulary in the layout model — size classes per
    axis and container-relative decisions — with posture and hinge geometry as
    environment values;
  - `C24` **per-property native mappers** as the structure every backend uses
    to apply properties to host objects, customizable by applications;
  - `C49-1` the **surface vocabulary** — widget, live activity, tile, extension,
    instant application, companion device, tray extra, jump list, taskbar
    progress — as capabilities, so each backend answers it from the start;
  - `C65` **platform-group crates** (Apple hosts, draw-list hosts) with trait
    contracts their members implement, decided before a second member exists;
  - `C68` the **shape of capability grants** — *may this part of the
    application use it?* — distinct from availability, so services are
    obtainable only through a scoped grant (enforcement in Milestone 51).
- `C21` typestate adopted as a design rule and review checklist for framework
  APIs.

**Done when.** A new-backend conformance checklist exists, the Windows backend
passes it, and each item names the test that proves it — including the
environment, command, mapper, and grant contracts, each exercised on Windows.

## Milestone 40 — Interoperability and incremental adoption

**Why.** The one asymmetry that runs against us is accumulated ecosystem
(`method-and-stack.md`), and the only strategy that has ever beaten it is being
usable *inside* what already exists. Every archetype that grew fast grew this
way. It is Tier 0 because embedding constrains how a backend realizes its root,
and retrofitting it means rewriting each backend's root.

**Covers.** `X-INTEROP-1`, `D-LG-1`, `D-LG-2`, `M-AS-1`, `M-AS-2`, `M-MP-1`,
`M-MP-2`, `D-GX-1`, `M-EN-1`, `W-CL-2`, `W-MS-1`, `E-HAL-1`; concepts `C43-1`,
`C66-1`.

**Scope.**

- `X-INTEROP-1` **Embedding, both directions.** Our tree realized into a
  caller-supplied host window, view, or DOM node; and a foreign host object
  adopted as a leaf of our tree, laid out and clipped by our layout model.
- Library-only mode: application model, state, scheduler, services, and data
  layer compiled into an existing native application through a generated typed
  interface, with no UI dependency.
- Surface handoff: a node owning a host-native rendering surface, with
  documented lifetime, resize, DPI, and present semantics.
- Guest-runtime mode: our runtime driven from someone else's `main` and
  initialization, owning neither startup nor the loop (required for vendor
  embedded SDKs and for mounting inside an existing HTTP service).
- A documented adoption ladder — library, embedded subtree, full application —
  with a worked example at each rung.
- `C66` one annotated interface description of the library-only surface —
  ownership and threading explicit — from which bindings for each host
  language are generated and tested, instead of hand-written per language.
- `C43` export of a RustNative component as a web custom element with typed
  attributes, DOM events, and preserved accessibility relationships — the web
  rung of the adoption ladder.

**Done when.** A sample existing application on each shipped backend hosts a
RustNative subtree, and a sample RustNative application hosts a foreign
control, both under test.

---

# Tier 1 — Alongside the backend work

These run continuously, like `PLAN.md` section 9, and each new backend is
expected to satisfy them as part of being called complete.

## Milestone 41 — Guarantees and conformance suites

**Why.** Our root-layer advantages (`foundations.md` F0.4, F1.4, F2.1, F3.3)
are currently implementation properties, not guarantees. An unproven guarantee
is marketing; a tested one is a moat. This milestone converts each one.

**Covers.** `X-L3-1`, `X-L3-2`, `X-L3-4`, `X-L3-6`, `X-L3-8`, `X-L2-1`,
`X-L2-2`, `X-L1-1`, `X-L1-4`, `X-L0-2`, `X-L5-1`, `W-FG-1`, `D-FP-1`,
`D-FP-2`, `M-FP-1`, `D-WV-1`, `D-SD-2`, `X-L2-3`; concept `C09-1`.

**Scope.**

- Syntax equivalence as a standing guarantee: the suite Milestone 53 creates
  lives here permanently, so a node kind or modifier added in one syntax only
  fails the build.
- A documented invalidation contract with tests that fail on
  over-invalidation, plus render-cause tracing that names the state, prop,
  resource, or effect responsible.
- The transient-state fast path (2.10) as a contract with tests.
- Scope-bound cancellation as a public guarantee: no task observes or mutates
  state after its owner unmounts, proven per target.
- Native-object lifetime as a guarantee, with leak detection as a CI gate.
- Host-object reuse and modal-operation conformance (resize, menu tracking,
  native dialogs, drag loops) per backend.
- A fidelity conformance checklist per backend, measured against the host's own
  first-party applications.
- A text conformance suite: complex scripts, bidirectional text, grapheme
  clusters and emoji sequences, font fallback, spaceless line breaking, caret
  and selection geometry.
- A layout conformance suite at multiple text scales with pseudo-localized
  strings, asserting no clipping, overlap, or lost targets.
- Accessibility assertions in CI plus a recorded screen-reader pass per
  backend.
- The published comparison methodology and results against self-drawing and
  webview archetypes.
- `C09` the batching guarantee as a tested property: every state change caused
  by one message becomes visible in the same render, and no render observes a
  partial set.

**Done when.** Each guarantee has a named test, and no backend is called
complete without passing the suite.

## Milestone 42 — Budgets

**Why.** Every performance claim in this analysis is an assertion until it is
measured, and the archetypes we beat on footprint (`web.md` W3, W10, W11;
`desktop.md` D7a; `mobile.md` M3) are beaten only with numbers.

**Covers.** `X-L0-1`, `X-L0-3`, `W-RS-1`, `W-RS-2`, `W-SL-1`, `W-ED-2`,
`M-BR-2`, `D-WS-1`, `E-GUI-4`, `W-SH-1`, `M-OB-4`; concepts `C42-4`,
`C62-1`, `C62-2`, `C83-2`.

**Scope.** Declared per-target budgets — cold start, resident memory, artifact
size, frame-time distribution including worst case, input latency, per-route
client payload, serverless cold start, edge CPU time per route, embedded RAM
and flash, container image size, and build time — measured in CI on every
commit, on a low-end reference device or profile where one exists, with
regressions failing the build and the numbers published. The concept survey
adds three kinds of budget the list above misses: user-centric web metrics —
largest content paint, interaction responsiveness, and layout shift, with zero
layout shift for framework-controlled content (`C42-4`); a startup *phase*
model traced on every backend — process start, runtime ready, first frame,
first content, interactive — with each phase budgeted and optional
profile-guided release builds (`C62`); and boot-to-first-frame on embedded
reference boards (`C83-2`).

**Done when.** A budget file exists per target, CI enforces it, and the public
documentation quotes the measured numbers rather than adjectives.

## Milestone 43 — The developer loop

**Why.** Our substrate's one structural disadvantage (`foundations.md` F7), and
the pillar most cited in framework selection. No competitor will fix it for us.

**Covers.** `X-L7-1`, `X-L7-2`, `M-BR-4`, `E-PR-1`; concepts `C55-1`–`C55-2`,
`C56-1`, `C57-1`, `C58-1`–`C58-3`, `C59-1`, `C90-1`.

**Scope.** Rebuild-and-restart with application state preserved from a
serialized snapshot, against a declared wall-clock budget; optional dynamic
library reload for the application crate where the platform allows; the loop
working on-device for mobile and embedded, not only on the development machine;
board and device quickstart in one command including flashing, logging, and
restart; and a measured first-hour target — project creation to running on a
device in three commands or fewer.

The concept survey supplies the mechanisms that make that loop competitive
rather than merely fast:

- `C55` **previews and a catalogue**: components rendered in isolation across a
  configuration matrix (theme, locale and pseudo-locale, text scale, size
  class, direction, contrast), in both syntaxes, on the development machine's
  backend and on the headless backend;
- `C59` **development builds** per device target, loading the application crate
  as a separately rebuilt unit so a change reaches the device without
  reinstalling or re-signing;
- `C58` **development services** derived from declared bindings, a continuous
  test mode, and an in-application **error overlay** on every backend pointing
  at source positions, including `.rsx` positions;
- `C57` **`rustnative generate`** for components with their preview and test,
  screens wired into navigation, services, and server resources;
- `C56` a structural editing API on the markup language server, so a visual
  designer can be built without a second model;
- `C90` on-demand installation of toolchains and board support.

**Done when.** The loop's wall-clock time is in the budget file (M42) and is
met on every shipped backend.

## Milestone 59 — The toolchain and service layer

**Why.** `mobile.md` M8 on a closer reading: the layer between a repository and
an installed, updatable application is where a framework is most often chosen
or abandoned, and it is the one competitive surface where none of our
root-layer advantages help — the archetype's mechanisms are independent of its
substrate. Milestones 43, 50, and 52 already built a large part of this layer
on Windows; this milestone completes it and, critically, defines the contracts
*before* the device backends arrive, so Milestones 35–37 inherit a release
process instead of each inventing one.

**Covers.** `C93-1`–`C93-4`, `C94-1`, `C94-2`, `C95-1`–`C95-3`, `C96-1`,
`C97-1`–`C97-4`, `C98-1`, `C98-2`, `C99-1`, `C99-2`, `C100-1`, `C100-2`,
`C101-1`, `C102-1`, `C102-2`, `C103-1`, `C104-1`, `C105-1`, and the unfinished
halves of `C59-1`, `C63-1`, `C63-2`, `C64-1`, `C64-2`, `C90-1`.

**Scope.**

- **The release train** (`C93`): one platform version across the crates, the
  CLI, the templates, and the package index; a published supported-host matrix
  per version; `add` resolving the compatible version rather than the newest;
  `upgrade` moving the train and reporting per dependency what moved and what
  is held back.
- **Diagnostics** (`C94`): `doctor` extended to toolchains and platform SDKs,
  dependency compatibility against the train, configuration against its schema,
  generated-native drift, and credential material — each failure printed with
  the command that fixes it, plus a machine-readable report and a CI mode.
- **Profiles and credentials** (`C95`): named build profiles binding target,
  configuration variant, environment, secrets, update channel, and distribution
  intent; a credential contract with local, vault, and remote-service backings,
  so key custody is a choice rather than a product; secrets resolved per build
  and proven absent from the artifact.
- **Submission** (`C96`) as a command per host with a store, with metadata,
  release notes, and phased-release percentage from the repository.
- **Compatibility and channels** (`C97`): a fingerprint over declared native
  inputs recorded in every build and update manifest, with delivery refused on
  mismatch; channels decoupled from builds so promotion repoints rather than
  rebuilds; an embedded fallback payload; and adoption reporting under the
  telemetry policy.
- **Pipelines** (`C98`): every release step a scriptable command with
  machine-readable output, plus generated pipeline descriptions for two CI
  systems — owning the jobs, not the runner.
- **Extension ergonomics** (`C99`): a native-module declaration generating the
  portable trait, per-backend glue, manifest contributions, and a headless test
  double, with the boundary checked on both sides; and autolinking, so adding a
  package with native code needs no project edit.
- **One router for every target** (`C100`): native navigation, web URLs, server
  routes, and each host's deep-link configuration derived from one table, with
  a static export map.
- **The rest of the chore layer**: generated launch assets per host from one
  source (`C101`); a shareable playground that compiles a snippet to the web
  target, with documentation examples runnable from the same sources
  (`C102`); device discovery, scannable pairing, relay fallback, and
  multi-device fan-out for the development host (`C103`); `eject` and
  `diff-native` with a documented path back (`C104`); and `bundle explain`
  attributing artifact size per crate, asset, and dependency (`C105`).

**Done when** a device backend can be released end to end without a manual
step: `doctor` clean, a profile built and signed with a credential the
developer never handled, submitted, an update published to a channel and
refused by a build whose fingerprint does not match, and the whole run
reproducible from a generated pipeline.

**Depends on** Milestone 43 (the loop), Milestone 50 (updates and adapters),
and Milestone 52 (packages and policy) — each of which it extends rather than
repeats.

## Milestone 44 — Inspection and diagnostics

**Why.** Dynamic substrates get inspection from reflection; we must expose it
deliberately. `PLAN.md` section 9 lists it as "eventually add", which
understates what it decides.

**Covers.** `X-L7-3`, `X-L7-4`, `X-L3-5`, `D-IM-1`, `E-RS-2`; concepts
`C04-2`, `C18-2`, `C24-2`, `C61-1`–`C61-3`, `C88-1`, `C92-1`.

**Scope.** A runtime inspection protocol — declarative tree, realized host
objects and their mapping, state and props (readable and editable), layout with
per-node explanation, event and render tracing, task and scope inspection, host
object lifetimes — over a transport that works locally, on-device, and
remotely; an inspector client shipped with the CLI; an in-application overlay
on the draw-list path for hosts without a second screen; and a reduced form
using deferred host-side formatting for constrained targets.

From the concept survey: `C61` **record, replay, and time travel** — input,
messages, and service responses recorded with redaction rules, replayed
deterministically on the headless and originating backends, stepped backwards
in the inspector, and convertible into a regression test — which message-only
state change and replaceable clock and executor make attainable here in a way
ambient-mutation frameworks cannot match; device recordings that include
sensor and service inputs and replay in the simulator (`C88`); render-or-skip
reasons per component (`C04-2`), property-value provenance (`C18-2`), active
mapper customizations (`C24-2`); and scheduler and task events in existing
embedded trace formats (`C92`).

**Done when.** Every backend answers the protocol, including terminal and
embedded in reduced form.

## Milestone 45 — Test infrastructure

**Why.** `PLAN.md` 2.13 caps verification at what hardware we have. A headless
backend lifts most of that cap for everything above L2, and it is the
prerequisite for testing the application layer built in Tier 2.

**Covers.** `X-L7-5`, `X-L7-6`, `X-L7-7`, `X-L7-8`, `M-OB-2`, `M-OB-4`,
`E-GUI-2`; concepts `C11-1`, `C11-2`, `C14-1`, `C55-3`, `C60-1`, `C60-2`,
`C82-1`.

**Scope.** A headless reference backend realizing the tree into an inspectable
model; synthetic input dispatched through the real input path; a deterministic
test clock and executor; golden and visual regression tests over realized
output; a lifecycle conformance suite (process death and restoration,
configuration changes, deep-link entry during restoration, low-memory trim); a
device and emulator matrix in CI; and a host-side device simulator for
display-bearing embedded profiles.

From the concept survey: `C60` a **test query API over the portable
accessibility tree** — by role, accessible name, label, and state — so every UI
test is also an accessibility check and survives refactoring; `C11` an
**exhaustive test mode** in which an unasserted task, effect, or outgoing
message fails the test, with per-test dependency overrides; `C55-3` every
preview doubling as a golden test; `C14-1` a kill-and-restore test for each
destination's saved state; and `C82` a **hardware-in-the-loop runner** across
emulators, the simulator, and connected boards, whose per-board results are the
only basis for a "supported board" claim.

**Done when.** Component, interaction, and golden tests for the full
application layer run on a machine with none of the target hardware.

---

## Milestone 61 — Interaction depth: scrolling, gestures, and text

**Why.** `concepts-surface.md` Part B: the interactions users cannot name and
notice immediately. A keyboard that covers the field being typed into is the
most reliable one-star review in mobile software; nested scroll coordination is
where cross-platform applications are identified as such; and the text editing
surface is where self-drawing archetypes accumulate their longest-lived defects.
Tier 1, because a backend without these is not finished.

**Covers.** `C111-1`–`C111-3`, `C112-1`, `C112-2`, `C113-1`, `C113-2`,
`C114-1`, `C114-2`, `C115-1`.

**Scope.** Nested scroll coordination, snap points, sticky and collapsing
headers, pull-to-refresh, over-scroll, scroll restoration, and scroll-driven
animation progress; system insets (keyboard, toolbars, panels) as layout
properties animated with the host's own curve, with scroll-into-view for the
focused field; velocity tracking, host-conventional deceleration, rubber
banding, interruptible gestures, and cross-application drag with typed payloads
and file promises; input traits, autofill content types, secure and one-time-code
entry, and the host's edit menu, selection handles, spell checking, and
dictation; and spatial focus navigation with engagement and input-class-scaled
focus treatment.

**Done when** the interaction conformance suite passes on every shipped
backend: a form whose field stays visible as the keyboard animates, a
collapsing header driven by a nested scroller, an interrupted fling, a field
that autofills a one-time code, and a screen fully operable by directional
input.

## Milestone 65 — Engineering discipline

**Why.** `concepts-discipline.md`: the practices that keep this plan's own
promises true as the codebase grows. Each converts something held by review and
habit into something a build fails on.

**Covers.** `C128-1`, `C128-2`, `C129-1`, `C130-1`–`C130-3`, `C131-1`–`C131-3`,
`C132-1`.

**Scope.** A committed public-API snapshot diffed in CI against the declared
version bump, with a sealing policy; section 2's invariants as executable
boundary tests; flakiness detection with automatic quarantine, sharding,
coverage gating on changed code, continuous fuzzing of parsers and manifest
readers, property tests over reconciliation and layout, and sanitizer runs over
every unsafe surface; automated dependency updates behind the suite, signed
build provenance, a mirrored dependency set, and a bit-identical
reproducibility check; and release-health thresholds over guardrail metrics
that halt and revert a staged rollout.

**Done when** a breaking change cannot reach a minor release, a platform
dependency cannot reach the core, an artifact rebuilds bit-identically, and a
rollout halts itself on a health breach.

# Tier 2 — Before any public release

The application layer every competing archetype has and we do not. These are
the reasons commercial software picks a framework.

## Milestone 46 — Internationalization and localization

**Why.** The largest single omission in the current plan
(`parity-matrix.md` L5). Every archetype at every maturity level has an answer.
A framework without one is not viable for commercial software, and retrofitting
it touches every string, every layout, and every backend.

**Covers.** `X-L5-3`, `X-L5-4`; concept `C41-2`.

**Scope.** Typed message catalogues with plural and grammatical-gender
categories; compile-time-checked placeholders; locale-aware number, date,
currency, unit, and collation behaviour delegated to host facilities where they
exist; bidirectional text and locale-aware casing; runtime locale switching
that updates the tree; an extraction and merge workflow for translators with
context; pseudo-localization wired into the dev loop and the layout
conformance suite (M41); locale delivered through the environment (`C15`) so a
switch invalidates only readers; and locale-aware web routing with generated
language alternates (`C41-2`).

## Milestone 47 — State, resilience, and data

**Why.** Four **Absent** rows that together are most of what an application
actually does: shared state, error containment, the async data layer, and
forms.

**Covers.** `X-L4-1`, `X-L4-2`, `X-L4-3`, `X-DATA-1`, `X-DATA-2`, `X-DATA-3`,
`W-SF-7`, `M-FP-2`, `M-FP-3`, `D-MC-1`; concepts `C08-1`, `C08-2`, `C09-2`,
`C10-1`, `C10-2`, `C11-2`, `C12-1`, `C12-2`, `C13-1`–`C13-3`, `C14-1`, `C16-1`,
`C17-1`, `C17-2`, `C29-1`, `C29-2`, `C30-1`, `C30-2`, `C31-1`, `C31-2`,
`C34-1`–`C34-3`, `C36-1`, `C87-1`.

**Scope.**

- A shared/scoped state contract — typed, observable, no global mutable state,
  with defined update ordering and the same lifetime discipline as component
  state — plus a state history contract giving undo/redo.
- Error boundaries: a subtree may fail, be contained, present a fallback, and
  be retried, with the failure reported through the diagnostic channel.
- `X-DATA-1` the async data layer: typed queries keyed by identity, declared
  cache lifetimes, deduplication, background revalidation, retries with
  backoff, pagination and infinite scrolling, optimistic updates with rollback,
  and invalidation composed with the component lifecycle.
- `X-DATA-2` offline mutation queueing with a conflict policy.
- `X-DATA-3` schema migration for persisted state, with up/down and a dry run.
- One validation and forms model shared by client and server — one schema, one
  set of messages, checked at compile time — with two-way binding ergonomics,
  dirty tracking, and a submission lifecycle.
- Constrained background work as a portable service contract, and asset/image
  loading with decode, downscale, caching, and lifetime-bound cancellation.

From the concept survey, the details that separate a real application layer
from a list of features:

- **State.** Derived values cached and recomputed only on changed inputs, and
  slice subscription for shared stores (`C08`); a move-based "prepare off the UI
  thread, apply atomically" pattern (`C09-2`); a documented state-machine
  pattern on enums whose entry and exit effects are tied to task scopes, with
  optional diagram generation (`C10`); stream collection bound to component
  scope with stated hot/cold and backpressure semantics (`C12`).
- **Navigation and lifetime.** Navigation state as a typed value from which the
  host navigation stack is reconciled, typed query parameters on the web, and
  per-destination state scope (`C13`); a declared saved-state subset per
  destination written to the host's restoration mechanism (`C14`); service
  scopes per application, window, destination, and request (`C16`), with
  per-test overrides (`C11-2`).
- **Failure.** Error boundaries built as supervision policies — isolate,
  restart with backoff, escalate — on task scopes and subtrees, with a stated
  default that a child's failure does not cancel its siblings (`C17`).
- **Data.** Query results as an exhaustive state type — loading, empty,
  failure, success, stale-while-refreshing — and colocated, batched data
  requirements (`C29`); freshness and retention as separate lifetimes,
  revalidation on focus and reconnect, hierarchical key invalidation, prefetch,
  cancellation when unobserved, and structural sharing (`C30`); live queries
  over local storage and a documented repository pattern with a gap-filling
  paging source (`C31`); an HTTP interceptor chain, certificate pinning, and
  declared endpoint interfaces (`C34`).
- **Forms.** A changeset type — typed casting, per-field errors, storage
  constraint violations mapped to fields, and a validated output type distinct
  from raw input (`C36`, with typestate from `C21`).
- **Long-running work.** A long-running operation primitive — goal, progress
  stream, cancellation, pre-emption, result — bound to a task scope and shown
  by standard progress components (`C87`).

## Milestone 48 — Components, tokens, and visualization

**Why.** Primitives are not a component set, and the design-to-code path is how
applications are actually built (`web.md` W14; `desktop.md` D3).

**Covers.** `X-UI-1`, `X-UI-2` / `X-L3-7`, `X-VIZ-1`, `D-SD-1`, `E-GUI-3`;
concepts `C18-1`, `C19-1`, `C19-2`, `C20-3`, `C22-3`, `C23-1`, `C25-1`,
`C26-1`–`C26-3`, `C27-1`, `C28-1`, `C28-2`.

**Scope.** A component library covering the controls applications need,
realized natively per backend with documented accessibility semantics for each;
a design-token pipeline into the theme system with a documented schema and an
explicit split between semantic roles mapped to host appearance and absolute
brand values, emitting Milestone 58's token file rather than a format of its own
so an exported design system and a hand-written theme are one artifact; charting
and visualization on the draw-list path with an accessible alternative for every
visual encoding; a documented hybrid pattern
for custom-drawn subtrees inside a natively realized tree; and a constrained
text profile declaring which scripts each embedded profile supports.

From the concept survey: a **headless behaviour layer** for composite controls
— list selection, combobox, menu, tabs, tree, grid navigation, date entry —
carrying focus, keyboard, and accessibility semantics independent of
appearance, on which drawn controls and application composites are built
(`C19`); **container-owned typed layout data** for children (`C18-1`); a
**per-host idiom table** for every control (`C23`); an **adaptive navigation**
component and a **command palette** (`C22-3`, `C20-3`); **matched-geometry
transitions** keyed by shared identity (`C25`); non-copying **sort, filter, and
group views**, animated identity diffs, and **sectioned compositional lists**
(`C26`); a **document model** with autosave, versions, recent documents, and
per-document undo bound to commands (`C27`); and **host content controls** —
embedded web content, media playback with system media controls, and camera
preview — plus the pattern for adding more (`C28`).

## Milestone 54 — Responsiveness under load

**Why.** No row in the layer matrix covers what happens when a render is
*expensive*. The concept survey (`concepts-core.md` Part A) shows every mature
framework reached the same answer — prioritized, interruptible, visibility-aware
work — and had to retrofit it. RustNative's pure render, separated effects, and
structured scopes are exactly the preconditions that retrofit needed. It is
Tier 2 because the first data-heavy application will expose its absence, and
the scheduler contract is cheaper to extend before the application layer
(M47) builds on it.

**Covers.** `C01-1`–`C01-4`, `C02-1`–`C02-3`, `C03-1` (contract), `C04-1`,
`C75-1`, `C76-1`.

**Scope.**

- **Update priorities** attached to the message or state change that causes
  them — immediate, normal, deferrable — rather than a second API.
- **Interruptible reconciliation** for deferrable updates: work split at
  component boundaries, yielding to the host between frames, discarded when
  superseded, with the guarantee that one frame never mixes two versions of the
  same state.
- **Deferred values and pending transitions**, so a component keeps showing
  previous content while new content is prepared and can show that it is stale.
- **Render purity enforced by type**: render receives shared access only.
- **Suspendable task scopes** driven by visibility and host lifecycle, with the
  rule for in-flight work stated per task kind, and offscreen subtrees retained
  at the lowest priority.
- **Skipping by props equality**, decided from the props type, with a report
  when a component cannot participate.
- **The change-detection contract** published: which strategy this is, what
  triggers invalidation, what does not.
- **On constrained targets**, the executor running as one task at a declared
  priority under a static-priority system, never above real-time work, and
  frame pacing that stops completely when idle, verified by measured idle
  current.

**Done when.** A reference application filtering a large data set keeps input
latency within its Milestone 42 budget while the filtered view updates, on at
least two backends; a hidden screen performs no periodic work; and an
embedded reference board shows no periodic wake when idle.

---

## Milestone 60 — The visual pipeline

**Why.** `concepts-surface.md` Part A: the plan reasons about layout and
animation but not about frames — compositing and damage, the deadline on a
variable-refresh display, the colour space a token is in, and how a subtree
becomes an image.

**Covers.** `C106-1`, `C106-2`, `C107-1`–`C107-3`, `C108-1`, `C108-2`,
`C109-1`, `C110-1`, `C110-2`.

**Scope.** A portable promote/opaque/damage vocabulary mapped to each host's
compositor, with layer, damage, and overdraw diagnostics; frame pacing against
the host's display signal including variable refresh, input timestamped at
arrival so input-to-present latency is measured, frame-deadline attribution,
and a hang detector; a declared colour space with conversion at the backend
boundary and display capability as environment values; surface-role materials
mapped to each host's material system under reduce-transparency and contrast
settings; and subtree capture shared by golden tests, share images, export, and
print, plus a secure-content marker.

**Done when** latency and deadline attribution are in the budget file on two
backends, a brand colour matches across gamuts, a sheet uses the host's
material and degrades correctly, and a secure subtree is absent from a
screenshot.

## Milestone 62 — Media, capture, and files

**Why.** `concepts-surface.md` Part C: the services that turn screens into
products, and the most common reason a team drops out of a portable framework
into per-platform code.

**Covers.** `C116-1`, `C116-2`, `C117-1`, `C118-1`, `C119-1`.

**Scope.** An audio session contract (category, focus, ducking, interruption,
routing, background playback) with now-playing metadata and transport controls;
camera, microphone, and screen capture as pipelines with permission states and
host privacy indicators; photo and document pickers, scoped-access bookmarks,
file providers, and file promises; and background transfer that resumes,
reports progress, respects metered and low-data policy, and delivers completion
as an entry point.

**Done when** an example records audio while honouring focus loss, scans a code
with correct privacy indicators, imports a photo without a library permission,
and completes a large upload after being backgrounded.

## Milestone 63 — Trust: identity lifecycle, integrity, consent, and portability

**Why.** `concepts-product.md` Part A: obligations imposed by stores,
regulators, and security reviewers. None is a feature a user asks for; each can
stop a release, and consent and backup classification are far more expensive to
retrofit after data exists.

**Covers.** `C120-1`, `C120-2`, `C121-1`, `C122-1`–`C122-3`, `C123-1`,
`C123-2`.

**Scope.** Account deletion, cross-device session revocation, and a device
authorization flow in the authentication contract, with sign-in through each
host's credential broker; platform attestation as a capability with replay
protection and an observe/degrade/block policy ladder; a typed consent contract
enforced by telemetry, analytics, and the data layer, tracking authorization
per host, generated privacy labels and export declarations, and data-residency
declarations the adapters enforce; and backup classification per store with a
restore-onto-a-different-device test, plus export and deletion operations.

**Done when** an example deletes an account end to end, refuses to emit
analytics without consent, carries generated privacy labels, and survives a
restore onto a different device with credentials excluded and data intact.

## Milestone 67 — The extended style range

**Why.** `foundations.md` F3.4, the ceiling on the other side: a vocabulary cut
to what the narrowest host realizes styles every richer host below what it
offers, and fails on the classes a developer writes first. The capability table
Milestone 58 introduced already answers per property and per host; used in both
directions, with guards in the application, it lets the range grow without any
host pretending. It is Tier 2 because it is part of what the application layer
looks like to a team choosing a framework, and because the guards are cheaper to
introduce before applications grow habits around the portable range.

**Covers.** `X-L3-17`, `X-L3-18`, `X-L3-19`, `X-L3-20`.

**Scope.** Capability, complement, exact, and target variants in both
spellings, decided per realizing backend; declared targets in `rustnative.toml`
checked at build time; approximations reported and refusable; the headless
backend realizing every property as the reference; the property families —
borders, backgrounds and gradients, transforms, effects, motion on Milestone
27's timelines, text, aspect and stacking, interaction, and control parts —
each answered by every backend for native controls and framework-owned boxes;
and the structural variants resolved from the reconciler's child lists. The
plan states where the range will show unevenly — chiefly on Windows — and that
the guards make the difference honest rather than equal.

**Done when** every family exists in both spellings with equivalence cases,
every shipped backend answers every new property and the native answers are
read back, guards resolve per backend inside one binary, an unguarded property
fails the build for each declared target that cannot realize it, a
class-spelled transition runs on the framework's timeline and as generated CSS,
and structural variants agree between the core and the browser.

## Milestone 69 — Windows: scale and composition

**Why.** `desktop.md`: per-monitor scaling is the first item on the Windows
list (`D-WIN-1`), and the backend's unit mapping still equates a logical pixel
with a device pixel. And the Windows backend is where the extended range shows
least, because a GDI child window cannot shadow, blend, or anti-alias; the
host's own compositor can decorate the boxes the framework owns without
touching a control (`D-WIN-2`). Behind both stands a control-set decision that
should be made on evidence rather than drift (`D-WIN-3`).

**Covers.** `D-WIN-1`, `D-WIN-2`, `D-WIN-3`; concept `C106` with Milestone 60.

**Scope.** Layout, fonts, regions, and surfaces scaled per window, with live
monitor changes on the same native windows and an updated unit mapping, before
anything else; then a time-boxed prototype of host composition for
framework-owned containers, with the airspace limits stated as capability
answers, measured against the Windows budgets, and adopted or not on the record;
and the criteria for the control-set decision written down, with the decision
itself left to the evidence.

**Done when** a window crossing monitors of different scale keeps its native
windows at the correct size under a conformance case, the prototype's
measurements and decision are recorded, and the control-set criteria exist.

# Tier 3 — With and after the web track

## Milestone 49 — The server application model

**Why.** The largest uncontested opening identified anywhere in this analysis
(`web.md` W7): nobody offers a batteries-included application backend in a
compiled, statically typed language with a native UI story attached. It is
Tier 3 because it depends on Web milestone H's render path and on M47's data
and forms work.

**Covers.** `W-SF-1`…`W-SF-6`, `W-MF-1`, `W-EP-1`, `W-MS-1`; concepts
`C05-1`, `C05-2`, `C35-1`, `C35-2`, `C37-1`, `C38-1`, `C39-1`, `C39-2`,
`C40-1`, `C41-1`, `C52-2`, `C54-1` (sending).

**Scope.** A server application model built on the same component, scheduler,
and service contracts as the client — typed request handling, middleware,
sessions, error pages; authentication and authorization with session and token
strategies, password and passkey handling, provider federation, and a typed
policy model; a data layer with compile-time-checked queries, migrations with
up/down and dry run, pooling, and transaction scoping bound to request
lifetime; durable background jobs with retries, backoff, idempotency keys, and
inspection; secure-by-default request handling; a generated administrative
surface derived from schema and policy; typed server functions with one
definition checked at both call sites; layered typed configuration; and
mountability inside an existing compiled-language HTTP service.

From the concept survey: **server-only components** whose code is excluded from
the client build and whose boundary is checked at compile time (`C05`); the
server model built on the **Rust ecosystem's established service and middleware
abstraction** with typed extractors, not a private pipeline (`C37`); an **API
schema derived from handler types** with generated documentation, validation,
clients for other languages, and contract tests (`C35`); **migrations generated
from model changes** (`C38`); **data-layer authorization policies** enforced
for queries and subscriptions alike, and client-side adapters for existing
hosted backends (`C39`); **feature-driven defaults** with a build-time
explanation report (`C40`); **typed per-route web metadata** and generated
sitemaps (`C41-1`); passkey sign-in (`C52-2`); and server-side push sending
(`C54-1`).

## Milestone 50 — Deployment, updates, and fleet operations

**Why.** Shipping and updating is where frameworks are judged after the demo,
and we currently have no answer for updates on any target
(`mobile.md` M3; `embedded.md` E6).

**Covers.** `W-MF-6`, `W-MF-7`, `W-DP-1`, `W-DP-2`, `W-DP-3`, `W-SL-2`,
`W-SL-3`, `W-SL-4`, `W-ED-1`, `W-ED-3`, `W-SH-1`, `M-BR-1`, `E-MW-1`,
`E-MW-2`, `E-BL-1`, `W-IS-1`, `W-IS-3`, `W-HM-1`, `W-HM-2`; concepts
`C42-1`–`C42-3`, `C47-1`, `C47-2`, `C48-1`, `C53-1`, `C63-1`, `C63-2`,
`C64-1`, `C64-2`, `C81-1`, `C89-1` (model payloads).

**Scope.** Deployment adapters as a stable documented contract with local
emulators for each shape; `rustnative deploy` plus export of standard
infrastructure descriptions so existing pipelines consume our output; preview
deployments, staged rollout, and one-command rollback; host-limit capabilities
and request-scoped cancellation on per-request hosts; edge storage and cache
contracts preserving each platform's semantics; container and embedded-Linux
package output; mobile over-the-air updates within each host's rules with
signing, staged rollout, rollback, and version pinning; firmware update with
A/B slots, verification, and automatic rollback, delegating to an existing
bootloader; zero-payload static routes and a progressive-enhancement baseline;
and single-artifact server deployment with embedded assets.

From the concept survey: **native project files as generated outputs** on every
backend, never hand-edited, with a typed configuration-plugin hook for
capability packages (`C63`); a **shared build cache** and **remote build and
signing** — explicitly not counted as verification (`C64`); **immutable
revisions and traffic splitting**, and a startup rule that keeps
snapshot-restore hosts safe (`C47`); **resource bindings** declared in project
metadata and used to derive infrastructure and least-privilege permissions
(`C48`); the web **image and font pipelines** and prefetch on intent (`C42`);
store-delivered **dynamic asset and feature packs** (`C53`); **multi-image
firmware builds and signing** with anti-rollback (`C81`); and **model assets**
as a versioned payload type in the update paths (`C89`).

## Milestone 51 — Observability, security, and compliance

**Why.** What makes a framework acceptable to buyers who never read a benchmark
(`web.md` W9; `desktop.md` D7a; `embedded.md` E3).

**Covers.** `X-OBS-1`, `W-EP-2`, `W-EP-3`, `D-WS-2`, `E-SF-1`, `E-SF-2`,
`E-BL-2`, `E-OB-1`, `E-OB-2`, `E-OB-3`, `E-IOT-1`, `E-AI-1`, `E-RB-1`,
`D-CT-2`; concepts `C67-1`, `C68-1` (enforcement), `C69-1`, `C70-1`, `C77-1`,
`C77-2`, `C78-1`, `C80-1`, `C80-2`, `C89-1` (capabilities).

**Scope.** `X-OBS-1` crash capture with per-target symbolication, structured
logging, metrics, and tracing spanning the client/server boundary, with opt-in
privacy-respecting telemetry and a published data policy, and the ability to
reconstruct the tree state at failure from the inspection protocol; a stated
threat model per target with capability scoping and escape-hatch reach;
generated compliance evidence (dependency inventory and licences, software bill
of materials, accessibility conformance, privacy and permission manifests); a
stated certification posture with requirement traceability maintained as build
output; power-aware scheduling, watchdog integration, safe-state panic paths,
and bounded-allocation mode on embedded profiles; and service contracts for
messaging, provisioning and device identity, bounded-latency inference, and
node-graph transports.

From the concept survey: **capability grants enforced** — services obtainable
only through a scoped grant, and third-party packages receiving only what they
declare (`C68`); an optional **isolated worker process** with a typed message
boundary for applications hosting untrusted content (`C67`); **web security
primitives on by default** — nonce-based content security policy, subresource
integrity, typed dangerous sinks, cross-origin isolation, permissions policy
(`C69`); instrumentation through the **vendor-neutral tracing and metrics
standard** with client-to-server trace propagation (`C70`); **pools, arenas,
and high-water-mark reports** for framework structures on embedded (`C77`); the
framework running **unprivileged or supervised** and recovering state after a
supervised restart (`C78`); a **power-loss-resilient state store** verified by
power-cut testing, and generated partition layouts (`C80`); and accelerator
availability as capability answers (`C89`).

## Milestone 52 — The project around the framework

**Why.** L9 decides whether anything above gets a second project, and two items
here — the stability policy and the machine-readable description — are direct
answers to the loudest complaint about the incumbent archetype and to how a
growing share of code is now written.

**Covers.** `W-MF-8`, `X-DOC-1`, `X-DOC-2`, `X-ECO-1`, `X-ECO-2`, `E-RS-3`,
`E-PR-2`, `D-CT-1`, `E-RS-1`, `E-EC-1`, `E-EC-2`, `E-HAL-2`, `E-K-3`;
concepts `C03-1`, `C57-2`, `C57-3`, `C71-1`, `C71-2`, `C73-1`, `C79-1`.

**Scope.**

- A published stability and deprecation policy with a support window, plus
  automated migration for every breaking change.
- `X-DOC-1` task-oriented guides and a published generated API reference;
  `X-DOC-2` a runnable example per subsystem and per supported board.
- `X-ECO-1` a third-party capability package contract: a community-authored
  service or control implementing a portable contract with per-backend code,
  discoverable and versioned, with no framework fork required.
- `X-ECO-2` a machine-readable description of the framework — component and
  service contracts, capabilities, events, and layout semantics — so
  code-generating tools produce correct code rather than plausible code.
- A stated non-duplication policy for the embedded Rust ecosystem, with the
  `Executor` contract demonstrated against an existing embedded executor,
  subsystem capability mapping for a configuration-driven RTOS ecosystem, and
  peripheral access left to the existing trait ecosystem.
- The one-stack span demonstrated by a single application genuinely built for
  desktop and for a device, and documented core cost characteristics (stack,
  heap, worst-case timing).
- From the concept survey: **codemods** shipped with every breaking release and
  run by `rustnative upgrade` — the mechanism behind the stability promise
  (`C57-2`) — and **feature kits** that generate working, tested
  authentication, commerce, and administration (`C57-3`); **compatibility
  metadata** in every capability package — supported backends, required grants,
  framework version range — with a generated index, and package contributions
  scoped to what the package declares (`C71`); a published **change-detection
  contract** for developers arriving from other strategies (`C03-1`); board
  metadata consumed from existing hardware descriptions (`C73`); and an
  executor and clock adapter for a standard RTOS interface (`C79`).
- The **rejected concepts** recorded in `concepts-delivery.md` `C72` kept
  current, so a declined idea is found rather than re-proposed.

## Milestone 55 — Reconciliation beyond the screen: real time and sync

**Why.** Three concepts the survey surfaced are the framework's own core idea —
declare desired state, reconcile the real thing towards it — applied somewhere
other than a UI tree: local-first data sync between devices and a server
(`C32`), a server-held UI tree reconciled into a browser over a persistent
connection (`C33`), and a device fleet reconciled towards a desired
configuration (`C84`). Each is how its archetype wins (`web.md` W15, W16;
`embedded.md` E10), and each is a place where owning a reconciler is an
advantage rather than an implementation detail.

**Covers.** `C07-1`, `C32-1`–`C32-3`, `C33-1`–`C33-3`, `C84-1`, `C85-1`,
`C85-2`.

**Scope.**

- **Sync.** A sync service contract — local-first reads and writes, background
  replication, server push, partial replication, and a declared conflict policy
  per collection — with at least one adapter; optional conflict-free replicated
  types for collaboration, with merge functions property-tested; schema
  versioning across clients at different application versions.
- **Server-interactive UI.** Per-connection component trees on the server,
  events over a persistent connection, reconciler-produced diffs applied on the
  client, reconnection with state recovery, deployment draining, and optimistic
  client hooks; render mode selectable per subtree (static, server-interactive,
  client-interactive, automatic) with state transfer on switch.
- **Channels and presence** as service contracts usable by every mode.
- **Devices.** A typed desired/reported state contract with on-device
  reconciliation, conflict policy, and offline catch-up, sharing machinery with
  the sync service; messaging with explicit delivery-guarantee, retained-value,
  last-will, and persistent-session semantics; and a mapping between typed
  application state and standard device data models, with commissioning
  delegated to existing stacks.

**Done when.** One collaborative example works offline on two devices and
converges; one server-interactive example survives a reconnect and a deploy;
and one device example converges to a desired configuration after being
offline.

**Depends on** Milestone 47 (data layer), Milestone 49 (server model), Web
milestone H.

## Milestone 56 — Durable and event-driven execution

**Why.** The serverless track (Web milestone K) is request-shaped only, yet most
per-invocation workloads are events, and business processes need execution
that survives restarts (`web.md` W10, W11, W12). Rust async functions are
already state machines, and structured scopes already model step lifetime, so
the determinism durable execution requires can be *enforced by type* rather
than linted for.

**Covers.** `C17-1` (server and device processes), `C44-1`, `C44-2`,
`C45-1`, `C45-2`, `C46-1`, `C87-1` (server side).

**Scope.**

- **Event handlers** as a serverless entry point with a standard event
  envelope, batching, partial-failure reporting, retries, dead-letter routing,
  idempotency keys, and the same invocation-bounded task scope as requests.
- **Durable workflows**: steps with recorded results, replay on restart,
  durable timers, external signals, compensation, and versioning rules for
  in-flight executions — with non-deterministic operations reachable only
  through the workflow context, so a non-deterministic workflow does not
  compile; at least one engine adapter.
- **Stateful actors**: identity, single-instance serialized execution, private
  durable storage, and alarms, with an edge adapter and a single-process local
  implementation for development.
- **Supervision** applied to long-lived server and device processes, and
  long-running operations with progress and cancellation exposed across the
  client/server boundary.

**Done when.** A workflow survives a process kill mid-step and completes
exactly once per step; an event handler processes a batch with partial
failures correctly; and an actor-backed collaborative session works under the
local implementation and one edge adapter.

**Depends on** Web milestone K, Milestone 49.

## Milestone 57 — Surfaces beyond the main window, and product services

**Why.** Cross-platform frameworks are most often abandoned at the point where
an application needs a widget, an extension, push, purchases, or secure storage
and has to drop to native code (`concepts-app.md` Part E). The vocabulary lands
in Milestone 39 so every backend answers it; this milestone realizes it.

**Covers.** `C49-2`, `C49-3`, `C50-1`, `C51-1`, `C52-1`, `C54-1`.

**Scope.**

- **Surfaces.** Widgets and tray or menu-bar extras realized from a restricted
  subset of the portable tree on the backends that have them, with data shared
  with the main application through a declared store; share and action
  extensions receiving typed payloads; and each remaining surface in the
  Milestone 39 vocabulary answered honestly per backend.
- **Push.** Registration, token rotation, topics, rich and actionable
  notifications with actions delivered as messages, and background delivery.
- **Commerce.** Catalogue, purchase, entitlements, restoration, and
  subscription state with per-store adapters and server-side receipt
  validation.
- **Secure storage.** A contract backed by each host's protected store, with
  capability answers for hardware backing and biometric gating.
- **Feature flags and remote configuration**: typed flags with compiled
  defaults, caching, offline behaviour, environment integration, and local
  overrides for development and tests.

**Done when.** A reference mobile application ships a widget, a share
extension, push with actions, an in-application purchase, secure token storage,
and a remotely toggled feature, with no application-authored native code.

**Depends on** Milestone 39 (vocabulary, grants), the relevant backends,
Milestone 49 (server-side validation and sending).

---

## Milestone 64 — Product operations

**Why.** `concepts-product.md` Part B: what a product team needs once the
application works — links that survive installation, guidance that teaches,
instrumentation that cannot drift, and a failure trail that is data rather than
a recording of someone's screen.

**Covers.** `C124-1`, `C124-2`, `C125-1`, `C125-2`, `C126-1`, `C126-2`,
`C127-1`.

**Scope.** Association files generated from the route table and verified in CI
against the live domain, with a deferred deep-link contract; onboarding,
what's-new tied to version state, coaching marks, and targeted messages with
frequency caps, plus the host review-request API behind its quota; typed,
versioned analytics events with generated schema documentation and
consent-gated emission, and metric definitions read by both dashboards and
experiments; and a sampled, redacted, consent-gated production diagnostic trail
that shares its format with the development recorder.

**Done when** a link installs the application and lands on its content, a
what's-new screen appears once per version, an analytics event cannot be
emitted without consent or schema, and a crash report carries a replayable
trail with no user text in it.

## Milestone 66 — Reach: enterprise, shell, constrained networks, and the field

**Why.** `concepts-reach.md`: the mechanisms that decide whether the framework
can be deployed where applications actually run — a managed fleet, a desktop
shell, a metered network, a production line. This band is where "every target
is first-class" is tested hardest.

**Covers.** `C133-1`, `C133-2`, `C134-1`, `C134-2`, `C135-1`, `C135-2`,
`C139-1`, `C139-2`, `C140-1`, `C141-1`.

**Scope.** A managed-configuration layer with a published schema generated from
the application's configuration declarations, compliance reporting, kiosk mode,
and enterprise distribution with offline licence validation; file-type
associations and protocol handlers registered by packaging, single-instance
handoff, login items, global hotkeys, badges and progress, and power and
display events; binary delta updates with a patch-chain policy, resumable
fetch, and network-aware policy per channel; and the field set — a device time
contract, a calibration and factory-data store, offline-buffered diagnostics
with a support bundle and gated remote console, and provisioning contracts for
per-unit data, a manufacturing test mode, and factory reset.

**Done when** an administrator configures a deployed application without a
rebuild, a double-clicked document opens in the running instance, an update
completes over a metered link as a delta, and a field device is diagnosed and
reset without a site visit.

# Optional — On demand

## Milestone 68 — Relational styling, container queries, and host passthrough

**Why.** `foundations.md` F3.4, the ceiling of the relational half: what
selectors are used for is mostly relationships between named nodes, which a
framework that owns the tree can resolve without a matcher. The positional ones
are in Milestone 67. The state- and size-dependent ones need a dependency index
— a permanent cost — and a component model with typed state already expresses
each of them through props or the environment, and container-relative decisions
through Milestone 39's size classes. That is the case for specifying them fully
and building them only on demand.

**Covers.** `X-L3-21`, `X-L3-22`.

**Scope.** Group, peer, and direct-children relationships on interaction and
form state, through a dependency index that keeps invalidation bounded and
transient; container-relative variants under the containment rule, settling in
one relayout; decorative generated content as reconciler-inserted nodes hidden
from accessibility; and host passthrough, confined to one host that speaks a
styling language, uninterpreted, outside the equivalence suite, and reported as
unverified. Each part has a recorded trigger and is taken up on its own.

**Done when**, per part taken up, both spellings exist with equivalence cases,
the core and every shipped backend agree, invalidation is held by the
over-invalidation suite, and passthrough is reported as unverified wherever it
appears.

# Dependency order

```text
M53 markup syntax ─────────────────┐
M58 style spellings ───────────────┤
M39 portable-surface obligations ──┼─→ every later backend
M40 interoperability ──────────────┘

M53 markup ──→ M58 attribute surface
M39 size classes ──→ M58 responsive variants

M53 + M58 equivalence suites ──→ owned by M41 thereafter

M41 guarantees        ─┐
M42 budgets           ─┼─ continuous, gate each backend's completion
M43 developer loop    ─┤
M44 inspection        ─┤
M45 test infrastructure┤ ──→ required by M46–M48 and M54
M59 toolchain layer   ─┤ ──→ required by every device backend's release
M61 interaction depth ─┤ ──→ part of every backend's definition of done
M65 discipline        ─┘ ──→ gates releases and the stability policy

M60 visual pipeline ─────────┐
M62 media, capture, files ───┤
M63 trust and compliance ────┤
M54 responsiveness ──────────┤
M46 internationalization ────┤
M47 state, resilience, data ─┼─→ M49 server model ──┬─→ M55 real time and sync
M48 components and tokens  ──┘        ↓             ├─→ M56 durable and event-driven
                              M50 deployment and    └─→ M57 surfaces and
                                  updates                   product services
                                     ↓
                              M51 observability and compliance
                                     ↓
                              M52 the project
                                     ↓
                              M64 product operations · M66 reach

M58 style spellings ─┬─→ M67 extended style range ──→ M68 relational styling
M27 timelines ───────┤                                 (optional, on demand)
M45 headless ────────┘          ↑
M69 Windows scale ──────────────┘ (its Windows half)
M69 Windows composition ←── M60 compositing vocabulary, M42 budgets
```

Three web concepts — offline strategies and worker choreography (`C136`),
navigation performance and route announcements (`C137`), and storage
partitioning with the embedded-context test (`C138`) — belong to Web milestones
I and J, and `PLAN.md` carries them there.

Four further concept requirements belong to backend milestones rather than to
section 11, and `PLAN.md` carries them there: partial prerendering (`C06-1`) in Web
milestone H; typed peripherals in framework drivers (`C74-1`), asynchronous
display region transfer (`C91-1`), and the direct-to-display embedded Linux
profile (`C83-1`) in Milestone 37. The designer non-goal (`C56-2`) is kept with
the rejected concepts in Milestone 52.

`E-K-1`, `E-K-2`, `X-L1-3`, and `X-L1-5` (the `no_std` core profile, the
non-`Send` executor, and the host clock) are already in `PLAN.md`'s long-range
roadmap as shared core work and remain where they are: before the targets that
need them.

---

# The differentiation statement

Parity on the matrix is the price of being considered. This is the argument for
being chosen, and every clause is a claim the work above is designed to make
testable rather than rhetorical.

1. **Host-native realization on every target, from one application model.**
   Competitors have one or the other. The ones with host fidelity are
   single-platform; the ones that are portable draw their own widgets and
   inherit a permanent accessibility, input-method, and system-settings deficit
   (`foundations.md` F2.1 versus F2.2). Proven by M41's conformance suites and
   M41's published comparison, not asserted.

2. **No runtime tax anywhere on the range.** One substrate from a browser
   sandbox to a microcontroller, with declared and CI-enforced startup, memory,
   and size budgets (M42). The archetypes that lead on ergonomics cannot follow
   here, because the tax is their substrate.

3. **Correctness properties that are guarantees rather than conventions.**
   Scope-bound task cancellation, deterministic native-object lifetime, typed
   boundaries where competitors serialize, and compile-time checking across the
   client/server seam (M41, M49). Each with a conformance test behind it.

4. **One seam count of one.** UI, state, layout, routing, persistence, data,
   forms, services, packaging, and deployment versioned as one product with one
   stability policy (M46–M52) — answering both the assembly problem and the
   migration complaint that dominates the incumbent's feedback.

5. **Adoptable inside what already exists.** Library, then embedded subtree,
   then application (M40). This is the only clause that addresses the one
   asymmetry running against us, and it is therefore the one that must not slip.

6. **Reconciliation as a general capability, not a UI trick.** The same idea —
   declare desired state, reconcile reality towards it — drives the UI tree,
   local-first data sync, server-interactive UI over a persistent connection,
   and device fleets (M55). Every competing archetype owns one of those; none
   owns the mechanism that unifies them. This clause is only as strong as the
   concept survey's claim that they are one mechanism, and M55's done-when
   criteria are what test it.
