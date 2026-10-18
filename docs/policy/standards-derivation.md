# How the engineering standards were derived

`STANDARDS.md` is the result of a question loop run over the whole
repository. This page records the loop: its questions, the evidence it ran
on, what each pass found, and why it stopped. A proposal to change the
standards answers one of the same questions (`STANDARDS.md` section 36).

## The questions

Each domain was taken through nine questions, in this order:

| # | Question | What it produces |
|---|---|---|
| Q1 | **What?** What does this domain mean for this framework specifically? | The scope |
| Q2 | **Why?** What breaks, and for whom, if it is not held? | The rationale |
| Q3 | **Covers enough?** Does the scope reach every crate, backend, language, and lifecycle stage? | Gaps in scope |
| Q4 | **Explore more?** What adjacent concern does this one touch that has no home yet? | New domains or standards |
| Q5 | **How to enforce?** Which mechanism holds each standard? | The enforcement |
| Q6 | **Why these methods?** Why this rung of the ladder (E1–E6) and not another? | The rung, justified |
| Q7 | **Is this enforcement enough?** Can the standard be broken with every mechanism green? | Holes in enforcement |
| Q8 | **Are there better ways?** Can a higher rung hold it: a type, a denied lint, a generated artifact? | Promotions |
| Q9 | **Ask again from the beginning?** Did this pass find anything at all? | Another pass, or the stop |

Q9 makes the sequence a loop. Every finding of a pass (a new domain, a
scope gap, an enforcement hole, a promotion) went into the next pass as
input. The loop stopped at the first pass in which all nine questions, for
every domain, produced nothing new.

## The evidence

The loop ran against the repository at commit `c2a2e67`, reading:

- **Every tracked file, by script.** Each of the 1,076 files under the
  working tree (outside `.git`) was read and measured for: `unsafe` blocks,
  functions, and impls; `SAFETY:` comments; `unwrap`/`expect`/`panic!`/
  `unreachable!`; `#[allow]` with and without `reason`; crate-level
  attributes; `println!`/`eprintln!`/`dbg!`; process-wide statics and
  `static mut`; `process::exit`; thread spawns and sleeps; `transmute` and
  raw pointers; host `cfg`s; `missing_docs` and `forbid(unsafe_code)`
  declarations; test functions and `#[ignore]` reasons; `#[non_exhaustive]`
  and `#[must_use]`; unbounded channels; lossy `as` casts; credential-like
  literals; plain-`http://` hosts; `// req:` traceability tags; encoding,
  line endings, final newlines, trailing whitespace, and control bytes.
- **Every manifest and configuration file in full.** All 60 `Cargo.toml`
  files, `Cargo.lock` (384 packages), `clippy.toml`, `deny.toml`,
  `rustfmt.toml`, `rust-toolchain.toml`, `.gitignore`, `.gitattributes`,
  `.github/workflows/ci.yml`, and every file in `budgets/`.
- **Every governance and policy document in full.** `CONTRIBUTING.md`,
  `SECURITY.md`, `docs/policy/`, `docs/security/`, `docs/guarantees.md`,
  `docs/telemetry-policy.md`, `docs/server/security-checklist.md`,
  `docs/conformance/`, `docs/performance/`, `docs/invalidation.md`,
  `docs/api/README.md`, `docs/deploy/update-rules.md`, and
  `docs/comparison/methodology.md`. `PLAN.md` section 2 (principles),
  section 9 (cross-cutting quality), and Milestone 65 (engineering
  discipline), plus the heading structure of `PLAN.md`, `BUILD_STATUS.md`,
  `README.md`, and `docs/history/2026-08-29-engineering-audit.md`.
- **The tests that already police the repository.** `doc_claims.rs`,
  `doc_parity.rs`, `describe.rs`, the syntax and style equivalence suites,
  and the compile-fail suites.
- **Code read at each flagged site.** Every scanner hit that could be a
  defect was opened and read in context. The findings below come from that
  reading.
- **Current external references,** checked on 2026-10-18: WCAG 2.2 (a W3C
  Recommendation since 2023, with WCAG 3 still a draft), SLSA v1.1/v1.2
  build levels, `cargo-semver-checks`, OpenSSF Scorecard checks, the EU
  Cyber Resilience Act's reporting dates, Contributor Covenant 3.0, the
  stabilization of `#[expect]` and `allow_attributes_without_reason` in
  Rust 1.81, and the rust-lang/rust LLM policy of August 2026.

### Scan figures at commit `c2a2e67`

