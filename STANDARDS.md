# RustNative Engineering Standards

This document sets the bar every contribution to RustNative has to clear,
whether it comes from a maintainer, an outside contributor, or a tool. It
covers 33 domains, from architecture to governance. Each domain lists its
standards, and each standard says why it exists and what enforces it.

The standards come from a question loop run against the whole repository
twice. The first round read every tracked file by script and the
governance documents in full. The second round:

- compiled and linted every crate on Linux, Windows, and Android with about
  60 lints the workspace does not enable;
- ran the test suites three times, plus the GTK backend under Xvfb, and the
  supply-chain, workflow, shell, and unused-dependency tools;
- ran Miri over the crates it can execute;
- fuzzed eleven parsers and decoders with 2.5 million generated inputs;
- read the security-relevant modules in full;
- reproduced the suspected defects against the real code.

Each pass asked of each domain: what is it, why does it matter, does it
cover enough, is there more to explore, how is it enforced, why that
enforcement, is it enough, and is there a better way. A pass then started
again with everything the last one found, and the loop stopped only when a
full pass found nothing new. `docs/policy/standards-derivation.md` records
both rounds and the evidence.

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
6. [Native fidelity, capability honesty, and portable contracts (NAT)](#6-native-fidelity-capability-honesty-and-portable-contracts-nat)
7. [Correctness and determinism (COR)](#7-correctness-and-determinism-cor)
8. [Stability: panics, boundaries, teardown (STB)](#8-stability-panics-boundaries-teardown-stb)
9. [Reliability and resilience (REL)](#9-reliability-and-resilience-rel)
10. [Error handling (ERR)](#10-error-handling-err)
11. [Memory safety, unsafe code, and FFI (MEM)](#11-memory-safety-unsafe-code-and-ffi-mem)
12. [Resource and lifetime management (RES)](#12-resource-and-lifetime-management-res)
13. [Concurrency and async (CON)](#13-concurrency-and-async-con)
14. [Data integrity and persistence (DAT)](#14-data-integrity-and-persistence-dat)
15. [Security (SEC)](#15-security-sec)
16. [Network services and listeners (NET)](#16-network-services-and-listeners-net)
17. [Cryptography, keys, and signing (CRY)](#17-cryptography-keys-and-signing-cry)
18. [Privacy and data protection (PRV)](#18-privacy-and-data-protection-prv)
19. [Supply chain and build integrity (SUP)](#19-supply-chain-and-build-integrity-sup)
20. [Performance and resource footprint (PRF)](#20-performance-and-resource-footprint-prf)
21. [Accessibility (A11Y)](#21-accessibility-a11y)
22. [Internationalization, localization, text, and input (I18N)](#22-internationalization-localization-text-and-input-i18n)
23. [Visual and interaction quality (UX)](#23-visual-and-interaction-quality-ux)
24. [Developer experience (DX)](#24-developer-experience-dx)
25. [Documentation and examples (DOC)](#25-documentation-and-examples-doc)
26. [Observability and debuggability (OBS)](#26-observability-and-debuggability-obs)
27. [Testing and verification (TST)](#27-testing-and-verification-tst)
28. [Code quality, style, and maintainability (QLT)](#28-code-quality-style-and-maintainability-qlt)
29. [Version control and contribution workflow (VCS)](#29-version-control-and-contribution-workflow-vcs)
30. [CI and the build system (CI)](#30-ci-and-the-build-system-ci)
31. [Release, distribution, and operations (RLS)](#31-release-distribution-and-operations-rls)
32. [Portability, interoperability, and extensibility (PRT)](#32-portability-interoperability-and-extensibility-prt)
33. [Legal, licensing, and regulatory compliance (LEG)](#33-legal-licensing-and-regulatory-compliance-leg)
34. [Governance, community, and conduct (GOV)](#34-governance-community-and-conduct-gov)
35. [The contribution checklist](#35-the-contribution-checklist)
36. [Waivers](#36-waivers)
37. [Findings and enforcement owed](#37-findings-and-enforcement-owed)
38. [Changing these standards](#38-changing-these-standards)

---

## 0. How to read this document

### Normative words

**MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** are used as
RFC 2119 and RFC 8174 define them. A MUST cannot be broken without a waiver
(section 36). Breaking a SHOULD needs a reason written in the change, where
the reviewer can see it.

### Identifiers

Each standard has a stable ID, such as `NET-2`. Cite it in review comments,
commit messages, waivers, and `#[allow]` reasons, for example
`reason = "MEM-3: …"`. An ID is never reused. A retired standard keeps its
number and is marked *retired*.

Findings are cited by their register number, such as `F-21`. The register
in section 37 records each open defect the scan found, where it is, and
whether it was reproduced by running it. A standard that a finding breaks cites the
finding, so a contributor can see the rule's cost in this codebase.

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

Five rules govern the ladder:

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
5. **Evidence is executed.** A security or reliability claim, in either
   direction, is settled by running it: a test, a probe, a fuzz run, or a
   tool. Reading code proposes a finding, and running code confirms or
   retires it. The second round of the scan retired two of its own
   first-round claims this way.

### Status tags

| Tag | Meaning |
|---|---|
| **[Enforced]** | A mechanism in the repository fails the build or the suite today. |
| **[Partial]** | Enforced for some crates, targets, or cases. The gap is named. |
| **[Owed]** | The standard applies now, but its mechanism is not built. It is listed in section 37. |
| **[Review]** | By its nature, held by review or process (E5 or E6). |

---

## 1. The domains

The full list of aspects a native, cross-platform application framework
with server, web, sync, durable-execution, and tooling components has to
hold, grouped into the domains below.

| # | Domain | Prefix | What it includes |
|---|---|---|---|
| 2 | Architecture and boundaries | ARC | core/platform separation, dependency direction, platform groups, single responsibility, global state, one implementation per primitive |
| 3 | API design and ergonomics | API | naming, typestate, sealing, builders, defaults, `must_use`, secure-by-default shapes |
| 4 | Compatibility and versioning | CMP | semver, deprecation, MSRV, wire and persisted formats, host OS, browser, and API-level support |
| 5 | Syntax and style equivalence | EQV | builder = markup, typed style = utility = declaration |
| 6 | Native fidelity, capability honesty, portable contracts | NAT | real host controls, "decorate the box", capability tables, identical contract semantics on every backend |
| 7 | Correctness and determinism | COR | reconciliation, layout, invalidation, numerics, strings, time, randomness, identity |
| 8 | Stability | STB | panic policy, host-callback boundaries, signal handlers, error boundaries, teardown, crash capture |
| 9 | Reliability and resilience | REL | failure modes, timeouts, retries, idempotency, multi-instance safety, fail-closed decisions, offline |
| 10 | Error handling | ERR | typed errors, messages, discarded results, lost causes |
| 11 | Memory safety, unsafe, FFI | MEM | `unsafe` containment, safety proofs, typed accessors, FFI ownership, JNI/COM/GObject rules, Miri |
| 12 | Resource and lifetime management | RES | native objects, handles, subscriptions, leaks, census baselines |
| 13 | Concurrency and async | CON | thread affinity, task scopes, cancellation, reentrancy, locks and callouts |
| 14 | Data integrity and persistence | DAT | migrations, durable writes, state restore, paths from data, CRDT convergence |
| 15 | Security | SEC | secure defaults, untrusted input, code loading, URLs handed to the OS, local surfaces, secrets |
| 16 | Network services and listeners | NET | loopback is not a boundary, size before allocation, deadlines, accept loops, one choke point per check |
| 17 | Cryptography, keys, and signing | CRY | vetted primitives, typed key material, domain separation, rotation, pinning, code signing |
| 18 | Privacy | PRV | consent, telemetry, redaction, data minimization, platform privacy manifests |
| 19 | Supply chain and build integrity | SUP | dependencies, unused crates, licences, vendoring, CI hardening, toolchain pinning, provenance |
| 20 | Performance and resource footprint | PRF | budgets, latency, memory, artifact size, energy, build time, scalability |
| 21 | Accessibility | A11Y | host accessibility trees, roles, names, focus, live regions, contrast, assistive-technology passes |
| 22 | Internationalization and text | I18N | locales, plurals, RTL, bidi, IME, complex scripts, pseudo-localization |
| 23 | Visual and interaction quality | UX | theming, DPI, high contrast, reduced motion, adaptive layout, platform idioms |
| 24 | Developer experience | DX | CLI, dev loop, compile-time diagnostics, onboarding, configuration and environment variables, features |
| 25 | Documentation and examples | DOC | rustdoc, guides, both syntaxes, claims backed by tests and budgets, examples and templates, status honesty |
| 26 | Observability | OBS | tracing, metrics, logs, inspection protocol, diagnostics output |
| 27 | Testing and verification | TST | test levels, conformance, property, fuzzing, hostile-peer tests, Miri, goldens, determinism, coverage |
| 28 | Code quality and maintainability | QLT | formatting, lints, suppressions, naming, comments, module size, generated code, source hygiene, non-Rust languages |
| 29 | Version control and contribution workflow | VCS | commits, PRs, review, ownership, generated and binary files |
| 30 | CI and the build system | CI | gates, matrices, every check on every target, caching, timeouts, toolchains |
| 31 | Release, distribution, and operations | RLS | release notes, codemods, packaging, updates, rollouts, server operations |
| 32 | Portability, interoperability, extensibility | PRT | targets, `no_std`, embedding, adoption ladder, capability packages, mappers |
| 33 | Legal and regulatory | LEG | licences, DCO, vendored attribution, trademarks, CRA, accessibility law, data protection, store policies |
| 34 | Governance and community | GOV | conduct, roles, decision-making, RFCs, triage, AI-assisted contributions, status honesty, continuity |

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
- *Enforcement:* E3, the same graph test. **[Owed]** Group decisions: E5,
  CODEOWNERS on `docs/conformance/platform-groups.md`.

**ARC-4 — Dependency direction is one-way.** The allowed order is
types → core → style and markup → group crates → backends → host →
applications. A lower layer never names a higher one.
- *Enforcement:* E3, the graph test of ARC-1 **[Owed]**.

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
not count, so split only where a seam exists. Three files exceed it:
`rustnative-webgen/src/methods.rs` (3,051 lines),
`rustnative-tui/src/session.rs`, and `rustnative-core/src/node.rs`.
- *Enforcement:* `clippy::too_many_lines` at 200 lines per function
  (`clippy.toml`) **[Enforced]**. File size: an E3 report, warning only
  **[Owed]**.

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

**ARC-12 — Each primitive has exactly one implementation.** Some primitives
guard a security or durability property: HTML escaping, constant-time
comparison, the URL policy (SEC-13), durable atomic file writes (DAT-3),
path-component validation (DAT-6), origin checks (NET-5), and token parsing.
Each exists once, in the lowest crate all its callers can depend on, and
every backend and crate calls that one.
- *Why:* copies drift, and the drift is a vulnerability. The scan found
  constant-time comparison hand-written three times, the URL check in one
  backend out of five, and the WebSocket origin check in one of two entry
  points; each is now one function (`rustnative_core::secret::constant_time_eq`,
  `UrlPolicy`, and the live server's origin check). HTML escaping is still
  implemented twice (F-27), and durable writes exist in two versions, only
  one of which `fsync`s (F-21).
- *Enforcement:* E3, a duplicate-signature scan over these primitives (a
  second `fn escape`, a second `constant_time_eq`, a second atomic write)
  **[Owed]**. E5 today.

---

## 3. API design and ergonomics (API)

The public API is the product. Everything else is implementation.

**API-1 — Make invalid states unrepresentable.** When states permit
different operations, the states are types (`docs/conformance/typestate.md`).
When validity depends on runtime data, the function returns `Result`.
Prefer enums over boolean pairs, newtypes over bare integers for
identifiers, and constructors that validate over public fields.
- *Enforcement:* E5 with the typestate guide **[Review]**.

**API-2 — Public items are documented and exemplified.** Every public item
has rustdoc. Every major entry point has a runnable example. Fallible
functions have `# Errors`, panicking ones have `# Panics`, and `unsafe fn`
has `# Safety`.
- *Enforcement:* E2. `missing_docs` is denied in every library crate, and
  `rustnative-conformance/tests/missing_docs.rs` (E3) fails when a
  `crates/*/src/lib.rs` drops the attribute. `missing_errors_doc` and
  `missing_panics_doc` are denied in CI by `-D warnings`. **[Enforced]**

**API-3 — Follow the Rust API Guidelines.** Naming (`as_`/`to_`/`into_`,
`with_*` modifiers, `try_*` for fallible variants), standard traits
(`Debug` on every public type, `Clone`/`PartialEq`/`Eq`/`Hash`/`Default`
where meaningful), conversions through `From`/`TryFrom`, and iterators
rather than index loops in the API.
- *Enforcement:* E2 through `clippy::pedantic` **[Enforced]**, plus
  `missing_debug_implementations`, which finds 114 public types without
  `Debug` (`rustnative-server` 17, `-sync` 14, `-core` 13) **[Owed]**.

**API-4 — Design for additive evolution.** Public enums that may grow are
`#[non_exhaustive]`. Traits implemented only by the framework are sealed.
Structs that may gain fields keep private fields and a constructor or
builder. Handles are opaque. These choices are written in a sealing policy
(`PLAN.md` Milestone 65, `C128`) and applied before 1.0.
- *Why:* 220 of 257 public enums are exhaustive, `Node` and `NodeKind`
  among them. Every node kind added under EQV-1 is therefore a breaking
  change for any application that matches on a node. None of 71 public
  traits is sealed, and 308 public structs expose their fields (F-38).
- *Enforcement:* E4 with the API snapshot (CMP-2) **[Owed]**, plus E3: a
  scan listing public enums and traits without a recorded decision
  **[Owed]**.

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
name or its docs. A function that panics in some execution context says so.
`Chunks::send` uses Tokio's `blocking_send`, which panics when called from
async code (F-36). The UI thread never calls a blocking function (CON-4).
- *Enforcement:* E5 **[Review]**.

**API-8 — Generic over concrete only when the caller gains.** Each generic
parameter, trait object, or `async-trait` use has a reason the caller can
see. Monomorphization cost counts against the artifact-size budgets
(PRF-3).
- *Enforcement:* E5, and E3 through the budgets **[Partial]**.

**API-9 — One way to do a thing.** A new API that duplicates an existing
one either replaces it, with a deprecation (CMP-3), or is refused. The two
syntaxes of EQV are the deliberate exception, and they produce one tree.
- *Enforcement:* E5 **[Review]**.

**API-10 — The secure sequence is one call.** When correctness or security
needs operations in a fixed order (renewing the session id on sign-in,
verifying before trusting, checking the origin before upgrading), the API
offers one function that performs the whole sequence, and that function is
what examples and templates use. A safety step left to the caller's memory
is an API defect.
- *Why:* the session module documents that sign-in must call
  `session.renew()`. Yet the authentication kit every new application is
  scaffolded from (`crates/rustnative/templates/kits/auth.rs`) and both
  server examples set the principal without renewing (F-16).
- *Enforcement:* E1, the one-call API (`Session::sign_in(principal)`),
  plus E3, a template test that signs in and asserts a new session id
  **[Owed]**.

---

## 4. Compatibility, versioning, and stability of contracts (CMP)

The stability policy is `docs/policy/stability.md`. These standards make it
binding on each change.

**CMP-1 — Semantic versioning across every contract.** The covered surface
is every published crate's public API, the markup syntax, the style
spellings, `rustnative.toml`, the `rustnative` command line, the
environment variables (DX-4), the inspection protocol, and the wire
formats. Before 1.0 a minor release may break the surface. A patch release
never does.
- *Enforcement:* E4 via CMP-2 **[Owed]**.

**CMP-2 — The public API is a committed artifact.** A snapshot of each
crate's public items, next to `docs/api/framework.json`, is regenerated in
CI. A diff larger than the declared version bump allows fails the build.
`cargo-semver-checks` (242 lints at the end of 2025) runs on every
published crate.
- *Why:* a breaking change must be impossible to land by accident, not
  merely discouraged.
- *Enforcement:* E4 **[Owed]** (`PLAN.md` Milestone 65, `C128`).
  `framework.json` staleness is **[Enforced]** by
  `crates/rustnative/tests/describe.rs`.

**CMP-3 — Deprecate before removing.** An item marked for removal is
`#[deprecated(since, note)]`, names its replacement, and stays for at least
two minor releases. The markup and style compilers warn on deprecated
elements, attributes, and classes the same way.
- *Enforcement:* E2 (`deprecated` warns, and `-D warnings` in CI makes
  internal uses fail) **[Enforced]**. The window length: E5.

**CMP-4 — Every breaking change ships a codemod.** `rustnative upgrade`
carries a codemod with corpus cases in
`crates/rustnative/tests/codemod-corpus/`. It is idempotent and reports
every position it cannot rewrite with certainty. A breaking change with no
codemod is a release blocker unless no rewrite is possible.
- *Enforcement:* E3, the corpus tests **[Enforced]**. Presence of a codemod
  for each break: E5 **[Review]**, E4 once CMP-2 exists.

**CMP-5 — MSRV is declared, checked, and raised deliberately.**
`rust-version` (1.85) is checked on every change. Raising it is a minor
release before 1.0 and a major one after, and the release notes say so.
- *Enforcement:* E3, the `msrv` CI job **[Enforced]**.

**CMP-6 — Wire and persisted formats are versioned.** Every format that
crosses a process, a device, or time carries a version: the sync and
inspection protocols, server-function calls, saved application state,
durable workflow histories, update manifests, sealed session cookies, and
migrations. Readers accept every version still in the support window. An
old reader meeting a newer version fails with a typed error, never with a
misparse.
- *Enforcement:* E3, golden fixtures for each supported version, decoded
  by the current reader **[Partial]**: `wire.rs` conformance tests exist,
  and a fixture for every format and version is **[Owed]**.

**CMP-7 — Host support is a table, not a hope.** Each backend states the
oldest OS, toolkit, API level, and browser it supports
(`docs/linux/desktop-matrix.md` is the model). Dropping one is a minor
release with a release note. Using an API newer than the floor requires a
runtime check and a fallback or a capability answer.
- *Enforcement:* E3, CI on the floor where a runner exists (Android API
  levels, Linux distributions) **[Partial]**. E5 elsewhere.

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
`Node` values, with the markup compiled from a `.rsx` file and through
`rsx!`.
- *Enforcement:* E3, `rustnative-conformance/tests/syntax_equivalence.rs`,
  and E4, `framework.json` **[Enforced]**. Detecting a builder method with
  no attribute is **[Partial]**.

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
file or the `rsx!` call, at a compiler's level of quality. It never panics:
the `rsx!` front end held through 100,000 generated token sequences
without one.
- *Enforcement:* E3, trybuild `.stderr` goldens in
  `rustnative-conformance/tests/ui` **[Enforced]**. A new diagnostic adds a
  case.

**EQV-7 — Documentation shows both.** Every tree in prose appears in both
syntaxes, labelled and side by side, and every styled example shows both
spellings, unless an exemption comment gives the reason.
- *Enforcement:* E3, `rustnative-conformance/tests/doc_parity.rs`
  **[Enforced]** for `README.md`, `PLAN.md`, `docs/guides/`, and runnable
  doc examples. Extending it to every `docs/` page is **[Owed]**.

---

## 6. Native fidelity, capability honesty, and portable contracts (NAT)

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
  elsewhere **[Partial]**. `docs/idioms/` holds Windows only, so the other
  hosts are **[Owed]**.

**NAT-7 — A portable contract has portable semantics.** A service contract
in the core states its observable behaviour: limits, timeouts, redirects,
errors, and what it refuses. Every backend implements exactly that. A
backend may realize the contract in its host's way, but it may not change
what an application sees.
- *Why:* `HttpService` has a 30-second timeout on Android and the host
  library's defaults on Windows and Linux, and no backend bounds the
  response body (F-24). `SystemService::open_url` checked URLs on one
  backend of five until one `UrlPolicy` was given to all of them (SEC-13).
- *Enforcement:* E3, a conformance suite per service contract in
  `rustnative-conformance`, run on every backend, as the UI guarantees are
  **[Owed]**.

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
  Raw `as` casts remain at 255 sites on Linux, plus 169 in the Windows
  backend and 42 in the Android backend. Promoting
  `clippy::as_conversions` for backend FFI modules is **[Owed]**.

**COR-5 — Time comes from a `Clock`.** No portable production code reads the
system clock directly. Tests use the deterministic clock and executor.
Timeouts and backoff are expressed in clock time.
- *Enforcement:* E1, through the `Clock` and `Executor` contracts, plus a
  source scan for `Instant::now()`/`SystemTime::now()` outside backends,
  clock implementations, and test modules **[Owed]**. The scan found no
  production violation in the core.

**COR-6 — Randomness is explicit.** Security randomness comes from the OS
CSPRNG. Non-security randomness is seeded and injectable so tests replay.
Nothing on the render path is random.
- *Enforcement:* E5, plus a scan forbidding non-CSPRNG sources in
  `rustnative-server/src/auth` and `security.rs` **[Owed]**.

**COR-7 — Iteration order that reaches output is defined.** Anything that
reaches output (markup, generated CSS or JS, artifacts, the SBOM, goldens)
iterates in a defined order: `BTreeMap`, sorted vectors, or insertion
order. `HashMap` iteration never reaches output.
- *Why:* reproducible builds (SUP-7) and stable goldens depend on it.
- *Enforcement:* E3, goldens and the reproducibility check **[Partial]**.

**COR-8 — Floating point is handled deliberately.** Comparisons use a stated
tolerance or total ordering (`total_cmp`). NaN cannot enter layout
(constructors reject or clamp it). Colour and unit conversions are tested
at their boundaries.
- *Enforcement:* E2, `clippy::float_cmp` (pedantic) **[Enforced]**, plus E3
  conversion tests. The style parsers held through 900,000 generated
  inputs (`1e309`, `NaN`, nested functions) without a panic.

**COR-9 — Merge functions converge.** Every CRDT and sync merge is
commutative, associative, and idempotent. Proptest proves it for every type
that merges.
- *Enforcement:* E3, `rustnative-sync/tests/crdt.rs` with proptest
  **[Enforced]** for existing types. A new mergeable type adds its property.

**COR-10 — Strings are sliced only at character boundaries.** Byte-index
slicing of a `str` (`&s[..n]`) is allowed only where `n` comes from the
same string's `char_indices`, `find`, or a check that the prefix is ASCII.
Untrusted or user text is sliced with `get(..n)`, `strip_prefix`, or
grapheme segmentation.
- *Why:* the scan's fuzzer found five panics in the terminal backend's
  URL check. Every one came from `url[..scheme.len()]` on non-ASCII input,
  so `"mailto:€"` crashed the backend. Clippy found 124 such slices in
  non-test code.
- *Enforcement:* E2, `clippy::string_slice` at warn in
  `[workspace.lints.clippy]` and denied in CI by `-D warnings`; each
  remaining site carries an `#[allow]` naming why its index is a boundary
  **[Enforced]**.

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
  **[Enforced]** `clippy::panic` (17 sites outside tests, most in build
  scripts and harnesses) and `indexing_slicing` (685 sites) are allowed
  today. Promoting `panic` with reasons is **[Owed]**.

**STB-2 — Every host-callable function enters through the boundary.** Every
function the host calls catches unwinding before it can leave Rust: a
window procedure, a subclass procedure, a JNI native, a GObject signal, a
COM method, a WASI export, a console control handler, or an exception
filter. Each backend has one boundary helper, and every such function calls
it as its first act. Since Rust 1.81 a panic that reaches an `extern "C"`
or `extern "system"` function aborts the process, skipping every teardown
STB-3 promises.
- *Why:* the scan enumerated all 47 host-callable functions. The main entry
  points are guarded: every Android JNI native goes through
  `jni_host::guard`, and Windows window procedures go through their
  boundary. About 15 secondary callbacks are not: Windows' padding
  `subclass_proc`, which does unchecked arithmetic on a `RECT`,
  `surface_proc`, the single-instance `listener_proc`,
  `notification_host_proc`, the crash `on_exception`, and the terminal's
  console `on_control` (F-31).
- *Enforcement:* E3, a scan that every `extern "system"`/`extern "C"`
  function body outside an extern block calls its crate's boundary helper,
  with an allowlist for signal handlers (STB-9) **[Owed]**. Conformance row
  10 (a deliberate panic restores the host) **[Enforced]** for the main
  entry points.

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
- *Enforcement:* E2, `clippy::exit` in library crates **[Owed]**. No
  library crate calls it today.

**STB-7 — Release health gates rollouts.** Crash-free sessions, startup
failures, hangs, and frame drops have declared thresholds. A staged rollout
halts on a breach and reverts on a severe one (`PLAN.md` Milestone 65,
`C132`).
- *Enforcement:* E3/E6 **[Owed]**.

**STB-8 — Allocation size is never attacker-chosen.** Rust aborts the
process when an allocation fails, so a length that came from outside the
process (a network header, a file, IPC) is checked against a stated limit
before it sizes an allocation. See NET-2 for listeners. This standard
covers files, IPC, and every decoder.
- *Why:* a peer's declared length can be anything its encoding allows:
  256 MiB for an MQTT packet. Sixteen 5-byte connections aborted a broker
  that trusted it, under a 3 GiB address-space limit.
- *Enforcement:* E3, fuzz targets with an allocation-limited harness
  (TST-6) **[Owed]**.

**STB-9 — Signal handlers are async-signal-safe.** A signal handler only
writes to a pipe or sets an atomic. It saves and restores `errno`, and may
call `_exit`. It never allocates, locks, logs, or unwinds. The terminal
backend's `on_signal` (`rustnative-tui/src/term/unix.rs`) is the model.
- *Enforcement:* E5, with the model as the reference **[Review]**.

---

## 9. Reliability and resilience (REL)

**REL-1 — Every wait has a bound.** Network calls, IPC, child processes,
locks across processes, emulator and device steps, and browser automation
all have a timeout. The timeout is chosen, documented, and surfaced as a
typed error.
- *Enforcement:* E3, the per-contract conformance suite of NAT-7 and the
  listener tests of NET-8 **[Owed]**. CI jobs: CI-5.

**REL-2 — Retries are bounded, jittered, and safe.** Retries use bounded
attempts and exponential backoff *with jitter*, and retry only operations
that are idempotent or carry an idempotency key. The job queue backs off
`2^n` seconds with no jitter, so a failure shared by many jobs retries them
in lockstep (F-34).
- *Enforcement:* E3, durable-execution and sync tests **[Partial]**.

**REL-3 — Effects that must happen once are idempotent or transactional.**
Background jobs, queued offline calls, durable workflow steps, and server
functions that change state take idempotency keys or run as exactly-once
transactional steps. Poison messages go to a dead-letter queue.
- *Enforcement:* E3, `rustnative-durable/tests/durable.rs` and the offline
  replay tests **[Enforced]** for existing paths.

**REL-4 — Degrade, don't fail, when a capability is missing.** A component
that uses an optional capability checks for it and has a defined fallback.
A missing capability never crashes and never no-ops silently.
A host dependency that can abort the process when it is missing or broken
(a toolkit plug-in, a codec, a portal) is probed before first use, and its
absence is answered as an unavailable capability (NAT-3).
- *Why:* on Linux, a media node becomes `gtk::Video`, and GTK's GStreamer
  module calls `g_error` when the `playbin` element is not installed. That
  aborts the process: the GTK backend's whole test binary died with
  `SIGTRAP` on a machine with GTK's media module but without
  `gstreamer1.0-plugins-base` (F-41). The same machine running an
  application would crash it.
- *Enforcement:* E3, headless tests with capabilities withheld
  **[Partial]**, plus a Linux test on a session without the plug-in
  **[Owed]**.

**REL-5 — Offline and constrained networks are first-class.** Code that
talks to a network states its behaviour when offline, on high latency, and
on metered connections. Queued work survives a restart.
- *Enforcement:* E3, `offline_browser.rs` and sync tests **[Partial]**.

**REL-6 — Lifecycle events are honoured.** Suspend, resume, low memory,
process death and restore, configuration change, and window close each have
a defined path. State is restorable after process death on the hosts that
kill processes.
- *Enforcement:* E3, `lifecycle_conformance.rs` and
  `tools/android-lifecycle-test.sh` **[Enforced]**.

**REL-7 — Untrusted peers cannot exhaust a service.** Network services are
held to section 16 (NET). This standard covers the remaining channels the
same way: files the person opens, IPC, clipboard and drag data, and
packages.
- *Enforcement:* E3, fuzzing with limits (TST-6) **[Owed]**.

**REL-8 — Nothing assumes it is the only instance.** Recovery, migration,
scheduling, and cache logic is correct when several processes share a
database, a directory, or a port range. Work is claimed with a lease
carrying an owner and an expiry, and only expired leases are reclaimed.
- *Why:* the job queue's constructor runs
  `UPDATE _jobs SET status = 'queued' WHERE status = 'running'`. A second
  instance starting against the same database therefore re-runs jobs a
  live instance is executing (F-33).
- *Enforcement:* E3, a two-instance test per shared-state component
  **[Owed]**.

**REL-9 — Policy and security decisions fail closed.** When the input to a
decision cannot be read (a failed system call, a JNI exception, a parse
error), the decision is the restrictive one: refuse the update, deny the
grant, treat the request as unauthenticated.
- *Why:* the Android updater forbids self-update on Play installs by
  checking the installer package name. The Java helper returns `null` on
  any exception, and a JNI failure gives no name either, so a failed check
  reads as "not from Play" and self-update proceeds (F-17). The Linux HTTP
  client enforces certificate pins by refusing when it *sees* a handshake
  with the wrong certificate. Behind an HTTPS proxy that answered the
  tunnel with `403`, no handshake happened, the check never ran, and the
  proxy's response was returned as the pinned host's (F-42). A security
  check must accept on a positive verdict, never on the absence of a
  negative one.
- *Enforcement:* E3, a fault-injection test per decision (make the input
  unreadable and assert refusal) **[Owed]**. E5 today.

---

## 10. Error handling (ERR)

**ERR-1 — Errors are typed.** Public fallible functions return a crate error
type: an enum, `#[non_exhaustive]`, implementing `std::error::Error` and
`Display`, with `source()` chaining. `Result<_, String>` and
`Box<dyn Error>` are not used in public APIs. There are 134
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
file and position, the rule broken, and the fix. Client-facing server
errors carry no internal detail (SEC-18).
- *Enforcement:* E3, CLI and trybuild golden outputs **[Partial]**. E5.

**ERR-3 — Results are not discarded silently.** `let _ =` on a `Result`,
`.ok()` used as a statement, and an empty `Err(_) => {}` are allowed only
with a comment saying why the failure does not matter. Best-effort cleanup
in `Drop` is the common legitimate case.
- *Why:* there are 439 discarded `must_use` values and 12
  `.ok()`-as-statement sites outside tests. The latter are all Cairo
  `save`/`restore`/`fill` results in the Linux canvas.
- *Enforcement:* E2, `clippy::let_underscore_must_use` and
  `clippy::unused_result_ok` at warn, each remaining site with a reason
  **[Owed]**.

**ERR-4 — Fail at the boundary, once.** Validate input where it enters
(parse, don't validate, and keep the validated type). Inner code takes the
validated type and does not re-check.
- *Enforcement:* E1 where typestate applies (`Validated<T>`). E5 elsewhere
  **[Partial]**.

**ERR-5 — A mapped error keeps its cause.** `map_err(|_| …)` that drops the
original error is allowed only when the cause is secret or meaningless to
the caller, with a comment. Otherwise the new error carries the old as its
`source()` or in its message. There are 62 cause-dropping maps, 21 of them
in `rustnative-server`.
- *Enforcement:* E2, `clippy::map_err_ignore` at warn **[Owed]**.

---

## 11. Memory safety, unsafe code, and FFI (MEM)

The workspace holds about 970 `unsafe` blocks, most of them in
`rustnative-windows` (753), `-linux` (77), and `-tui` (72), with about 980
`SAFETY:` comments. Miri found no undefined behaviour in the crates it can
run (`rustnative-types`, `-drawlist`, `-i18n`, `-interop`, including the
interop runtime's reentrancy and panic-containment tests). The discipline
is strong, and these standards keep it that way.

**MEM-1 — `unsafe` lives only where the host requires it.** Backends, FFI
bridges, and named low-level modules hold it. A crate with no need for it
declares `#![forbid(unsafe_code)]`. Eighteen crates have no `unsafe` and
do not forbid it: `rustnative-components`, `-conformance`, `-data`,
`-drawlist`, `-durable`, `-headless`, `-host`, `-i18n`, `-macros`,
`-markup`, `-observe`, `-server-macros`, `-style`, `-sync`, `-types`,
`-update`, `-web-macros`, and `-web-testing`.
- *Enforcement:* E2, `#![forbid(unsafe_code)]` per crate **[Owed]**.

**MEM-2 — Every `unsafe` block carries its proof, one operation at a time.**
A `// SAFETY:` comment immediately above the block states the invariant
that makes it sound: why pointers are valid, why lifetimes hold, which
thread, and who owns the result. A block holds one unsafe operation, so
each proof matches one claim. "Safe because we checked" is not a proof.
- *Why:* 58 blocks perform several unsafe operations under one comment (42
  in `rustnative-windows`, 8 in `-linux`, 6 in `-tui`, 1 in `-android`, 1
  in the CLI), and there a proof can cover the first operation and not the
  others.
- *Enforcement:* E2, `clippy::undocumented_unsafe_blocks = "deny"` and
  `unsafe_op_in_unsafe_fn = "deny"` **[Enforced]**, plus
  `clippy::multiple_unsafe_ops_per_block` **[Owed]**. Proof quality: E5,
  CODEOWNERS on backend crates **[Owed]**.

**MEM-3 — `unsafe impl Send`/`Sync` is argued, not assumed.** Each has a
`SAFETY:` comment naming the thread contract of the wrapped resource. All
nine in the workspace have one, and clippy flags one (`OwnerSlot<T>` in
`rustnative-interop`) whose generic field it cannot verify. That one needs
a `T: Send` bound or a proof of why `T` never crosses.
- *Enforcement:* E2, `clippy::non_send_fields_in_send_ty` **[Owed]**.

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
cites the documented signature it relies on. One exists, in
`rustnative-windows/src/native/accelerator.rs`.
- *Enforcement:* E2, `clippy::transmute_ptr_to_ptr` and
  `missing_transmute_annotations` **[Enforced]**. E5.

**MEM-6 — The unsafe surface is checked by tools, not only by eyes.** Miri
runs in CI over every crate it can execute. AddressSanitizer and
ThreadSanitizer run over each backend's FFI tests where the target supports
them (`PLAN.md` Milestone 65, `C130`).
- *Enforcement:* E3 **[Owed]**. Miri is clean on four crates today, so
  adopting it costs a nightly job and no fixes.

**MEM-7 — Non-Rust bridge code holds the same bar.** The Java host library,
the JavaScript runtime, C and C# adoption hosts, and shell scripts follow
QLT-10. JNI calls check and clear pending exceptions. Java catches the
narrowest exception type. `catch (Throwable)` is never used, because it
swallows `OutOfMemoryError`, `StackOverflowError`, and linkage errors.
`RnPosture` has three such catches, and `RnServices` catches bare
`Exception` around the installer decision of REL-9.
- *Enforcement:* E3, the Android device suite **[Partial]**, plus
  `javac -Xlint:all -Werror` and an Error Prone or PMD rule for broad
  catches **[Owed]**.

**MEM-8 — Environment mutation is single-threaded.** `std::env::set_var`
and `remove_var` (`unsafe` in edition 2024) are called only before any
thread starts, with a `SAFETY:` comment saying so. Prefer `Command::env`
for children.
- *Enforcement:* E2, since edition 2024 makes them `unsafe`
  **[Enforced]**.

**MEM-9 — Each unsafe invariant has one typed accessor.** When a raw slot,
pointer, or handle carries an invariant, every access goes through one
narrow, typed function that documents it once. Raw casts are not scattered
across call sites. `rustnative-windows/src/native/user_data.rs`, which puts
the window's single untyped `GWLP_USERDATA` slot behind `RuntimeSlot` and
`BackgroundColorSlot`, is the model.
- *Enforcement:* E5 **[Review]**.

**MEM-10 — Ownership transfer is explicit; `mem::forget` is not a tool.** A
guard whose resource passes to the host is disarmed through a named method
(`into_raw`, `disarm`) whose doc says who now owns the resource.
`mem::forget` is not used on anything holding a resource.
- *Why:* `rustnative-linux/src/desktop/notifications.rs` forgets a D-Bus
  signal subscription for every notification sent, which leaks one
  subscription and its closure per notification for the life of the
  process (F-19). `rustnative-windows/src/native/menu.rs` uses it as an
  ownership transfer, which is correct but reads the same way.
- *Enforcement:* E2, `clippy::mem_forget` at deny, with reasons for
  transfers **[Owed]**.

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
files, child processes, threads, and subscriptions are released by a
guard's `Drop`, on every path, panics included. Threads are joined and
children are waited on.
- *Enforcement:* E3, lifetime tests **[Partial]**. E5.

**RES-4 — Caches are bounded.** Every cache (style objects, fonts, the
response cache, image decodes, glyph tables, rate-limit buckets, challenge
stores) has a size or entry bound and an eviction policy that keeps the
entries still doing work.
- *Enforcement:* E3, unit tests per cache **[Partial]**.

**RES-5 — Stale events are rejected.** An event for a destroyed object or a
superseded generation is dropped, never delivered to a reused identity.
- *Enforcement:* E3, stale-event tests per backend **[Enforced]**.

**RES-6 — Every registration has an owner that unregisters it.** A signal
subscription, callback registration, timer, or listener is held by an
object whose drop unregisters it. Registrations made per event (one per
notification, per request) are counted in the census of RES-2, so a leak
fails a test.
- *Enforcement:* E3, a census test that sends many notifications and
  asserts the subscription count is flat **[Owed]** (F-19).

---

## 13. Concurrency and async (CON)

**CON-1 — Thread affinity is enforced by type where possible.** Tree-holding
types are `!Send`. UI-thread APIs take `&UiThread`. Where the type system
cannot express affinity, a debug assertion does.
- *Enforcement:* E1, `rustnative-core/tests/affinity.rs` **[Enforced]**.

**CON-2 — Async work is owned by a task scope.** A task belongs to a
component's scope and is cancelled when it unmounts. No message is
delivered after unmount (`PLAN.md` 2.12).
- *Enforcement:* E3, `suites::no_message_after_unmount` and the
  cancellation property test **[Enforced]**.

**CON-3 — Workers never mutate component state.** Results return as
messages through the scheduler.
- *Enforcement:* E1, since component state is not reachable from `Send`
  contexts **[Enforced]**.

**CON-4 — The UI thread never blocks.** No synchronous I/O, sleeping,
blocking channel receive, or contended lock on the UI thread. Host modal
loops keep the framework running.
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
`.await`. Where two locks nest, their order is documented. `parking_lot`
and `std` locks are not mixed in one subsystem.
- *Enforcement:* E2, `clippy::await_holding_lock` and
  `await_holding_refcell_ref` **[Enforced]**, plus
  `clippy::significant_drop_tightening`, which finds 33 guards held past
  their last use **[Owed]**. Lock order: E5.

**CON-7 — Channels are bounded unless proven otherwise.** An unbounded
channel needs a comment showing why its producer is bounded. There are 19
unbounded channels outside tests.
- *Enforcement:* E3, a scan with an allowlist **[Owed]**.

**CON-8 — The executor is an abstraction.** Core code uses the `Executor`,
`LocalExecutor`, and `Clock` contracts, never a specific runtime's global.
Tokio is used behind them.
- *Enforcement:* E5 **[Review]**, plus ARC-1's scan for `tokio::spawn` in
  the core **[Owed]**.

**CON-9 — Never call out while holding a lock.** No callback, waker,
exporter, host function, or application code is invoked while a lock is
held. Copy what is needed, release the guard, then call. A temporary in an
`if let` or `match` scrutinee lives to the end of the block, so
`if let Some(f) = mutex.lock().clone() { f() }` calls `f` *under* the lock.
Bind the value with `let` first.
- *Why:* the core scheduler invokes the host's waker in exactly that form
  (`scheduler/mod.rs` and `supervise.rs`), and the tracer calls the
  exporter from `Drop` while holding the span record's lock
  (`rustnative-observe/src/trace.rs`) (F-30). A waker or exporter that
  takes the same lock deadlocks.
- *Enforcement:* E2, `clippy::significant_drop_in_scrutinee` at deny
  (6 sites) **[Owed]**.

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
- *Enforcement:* E3, fixtures for each persisted format **[Owed]**.

**DAT-3 — Writes are atomic and durable, through one implementation.**
Files are written to a temporary file in the same directory, `fsync`ed,
renamed, and the directory is `fsync`ed. Multi-step database changes run in
one transaction. A crash or power loss at any point leaves the old state or
the new one, never a mix or an empty file. There is one such function
(ARC-12).
- *Why:* `rustnative-core/src/services/file_state.rs` does all of this.
  `rustnative-update`'s `write_atomically`, which records update state,
  renames without any `fsync` (F-21).
- *Enforcement:* E3, a power-cut test (`RUSTNATIVE_POWER_CUT_DIR` exists for
  the core's version) for every durable writer **[Partial]**.

**DAT-4 — Queries are typed and bound.** SQL uses bound parameters only.
`query!` is checked against the schema at compile time. Identifiers that
must be interpolated come only from the model, typed (SEC-17).
- *Enforcement:* E1/E2, the macro and compile-fail doc tests
  **[Enforced]**, except `RowPolicies`, which takes raw `&str` fragments
  (F-28) **[Partial]**.

**DAT-5 — Data the person owns is portable.** Export and deletion paths
exist for personal data the framework stores (`PLAN.md` Milestone 63).
- *Enforcement:* E3 **[Partial]**.

**DAT-6 — A path built from external data is one validated component.** A
name that arrives from a manifest, a request, a package, or IPC becomes a
path only after it is checked as a single normal component: no separators,
no `..`, no prefix, no root. `Path::join` with an absolute path replaces
the base, so an unchecked join can escape the directory. The dev agent does
this right (`Path::file_name`). `rustnative-update`'s `model_directory`
joins a manifest-supplied name unchecked (F-22).
- *Enforcement:* E1, a `PathComponent` type with a validating constructor
  used by every such join, plus E3 tests with `..`, `/`, `\`, `C:`, and
  absolute names **[Owed]**.

---

## 15. Security (SEC)

The threat models are in `docs/security/` and the server checklist is in
`docs/server/security-checklist.md`. These standards apply to every
contribution, not only to security work. Listeners are in NET, and
cryptography in CRY.

**SEC-1 — Secure by default, insecure by name.** Every protection in the
server checklist is on unless an application turns it off. Shipping without
one is a defect. An opt-out is named for what it gives up.
- *Enforcement:* E3, one named test per checklist row **[Enforced]**.

**SEC-2 — Every new entry point updates the threat model.** A new listener,
file format, IPC channel, deep-link scheme, plugin surface, service grant,
or developer tool that accepts input adds its row (entry point, threat,
mitigation, holding test) to the relevant `docs/security/threat-model-*.md`
in the same change. Developer tooling is in scope. The local deploy
control API, the dev agent, and the inspection server are the surfaces
where the scan found its most serious defects, none of which had a
threat-model row; their rows are now in `threat-model-server.md` and
`threat-model-windows.md`.
- *Enforcement:* E5, CODEOWNERS on listeners and parsers **[Owed]**, plus
  E3: a test that every listener bound in `crates/` appears in a threat
  model **[Owed]**.

**SEC-3 — Untrusted input is parsed defensively.** Network messages, files
the person opens, deep links, clipboard and drag data, attributes on custom
elements, environment variables, manifests, and packages are untrusted. Use
typed, length-checked parsers that reject what they do not understand,
never truncate it (the passkey CBOR reader silently caps arrays and maps at
1,024 entries and misreads what follows, F-29). Never evaluate input as
code or markup. Parse risky formats in an isolated worker where the host
allows it (`IsolatedWorker` on Windows).
- *Enforcement:* E3, fuzzing (TST-6) **[Owed]**. Isolation tests
  **[Enforced]** on Windows.

**SEC-4 — Output is escaped by construction.** HTML text and attributes are
escaped by the renderer. Raw markup is only a `&'static str` in source. JSON
in pages escapes `<`, `>`, and `&`. Shell and SQL never take interpolated
input. Text written to a terminal has its control characters removed (the
terminal backend's `Grid::print` and title encoder do this).
- *Enforcement:* E1, `Html` types and the `query!` macro **[Enforced]**.

**SEC-5 — Authorization is in the type of every route.** A route with no
stated access (`public`, `signed_in`, `authorized::<Policy>`) does not
compile. Data reads go through row policies when declared. Server functions
re-check access and never trust client state.
- *Enforcement:* E1, compile-fail doc test **[Enforced]**.

**SEC-6 — Every resource an attacker can consume is bounded.** Request
bodies, message sizes, nesting depth, connection count, handshake time,
decompression ratio, image dimensions, and fuel and memory on edge hosts
are bounded, with sizes checked before allocating (STB-8, NET-2). Parsers
that recurse bound their depth, as the passkey CBOR reader does (16
levels) and `serde_json` does (128).
- *Enforcement:* E3, a limits test per parser and listener, and fuzzing
  **[Partial]**.

**SEC-7 — Cryptography follows CRY.** Section 17 holds the rules. A change
to cryptographic code needs two maintainer approvals.
- *Enforcement:* E5, CODEOWNERS **[Owed]**.

**SEC-8 — Code is loaded only from trusted locations.** Native libraries are
loaded by full path or with a restricted search. On Windows that means
`LoadLibraryExW` with `LOAD_LIBRARY_SEARCH_SYSTEM32` for system DLLs
(as `accelerator.rs` loads `dxcore.dll`), never bare `LoadLibraryW`, which
searches the application's directory and the current one first. Child
processes are started by absolute path or a resolved
toolchain path, never through a shell with interpolated arguments.
- *Enforcement:* E3, a scan forbidding `LoadLibraryW`/`LoadLibraryA` and
  `Command::new("sh"|"cmd")` with dynamic arguments outside tests
  **[Owed]**.

**SEC-9 — Secrets never print, log, serialize, or ride on a command line.**
Secrets are `Secret<T>`. There is no global configuration holding them.
Test fixtures use obviously fake values. Credentials never appear in URLs,
span names, query strings, panics, error messages, or another process's
argument list. A tool that needs a secret reads it from an environment
variable or file descriptor it names, as the Android packager does.
- *Why:* the Windows packager passes the PFX password to `signtool` as
  `/p <password>`. Every process listing and endpoint log on the build
  machine can see it, although the module's own doc says the tool never
  holds a password (F-25).
- *Enforcement:* E1 `Secret<T>` **[Enforced]**, plus E3 secret scanning in
  CI with push protection **[Owed]**, plus E3 tests that no spawned
  command's arguments contain a value read from a password variable
  **[Owed]**.

**SEC-10 — Updates and packages are verified before use.** Update manifests
are Ed25519-signed with the key pinned at build, checked against the
application id, expiring, and digest-verified before anything switches.
Packages never unpack outside their directory, and downgrades are refused.
Signed fields that become paths are validated anyway (DAT-6).
- *Enforcement:* E3, the update and package tests **[Enforced]**. The
  update-manifest parser held through 300,000 generated inputs without a
  panic.

**SEC-11 — Operational and debug surfaces are off, local, and
authenticated.** Inspection is off unless enabled, loopback-only, and
tokened, and its token is protected as SEC-14 requires. `/healthz` and
`/metrics` are feature-gated and documented as unauthenticated. Admin,
deploy, and dev-agent APIs require a credential (NET-1).
- *Enforcement:* E3: the inspection server, the dev agent, and the deploy
  control API each refuse a caller without their per-run token, with tests
  (`rustnative-core/tests/inspection.rs`, `rustnative/tests/dev.rs`,
  `rustnative-server/tests/deploy.rs`) **[Enforced]**.

**SEC-12 — Vulnerabilities are handled privately and on time.** Reports go
through `SECURITY.md`. They are acknowledged within five working days. The
fix and advisory are coordinated with the reporter. A fixed vulnerability
gets a regression test, an advisory (and a RUSTSEC entry for published
crates), and a release note.
- *Enforcement:* E6 **[Review]**.

**SEC-13 — URLs and paths handed to the operating system pass one portable
policy.** Before any backend gives the OS a URL to open, the URL passes
the core's policy: an allowed scheme (`http`, `https`, `mailto`, plus the
application's own registered schemes), no control characters or
whitespace, and no local paths or UNC shares unless the application opted
in explicitly. An OS "open" call launches whatever handler is registered,
and a protocol handler or `\\host\share\x.exe` is code execution.
- *Why:* `ShellExecuteW` runs a local executable, a UNC path, or any
  protocol handler (`ms-msdt:`, `search-ms:`), and GIO, the OpenURI portal,
  and Android's `ACTION_VIEW` open any scheme an installed application
  claims. The scan found only the terminal backend checking, with a checker
  that panicked on non-ASCII.
- *Enforcement:* E3, `rustnative_core::UrlPolicy`, which the Windows,
  Linux, Android, terminal, and headless backends' `open_url` call before
  anything reaches the OS, with refusal and property tests in
  `rustnative-core` and per backend **[Enforced]**. The same refusals in
  the NAT-7 conformance suite are **[Owed]**.

**SEC-14 — Local secrets and executables never live in shared
directories.** A token, key, endpoint file, socket, or executable the
framework writes goes in a per-user directory (`$XDG_RUNTIME_DIR`, the
user's cache directory, `%LOCALAPPDATA%`). The directory is created `0700`
and verified for owner and mode when it already exists. Files are created
`0600` with `O_CREAT | O_EXCL | O_NOFOLLOW`. The system temp directory is
shared on Linux, where another user can pre-create the directory, read
what is placed there, or plant symlinks.
- *Why:* reproduced. The inspection server wrote its access token to
  `/tmp/rustnative-inspect/<pid>.json` with mode `644` in a `755`
  directory, readable by every local user, who could then read and edit
  the running application over loopback. The dev agent wrote the
  executable it was sent into `/tmp/rustnative-dev-agent/`, through a call
  that follows symlinks, and ran it.
- *Enforcement:* E3, `rustnative_core::secret` (the per-user directory,
  `create_private_file`, `create_private_executable`), which the
  inspection server, the dev agent, and `rustnative deploy local` use, with
  mode, owner, and symlink tests on Unix **[Enforced]**. A scan forbidding
  `std::env::temp_dir()` for local IPC is **[Owed]**.

**SEC-15 — Templates and examples are security-critical code.** Project
templates, capability kits, and examples are what applications copy. They
follow every standard here, model the secure sequence (API-10), and are
reviewed by an owner of the area they demonstrate.
- *Enforcement:* E3, template tests (sign-in renews the session, forms
  carry CSRF tokens), E5 CODEOWNERS on `crates/rustnative/templates/`
  **[Owed]** (F-16).

**SEC-16 — Unverified data is typed as unverified.** A value whose
authenticity has not been checked (an identity token, a webhook body, a
client-supplied claim) reaches application code wrapped in a type such as
`Unverified<T>`. Only a verifying function unwraps it.
- *Why:* the OAuth client returns the provider's OpenID Connect `id_token`
  as a plain `Option<String>`, unverified: no signature, issuer, audience,
  expiry, or nonce check, and no nonce is sent (F-15). An application that
  reads identity from it trusts a forgeable token.
- *Enforcement:* E1 **[Owed]**.

**SEC-17 — SQL identifiers are typed values from the schema.** An API that
puts a table or column name into SQL takes a typed identifier produced from
the schema or model, never a `&str`. Raw SQL fragments in a public API are
named as such (`raw_sql`) and documented as injection-capable.
- *Why:* `RowPolicies::select`, `visible`, and `may_read` interpolate
  caller-supplied `table`, `columns`, and `condition` strings, so the
  checklist's "SQL is bound parameters, by construction" holds only for
  the paths that avoid them (F-28).
- *Enforcement:* E1 **[Owed]**.

**SEC-18 — Clients never see internal error detail.** A response to a
client carries a status, a stable error code, and a message written for
that client. Database errors, file paths, stack traces, and dependency
messages are logged, not returned. The admin surface returns SQLite's error
text in `400` bodies (F-28).
- *Enforcement:* E3, `error_pages_follow_accept_and_hide_internals`
  **[Partial]**: it covers server errors, not handler-built `400`s.

---

## 16. Network services and listeners (NET)

Every listener the framework ships is attack surface: the HTTP server, the
traffic splitter, the deploy control API, the MQTT broker, live and sync
sessions, the inspection server, the dev agent, and the emulators. The
scan found that the code in front of these listeners (sessions, CSRF,
passkeys, the security headers) is strong, and that the listeners
themselves are where it fails.

**NET-1 — Loopback is not an authentication boundary.** A listener that
changes state requires a credential on every request, even on
`127.0.0.1`. Any web page the person visits can send requests to loopback,
and DNS rebinding can make a page's own origin resolve there. A loopback
listener also:
- rejects a `Host` header other than its own address;
- refuses requests carrying `Origin` or `Sec-Fetch-Site: cross-site`
  unless it is meant for browsers;
- requires the content type it parses. A JSON endpoint refuses a body
  whose `Content-Type` is not `application/json`, which a cross-site
  "simple" request cannot send without a preflight.

- *Why:* reproduced. Without a credential, a CSRF check, or a
  `Content-Type` check, two cross-site `text/plain` POSTs, the kind any page
  can send, registered an attacker's server as a revision on
  `rustnative deploy local`'s control API and moved 100% of the public
  proxy's traffic to it, users' cookies included.
- *Enforcement:* E1, a `Json<T>` that refuses any body not declared
  `application/json` or `application/*+json` (415) **[Enforced]**. E3, the
  deploy control API's per-run token, `Host` check, and cross-site refusal,
  with cross-site, token, and rebinding tests
  (`rustnative-server/tests/deploy.rs`) **[Enforced]**. A
  `LoopbackService` builder that cannot be constructed without a token is
  **[Owed]**.

**NET-2 — Size is checked before allocation.** A length read from a peer is
compared with a stated maximum before it sizes a buffer, and the maximum is
configurable with a small default.
- *Why:* reproduced. An MQTT broker that allocated whatever remaining
  length a client declared, up to 256 MiB, before reading or
  authenticating anything, was aborted by sixteen 5-byte packets.
- *Enforcement:* E3, an oversized-length test per listener: the MQTT
  broker (`rustnative-sync/tests/mqtt_limits.rs`), live sessions
  (`tests/live.rs`), the inspection server
  (`rustnative-core/tests/inspection.rs`), and the HTTP server and traffic
  splitter (`rustnative-server/tests/listener.rs`, `tests/deploy.rs`)
  **[Enforced]**.

**NET-3 — Every connection has deadlines and every listener has a cap.**
The cap counts connections and sessions. The deadlines cover the
handshake, the first request or packet, the request headers, the body,
idle time, and the whole request. Authentication and rate limiting run
before the expensive work.
- *Why:* the scan found an HTTP server whose header-read timeout never
  applied (hyper had no timer), bodies collected with no deadline or size
  limit, live sessions accepting 64 MiB per message before the hello, and
  an inspection server reading unbounded lines before its token check.
- *Enforcement:* E3, slow-client and flood tests per listener (NET-8): the
  HTTP server, traffic splitter, live sessions, inspection server, and
  MQTT broker each have a connection ceiling and deadlines with tests
  **[Enforced]**.

**NET-4 — An accept error never stops a server.** `EMFILE`, `ENFILE`,
`ENOBUFS`, `ECONNABORTED`, and other per-connection accept errors are
transient: log them, back off briefly, and continue. Only shutdown ends an
accept loop.
- *Why:* reproduced. A `serve` loop that ended on the first accept error
  stopped permanently after 19 client connections under a file-descriptor
  limit of 48.
- *Enforcement:* E3, a descriptor-exhaustion test per listener. The HTTP
  server's is `rustnative-server/tests/accept_errors.rs` **[Enforced]**;
  the traffic splitter, live sessions, inspection server, and MQTT broker
  share the same backoff, and their own descriptor-exhaustion tests are
  **[Owed]**.

**NET-5 — Each check has one choke point.** Origin checks, authentication,
rate limiting, and size limits are applied by one function that every
entry point to a service passes through. A second entry point that skips
the check is the defect this standard exists for.
- *Why:* the scan found `LiveServer::mount` checking the WebSocket's
  `Origin` and `LiveServer::serve`, the standalone entry point to the same
  sessions, not checking it. Both now call one function.
- *Enforcement:* E3, each entry point tested against the same hostile
  inputs: live sessions' standalone listener and `mount`
  (`rustnative-sync/tests/live.rs`) **[Enforced]**; the other services
  **[Owed]**.

**NET-6 — Rate limits key on what an attacker cannot multiply.** IPv4
clients are keyed by address and IPv6 clients by their `/64` prefix,
because one IPv6 host controls a whole `/64`. A full table evicts the
entries that are no longer limiting. It never resets every bucket, since
an attacker who can fill the table could then clear everyone's limit.
- *Why:* the rate limiter keys IPv6 clients by full address, and when more
  than 100,000 clients are mid-limit it clears the table (F-06).
- *Enforcement:* E3, a test with addresses from one `/64` and a test that a
  full table keeps limiting **[Owed]**.

**NET-7 — Graceful shutdown is real and tested.** A server that says "requests
in flight finish first" tracks its connections, stops accepting, drains
them within a bound, and has a test that a slow request completes across a
shutdown.
- *Why:* the scan found `serve` documenting this, returning immediately,
  and leaving its connection tasks detached, with no test.
- *Enforcement:* E3, `rustnative-server/tests/listener.rs`: requests in
  flight finish across a shutdown, and only within
  `Security::shutdown_grace` **[Enforced]**.

**NET-8 — Every listener has a hostile-peer test suite.** The suite sends
malformed input, an oversized length, a slow client, a connection flood,
descriptor exhaustion, and cross-site and rebinding requests where they
apply. It asserts each is refused and that the listener keeps serving
others. Its decoder has a fuzz target (TST-6).
- *Enforcement:* E3 **[Partial]**: every listener has oversized-length,
  slow-client, and flood tests (NET-2, NET-3), and the deploy control API
  has cross-site and rebinding tests. Descriptor exhaustion beyond the
  HTTP server (NET-4) and the fuzz targets are owed.

**NET-9 — Outbound clients bound what they accept.** HTTP, WebSocket, and
sync clients bound response size, follow redirects only by a stated policy
(never across a pinned host's origin, as the Windows client already
ensures), and time out. These are part of the portable contract (NAT-7),
identical on every backend.
- *Why:* response bodies are read without limit, and timeouts differ by
  backend (F-24).
- *Enforcement:* E3, the `HttpService` conformance suite **[Owed]**.

---

## 17. Cryptography, keys, and signing (CRY)

**CRY-1 — Primitives come from vetted crates, comparisons included.** Use
RustCrypto, `ed25519-dalek`, `argon2`, and `subtle`. Never implement a
primitive, including constant-time comparison. A hand-written fold is not
guaranteed to stay constant-time after optimization. Every comparison
of a secret goes through `rustnative_core::secret::constant_time_eq`,
backed by `subtle::ConstantTimeEq` (ARC-12); `rustnative-server`
re-exports it as `security::constant_time_eq`.
- *Enforcement:* E3, a scan for XOR-fold comparison patterns **[Owed]**.

**CRY-2 — Key material is a type that validates itself.** A key is
constructed from a fixed-size array or a validated source, never from an
arbitrary `Vec<u8>`. A constructor that documents "at least 32 bytes" also
enforces it. Signing and issuing functions return `Result` and never fall
back to an empty value.
- *Why:* `TokenSigner::new` accepts a key of any length, empty included,
  and `issue` uses `unwrap_or_default`, a path that would emit an empty
  signature (F-23).
- *Enforcement:* E1 **[Owed]**.

**CRY-3 — Every MAC, AEAD, and signature binds its purpose.** Sealed data
binds what it is for and its format version, as associated data, a
`typ`/`aud` claim, or a domain-separation prefix, so a value made for one
purpose is refused for another. Each purpose derives its own key (HKDF)
from the root secret.
- *Why:* session cookies use AES-256-GCM with no associated data, and
  bearer tokens carry only a subject and an expiry. A token issued by a
  signer for any purpose is accepted as an access credential by every
  route that signer guards (F-23).
- *Enforcement:* E1 and E3 cross-purpose refusal tests **[Owed]**.

**CRY-4 — Every long-lived key has a rotation and a recovery path.** The
standard covers session keys, token keys, update-signing keys, and
code-signing certificates. Each has a documented procedure for rotation
without an outage and for compromise. Sessions have rotation
(`with_previous_keys`). The update verifier pins one key, with no path to a
successor key short of an out-of-band reinstall.
- *Enforcement:* E3 for rotation where it is implemented **[Partial]**, E6
  for the compromise procedure **[Owed]**.

**CRY-5 — Pin public keys, with a backup.** Certificate pins name the
SHA-256 of the subject public key info (SPKI), not of the whole
certificate. Every pinned host carries at least one backup pin for a key
not yet in use, so a certificate renewal does not lock every deployed
application out of its own server.
A pinned request succeeds only when a matching pin was seen on the
connection that carried it. No verdict means refusal (REL-9).
- *Why:* `CertificatePins` pins the digest of the whole leaf certificate,
  which changes on every renewal (F-24). On Linux the pin check fails open
  behind a proxy (F-42).
- *Enforcement:* E1, a pin type that requires two SPKI digests, plus E3
  **[Owed]**.

**CRY-6 — Code signatures are timestamped.** Every Authenticode, MSIX, APK,
and notarization signature uses SHA-256 and an RFC 3161 timestamp, so it
stays valid after the signing certificate expires.
- *Why:* `rustnative package` calls `signtool sign /fd SHA256` without
  `/tr` and `/td`, so every package it signs stops verifying when the
  certificate expires (F-26).
- *Enforcement:* E3, an argument test and a verification of the signed
  output **[Owed]**.

**CRY-7 — Verify strictly, then parse.** Signatures are verified over the
exact published bytes before those bytes are parsed, as the update
verifier does, using strict verification (`verify_strict` for Ed25519).
Version comparisons in security decisions reject malformed versions
rather than reading a non-numeric part as zero.
- *Enforcement:* E3 **[Partial]**: verify-then-parse is tested. Strictness
  and malformed-version refusal are **[Owed]**.

---

## 18. Privacy and data protection (PRV)

**PRV-1 — Nothing leaves the machine without consent.** Telemetry starts
without consent. The application records the person's choice. Withdrawing
consent stops sending at once and drops what was not yet sent
(`docs/telemetry-policy.md`).
- *Enforcement:* E3, the consent tests in `rustnative-observe`
  **[Enforced]**.

**PRV-2 — There is no framework-operated endpoint.** The framework never
phones home. Every destination is named by the application's publisher.
- *Enforcement:* E3, a scan for hard-coded hosts in non-test code
  **[Owed]**. Thirty `http://` literals outside tests need triage.

**PRV-3 — Collect the minimum, redact the rest.** Spans carry routes, never
query strings. Bodies, typed text, file contents, and secrets are never
recorded. Inspection recordings apply the same redaction, and the
inspection channel itself is protected (SEC-14), since it exposes
application state.
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

## 19. Supply chain and build integrity (SUP)

`cargo deny check` (advisories, bans, licences, sources) and `cargo audit`
(1,338 advisories against 383 crates) both pass.

**SUP-1 — Every dependency is justified and used.** A new dependency needs a
reason in the change: what it replaces, why the standard library or an
existing dependency is not enough, its maintenance status, its licence, its
`unsafe` footprint, and its effect on build time and artifact size.
Features are minimal (`default-features = false`). A dependency nothing
uses is removed. `cargo machete` finds four crates declaring unused ones:
`rustnative-web-macros` (`proc-macro2`, `quote`, `syn`, since it only
forwards to `rustnative-webgen`), `rustnative-durable` (`http`),
`rustnative-tui` (`serde_json`), and `examples/kits` (`serde`).
- *Enforcement:* E3, `cargo deny` **[Enforced]**, plus `cargo machete` in
  CI **[Owed]**. Justification: E5 **[Review]**.

**SUP-2 — Licences are compatible.** Every dependency is compatible with
`MIT OR Apache-2.0`, and the allowed list is in `deny.toml`.
- *Enforcement:* E3, `cargo deny check licenses` **[Enforced]**.

**SUP-3 — Advisories fail the build.** Vulnerable and yanked crates fail CI.
Unmaintained crates are reported and need a written decision.
- *Enforcement:* E3, `cargo deny` with `yanked = "deny"` and `cargo audit`
  **[Enforced]**. An `unmaintained` policy in `deny.toml` is **[Owed]**.

**SUP-4 — Duplicates are explained.** Multiple versions of a crate are
allowed only when listed with a reason (`clippy.toml`
`allowed-duplicate-crates`). `cargo deny` reports four duplicates:
`syn` and `windows-sys` are explained, and `getrandom` (two versions of
the CSPRNG crate) and `downcast-rs` are not.
- *Enforcement:* E3, `cargo deny bans` at `deny` for unlisted duplicates
  **[Owed]** (it warns today).

**SUP-5 — Vendored code is recorded.** Vendored files keep their licence,
origin, version, and the regeneration procedure in a `VENDORED.md`
(`rustnative-i18n/VENDORED.md`, `rustnative-style/vendor/`). Vendored code
is not edited in place. Changes go upstream or into a documented patch.
- *Enforcement:* E5, CODEOWNERS on `vendor/` **[Owed]**.

**SUP-6 — CI is hardened.** Workflow tokens are least-privilege (`contents:
read` by default). Every action is pinned to a full commit SHA with the
version in a comment and updated by a bot. Checkouts set
`persist-credentials: false`. Container images are pinned by digest.
Untrusted pull requests never see secrets.
- *Why:* `zizmor` reports 43 actions pinned by tag, third-party ones
  included (`reactivecircus/android-emulator-runner@v2`,
  `taiki-e/install-action`, `gradle/actions`), 13 checkouts that leave the
  token on disk, and 2 floating images (`fedora:latest`,
  `archlinux:latest`) (F-35).
- *Enforcement:* E3, `zizmor` and `actionlint` in CI, plus OpenSSF
  Scorecard (`Token-Permissions`, `Pinned-Dependencies`) **[Owed]**.
  `permissions: contents: read` **[Enforced]**.

**SUP-7 — Builds are reproducible.** An independent rebuild of a released
artifact is bit-identical, and any unavoidable variance is documented.
- *Enforcement:* E3, a reproducibility job **[Owed]** (`C131`).

**SUP-8 — Releases carry provenance.** Every released artifact has signed
build provenance meeting SLSA Build Level 2 or higher (hosted build, signed
provenance), plus an SBOM (CycloneDX from `rustnative compliance`).
- *Enforcement:* E3 in the release workflow **[Owed]**.

**SUP-9 — Dependency updates are automated and gated.** A bot (Dependabot
or Renovate) proposes updates for Cargo, GitHub Actions, and container
images. Updates merge only on a green full suite.
- *Enforcement:* E3 **[Owed]**.

**SUP-10 — The lockfile is committed and respected.** `Cargo.lock` is
committed. CI builds with `--locked` so the lockfile cannot drift silently.
- *Enforcement:* E3, `--locked` on CI builds **[Owed]**.

**SUP-11 — The toolchain is pinned to an exact version.**
`rust-toolchain.toml` names an exact release (`channel = "1.99.1"`), not
`stable`, and a bot proposes each new release as a pull request that fixes
its new lints. A verdict that changes with the date the contributor last
ran `rustup update` is not a verdict.
- *Why:* with `channel = "stable"`, the CI clippy gate fails on Rust 1.97
  (`doc_markdown` on "SQLite" in `rustnative-server-macros`) and passes on
  1.99.1, so the same commit is red for one contributor and green for
  another (F-37).
- *Enforcement:* E3, CI reads the pinned version **[Owed]**.

---

## 20. Performance and resource footprint (PRF)

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
style resolution, hit-testing, virtualization, and text measurement
document their complexity. A change that worsens it needs a measurement and
a reason. O(n²) over tree size is not accepted on a per-frame or per-event
path. The same holds for per-request paths on servers: a full scan of a
100,000-entry table under a mutex on every request is not acceptable.
- *Enforcement:* E3, `core_costs.rs` and the criterion benches
  **[Partial]**.

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
- *Enforcement:* E3, `native_frames_stop_when_the_animation_settles` and
  equivalents **[Partial]**. An idle-wakeups budget key per target is
  **[Owed]**.

**PRF-8 — Large data scales.** Lists, tables, and filters work over
200,000-row data sets without blocking input (Milestone 54). Virtualization
renders only when its range moves.
- *Enforcement:* E3, the virtualization tests and `filter_*` budgets
  **[Enforced]**.

**PRF-9 — Build time and compile cost are budgets too.** Proc-macro
expansion, `.rsx` compilation, and class resolution have budgets
(`rsx_compile_ms_per_kloc`, `class_resolve_us`, `build_time_*`). A full
debug build of the workspace's tests needs about 27 GB of disk, which a
contributor's machine and CI must hold. A `target_size_gb` key belongs in
the budgets.
- *Enforcement:* E3 **[Partial]**: build times are optional per run, and
  the disk key is **[Owed]**.

---

## 21. Accessibility (A11Y)

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
  `keyboard_traversal_shows_focus_cues`, and `layout_conformance.rs`
  **[Enforced]**.

**A11Y-4 — Targets and text scale.** Interactive targets are at least 24×24
logical pixels (WCAG 2.2, 2.5.8). Layout holds at 100%, 150%, and 200% text
scale without clipping or overlap.
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

## 22. Internationalization, localization, text, and input (I18N)

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
- *Enforcement:* E1, since `EdgeInsets` has no physical fields, plus E3,
  conformance row 3 **[Enforced]**.

**I18N-3 — Text goes through the host's text stack.** Shaping, bidi,
grapheme clusters, emoji sequences, line breaking, and spaceless scripts
are the host's. The framework never splits a string by `char` or byte for
display or caret movement. It uses grapheme segmentation (COR-10).
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

**I18N-6 — Pseudo-localization, RTL, and non-ASCII input are in the test
matrix.** Layout conformance runs pseudo-localized (expanded and accented)
and mirrored at every text scale. Every function taking text from people
or the network is tested with non-ASCII, multi-byte, and astral-plane
input.
- *Enforcement:* E3, `layout_conformance.rs` **[Enforced]**. Non-ASCII
  inputs in parser and service tests are **[Partial]**: the URL checker's
  non-ASCII panics were found by the scan's fuzzer, not by a test.

---

## 23. Visual and interaction quality (UX)

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
framework controls reserves its size (CLS 0 in `budgets/web.toml`).
- *Enforcement:* E3, the `cls` budget **[Enforced]**.

**UX-5 — Adaptive layout is by container.** Size classes per axis, decided
by the container a component is in, not by the device.
- *Enforcement:* E3, conformance row 13 **[Enforced]**.

**UX-6 — Visual regressions are caught.** Realized output is compared
against goldens per machine class. A missing golden fails with its capture
written beside it for review.
- *Enforcement:* E3, the Windows capture goldens, headless and terminal
  goldens, and gallery goldens **[Enforced]**.

---

## 24. Developer experience (DX)

**DX-1 — First run is one command and a budget.** `rustnative new`, `cd`,
`rustnative run <target>` works on a clean machine with the documented
prerequisites, inside the `first_run_s` budget (`budgets/linux.toml`).
`rustnative doctor` explains every missing prerequisite with the fix.
- *Enforcement:* E3, `first_run_s` (optional key) and the CLI tests
  **[Partial]**.

**DX-2 — The dev loop is budgeted and keeps state.** Edits reload within the
`dev_loop_restart_ms` budget (`budgets/linux.toml`), with state kept where
the target allows, and a broken build is shown over the running
application.
- *Enforcement:* E3, `dev_loop_restart_ms` (optional key) and `web_dev.rs`
  **[Enforced]**.

**DX-3 — Compile-time errors are first-class output.** A macro or build-step
diagnostic points at the user's span, names the rule, and suggests the
fix. It never surfaces a panic, a generated-code span, or a backtrace.
- *Enforcement:* E3, trybuild goldens **[Enforced]**.

**DX-4 — Configuration and environment variables are a documented,
registered surface.** `rustnative.toml` and every environment variable the
framework reads are listed in one generated reference: name, purpose,
values, default, and whether it is for users, CI, or tests only. Unknown
configuration keys are errors with a suggestion. Every variable is prefixed
`RUSTNATIVE_`. A variable that changes security behaviour says so.
- *Why:* the code reads 40 `RUSTNATIVE_*` variables, and 18 appear in no
  document. The user-facing ones include `RUSTNATIVE_TUI_MOUSE`,
  `RUSTNATIVE_UPDATE_ROOT`, `RUSTNATIVE_WEB_ADDR`, `RUSTNATIVE_WASM_DIR`,
  `RUSTNATIVE_GRADLE`, and `RUSTNATIVE_NODE` (F-39).
- *Enforcement:* E4, a reference generated from one registry the code reads
  through, diffed in CI **[Owed]**. `deny_unknown_fields` on configuration
  types **[Partial]**.

**DX-5 — Cargo features are additive and tested.** Enabling a feature never
removes API or changes behaviour silently. Each feature combination that CI
promises builds. `--all-features` builds on every host where it is
meaningful.
- *Enforcement:* E3, `--all-features` clippy **[Enforced]**, plus
  `cargo hack --each-feature` on library crates **[Owed]**.

**DX-6 — The CLI is a stable interface.** Commands, flags, exit codes, and
machine-readable output (`--json`) are covered by CMP-1. Output intended
for people goes to stderr or stdout consistently, and errors exit
non-zero.
- *Enforcement:* E3, CLI tests **[Partial]**.

**DX-7 — Tooling explains itself.** `rustnative expand` shows what markup
and classes become. `rustnative describe --json` is the machine-readable
surface for code generators and assistants.
- *Enforcement:* E4, `describe.rs` **[Enforced]**.

**DX-8 — A contributor can reproduce CI on one machine.** Every gate runs
locally with one documented command, with the pinned toolchain (SUP-11)
and documented disk and memory needs (PRF-9). Gates for other targets run
as cross-checks (`cargo clippy --target x86_64-pc-windows-msvc`,
`--target aarch64-linux-android`), and these pass from Linux today.
- *Enforcement:* E3, a `tools/ci-local.sh` exercised in CI **[Owed]**.

---

## 25. Documentation and examples (DOC)

**DOC-1 — Docs change with the code.** A behaviour change updates its
rustdoc, its guide, and its status entry (`BUILD_STATUS.md`) in the same
change. A doc that says something the code does not do is a defect.
- *Enforcement:* E3/E4, doctests, `describe.rs`, `doc_parity.rs`, and
  `doc_claims.rs` **[Enforced]**. E5 for prose.

**DOC-2 — Rustdoc builds clean.** No broken intra-doc links and no
warnings.
- *Enforcement:* E3, `RUSTDOCFLAGS=-D warnings` in CI **[Enforced]**.

**DOC-3 — Examples compile, run, and are tested.** Every example is a
workspace member, builds in CI, and has at least one test, or a stated
reason it cannot (a benchmark harness, a device-only host). Eleven examples
have no `tests/` directory.
- *Enforcement:* E3, `cargo check --workspace --all-targets` **[Enforced]**.
  A test per example is **[Owed]**.

**DOC-4 — Status is honest.** `BUILD_STATUS.md` states what was verified and
on what hardware. Work reasoned through but not run is recorded as exactly
that. A platform is called supported only after it runs on real hardware
(`PLAN.md` 2.13).
- *Enforcement:* E5/E6 **[Review]**.

**DOC-5 — Writing is plain and precise.** Write in short declarative
sentences, define terms once, and keep marketing language out. Claims cite
a test or a budget. Documentation is written for a reader arriving without
context.
- *Enforcement:* E3 for performance words (PRF-3), plus a spelling and link
  check over Markdown (`typos`, `lychee`) **[Owed]**.

**DOC-6 — Working notes are labelled.** Plans live in `docs/superpowers/`,
history in `docs/history/`, surveys of other projects in
`docs/ecosystem-analysis/`. None of them is presented as current behaviour.
- *Enforcement:* E5 **[Review]**.

**DOC-7 — A behavioural claim in rustdoc is held by a test.** A doc comment
promising behaviour ("requests in flight finish first", "never holds a
password", "compared in constant time", "valid once") names the test that
holds it, or is a doctest. The repository's "no claim without a test" rule
reaches the API reference, not only the Markdown.
- *Why:* the scan found three rustdoc claims false and untested. `serve`
  did not drain in-flight requests; it does now, with a test (NET-7). The
  signing module passes the password on the command line (F-25).
  `TokenSigner::new`'s "at least 32 bytes" is not checked (F-23).
- *Enforcement:* E5, plus a reviewer's checklist item, and E3 where a claim
  vocabulary can be scanned (`never`, `always`, `only`, `once`, `constant
  time`) for a nearby test reference **[Owed]**.

---

## 26. Observability and debuggability (OBS)

**OBS-1 — Library crates do not write to stdout or stderr.** Diagnostics go
through the framework's diagnostics and tracing sinks (`rustnative-observe`)
so an application controls where they go. The CLI, build scripts,
examples, explicit exporters (the stdout exporter), and tests MAY print.
`rustnative-core` writes to stderr in eight places, in the component tree
and the inspection start-up.
- *Enforcement:* E2, `clippy::print_stdout` and `print_stderr` denied in
  library crates, allowed per item with a reason **[Owed]**.

**OBS-2 — Spans and metrics follow the conventions.** Names and attributes
follow `rustnative_observe::conventions` (OpenTelemetry semantic
conventions). Spans carry paths, not query strings.
- *Enforcement:* E3, `rustnative-observe/tests/observe.rs` **[Partial]**.

**OBS-3 — Every backend answers the inspection protocol.** The tree, the
realized host objects and their rectangles, lifetimes, capabilities, and
style tables (conformance row 26).
- *Enforcement:* E3 per backend **[Enforced]**.

**OBS-4 — Every render records its cause.** `RenderCause` is recorded for
every render so over-rendering is diagnosable.
- *Enforcement:* E3 **[Enforced]**.

**OBS-5 — Debug builds diagnose, release builds stay quiet.** Expensive
checks and verbose diagnostics are behind `debug_assertions` or a feature,
never silently on in release.
- *Enforcement:* E3 through the budgets **[Partial]**.

**OBS-6 — Observability code never blocks or deadlocks the observed
code.** Exporters run off the hot path, are never called under a lock
(CON-9), and drop data rather than block when their queue is full.
- *Enforcement:* E3, a test with a slow and a reentrant exporter
  **[Owed]** (F-30).

---

## 27. Testing and verification (TST)

The suite is sound where it runs. Three full runs of the workspace (241
suites, Linux backend excluded as CI does) produced identical results:
1,183 passed, 0 failed, 12 ignored, and no flakes. The GTK backend's 90
tests passed under Xvfb but one: the pin test, which caught F-42 because
this machine routes HTTPS through a proxy. The failures the runs did
produce were all failures of hermeticity or of a host dependency (TST-17,
REL-4), and the suite's own skip logic would have hidden the most serious
one in CI (TST-9).

**TST-1 — Every behaviour change has a test, written first where
practical.** A bug fix includes the test that failed before the fix.
Lifecycle and reentrancy changes need tests of their ordering.
- *Enforcement:* E5 **[Review]**, plus TST-11 coverage on changed lines
  **[Owed]**.

**TST-2 — Tests run on the real host.** Platform integration tests drive
the production code path on the real host: real HWNDs, GTK on Xvfb,
Wayland, and Xwayland, an Android emulator, headless Edge or Chromium, a
pty, ConPTY, tmux, and SSH. Synthetic input goes through the real input
path, never a test-only shortcut.
- *Enforcement:* E3, the per-backend CI jobs **[Enforced]**.

**TST-3 — The headless backend is the reference.** Portable behaviour is
tested against `rustnative-headless` with the deterministic clock and
executor, and on one core.
- *Enforcement:* E3, the `headless` CI job **[Enforced]**.

**TST-4 — Shared suites are written once.** Suites go in
`rustnative-conformance` over the `ConformanceHost` seam and run on every
backend. Service contracts get suites too (NAT-7).
- *Enforcement:* E3 **[Enforced]** for UI guarantees, **[Owed]** for
  services.

**TST-5 — Properties, not only examples.** Reconciliation, identity,
layout, scroll ranges, cancellation, and merge functions have proptest
properties. Failing seeds are committed (`*.proptest-regressions`) and
never deleted.
- *Enforcement:* E3 **[Enforced]**.

**TST-6 — Parsers and decoders are fuzzed.** Each of these has a
`cargo-fuzz` target: the markup and style parsers, manifest and package
readers, wire and protocol decoders (sync, MQTT, inspection,
server-function calls, CBOR), the terminal input decoder, and every
function that validates text from outside. Targets run briefly in CI and
continuously elsewhere, under an allocation limit (STB-8). Crashes become
regression tests.
- *Why:* a 2.5-million-input generator run over eleven parsers found
  panics in exactly one function (the terminal backend's URL check, five
  distinct panics) and none in the others. The parsers are robust today, and fuzz
  targets are what keep them so.
- *Enforcement:* E3 **[Owed]** (`C130`). The scan's generators are a
  starting corpus.

**TST-7 — Tests are deterministic.** Tests use no wall-clock sleeps to wait
for a condition. They poll with a deadline or use the test clock. There is
no order dependence, no shared global state between tests, and no network
beyond loopback. Twenty-four `thread::sleep` calls remain in test
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
skip that CI can make fatal (`RUSTNATIVE_BROWSER_REQUIRED`). It never
passes silently. Nineteen places in tests print "skipped" and return, so
the test passes. One of them is the Linux pin test's TLS half, which
treats any error as "offline?" and would have hidden F-42 on a CI runner
without network access.
- *Enforcement:* E3, CI sets the `…_REQUIRED` variables on hosts that must
  run them, and a scan rejects a silent `return` after a skip message
  **[Partial]**.

**TST-10 — Goldens are reviewed, not rubber-stamped.** A golden update
states in the change why the output changed. Binary goldens are kept per
machine class and stay small.
- *Enforcement:* E5 **[Review]**.

**TST-11 — Coverage is measured and changed code is covered.** Line and
branch coverage (`cargo llvm-cov`) is reported per crate. Changed lines in
a pull request meet a threshold, with exceptions stated. Compilers and
validators have a higher floor. `rustnative-webgen`, a 6,733-line
Rust-to-JavaScript compiler, has five direct tests, all of them refusals,
and relies on agreement tests elsewhere. `rustnative-server-macros`, which
checks SQL at compile time, has none of its own.
- *Enforcement:* E3 **[Owed]** (`C130`).

**TST-12 — Hardware-only work is recorded as such.** What cannot be tested
without hardware is listed in `BUILD_STATUS.md` as not verified, never
assumed to pass.
- *Enforcement:* E5/E6 **[Review]**.

**TST-13 — Requirements trace to tests.** Requirements with IDs (`C…`,
`W-…`) are annotated `// req: ID` at the tests that verify them, feeding
`traceability.json`.
- *Enforcement:* E4, `rustnative compliance` **[Partial]**. Annotation is
  sparse (9 uses).

**TST-14 — Hostile inputs are tests, not only reviews.** Every security
claim has a test that plays the attacker: a cross-site request, a replay,
an oversized or slow peer, a malformed token, a forged signature, a planted
file, a non-ASCII URL, an unreachable verdict. The scan reproduced its
critical and high findings (the deploy control API's forgery, the MQTT
allocation, the server's accept loop, the URL checker's panics, the
inspection token's mode) by writing such tests against the real code.
The suite of the day caught none of them, and caught F-42 only because this
machine happened to route HTTPS through a proxy.
- *Enforcement:* E3 **[Owed]**. Reviewers ask "which test attacks this?"
  (E5).

**TST-15 — Miri runs in CI.** A nightly job runs `cargo miri test` over
every crate Miri can execute, extended as crates are made Miri-compatible.
- *Enforcement:* E3 **[Owed]**. `rustnative-types`, `-drawlist`, `-i18n`,
  and `-interop` pass today. The core's identity and layout tests pass
  until the first test that parks a thread, where Miri rejects
  `parking_lot_core` 0.9.12's Linux `futex` call (an argument-type check in
  a dependency, not this code). The core's Miri job needs that upstream
  issue tracked or a Miri-specific lock path.

**TST-16 — The suite fits its machines.** A full test run fits the disk and
memory of CI's runners and of a contributor's documented machine, with
debug information and incremental artifacts sized to match (PRF-9). The
scan's first full run exhausted a 39 GB allowance and lost test binaries
to "no space left on device".
- *Enforcement:* E3, a CI step recording `target/` size **[Owed]**.

**TST-17 — Tests are hermetic.** A test's verdict does not depend on the
ambient environment: `RUST_BACKTRACE`, locale, `TERM`, proxy variables,
the user's configuration, or which optional system packages are present.
Tests that spawn children set the child's environment explicitly
(`env_clear`, then what the child needs). Tests that need a system package
check for it and report a skip CI can make fatal (TST-9).
- *Why:* with `RUST_BACKTRACE=1`, a common developer setting, two terminal
  tests fail, because the backtrace scrolls the panic message off the
  16-row screen they read (F-40). With it unset, all ten pass.
- *Enforcement:* E3, a CI job that runs the suite under a hostile
  environment (`RUST_BACKTRACE=full`, a non-English locale, a proxy, a
  narrow `TERM`) **[Owed]**.

---

## 28. Code quality, style, and maintainability (QLT)

**QLT-1 — Formatting is not a discussion.** `rustfmt` with the repository's
`rustfmt.toml`.
- *Enforcement:* E3, `cargo fmt --check` **[Enforced]**.

**QLT-2 — The lint policy is curated and ratcheted.** The workspace policy
is `clippy::pedantic` and `clippy::cargo` at warn, a chosen set of
`restriction` lints, and `-D warnings` in CI. New lints from a new
toolchain are fixed or allowed with a reason in the pull request that pins
it (SUP-11). Every crate inherits `[lints] workspace = true`.
- *Enforcement:* E2/E3 **[Enforced]**. All 60 manifests inherit it.

**QLT-3 — Every suppression carries its reason.** `#[allow]` and
`#[expect]` take `reason = "…"`, citing a standard ID where one applies.
Prefer `#[expect]`, which warns when the suppression is no longer needed.
About a quarter of the workspace's `#[allow]` attributes have no reason:
38 in the Windows backend alone, which a Linux-only scan does not see (CI-9).
No `#[expect]` is used.
- *Enforcement:* E2, `clippy::allow_attributes_without_reason = "deny"`
  (stable since Rust 1.81, below the MSRV), then
  `clippy::allow_attributes = "warn"` **[Owed]**.

**QLT-4 — No unfinished code.** No `todo!`, `unimplemented!`, `dbg!`, or
`TODO`/`FIXME` without an issue link.
- *Enforcement:* E2, `todo` and `unimplemented` denied **[Enforced]**.
  `dbg_macro` and an issue-link check for `TODO` are **[Owed]**.

**QLT-5 — Names say what things are.** Name things for the domain concept.
No abbreviations beyond the host's own vocabulary (`HWND`, `JNI`). Test
names are sentences describing the behaviour
(`a_revoked_session_opens_as_none`).
- *Enforcement:* E2, `clippy::many_single_char_names` and `similar_names`
  **[Enforced]**. E5.

**QLT-6 — Comments explain why, not what.** Module docs state the module's
responsibility and its invariants. Comments carry the reason a non-obvious
choice was made, citing `PLAN.md`, a milestone, or a standard. Comments
that only narrate the code are removed.
- *Enforcement:* E5 **[Review]**.

**QLT-7 — No dead code and no dead dependencies.** Unused items are removed,
not `#[allow(dead_code)]`-ed, unless the item is used on a target CI does
not build. Then the reason names the target. Unused dependencies go too
(SUP-1).
- *Enforcement:* E2, `dead_code` warns, denied in CI **[Enforced]**.
  Dependencies: `cargo machete` **[Owed]**.

**QLT-8 — Generated code is reproducible and marked.** Generated files
(`framework.json`, width tables, expansion goldens, generated JS and CSS)
have a generator in the repository, a header naming it, and a staleness
test. Generated code is never edited by hand.
- *Enforcement:* E4 **[Partial]**: `describe.rs` and the width-table
  generator hold for their files.

**QLT-9 — Source files are clean text.** UTF-8, LF line endings, a final
newline, no trailing whitespace, and no control characters other than tab
and newline.
- *Why:* broken escape sequences left raw control bytes in three doc
  comments: a NUL in `rustnative-web/src/client.rs`, backspaces in
  `rustnative-webgen/src/items.rs`, and a BEL in
  `rustnative-style/src/web.rs`.
- *Enforcement:* E3, `rustnative-conformance/tests/control_bytes.rs` fails
  on any control character but tab and newline in a tracked text file
  **[Enforced]**. `.gitattributes` enforces LF **[Enforced]**. The checks
  for a final newline and trailing whitespace are **[Owed]**.

**QLT-10 — Every language in the repository has a standard.**
- **Java** (Android host library): Google Java Style, `final` classes by
  default, reflection only behind a capability probe, Javadoc on every
  class, the narrowest catch (MEM-7). Enforcement: `javac -Xlint:all
  -Werror` and a broad-catch rule in the Android job **[Owed]**.
- **JavaScript** (`rn.js`, `sw.js`): ES modules, `"use strict"`, no
  dependencies, no `eval`, `new Function`, `innerHTML`, or
  `document.write` (none today). Behaviour is held equal to Rust by
  `rustnative-web/tests/runtime.rs`. Enforcement: a forbidden-API scan
  **[Owed]**, equality E3 **[Enforced]**.
- **Shell** (`tools/*.sh`): `#!/usr/bin/env bash`, `set -euo pipefail`
  (sourced files `set -u` only), quoted expansions, `shellcheck` clean.
  `shellcheck` reports 15 findings: `cd` without a failure branch,
  masked return values, and one unquoted expansion. Enforcement:
  `shellcheck` in CI **[Owed]**.
- **SQL** (migrations): one statement per concern, an explicit `down` for
  every `up`. Enforcement: DAT-1.
- **C and C#** (adoption hosts, probes): compile warning-free at the
  highest level (`/W4`, `-Wall -Wextra`). Enforcement: E5 **[Review]**.
- **CSS** (component and example styles): only the framework's style
  vocabulary, tokens over literals. Enforcement: the style compiler
  **[Enforced]**.
- **TOML and YAML:** formatted consistently, and workflows linted.
  Enforcement: `taplo fmt --check`, `actionlint`, and `zizmor` **[Owed]**.

**QLT-11 — Duplication is removed at the second copy.** Logic copied
between two places moves into a shared function, module, or group crate
when the second copy appears, as `rustnative-update` was extracted. For
security and durability primitives this is a MUST (ARC-12).
- *Enforcement:* E5 **[Review]**, and ARC-12's scan.

**QLT-12 — Generated code meets the lint policy.** Code a build step or
macro writes into a user's crate (`compile_rsx` output, `rsx!` and
`query!` expansions, generated bindings) compiles clean under the
workspace's lints, or carries `#[automatically_derived]` and reasoned
allows. A user who denies `missing_docs` or
`allow_attributes_without_reason` must not be broken by the framework.
`compile_rsx`'s output for `examples/adoption-library` raises 8
`missing_docs` and 9 reasonless-allow findings.
- *Enforcement:* E3, a test compiling generated output under the strict
  lint set **[Owed]**.

---

## 29. Version control and contribution workflow (VCS)

**VCS-1 — One concern per change.** A pull request does one thing: a
feature, a fix, a refactor, or a dependency update. Refactors that make a
feature possible land first, separately.
- *Enforcement:* E5 **[Review]**.

**VCS-2 — Commit messages explain.** The subject is a short summary
prefixed with its area, in the style of the existing history ("Terminal
backend: …", "CI: …"). The body says why, cites standard IDs and
milestones, and names the tests that hold the change. Each commit builds.
- *Enforcement:* E5 **[Review]**, plus a commit-message lint for the
  subject format **[Owed]**.

**VCS-3 — Contributors certify their right to contribute.** Every commit
carries `Signed-off-by:` under the Developer Certificate of Origin 1.1.
- *Enforcement:* E3, a DCO check on pull requests **[Owed]**.

**VCS-4 — Ownership is explicit.** A `CODEOWNERS` file assigns owners to the
core, each backend, the server's security code, every listener (NET),
cryptography (CRY), `unsafe`-heavy modules, templates, CI, `budgets/`,
`deny.toml`, `vendor/`, and these standards. Changes there need an owner's
approval.
- *Enforcement:* E5, branch protection with required CODEOWNERS review
  **[Owed]**.

**VCS-5 — The default branch is protected.** Required status checks are all
CI jobs that must pass. Required reviews are one, or two for CRY and NET
areas. No force-push or direct push. Linear history or merge commits are
chosen once and kept.
- *Enforcement:* E3, repository settings **[Owed]**, verified by OpenSSF
  Scorecard `Branch-Protection`.

**VCS-6 — Build outputs are never committed.** `target/` directories at any
depth, captures, stack dumps, and local state stay out of the repository.
- *Why:* while `.gitignore` ignored only the root `/target/`, an example's
  dev-loop state file under `examples/hello-label/target/` was committed.
- *Enforcement:* `.gitignore` ignores `target/` at every depth
  **[Enforced]**. A CI check that fails on tracked files under any
  `target/` is **[Owed]**.

**VCS-7 — Binary files are justified and small.** A binary (goldens, icons,
fixtures) has a reason, the smallest form that serves it, and a size limit.
Large binaries go in Git LFS or are generated.
- *Enforcement:* E3, a size check on added binaries **[Owed]**.

**VCS-8 — Pull requests use the template.** The template has these
sections: what and why, the standards touched, the tests added (including
the hostile test, TST-14), the hosts and targets verified on, the
documentation updated, a security and privacy note, and the AI-assistance
disclosure (GOV-6).
- *Enforcement:* E5, `.github/pull_request_template.md` **[Owed]**.

**VCS-9 — Review is a standard, not a formality.** Reviewers check the
change against the checklist in section 35. They reproduce claims
("verified on Windows") or ask for evidence. They ask which test attacks a
security claim. They separate blocking findings from optional ones. Review
comments are about the code. Authors answer every comment with a change or
a reason.
- *Enforcement:* E5 **[Review]**.

---

## 30. CI and the build system (CI)

**CI-1 — CI is the definition of "builds".** If it is not checked in CI, it
is not promised. Every gate listed in `CONTRIBUTING.md` runs in CI, and
every CI gate is runnable locally with a documented command (DX-8).
- *Enforcement:* E3 **[Enforced]**.

**CI-2 — Each backend is tested on its own host.** Windows on
`windows-latest`, GTK on Ubuntu, Fedora, and Arch, Android on an emulator,
the terminal on a pty, tmux, and SSH, the Web in headless Edge or Chromium,
and Apple targets with `cargo check` until hardware exists.
- *Enforcement:* E3 **[Enforced]**.

**CI-3 — Warnings are errors.** `RUSTFLAGS=-D warnings` and
`RUSTDOCFLAGS=-D warnings`.
- *Enforcement:* E3 **[Enforced]**.

**CI-4 — The toolchain is the pinned one, plus the MSRV.** CI uses the
version in `rust-toolchain.toml` (SUP-11) and checks the MSRV.
- *Enforcement:* E3 **[Partial]**: the MSRV is checked, and CI uses
  floating stable today.

**CI-5 — Every job has a timeout, and superseded runs are cancelled.**
`timeout-minutes` is set on every job (only the Android job has one). A
`concurrency` group cancels superseded runs of a branch.
- *Enforcement:* E3, `actionlint` plus a check for `timeout-minutes`
  **[Owed]**.

**CI-6 — Caches never change results.** A cache speeds up a build but never
changes its outcome. A cold-cache run is identical, and cache keys include
the toolchain and lockfile.
- *Enforcement:* E3, scheduled cold-cache runs **[Owed]**.

**CI-7 — Disabled jobs are tracked.** A job commented out or disabled (the
Windows cross-compile under Wine) has an issue and an owner, and its reason
is in the workflow.
- *Enforcement:* E5 **[Review]**.

**CI-8 — Artifacts from CI are for diagnosis.** Golden captures and budget
reports are uploaded on failure, so reviewers can see what changed.
- *Enforcement:* E3 **[Enforced]**.

**CI-9 — Every check runs on every target that compiles the code.** Lints,
scans, and extended lint passes run for each target whose `cfg` includes
code the others exclude: Windows, Android, wasm32, and Apple. A check run
only on Linux does not see `#[cfg(windows)]` code. The Windows backend's 38
reasonless suppressions and 42 multi-operation `unsafe` blocks are
invisible to a Linux-only pass.
- *Enforcement:* E3 **[Partial]**: the CI clippy gate runs on Windows and
  Android, but scans and extended lints do not exist yet.

---

## 31. Release, distribution, and operations (RLS)

**RLS-1 — Releases are cut from CI, never from a laptop.** A tag on the
default branch triggers the release workflow. It builds, signs with a
timestamp (CRY-6), writes provenance and an SBOM (SUP-8), and publishes.
- *Enforcement:* E3 **[Owed]**.

**RLS-2 — Every release has notes.** Notes list breaking changes with their
codemods (CMP-4), deprecations, MSRV and toolchain changes, host-support
changes, security fixes with advisory IDs, and budget changes. Notes are
written from the changes' own descriptions, so each pull request states its
user-visible effect.
- *Enforcement:* E5, with a release checklist **[Owed]**.

**RLS-3 — The crates release together under one version.** A seam count of
one, per `docs/policy/stability.md`.
- *Enforcement:* E3, a version-consistency check over the workspace
  **[Owed]**. Today the workspace dependencies pin one version.

**RLS-4 — Packages follow each host's rules.** Signing, store policies, and
update rules are those in `docs/deploy/update-rules.md`. The updater never
runs where the host forbids self-update, and the decision fails closed
(REL-9).
- *Enforcement:* E3, the packaging and update tests **[Partial]** (F-17).

**RLS-5 — Server deployments are revisable and their controls are
protected.** Revisions are immutable, traffic moves by percentage, rollback
is one command, and health and readiness are real. The control surface
that moves traffic is authenticated (NET-1), and previewing an unpromoted
revision (`x-revision`) is restricted to authorized callers.
- *Enforcement:* E3, `rustnative-server/tests/deploy.rs` **[Enforced]**: the
  mechanics are tested, the control surface is authenticated (NET-1), and
  a preview needs the run's preview token (`x-preview-token`), which a
  splitter without one never accepts.

**RLS-6 — Published crates are complete.** Each published crate has its
`description`, `license`, `repository`, `readme`, `keywords`, and
`categories`, plus `include`/`exclude` so tests and goldens are not
shipped needlessly, and `cargo publish --dry-run` passes.
- *Enforcement:* E2, `clippy::cargo` (`cargo_common_metadata`)
  **[Enforced]**, plus a `publish --dry-run` job **[Owed]**.

---

## 32. Portability, interoperability, and extensibility (PRT)

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
Windows, Linux, and macOS can build and test what their host supports, and
can cross-check the others (DX-8). A script works on the platforms it
claims, or says which ones it supports.
- *Enforcement:* E3, the CI matrix **[Partial]**.

**PRT-7 — Embedded targets reuse the ecosystem.** Use `embedded-hal`,
existing executors, `probe-rs`, and board descriptions. The framework never
writes a second copy (`docs/policy/embedded.md`).
- *Enforcement:* E5 **[Review]**.

---

## 33. Legal, licensing, and regulatory compliance (LEG)

**LEG-1 — Contributions are dual-licensed.** Every contribution is under
`MIT OR Apache-2.0`, the workspace licence. Contributing is agreement, and
the DCO sign-off records it (VCS-3).
- *Enforcement:* E3, the DCO check **[Owed]**.

**LEG-2 — Third-party code keeps its notices.** Vendored and copied code
keeps its licence file and attribution. Compatible licences only (SUP-2).
Code from a source whose licence is unknown is not accepted.
- *Enforcement:* E3, `cargo deny` for crates **[Enforced]**. E5 for
  vendored and copied files.

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
  (PRV-4). A store's prohibition on self-update is a compliance obligation,
  so its check fails closed (REL-9).
- *Enforcement:* E6 **[Review]**, with a yearly review recorded in
  `docs/security/certification-posture.md`.

**LEG-5 — Names and marks are respected.** Third-party product names are
used descriptively. No logos or trademarks are shipped without permission.
No code, assets, or text are copied from projects whose terms forbid it.
- *Enforcement:* E5 **[Review]**.

**LEG-6 — Export-relevant code is known.** Cryptographic functionality is
listed in the release notes and SBOM, so distributors can meet their export
obligations.
- *Enforcement:* E4, the SBOM **[Partial]**.

---

## 34. Governance, community, and conduct (GOV)

**GOV-1 — A code of conduct applies everywhere the project meets.** The
project adopts Contributor Covenant 3.0 (July 2025) as
`CODE_OF_CONDUCT.md`, with a named private contact and its "addressing and
repairing harm" enforcement process.
- *Enforcement:* E6 **[Owed]**, since the file does not exist yet.

**GOV-2 — Roles are written down.** `GOVERNANCE.md` defines contributor,
reviewer, maintainer, and area owner. It says how each role is earned and
lost, who can merge, who can release, and how decisions are made (lazy
consensus, then a maintainer vote). It lists the maintainers.
- *Enforcement:* E6 **[Owed]**.

**GOV-3 — Issues are triaged and labelled.** Issue templates cover bugs
(with host, version, and reproduction), feature requests (with the use case
and the principle it serves), backend gaps, and documentation. A triager
acknowledges new issues and labels them by area, kind, and priority. Good
first issues are marked and kept current. Many of the owed checks in
section 37 are good first issues.
- *Enforcement:* E6, `.github/ISSUE_TEMPLATE/` **[Owed]**.

**GOV-4 — Communication is in the open.** Design discussion happens in
issues, RFCs, and pull requests, where it is archived. Decisions made
elsewhere are summarized there before they take effect. Security issues are
the exception (SEC-12).
- *Enforcement:* E6 **[Review]**.

**GOV-5 — Significant changes go through an RFC.** A new crate, platform,
listener, public protocol, change to a `PLAN.md` section 2 principle, or
change to these standards needs a written proposal with motivation, design,
alternatives, drawbacks, and a security section. It needs a comment period
of at least seven days and a recorded decision.
- *Enforcement:* E6 **[Owed]**, with `docs/rfcs/` and a template.

**GOV-6 — AI-assisted contributions are owned by a person.** Tools,
including AI assistants, MAY help. The contributor:
- discloses material AI assistance in the pull request;
- understands, has reviewed, and can explain every line, and answers review
  in their own words;
- MUST NOT submit AI-generated `SAFETY:` proofs, CRY or NET code, or
  threat-model entries without independent verification, stated in the
  pull request with the test that attacks it (TST-14);
- ensures nothing in the contribution infringes a licence (LEG-2);
- does not open bulk, unreviewed, or speculative pull requests.

Maintainers MAY close contributions that break this without detailed
review. For comparison, the rust-lang/rust repository adopted an LLM
policy in August 2026.
- *Enforcement:* E5/E6, a disclosure field in the pull-request template
  **[Owed]**.

**GOV-7 — Status and promises are honest.** The project says what is
verified, on what, and what is owed (DOC-4, TST-12, NAT-3). It does not
announce support it has not tested, and it compares itself with other
projects only by the published methodology
(`docs/comparison/methodology.md`).
- *Enforcement:* E5/E6 **[Review]**.

**GOV-8 — The project plans for its own continuity.** Each area has at
least two people able to review it, documented in CODEOWNERS. Release and
signing keys have documented custody and a recovery procedure (CRY-4).
Knowledge held by one person is written down.
- *Enforcement:* E6 **[Owed]**.

**GOV-9 — Recognition is part of the work.** Contributors are credited in
release notes. Reviewers' time is acknowledged.
- *Enforcement:* E6 **[Review]**.

---

## 35. The contribution checklist

Authors fill this in, and reviewers verify it. "N/A" is a valid answer only
with a reason.

**Every change**
- [ ] One concern (VCS-1). Commit messages explain why and cite standard IDs
      (VCS-2). Signed off (VCS-3).
- [ ] The local gates in `CONTRIBUTING.md` pass with the pinned toolchain:
      fmt, clippy with `-D warnings` (also cross-checked for the Windows
      and Android targets when backend or portable code changed), test,
      doc, deny, audit. MSRV checked.
- [ ] Tests added or updated (TST-1). A bug fix includes its failing test.
- [ ] Docs, guides, and `BUILD_STATUS.md` updated (DOC-1). Every
      behavioural claim in rustdoc is held by a test (DOC-7). Status is
      honest about which hosts it was verified on (DOC-4).
- [ ] No new `#[allow]` without a reason (QLT-3). No `unwrap` or `expect`
      on reachable paths (STB-1). No new `Result<_, String>` (ERR-1). No
      discarded results or dropped causes without a comment (ERR-3, ERR-5).
- [ ] No build outputs or large binaries committed (VCS-6, VCS-7). Source
      is clean text (QLT-9).
- [ ] AI assistance disclosed, if material (GOV-6).

**When it applies**
- [ ] **Public API:** documented with an example, `#[must_use]` and
      `#[non_exhaustive]` decided, semver impact stated, deprecation and
      codemod for breaks, secure sequences as one call (API-2, API-4,
      API-10, CMP-1…4).
- [ ] **Node kind, modifier, or style property:** both syntaxes, both
      spellings, equivalence case, every backend's answer, accessibility
      mapping (EQV-1, EQV-4, A11Y-2).
- [ ] **Backend code:** conformance rows updated, capability advertised only
      if realized, ownership module updated, census returns to baseline,
      every host callback goes through the boundary (NAT-3, NAT-4, ARC-7,
      RES-2, STB-2).
- [ ] **Service contract:** the same observable behaviour on every backend,
      held by the contract's conformance suite (NAT-7).
- [ ] **`unsafe` or FFI:** one operation and one `SAFETY:` proof per block,
      typed accessor for any shared invariant, ownership documented, no
      `mem::forget`, owner review (MEM-2, MEM-9, MEM-10, MEM-4).
- [ ] **Async, threads, or locks:** owned by a scope, no UI-thread
      blocking, bounded channels, no lock across `.await`, nothing called
      under a lock (CON-2, CON-4, CON-6, CON-7, CON-9).
- [ ] **Listener or network client:** credential even on loopback, `Host`
      and cross-site checks, content type enforced, size before
      allocation, deadlines and caps, accept errors survived, one choke
      point, hostile-peer tests (NET-1…9).
- [ ] **Untrusted input or parser:** threat-model row, reject never
      truncate, depth bound, fuzz target, non-ASCII cases (SEC-2, SEC-3,
      SEC-6, TST-6, I18N-6).
- [ ] **Anything handed to the OS:** URL policy, validated path components,
      restricted library search, no shell (SEC-13, DAT-6, SEC-8).
- [ ] **Local files, sockets, or tokens:** per-user directory, `0700`/`0600`,
      `O_EXCL | O_NOFOLLOW` (SEC-14).
- [ ] **Crypto, auth, keys, or signing:** vetted primitives, typed keys,
      purpose binding, rotation, SPKI pins with a backup, timestamped
      signatures, two approvals (CRY-1…7, SEC-7).
- [ ] **Security or policy decision:** fails closed, with a fault-injection
      test (REL-9).
- [ ] **Shared state across processes:** leases, no single-instance
      assumption (REL-8).
- [ ] **Persisted or wire format:** versioned, fixture for the old version,
      migration, durable writes through the one implementation (CMP-6,
      DAT-2, DAT-3).
- [ ] **Template, kit, or example:** follows every standard and models the
      secure path (SEC-15).
- [ ] **Performance-sensitive path:** budgets pass, and any budget change is
      justified (PRF-1, PRF-2).
- [ ] **New dependency or environment variable:** justified, licence
      checked, features minimal, registered and documented (SUP-1, DX-4).
- [ ] **User-visible text:** localized, RTL and pseudo-localization checked
      (I18N-1, I18N-6).
- [ ] **Telemetry or data handling:** consent, redaction, minimization
      (PRV-1, PRV-3).

### Definition of done

A change is done when every applicable checklist item is satisfied, every CI
gate is green on the change's head, every review thread is answered, and the
change's claims (verified hosts, guarantees, numbers, security properties)
are held by a test, a budget, or a recorded pass.

---

## 36. Waivers

A MUST can be waived only when following it is impossible or clearly worse
for the project.

1. The waiver is written in the pull request, citing the standard ID, the
   reason, the risk, and the scope (a file, an item, or a release).
2. It is approved by a maintainer who owns the area, plus one more for SEC,
   NET, CRY, MEM, or SUP standards.
3. In code it appears as `#[allow(…, reason = "waiver: <ID> — …")]` or a
   comment of the same form at the site.
4. A waiver with no end date is reviewed at each minor release.

A waiver that keeps recurring is evidence the standard is wrong, and it is
brought to GOV-5.

---

## 37. Findings and enforcement owed

### The findings register

These are the defects the scan found while deriving the standards, at
commit `c2a2e67`. "Reproduced" means a test or probe ran the real code and
showed the defect. "Read" means the code was read in context and the
defect is certain from the code. Each is cited by the standards it breaks.
A fixed finding leaves the register and its ID is not reused;
`docs/policy/standards-derivation.md` records how the scan found it.

| ID | Severity | Where | Defect | Evidence | Breaks |
|---|---|---|---|---|---|
| F-06 | Medium | `rustnative-server/src/security.rs` `RateLimiter` | IPv6 keyed per address; a full table clears every bucket | Read | NET-6 |
| F-15 | Medium | `rustnative-server/src/auth/oauth.rs` | `id_token` returned unverified, no nonce | Read | SEC-16 |
| F-16 | Low | `crates/rustnative/templates/kits/auth.rs`, `examples/web-notes`, `examples/server-demo` | Sign-in without `session.renew()`, which the docs require | Read | API-10, SEC-15 |
| F-17 | Medium | `rustnative-android/src/update.rs` `from_play`, `RnServices.installer` | Play-install check fails open | Read | REL-9, RLS-4, LEG-4 |
| F-19 | Medium | `rustnative-linux/src/desktop/notifications.rs` | `mem::forget` leaks a D-Bus subscription per notification | Read | MEM-10, RES-6 |
| F-21 | Medium | `rustnative-update/src/lib.rs` `write_atomically` | Rename without `fsync`; a second, weaker atomic write | Read | DAT-3, ARC-12 |
| F-22 | Low | `rustnative-update/src/lib.rs` `model_directory` | Manifest-supplied name joined into a path unchecked | Read | DAT-6 |
| F-23 | Medium | `rustnative-server/src/auth/token.rs`, `session.rs` | Any-length key accepted; no purpose binding; empty-signature fallback | Read | CRY-2, CRY-3, DOC-7 |
| F-24 | Medium | `rustnative-core/src/services/mod.rs` `CertificatePins`; HTTP clients | Whole-certificate pins; unbounded response bodies; per-backend timeouts | Read | CRY-5, NET-9, NAT-7 |
| F-25 | Medium | `crates/rustnative/src/package/sign.rs` | PFX password passed on `signtool`'s command line | Read | SEC-9, DOC-7 |
| F-26 | Medium | `crates/rustnative/src/package/sign.rs` | No RFC 3161 timestamp on signatures | Read | CRY-6 |
| F-27 | Low | `rustnative-web/src/html.rs`, `rustnative-server/src/response.rs` | HTML escaping implemented twice | Read | ARC-12 |
| F-28 | Medium | `rustnative-server/src/db/policy.rs`, `admin.rs` | Raw `&str` SQL fragments in a public API; database errors returned to clients | Read | SEC-17, SEC-18, DAT-4 |
| F-29 | Low | `rustnative-server/src/auth/passkey.rs` `cbor` | Arrays and maps over 1,024 entries silently truncated | Read | SEC-3 |
| F-30 | Medium | `rustnative-core/src/scheduler/`, `rustnative-observe/src/trace.rs` | Waker and exporter invoked under a lock | Read; clippy `significant_drop_in_scrutinee` | CON-9, OBS-6 |
| F-31 | Medium | Windows `subclass_proc`, `surface_proc`, `listener_proc`, `notification_host_proc`, `on_exception`; terminal `on_control` | Host callbacks without the panic boundary | Read (all 47 enumerated) | STB-2 |
| F-33 | Medium | `rustnative-server/src/jobs.rs` | Start-up requeues every running job, including another instance's | Read | REL-8 |
| F-34 | Low | `rustnative-server/src/jobs.rs` `backoff` | Backoff without jitter | Read | REL-2 |
| F-35 | Medium | `.github/workflows/ci.yml` | 43 tag-pinned actions, 13 persisted credentials, 2 floating images | `zizmor` | SUP-6 |
| F-36 | Low | `rustnative-server/src/body.rs` `Chunks::send` | Panics when called from async code | Read | API-7 |
| F-37 | Medium | `rust-toolchain.toml` | Unpinned toolchain: CI's clippy gate fails on 1.97, passes on 1.99.1 | Reproduced | SUP-11, CI-4 |
| F-38 | Low | workspace | 220 of 257 public enums exhaustive (`Node`, `NodeKind`); no sealed traits | Count | API-4 |
| F-39 | Low | workspace | 18 of 40 `RUSTNATIVE_*` variables undocumented | Count | DX-4 |
| F-40 | Low | `rustnative-tui/tests/real_terminal.rs` | Two panic tests fail when `RUST_BACKTRACE=1` | Reproduced: fail with 1, pass with 0 | TST-17 |
| F-41 | Medium | `rustnative-linux/src/gtk/foreign.rs` (media node) | A missing GStreamer `playbin` aborts the process through GTK's `g_error` | Reproduced: the GTK test binary died with `SIGTRAP` | REL-4, NAT-3 |
| F-42 | High | `rustnative-linux/src/services/http.rs` `send` | Pin check fails open: no TLS handshake event, no refusal. A proxy's response is returned as the pinned host's | Reproduced by the repo's own test behind an HTTPS proxy | REL-9, CRY-5, TST-9 |

Number F-32 is unused: the first round's claim that no first-party parser
bounds its recursion depth was retired by the second round, which found
the CBOR reader's limit. The first round's claim that nine crates lacked
documentation was narrowed by the second round to "documented but not
gated" (API-2).

### What was checked and found sound

The test suite: three identical workspace runs (1,183 passed, no flakes)
and the GTK backend's 89 of 90 under Xvfb. Supply chain (`cargo deny`,
`cargo audit`). The CI clippy gate on Linux,
Windows, and Android with the current toolchain. Miri on four crates. 2.5
million fuzzed inputs to ten parsers and decoders, with no panic. The HTML
escaper. The CSRF double-submit check. Session sealing and rotation.
Passkey challenge handling. Argon2id parameters. Update verify-then-parse.
The terminal's output sanitizing and signal handler. The Windows user-data
accessors. The JNI boundary helper. Server functions' forgery protection.

### Mechanisms owed, in order

Each mechanism raises the standards listed to their target rung. They are
ordered by how much protection each buys per unit of work.

| # | Mechanism | Raises | Rung |
|---|---|---|---|
| 1 | Pin the toolchain exactly, and pin actions, credentials, and images (F-35, F-37) | SUP-11, SUP-6, CI-4 | E3 |
| 2 | Fail-closed Play check and positive-verdict pin check on Linux, each with a fault-injection test (F-17, F-42) | REL-9, CRY-5 | E3 |
| 3 | Workspace lint promotions: `allow_attributes_without_reason`, `significant_drop_in_scrutinee`, `mem_forget`, `multiple_unsafe_ops_per_block`, `print_stdout`/`print_stderr` (libraries), `exit`, `map_err_ignore`, `let_underscore_must_use`, `unused_result_ok`, `missing_debug_implementations` | API-3, QLT-3, CON-9, MEM-2, MEM-10, OBS-1, STB-6, ERR-3, ERR-5 | E2 |
| 4 | `#![forbid(unsafe_code)]` in the 18 crates without `unsafe` | MEM-1 | E2 |
| 5 | Boundary scan for every `extern` function, and the missing boundaries added (F-31) | STB-2 | E3 |
| 6 | Typed keys; purpose binding; SPKI pins with backups; timestamped signing without argv secrets (F-23, F-24, F-25, F-26) | CRY-2…6, SEC-9 | E1/E3 |
| 7 | `Unverified<T>`, typed SQL identifiers, client-safe error bodies, and one-call sign-in (F-15, F-16, F-28) | SEC-15…18, API-10 | E1 |
| 8 | One durable atomic write; `PathComponent` (F-21, F-22) | DAT-3, DAT-6, ARC-12 | E1/E3 |
| 9 | Leases for the job queue; jittered backoff (F-33, F-34) | REL-8, REL-2 | E3 |
| 10 | Service-contract conformance suites (`HttpService`, `SystemService`) | NAT-7, NET-9, TST-4 | E3 |
| 11 | CODEOWNERS, branch protection, PR and issue templates, DCO check | VCS-3…8, GOV-3 | E3/E5 |
| 12 | Architecture boundary test over `cargo metadata`, plus a host-`cfg` scan of the core | ARC-1…4 | E3 |
| 13 | `cargo-fuzz` targets seeded from the scan's generators; a nightly Miri job | TST-6, TST-15, MEM-6 | E3 |
| 14 | Public API snapshot, `cargo-semver-checks`, and the sealing decisions | CMP-1, CMP-2, API-4 | E4 |
| 15 | `cargo machete`, `--locked`, an `unmaintained` policy, denied unlisted duplicates, an update bot | SUP-1, SUP-3, SUP-4, SUP-9, SUP-10 | E3 |
| 16 | Environment-variable registry and its generated reference (F-39) | DX-4 | E4 |
| 17 | Extended lints and scans on the Windows, Android, and wasm targets | CI-9 | E3 |
| 18 | Typed-error burn-down for the 134 `Result<_, String>` signatures | ERR-1 | E3 |
| 19 | Coverage on changed lines, with floors for compilers | TST-11 | E3 |
| 20 | Flake detection and quarantine; sleep and silent-skip scans | TST-7…9 | E3 |
| 21 | `cargo hack --each-feature`; persisted-format fixtures | DX-5, CMP-6, DAT-2 | E3 |
| 22 | `shellcheck`, `javac -Xlint:all -Werror`, a broad-catch rule, `taplo`, the JS forbidden-API scan | QLT-10, MEM-7 | E3 |
| 23 | Markdown spelling and link checks; `doc_parity` for all of `docs/`; rustdoc claim scan | DOC-5, DOC-7, EQV-7 | E3 |
| 24 | Release workflow with provenance (SLSA Build L2+), SBOM, `publish --dry-run`, and a reproducibility check | SUP-7, SUP-8, RLS-1, RLS-6 | E3 |
| 25 | `CODE_OF_CONDUCT.md`, `GOVERNANCE.md`, `docs/rfcs/` | GOV-1, GOV-2, GOV-5 | E6 |
| 26 | A test and an accessibility query per example; generated-code lint test | DOC-3, A11Y-8, QLT-12 | E3 |
| 27 | Release-health gating of rollouts | STB-7 | E3 |
| 28 | A hostile-environment CI job; probing abort-prone host dependencies before use (F-40, F-41) | TST-17, REL-4 | E3 |

When an item lands, the standards it raises change their tag to
**[Enforced]**, and the findings it fixes are removed from the register,
in the same change.

---

## 38. Changing these standards

These standards are themselves subject to GOV-5. A proposal to add, change,
or retire a standard states which loop question it answers (see
`docs/policy/standards-derivation.md`): a domain not covered, a standard
that covers too little, enforcement that is not enough, or a better
enforcement found. It includes the evidence, executed where the claim
allows (rule 5 of the ladder). Enforcement only ratchets upward (section
0). A standard is retired only when a stronger one replaces it, and the
retired ID is kept, marked *retired*.
