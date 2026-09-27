# Parity matrix — where RustNative stands

Scored with the rules in [`method-and-stack.md`](method-and-stack.md):
**Met** (shipped, verified on real hardware, regression-tested), **Partial**
(shipped for some targets or cases), **Planned** (specified in `PLAN.md` at
implementable depth, not built), **Absent** (not specified anywhere).

Evidence is `PLAN.md` sections and milestones and `BUILD_STATUS.md` records.
"Verified" means `PLAN.md` 2.13's meaning: run on the real host.

Current reality (2026-09-27): Milestones 1–32 and, on the Windows backend,
Milestones 39–58 are built; `BUILD_STATUS.md` records what each pass verified
and what it owes. Every other backend — macOS, Linux, Android, iOS, embedded,
terminal — and the Web track's milestones A–K remain **Planned**, so most rows
below read "Met (Windows), owed elsewhere". Scores are therefore given as
*core contract* first and *coverage across targets* second, because those two
numbers differ enormously and averaging them would hide the real position.

**On the layer tables below.** They were written before Milestones 39–58 and
are refreshed only where this analysis has direct evidence; `BUILD_STATUS.md`
is authoritative for anything they disagree about. The concept tables were
refreshed on 2026-09-27 for every concept that milestone set names as built.

---

## L0 — Substrate

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Compiled, no runtime floor, deterministic destruction | **Met** | Rust throughout; `rustnative-core` has no platform dependency (2.4) | — |
| Declared startup / memory / size budgets | **Absent** | Performance listed as "measure and eventually optimize" (section 9) | `X-L0-1`, `X-L0-3` |
| `no_std`-capable core profile | **Planned** | Named in Milestone 37 and the long-range roadmap | `E-K-1` |
| Foreign-boundary ownership rules documented per backend | **Partial** | Windows confines raw handles; rule stated but not generalized | `X-L0-5` |
| Escape-hatch contract (obtain, use, invalidate) | **Partial** | 2.6 states the principle; no contract document | `X-L0-6` |
| Native-object lifetime as a tested guarantee | **Partial** | Ownership implemented; leak diagnostics are "eventually" | `X-L0-2` |

## L1 — Execution

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Host-owned loop with scheduler wake-ups | **Met** (Windows) / **Planned** (rest) | Milestone 17; per-backend run-loop integration in section 8 | `X-L1-1` |
| Modal-operation conformance (resize, menus, dialogs, drag) | **Absent** | Not specified as a test anywhere | `X-L1-1` |
| Thread affinity enforced, not documented | **Partial** | Enforced in practice; not stated as a typed or asserted rule | `X-L1-2` |
| Executor seam | **Partial** | `Executor` contract exists (2.4); non-`Send` profile outstanding | `X-L1-3`, `E-K-2` |
| Structured task scopes bound to lifetime | **Met** (core) | Milestone 18 | `X-L1-4` (make it a stated guarantee + conformance test) |
| Host clock abstraction | **Planned** | Named in the long-range roadmap | `X-L1-5` |
| Deterministic test clock/executor | **Absent** | — | `X-L7-6` |

## L2 — Realization

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Host-native realization with identity and reuse | **Met** (Windows) | Milestones 2, 5; 2.7, 2.8 | — |
| Draw-list path for hosts without controls | **Met** (core) / **Planned** (targets) | Milestone 29; reused by Milestones 37, 38 | `E-GUI-1` |
| Per-backend fidelity conformance against first-party applications | **Absent** | Not specified | `X-L2-1`, `D-FP-1`, `M-FP-1` |
| Published fidelity/accessibility comparison methodology | **Absent** | — | `X-L2-2` |
| Host-window/view embedding (both directions) | **Absent** | Not specified for any backend | `D-LG-1`, `D-LG-2`, `M-AS-1`, `M-AS-2` |
| Host rendering-surface handoff | **Absent** | Milestone 29 accepts our draw list only | `D-GX-1`, `M-EN-1` |