| Signal | Non-test code | Tests |
|---|---|---|
| Lines of Rust | 185,360 | 27,125 |
| `unsafe` blocks / `SAFETY:` comments | 974 / 984 | 37 / 38 |
| `.unwrap()` / `.expect(` | 442 / 456 (allowed per site, or invariant) | 1,238 / 197 |
| `#[allow(` with `reason` | 139 of 191 | — |
| `#[expect(` | 0 | 0 |
| `Result<_, String>` signatures | 134 | — |
| Unbounded channels | 19 | 1 |
| `thread::sleep` in test directories | — | 24 |
| Crates denying `missing_docs` | 19 of 28 library crates | — |
| Crates forbidding `unsafe_code` | 0 (18 have no `unsafe`) | — |
| Explicit recursion-depth limits in first-party parsers | 0 | — |
| Files over 2,000 lines | 3 | — |
| Files with control bytes | 3 | — |
| Tracked build outputs | 1 | — |

---

## Pass 1: the seed list

The seed list came from the request: stability, reliability, security,
performance, and developer experience. Pass 1 took each of them through
Q1–Q9. Its Q3 and Q4 answers grew the list to 31 domains before Pass 2.

### Stability

- **Q1 What:** the framework never takes down the process it lives in, and
  leaves the host as it found it.
- **Q2 Why:** a framework panic is a crash in code the application did not
  write and cannot catch.
- **Q3 Covers enough:** no. "Stability" mixed three things: crash-freedom
  (STB), API stability (CMP), and behavioural determinism (COR). They were
  split.
- **Q4 Explore more:** panics crossing foreign frames (MEM), teardown of
  host state, error boundaries, crash evidence and its privacy (PRV).
- **Q5 Enforce:** `unwrap_used`/`expect_used` lints, conformance row 10
  (deliberate panics per backend), teardown tests.
- **Q6 Why these:** lints catch the source at E2. Only a real host shows
  whether the host survived, which takes E3.
- **Q7 Enough:** no. `clippy::panic` and `indexing_slicing` are allowed, and
  `process::exit` is not policed.
- **Q8 Better:** promote `clippy::panic` with reasons. Add a `process::exit`
  scan. Add release-health gating for what tests cannot see.
- **Q9 Again:** yes, three new domains.

### Reliability

- **Q1 What:** correct behaviour under failure: timeouts, retries, partial
  networks, lifecycle interruptions, hostile peers.
- **Q2 Why:** applications built on the framework inherit its failure modes.
- **Q3 Covers enough:** no. Resource exhaustion by an untrusted peer is both
  a reliability and a security concern, and needed a standard in each
  (REL-7, SEC-6).
- **Q4 Explore more:** idempotency (durable, offline replay), offline and
  constrained networks, capability absence, process death, data integrity
  (DAT).
- **Q5 Enforce:** durable, offline, and lifecycle tests. The HTTP server's
  limits test.
- **Q6 Why these:** failure behaviour can only be shown by inducing the
  failure (E3).
- **Q7 Enough:** no. The scan of listeners found the MQTT broker
  allocating a client-declared length of up to 256 MiB before any check,
  with no connection cap and no first-packet timeout. Nothing in the suite
  would notice.
- **Q8 Better:** a limits test per listener. Fuzz targets for every decoder.
- **Q9 Again:** yes.

### Security

- **Q1 What:** secure defaults, untrusted input, cryptography, authorization,
  code loading, secrets, and disclosure.
- **Q2 Why:** every application inherits the framework's attack surface.
- **Q3 Covers enough:** no. The threat models cover Windows, the server, and
  the Web. Code loading, process spawning, and CI as an attack surface
  (SUP) had no home.
- **Q4 Explore more:** privacy (PRV) and supply chain (SUP) split out.
  Updates and packages joined SEC-10. Regulatory reporting (LEG-4).
- **Q5 Enforce:** types (`Secret<T>`, route access, `Html`, `query!`), one
  test per checklist row, `cargo deny`, `cargo audit`.
- **Q6 Why these:** the strongest protections already sit at E1, which is
  the right rung. A route that cannot compile without access cannot ship
  without it.
- **Q7 Enough:** no. A bare-name `LoadLibraryW("dxcore.dll")` in
  `rustnative-windows/src/native/accelerator.rs` follows the default DLL
  search order. No first-party parser bounds recursion depth. No fuzzing
  exists.
- **Q8 Better:** scans forbidding bare `LoadLibrary*` and shell
  interpolation. Fuzzing. Secret scanning with push protection.
  CODEOWNERS with two approvals on crypto.
- **Q9 Again:** yes.

