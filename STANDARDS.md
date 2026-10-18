# RustNative Engineering Standards

This document sets the bar every contribution to RustNative has to clear,
whether it comes from a maintainer, an outside contributor, or a tool. It
covers 31 domains, from architecture to governance. Each domain lists its
standards, and each standard says why it exists and what enforces it.

The standards were derived by a question loop run against every file in the
repository. The loop asked of each domain: what is it, why does it matter,
does it cover enough, is there more to explore, how is it enforced, why that
enforcement, is it enough, and is there a better way. The loop then started
again with everything the last pass found, and stopped only when a full pass
found nothing new. `docs/policy/standards-derivation.md` records the loop.

Related documents:

- `CONTRIBUTING.md`: the commands to run and the framework's design rules.
- `PLAN.md`, section 2: the architectural principles these standards protect.
- `SECURITY.md`: how to report a vulnerability.
- `docs/guarantees.md`: every guarantee and the test that holds it.

If this document disagrees with `PLAN.md` section 2, the plan wins, and the
disagreement is a defect to fix here.

---

## Contents

0. [How to read this document](#0-how-to-read-this-document)
1. [The domains](#1-the-domains)
2. [Architecture and boundaries (ARC)](#2-architecture-and-boundaries-arc)
3. [API design and ergonomics (API)](#3-api-design-and-ergonomics-api)
4. [Compatibility, versioning, and stability of contracts (CMP)](#4-compatibility-versioning-and-stability-of-contracts-cmp)
5. [Syntax and style equivalence (EQV)](#5-syntax-and-style-equivalence-eqv)
6. [Native fidelity and capability honesty (NAT)](#6-native-fidelity-and-capability-honesty-nat)
7. [Correctness and determinism (COR)](#7-correctness-and-determinism-cor)
8. [Stability: panics, boundaries, teardown (STB)](#8-stability-panics-boundaries-teardown-stb)
9. [Reliability and resilience (REL)](#9-reliability-and-resilience-rel)
10. [Error handling (ERR)](#10-error-handling-err)
11. [Memory safety, unsafe code, and FFI (MEM)](#11-memory-safety-unsafe-code-and-ffi-mem)
12. [Resource and lifetime management (RES)](#12-resource-and-lifetime-management-res)
13. [Concurrency and async (CON)](#13-concurrency-and-async-con)
14. [Data integrity and persistence (DAT)](#14-data-integrity-and-persistence-dat)
15. [Security (SEC)](#15-security-sec)
16. [Privacy and data protection (PRV)](#16-privacy-and-data-protection-prv)
17. [Supply chain and build integrity (SUP)](#17-supply-chain-and-build-integrity-sup)
18. [Performance and resource footprint (PRF)](#18-performance-and-resource-footprint-prf)
19. [Accessibility (A11Y)](#19-accessibility-a11y)
20. [Internationalization, localization, text, and input (I18N)](#20-internationalization-localization-text-and-input-i18n)
21. [Visual and interaction quality (UX)](#21-visual-and-interaction-quality-ux)
22. [Developer experience (DX)](#22-developer-experience-dx)
23. [Documentation and examples (DOC)](#23-documentation-and-examples-doc)
24. [Observability and debuggability (OBS)](#24-observability-and-debuggability-obs)
25. [Testing and verification (TST)](#25-testing-and-verification-tst)
26. [Code quality, style, and maintainability (QLT)](#26-code-quality-style-and-maintainability-qlt)
27. [Version control and contribution workflow (VCS)](#27-version-control-and-contribution-workflow-vcs)
28. [CI and the build system (CI)](#28-ci-and-the-build-system-ci)
29. [Release, distribution, and operations (RLS)](#29-release-distribution-and-operations-rls)
30. [Portability, interoperability, and extensibility (PRT)](#30-portability-interoperability-and-extensibility-prt)
31. [Legal, licensing, and regulatory compliance (LEG)](#31-legal-licensing-and-regulatory-compliance-leg)
32. [Governance, community, and conduct (GOV)](#32-governance-community-and-conduct-gov)
33. [The contribution checklist](#33-the-contribution-checklist)
34. [Waivers](#34-waivers)
35. [Enforcement owed](#35-enforcement-owed)
36. [Changing these standards](#36-changing-these-standards)

---

## 0. How to read this document

### Normative words

**MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** are used as
RFC 2119 and RFC 8174 define them. A MUST cannot be broken without a waiver
(section 34). Breaking a SHOULD needs a reason written in the change, where
the reviewer can see it.

### Identifiers

Each standard has a stable ID, such as `SEC-4`. Cite it in review comments,
commit messages, waivers, and `#[allow]` reasons, for example
`reason = "MEM-3: …"`. An ID is never reused. A retired standard keeps its
number and is marked *retired*.

### The enforcement ladder

Every standard names its enforcement. Mechanisms are ranked: a higher rung
is more reliable because it needs nobody to remember anything.

| Rung | Mechanism | Example |
|---|---|---|
| **E1** | **Unrepresentable.** The type system makes the violation impossible to write. | `NativeHandle<Live>`, `Secret<T>`, route access as a type |
| **E2** | **Compiler or lint, denied.** The build fails. | `undocumented_unsafe_blocks = "deny"`, `-D warnings` |
| **E3** | **Automated test or CI gate.** The suite fails. | `doc_claims.rs`, `invalidation.rs`, `cargo deny` |
| **E4** | **Generated artifact, diffed.** CI fails when the committed artifact and the regenerated one differ. | `docs/api/framework.json` and `describe.rs` |
| **E5** | **Required review.** A named owner must approve against a checklist. | CODEOWNERS on `unsafe`, security, CI |
| **E6** | **Process.** A recorded human activity. | A screen-reader pass, a disclosure timeline |

Four rules govern the ladder:

1. **Highest feasible rung.** A standard is enforced on the highest rung
   that can hold it. A standard held only at E5 or E6 says why no higher
   rung can hold it.
2. **Ratchet.** Enforcement never moves down the ladder. A lint promoted to
   `deny` stays denied. A test once required is not made optional.
3. **No claim without a holder.** This follows `PLAN.md` Milestone 41's
   rule: a guarantee with no named test is not made. A standard whose
   enforcement does not exist yet says so with **[Owed]**. Contributors
   still follow an owed standard, and reviewers check it by hand until the
   enforcement exists.
4. **A violation found by a person becomes a check.** When a reviewer
   catches the same class of defect twice, the second catch includes, or
   opens an issue for, the check that would have caught it.

### Status tags

| Tag | Meaning |
|---|---|
| **[Enforced]** | A mechanism in the repository fails the build or the suite today. |
| **[Partial]** | Enforced for some crates, targets, or cases. The gap is named. |
| **[Owed]** | The standard applies now, but its mechanism is not built. It is listed in section 35. |
| **[Review]** | By its nature, held by review or process (E5 or E6). |

---

## 1. The domains

The full list of aspects a native, cross-platform application framework
with server, web, sync, durable-execution, and tooling components has to
hold, grouped into the domains below.

| # | Domain | Prefix | What it includes |
|---|---|---|---|
| 2 | Architecture and boundaries | ARC | core/platform separation, dependency direction, platform groups, single responsibility, global state |
| 3 | API design and ergonomics | API | naming, typestate, sealing, builders, defaults, `must_use`, conversion traits |
| 4 | Compatibility and versioning | CMP | semver, deprecation, MSRV, wire and persisted formats, host OS, browser, and API-level support |
| 5 | Syntax and style equivalence | EQV | builder = markup, typed style = utility = declaration |
| 6 | Native fidelity and capability honesty | NAT | real host controls, "decorate the box", capability tables, cross-platform parity |
| 7 | Correctness and determinism | COR | reconciliation, layout, invalidation, numerics, time, randomness, identity |
| 8 | Stability | STB | panic policy, error boundaries, teardown, crash capture |
| 9 | Reliability and resilience | REL | failure modes, timeouts, retries, idempotency, offline, degraded modes, network |
| 10 | Error handling | ERR | typed errors, messages, `# Errors` docs, context |
| 11 | Memory safety, unsafe, FFI | MEM | `unsafe` containment, safety comments, FFI ownership, JNI/COM/GObject/ObjC rules |
| 12 | Resource and lifetime management | RES | native objects, handles, file descriptors, leaks, census baselines |
| 13 | Concurrency and async | CON | thread affinity, task scopes, cancellation, reentrancy, blocking |
| 14 | Data integrity and persistence | DAT | migrations, state restore, CRDT convergence, durable workflows, schema |
| 15 | Security | SEC | secure defaults, untrusted input, resource limits, crypto, authz, injection, code loading, secrets |
| 16 | Privacy | PRV | consent, telemetry, redaction, data minimization, platform privacy manifests |
| 17 | Supply chain and build integrity | SUP | dependencies, licenses, vendoring, CI hardening, provenance, reproducibility, signing |
| 18 | Performance and resource footprint | PRF | budgets, latency, memory, artifact size, energy, build time, scalability |
| 19 | Accessibility | A11Y | host accessibility trees, roles, names, focus, live regions, contrast, assistive-technology passes |
| 20 | Internationalization and text | I18N | locales, plurals, RTL, bidi, IME, complex scripts, pseudo-localization |
| 21 | Visual and interaction quality | UX | theming, DPI, high contrast, reduced motion, adaptive layout, platform idioms |
| 22 | Developer experience | DX | CLI, dev loop, compile-time diagnostics, onboarding, configuration, Cargo features |
| 23 | Documentation and examples | DOC | rustdoc, guides, both syntaxes, claims backed by budgets, examples, status honesty |
| 24 | Observability | OBS | tracing, metrics, logs, inspection protocol, diagnostics output |
| 25 | Testing and verification | TST | test levels, conformance, property, fuzzing, goldens, determinism, flakiness, coverage, hardware |
| 26 | Code quality and maintainability | QLT | formatting, lints, naming, comments, module size, dead code, generated code, source hygiene, non-Rust languages |
| 27 | Version control and contribution workflow | VCS | commits, PRs, review, ownership, generated and binary files |
| 28 | CI and the build system | CI | gates, matrices, caching, timeouts, toolchains, cross-compilation |
| 29 | Release, distribution, and operations | RLS | release notes, codemods, packaging, signing, updates, rollouts, server operations |
| 30 | Portability, interoperability, extensibility | PRT | targets, `no_std`, embedding, adoption ladder, capability packages, mappers |
| 31 | Legal and regulatory | LEG | licenses, DCO, vendored attribution, trademarks, CRA, accessibility law, data protection, store policies |
| 32 | Governance and community | GOV | conduct, roles, decision-making, RFCs, triage, AI-assisted contributions, status honesty, sustainability |

---

## 2. Architecture and boundaries (ARC)

The architecture is the framework's most expensive asset to repair. One
platform type leaking into the core breaks the promise every backend
depends on.

**ARC-1 — The core is platform-free.** `rustnative-core` MUST NOT depend on
an operating-system, toolkit, browser, terminal, or hardware API, directly
or transitively, and MUST NOT contain `cfg(target_os)`, `cfg(windows)`,
`cfg(unix)`, or similar host conditionals (`PLAN.md` 2.4). When a host needs
something the core's contracts cannot express, the contract is widened
portably.
- *Why:* every backend and every portable test stands on this.
- *Enforcement:* E3. A test over `cargo metadata` that fails when the core's
  dependency closure gains a platform crate, plus a source scan for host
  `cfg`s in `crates/rustnative-core`. **[Owed]** (`PLAN.md` Milestone 65,
  `C129`). Today it is held by review.

**ARC-2 — A platform is named in one place.** Only `rustnative-host` chooses
a backend. Applications, examples, templates, and capability packages ask
for capabilities and services, never for a platform.
- *Enforcement:* E3, the dependency-graph test of ARC-1, extended so that no
  crate except `rustnative-host`, the backends' own examples, and adoption
  examples depends on a backend crate. **[Owed]**

**ARC-3 — A backend never depends on another backend.** Code shared between
backends lives in a platform-group crate (`docs/conformance/platform-groups.md`).
The group decision is recorded before the group's second member is written.
- *Enforcement:* E3, the same graph test. **[Owed]** Group decisions:
  E5, CODEOWNERS on `docs/conformance/platform-groups.md`.

**ARC-4 — Dependency direction is one-way.** The allowed order is
types → core → style and markup → group crates → backends → host →
applications. A lower layer never names a higher one. Cycles are
impossible in Cargo. Layering violations are not, and the graph test of
ARC-1 rejects them.
- *Enforcement:* E3 **[Owed]**.

**ARC-5 — Capabilities, not platform checks.** Portable code decides by
`Capability`, never by asking which platform it runs on (`PLAN.md` 2.5). A
capability that only one backend's own type can realize is a missing
portable contract, and it is added to `rustnative_core::services` first.
- *Enforcement:* E1, since the portable API exposes no platform enum.
  E5 for new services.

**ARC-6 — Do not tell targets apart by `cfg` when they share a compiler
target.** iOS and iPadOS share `target_os = "ios"`. Such targets are chosen
once at launch, and downstream code asks the backend it was handed.
- *Enforcement:* E5 **[Review]**. A `cfg` can express the wrong thing
  without any tool seeing it.

**ARC-7 — One ownership module per backend.** All host-object conventions
of a backend are stated in one module, as
`rustnative-windows/src/native/ownership.rs` does, and each convention
names the test that asserts it.
- *Enforcement:* E3, the per-backend lifetime tests. E5 for the module
  itself. **[Partial]**

**ARC-8 — No global mutable state on the render path.** Reconciliation,
layout, style resolution, and realization take their state from their
owners. Process-wide statics (`OnceLock`, `LazyLock`, `thread_local!`) are
allowed only for host-mandated singletons (window classes, the JVM, a COM
apartment) and immutable tables. Each such static carries a comment naming
why it must be global.
- *Why:* hidden global state defeats deterministic tests and multi-window,
  multi-application, and embedding use.
- *Enforcement:* E3, a scan that lists statics in render-path modules and
  fails on unlisted ones. **[Owed]** E5 today.

**ARC-9 — Responsibilities are bounded.** A module owns one domain concept.
A type has one reason to change. A source file over 2,000 lines SHOULD be
split along a responsibility seam. Splitting just to cut the line count does
not count, so split only where a seam exists. At the time of writing three
files exceed it (`rustnative-webgen/src/methods.rs`,
`rustnative-tui/src/session.rs`, `rustnative-core/src/node.rs`).
- *Enforcement:* `clippy::too_many_lines` at 200 lines per function
  (`clippy.toml`) **[Enforced]**. File size: E3 report, warning only.
  **[Owed]**

**ARC-10 — Escape hatches are explicit and scoped.** Native access is
through a documented, typed escape hatch (`NativeHandle<Unchecked>` →
`validate` → `NativeHandle<Live>`) scoped to a node, never ambient, and
never imported into portable crates (`PLAN.md` 2.6).
- *Enforcement:* E1 **[Enforced]**.

**ARC-11 — Decisions are written down before they are coded.** A change
that alters a principle of `PLAN.md` section 2, adds a crate, adds a
platform group, or adds a public protocol needs a written design (an RFC,
see GOV-5), accepted before the code is reviewed.
- *Enforcement:* E5/E6 **[Review]**.

---

## 3. API design and ergonomics (API)

The public API is the product. Everything else is implementation.

**API-1 — Make invalid states unrepresentable.** When states permit
different operations, the states are types (`docs/conformance/typestate.md`).
When validity depends on runtime data, the function returns `Result`.
Prefer enums over boolean pairs, newtypes over bare integers for
identifiers, and constructors that validate over public fields.
- *Enforcement:* E5 with the typestate guide. **[Review]**

**API-2 — Public items are documented and exemplified.** Every public item
has rustdoc. Every major entry point has a runnable example. Fallible
functions have `# Errors`, panicking ones have `# Panics`, and `unsafe fn`
has `# Safety`.
- *Enforcement:* E2. `missing_docs` is denied in 19 of 28 library crates.
  It is missing from `rustnative-components`, `-data`, `-durable`, `-host`,
  `-observe`, `-server`, `-server-macros`, `-sync`, and `-update`, despite
  `docs/api/README.md` saying every crate denies it. The fix is
  `missing_docs = "deny"` in `[workspace.lints.rust]`, so no crate can be
  left out. `clippy::missing_errors_doc` and `missing_panics_doc` are
  warnings, denied in CI by `-D warnings`. **[Partial]**

**API-3 — Follow the Rust API Guidelines.** Naming (`as_`/`to_`/`into_`,
`with_*` modifiers, `try_*` for fallible variants), standard traits
(`Debug` on every public type, `Clone`/`PartialEq`/`Eq`/`Hash`/`Default`
where meaningful), conversions through `From`/`TryFrom`, and iterators
over index loops in the API.
- *Enforcement:* E2 through `clippy::pedantic` **[Enforced]**, plus
  `missing_debug_implementations` **[Owed]**, plus E5.

**API-4 — Design for additive evolution.** Public enums that may grow are
`#[non_exhaustive]`. Traits implemented only by the framework are sealed.
Structs that may gain fields keep private fields and a constructor or
builder. Handles are opaque. These choices are written in a sealing policy
(`PLAN.md` Milestone 65, `C128`).
- *Enforcement:* E4 with the API snapshot (CMP-2) **[Owed]**. E5 today.

**API-5 — Results that matter cannot be ignored.** Functions whose return
value carries the effect (builders, `Result`, handles, guards) are
`#[must_use]`.
- *Enforcement:* E2, `clippy::must_use_candidate` (pedantic) and
  `unused_must_use` **[Enforced]**.

**API-6 — Defaults are the safe and common choice.** A default configuration
is secure (SEC-1), accessible (A11Y-1), private (PRV-1), and correct for the
common case. An option that weakens one of these is named for what it does
(`csrf_exempt`, `allow_insecure_…`) and documented with its risk.
- *Enforcement:* E3, the server security-default tests **[Partial]**. E5.

**API-7 — No surprise allocation, blocking, or I/O.** A function that
blocks, performs I/O, or allocates in proportion to its input says so in its
name or its docs. The UI thread never calls such a function (CON-4).
- *Enforcement:* E5 **[Review]**.

**API-8 — Generic over concrete only when the caller gains.** Each generic
parameter, trait object, or `async-trait` use has a reason the caller can
see. Monomorphization cost counts against the artifact-size budgets (PRF-3).
- *Enforcement:* E5, and E3 through the budgets **[Partial]**.

**API-9 — One way to do a thing.** A new API that duplicates an existing
one either replaces it, with a deprecation (CMP-3), or is refused. The two
syntaxes of EQV are the deliberate exception, and they produce one tree.
- *Enforcement:* E5 **[Review]**.

---

## 4. Compatibility, versioning, and stability of contracts (CMP)

The stability policy is `docs/policy/stability.md`. These standards make it
binding on each change.

**CMP-1 — Semantic versioning across every contract.** The covered surface
is every published crate's public API, the markup syntax, the style
spellings, `rustnative.toml`, the `rustnative` command line, the inspection
protocol, and the wire formats. Before 1.0 a minor release may break the
surface. A patch release never does.
- *Enforcement:* E4 via CMP-2 **[Owed]**.

**CMP-2 — The public API is a committed artifact.** A snapshot of each
crate's public items, next to `docs/api/framework.json`, is regenerated in
CI. A diff larger than the declared version bump allows fails the build.
`cargo-semver-checks` (242 lints at the end of 2025) runs on every published
crate.
- *Why:* a breaking change must be impossible to land by accident, not
  merely discouraged.
- *Enforcement:* E4 **[Owed]** (`PLAN.md` Milestone 65, `C128`).
  `framework.json` staleness is **[Enforced]** by
  `crates/rustnative/tests/describe.rs`.

**CMP-3 — Deprecate before removing.** An item marked for removal is
`#[deprecated(since, note)]`, names its replacement, and stays for at least
two minor releases. The markup and style compilers warn on deprecated
elements, attributes, and classes the same way.
- *Enforcement:* E2 (`deprecated` warns, `-D warnings` in CI makes internal
  uses fail) **[Enforced]**. Window length: E5.

**CMP-4 — Every breaking change ships a codemod.** `rustnative upgrade`
carries a codemod with corpus cases in
`crates/rustnative/tests/codemod-corpus/`. It is idempotent and reports
every position it cannot rewrite with certainty. A breaking change with no
codemod is a release blocker unless no rewrite is possible.
- *Enforcement:* E3, the corpus tests **[Enforced]**. Presence of a codemod
  for each break: E5 **[Review]**, E4 once CMP-2 exists.

**CMP-5 — MSRV is declared, checked, and raised deliberately.**
`rust-version` (now 1.85) is checked on every change. Raising it is a minor
release before 1.0 and a major one after, and is stated in the release
notes. Code SHOULD build on the newest stable without new warnings, because
CI runs the newest stable.
- *Enforcement:* E3, the `msrv` CI job **[Enforced]**.

**CMP-6 — Wire and persisted formats are versioned.** Every format that
crosses a process, a device, or time carries a version: the sync and
inspection protocols, server-function calls, saved application state,
durable workflow histories, update manifests, and migrations. Readers accept
every version still in the support window. An old reader meeting a newer
version fails with a typed error, never with a misparse.
- *Enforcement:* E3. Golden fixtures for each supported version, decoded by
  the current reader. **[Partial]**: `wire.rs` conformance tests exist. A
  fixture for every format and version is **[Owed]**.

**CMP-7 — Host support is a table, not a hope.** Each backend states the
oldest OS, toolkit, API level, and browser it supports
(`docs/linux/desktop-matrix.md` is the model). Dropping one is a minor
release with a release note. Using an API newer than the floor requires a
runtime check and a fallback or a capability answer.
- *Enforcement:* E3, CI on the floor where a runner exists (Android
  API levels, Linux distributions) **[Partial]**. E5 elsewhere.

**CMP-8 — Capability packages declare their framework range.** A package
outside its range fails to install with `PackageError::Framework`, never at
run time.
- *Enforcement:* E1/E3 **[Enforced]**.

---

## 5. Syntax and style equivalence (EQV)

These rules come from `PLAN.md` 2.9 and 2.14. They keep the framework's two
syntaxes and its style spellings from drifting apart.

**EQV-1 — A node kind or modifier lands in both syntaxes at once.** It lands
with its builder form, its markup element or attribute, an entry in
`rustnative-markup/src/table.rs`, and an equivalence case asserting equal
`Node` values, with the markup compiled from a `.rsx` file and through `rsx!`.
- *Enforcement:* E3, `rustnative-conformance/tests/syntax_equivalence.rs`,
  and E4, `framework.json` **[Enforced]**. Detecting a builder method with
  no attribute: **[Partial]**.

**EQV-2 — Markup emits only builder calls.** No node kind, runtime type,
allocation, or behaviour exists on one side only.
- *Enforcement:* E3, expansion goldens
  (`rustnative-markup/tests/expansion_goldens.rs`) **[Enforced]**.

**EQV-3 — `.rsx` only wraps.** `compile_rsx()` finds markup and wraps it in
`rsx!`. It never parses or lowers markup itself and never accepts what the
macro rejects.
- *Enforcement:* E3, the `.rsx` compiler tests **[Enforced]**.

**EQV-4 — A style property lands with every spelling and every backend's
answer.** That means its typed form, its declaration name, its utility
spelling or a documented note that it has none, an equivalence case, and
each backend's answer (realized, approximated, or unavailable), given
separately for a native control and a framework-owned box. An unavailable
style is a build-time diagnostic, never a silent no-op.
- *Enforcement:* E3, `style_equivalence.rs`, the compile-fail suites
  `style_ui` and `style_ui_windows`, and each backend's capability-table
  test **[Enforced]**.

**EQV-5 — No selectors, no second engine.** A style that depends on another
node is a typed relationship resolved by a bounded lookup. A variant needing
a tree search is refused.
- *Enforcement:* E1, since the vocabulary has no selector type, plus E5
  **[Enforced]**.

**EQV-6 — Diagnostics point at what the developer wrote.** A markup or style
error is reported at the element, attribute, or class written in the `.rsx`
file or the `rsx!` call, at a compiler's level of quality.
- *Enforcement:* E3, trybuild `.stderr` goldens in
  `rustnative-conformance/tests/ui` **[Enforced]**. A new diagnostic adds a
  case.

**EQV-7 — Documentation shows both.** Every tree in prose appears in both
syntaxes, labelled and side by side, and every styled example shows both
spellings, unless an exemption comment gives the reason.
- *Enforcement:* E3, `rustnative-conformance/tests/doc_parity.rs`
  **[Enforced]** for `README.md`, `PLAN.md`, `docs/guides/`, and runnable
  doc examples. Extending it to every `docs/` page: **[Owed]**.

---

## 6. Native fidelity and capability honesty (NAT)

**NAT-1 — Real host controls.** Wherever the host has a native equivalent,
the node is realized as that control and keeps its focus visuals, text
editing, input method, and accessibility (`PLAN.md` 2.2).
- *Enforcement:* E3, fidelity tests (`controls_are_the_systems_own_classes`
  and the per-backend equivalents) **[Enforced]** for Windows, Linux,
  Android, Web, and terminal.

**NAT-2 — Decorate the box, never the control.** No backend captures,
redraws, or owner-draws a native control to satisfy a style. A style that
would need it is answered unavailable for native controls.
- *Enforcement:* E5 **[Review]**, plus conformance row 25 once Milestone 67
  lands **[Owed]**.

**NAT-3 — A capability is advertised only once it is realized.** Never
because the plan says it will be (`PLAN.md` 2.13).
- *Enforcement:* E3, per backend:
  `capabilities_only_advertise_realized_backend_features` and the
  equivalents **[Enforced]**.

**NAT-4 — A backend is complete only when its conformance column is.** Every
row of `docs/conformance/new-backend-checklist.md` has a test or an honest
answer (n/a, or a refused capability). A row with no test is not satisfied.
- *Enforcement:* E3 per row, E5 for the table **[Enforced]**.

**NAT-5 — Every backend is written against its own host's primitives.**
There are no emulated window managers, canvas facsimiles of OS widgets, or
desktop assumptions on mobile, web, terminal, or embedded targets
(`PLAN.md` 2.3).
- *Enforcement:* E5 **[Review]**.

**NAT-6 — Platform idioms are documented and honoured.** Each backend
documents its host's conventions in `docs/idioms/<host>.md`: menu placement,
dialog button order, shortcuts, back navigation, window chrome. A component
honours them by default.
- *Enforcement:* E3 where testable (command and shortcut routing), E5
  elsewhere **[Partial]**. `docs/idioms/` holds Windows only; other hosts
  are **[Owed]**.

---

## 7. Correctness and determinism (COR)

**COR-1 — The invalidation contract is exact.** Exactly the components a
change concerns re-render (`docs/invalidation.md`), and the render log
proves it.
- *Enforcement:* E3, `rustnative-core/tests/invalidation.rs` **[Enforced]**.
  A change to invalidation updates the table and the test together.

**COR-2 — Identity is stable and scoped.** Component-local keys; platform
node IDs are runtime-scoped and opaque. Native objects are reused while
identity holds. Duplicate keys are a structured error, not a panic.
- *Enforcement:* E3, proptest identity and reconciliation properties
  (`rustnative-core/tests/property_tests.rs`) **[Enforced]**.

**COR-3 — Layout is deterministic and platform-independent.** The same tree,
constraints, and measurements give the same geometry on every backend. A
backend converts units at its own boundary with one documented rounding
rule (half away from zero), and reports measurements in that unit
(`PLAN.md` 2.11).
- *Enforcement:* E3, layout property tests, `layout_conformance.rs`, and the
  per-backend unit-mapping tests **[Enforced]**.

**COR-4 — Numeric conversions never wrap silently.** Casts that can
truncate, wrap, lose sign, or lose precision are denied. Every remaining
cast is preceded by a range-establishing `min`/`max`/`clamp`/`saturating_*`
and carries an `#[allow(…, reason)]`. Use `try_from` where the range is not
established. Identifier counters fail rather than wrap.
- *Enforcement:* E2, `cast_possible_truncation`, `cast_possible_wrap`,
  `cast_sign_loss`, and `cast_precision_loss` are denied **[Enforced]**.
  Overflow in debug builds: E3. Arithmetic on geometry: saturating by
  convention, E5.

**COR-5 — Time comes from a `Clock`.** No portable code reads the system
clock directly. Tests use the deterministic clock and executor. Timeouts and
backoff are expressed in clock time.
- *Enforcement:* E1, through the `Clock` and `Executor` contracts, plus a
  source scan for `Instant::now()`/`SystemTime::now()` outside backends and
  clock implementations **[Owed]**.

**COR-6 — Randomness is explicit.** Security randomness comes from the OS
CSPRNG (`getrandom`). Non-security randomness is seeded and injectable so
tests replay. Nothing on the render path is random.
- *Enforcement:* E5, plus a scan forbidding non-CSPRNG sources in
  `rustnative-server/src/auth` and `security.rs` **[Owed]**.

**COR-7 — Iteration order that reaches output is defined.** Anything that
reaches output (markup, generated CSS or JS, artifacts, the SBOM, goldens)
iterates in a defined order: `BTreeMap`, sorted vectors, or insertion order.
`HashMap` iteration never reaches output.
- *Why:* reproducible builds (SUP-7) and stable goldens depend on it.
- *Enforcement:* E3, goldens and the reproducibility check **[Partial]**.

**COR-8 — Floating point is handled deliberately.** Comparisons use a stated
tolerance or total ordering (`total_cmp`). NaN cannot enter layout
(constructors reject or clamp it). Colour and unit conversions are tested at
their boundaries.
- *Enforcement:* E2, `clippy::float_cmp` (pedantic) **[Enforced]**, plus E3
  conversion tests.

**COR-9 — Merge functions converge.** Every CRDT and sync merge is
commutative, associative, and idempotent. Proptest proves it for every type
that merges.
- *Enforcement:* E3, `rustnative-sync/tests/crdt.rs` with proptest
  **[Enforced]** for existing types. A new mergeable type adds its property.

---

## 8. Stability: panics, boundaries, teardown (STB)

A framework runs inside someone else's process. A panic in framework code is
a crash the application did not write and cannot catch.

**STB-1 — Library code does not panic on reachable paths.** `unwrap`,
`expect`, `panic!`, `unreachable!`, and indexing that can fail are not used
on paths that inputs, the host, or timing can reach. The one exception is an
internal invariant the runtime just established. It carries
`#[allow(clippy::expect_used, reason = "…the invariant…")]` with the
invariant written out.
- *Enforcement:* E2, `unwrap_used` and `expect_used` warn, and CI's
  `-D warnings` denies them. Tests are exempt (`clippy.toml`).
  **[Enforced]** `clippy::panic` and `indexing_slicing` are currently
  allowed. Promoting `panic` to warn, with each remaining use given a
  reason, is **[Owed]**.

**STB-2 — Panics are contained at every host boundary.** Every callback the
host invokes catches unwinding before it reaches foreign frames: a window
procedure, a JNI native, a GObject signal, a COM method, a WASI export, or a
signal handler. A panic in a component is routed to its error boundary or
ends the application cleanly. It never aborts in the middle of a foreign
frame and never leaves the host broken.
- *Enforcement:* E3, conformance row 10 per backend: a deliberate panic
  restores capture, cursor, terminal, and JVM **[Enforced]**. A new host
  entry point adds its case.

**STB-3 — Teardown restores the host.** On exit, panic, or signal, the
framework releases grabs, capture, cursor clips, terminal modes, and
foreign state, so the host is left as it was found. On embedded targets
(when they land) the hardware is left in a safe state.
- *Enforcement:* E3, the teardown tests per backend **[Enforced]**.

**STB-4 — Error boundaries isolate subtrees.** A failing component is
contained by the nearest boundary under its supervision policy. A failure
with no boundary reaches the application, never the process.
- *Enforcement:* E3, `rustnative-core/tests/state_resilience.rs`
  **[Enforced]**.

**STB-5 — Crashes leave evidence, locally.** Crash capture records enough to
diagnose. The data stays on the machine unless the application, with
consent, sends it (PRV-3).
- *Enforcement:* E3, the crash tests per backend **[Partial]**.

**STB-6 — Process exit belongs to the application.** Library code never
calls `std::process::exit` or `abort` except in an explicitly documented
fatal path (a second signal while stuck). Examples MAY exit.
- *Enforcement:* E3, a source scan over `crates/` **[Owed]**. At the time of
  writing no library crate calls it.

**STB-7 — Release health gates rollouts.** Crash-free sessions, startup
failures, hangs, and frame drops have declared thresholds. A staged rollout
halts on a breach and reverts on a severe one (`PLAN.md` Milestone 65,
`C132`).
- *Enforcement:* E3/E6 **[Owed]**.

---

## 9. Reliability and resilience (REL)

**REL-1 — Every wait has a bound.** Network calls, IPC, child processes,
locks across processes, emulator and device steps, and browser automation
all have a timeout. The timeout is chosen, documented, and surfaced as a
typed error.
- *Enforcement:* E5, plus a scan for network constructors without a timeout
  **[Owed]**. CI jobs: CI-5.

**REL-2 — Retries are bounded, backed off, and safe.** Retries use bounded
attempts, exponential backoff with jitter, and retry only operations that
are idempotent or carry an idempotency key.
- *Enforcement:* E3, durable-execution and sync tests **[Partial]**.

**REL-3 — Effects that must happen once are idempotent or transactional.**
Background jobs, queued offline calls, durable workflow steps, and
server functions that change state take idempotency keys or run as
exactly-once transactional steps. Poison messages go to a dead-letter
queue.
- *Enforcement:* E3, `rustnative-durable/tests/durable.rs`, the offline
  replay tests **[Enforced]** for existing paths.

**REL-4 — Degrade, don't fail, when a capability is missing.** A component
that uses an optional capability checks for it and has a defined fallback.
A missing capability never crashes and never no-ops silently.
- *Enforcement:* E3, headless tests with capabilities withheld
  **[Partial]**.

**REL-5 — Offline and constrained networks are first-class.** Code that
talks to a network states its behaviour when offline, on high latency, and
on metered connections. Queued work survives a restart.
- *Enforcement:* E3, `offline_browser.rs` and sync tests **[Partial]**.

**REL-6 — Lifecycle events are honoured.** Suspend, resume, low memory,
process death and restore, configuration change, and window close each have
a defined path. State is restorable after process death on the hosts that
kill processes.
- *Enforcement:* E3, `lifecycle_conformance.rs`,
  `tools/android-lifecycle-test.sh` **[Enforced]**.

**REL-7 — Untrusted peers cannot exhaust a server.** Every listener the
framework ships (the HTTP server, the MQTT broker, sync and live sessions,
inspection, emulators) bounds connections, request and message size,
handshake time, and idle time. It allocates only after the size is checked
against the bound. See SEC-6.
- *Finding at adoption:* `rustnative-sync/src/mqtt.rs` reads the remaining
  length (up to 268,435,455 bytes) and allocates `vec![0u8; length]` before
  any authentication, with no connection cap and no timeout on the first
  packet.
- *Enforcement:* E3, a limits test per listener **[Partial]**: the HTTP
  server has one (`clients_are_rate_limited_and_bodies_bounded`).

---

## 10. Error handling (ERR)

**ERR-1 — Errors are typed.** Public fallible functions return a crate error
type (an enum, `#[non_exhaustive]`, implementing `std::error::Error` and
`Display`, with `source()` chaining). `Result<_, String>` and
`Box<dyn Error>` are not used in public APIs. At adoption there are 134
`Result<_, String>` signatures outside tests, concentrated in
`rustnative-style`, `rustnative-web/src/capability.rs`,
`rustnative-server/src/deploy.rs`, and `rustnative-sync`.
- *Why:* callers must be able to match on failures. A string can only be
  displayed.
- *Enforcement:* E3, a scan that fails on new `Result<…, String>` in public
  signatures, with a burn-down list for the existing ones **[Owed]**.

**ERR-2 — Messages say what, where, and what to do.** An error message
names the operation, the input or resource, and the remedy, without
secrets (SEC-9). Developer-facing diagnostics (CLI, build, macros) name the
file and position, the rule broken, and the fix.
- *Enforcement:* E3, CLI and trybuild golden outputs **[Partial]**. E5.

**ERR-3 — Errors are not swallowed.** `let _ =` on a `Result`, `.ok()` that
discards, and an empty `Err(_) => {}` are allowed only with a comment saying
why the failure does not matter. Best-effort cleanup in `Drop` is the common
legitimate case.
- *Enforcement:* E2, `let_underscore_must_use` and
  `clippy::let_underscore_untyped` **[Owed]**. E5 today.

**ERR-4 — Fail at the boundary, once.** Validate input where it enters
(parse, don't validate, and keep the validated type). Inner code takes the
validated type and does not re-check.
- *Enforcement:* E1 where typestate applies (`Validated<T>`). E5 elsewhere
  **[Partial]**.

---

## 11. Memory safety, unsafe code, and FFI (MEM)

At adoption the workspace holds about 970 `unsafe` blocks, most of them in
`rustnative-windows` (753), `-linux` (77), and `-tui` (72), with about 980
`SAFETY:` comments. The discipline is already strong. These standards keep
it that way.

**MEM-1 — `unsafe` lives only where the host requires it.** Backends, FFI
bridges, and named low-level modules hold it. A crate with no need for it
declares `#![forbid(unsafe_code)]`. At adoption, crates with no
`unsafe` (`rustnative-components`, `-conformance`, `-data`, `-drawlist`,
`-durable`, `-headless`, `-host`, `-i18n`, `-macros`, `-markup`,
`-observe`, `-server-macros`, `-style`, `-sync`, `-types`, `-update`,
`-web-macros`, `-web-testing`) do not yet forbid it.
- *Enforcement:* E2, `#![forbid(unsafe_code)]` per crate **[Owed]**.

**MEM-2 — Every `unsafe` block carries its proof.** A `// SAFETY:` comment
immediately above it states the invariant that makes it sound: why
pointers are valid, why lifetimes hold, which thread, and who owns the
result. "Safe because we checked" is not a proof.
- *Enforcement:* E2, `clippy::undocumented_unsafe_blocks = "deny"` and
  `unsafe_op_in_unsafe_fn = "deny"` **[Enforced]**. Proof quality: E5,
  with CODEOWNERS on backend crates **[Owed]**.

**MEM-3 — `unsafe impl Send`/`Sync` is argued, not assumed.** Each has a
`SAFETY:` comment naming the thread contract of the wrapped resource.
- *Enforcement:* E2, `clippy::non_send_fields_in_send_ty` **[Owed]**.
  E5 today.

**MEM-4 — FFI ownership is written down.** Every crossing of a foreign
boundary (Win32, COM, GObject, JNI, Objective-C, WASI imports, terminal
syscalls) states, in the backend's ownership module (ARC-7), who allocates,
who frees, reference-count rules, string encoding and termination, and
thread affinity. Raw handles cross threads only as integers handed back to
the host.
- *Enforcement:* E5, plus the lifetime tests of RES **[Partial]**.

**MEM-5 — `transmute` and pointer casts are a last resort.** Use typed
bindings, `from_raw`/`into_raw`, `cast()`, or `core::ptr` APIs first. Each
remaining `transmute` names its source and target types explicitly and
cites the documented signature it relies on. One exists at adoption, in
`rustnative-windows/src/native/accelerator.rs`.
- *Enforcement:* E2, `clippy::transmute_ptr_to_ptr`,
  `missing_transmute_annotations` (pedantic) **[Enforced]**. E5.

**MEM-6 — The unsafe surface is checked by tools, not only by eyes.** Miri
runs over the portable crates' unsafe-adjacent tests. AddressSanitizer and
ThreadSanitizer run over each backend's FFI tests where the target supports
them (`PLAN.md` Milestone 65, `C130`).
- *Enforcement:* E3 **[Owed]**.

**MEM-7 — Non-Rust bridge code holds the same bar.** The Java host library,
the generated and hand-written JavaScript runtime, C and C# adoption
hosts, and shell scripts follow QLT-10. JNI calls check and clear pending
exceptions. Every Java `catch` either rethrows, reports to Rust, or states
why the exception is safe to drop.
- *Enforcement:* E3, the Android device suite **[Partial]**. E5.

**MEM-8 — Environment mutation is single-threaded.** `std::env::set_var`
and `remove_var` (`unsafe` in edition 2024) are called only before any
thread starts, with a `SAFETY:` comment saying so. Prefer
`Command::env` for children.
- *Enforcement:* E2, edition 2024 makes them `unsafe` **[Enforced]**.

---

## 12. Resource and lifetime management (RES)

**RES-1 — Every native object has one owner and one destruction path.**
Registries own what they create and borrow what they do not. Destruction is
explicit and single, so a double destroy is structurally impossible.
- *Enforcement:* E1 by construction (no `Drop` on `NativeObject`, one
  `destroy`), plus E3 registry tests **[Enforced]** for Windows. Each
  backend's ownership module states its equivalent.

**RES-2 — Mount and unmount return to baseline.** After repeated mount and
unmount cycles, the host-object census (GDI/USER objects, widgets, views,
JNI global references, DOM elements, file descriptors) returns to its
baseline.
- *Enforcement:* E3, `suites::mount_unmount_returns_to_baseline`, the GDI
  leak gate (100 cycles), and the per-backend census tests **[Enforced]**.

**RES-3 — RAII for every acquired resource.** Handles, locks, temporary
files, child processes, threads, and subscriptions are released by a guard's
`Drop`, on every path, panics included. Threads are joined and children are
waited on.
- *Enforcement:* E3, lifetime tests **[Partial]**. E5.

**RES-4 — Caches are bounded.** Every cache (style objects, fonts, the
response cache, image decodes, glyph tables) has a size or entry bound and
an eviction policy.
- *Enforcement:* E3, unit tests per cache **[Partial]**.

**RES-5 — Stale events are rejected.** An event for a destroyed object or a
superseded generation is dropped, never delivered to a reused identity.
- *Enforcement:* E3, stale-event tests per backend **[Enforced]**.

---

## 13. Concurrency and async (CON)

**CON-1 — Thread affinity is enforced by type where possible.** Tree-holding
types are `!Send`. UI-thread APIs take `&UiThread`. Where the type system
cannot express affinity, a debug assertion does.
- *Enforcement:* E1, `rustnative-core/tests/affinity.rs` **[Enforced]**.

**CON-2 — Async work is owned by a task scope.** A task belongs to a
component's scope and is cancelled when it unmounts. No message is
delivered after unmount (`PLAN.md` 2.12).
- *Enforcement:* E3, `suites::no_message_after_unmount` and the cancellation
  property test **[Enforced]**.

**CON-3 — Workers never mutate component state.** Results return as
messages through the scheduler.
- *Enforcement:* E1, component state is not reachable from `Send` contexts
  **[Enforced]**.

**CON-4 — The UI thread never blocks.** No synchronous I/O, sleeping,
blocking channel receive, or contended lock on the UI thread. Host modal
loops keep the framework running (the modal-operation conformance row).
- *Enforcement:* E3, the responsiveness tests and budgets
  (`filter_input_latency_*`) **[Partial]**. A debug-build watchdog that
  reports UI-thread stalls is **[Owed]**.

**CON-5 — Reentrancy is designed, not discovered.** A host callback that can
re-enter the framework (a window procedure during `DestroyWindow`, a JNI
call back into Rust, a GTK signal during a property set) is identified in
code. Reentrant entry is either supported and tested or rejected with a
defined behaviour.
- *Enforcement:* E3, reentrancy tests per backend **[Partial]**.

**CON-6 — Locks are few, short, and ordered.** No lock is held across
`.await` or a host call. Where two locks nest, their order is documented.
`parking_lot` and `std` locks are not mixed in one subsystem.
- *Enforcement:* E2, `clippy::await_holding_lock` and
  `await_holding_refcell_ref` **[Enforced]** (pedantic/suspicious). Lock
  order: E5.

**CON-7 — Channels are bounded unless proven otherwise.** An unbounded
channel needs a comment showing why its producer is bounded. At adoption
there are 19 unbounded channels outside tests.
- *Enforcement:* E3, a scan with an allowlist **[Owed]**.

**CON-8 — The executor is an abstraction.** Core code uses the `Executor`,
`LocalExecutor`, and `Clock` contracts, never a specific runtime's global.
Tokio is used behind them.
- *Enforcement:* E5 **[Review]**, plus ARC-1's scan for `tokio::spawn` in the
  core **[Owed]**.

---

## 14. Data integrity and persistence (DAT)

**DAT-1 — Migrations are forward, reversible, and dry-runnable.** Each
schema migration has an up and a down step and a dry run, and is tested
against a database at the previous version.
- *Enforcement:* E3, the migration tests (`rustnative-server/src/db/migrate.rs`,
  `examples/server-demo/migrations`) **[Enforced]**.

**DAT-2 — Persisted state survives upgrades.** Saved application state,
offline queues, and durable histories written by version N are readable by
version N+1 (CMP-6). A format change ships a migration and a fixture.
- *Enforcement:* E3, fixtures **[Owed]** for each persisted format.

**DAT-3 — Writes are atomic.** Files are written to a temporary file in the
same directory, flushed, and renamed. Multi-step database changes run in one
transaction. A crash at any point leaves the old or the new state, never a
mix.
- *Enforcement:* E3, crash-injection tests in `examples/workflow-crash`
  **[Partial]**.

**DAT-4 — Queries are typed and bound.** SQL uses bound parameters only.
`query!` is checked against the schema at compile time. Identifiers that
must be interpolated come only from the model.
- *Enforcement:* E1/E2, the macro and compile-fail doc tests **[Enforced]**.

**DAT-5 — Data the person owns is portable.** Export and deletion paths
exist for personal data the framework stores (`PLAN.md` Milestone 63).
- *Enforcement:* E3 **[Partial]**.

---

## 15. Security (SEC)

The threat models are in `docs/security/` and the server checklist is in
`docs/server/security-checklist.md`. These standards apply to every
contribution, not only to security work.

**SEC-1 — Secure by default, insecure by name.** Every protection in the
server checklist is on unless an application turns it off. Shipping without
one is a defect. An opt-out is named for what it gives up.
- *Enforcement:* E3, one named test per checklist row **[Enforced]**.

**SEC-2 — Every new entry point updates the threat model.** A new listener,
file format, IPC channel, deep-link scheme, plugin surface, or service grant
adds its row (entry point, threat, mitigation, holding test) to the relevant
`docs/security/threat-model-*.md`, in the same change.
- *Enforcement:* E5, CODEOWNERS on listeners and parsers **[Owed]**.

**SEC-3 — Untrusted input is parsed defensively.** Network messages, files
the person opens, deep links, clipboard and drag data, attributes on custom
elements, environment variables, manifests, and packages are untrusted. Use
typed, length-checked parsers. Never evaluate input as code or markup.
Parse risky formats in an isolated worker where the host allows it
(`IsolatedWorker` on Windows).
- *Enforcement:* E3, fuzzing (TST-6) **[Owed]**. Isolation tests
  **[Enforced]** on Windows.

**SEC-4 — Output is escaped by construction.** HTML text and attributes are
escaped by the renderer. Raw markup is only a `&'static str` in source. JSON
in pages escapes `<`, `>`, and `&`. Shell and SQL never take interpolated
input.
- *Enforcement:* E1, `Html` types and the `query!` macro **[Enforced]**.

**SEC-5 — Authorization is in the type of every route.** A route with no
stated access (`public`, `signed_in`, `authorized::<Policy>`) does not
compile. Data reads go through row policies when declared. Server functions
re-check access and never trust client state.
- *Enforcement:* E1, compile-fail doc test **[Enforced]**.

**SEC-6 — Every resource an attacker can consume is bounded.** Request
bodies, message sizes, nesting depth, connection count, handshake time,
decompression ratio, image dimensions, and fuel and memory on edge hosts are
bounded. Size is checked before allocating. Parsers that recurse bound their
depth. At adoption no first-party parser has an explicit depth limit, and
the MQTT broker allocates before checking (REL-7).
- *Enforcement:* E3, a limits test per parser and listener, and fuzzing
  **[Partial]**.

**SEC-7 — Cryptography is boring.** Use reviewed crates (RustCrypto,
`ed25519-dalek`, `argon2`). Never implement primitives. Compare secrets in
constant time. Use AEADs with unique nonces, keys from a CSPRNG, and
algorithm choices in one module per concern. A change to crypto code needs
two maintainer approvals.
- *Enforcement:* E5, two approvals via CODEOWNERS on
  `rustnative-server/src/auth`, `security.rs`, and `rustnative-update`
  **[Owed]**. Constant-time comparison: E3 unit tests **[Enforced]**.

**SEC-8 — Code is loaded only from trusted locations.** Native libraries
are loaded by full path or with a restricted search (Windows:
`LoadLibraryExW` with `LOAD_LIBRARY_SEARCH_SYSTEM32` for system DLLs, never
bare `LoadLibraryW`). Child processes are started by absolute path or a
resolved toolchain path, never through a shell with interpolated
arguments. URLs are opened without a command interpreter.
- *Finding at adoption:* `rustnative-windows/src/native/accelerator.rs`
  loads `dxcore.dll` with `LoadLibraryW` by bare name, which follows the
  default DLL search order.
- *Enforcement:* E3, a scan forbidding `LoadLibraryW`, `LoadLibraryA`, and
  `Command::new("sh"|"cmd")` with dynamic arguments outside tests
  **[Owed]**.

**SEC-9 — Secrets never print, log, or serialize.** Secrets are
`Secret<T>`. There is no global configuration holding them. Test fixtures
use obviously fake values. Credentials never appear in URLs, span names,
query strings, panics, or error messages.
- *Enforcement:* E1 `Secret<T>` **[Enforced]**, plus E3 secret scanning in
  CI (gitleaks or GitHub secret scanning with push protection) **[Owed]**.

**SEC-10 — Updates and packages are verified before use.** Update manifests
are Ed25519-signed with the key pinned at build, application-checked,
expiring, and digest-verified before anything switches. Packages never
unpack outside their directory, and downgrades are refused.
- *Enforcement:* E3, the update and package tests **[Enforced]**.

**SEC-11 — Operational and debug surfaces are off or local.** Inspection is
off unless enabled, loopback-only, and tokened. `/healthz` and `/metrics`
are feature-gated and documented as unauthenticated. Admin and deploy APIs
sit behind policies.
- *Enforcement:* E3 **[Enforced]**.

**SEC-12 — Vulnerabilities are handled privately and on time.** Reports go
through `SECURITY.md`. They are acknowledged within five working days. The
fix and advisory are coordinated with the reporter. A fixed vulnerability
gets a regression test, an advisory (and a RUSTSEC entry for published
crates), and a release note.
- *Enforcement:* E6 **[Review]**.

---

## 16. Privacy and data protection (PRV)

**PRV-1 — Nothing leaves the machine without consent.** Telemetry starts
without consent. The application records the person's choice. Withdrawing
consent stops sending at once and drops what was not yet sent
(`docs/telemetry-policy.md`).
- *Enforcement:* E3, the consent tests in `rustnative-observe` **[Enforced]**.

**PRV-2 — There is no framework-operated endpoint.** The framework never
phones home. Every destination is named by the application's publisher.
- *Enforcement:* E3, a scan for hard-coded hosts in non-test code. 30
  `http://` literals exist outside tests at adoption and need triage.
  **[Owed]**

**PRV-3 — Collect the minimum, redact the rest.** Spans carry routes, never
query strings. Bodies, typed text, file contents, and secrets are never
recorded. Inspection recordings apply the same redaction.
- *Enforcement:* E3, redaction tests **[Partial]**.

**PRV-4 — Platform privacy declarations are generated.** Permissions,
privacy manifests, and data-use declarations come from `rustnative.toml`
and `rustnative compliance`, never from hand-maintained copies.
- *Enforcement:* E4 **[Enforced]** for generated outputs.

**PRV-5 — Permissions are requested in context.** A permission is requested
when the feature needing it is used, with the portable state model
(`NotAsked`, `Granted`, `Limited`, `Denied`, `PermanentlyDenied`), never at
startup "just in case".
- *Enforcement:* E5 for examples and components **[Review]**.

---

## 17. Supply chain and build integrity (SUP)

**SUP-1 — Every dependency is justified.** A new dependency needs a reason
in the change: what it replaces, why the standard library or an existing
dependency is not enough, its maintenance status, its licence, its
`unsafe` footprint, and its effect on build time and artifact size. Prefer
fewer, well-maintained crates. Features are minimal (`default-features =
false`).
- *Enforcement:* E3, `cargo deny` (licences, sources, bans, advisories)
  **[Enforced]**. Justification: E5 **[Review]**. Unused dependencies:
  `cargo machete` or `cargo udeps` in CI **[Owed]**.

**SUP-2 — Licences are compatible.** Every dependency is compatible with
`MIT OR Apache-2.0`, and the allowed list is in `deny.toml`.
- *Enforcement:* E3, `cargo deny check licenses` **[Enforced]**.

**SUP-3 — Advisories fail the build.** Vulnerable and yanked crates fail CI.
Unmaintained crates are reported and need a written decision.
- *Enforcement:* E3, `cargo deny` with `yanked = "deny"` and `cargo audit`
  **[Enforced]**. `unmaintained` policy in `deny.toml`: **[Owed]**.

**SUP-4 — Duplicates are explained.** Multiple versions of a crate are
allowed only when listed with a reason (`clippy.toml`
`allowed-duplicate-crates`).
- *Enforcement:* E2/E3, `clippy::multiple_crate_versions` and `cargo deny
  bans` (warn) **[Enforced]**.

**SUP-5 — Vendored code is recorded.** Vendored files keep their licence,
origin, version, and the regeneration procedure in a `VENDORED.md`
(`rustnative-i18n/VENDORED.md`, `rustnative-style/vendor/`). Vendored code
is not edited in place. Changes go upstream or into a documented patch.
- *Enforcement:* E5, CODEOWNERS on `vendor/` **[Owed]**.

**SUP-6 — CI is hardened.** Workflow tokens are least-privilege (`contents:
read` by default). Third-party actions are pinned to a full commit SHA with
the version in a comment, updated by a bot. Untrusted pull requests never
see secrets. Self-hosted runners are never used for forks.
- *Finding at adoption:* actions are pinned by tag (`actions/checkout@v4`,
  `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`).
- *Enforcement:* E3, OpenSSF Scorecard (`Token-Permissions`,
  `Pinned-Dependencies`) in CI and `zizmor` or `actionlint` for workflow
  lint **[Owed]**. `permissions: contents: read` **[Enforced]**.

**SUP-7 — Builds are reproducible.** An independent rebuild of a released
artifact is bit-identical, and any unavoidable variance is documented.
- *Enforcement:* E3, a reproducibility job **[Owed]** (`C131`).

**SUP-8 — Releases carry provenance.** Every released artifact has signed
build provenance meeting SLSA Build Level 2 or higher (hosted build,
signed provenance), plus an SBOM (CycloneDX from `rustnative compliance`).
- *Enforcement:* E3 in the release workflow **[Owed]**.

**SUP-9 — Dependency updates are automated and gated.** A bot (Dependabot
or Renovate) proposes updates for Cargo, GitHub Actions, and the Gradle and
JavaScript toolchains if they are added. Updates merge only on a green full
suite.
- *Enforcement:* E3 **[Owed]**.

**SUP-10 — The lockfile is committed and respected.** `Cargo.lock` is
committed. CI builds with `--locked` so the lockfile cannot drift silently.
- *Enforcement:* E3, `--locked` on CI builds **[Owed]**.

---

## 18. Performance and resource footprint (PRF)

Performance claims are numbers, and the numbers live in `budgets/`. See
`budgets/SCHEMA.md`.

**PRF-1 — Every target has a budget file, enforced on every push.** Cold
start, input latency, frame times, resident memory, artifact size, payload,
edge fuel, and build time are measured against `budgets/<target>.toml`. A
measurement over `max × (1 + tolerance)` fails the build. An unbudgeted
measurement and an unmeasured key both fail.
- *Enforcement:* E3, `rustnative bench --check` in the `budgets` CI job
  **[Enforced]**.

**PRF-2 — Budgets move only with a reason.** Raising a `max` requires the
measured values, the cause, and a statement that the cause is necessary,
written in the budget file's comments and the commit. Lowering one after an
improvement is encouraged, so improvements are kept.
- *Enforcement:* E5, CODEOWNERS on `budgets/` **[Owed]**.

**PRF-3 — No performance claim without a budget.** Documentation that calls
anything fast, light, small, or quick cites `budgets/<target>.toml` in the
same paragraph.
- *Enforcement:* E3, `rustnative-conformance/tests/doc_claims.rs`
  **[Enforced]** for `README.md` and `docs/`.

**PRF-4 — Complexity is stated for hot paths.** Reconciliation, layout,
style resolution, hit-testing, virtualization, and text measurement document
their complexity. A change that worsens it needs a measurement and a reason.
O(n²) over tree size is not accepted on a per-frame or per-event path.
- *Enforcement:* E3, `core_costs.rs` and the criterion benches **[Partial]**.

**PRF-5 — Transient interaction does not re-render.** Scrolling, focus
movement, pointer movement, and animation frames update host state directly
(`PLAN.md` 2.10).
- *Enforcement:* E3, `scrolling_renders_nothing`,
  `typing_renders_only_the_owning_component`, and the animation tests
  **[Enforced]**.

**PRF-6 — Low-end profiles are tested.** Budgets are enforced on one core
(`taskset -c 0`, `--low-end`) and on throttled browser profiles where
budgets exist.
- *Enforcement:* E3 **[Enforced]** for headless, terminal, and Web.

**PRF-7 — Energy is a resource.** No polling where the host offers a
notification. Timers are coalesced and stop when idle (frames stop when an
animation settles). Wake-ups and background work respect power-saving
modes.
- *Enforcement:* E3, `native_frames_stop_when_the_animation_settles` and the
  equivalents **[Partial]**. An idle-wakeups budget key per target is
  **[Owed]**.

**PRF-8 — Large data scales.** Lists, tables, and filters work over
200,000-row data sets without blocking input (Milestone 54). Virtualization
renders only when its range moves.
- *Enforcement:* E3, the virtualization tests and `filter_*` budgets
  **[Enforced]**.

**PRF-9 — Build time and compile cost are budgets too.** Proc-macro
expansion, `.rsx` compilation, and class resolution have budgets
(`rsx_compile_ms_per_kloc`, `class_resolve_us`, `build_time_*`).
- *Enforcement:* E3 **[Enforced]**, with build times optional per run.

---

## 19. Accessibility (A11Y)

Accessibility is a correctness property, never a feature to add later. The
reference is WCAG 2.2 (a W3C Recommendation since October 2023) for the Web
and its host-platform equivalents elsewhere. WCAG 3 is still a draft and is
not cited as a requirement.

**A11Y-1 — Everything goes into the host's accessibility tree.** Every node
has its role, name, state, value, relationships, and position exposed
through the host's API: UI Automation, AT-SPI, `AccessibilityNodeInfo`,
ARIA and HTML, and `NSAccessibility`/`UIAccessibility` when they land.
- *Enforcement:* E3, the per-backend accessibility suites (`native_uia_*`,
  `docs/linux/accessibility.md`, `docs/android/accessibility.md`)
  **[Enforced]** for shipped backends.

**A11Y-2 — A new node kind or component lands accessible.** Its role
mapping on every backend, its keyboard operation, its focus behaviour, and
its accessibility test are part of the same change.
- *Enforcement:* E3, the portable accessibility-model test
  (`rustnative-core/tests/accessibility_model.rs`) and per-backend suites
  **[Partial]**. A check that every `NodeKind` has a role mapping per
  backend is **[Owed]**.

**A11Y-3 — Everything is operable by keyboard.** Every interactive control
is reachable by Tab in reading order and activatable from the keyboard, and
the focus indicator is visible.
- *Enforcement:* E3, `native_focus_traversal`,
  `keyboard_traversal_shows_focus_cues`, `layout_conformance.rs`
  **[Enforced]**.

**A11Y-4 — Targets and text scale.** Interactive targets are at least
24×24 logical pixels (WCAG 2.2, 2.5.8). Layout holds at 100%, 150%, and 200%
text scale without clipping or overlap.
- *Enforcement:* E3, `rustnative-conformance/tests/layout_conformance.rs`
  **[Enforced]**.

**A11Y-5 — Host settings are honoured without application code.** High
contrast, reduced motion, text scale, and colour scheme reach every
realized style at run time.
- *Enforcement:* E3, conformance rows 11 and 22 **[Enforced]**.

**A11Y-6 — Live changes are announced.** Status and live regions announce
through the host.
- *Enforcement:* E3, `native_uia_live_region_change_is_announced` and
  equivalents **[Partial]**.

**A11Y-7 — A person verifies with real assistive technology.** Before a
backend is called supported, a recorded pass with the host's own screen
reader (Narrator or NVDA, Orca, TalkBack, VoiceOver) is filed using the
checklist in `docs/conformance/`. No pass is claimed until it is recorded.
- *Enforcement:* E6 **[Review]**. The Windows pass is owed.

**A11Y-8 — Examples and components are accessible.** Every example and
every component in `rustnative-components` passes the accessibility suite.
An inaccessible example teaches inaccessibility.
- *Enforcement:* E3, an accessibility query test per example **[Owed]**.

---

## 20. Internationalization, localization, text, and input (I18N)

**I18N-1 — No user-facing string is hard-coded in framework components.**
Text that a person sees (component labels, default dialog buttons, error
text shown to end users) comes from localization resources with plural and
gender rules (`rustnative-i18n`, Fluent `.ftl`). Developer-facing
diagnostics are English and stable.
- *Enforcement:* E3, a pseudo-localization run over the component gallery
  that fails on unlocalized text **[Partial]**.

**I18N-2 — Layout is logical.** Use `start`/`end`, never `left`/`right`, in
the portable model. Mirroring happens once, either by the host or by the
backend, never twice.
- *Enforcement:* E1, `EdgeInsets` has no physical fields, plus E3, conformance
  row 3 **[Enforced]**.

**I18N-3 — Text goes through the host's text stack.** Shaping, bidi,
grapheme clusters, emoji sequences, line breaking, and spaceless scripts are
the host's. The framework never splits a string by `char` or byte for
display or caret movement. It uses grapheme segmentation.
- *Enforcement:* E3, `text_goes_through_the_systems_own_stack` and the
  terminal width tables **[Partial]**.

**I18N-4 — Input methods work everywhere text is entered.** Composition,
candidate windows at the caret, dead keys, and on-screen keyboards are
supported in every text control and focusable container.
- *Enforcement:* E3, `native_ime_composition_on_a_focusable_container` and
  equivalents **[Partial]**. A live CJK IME session is E6, and it is owed.

**I18N-5 — Locale-sensitive formatting is never hand-rolled.** Numbers,
dates, times, lists, plurals, and collation use the locale's rules.
- *Enforcement:* E3, `rustnative-core/tests/i18n.rs` **[Partial]**.

**I18N-6 — Pseudo-localization and RTL are in the test matrix.** Layout
conformance runs pseudo-localized (expanded and accented) and mirrored at
every text scale.
- *Enforcement:* E3, `layout_conformance.rs` **[Enforced]**.

---

## 21. Visual and interaction quality (UX)

**UX-1 — Styles resolve through tokens.** Semantic tokens map to host
appearance. Brand values apply as given. A theme or scheme change is a
re-resolution on existing native objects, never a rebuild.
- *Enforcement:* E3, conformance row 22 **[Enforced]**.

**UX-2 — DPI and scale are exact.** Per-monitor scale changes, fractional
scales, and one rounding rule (COR-3) leave no blurry text and no
off-by-one gaps.
- *Enforcement:* E3, the DPI integration tests **[Partial]**.

**UX-3 — Motion is purposeful and optional.** Animations respect reduced
motion (they snap), never block input, and stop when settled.
- *Enforcement:* E3, `native_reduced_motion_arrives_immediately`
  **[Enforced]**.

**UX-4 — No layout shift from framework-controlled content.** Content the
framework controls reserves its size (CLS 0 on the Web budget).
- *Enforcement:* E3, `budgets/web.toml` `cls` **[Enforced]**.

**UX-5 — Adaptive layout is by container.** Size classes per axis, decided
by the container a component is in, not by the device.
- *Enforcement:* E3, conformance row 13 **[Enforced]**.

**UX-6 — Visual regressions are caught.** Realized output is compared
against goldens per machine class. A missing golden fails with its capture
written beside it for review.
- *Enforcement:* E3, the Windows capture goldens, headless and terminal
  goldens, and gallery goldens **[Enforced]**.

---

## 22. Developer experience (DX)

**DX-1 — First run is one command and a budget.** `rustnative new`,
`cd`, `rustnative run <target>` works on a clean machine with the
documented prerequisites, inside the `first_run_s` budget.
`rustnative doctor` explains every missing prerequisite with the fix.
- *Enforcement:* E3, `first_run_s` (optional key) and the CLI tests
  **[Partial]**.

**DX-2 — The dev loop is budgeted and keeps state.** Edits reload within
the `dev_loop_restart_ms` budget (`budgets/linux.toml`), with state kept
where the target allows, and a broken build is shown over the running
application.
- *Enforcement:* E3, `dev_loop_restart_ms` (optional key) and `web_dev.rs`
  **[Enforced]**.

**DX-3 — Compile-time errors are first-class output.** A macro or build-step
diagnostic points at the user's span, names the rule, and suggests the fix.
It never surfaces a panic, a generated-code span, or a backtrace.
- *Enforcement:* E3, trybuild goldens **[Enforced]**.

**DX-4 — Configuration is typed, validated, and documented.**
`rustnative.toml` and every environment variable the framework reads are
documented in one place. Unknown keys are errors with a suggestion. Defaults
are stated. Every environment variable is prefixed `RUSTNATIVE_`.
- *Enforcement:* E3, `deny_unknown_fields` on configuration types
  **[Partial]**, plus a generated reference of environment variables
  **[Owed]**.

**DX-5 — Cargo features are additive and tested.** Enabling a feature never
removes API or changes behaviour silently. Each feature combination that
CI promises builds. `--all-features` builds on every host where it is
meaningful.
- *Enforcement:* E3, `--all-features` clippy **[Enforced]**, plus
  `cargo hack --each-feature` on library crates **[Owed]**.

**DX-6 — The CLI is a stable interface.** Commands, flags, exit codes, and
machine-readable output (`--json`) are covered by CMP-1. Output intended for
people goes to stderr or stdout consistently, and errors exit non-zero.
- *Enforcement:* E3, CLI tests **[Partial]**.

**DX-7 — Tooling explains itself.** `rustnative expand` shows what markup
and classes become. `rustnative describe --json` is the machine-readable
surface for code generators and assistants.
- *Enforcement:* E4, `describe.rs` **[Enforced]**.

---

## 23. Documentation and examples (DOC)

**DOC-1 — Docs change with the code.** A behaviour change updates its
rustdoc, its guide, and its status entry (`BUILD_STATUS.md`) in the same
change. A doc that says something the code does not do is a defect.
- *Enforcement:* E3/E4, doctests, `describe.rs`, `doc_parity.rs`,
  `doc_claims.rs` **[Enforced]**. E5 for prose.

**DOC-2 — Rustdoc builds clean.** No broken intra-doc links and no
warnings.
- *Enforcement:* E3, `RUSTDOCFLAGS=-D warnings` in CI **[Enforced]**.

**DOC-3 — Examples compile, run, and are tested.** Every example is a
workspace member, builds in CI, and has at least one test, or a stated
reason it cannot (a benchmark harness, a device-only host). At adoption,
11 examples have no `tests/` directory.
- *Enforcement:* E3, `cargo check --workspace --all-targets` **[Enforced]**.
  A test per example: **[Owed]**.

**DOC-4 — Status is honest.** `BUILD_STATUS.md` states what was verified
and on what hardware. Work reasoned through but not run is recorded as
exactly that. A platform is called supported only after it runs on real
hardware (`PLAN.md` 2.13).
- *Enforcement:* E5/E6 **[Review]**.

**DOC-5 — Writing is plain and precise.** Write in short declarative
sentences, define terms once, and keep marketing language out. Claims cite
a test or a budget. Documentation is written for a reader arriving without
context.
- *Enforcement:* E3 for performance words (PRF-3), plus a spelling and
  link check over Markdown (`typos`, `lychee`) **[Owed]**.

**DOC-6 — Working notes are labelled.** Plans live in `docs/superpowers/`,
history in `docs/history/`, surveys of other projects in
`docs/ecosystem-analysis/`. None of them is presented as current behaviour.
- *Enforcement:* E5 **[Review]**.

---

## 24. Observability and debuggability (OBS)

**OBS-1 — Library crates do not write to stdout or stderr.** Diagnostics go
through the framework's diagnostics and tracing sinks (`rustnative-observe`)
so an application controls where they go. The CLI, build scripts, examples,
explicit exporters (the stdout exporter), and tests MAY print.
- *Finding at adoption:* `rustnative-core/src/component/tree.rs` and
  `rustnative-core/src/inspect/answer.rs` call `eprintln!`.
- *Enforcement:* E2, `clippy::print_stdout` and `print_stderr` denied in
  library crates, allowed per item with a reason **[Owed]**.

**OBS-2 — Spans and metrics follow the conventions.** Names and attributes
follow `rustnative_observe::conventions` (OpenTelemetry semantic
conventions). Spans carry paths, not query strings.
- *Enforcement:* E3, `rustnative-observe/tests/observe.rs` **[Partial]**.

**OBS-3 — Every backend answers the inspection protocol.** The tree, realized
host objects and their rectangles, lifetimes, capabilities, and style
tables (conformance row 26).
- *Enforcement:* E3 per backend **[Enforced]**.

**OBS-4 — Every render records its cause.** `RenderCause` is recorded for
every render so over-rendering is diagnosable.
- *Enforcement:* E3 **[Enforced]**.

**OBS-5 — Debug builds diagnose, release builds stay quiet.** Expensive
checks and verbose diagnostics are behind `debug_assertions` or a feature,
never silently on in release.
- *Enforcement:* E3 through the budgets **[Partial]**.

---

## 25. Testing and verification (TST)

**TST-1 — Every behaviour change has a test, written first where
practical.** A bug fix includes the test that failed before the fix.
Lifecycle and reentrancy changes need tests of their ordering.
- *Enforcement:* E5 **[Review]**, plus TST-8 coverage on changed lines
  **[Owed]**.

**TST-2 — Tests run on the real host.** Platform integration tests drive the
production code path on the real host: real HWNDs, GTK on Xvfb, Wayland,
and Xwayland, an Android emulator, headless Edge or Chromium, a pty,
ConPTY, tmux, and SSH. Synthetic input goes through the real input path,
never a test-only shortcut.
- *Enforcement:* E3, the per-backend CI jobs **[Enforced]**.

**TST-3 — The headless backend is the reference.** Portable behaviour is
tested against `rustnative-headless` with the deterministic clock and
executor, and on one core.
- *Enforcement:* E3, the `headless` CI job **[Enforced]**.

**TST-4 — Shared suites are written once.** Suites go in
`rustnative-conformance` over the `ConformanceHost` seam and run on every
backend.
- *Enforcement:* E3 **[Enforced]**.

**TST-5 — Properties, not only examples.** Reconciliation, identity, layout,
scroll ranges, cancellation, and merge functions have proptest properties.
Failing seeds are committed (`*.proptest-regressions`) and never deleted.
- *Enforcement:* E3 **[Enforced]**.

**TST-6 — Parsers and decoders are fuzzed.** The markup and style parsers,
manifest and package readers, wire and protocol decoders (sync, MQTT,
inspection, server-function calls), and the terminal's escape-sequence
input parser have `cargo-fuzz` targets, run in CI briefly and continuously
elsewhere. Crashes become regression tests.
- *Enforcement:* E3 **[Owed]** (`C130`).

**TST-7 — Tests are deterministic.** Tests use no wall-clock sleeps to wait
for a condition. They poll with a deadline or use the test clock. There is
no order dependence, no shared global state between tests, and no network
beyond loopback. At adoption 24 `thread::sleep` calls remain in test
directories, and each needs a reason or a rewrite.
- *Enforcement:* E3, repeated runs that detect flakiness, plus a sleep scan
  with an allowlist **[Owed]**.

**TST-8 — Flaky tests are quarantined with an owner and an expiry, never
endured or silenced.** A quarantined test is tracked in an issue, keeps
running in a non-blocking job, and its expiry forces a fix or a deletion
decision. Skipping, disabling, or deleting a test to make CI pass is not
allowed.
- *Enforcement:* E3/E6 **[Owed]** (`C130`).

**TST-9 — A test that cannot run says so.** A test needing hardware, a
display, a session bus, or a browser is `#[ignore = "reason"]` or reports a
skip that CI can make fatal (`RUSTNATIVE_BROWSER_REQUIRED`). It never passes
silently. At adoption 19 places in tests print "skipped" and return, so the test passes.
- *Enforcement:* E3, CI sets the `…_REQUIRED` variables on hosts that must
  run them, and a scan rejects silent `return` after a skip message
  **[Partial]**.

**TST-10 — Goldens are reviewed, not rubber-stamped.** A golden update
states in the change why the output changed. Binary goldens are kept per
machine class and stay small.
- *Enforcement:* E5 **[Review]**.

**TST-11 — Coverage is measured and changed code is covered.** Line and
branch coverage (`cargo llvm-cov`) is reported per crate. Changed lines in
a pull request meet a threshold, with exceptions stated.
- *Enforcement:* E3 **[Owed]** (`C130`).

**TST-12 — Hardware-only work is recorded as such.** What cannot be tested
without hardware is listed in `BUILD_STATUS.md` as not verified, never
assumed to pass.
- *Enforcement:* E5/E6 **[Review]**.

**TST-13 — Requirements trace to tests.** Requirements with IDs (`C…`,
`W-…`) are annotated `// req: ID` at the tests that verify them, feeding
`traceability.json`.
- *Enforcement:* E4, `rustnative compliance` **[Partial]**. Annotation is
  sparse at adoption (9 uses).

---

## 26. Code quality, style, and maintainability (QLT)

**QLT-1 — Formatting is not a discussion.** `rustfmt` with the repository's
`rustfmt.toml`.
- *Enforcement:* E3, `cargo fmt --check` **[Enforced]**.

**QLT-2 — The lint policy is curated and ratcheted.** The workspace policy is
`clippy::pedantic` and `clippy::cargo` at warn, a chosen set of
`restriction` lints, and `-D warnings` in CI. New lints from a new stable
release are fixed or explicitly allowed with a reason before the release
reaches CI. Every crate inherits `[lints] workspace = true`.
- *Enforcement:* E2/E3 **[Enforced]**. All 60 manifests inherit it.

**QLT-3 — Every suppression carries its reason.** `#[allow]` and `#[expect]`
take `reason = "…"`, citing a standard ID where one applies. Prefer
`#[expect]`, which warns when the suppression is no longer needed. At
adoption 52 of 191 `#[allow]` attributes have no reason, and no `#[expect]`
is used.
- *Enforcement:* E2, `clippy::allow_attributes_without_reason = "deny"`
  (stable since Rust 1.81, below the MSRV), then
  `clippy::allow_attributes = "warn"` **[Owed]**.

**QLT-4 — No unfinished code.** No `todo!`, `unimplemented!`, `dbg!`, or
`TODO`/`FIXME` without an issue link.
- *Enforcement:* E2, `todo` and `unimplemented` denied **[Enforced]**.
  `dbg_macro` and an issue-link check for `TODO`: **[Owed]**.

**QLT-5 — Names say what things are.** Name things for the domain concept.
No abbreviations beyond the host's own vocabulary (`HWND`, `JNI`). Test
names are sentences describing the behaviour
(`a_revoked_session_opens_as_none`).
- *Enforcement:* E2, `clippy::many_single_char_names`, `similar_names`
  **[Enforced]**. E5.

**QLT-6 — Comments explain why, not what.** Module docs state the module's
responsibility and its invariants. Comments carry the reason a non-obvious
choice was made, citing `PLAN.md`, a milestone, or a standard. Comments that
only narrate the code are removed.
- *Enforcement:* E5 **[Review]**.

**QLT-7 — No dead code.** Unused items are removed, not `#[allow(dead_code)]`-ed,
unless the item is used on a target CI does not build. Then the reason
names the target.
- *Enforcement:* E2, `dead_code` warns, denied in CI **[Enforced]**.
  Reasons: QLT-3.

**QLT-8 — Generated code is reproducible and marked.** Generated files
(`framework.json`, width tables, expansion goldens, generated JS and CSS)
have a generator in the repository, a header naming it, and a staleness
test. Generated code is never edited by hand.
- *Enforcement:* E4 **[Partial]**: `describe.rs` and the width-table
  generator hold for their files.

**QLT-9 — Source files are clean text.** UTF-8, LF line endings, a final
newline, no trailing whitespace, and no control characters other than tab
and newline.
- *Finding at adoption:* corrupted escape sequences left raw control bytes
  in doc comments: a NUL in `rustnative-web/src/client.rs` ("Subscription
  \0 delivered a value"), backspaces in `rustnative-webgen/src/items.rs`
  ("\breaks"), and a BEL in `rustnative-style/src/web.rs` ("\auto").
- *Enforcement:* E3, a source-hygiene test over every tracked text file
  **[Owed]**. `.gitattributes` enforces LF **[Enforced]**.

**QLT-10 — Every language in the repository has a standard.**
- **Java** (Android host library): Google Java Style, `final` classes by
  default, no reflection, Javadoc on every class, no swallowed exceptions.
  Enforcement: `javac -Xlint:all -Werror` in the Android job **[Owed]**.
- **JavaScript** (`rn.js`, `sw.js`): ES modules, `"use strict"`, no
  dependencies, no `eval`, `new Function`, `innerHTML`, or `document.write`
  (none at adoption). Behaviour is held equal to Rust by
  `rustnative-web/tests/runtime.rs`. Enforcement: a scan for the forbidden
  APIs **[Owed]**. Equality: E3 **[Enforced]**.
- **Shell** (`tools/*.sh`): `#!/usr/bin/env bash`, `set -euo pipefail`,
  quoted expansions, `shellcheck` clean. At adoption
  `tools/android-env.sh` has no `set -e`. As a sourced file, it may need
  `set -u` only. Enforcement: `shellcheck` in CI **[Owed]**.
- **SQL** (migrations): one statement per concern, an explicit `down` for
  every `up`. Enforcement: DAT-1.
- **C and C#** (adoption hosts, probes): compile warning-free at the
  highest level (`/W4`, `-Wall -Wextra`). Enforcement: E5 **[Review]**.
- **CSS** (component and example styles): only the framework's style
  vocabulary, tokens over literals. Enforcement: the style compiler
  **[Enforced]**.
- **TOML and YAML:** formatted consistently. Enforcement: `taplo fmt
  --check` and `actionlint` **[Owed]**.

**QLT-11 — Duplication is removed at the second copy.** Logic copied
between two places moves into a shared function, module, or group crate
when the second copy appears, as `rustnative-update` was extracted.
- *Enforcement:* E5 **[Review]**.

---

## 27. Version control and contribution workflow (VCS)

**VCS-1 — One concern per change.** A pull request does one thing: a
feature, a fix, a refactor, or a dependency update. Refactors that make a
feature possible land first, separately.
- *Enforcement:* E5 **[Review]**.

**VCS-2 — Commit messages explain.** The subject is a short summary
prefixed with its area, in the style of the existing history ("Terminal
backend: …", "CI: …"). The body says why, cites standard IDs and
milestones, and names the tests that hold the change. Each commit
builds.
- *Enforcement:* E5 **[Review]**, plus a commit-message lint for the
  subject format **[Owed]**.

**VCS-3 — Contributors certify their right to contribute.** Every commit
carries `Signed-off-by:` under the Developer Certificate of Origin 1.1.
- *Enforcement:* E3, a DCO check on pull requests **[Owed]**.

**VCS-4 — Ownership is explicit.** A `CODEOWNERS` file assigns owners to the
core, each backend, the server's security code, `unsafe`-heavy modules, CI,
`budgets/`, `deny.toml`, `vendor/`, and these standards. Changes there need
an owner's approval.
- *Enforcement:* E5, branch protection with required CODEOWNERS review
  **[Owed]**.

**VCS-5 — The default branch is protected.** Required status checks are all
CI jobs that must pass. Required reviews are one, or two for SEC-7
areas. No force-push or direct push. Linear history or merge commits are
chosen once and kept.
- *Enforcement:* E3, repository settings **[Owed]**, verified by OpenSSF
  Scorecard `Branch-Protection`.

**VCS-6 — Build outputs are never committed.** `target/` directories at any
depth, captures, stack dumps, and local state stay out of the repository.
- *Finding at adoption:* `examples/hello-label/target/rustnative-dev/last-restart.json`
  is tracked, because `.gitignore` ignores only the root `/target/`.
- *Enforcement:* E3, `.gitignore` patterns (`target/`), plus a CI check
  that fails on tracked files under any `target/` **[Owed]**.

**VCS-7 — Binary files are justified and small.** A binary (goldens, icons,
fixtures) has a reason, the smallest form that serves it, and a size
limit. Large binaries go in Git LFS or are generated.
- *Enforcement:* E3, a size check on added binaries **[Owed]**.

**VCS-8 — Pull requests use the template.** The template has these
sections: what and why, the standards touched, the tests added, the hosts
verified on, the documentation updated, and a security and privacy note.
- *Enforcement:* E5, `.github/pull_request_template.md` **[Owed]**.

**VCS-9 — Review is a standard, not a formality.** Reviewers check the
change against the checklist in section 33. They reproduce claims
("verified on Windows") or ask for evidence. They separate blocking
findings from optional ones. Review comments are about the code. Authors
answer every comment with a change or a reason.
- *Enforcement:* E5 **[Review]**.

---

## 28. CI and the build system (CI)

**CI-1 — CI is the definition of "builds".** If it is not checked in CI, it
is not promised. Every gate listed in `CONTRIBUTING.md` runs in CI, and
every CI gate is runnable locally with a documented command.
- *Enforcement:* E3 **[Enforced]**.

**CI-2 — Each backend is tested on its own host.** Windows on
`windows-latest`, GTK on Ubuntu, Fedora, and Arch, Android on an emulator,
the terminal on a pty, tmux, and SSH, the Web in headless Edge or Chromium,
and Apple targets with `cargo check` until hardware exists.
- *Enforcement:* E3 **[Enforced]**.

**CI-3 — Warnings are errors.** `RUSTFLAGS=-D warnings` and
`RUSTDOCFLAGS=-D warnings`.
- *Enforcement:* E3 **[Enforced]**.

**CI-4 — The toolchain is newest stable, plus the MSRV.** Lints from a new
stable release are addressed before they reach CI (CONTRIBUTING).
- *Enforcement:* E3 **[Enforced]**.

**CI-5 — Every job has a timeout, and superseded runs are cancelled.**
`timeout-minutes` is set on every job (only the Android job has one at
adoption). A `concurrency` group cancels superseded runs of a branch.
- *Enforcement:* E3, `actionlint` plus a check for `timeout-minutes`
  **[Owed]**.

**CI-6 — Caches never change results.** A cache speeds up a build but never
changes its outcome. A cold-cache run is identical, and cache keys include
the toolchain and lockfile.
- *Enforcement:* E3, scheduled cold-cache runs **[Owed]**.

**CI-7 — Disabled jobs are tracked.** A job commented out or disabled (the
Windows cross-compile under Wine) has an issue and an owner, and its
reason is in the workflow.
- *Enforcement:* E5 **[Review]**.

**CI-8 — Artifacts from CI are for diagnosis.** Golden captures and budget
reports are uploaded on failure, so reviewers can see what changed.
- *Enforcement:* E3 **[Enforced]**.

---

## 29. Release, distribution, and operations (RLS)

**RLS-1 — Releases are cut from CI, never from a laptop.** A tag on the
default branch triggers the release workflow. It builds, signs, writes
provenance and an SBOM (SUP-8), and publishes.
- *Enforcement:* E3 **[Owed]**.

**RLS-2 — Every release has notes.** Notes list breaking changes with their
codemods (CMP-4), deprecations, MSRV changes, host-support changes,
security fixes with advisory IDs, and budget changes. Notes are written
from the changes' own descriptions, so each pull request states its
user-visible effect.
- *Enforcement:* E5, with a release checklist **[Owed]**.

**RLS-3 — The crates release together under one version.** A seam count of
one, per `docs/policy/stability.md`.
- *Enforcement:* E3, a version-consistency check over the workspace
  **[Owed]**. Today the workspace dependencies pin one version.

**RLS-4 — Packages follow each host's rules.** Signing, store policies, and
update rules are those in `docs/deploy/update-rules.md`. The updater never
runs where the host forbids self-update.
- *Enforcement:* E3, the packaging and update tests **[Enforced]**.

**RLS-5 — Server deployments are revisable.** Revisions are immutable,
traffic moves by percentage, rollback is one command, and health and
readiness are real.
- *Enforcement:* E3, `rustnative-server/tests/deploy.rs` **[Enforced]**.

**RLS-6 — Published crates are complete.** Each published crate has its
`description`, `license`, `repository`, `readme`, `keywords`, and
`categories`, `include`/`exclude` so tests and goldens are not shipped
needlessly, and `cargo publish --dry-run` passes.
- *Enforcement:* E2, `clippy::cargo` (`cargo_common_metadata`)
  **[Enforced]**, plus a `publish --dry-run` job **[Owed]**.

---

## 30. Portability, interoperability, and extensibility (PRT)

**PRT-1 — Portable crates build everywhere the plan promises.** The core
and portable crates build for macOS and iOS (`cargo check`), and the value
types build for bare metal (`thumbv7em-none-eabihf`, `no_std`).
- *Enforcement:* E3, the `apple` and `no-std` jobs **[Enforced]**.

**PRT-2 — `no_std` crates stay `no_std`.** A crate declared `no_std`
(`rustnative-types`) never gains a `std` dependency.
- *Enforcement:* E3 **[Enforced]**.

**PRT-3 — Incremental adoption keeps working.** Embedding RustNative in a
host application, and a host's controls in RustNative, are supported paths
(`docs/interop/adoption-ladder.md`), each with an example and a test
(`adoption-*`).
- *Enforcement:* E3 **[Enforced]**.

**PRT-4 — Extension points are contracts.** Capability packages, native
mappers, services, and the inspection protocol are documented contracts.
Extending them never requires forking the framework, and they are subject
to CMP.
- *Enforcement:* E3, the package and mapper tests **[Enforced]**.

**PRT-5 — Capability grants are scoped and enforced per call.** A package
reaches services only through `ScopedServices` and its declared grants.
- *Enforcement:* E1/E3, `rustnative-core/tests/grants.rs` **[Enforced]**.

**PRT-6 — The development host is not the only host.** Contributors on
Windows, Linux, and macOS can build and test what their host supports. A
script works on the platforms it claims, or says which ones it supports.
- *Enforcement:* E3, the CI matrix **[Partial]**.

**PRT-7 — Embedded targets reuse the ecosystem.** Use `embedded-hal`,
existing executors, `probe-rs`, and board descriptions. The framework never
writes a second copy (`docs/policy/embedded.md`).
- *Enforcement:* E5 **[Review]**.

---

## 31. Legal, licensing, and regulatory compliance (LEG)

**LEG-1 — Contributions are dual-licensed.** Every contribution is under
`MIT OR Apache-2.0`, the workspace licence. Contributing is agreement, and
the DCO sign-off records it (VCS-3).
- *Enforcement:* E3, the DCO check **[Owed]**.

**LEG-2 — Third-party code keeps its notices.** Vendored and copied code
keeps its licence file and attribution. Compatible licences only (SUP-2).
Code from a source whose licence is unknown is not accepted.
- *Enforcement:* E3, `cargo deny` for crates **[Enforced]**. E5 for vendored
  and copied files.

**LEG-3 — Generated evidence, never hand-maintained.** The SBOM, licence
inventory, privacy manifest, and traceability come from `rustnative
compliance`, so they cannot drift.
- *Enforcement:* E4 **[Enforced]**.

**LEG-4 — Regulatory obligations are tracked.**
- **EU Cyber Resilience Act:** manufacturers' vulnerability-reporting
  obligations apply from 11 September 2026. As an open-source project the
  framework MAY fall under the "open-source software steward" provisions,
  whose dates sources currently disagree on. The project keeps a
  documented security policy (SECURITY.md, SEC-12), an SBOM, and a
  coordinated-disclosure process that downstream manufacturers can rely
  on. A maintainer checks the official text and Commission guidance on
  steward dates before relying on any date.
- **Accessibility law** (the European Accessibility Act, Section 508, and
  equivalents): A11Y and its conformance evidence.
- **Data protection** (GDPR and equivalents): PRV.
- **App stores:** the update rules (RLS-4) and permission declarations
  (PRV-4).
- *Enforcement:* E6 **[Review]**, with a yearly review recorded in
  `docs/security/certification-posture.md`.

**LEG-5 — Names and marks are respected.** Third-party product names are
used descriptively. No logos or trademarks are shipped without permission.
No code, assets, or text are copied from projects whose terms forbid it.
- *Enforcement:* E5 **[Review]**.

**LEG-6 — Export-relevant code is known.** Cryptographic functionality is
listed in the release notes and SBOM, so distributors can meet their
export obligations.
- *Enforcement:* E4, the SBOM **[Partial]**.

---

## 32. Governance, community, and conduct (GOV)

**GOV-1 — A code of conduct applies everywhere the project meets.** The
project adopts Contributor Covenant 3.0 (July 2025) as `CODE_OF_CONDUCT.md`,
with a named private contact and its "addressing and repairing harm"
enforcement process.
- *Enforcement:* E6 **[Owed]** (the file does not exist yet).

**GOV-2 — Roles are written down.** `GOVERNANCE.md` defines contributor,
reviewer, maintainer, and area owner. It says how each role is earned and
lost, who can merge, who can release, and how decisions are made (lazy
consensus, then a maintainer vote). It lists the maintainers.
- *Enforcement:* E6 **[Owed]**.

**GOV-3 — Issues are triaged and labelled.** Issue templates cover bugs
(with host, version, and reproduction), feature requests (with the use
case and the principle it serves), backend gaps, and documentation. A
triager acknowledges new issues and labels them by area, kind, and
priority. Good first issues are marked and kept current.
- *Enforcement:* E6, `.github/ISSUE_TEMPLATE/` **[Owed]**.

**GOV-4 — Communication is in the open.** Design discussion happens in
issues, RFCs, and pull requests, where it is archived. Decisions made
elsewhere are summarized there before they take effect. Security issues are
the exception (SEC-12).
- *Enforcement:* E6 **[Review]**.

**GOV-5 — Significant changes go through an RFC.** A new crate, platform,
public protocol, change to a `PLAN.md` section 2 principle, or change to
these standards needs a written proposal with motivation, design,
alternatives, and drawbacks. It needs a comment period of at least seven
days and a recorded decision.
- *Enforcement:* E6 **[Owed]**, with `docs/rfcs/` and a template.

**GOV-6 — AI-assisted contributions are owned by a person.** Tools,
including AI assistants, MAY help. The contributor:
- discloses material AI assistance in the pull request;
- understands, has reviewed, and can explain every line, and answers review
  in their own words;
- MUST NOT submit AI-generated `SAFETY:` proofs, security-sensitive code
  (SEC-7 areas), or threat-model entries without their own independent
  verification, stated in the PR;
- ensures nothing in the contribution infringes a licence (LEG-2);
- does not open bulk, unreviewed, or speculative pull requests.

Maintainers MAY close contributions that break this without detailed
review. For comparison, the rust-lang/rust repository adopted an LLM
policy in August 2026.
- *Enforcement:* E5/E6, a disclosure field in the PR template **[Owed]**.

**GOV-7 — Status and promises are honest.** The project says what is
verified, on what, and what is owed (DOC-4, TST-12, NAT-3). It does not
announce support it has not tested, and it compares itself with other
projects only by the published methodology
(`docs/comparison/methodology.md`).
- *Enforcement:* E5/E6 **[Review]**.

**GOV-8 — The project plans for its own continuity.** Each area has at
least two people able to review it, documented in CODEOWNERS. Release and
signing keys have documented custody and a recovery procedure. Knowledge
held by one person is written down.
- *Enforcement:* E6 **[Owed]**.

**GOV-9 — Recognition is part of the work.** Contributors are credited in
release notes. Reviewers' time is acknowledged.
- *Enforcement:* E6 **[Review]**.

---

## 33. The contribution checklist

Authors fill this in, and reviewers verify it. "N/A" is a valid answer only
with a reason.

**Every change**
- [ ] One concern (VCS-1). Commit messages explain why and cite standard IDs
      (VCS-2). Signed off (VCS-3).
- [ ] The local gates in `CONTRIBUTING.md` pass: fmt, clippy with
      `-D warnings` on newest stable, test, doc, deny, audit. MSRV checked.
- [ ] Tests added or updated (TST-1). A bug fix includes its failing test.
- [ ] Docs, guides, and `BUILD_STATUS.md` updated (DOC-1). Status is honest
      about which hosts it was verified on (DOC-4).
- [ ] No new `#[allow]` without a reason (QLT-3). No `unwrap` or `expect`
      on reachable paths (STB-1). No new `Result<_, String>` (ERR-1).
- [ ] No build outputs or large binaries committed (VCS-6, VCS-7). Source
      is clean text (QLT-9).
- [ ] AI assistance disclosed, if material (GOV-6).

**When it applies**
- [ ] **Public API:** documented with an example, `#[must_use]` and
      `#[non_exhaustive]` considered, semver impact stated, deprecation and
      codemod for breaks (API-2, API-4, CMP-1…4).
- [ ] **Node kind, modifier, or style property:** both syntaxes, both
      spellings, equivalence case, every backend's answer, accessibility
      mapping (EQV-1, EQV-4, A11Y-2).
- [ ] **Backend code:** conformance rows updated, capability advertised only
      if realized, ownership module updated, census returns to baseline,
      panic restores the host (NAT-3, NAT-4, ARC-7, RES-2, STB-2).
- [ ] **`unsafe` or FFI:** `SAFETY:` proof per block, ownership documented,
      owner review (MEM-2, MEM-4, VCS-4).
- [ ] **Async or threads:** owned by a scope, no UI-thread blocking, bounded
      channels, no lock across `.await` (CON-2, CON-4, CON-6, CON-7).
- [ ] **Untrusted input, listener, or parser:** threat model row, limits
      before allocation, depth bounds, fuzz target (SEC-2, SEC-6, TST-6,
      REL-7).
- [ ] **Crypto, auth, or update code:** two approvals (SEC-7).
- [ ] **Persisted or wire format:** versioned, fixture for the old version,
      migration (CMP-6, DAT-2).
- [ ] **Performance-sensitive path:** budgets pass; any budget change is
      justified (PRF-1, PRF-2).
- [ ] **New dependency:** justified, licence checked, features minimal
      (SUP-1).
- [ ] **User-visible text:** localized, RTL and pseudo-localization checked
      (I18N-1, I18N-6).
- [ ] **Telemetry or data handling:** consent, redaction, minimization
      (PRV-1, PRV-3).

### Definition of done

A change is done when every applicable checklist item is satisfied, every CI
gate is green on the change's head, every review thread is answered, and
the change's claims (verified hosts, guarantees, numbers) are held by a test,
a budget, or a recorded pass.

---

## 34. Waivers

A MUST can be waived only when following it is impossible or clearly worse
for the project.

1. The waiver is written in the pull request, citing the standard ID, the
   reason, the risk, and the scope (a file, an item, or a release).
2. It is approved by a maintainer who owns the area, plus one more for SEC,
   MEM, or SUP standards.
3. In code it appears as `#[allow(…, reason = "waiver: <ID> — …")]` or a
   comment of the same form at the site.
4. A waiver with no end date is reviewed at each minor release.

A waiver that keeps recurring is evidence the standard is wrong, and it is
brought to GOV-5.

---

## 35. Enforcement owed

These mechanisms are needed to raise standards marked **[Owed]** or
**[Partial]** to their target rung. They are ordered by how much protection
each buys per unit of work. Items marked ★ fix a defect found while scanning
the repository for these standards.

| # | Mechanism | Raises | Rung |
|---|---|---|---|
| 1 | ★ Source-hygiene test over tracked text files: control characters, final newline, trailing whitespace. Fix the three corrupted doc comments. | QLT-9 | E3 |
| 2 | ★ `.gitignore` `target/` at every depth. Remove the tracked `examples/hello-label/target/…` file. Add a CI check. | VCS-6 | E3 |
| 3 | ★ `missing_docs = "deny"` in `[workspace.lints.rust]` (9 crates lack it) | API-2 | E2 |
| 4 | ★ `clippy::allow_attributes_without_reason = "deny"`, and migrate 52 reasonless allows | QLT-3 | E2 |
| 5 | ★ MQTT broker: cap remaining length, connections, and first-packet time, and allocate after the check | REL-7, SEC-6 | E3 |
| 6 | ★ `LoadLibraryExW(…, LOAD_LIBRARY_SEARCH_SYSTEM32)` for `dxcore.dll`, and a scan forbidding bare `LoadLibraryW` | SEC-8 | E3 |
| 7 | `#![forbid(unsafe_code)]` in the crates with no `unsafe` | MEM-1 | E2 |
| 8 | `clippy::print_stdout` and `print_stderr` in library crates | OBS-1 | E2 |
| 9 | Pin actions by SHA, add `timeout-minutes`, `concurrency`, `actionlint`/`zizmor`, and Scorecard | SUP-6, CI-5 | E3 |
| 10 | CODEOWNERS, branch protection, PR and issue templates, DCO check | VCS-3…8, GOV-3 | E3/E5 |
| 11 | Architecture boundary test over `cargo metadata`, plus a host-`cfg` scan of the core | ARC-1…4 | E3 |
| 12 | Public API snapshot and `cargo-semver-checks` | CMP-1, CMP-2, API-4 | E4 |
| 13 | `cargo-fuzz` targets for every parser and decoder | TST-6, SEC-3, SEC-6 | E3 |
| 14 | Typed-error burn-down for the 134 `Result<_, String>` signatures, plus a scan | ERR-1 | E3 |
| 15 | Coverage on changed lines (`cargo llvm-cov`) | TST-11 | E3 |
| 16 | Miri on portable crates, ASan/TSan on backend FFI tests | MEM-6 | E3 |
| 17 | Flake detection and quarantine with owner and expiry, plus a sleep scan | TST-7, TST-8 | E3 |
| 18 | Silent-skip scan (19 sites), and `…_REQUIRED` variables in CI | TST-9 | E3 |
| 19 | Dependency-update bot, `--locked`, `cargo machete`, `unmaintained` policy | SUP-1, SUP-3, SUP-9, SUP-10 | E3 |
| 20 | `cargo hack --each-feature` on library crates | DX-5 | E3 |
| 21 | Fixtures for every persisted and wire format version | CMP-6, DAT-2 | E3 |
| 22 | `shellcheck`, `javac -Xlint:all -Werror`, `taplo`, the JS forbidden-API scan | QLT-10, MEM-7 | E3 |
| 23 | Markdown spelling and link checks (`typos`, `lychee`), and extend `doc_parity` to all of `docs/` | DOC-5, EQV-7 | E3 |
| 24 | Release workflow with provenance (SLSA Build L2+), SBOM, `publish --dry-run`, reproducibility check | SUP-7, SUP-8, RLS-1, RLS-6 | E3 |
| 25 | `CODE_OF_CONDUCT.md`, `GOVERNANCE.md`, `docs/rfcs/` | GOV-1, GOV-2, GOV-5 | E6 |
| 26 | A test per example, and an accessibility query per example | DOC-3, A11Y-8 | E3 |
| 27 | Release-health gating of rollouts | STB-7 | E3 |

When an item lands, the standards it raises change their tag to
**[Enforced]** in the same change.

---

## 36. Changing these standards

These standards are themselves subject to GOV-5. A proposal to add, change,
or retire a standard states which loop question it answers (see
`docs/policy/standards-derivation.md`): a domain not covered, a
standard that covers too little, enforcement that is not enough, or a
better enforcement found. Enforcement only ratchets upward (section 0). A
standard is retired only when a stronger one replaces it, and the retired
ID is kept, marked *retired*.