## L3 — Semantic UI

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Stable identity, keyed reconciliation, host-object reuse | **Met** | Milestones 2, 4 | — |
| Portable layout with host intrinsic measurement | **Met** (Windows) | Milestones 6–9, `IntrinsicMeasurer`, 2.11 | — |
| Transient-state fast path | **Partial** | 2.10 is a principle, not a contract with tests | `W-FG-1` |
| Invalidation contract + over-invalidation tests | **Absent** | — | `X-L3-1` |
| Render-cause tracing | **Absent** | — | `X-L3-2` |
| Right-to-left as a layout-model property | **Absent** | Layout model has no start/end mirroring story | `X-L3-3` |
| Text-scale and translation-growth conformance | **Absent** | — | `X-L3-4` |
| Layout explanation in tooling | **Planned** | "layout overlays" under section 9 tooling | `X-L3-5` |
| Text conformance (complex scripts, bidi, clusters, fallback, breaking, caret) | **Partial** | Host measurement delegated; no conformance suite | `X-L3-6` |
| Theming | **Met** (Windows) | Milestone 21 | — |
| Design-token pipeline | **Absent** | — | `X-L3-7` / `X-UI-2` |
| Typed style spelling (properties, overrides, state variants) | **Met** (Windows) | Milestone 21; `VisualStyle`, `ComponentStyle`, resolved before realization (2.14) | — |
| Utility-class style spelling at equal vocabulary | **Planned** | 2.14; Milestone 58 | `X-L3-12` |
| Declaration vocabulary without a cascade | **Planned** | 2.14; Milestone 58 `rustnative-style` | `X-L3-13` |
| Tokens resolved at resolution time (runtime theme switch, no rebuild) | **Partial** | Theme resolution exists; declarations and token references do not | `X-L3-14` |
| Per-backend style capability table | **Planned** | Milestone 58; Windows silently ignores border colour and radius today (`BUILD_STATUS.md`) | `X-L3-15` |
| Documented unit mapping per host, with rounding | **Partial** | 2.11 states the conversion boundary; no table, no conformance case | `X-L3-16` |
| Builder authoring surface | **Met** | Every node kind and modifier since Milestone 1; 2.9 | — |
| Markup authoring surface at equal capability, in `.rsx` files and `rsx!` | **Planned** | 2.9; Milestone 53 | `X-L3-8` |
| Markup diagnostics at compiler quality, through both carriers | **Planned** | Milestone 53 compile-failure suite | `X-L3-9` |
| Markup tooling parity (format, editor, expansion view) | **Planned** | Milestones 53, 43 | `X-L3-10` |
| `.rsx` compile step invisible in use (source-mapped diagnostics, language-server proxy, whole-file formatter) | **Planned** | Milestones 53, 43 | `X-L3-11` |

## L4 — Application model

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Components, props, callbacks, lifecycle | **Met** | Milestones 13–16 | — |
| Local state, effects, reactive invalidation | **Met** | Milestones 3, 19 | — |
| Scope-bound async with no post-unmount mutation | **Met** (core) | Milestones 17, 18; 2.12 | `X-L1-4` |
| Shared/scoped state contract without globals | **Absent** | Props and callbacks only; no shared-state story | `X-L4-1` |
| Undo/redo and state history | **Absent** | — | `X-L4-2` |
| Error boundaries with fallback and retry | **Absent** | — | `X-L4-3` |
| Per-target panic and teardown policy | **Partial** | Stated only for the terminal backend (Milestone 38) | `X-L4-4` |

## L5 — Host integration

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Portable accessibility model with per-host bridge | **Met** (Windows) / **Planned** (rest) | Milestones 11, 26; section 8 per backend | — |
| Accessibility assertions in CI + screen-reader pass per backend | **Absent** | Verification described per backend, not automated | `X-L5-1` |
| Advanced input, focus, IME | **Met** (Windows) / **Planned** (rest) | Milestones 11, 12, 25 | — |
| Gesture arbitration with host recognizers | **Partial** | Stated for one mobile backend only | `X-L5-2` |
| **Localization and internationalization** | **Absent** | **Not mentioned anywhere in `PLAN.md` or `README.md`** | `X-L5-3`, `X-L5-4` |
| System settings honoured (contrast, reduced motion, text scale, colour scheme) | **Partial** | Reduced motion and appearance appear per backend; no portable contract or conformance | `D-FP-1`, `M-FP-1` |