### Performance

- **Q1 What:** declared budgets per target, from cold start to edge fuel.
- **Q2 Why:** without numbers a claim is an anecdote, and regressions
  arrive one harmless-looking change at a time.
- **Q3 Covers enough:** mostly. `budgets/` and `rustnative bench --check`
  cover eight targets. Energy and idle wake-ups had no key.
- **Q4 Explore more:** memory and artifact size (already budgeted), build
  time (budgeted, optional), algorithmic complexity on hot paths,
  scalability over large data (Milestone 54).
- **Q5 Enforce:** the `budgets` CI job, `doc_claims.rs`, `core_costs.rs`.
- **Q6 Why these:** a measured gate on every push is the only way a budget
  holds (E3). A text scan keeps the prose honest.
- **Q7 Enough:** almost. A `max` can be raised in the same change that
  regresses it.
- **Q8 Better:** CODEOWNERS on `budgets/` with a written reason (PRF-2). An
  idle-wake-ups key (PRF-7).
- **Q9 Again:** yes.

### Developer experience

- **Q1 What:** the path from `rustnative new` to a running, debuggable,
  shippable application, and the path from a clone to a merged contribution.
- **Q2 Why:** adoption and contribution both stop at the first confusing
  error.
- **Q3 Covers enough:** no. It covered application developers and left out
  contributors (VCS, CI, GOV) and the documentation they read (DOC).
- **Q4 Explore more:** compile-time diagnostics (EQV-6, DX-3), configuration
  and environment variables, Cargo features, the CLI as a stable interface,
  observability (OBS), the machine-readable API for tools.
- **Q5 Enforce:** trybuild goldens, `describe.rs`, the dev-loop and
  first-run budgets.
- **Q6 Why these:** diagnostics are output, so goldens hold them (E3).
- **Q7 Enough:** no. Feature combinations are untested beyond
  `--all-features`. Environment variables are documented only where they
  are used.
- **Q8 Better:** `cargo hack --each-feature`. A generated
  environment-variable reference.
- **Q9 Again:** yes.

### Domains added by Pass 1's Q3 and Q4

Splitting and exploring the seed list produced these domains, each taken
through Q1–Q9 in Pass 2: architecture (ARC), API design (API),
compatibility (CMP), syntax and style equivalence (EQV), native fidelity
(NAT), correctness (COR), error handling (ERR), memory safety and FFI (MEM),
resource lifetime (RES), concurrency (CON), data integrity (DAT), privacy
(PRV), supply chain (SUP), accessibility (A11Y), internationalization
(I18N), visual quality (UX), documentation (DOC), observability (OBS),
testing (TST), code quality (QLT), version control and workflow (VCS), CI,
release and operations (RLS), portability (PRT), legal and regulatory
(LEG), and governance (GOV).

---

## Pass 2: every domain

| Domain | Q1–Q2: what and why | Q3–Q4: scope gaps and adjacent concerns | Q5–Q6: enforcement and its rung | Q7–Q8: holes and promotions |
|---|---|---|---|---|
| ARC | `PLAN.md` 2.4–2.6: a platform-free core, one place that names a platform, no backend depending on another. Breaking it breaks every backend at once. | Global state on the render path. File and module size. Written decisions before code. | Today held only by review and by the core's manifest. `PLAN.md` Milestone 65 (`C129`) calls for tests. | No test fails if the core gains a platform crate. Promote to E3 with a `cargo metadata` graph test and a host-`cfg` scan. |
| API | The public API is the product. | Typestate, sealing, `must_use`, defaults, no hidden blocking. | `clippy::pedantic`, `missing_docs`, `missing_errors_doc`. | `missing_docs` is absent from 9 library crates while `docs/api/README.md` says every crate denies it. Promote to `[workspace.lints.rust]` so no crate can be missed. |
| CMP | `docs/policy/stability.md` exists, and its promises need teeth. | Wire and persisted formats, host OS and browser floors, capability-package ranges. | MSRV job, codemod corpus, `describe.rs`. | Nothing detects a breaking API change. Promote to E4 with an API snapshot and `cargo-semver-checks`. |
| EQV | `PLAN.md` 2.9 and 2.14. Equality decays unless tested. | Diagnostics quality. Documentation parity outside `docs/guides/`. | Equivalence suites, expansion goldens, compile-fail suites, `doc_parity.rs`. | Strong already. Extend `doc_parity` to all of `docs/`. |
| NAT | `PLAN.md` 2.2–2.3, 2.13: real controls, honest capabilities. | Platform idioms per host (only `docs/idioms/windows.md` exists). | Capability tests, the conformance checklist. | "Decorate the box" waits on Milestone 67's row 25, so review holds it until then. |
| COR | Determinism is what makes the guarantees testable. | Numerics, time, randomness, iteration order, floats, CRDT convergence. | Cast lints denied, proptest, invalidation tests. | Direct clock reads are unpoliced. Add a scan. Hash-order output is caught only by goldens. Add a reproducibility check. |
| ERR | Callers must match failures, and a string can only be shown. | Swallowed errors. Validation at the boundary. | `# Errors` docs lint. | 134 `Result<_, String>` signatures. Add a scan with a burn-down list. Add `let_underscore_must_use`. |
| MEM | About 970 `unsafe` blocks. The discipline is strong (984 `SAFETY:` comments, denied lints). | Non-Rust bridge code (Java, JS, C, C#). Environment mutation. `unsafe impl Send/Sync`. | `undocumented_unsafe_blocks`, `unsafe_op_in_unsafe_fn`. | 18 crates have no `unsafe` and do not forbid it. Promote to `forbid`. Proof quality is review-only, so add tools: Miri, ASan, TSan. |
| RES | Leaked host objects are invisible until a long session fails. | Caches, stale events, threads and child processes. | Census tests per backend, the GDI leak gate. | Bounded caches are tested per cache, not as a rule. Kept at E3 per cache. |
| CON | `PLAN.md` 2.12 and affinity. | Reentrancy, lock order, unbounded channels, executor abstraction. | `!Send` types, cancellation property, `await_holding_lock`. | 19 unbounded channels with no justification. Add a scan with an allowlist. Add a UI-thread stall watchdog in debug builds. |
| DAT | Data outlives versions. | Atomic writes, persisted-format upgrades, export and deletion. | Migration tests, `query!`. | No fixtures for old persisted formats. Add one per format. |
| PRV | Consent and minimization are law and trust. | Hard-coded hosts, platform privacy declarations, in-context permissions. | Consent tests, generated manifests. | 30 plain-`http://` literals outside tests need triage. Add a host scan. |
| SUP | The dependency tree is code the project ships. | Vendoring, CI as attack surface, provenance, reproducibility, update automation, `--locked`. | `cargo deny`, `cargo audit`, `permissions: contents: read`. | Actions are pinned by tag, not SHA. No provenance and no reproducibility check. Promote with Scorecard, SHA pins, and a release workflow. |
| A11Y | An inaccessible framework produces inaccessible applications at scale. | Examples and components as teaching material. Live regions. Target size (WCAG 2.2, 2.5.8). | UIA, AT-SPI, and TalkBack suites, layout conformance. | A person's screen-reader pass is irreducibly E6. A check that every node kind has a role mapping per backend is promotable to E3. |
| I18N | The framework's own strings and layout must work in every locale. | IME, bidi, graphemes, locale formatting, pseudo-localization. | Logical `EdgeInsets` (E1), layout conformance, text-stack tests. | A live CJK IME session is manual and owed. |
| UX | Host fidelity is the framework's reason to exist. | DPI, motion, layout shift, visual regression. | Goldens, reduced-motion tests, the CLS budget. | Adequate. |
| DOC | A doc that disagrees with the code is a defect. | Status honesty, working-note labelling, examples. | Doctests, `doc_claims.rs`, `doc_parity.rs`, `RUSTDOCFLAGS=-D warnings`. | 11 examples have no tests. There is no spelling or link check. |
| OBS | Applications must control where diagnostics go. | Conventions, inspection, render causes, debug-only cost. | Inspection conformance row 26, observe tests. | Library code in `rustnative-core` calls `eprintln!`. Promote with `print_stderr`/`print_stdout` denied in library crates. |
| TST | Tests are how every other standard is held. | Fuzzing, coverage, flakiness, silent skips, goldens, traceability, hardware-only work. | Per-host CI jobs, conformance suites, proptest, reasons on `#[ignore]`. | No fuzzing, coverage, or flake quarantine (all `C130`). 19 places print "skipped" and pass. 24 sleeps in tests. |
| QLT | Readable code is reviewable code. | Suppression reasons, generated code, source hygiene, every non-Rust language. | rustfmt, curated clippy policy, `todo`/`unimplemented` denied. | 52 `#[allow]` without reason and no `#[expect]`. Promote with `allow_attributes_without_reason`. No standard existed for Java, JS, shell, SQL, C, C#, CSS, or TOML. |
| VCS | History is the project's memory. | DCO, CODEOWNERS, branch protection, templates, committed artifacts, binaries. | `.gitattributes` (LF). | A tracked `target/` file exists because `.gitignore` covers only the root. No CODEOWNERS, templates, or DCO. |
| CI | CI defines "builds". | Timeouts, concurrency, cache neutrality, disabled jobs. | The job matrix, `-D warnings`. | Only one job has `timeout-minutes`. No `concurrency`. No workflow lint. |
| RLS | A release is the moment every promise is checked. | Release notes, one version for all crates, publish completeness. | Packaging, update, and deploy tests. `cargo_common_metadata`. | No release workflow yet. Owed with SUP-8. |
| PRT | The plan promises every host class. | `no_std`, adoption, extension points, contributor hosts. | `apple` and `no-std` jobs, adoption examples. | Adequate for the current stage. |
| LEG | Licensing and regulation decide who may use the framework. | DCO, vendored notices, CRA, accessibility law, export, trademarks. | `cargo deny` licences, generated compliance evidence. | The CRA steward dates are unsettled across sources, so the standard requires checking the official text rather than citing a date. |
| GOV | Outside contributors need to know the rules of the room. | Conduct, roles, RFCs, triage, AI-assisted contributions, continuity. | None existed. | Every GOV standard starts owed. AI-assisted contributions needed their own standard (GOV-6). |

### What Pass 2's Q4 found that had no home

- **Source hygiene** (QLT-9). Three files hold control bytes left by broken
  escape sequences in doc comments: a NUL in
  `rustnative-web/src/client.rs`, two backspaces in
  `rustnative-webgen/src/items.rs`, and a BEL in
  `rustnative-style/src/web.rs`. No existing check reads source as bytes.
- **Languages other than Rust** (QLT-10, MEM-7): 31 Java files, two
  JavaScript modules, six shell scripts, SQL migrations, C and C# hosts, CSS,
  and TOML.
- **Environment mutation** (MEM-8). Edition 2024 already makes it `unsafe`.
  The standard records the rule the existing `SAFETY:` comments follow.
- **Generated artifacts** (QLT-8), as a class rather than case by case.
- **Configuration and environment variables** (DX-4), and **Cargo
  features** (DX-5).
- **Energy** (PRF-7).
- **Regulatory obligations** (LEG-4) and **AI-assisted contributions**
  (GOV-6), from the external references.

Each was taken through Q1–Q8 and written as the standard named. Q9: yes.

---

## Pass 3: the standards as a whole

Pass 3 asked the questions of the standards themselves rather than of a
domain.

- **Q1–Q2:** a standard without a stated enforcement is a wish, so every
  standard got an enforcement and a status tag.
- **Q3:** do the standards cover their own failure modes? There was no
  way to break a MUST legitimately, so section 34 (waivers) was added. There
  was no way to change a standard, so section 36 was added.
- **Q4:** a standard enforced only by review decays. That gave rule 4 of the
  ladder: a defect class caught twice by a person becomes a check.
- **Q5–Q6:** the enforcement ladder (E1–E6) and the "highest feasible rung"
  rule turn "how to enforce" from a choice per standard into a policy.
- **Q7:** enforcement can be weakened silently by relaxing a lint or making
  a test optional. That gave the ratchet rule.
- **Q8:** the owed mechanisms were collected and ordered in
  `STANDARDS.md` section 35, so they become work rather than prose. Six
  items fix defects the scan found.
- **Conflicts:** EQV's two syntaxes against API-9 ("one way to do a thing")
  were resolved by naming the syntaxes as the deliberate exception, because
  both produce one tree. PRF-3's rule against unbudgeted claims was applied
  to `STANDARDS.md` itself.
- **Q9:** yes, the waiver, change, ratchet, and second-catch rules were new.

---

## Pass 4: the stop

Pass 4 took all 31 domains and the cross-cutting rules through Q1–Q9 once
more, against the same evidence and the standards as written.

- **Q1–Q2:** every domain had a scope and a rationale.
- **Q3:** every crate, backend, non-Rust language, and lifecycle stage
  (author, review, CI, release, operation) was covered by at least one
  domain.
- **Q4:** no adjacent concern without a home was found.
- **Q5–Q6:** every standard named its mechanism and rung.
- **Q7:** every known hole was either closed by an existing mechanism or
  listed as owed.
- **Q8:** every promotion found was recorded in section 35.
- **Q9:** no new findings, so the loop stopped.

The loop restarts whenever its inputs change: a new crate, backend,
language, listener, or format, a new external requirement, or a waiver that
keeps recurring.