## L6 — Application services

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Service/resource system with typed contracts | **Met** | Milestone 20 | — |
| Capability model | **Met** | Milestone 22; 2.5 | — |
| Permission states as capability answers | **Partial** | Mentioned for Android; no portable enum | `M-OB-1` |
| Navigation and routing | **Met** (Windows) | Milestone 30 | — |
| Persistence and restoration | **Met** (Windows) | Milestone 30 | — |
| State-schema migration | **Absent** | Not specified | `X-DATA-3` (see gap plan) |
| **Async data layer (cache, dedupe, invalidate, paginate, optimistic)** | **Absent** | Resources exist; caching and invalidation do not | `X-DATA-1` |
| Offline mutation queueing | **Absent** | — | `X-DATA-2` |
| **Forms and validation** | **Absent** | Text input exists; no forms model | `W-SF-7` |
| Background/scheduled work with host constraints | **Absent** | — | `M-FP-2` |
| Image/asset loading with caching | **Absent** | — | `M-FP-3` |
| Component library | **Absent** | Primitives only | `X-UI-1` |
| Server application model (auth, data, jobs, admin) | **Absent** | Web track covers rendering, not the backend | `W-SF-1`…`W-SF-6` |

## L7 — Engineering loop

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Project scaffolding and CLI | **Met** (Windows) | Milestone 31 | — |
| **State-preserving reload / fast iteration** | **Absent** | Only a browser hot-reload bullet under Web milestone J | `X-L7-1`, `X-L7-2` |
| **Runtime inspection protocol and inspector** | **Planned** (vaguely) | Section 9 "tooling and diagnostics — eventually add…" | `X-L7-3`, `X-L7-4`, `D-IM-1` |
| Headless reference backend for tests | **Absent** | Platform integration tests need real hosts | `X-L7-5` |
| Synthetic input through the real input path | **Absent** | — | `X-L7-7` |
| Core/reconciliation/layout/scheduler unit tests | **Met** | Section 9; `BUILD_STATUS.md` | — |
| Golden/visual regression tests | **Absent** | — | `X-L7-8` (see gap plan) |
| Device/emulator matrix in CI | **Absent** | — | `M-OB-4` |

## L8 — Ship and operate

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Packaging and signing | **Met** (Windows) / **Planned** (rest) | Milestone 32; section 8 per backend | — |
| Deployment shapes (static / server / serverless / edge / container / firmware) | **Planned** | Web milestones H, K; Milestone 37 | `W-MF-6`, `W-SH-1`, `E-BL-1` |
| Deployment tooling (`deploy`, previews, rollback) | **Absent** | — | `W-DP-1`, `W-DP-2` |
| **Updates after ship (OTA, delta, firmware A/B)** | **Absent** | — | `M-BR-1`, `E-MW-1` |
| Performance budgets enforced in CI | **Absent** | Section 9 lists metrics to "measure and eventually optimize" | `X-L0-1` |
| Security threat model and supply-chain gates | **Partial** | `deny.toml`, `SECURITY.md`, `clippy.toml` exist; no stated threat model | `D-WS-2`, `E-SF-1` |
| Compliance evidence generated (SBOM, licences, accessibility, privacy) | **Absent** | — | `W-EP-3`, `E-BL-2` |
| Crash capture with symbolication, structured logs, metrics, tracing | **Absent** | — | `W-EP-2`, `X-OBS-1` (see gap plan) |

## L9 — The project

| Capability | Score | Evidence | Gap |
| --- | --- | --- | --- |
| Architecture documentation | **Met** | `PLAN.md`, `README.md`, `BUILD_STATUS.md`, `Audit.md` | — |
| Task-oriented guides and generated API reference | **Partial** | `missing_docs` enforced; no guide set or published reference | `X-DOC-1` |
| Runnable example per subsystem | **Partial** | `examples/` exists | `X-DOC-2` |
| Stability, deprecation, and migration policy | **Absent** | — | `W-MF-8` |
| Incremental adoption inside existing applications | **Absent** | — | `X-INTEROP-1` |
| Third-party extension ecosystem | **Absent** | — | `X-ECO-1` |
| Machine-readable framework description for code generation | **Absent** | — | `X-ECO-2` |
| Layered configuration and operational surface | **Absent** | — | `W-EP-1`, `W-EP-2` |

---

## Concepts

The concept catalogue ([`concepts-core.md`](concepts-core.md),
[`concepts-app.md`](concepts-app.md),
[`concepts-delivery.md`](concepts-delivery.md),
[`concepts-embedded.md`](concepts-embedded.md)) scores each concept on its own
page. Summarized here by layer so the two views can be read together; the
milestone column is where [`gap-plan.md`](gap-plan.md) schedules it.

### Execution and rendering model (L1–L3)

| Concept | Score | Milestone |
| --- | --- | --- |
| `C01` Interruptible rendering and priority lanes | **Absent** | 54 |
| `C02` Visibility- and lifecycle-aware work | **Absent** | 54 |
| `C03` Change-detection contract | **Partial** | 54 |
| `C04` Skipping by props equality | **Partial** | 54 |
| `C05` Server components | **Absent** | Web H, 49 |
| `C06` Partial prerendering | **Absent** | Web H |
| `C07` Per-subtree render modes | **Absent** | Web H, 55 |
| `C18` Container-owned layout data and value precedence | **Partial** | 48 |
| `C22` Adaptive layout (size classes, container-relative, posture) | **Absent** | 39, 58 |
| `C25` Shared-element transitions | **Absent** | 48 |

### State and composition (L4)

| Concept | Score | Milestone |
| --- | --- | --- |
| `C08` Derived state and slice subscription | **Absent** | 47 |
| `C09` Atomic batching guarantee | **Partial** | 47 |
| `C10` State-machine pattern | **Absent** | 47 |
| `C11` Exhaustive effect testing and dependency overrides | **Absent** | 45, 47 |
| `C12` Stream collection bound to scope | **Partial** | 47 |
| `C13` Navigation and query parameters as typed state | **Partial** | 47 |
| `C14` Destination-scoped and saved state | **Partial** | 47 |
| `C15` Typed environment and upward preferences | **Partial** | 39 |
| `C16` Service scopes per window, destination, request | **Partial** | 47 |
| `C17` Supervision policies | **Absent** | 47 |
| `C19` Headless behaviour layer | **Absent** | 48 |
| `C20` Command model | **Absent** | 39 |
| `C21` Typestate as a design rule | **Partial** | 39 |

### UI system and host integration (L3, L5)

| Concept | Score | Milestone |
| --- | --- | --- |
| `C23` Per-host idiom table | **Partial** | 48 |
| `C24` Per-property native mappers | **Absent** | 39 |
| `C26` Proxies, animated identity diffs, compositional lists | **Partial** | 48 |
| `C27` Document architecture | **Absent** | 48 |
| `C28` Host content controls (web content, media, camera) | **Absent** | 48 |
| `C49` Surfaces beyond the main window | **Absent** | 39 (vocabulary), 57 |

### Data, sync, and real time (L6)

| Concept | Score | Milestone |
| --- | --- | --- |
| `C29` Data-state contract, colocated data needs | **Absent** | 47 |
| `C30` Stale-while-revalidate details | **Absent** | 47 |
| `C31` Live queries, repository pattern | **Absent** | 47 |
| `C32` Local-first sync, CRDTs | **Absent** | 55 |
| `C33` Server-interactive mode, channels, presence | **Absent** | 55 |
| `C34` HTTP interceptors, pinning, declared endpoints | **Partial** | 47 |
| `C36` Changesets | **Absent** | 47 |

### Server, API, and distributed execution (L6, L8)

| Concept | Score | Milestone |
| --- | --- | --- |
| `C35` API contracts generated from types | **Absent** | 49 |
| `C37` Ecosystem middleware and typed extractors | **Absent** | 49 |
| `C38` Generated migrations | **Absent** | 49 |
| `C39` Data-layer policies; hosted-backend adapters | **Absent** | 49 |
| `C40` Feature-driven server defaults | **Absent** | 49 |
| `C41` Web metadata and localized routing | **Absent** | 49, 46 |
| `C42` Resource loading and user-centric metrics | **Absent** | 42, 50 |
| `C43` Custom-element export | **Absent** | 40 |
| `C44` Event-driven triggers | **Absent** | 56 |
| `C45` Durable execution | **Absent** | 56 |
| `C46` Single-instance edge actors | **Absent** | 56 |
| `C47` Revisions and traffic splitting | **Absent** | 50 |
| `C48` Resource bindings | **Absent** | 50 |

### Product services (L6, L8)

| Concept | Score | Milestone |
| --- | --- | --- |
| `C50` Typed feature flags and remote configuration | **Absent** | 57 |
| `C51` In-application commerce | **Absent** | 57 |
| `C52` Secure storage and passkeys | **Absent** | 57 |
| `C53` Dynamic delivery | **Absent** | 50 |
| `C54` Remote push | **Partial** | 57 |

### Engineering loop, build, and security (L7–L9)

| Concept | Score | Milestone |
| --- | --- | --- |
| `C55` Previews and catalogue | **Met** (Windows) | 43 |
| `C56` Structural editing API for designers | **Met** (Windows) | 53, 43 |
| `C57` Generators, codemods, feature kits | **Met** (Windows) | 43, 52 |
| `C58` Dev services, continuous tests, error overlay | **Met** (Windows) | 43 |
| `C59` Development builds | **Absent** — owed with the device backends | 43, 59 |
| `C60` Semantics-based test queries | **Absent** | 45 |
| `C61` Record, replay, time travel | **Met** (Windows) | 44 |
| `C62` Startup phases and profile-guided builds | **Met** (Windows) | 42 |
| `C63` Native projects as generated outputs, config plugins | **Partial** — manifest generation yes, plugin hook and the rule not stated | 50, 59 |
| `C64` Shared build cache, remote builds | **Partial** — local cache yes, remote build and signing no | 50, 59 |
| `C65` Platform-group crates | **Absent** | 39 |
| `C66` Generated bindings from one interface description | **Absent** | 40 |
| `C67` Optional isolated worker process | **Met** (Windows) | 51 |
| `C68` Capability grants distinct from availability | **Met** (Windows) | 39, 51 |
| `C69` Web security primitives on by default | **Absent** | 51 |
| `C70` Vendor-neutral instrumentation | **Met** (Windows) | 51 |
| `C71` Package compatibility metadata and scoping | **Met** (Windows) | 52 |

### The toolchain and service layer (L7–L9)

From [`concepts-toolchain.md`](concepts-toolchain.md), scored 2026-09-27
against the Windows backend's shipped state.

| Concept | Score | Milestone |
| --- | --- | --- |
| `C93` Versioned release train | **Partial** — stability policy, codemods, MSRV, per-package ranges; no platform version, no compatible-version resolution, no host matrix | 59 |
| `C94` Project and environment diagnostics | **Partial** — `doctor --install`; no dependency, configuration, credential, or drift checks | 59 |
| `C95` Build profiles and managed credentials | **Absent** — local signing only | 59 |
| `C96` Store submission as a command | **Absent** | 59 |
| `C97` Channels, runtime compatibility, native fingerprinting | **Partial** — signed, staged, pinned, rollback-capable updates; no fingerprint gating, channels, embedded fallback, or adoption reporting | 59 |
| `C98` Pipelines defined in the repository | **Absent** | 59 |
| `C99` Autolinking and declarative native modules | **Partial** — packages, grants, bindings; no module declaration, no autolinking of native code | 59 |
| `C100` One router for every target | **Partial** — one router, localized routes; no deep-link generation or static export map | 59, 49 |
| `C101` Generated launch assets | **Absent** — one icon, embedded as given | 59 |
| `C102` Shareable playground | **Absent** | 59 |
| `C103` Device pairing for the development loop | **Partial** — authenticated remote host over loopback; no discovery, pairing, relay, or fan-out | 59, 43 |
| `C104` Two-way escape ladder for generated projects | **Absent** | 59 |
| `C105` Artifact composition inspection | **Absent** — budgets enforced without attribution | 59, 42 |

### Embedded and device (L0–L8 on constrained targets)

| Concept | Score | Milestone |
| --- | --- | --- |
| `C73` Board metadata from existing descriptions | **Absent** | 52 |
| `C74` Typed peripherals in framework drivers | **Absent** | 37 |
| `C75` Executor as a prioritized task | **Absent** | 54 |
| `C76` Frame pacing that stops; sleeping executor | **Absent** | 54 |
| `C77` Pools, arenas, high-water marks | **Absent** | 51 |
| `C78` Unprivileged or supervised framework | **Absent** | 51 |
| `C79` Standard kernel interface adapter | **Absent** | 52 |
| `C80` Power-loss-resilient state store, partitions | **Absent** | 51 |
| `C81` Multi-image build and signing | **Absent** | 50 |
| `C82` Hardware-in-the-loop runner | **Absent** | 45 |
| `C83` Direct-to-display profile, boot-to-first-frame | **Absent** | 37, 42 |
| `C84` Device desired-state reconciliation | **Absent** | 55 |
| `C85` Messaging semantics, device data models | **Absent** | 55 |
| `C86` Flow-based integration | **Rejected** | — |
| `C87` Long-running operations | **Absent** | 56 |
| `C88` Device-input recording replayed in simulator | **Absent** | 44 |
| `C89` Accelerator capabilities, model assets | **Met** (Windows) | 51, 50 |
| `C90` On-demand toolchains and board support | **Partial** — toolchain install yes, board support owed | 43, 59 |
| `C91` Asynchronous display region transfer | **Absent** | 37 |
| `C92` Kernel-aware trace output | **Absent** | 44 |

---

## Reading the matrix

**Where we are genuinely strong.** L0 through L4 on the core contracts, and L2
through L6 on Windows. The root choices (`foundations.md` F0.4, F1.4, F2.1,
F3.3) are made, implemented, and verified on one host. That is a real
foundation, and it is the expensive half.

**Where the plan is complete but unbuilt.** Every other backend. That is a
scheduling and hardware fact (2.13), not a gap in this analysis's sense, and it
is not what this document is for.

**Where the plan itself is missing something.** Nine items scored **Absent**
that are not backend work and would be missing even if every backend shipped
tomorrow:

1. Localization and internationalization (`X-L5-3`) — the largest single
   omission; no commercial application ships without it.
2. The async data layer (`X-DATA-1`) — the most-used third-party layer in every
   competing ecosystem.
3. The engineering loop: fast iteration and runtime inspection (`X-L7-1`,
   `X-L7-3`) — our substrate's structural disadvantage, currently answered by a
   bullet list under "eventually".
4. Forms and validation (`W-SF-7`).
5. Shared/scoped state (`X-L4-1`) and error boundaries (`X-L4-3`).
6. Updates after ship — mobile OTA and firmware A/B (`M-BR-1`, `E-MW-1`).
7. Performance budgets enforced in CI (`X-L0-1`) — without which every
   performance claim we make is an assertion.
8. The server application model (`W-SF-*`) — the largest uncontested market
   opening identified anywhere in this analysis.
9. Interoperability and incremental adoption (`X-INTEROP-1`) — the only
   strategy that has ever worked against an accumulated ecosystem.

**Where the concept survey adds to that list.** Scoring the concepts separately
exposes gaps the layer view hides, because each is small enough to vanish
inside a larger row:

10. Responsiveness under load — priority lanes, interruptible rendering, and
    work that pauses when invisible (`C01`, `C02`). No row above covers what
    happens when a render is expensive.
11. Reconciliation beyond the screen — local-first sync, server-interactive UI,
    and device desired-state (`C32`, `C33`, `C84`) — absent, and each is the
    framework's core idea applied somewhere new.
12. Event-driven and durable execution (`C44`, `C45`) — the serverless track is
    request-shaped only.
13. Surfaces beyond the main window and the product services around them
    (`C49`–`C54`) — widgets, extensions, push, commerce, flags, secure storage.
14. Seven structural decisions that belong in Tier 0 because they fix how
    every backend is written: the typed environment (`C15`), the command model
    (`C20`), adaptive-layout vocabulary (`C22`), per-property native mappers
    (`C24`), the surface vocabulary (`C49-1`), platform-group crates (`C65`),
    and the shape of capability grants (`C68`).

**Where a spelling was missing rather than a capability.** Two mechanisms score
**Met** above and would still lose a comparison, because what is missing is what
a developer types rather than what the framework can do: the tree had one syntax
of two (`X-L3-8`, Milestone 53), and style resolution had one spelling of two
(`X-L3-12`, Milestone 58). Both are cheap while the corpus of
examples, templates, and components is small, and both get steadily more
expensive afterwards, which is why they are scheduled in Tier 0 rather than with
the application layer they superficially resemble.

Those fourteen, the two missing spellings, and the conformance suites that turn
our root-layer advantages from claims into tested guarantees are what
[`gap-plan.md`](gap-plan.md) schedules.
