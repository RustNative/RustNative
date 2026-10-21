# Layer Solidity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> [!IMPORTANT]
> ## ⚠ READ BEFORE ACTING ON ANY FINDING: patch or redesign
>
> **This instruction binds every person and every AI agent that implements, plans, or reviews work from this plan. Do not skip it, summarise it away, or treat it as optional background.** If you are an AI agent, restate this rule in your own plan before you change any code, and apply it to every finding you touch.
>
> **The verdict (2026-10-21, after five iterations and 373 recorded defects):** RustNative does **not** need a ground-up rearchitecture. Its layering, component tree, diff-ops model, and backend split held up across every pass. But it needs **more than patching**: where the same defect keeps recurring because the design invites it, the fix is a **targeted redesign** of that part, not another per-instance patch.
>
> **Redesign is authorised for any part of the codebase that needs it.** The six areas below are examples the loop identified, **not a complete or exclusive list.** Any crate, module, public API, persisted format, protocol, or test instrument may be redesigned when the criteria below are met, including parts this plan currently assigns a patch-sized task.
>
> **Examples of targeted redesigns already justified by the evidence:**
> 1. **Peer trust and ownership** (U24, U29): every registry keyed by an id records its owner; every internal control message carries a per-run token or arrives on a non-exported channel. Evidence: L9-F1/F2 (P0), L9-F27, L9-F29, L9-F7, L6-F14, L6-F56, L6-F57, L6-F59.
> 2. **Durable execution and queues** (U30, U33): leases, compare-and-set state changes, retention, and an idempotency key, owner, and expiry on every deferred write. Evidence: L9-F18, F31, F49, F51–F53; L7-F6; L8-F8.
> 3. **The Rust-to-JavaScript translator** (webgen): type-directed translation, and an evaluation-order-preserving intermediate step. Evidence: L7-F12, F20–F22, F28, F29.
> 4. **The test instruments** (U21, U36, U38): drivers report outcomes, run the product's own path, and every oracle is validated against an injected real failure. Evidence: L4-F1, L4-F17, L4-F19, L6-F47, L10-F24.
> 5. **Windows re-entry and lifetimes** (U31, U5): one guarded entry point and closure-scoped APIs instead of guards whose soundness depends on `Drop`. Evidence: L6-F21, L6-F54.
> 6. **Layout planning versus placement** (U38): space sharing and placement come from one function. Evidence: L3-RP7.
>
> **Redesign instead of patching when any of these holds:**
> - the same defect shape appears at two or more sites in the part, or a universal indicator's "Proven by" list names the part more than once;
> - the patch would add the same check at every call site, where a type, module boundary, or single function could make the bad state unrepresentable (E5, the `illegal-states-unrepresentable` discipline);
> - correctness depends on callers remembering a rule (dropping a guard, holding a lock, passing a token, escaping a string);
> - two code paths compute the same thing (plan and act, server and client, backend and backend, display and invocation) and can disagree;
> - the part's tests cannot witness the behaviour they name, so a patch could not be verified;
> - the patch would leave a documented contract false, or need the documentation weakened to match.
>
> **Patch when none of them holds**: a local missing bound, encoder, timeout, error mapping, or CLI edge case whose fix lives in one place, gated by its indicator.
>
> **How a redesign lands:** the reproducing tests go red first; the redesign is its own task (record it in Part D with the findings it closes and why a patch was not enough); breaking public-API or format changes are allowed when the redesign needs them, with a migration note, a semver bump, and frozen fixtures for the old format. **Never downgrade a redesign to a patch to keep a diff small, and never close a finding whose shape is still reachable elsewhere in the same part.**

**Goal:** Make every layer of RustNative a layer that can be built on safely: each one has an explicit, bounded, measured, and machine-enforced set of solidity indicators, and none of them can silently regress.

**Architecture:** One fixed ten-question method (Part A) applied to a universal indicator set shared by every crate (Part B) and to layer-specific indicators for each of the twelve layers, bottom-up in dependency order (Part C). **Confirmed defects are fixed first (Part D, wave 0)**, then measurement infrastructure lands, because nothing can be enforced that is not measured (wave 1); layer work follows in dependency order (waves 2–9). Part E is the loop record: Q10 turned the ten questions into a loop over a file-level coverage ledger, and it ends only when every file is read and a full pass finds nothing new.

**Evidence base (the loop's output, 2026-10-15):** the per-layer findings files under [`layer-solidity/`](layer-solidity/). Each finding has an ID (`L<n>-F<m>`), a severity, file:line evidence, and ✅ where a scratch program outside the repository reproduced it; each file ends with the indicators the finding adds.
- [L0](layer-solidity/L0-types.md), [L1](layer-solidity/L1-style.md), [L2](layer-solidity/L2-syntax-build.md)
- L3: [part 1](layer-solidity/L3-core.md), [part 2](layer-solidity/L3-core-part2.md), [part 3](layer-solidity/L3-core-part3.md)
- [L4](layer-solidity/L4-reference.md), [L5](layer-solidity/L5-groups.md), [L6](layer-solidity/L6-backends.md), [L7](layer-solidity/L7-web.md), [L8](layer-solidity/L8-app.md), [L9](layer-solidity/L9-server.md), [L10–L11](layer-solidity/L10-L11-cli-ci.md)

Every file's status (`unread`/`skimmed`/`read`) is in [`layer-solidity/ledger.md`](layer-solidity/ledger.md). Where this document and a findings file disagree, the findings file is newer and wins.

**Tech Stack:** Rust 2024 (MSRV 1.85, CI on latest stable), cargo workspace of 29 crates and 31 examples; GitHub Actions (`.github/workflows/ci.yml`); clippy (pedantic + cargo + chosen restriction lints), cargo-deny, cargo-audit; to add: cargo-fuzz (libFuzzer), proptest (already in 3 crates), Miri, ASan/TSan (nightly, Linux), cargo-llvm-cov, cargo-mutants, cargo-semver-checks, cargo-public-api, cargo-hack, loom where a lock-free/sync primitive exists.

**Spec:** the user request of 2026-10-15 (ten questions per layer, quoted in Part A), plus the project's own definitions this plan makes mechanical: `PLAN.md` §9 (cross-cutting quality), Milestone 41 (guarantees), 42 (budgets), 45 (test infrastructure), **65 (engineering discipline — currently unimplemented: `docs/ecosystem-analysis/parity-matrix.md:358-359` records C129 "Absent", C130 "Partial … no flakiness detection, coverage gate, fuzzing, or sanitizers")**; `docs/guarantees.md`; `docs/conformance/new-backend-checklist.md`; the 2026-10-12 review (`rustnative-tools/review-1012/REPORT.md`, `findings.md`, `todo.md`).

## Global Constraints

- This document changes no code. Every task below is a future change; each lands as its own logical commit **directly on `master`** (repository rule), trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- MSRV stays `1.85` (`Cargo.toml:10`, CI job `msrv`); everything must also be clippy-clean on the newest stable CI uses (lint locally with `cargo +<newest> clippy`, `CARGO_TARGET_DIR=target\r199` style, before pushing).
- Nightly-only tools (Miri, sanitizers, `cargo fuzz`) run in **separate, non-blocking-then-blocking CI jobs**; they never become a requirement of the stable build.
- No new runtime dependency is added to any library crate by this plan. Tooling deps go to `[dev-dependencies]`, `fuzz/` sub-crates (excluded from the workspace's publish set), or CI install steps.
- Gates that already pass stay passing: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `RUSTDOCFLAGS=-D warnings cargo doc --workspace --no-deps`, the Linux gate (`rustnative-tools/gate-linux.sh` in WSL), the Android clippy target, `docs/api/framework.json` regeneration when public API changes.
- Hardware-bound verification follows principle 2.13: what cannot be run here is recorded as **owed**, never assumed passing.
- Every ratchet file (counts that may only go down) lives under `tools/ratchets/` and is updated in the same commit that improves it.

## Review Focus

The five failure classes most likely to bite a person building on this framework that no current test exercises (each is pinned by a task in Part D):

1. **Adversarial or simply deep input to a recursive parser** — `.rsx` markup (`rustnative-markup/src/parse.rs`, recursive `parse_element`/`parse_child`), `app.css` (`rustnative-style/src/sheet.rs:125`), Fluent catalogues (`rustnative-i18n/src/lib.rs:340`), `.ril` IDL (`rustnative-interop/src/idl.rs:188`), the terminal input decoder (`rustnative-tui/src/decode.rs:365`), MQTT frames (`rustnative-sync/src/mqtt.rs:178`), the compact inspection format (`rustnative-core/src/inspect/compact.rs:141`). Expected: a bounded error, never a stack overflow, hang, or panic. No depth limit exists anywhere today (grep `MAX_DEPTH|max_depth` → no hits). Owner: Task 1.4 (fuzz targets) and Task 2.3 (depth limits).
2. **A panic escaping across an FFI callback** — 51 `extern "C"/"system" fn` callbacks (20 in `rustnative-android/src/jni_host/natives.rs`, window procedures and UIA subclass procs in `rustnative-windows`, 8 in `rustnative-web/src/wasm.rs`, terminal signal handlers). Since Rust 1.81 an unwind out of `extern "C"` aborts the process: no UB, but the host app dies with no crash report from the framework. Expected: every callback funnels through one guarded entry that converts a panic into the documented teardown policy. Owner: Task 6.1.
3. **Long-running processes and unbounded growth** — server, Live sessions, sync, durable actors and the TUI run for days. Expected: memory flat under a soak of the steady state. Only the surfaces queue and Live sessions were bounded by the fix-all pass; no soak test exists. Owner: Task 1.7 (soak harness) + per-layer bound inventories.
4. **Reading data written by an older version** — persisted component state (`rustnative-core/src/persistence`), `FileStateStore`, the durable workflow log, the offline mutation queue (`rustnative-data`), the update manifest, sync replicas, the inspection wire (`rustnative-core/src/wire.rs`). Expected: every format has a version and a frozen fixture from each released version that still loads. Owner: Task 1.9 + per-layer format tasks.
5. **A forged or misused thread-affinity proof** — `UiThread::claim()` (`rustnative-core/src/affinity.rs:27`) is public and unconditional, so "only the thread that claimed it can produce" holds per value, not per application; a second claim on a worker thread yields a valid-looking proof. Expected: one claim per application/backend, a second claim on another thread fails (or the proof is tied to the `Application` that issued it). Owner: Task 3.4.

---

# Part A — The method

Every layer is taken through the same ten questions, in this order, and each answer is written into the layer's section in Part C. The questions are the user's, made operational:

| # | Question | What a complete answer contains |
|---|---|---|
| Q1 | What indicates this layer is solid? | Indicators at four depths: **basic** (it builds, its tests pass), **standard** (idiomatic Rust norms: documented, linted, no panics in library paths), **production** (bounded resources, stable API and formats, observable failures, budgets), **edge** (adversarial input, concurrency interleavings, host misbehaviour, version skew, resource exhaustion). |
| Q2 | Do they cover all aspects? What is missing? | A coverage check against the **aspect grid** below; every empty cell is either filled with an indicator or justified as not applicable. |
| Q3 | Why is each one important? | The concrete failure in *this* layer that the indicator prevents, with the file it would happen in — never a generic "quality" reason. |
| Q4 | What are the bounds? | A number or a binary predicate per indicator, the direction it may move (ratchet), and its noise tolerance. "Good" is not a bound. |
| Q5 | How is it measured at every facet, reliably? | The instrument, where it runs, how often, and its **reliability grade** (below). If a better instrument exists, name it and either adopt it or say why not. |
| Q6 | How is it enforced and pushed to the extreme? | The **enforcement rung** reached today and the target rung (below). Pushing to the extreme means the highest rung that is technically possible for that indicator. |
| Q7 | Are these enforcement methods the best? | The alternatives considered, and why the chosen one wins (cost, false-positive rate, what it cannot see). |
| Q8 | How are they implemented safely here? | The tasks (Part D) — files, commands, order — and how each lands without breaking the existing gates (non-blocking first, then blocking, ratchet from the current value). |
| Q9 | Does the layer reach the limit or can it go further? | The residual gap between the target rung and the theoretical limit, named explicitly (e.g. "formal verification of the layout solver is possible, not worth it, because …"). |
| Q10 | Enough, or ask again? | A verdict and the **re-check triggers** that reopen the layer. Part E records the second pass. |

## The aspect grid (Q2's checklist)

Every layer is checked against all fourteen aspects. A layer section lists only the aspects that need layer-specific indicators; the universal indicators (Part B) cover the rest.

1. **Correctness** — does what it says on valid input.
2. **Robustness** — refuses invalid/hostile input gracefully, bounded.
3. **Panic and error discipline** — failures are values, not crashes; nothing dropped silently.
4. **Memory and unsafe soundness** — every `unsafe` justified, checked by a tool where one exists.
5. **Concurrency and affinity** — data races impossible, thread rules enforced by types or asserts.
6. **Resource bounds** — memory, handles, threads, queue lengths, file descriptors, time.
7. **Determinism and flakiness** — same input, same output, same test result.
8. **Performance** — budgets with regressions failing the build.
9. **Compatibility** — public API semver, persisted/wire formats, MSRV, platform versions.
10. **Security** — trust boundaries, secrets, integrity.
11. **Observability and diagnosability** — when it fails, the failure says where and why.
12. **Accessibility and i18n** (UI layers) — the output is usable by everyone in every locale.
13. **Documentation and status truth** — every claim has a test; every owed item is recorded.
14. **Architecture integrity** — dependencies point the right way; the layer's contract is the only way in.

## Measurement reliability grades (Q5)

| Grade | Meaning | Example today |
|---|---|---|
| **M0** | Opinion / review only | "unsafe is localized" |
| **M1** | A manual procedure someone can run | the Windows screen-reader pass (`docs/conformance/windows-screen-reader-pass.md`) |
| **M2** | An automated check that exists but is not run on every change | Linux GTK budgets (reference machine only) |
| **M3** | An automated check run in CI on every push | `cargo test --workspace` |
| **M4** | M3, and it cannot regress (gate or ratchet), with known noise | `rustnative bench --check` with `tolerance` |
| **M5** | M4, and adversarial/continuous: it searches for failures rather than replaying known cases (fuzzing, property tests with shrinking, mutation testing, sanitizers, soak) | `rustnative-core/tests/property_tests.rs` (partially: run in CI, but at the default 256 cases) |

A measurement is "reliable enough" when it is **M4 or better** and its false-positive rate is low enough that nobody is tempted to disable it (target: < 1 spurious failure per 200 CI runs, tracked by Task 1.6).

## Enforcement rungs (Q6)

| Rung | Mechanism | Failure is caught |
|---|---|---|
| **E0** | Written in a doc | never automatically |
| **E1** | Review checklist | by a person, sometimes |
| **E2** | Lint at `warn` / non-blocking CI job | visibly, but merges anyway |
| **E3** | Test that must pass | on CI, for the cases someone wrote |
| **E4** | Blocking CI gate or ratchet, including adversarial search | on CI, for cases nobody wrote |
| **E5** | Unrepresentable: the type system, the module system, or the build makes the violation impossible to write | at compile time, for everyone downstream too |

"Push to the extreme" = move each indicator to the highest rung it can physically reach. E5 is preferred wherever it costs no runtime and no ergonomics (the `illegal-states-unrepresentable` discipline); E4 where E5 is impossible (behaviour, performance); E3 only for things no tool can search.

---

# Part B — Universal indicators (every layer)

These hold for all 29 crates. Each layer in Part C inherits them; its section only adds what is specific. For each: **why** (Q3), **bound** (Q4), **instrument + grade** (Q5), **enforcement today → target** (Q6), **why this method** (Q7).

### U1. Builds everywhere it claims to

- **Why:** A crate that compiles only on the developer's OS breaks every other target silently — the Linux gate exists precisely because non-Windows `cfg` breakage was found repeatedly (`rustnative-tools/gate-linux.sh`). Feature combinations are the same problem in another axis: `rustnative-server` builds without its thread pool for `wasm32-wasip1` (`crates/rustnative-server/Cargo.toml` comment), and a feature that only compiles together with another is a broken promise to downstream.
- **Bound:** every crate × every declared target × every feature combination in its powerset (depth 2 for crates with > 6 features) compiles with `-D warnings`, on MSRV 1.85 and on latest stable. Binary: 0 failures.
- **Instrument:** `cargo hack check --feature-powerset --depth 2 --no-dev-deps` per crate; CI jobs `msrv`, `apple`, `no-std`, `test-linux`, `test-windows`, `android`. Grade today M3 for default features, **M0 for feature combinations** (no `cargo-hack` anywhere).
- **Enforcement:** E3 → **E4**: add a `features` CI job (Task 1.1).
- **Why this method:** `cargo hack` is the standard tool; writing feature matrices by hand rots. `--depth 2` bounds CI time; the full powerset runs weekly.

### U2. One lint policy, every exception justified and counted

- **Why:** The workspace lint table (`Cargo.toml` `[workspace.lints]`) is careful, but `#[allow]`/`#[expect]` is how it gets bypassed one site at a time. Counts in non-test sources today: core 43, android 29, linux 29, tui 15, drawlist 13, server 8, rustnative CLI 7. An uncounted allow is a policy that erodes without anyone deciding to.
- **Bound:** every `#[allow(...)]` in non-test code is an `#[expect(..., reason = "...")]` (stable since 1.81, so it warns when it becomes unnecessary) — 0 bare `allow`s; per-crate count of `expect` ≤ ratchet value, monotonically non-increasing. Every crate has `[lints] workspace = true`.
- **Instrument:** a script `tools/ratchets/lints.sh` counting `#[allow(` and `#[expect(` per crate (excluding `tests/`, `#[cfg(test)]` modules, `*_integration.rs`, `device_tests/`), compared with `tools/ratchets/lints.toml`. Clippy lint `clippy::allow_attributes_without_reason` + `clippy::allow_attributes` (restriction) for E5-like compiler enforcement. Grade M0 → M4.
- **Enforcement:** E1 → **E4** (Task 1.2). MSRV note: `#[expect]` requires 1.81 ≤ 1.85 ✔.
- **Why this method:** the lint makes the reason mandatory at the site (where reviewers look); the ratchet stops the count growing. A count alone without reasons would be gamed; reasons alone without a count would grow.

### U3. No panics on library paths, none across FFI

- **Why:** The workspace's own lint comment states it: a framework panic is a crash "they cannot catch at a call site they did not write". `unwrap_used`/`expect_used` are only `warn`, so `-D warnings` makes them fail CI — except where an `#[allow]` was added. Raw counts (includes in-file `#[cfg(test)]` modules, so they overstate): core 104 `unwrap()` / 33 `expect(` / 20 `panic!|unreachable!`; linux 84 `expect(`; CLI 64/78. Indexing (`indexing_slicing = "allow"`) and arithmetic overflow (`arithmetic_side_effects = "allow"`) are panic sources the policy accepts by decision. Across FFI, see Review Focus 2.
- **Bound:** (a) in non-test library code, `unwrap`/`expect`/`panic!`/`unreachable!`/`todo!` occur only under an `#[expect(..., reason)]` naming the invariant, and the count per crate is ratcheted; (b) **0** `extern "C"/"system" fn` bodies that are not wrapped by the crate's single guarded-entry helper (verified by a source test, Task 6.1); (c) `overflow-checks = true` in the `release` profile of the test/bench builds so arithmetic panics are found by tests rather than wrapping silently in production (decision recorded in Task 1.3).
- **Instrument:** `cargo clippy` with tests exempt (`clippy.toml` `allow-unwrap-in-tests = true` already); the lints ratchet (U2) counts the `expect`s that carry those allows; `tools/ratchets/ffi_entries.sh` lists every `extern ... fn` and checks its first statement. Proptest/fuzz (U6) actually *exercises* the remaining panic paths. Grade M3 → M5.
- **Enforcement:** E3 → **E4** (ratchet + FFI source test). E5 for FFI by making the guarded helper the only way to produce the callback (a macro `guarded_callback!` that emits the `extern fn`), so a raw `extern fn` stands out as a lint violation (`clippy::disallowed_macros`/a grep gate).
- **Why this method:** "no panics" cannot be proven by grep (indexing, overflow, `RefCell` borrows, `Option::unwrap` in deps); grep bounds the *declared* ones, fuzzing and property tests find the undeclared ones. Tools like `no-panic` (link-time check) only work on tiny leaf functions with optimization and are unusable on this codebase broadly; they are adopted for the few pure decoders where they fit (Q9 residual).

### U4. No silently dropped errors

- **Why:** The review found Windows classifies every ignored native result while Linux/Android dropped the same classes with bare `let _ =` (`findings.md`, "Error discipline differs"); the fix-all added `best_effort` helpers for Android/Linux. Raw `let _ =` counts in non-test code remain: CLI 90, interop 67, linux 64, build 47, core 42, tui 31, android 27, sync 26, server 26. Each is either a deliberate "can't be handled, logged" or a bug.
- **Bound:** every `let _ = <expr returning Result>` in non-test code is either replaced by the crate's classified helper (`best_effort(...)`, which logs with a category) or carries a `// ignored: <reason>` comment on the same line; ratchet the remaining count per crate to 0 over waves 2–9. `#[must_use]` on every public fn returning a value that is an error or a handle (`clippy::must_use_candidate` is pedantic → already warned).
- **Instrument:** `clippy::let_underscore_must_use`, `clippy::let_underscore_untyped` (restriction) at `warn` for library crates; ratchet script. Grade M0 → M4.
- **Enforcement:** E1 → **E4**; E5 via `#[must_use]` on result-like types.
- **Why this method:** the two clippy lints see type information grep cannot (`let _ = vec.pop()` is fine; `let _ = file.flush()` is not).

### U5. Unsafe is minimal, justified, and machine-checked

- **Why:** `unsafe` occurrences in `src/`: windows 783, linux 86, tui 57, android 30, interop 18, web 14, server 12, rustnative 7, build 4, core 1, webgen 1. `undocumented_unsafe_blocks = "deny"` and `unsafe_op_in_unsafe_fn = "deny"` already force a `SAFETY:` comment (E4 for *presence*). Nothing checks that the comment is *true*: no Miri, no sanitizers, no inventory.
- **Bound:** (a) each crate has `#![forbid(unsafe_code)]` unless it is on the allow-list `{windows, linux, android, tui, interop, web, server, rustnative, build, core, webgen}`; each on the list has one module documented as its ownership module (M39) and an unsafe count ratcheted in `tools/ratchets/unsafe.toml`; (b) **Miri clean** for every test that exercises pure-Rust unsafe (core's 1 site, interop runtime, web wasm glue, tui width table, drawlist if any); (c) **ASan + TSan clean** on Linux for `rustnative-linux` (GTK/AT-SPI FFI), `rustnative-tui` (termios, signals), `rustnative-interop` (C ABI); (d) Windows: Application Verifier (handles, heaps, locks) over `rustnative-windows` integration tests — the native equivalent of ASan on Win32.
- **Instrument:** a ratchet script counting `unsafe` blocks/fns/impls per crate; `cargo +nightly miri test -p <crate> <filter>`; `RUSTFLAGS=-Zsanitizer=address cargo +nightly test -Zbuild-std --target x86_64-unknown-linux-gnu`; `appverif.exe` driven by a PowerShell harness on `windows-latest`. Grade M0 → M5 (sanitizers search memory states).
- **Enforcement:** E4 for presence of SAFETY → **E4 for soundness** on Linux/pure-Rust (blocking after one green week), **E2 for Windows Application Verifier** (non-blocking; Windows runners are noisy) → E4 once stable. E5 for the 18 crates that need no unsafe: `#![forbid(unsafe_code)]` makes it unwritable.
- **Why this method:** Miri cannot execute FFI, so it only covers the pure-Rust unsafe; sanitizers cover FFI on Linux but need nightly + build-std; Application Verifier is the only practical memory/handle checker for Win32. Together they cover every unsafe site except Android JNI (owed: run the device suite under `hwasan` on an API 35 emulator image — Q9 residual).

### U6. Test depth matches risk

- **Why:** Test counts (attribute occurrences) are lopsided against risk: `rustnative-update` (signed-update verifier, security-critical) has 4 tests; `rustnative-i18n` (CLDR plural rules for every locale) 3; `rustnative-types` 1; `rustnative-interop` (C ABI generator + runtime) 7; `rustnative-style` (the style parser every app goes through) 28 unit tests and no `tests/` dir; `rustnative-server-macros`, `rustnative-web-macros`, `rustnative-web-testing` 0. Property tests exist in only three files (`core/tests/property_tests.rs`, `conformance/tests/cancellation_property.rs`, `sync/tests/crdt.rs`). Fuzz targets: **0**. Mutation testing: none — so nobody knows whether the existing tests would notice a broken branch.
- **Bound per crate class:**
  - parsers/decoders/verifiers: a fuzz target each, ≥ 1 CPU-hour per target per week without a new crash; a seed corpus checked in; every crash becomes a regression test;
  - algorithms with laws (diff, layout, merge, plural, color conversion, easing, version order): property tests at ≥ 1 024 cases in CI (`PROPTEST_CASES`), 100 000 nightly;
  - every crate: **mutation score** ≥ 70 % on changed files (cargo-mutants `--in-diff`), whole-crate score recorded weekly; security-critical crates (`update`, `server/src/auth`, `server/src/security.rs`, `server/src/cache.rs`) ≥ 90 %;
  - **line coverage on changed lines** ≥ 80 % (cargo-llvm-cov + diff), whole-workspace coverage reported, not gated.
- **Instrument:** cargo-fuzz in `fuzz/` crates; proptest; cargo-mutants; cargo-llvm-cov. Grade M3 → M5.
- **Enforcement:** E3 → **E4** (fuzz smoke 60 s/target per PR blocking; long fuzz nightly; mutants on diff blocking for the security set; coverage diff blocking at 80 %).
- **Why this method:** coverage says a line ran, not that a test would notice it being wrong — mutation score is the stronger signal, so it gates the critical set; coverage is the cheap breadth signal. Fuzzing beats hand-written negative tests for parsers because nobody guesses the 40-level nested input.

### U7. Deterministic and flake-free

- **Why:** CI history: the Windows golden failed by machine class, the Android transition test failed 1/48 on timing, the filter-demo typing test failed once in WSL under load (`RESUME.md`). Every flaky test teaches people to re-run instead of read. Headless already runs single-core (`taskset -c 0`, CI job `headless`) — that discipline is not applied elsewhere.
- **Bound:** every test passes **20/20** repeated runs on its CI OS (nightly job); 0 tests depend on wall-clock time where a `ManualClock`/test executor exists (M45); every randomized test logs its seed; quarantined tests carry an owner and an expiry date (file `tools/ratchets/quarantine.toml`), and expired entries fail CI.
- **Instrument:** nightly job: `cargo test --workspace -- --test-threads=1` ×20 (split by crate, matrix), plus a run with `--test-threads=64` to force contention; `cargo nextest` with `--retries 0` and JUnit output so a flake history accrues (`nextest` archive per run). Grep gate: `Instant::now|SystemTime::now|thread::sleep` in `tests/` outside an allow-list. Grade M2 → M4.
- **Enforcement:** E1 → **E4** (quarantine expiry is blocking; the nightly ×20 opens an issue rather than blocking pushes — blocking pushes on a nightly statistic is unreliable).
- **Why this method:** repeated runs are the only reliable flake detector; nextest gives per-test timing for sharding (M65 C130) for free.

### U8. Every growth has a bound or an owner

- **Why:** The review found an unbounded surfaces queue, an unbounded response cache, a rate-limiter table cleared wholesale at 100 000 keys, one OS thread per Live session, and a never-shrinking interner. All fixed or documented — found by reading. The next one will be found the same way unless growth is inventoried.
- **Bound:** every `Vec`/`VecDeque`/`HashMap`/`BTreeMap`/channel that can grow with *external* input (network, user, host events, time) has (a) a capacity bound with an explicit overflow policy (drop-oldest, refuse, back-pressure), or (b) a documented drain owner and a test that the drain runs; soak: steady-state RSS slope ≤ 1 % per hour over a 6-hour run for server, Live, sync, durable, TUI, and each native backend's idle window.
- **Instrument:** an inventory file per crate (`docs/bounds.md` section per crate: structure → bound → policy → test); a soak harness (Task 1.7) sampling RSS (`/proc/self/status VmRSS`, `GetProcessMemoryInfo`) and handle/fd counts (`/proc/self/fd`, `GetGuiResources`, `GetProcessHandleCount`) every 10 s; tokio task count where tokio runs. Grade M0 → M4.
- **Enforcement:** E0 → **E4** for the soak slope (weekly, blocking release tags); E3 for each bound's overflow-policy test; E5 where possible via bounded types (`BoundedQueue<T, const N>` from the existing code, never `Vec` for external-input queues — a `clippy::disallowed_types` entry per crate for `std::sync::mpsc::channel` in favour of `sync_channel`).
- **Why this method:** static analysis cannot know which collections grow with external input; an inventory makes the decision explicit and the soak catches what the inventory missed.

### U9. Concurrency rules enforced by types first

- **Why:** The core's affinity model (`rustnative-core/src/affinity.rs`) is the right design — `!Send` tree types, `UiThread` proofs, debug-asserted `ThreadAffinity`. `Mutex<|RwLock<` counts: core 28, server 17, sync 14, observe 10, CLI 7, linux 7. Lock ordering is undocumented; deadlocks are found in production.
- **Bound:** (a) every type holding tree state is `!Send` — asserted by `static_assertions`-style compile tests (`fn _assert_not_send<T: ?Sized>()` trybuild compile-fail cases); (b) every `Mutex` in a crate is listed in a lock-order doc comment at its module with a rank, and no code path takes a lower rank while holding a higher one (debug-mode checker: `parking_lot` `deadlock_detection` feature in tests); (c) any hand-written synchronization (atomics with orderings other than `SeqCst`, custom wakers) has a loom model test; (d) TSan clean (U5).
- **Instrument:** trybuild (already a dev-dep of conformance); `parking_lot/deadlock_detection` in a test-only feature with a background checker thread in test harnesses; loom; TSan. Grade M0–M3 → M5.
- **Enforcement:** E5 for `!Send` (already, plus compile-fail tests to keep it), E4 for TSan/loom, E3 for deadlock detection.
- **Why this method:** types are free and permanent; TSan finds races in FFI-adjacent code that types cannot see; loom is exhaustive but expensive, so only for custom primitives.

### U10. Performance has budgets, and the budgets are measured where they apply

- **Why:** `budgets/*.toml` exist for windows, linux, android, headless, web, serverless, tui; CI runs a matrix job `budgets`. Owed: memory only measured on Windows (`RESUME.md` item 1: `peak_memory_mb()` uses PowerShell `PeakWorkingSet64`); Linux frame times only on the reference machine; the Windows artifact budget measures the bench harness, not a shipping-shaped app (`budgets/windows.toml` comment). Core has a criterion bench (`rustnative-core/benches/core_benchmarks.rs`) whose results are not compared with anything.
- **Bound:** every key in every budget file is **measured on its CI OS** (a key reported `MISS` fails the job unless marked `optional = true` with a reason); every core micro-benchmark has a budget key (`budgets/core.toml`: diff of 1 000/10 000 nodes, layout of 1 000 nodes, dispatch, scheduler wake) with tolerance from 10 observed runs; regressions > tolerance fail.
- **Instrument:** `rustnative bench --check`; criterion with `--save-baseline` replaced by an instruction-count measurement on Linux (`iai-callgrind`/`cachegrind`) because wall-time benches on shared runners have ±30 % noise and instruction counts have < 1 %. Grade M3 (partial) → M4.
- **Enforcement:** E4 (already for most keys) → E4 everywhere, with instruction-count gates for the core.
- **Why this method:** instruction counts are the only reliable CI-hosted micro-benchmark signal; wall-time stays for user-visible budgets on the reference machine.

### U11. The public API and every persisted format are artifacts

- **Why:** `docs/api/framework.json` snapshots the `describe` surface and a test fails when it drifts (`crates/rustnative/src/describe.rs:15`) — good, but it covers the *framework description*, not the Rust items. M65 C128 asks for the crate items snapshot and semver gating; none exists. Persisted formats: see Review Focus 4.
- **Bound:** (a) per published crate, `cargo public-api` output committed under `docs/api/<crate>.txt`; a diff without a matching version bump fails CI; `cargo semver-checks check-release` passes against the last tag (once 0.1.0 is tagged; before that, the snapshot diff alone gates); (b) sealing policy: every public enum that may grow is `#[non_exhaustive]`, every trait not meant for downstream implementation is sealed — audited by a script listing public enums/traits without either; (c) every persisted/wire format has a `version` field and a fixture per released version under `tests/fixtures/<format>/v<N>.*` that must keep loading.
- **Instrument:** cargo-public-api (nightly rustdoc JSON — run in a nightly job, snapshot checked on stable by text diff); cargo-semver-checks; fixture tests. Grade M3 (describe only) → M4.
- **Enforcement:** E3 → **E4**; E5 via `#[non_exhaustive]`/sealed traits.
- **Why this method:** public-api snapshots make every change visible in review; semver-checks knows the rules humans misjudge (adding a field to an exhaustive struct is breaking).

### U12. Every claim has a test; every owed item is recorded

- **Why:** M41's rule ("every claim the framework makes is a guarantee with a named test, or it is not made", `docs/guarantees.md`) is the project's best idea, held at E1: the review found the Web called "complete" with most checklist cells `—`, and Android under-advertising. Doc drift was the largest finding class.
- **Bound:** (a) every test path named in `docs/guarantees.md` and `docs/conformance/new-backend-checklist.md` resolves to an existing `#[test]` fn (a link checker); (b) every capability a backend advertises (`capabilities::realized`) has a checklist row with a test, and every capability it realizes per the parity matrix is advertised; (c) `missing_docs = "warn"` (→ `-D warnings`) on every published crate; (d) doctests compile and run.
- **Instrument:** `tools/ratchets/claims.py` (`python -I`): parses the two docs, extracts `` `path::fn` `` references, checks with `grep -rn "fn <name>"` in the named file; a capability-parity test per backend reading its `realized` set and the checklist column. Grade M1 → M4.
- **Enforcement:** E1 → **E4**.
- **Why this method:** the claims are already written in a machine-readable-enough form; checking them is cheap and catches exactly the drift class the review found most of.

### U13. Architecture boundaries are tests

- **Why:** `rustnative-core/src/lib.rs` states the core "never talks to an operating system" and that each module "depends only on the modules below it in the table"; `PLAN.md` 2.4 states the core is platform-free. Today these hold by review (parity matrix: C129 "Absent").
- **Bound:** (a) dependency direction: a test over `cargo metadata` asserting the allowed crate graph (core → {types, style, i18n, macros} only; no backend depends on another backend; `host` is the only crate depending on several backends; no library crate depends on `rustnative` (CLI) or examples); (b) core contains 0 `cfg(target_os`/`cfg(windows`/`cfg(unix` (grep gate), and its dependency tree contains no `windows-sys`, `gtk4`, `jni`, `libc` (cargo tree gate); (c) core module layering: `use crate::<higher>` from a lower module fails (a small `syn`-based or grep test over `rustnative-core/src/*` using the module-map table order); (d) each backend's host calls confined to its ownership module(s) (grep for `windows_sys::`/`gtk4::`/`jni::` outside an allow-listed module set).
- **Instrument:** a workspace test crate `tools/archtest` (excluded from publish) or tests in `rustnative-conformance/tests/architecture.rs`. Grade M0 → M4.
- **Enforcement:** E0 → **E4**; dependency direction is E5 in principle (Cargo refuses cycles) but not for *unwanted* acyclic edges, hence the test.
- **Why this method:** a test that names the principle it protects ("2.4: the core must not depend on windows-sys") fails with an explanation, unlike a reviewer's memory.

### U14. Supply chain is gated and reproducible

- **Why:** `deny.toml` denies unknown registries/git and wildcard deps; `cargo audit` runs. Gaps: GitHub Actions are pinned by tag (`actions/checkout@v4`, `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`) — a moved tag runs foreign code with the repository's token; `rust-toolchain.toml` says `channel = "stable"` (unpinned: CI and local diverge, which is why "CI runs newer Rust than local" is a standing memory); `bans.multiple-versions = "warn"`; no SBOM, provenance, or reproducibility check (M65 C131).
- **Bound:** every action pinned by full commit SHA with the tag in a comment; Dependabot (actions + cargo) proposals gated by the full suite; `cargo deny check` blocking incl. `multiple-versions = "deny"` with explicit skips; release artifacts carry a CycloneDX SBOM and a SLSA provenance attestation (`actions/attest-build-provenance`); a reproducibility job builds the CLI twice in different paths/times and compares hashes (with `--remap-path-prefix` and `SOURCE_DATE_EPOCH`), documenting any residual variance.
- **Instrument:** `pinact`/a grep gate for `uses: .*@v`; cargo-deny; cargo-cyclonedx; diffoscope on mismatch. Grade M3 (partial) → M4.
- **Enforcement:** E3 → **E4**.
- **Why this method:** SHA pinning is the GitHub-recommended mitigation; reproducibility is the only way to verify a binary was built from the source it claims.

### U15. Failures are diagnosable

- **Why:** When a layer fails in the field, the error must say which layer, which object, which input. Windows has a classified error type (`rustnative-windows/src/error.rs`, 457 lines); others vary. The inspection protocol (M44) and crash capture (core `crash`) exist.
- **Bound:** every public error type implements `std::error::Error` + `Display` with the failing object named, `source()` chained; no `String` errors in public signatures of library crates (today: `rustnative-style::parse_color -> Result<_, String>`, `parse_value`, `parse_shadow`, `rustnative-sync::device::decode`, `mqtt::decode`, `server::jobs::parse`, `server::db::schema::parse` — all `Result<_, String>`); every backend answers the inspection protocol (checklist row) with a test.
- **Instrument:** a public-API scan (from U11's snapshot) for `Result<.*, String>` and `Box<dyn Error>` in public signatures; ratchet. Grade M0 → M4.
- **Enforcement:** E0 → **E4** via the API-snapshot grep; E5 by typed errors.
- **Why this method:** typed errors make "which failure" matchable by callers; a string error forces string matching downstream.

### U16. Trust boundaries validate and bound input

- **Why:** Every layer that reads bytes from outside the process (files, network, host IPC, terminal, other processes) is a trust boundary. Server's `body_limit` (1 MiB default, `server/src/security.rs:63`) is the only explicit size limit found.
- **Bound:** every trust-boundary reader has (a) a size limit, (b) a nesting-depth limit if recursive (default 128, configurable), (c) a time/fuel limit if it can loop, (d) a fuzz target (U6), (e) a test for each limit's refusal. Inventory in `docs/bounds.md` (shared with U8).
- **Instrument:** fuzzers with `-max_len` above the limit; per-limit tests. Grade M0 → M5.
- **Enforcement:** E0 → **E4**.
- **Why this method:** limits are the only defence against inputs that are valid but huge; fuzzing finds the paths that bypass them.

### U17. Teardown leaves the host safe on every exit path

- **Why:** `rustnative-core/src/teardown.rs` defines `TeardownPolicy`/`Restoration`; the TUI guard (`rustnative-tui/src/guard.rs`) is a model implementation (normal exit, panic hook chained, signals, SIGTSTP, Windows console events). Each backend owes the same exhaustiveness.
- **Bound:** for each backend, a test per exit path in its policy table: normal, panic (caught and uncaught), signal/OS shutdown request, parent death; after each, the host state is asserted restored (terminal modes, tray icons removed, single-instance lock released, temp files removed, state flushed).
- **Instrument:** per-backend teardown tests spawning a child process and killing it in each way. Grade varies (TUI M3; others M0–M2) → M4.
- **Enforcement:** E3 → E4 via the checklist row requiring each path.

### U18. Status is honest

- **Why:** see U12; additionally BUILD_STATUS/README headers drifted. Status text must be generated from, or checked against, the same sources the tests use.
- **Bound:** README "Current status" and BUILD_STATUS header backend lists are checked against a single machine-readable status file (`docs/status.toml`: backend → state → checklist completeness %), which the claims checker (U12) derives from the checklist.
- **Enforcement:** E1 → E4.

## Cross-cutting indicators found by the evidence loop (U20–U28)

The file-by-file loop (Part E) found the same defect *shapes* in several unrelated layers. A shape that recurs is a missing universal indicator, so each becomes one. Each one below names the findings that prove the shape, why it matters, its bound, the instrument that measures it, and the enforcement.

### U20. A transient failure never becomes a default value

- **Proven by:**
  - L3-F17: a state load error caches `None`, then overwrites real data.
  - L5-F6: a model's `current` read error is treated as version `"0"`, which opens a downgrade path.
  - L5-F5: an unparsable version part is treated as `0`.
  - L8-F1: a truncated cache file fails forever.
  - L1: `opacity: nan` becomes 0.
  - L2: a malformed `rustnative.toml` is ignored.
  - L3-F18: a schema mismatch reads as absent.
  - L10-F9 ✅: a `db rollback --to` name that matches no applied migration reverses every migration.
  - L10-F11 ✅: `tokens import` reads an unreadable style file as empty and overwrites it.
- **Why:** a default looks like valid data, so the next write turns a temporary fault into permanent loss, or into a security decision such as a downgrade.
- **Bound:** 0 sites where an `Err` from I/O, parsing, or decoding is mapped to a domain default (`unwrap_or_default`, `.ok().flatten()`, `unwrap_or(0)`) on a path that later writes, decides, or persists. Every such site either propagates the error or records it as a distinct state (`Unknown`/`Corrupt`/`Quarantined`).
- **Instrument:** a review-enforced lint list (`clippy::disallowed_methods` for `Result::ok` and `unwrap_or_default` in `persistence/`, `services/`, `update/`, `data/`), plus fault-injection stores and decoders that fail exactly once (an M5 test per persisted format).
- **Enforcement:** E0 → E4.

### U21. The instrument takes the product's path

- **Proven by:**
  - L4-F1: headless rebuilds from the snapshot while Windows, GTK, and Android apply diff ops, so L3-F1 is invisible to every test.
  - L4-F2: graceful "kill" masks L3-F4.
  - L4-F5 and L6-F8: virtual-list extents come from estimates.
  - L4-F3: `click` is a no-op on controls.
- **Why:** a test that exercises a different code path than production proves nothing about production.
- **Bound:** every reference or test driver consumes exactly the interface the product consumes (ops, real process kill, measured extents), with a shadow oracle comparing both paths on every settle.
- **Instrument:** the L4.1 shadow-tree oracle, and a mutation test that deletes one field from `node_changed` and must fail at least one test.
- **Enforcement:** E0 → E5 (the driver trait makes the other path unreachable).

### U22. Every input-proportional loop is bounded by output or by a limit

- **Proven by:**
  - L5-F12 ✅: 69 s to rasterize an off-screen ellipse.
  - L5-F13: stroke perimeter.
  - L3-F26: grid placement allocates up to the row index.
  - L6-F4: an unterminated paste or CSI buffers without bound.
  - L1: calc recursion.
  - L3-F8 ✅: stack overflow at depth 2 000–4 000.
  - L9-F4: Live frames and sessions per client.
  - L8-F2: image bytes and dimensions.
  - L7-F11 ✅: form and query decoding is quadratic in the field count and runs on the async workers.
  - L6-F40 ✅: TUI paint is quadratic in rows; L6-F37 ✅: a cache key that prints every pixel of an image.
  - L3-RP2 ✅: the UI task pool runs a self-waking task forever inside one pass.
  - L2-F30 ✅: a 77-byte Fluent term expands to 400 MB; L3-RP6 ✅: dropped sleeps keep timer entries that every pass scans; L9-F49: every job claim scans the queue's whole history; L9-F53 ✅: a signal-waiting workflow replays its whole journal on every tick.
- **Why:** these are freezes, aborts, and denial of service reachable from data, not from code.
- **Bound:** every loop or allocation whose trip count derives from input is bounded either by the visible output (clip first) or by a documented limit that returns an error. A cooperative scheduler polls each task at most once per pass.
- **Instrument:** fuzz targets with time and memory caps (libFuzzer `-timeout=1 -rss_limit_mb=512`), and per-command micro-benchmarks at 10⁹-sized inputs.
- **Enforcement:** E0 → E4.

### U23. Global side effects respect containment

- **Proven by:**
  - L6-F1 ✅: a *caught* panic restores the terminal while the app keeps running.
  - L3-F6: a contained panic writes a crash report.
  - L6-F2: a broken tree loops on panics.
- **Why:** a panic hook is process-global; making it act on panics the framework itself contains breaks the containment promise.
- **Bound:** a contained panic leaves every process-global and host state unchanged except one bounded, rate-limited diagnostic record.
- **Instrument:** the L3.2.7 containment matrix, extended with host-state assertions (console mode, alt screen, cursor, crash-directory entry count).
- **Enforcement:** E0 → E4.

### U24. Nothing a peer sends is trusted as identity, authority, or state

- **Proven by:**
  - L9-F1 ✅: sequential Live session ids, resumable by anyone.
  - L9-F2 ✅: a client snapshot becomes server state.
  - L9-F3 and L3-F12: events at disabled or hidden nodes are applied.
  - L3 part 3: wire `node_id` addresses any component, and `Foreign` creates host content.
- **Why:** server-interactive mode, the inspector, and sync exist so that authority stays on the trusted side.
- **Bound:** every frame from a peer is checked against server-held state (the last tree sent, the session principal, a sealed snapshot) before it changes anything.
- **Instrument:** a protocol fuzzer replaying mutated client frames against a reference app with a canary "authority" field that must never change, and the L9.1–L9.3 tests.
- **Enforcement:** E0 → E5 (types: `Sealed<Snapshot>`, `AuthorizedEvent`).

### U25. One atomic-write primitive

- **Proven by:**
  - L5-F7: update records are not fsynced.
  - L10-F1: `fmt` rewrites with `fs::write`.
  - L8-F1: image cache.
  - L6-F7: staged update files.
  - L3: `FileDocuments::write`.
  - By contrast, `FileStateStore` does it right (L3-F29).
- **Why:** the correct pattern exists once in the codebase and is reimplemented worse five times.
- **Bound:** 0 calls to `std::fs::write` and `File::create` on a destination path outside tests and build scripts. All writes go through `rustnative_core::fs::write_atomic` (temp → fsync → rename → dir fsync).
- **Instrument:** `clippy::disallowed_methods`, plus a crash-injection test (kill during write) per writer.
- **Enforcement:** E0 → E4.

### U26. Every output context has its own encoder

- **Proven by:**
  - L7-F1: `javascript:` URLs.
  - L7-F2 and L1: CSS values and tokens.
  - L7-F3: attribute names.
  - L6-F5: HTTP header CR/LF.
  - L9-F5: cookie attributes.
  - L5-F19: target-language keywords.
  - L6-F36: Android drop data and widgets use U+0001 as an in-band separator inside text other apps supply.
  - L8-F12 ✅: endpoint parameters are spliced into the URL path unescaped.
  - L2-F31, L5-F24 ✅: translator comments and IDL docs are written into generated Rust, C, and C# unescaped; L10-F22: paths go through space-separated `RUSTFLAGS`; L10-F23: `crash show` prints peer text to the terminal unfiltered.
  - By contrast, HTML text and attribute values, script JSON, terminal cells, and OSC payloads are correct.
- **Why:** escaping right in one context says nothing about the next.
- **Bound:** every sink (URL attribute, CSS value, CSS identifier, attribute name, header, cookie, generated identifier, shell argument, terminal) accepts only a typed value from that context's validator.
- **Instrument:** a context-sink inventory (grep for `format!` into each sink), and the OWASP corpus per sink.
- **Enforcement:** E1 → E5 (newtypes).

### U27. Release behaviour does not depend on ambient environment unless declared

- **Proven by:**
  - L3-F20 and L6-F8: `RUSTNATIVE_INSPECT` turns on an inspector in release builds.
  - L5-F2: `RUSTNATIVE_TARGET`/`--tui` anywhere in argv.
  - L3 part 3: env vars change release behaviour.
  - L6-F6: crash directory falls back to `temp_dir`.
- **Why:** the environment is attacker-influenced in many deployments (CI runners, shared hosts, launchers).
- **Bound:** a generated table of every `std::env::var` read in library crates, each marked `debug-only`, `declared-config`, or `test-only`. A release build ignores the debug-only ones.
- **Instrument:** a source test over `env::var(_os)` sites against `docs/env-vars.md`.
- **Enforcement:** E0 → E4.

### U28. Backends agree on observable behaviour

- **Proven by:**
  - L6-F11: shortcuts on non-Latin layouts work on Windows but not GTK.
  - L6 HTTP row: three timeout behaviours.
  - L1-P1: breakpoint precedence differs between Web and native.
  - L5-F16: drawn-host focus differs from native.
  - L4-F8: a shared driver too narrow to see any of this.
- **Why:** "write once" only holds if the same input gives the same observable result on every backend.
- **Bound:** every row of the backend checklist is expressed as one shared conformance suite run on every backend, and a divergence is a failure unless it is listed in `docs/conformance/divergences.md` with a reason.
- **Instrument:** an L4.4 driver covering every input category.
- **Enforcement:** E2 → E4.

## Cross-cutting indicators found in iteration 2 (U29–U33)

These shapes recurred across layers in iteration 2, session 3. Each has the same form as U20–U28.

### U29. Local listeners and control planes authenticate their callers

- **Proven by:**
  - L9-F16: the local deployment's control API turns CSRF off and makes every route public, so any web page can drive it.
  - L3-F19: the inspector's random token is right, but it is written to a shared temp directory, so any local user can read it.
  - L6-F14: `WM_COPYDATA` deep links are accepted from any same-session process.
  - L6-F39: any Android app can switch on inspection in a release build with an intent.
  - L6-F56, L6-F57, L6-F59: the Windows single-instance mutex and window class, and Android's surface-action, window, and update-status extras, are framework control messages that any same-user process or installed app can send, accepted because their fields look right. The bound covers every internal control intent and IPC message: it carries an unguessable per-run token or arrives on a non-exported component.
- **Why:** "only reachable locally" is not authentication. Every process of every user on the machine, and every browser page that can reach `localhost`, is a caller.
- **Bound:** every listener (TCP, pipe, D-Bus name, window message) names its authentication in one table. A listener without a token, a peer-credential check, or an OS ACL, or whose secret sits where other users can read it, is a failure.
- **Instrument:** a source test over `bind`/`listen`/`own_name`/window-message sites against that table, plus a test per listener that an unauthenticated call is refused.
- **Enforcement:** E0 → E4.

### U30. Queues make progress past poison items, and work is leased

- **Proven by:** L9-F18 and L9-F31 (job and outbox queues stall or loop on one failing item), L9-F39 (a failed saga compensation is never retried), L8-F8 (the data layer's offline mutation queue), L9-F51 ✅ (two workflow runners execute a step twice, then the loser compensates and fails the execution: durable state changes must be compare-and-set on the state they read).
- **Why:** one bad item must not stop every item behind it, and a crashed worker must not hold an item forever.
- **Bound:** every queue has a retry limit, a dead-letter place, and a lease with an expiry. A test enqueues a poison item followed by a good one and asserts that the good one completes.
- **Instrument:** one queue conformance suite, run against every queue in L8 and L9.
- **Enforcement:** E2 → E4.

### U31. Host re-entry is checked, never aliased; programmatic updates block their own notifications

- **Proven by:** L6-F21 ✅. On Windows, a programmatic `SetWindowTextW` re-enters the window procedure, aliases `&mut Runtime`, echoes `TextChanged`, and swallows the next keystroke. The same re-entry is reached through `TrackPopupMenu`, `WindowRegistry::sync` during `CreateWindowExW`, and `poison_runtime_and_quit`.
- **Reference:** GTK's queued work, `busy` flag, and `quietly` handler blocking; Android's `Bridge.muted`.
- **Why:** every native call that can pump messages is a re-entry point. A `&mut` taken twice is undefined behaviour, and an echoed change event is a feedback loop.
- **Bound:** every backend has one re-entry guard. A re-entrant call is queued, and a programmatic set never produces a user event.
- **Instrument:** the Task 3.2 re-entrancy fuzzer, run on each backend, plus a test that a programmatic set yields no event.
- **Enforcement:** E2 → E4.

### U32. Every process-wide cache keyed by data has an eviction bound

- **Proven by:** L6-F26 (Windows GDI brushes per colour), L6-F29 (fonts), L6-F32 (GTK CSS providers and D-Bus subscriptions), L9-F38 (prerendered shells per path), L3-RP1 (the inspectable-store registry), L7-F13 (the responsive-image store), L6-F37 ✅ (measure and theme caches keyed by `Debug` output), and L3-RP3 (the wire command-name table, which is capped but never frees a slot).
- **Why:** U8 asks for an owner. A cache keyed by colours, paths, or ids that the data chooses has no natural owner, so it needs a capacity.
- **Bound:** every `static` or thread-local map or set has a capacity or a documented key-space bound. The soak harness (Task 1.7) varies the key every frame and asserts a plateau.
- **Instrument:** a source test listing every process-wide collection, and the soak run.
- **Enforcement:** E0 → E4.

### U33. Deferred and offline writes carry an idempotency key, an owner, and an expiry

- **Proven by:** L7-F6 ✅ (the service worker replays `/_fn` calls twice, days later, under the next user's session; reproduced in Chromium), L8-F8 (the data layer's offline queue).
- **Why:** a write sent later must still mean what it meant when the user acted: once, by that principal, while it is still relevant.
- **Bound:** every queued write records a key the server deduplicates, the principal it was queued under (dropped if the session changed), and an expiry.
- **Instrument:** a test that loses a response after the server applied the call and asserts one effect; a test that signs in as another user before replay and asserts nothing is sent.
- **Enforcement:** E2 → E4.

### U24 extension: id registries record their owner

L9-F1, L9-F27, L9-F29, and L10-F7 share one shape: a registry keyed by an id (a Live session, a sync document, an operation, an order) that any caller holding the id can read or act on. Every such registry stores the principal that created each entry and checks it on every access. An id is never a capability unless it is unguessable *and* the design documents that it is one.

## Cross-cutting indicators found in iteration 3 (U34–U35)

### U34. Every bounded slot pool reclaims idle holders

- **Proven by:**
  - L9-F43 ✅: idle keep-alive connections hold the server's `max_connections` slots forever, so 1 024 quiet sockets lock every other client out. The cap itself was the fix for L9-F8.
  - L9-F44: an MQTT client with keep-alive 0 holds a broker slot for 24 hours.
  - L9-F45: the inspector's pre-token deadline is per line, so 16 trickling clients hold every slot.
  - L10-F13: the `dev --remote` agent serves one connection at a time with no timeout.
- **Why:** a cap without reclaim turns a resource limit into the denial of service it was meant to stop. The attacker no longer needs a flood, only patience.
- **Bound:** every pool that caps connections, sessions, or subscriptions pairs the cap with a total handshake deadline and an idle timeout. No holder keeps a slot longer than the larger of the two without sending traffic the protocol counts as activity.
- **Instrument:** one slot-pool test per listener. Fill the pool with holders that complete one request and go quiet, then assert that a new client is served within the idle timeout.
- **Enforcement:** E2 → E4.

### U35. Credentials are bound to their origin and their transport

- **Proven by:**
  - L8-F12 ✅: `BearerAuth` attaches the access token to any absolute or `http:` URL a request names.
  - L9-F15 ✅: the WNS access token is sent to a client-supplied channel URI.
  - L10-F13: `dev --remote` sends its token and every rebuilt executable in cleartext.
  - L10-F12: `update keygen` writes the signing key world-readable.
- **Why:** a token is a capability for one service. Sending it anywhere else, or over a channel anyone can read, hands that capability to whoever runs the other end.
- **Bound:** every credential records the origin (scheme, host, port) it belongs to. It is attached only to requests for that origin over TLS, or over loopback for development, and it is stored readable only by its owner.
- **Instrument:** a test per credential-bearing client that sends a request to another origin, and to an `http:` URL, and asserts the credential is absent; a file-mode test for every secret the CLI writes.
- **Enforcement:** E2 → E4.

## Cross-cutting indicators found in iteration 4 (U36–U37)

### U36. Every oracle has a positive witness

- **Proven by:**
  - L4-F17 ✅: the shared suite `scrolling_renders_nothing` passes on a backend whose scroll is a no-op.
  - L4-F18: the tmux "a resize is followed" check greps for text already on screen.
  - L6-F47 ✅: the Android device suite passes when a run stalls or is cut off, because the verdict counts failures and never requires the completion record.
  - L6-F46: `TuiHarness::settle` gives up silently after 100 rounds.
- **Why:** a check that only asserts the absence of a failure is also satisfied by an action that never happened. Such a gate stays green while the thing it guards is broken.
- **Bound:** every check of the form "X did not happen after A" also asserts that A took effect, and every run-level verdict requires the run's completion record and its exit status.
- **Instrument:** a mutation test that replaces each driver action (scroll, resize, click, key) with a no-op and asserts at least one suite fails, and a truncated-log test per CI verdict script.
- **Enforcement:** E2 → E4.

### U37. Every check also covers the indirect form

- **Proven by:**
  - L1-F21 ✅: the build-time token type check accepts any token whose value is another token, unchecked for existence, kind, or cycles.
  - L2-F29 ✅: Fluent variable references are checked, term references are not, and terms do not fall back across locales.
  - L1-F22 ✅: the token importer's alias map is overwritten on a name collision.
- **Why:** a validator that inspects only the direct form is bypassed by one level of indirection, which is the normal way design systems and translations are written.
- **Bound:** every validator resolves references (tokens, terms, aliases) to their final value before checking, and rejects dangling references and cycles.
- **Instrument:** for each validator, a test that the same error is raised when the value arrives through a reference.
- **Enforcement:** E3 → E4.

## Cross-cutting indicator found in iteration 5 (U38)

### U38. What is planned, shown, or declared is computed by the same function that acts

- **Proven by:**
  - L3-RP7 ✅: layout shares space using preferred sizes that ignore `Fixed`, then places with `Fixed`, so "fixed header plus fill body" overflows on every backend.
  - L3-RP8 ✅: a menu shows the focused declarer's command state but invokes the first declarer.
  - L7-F19 ✅: a streamed shell takes island indices from the first render and the island specs from the last.
  - L10-F20 ✅: a terminal package declares dependencies and a launch line from the command's idea of the app kind, not from the binary.
  - L6-F61 ✅: the terminal paints with one visibility rule and hit-tests with another.
- **Why:** when the preview and the action are computed separately, they drift apart, and the user acts on what the preview promised.
- **Bound:** every value computed for planning, display, or declaration comes from the function the action uses, or a test asserts the two agree on generated inputs.
- **Instrument:** paired property tests (sharing pass vs placement, `command_state` vs invocation, shell indices vs specs, declared dependencies vs `DT_NEEDED`, painted node vs hit-tested node).
- **Enforcement:** E3 → E4.

## Universal bounds summary (for quick reference)

| Indicator | Bound | Target grade/rung |
|---|---|---|
| U1 feature/target matrix | 0 failures, powerset depth 2 | M4/E4 |
| U2 lint exceptions | 0 bare `allow`; `expect` count non-increasing | M4/E4 |
| U3 panics | declared only, ratcheted; 0 unguarded FFI callbacks | M5/E4–E5 |
| U4 dropped errors | `let _ =` on `Result` → 0 unexplained | M4/E4 |
| U5 unsafe | forbid where unused; Miri/ASan/TSan clean; AppVerifier clean | M5/E4 |
| U6 test depth | fuzz per parser; ≥1 024 proptest cases; mutants ≥70 % (≥90 % critical); changed-line coverage ≥80 % | M5/E4 |
| U7 flakiness | 20/20 repeats; quarantine with expiry | M4/E4 |
| U8 growth | every external-input collection bounded or drained; soak slope ≤1 %/h | M4/E4 |
| U9 concurrency | `!Send` compile-fail tests; lock ranks; loom; TSan | M5/E5 |
| U10 performance | every budget key measured on its CI OS; instruction-count core gates | M4/E4 |
| U11 compatibility | public-api snapshot + semver-checks; non_exhaustive/sealed audit; format fixtures | M4/E4–E5 |
| U12/U18 claims | every named test exists; capability parity; missing_docs | M4/E4 |
| U13 architecture | crate graph + core purity + module layering + ownership modules | M4/E4 |
| U14 supply chain | SHA-pinned actions, pinned toolchain, SBOM, provenance, reproducible | M4/E4 |
| U15 diagnosability | no `String` errors in public API | M4/E4–E5 |
| U16 trust boundaries | size + depth + time limits, fuzzed | M5/E4 |
| U17 teardown | a test per exit path per backend | M4/E4 |
| U34 slot pools | every capped pool has a handshake deadline and an idle timeout; fill-and-wait test per listener | M4/E4 |
| U35 credentials | attached only to their own origin over TLS or loopback; stored owner-only | M4/E4 |
| U36 positive witness | every negative oracle also asserts its action took effect; verdicts require a completion record | M4/E4 |
| U37 indirect forms | validators resolve references first; dangling references and cycles refused | M4/E4 |
| U38 plan equals act | preview, display, and declaration share the acting function or a paired test | M4/E4 |

# Part C — The layers, bottom-up

The layer order follows the crate dependency graph (from each `Cargo.toml`), so every layer is built only on layers already made solid:

```
L11 Project, CI, release            (.github, budgets, deny.toml, tools/)
L10 Tooling                         rustnative (CLI: package, deploy, dev, lsp, emulate, bench)
L9  Server and distributed          rustnative-server, -server-macros, -sync, -durable, -observe
L8  Application layer               rustnative-components, -data, -i18n (runtime side)
L7  Web                             rustnative-web, -webgen, -web-macros
L6  Native backends                 rustnative-windows, -linux, -android, -tui
L5  Shared platform groups          rustnative-drawlist, -update, -host, -interop
L4  Reference and verification      rustnative-headless, -conformance, -web-testing
L3  Core runtime                    rustnative-core (ten sub-layers)
L2  Syntax and build                rustnative-markup, -macros, -build, -i18n (catalogue parser)
L1  Style model                     rustnative-style
L0  Value types                     rustnative-types (no_std)
```

Each section answers Q1–Q10. "Inherits U1–U18" is implied everywhere; only layer-specific content is written. Indicator IDs are `L<n>.<k>` and are referenced by the tasks in Part D.

---

## L0 — Value types (`rustnative-types`, 351 lines, no_std)

**Role:** geometry, colour, totally ordered scalars shared by every target including `thumbv7em-none-eabihf` (CI job `no-std`). Everything above compares, hashes, and serializes these values; an error here is an error everywhere.

**Q1 Indicators**
- *Basic:* builds `no_std` for `thumbv7em-none-eabihf` (CI ✔); 1 unit test today.
- *Standard:* `#![forbid(unsafe_code)]`; `missing_docs`; every type `Copy` where it is plain data.
- *Production:* **L0.1** every documented invariant holds for *all* inputs — `Scalar` is "a finite `f32` with total equality" (`src/scalar.rs:3`); `Eq`/`Ord`/`Hash` are mutually consistent (`a == b ⇒ hash(a) == hash(b)`, `cmp` total, antisymmetric, transitive). **L0.2** geometry arithmetic never panics and saturates where documented. **L0.3** colour conversions round-trip within a stated ε.
- *Edge:* **L0.4** `NaN`, `±∞`, `-0.0`, subnormals, `f32::MAX`, `i32::MIN` inputs to every constructor; **L0.5** serialized form stable (if `serde` is on) and identical across endianness/targets.

**Q2 Coverage:** aspects 1, 2, 3, 7, 9 apply; 5/6/10/12 not applicable (pure values); 8 applies only as "no allocation" — **add L0.6: no heap allocation in any function** (no_std without `alloc` proves it at build time — check whether `alloc` is used; if not, the `no-std` CI build without `alloc` is already the E5 proof).

**Q3 Why:** `Event` is `Eq` *because* `Scalar` makes `f32` lawful; if `Scalar::new(f32::INFINITY)` is accepted (the constructor maps only `NaN` and `0.0` — `src/scalar.rs:19-21`), the "finite" claim is false and any layout or animation code that trusts finiteness (division, interpolation) produces `inf`/`NaN` downstream. This is a **candidate defect** to confirm in Task 2.1. A `Hash`/`Eq` inconsistency would corrupt every `HashMap` keyed by geometry (the layout cache, the damage tracker in drawlist).

**Q4 Bounds:** L0.1: 0 counterexamples in 100 000 proptest cases per law, per type, per CI night; 1 024 per push. L0.3: |round-trip − original| ≤ 1/255 per sRGB channel (8-bit), ≤ 1e-4 in linear float. L0.6: build without `alloc` succeeds.

**Q5 Measurement:** proptest law suites (`tests/laws.rs`) with explicit `prop_oneof![Just(f32::NAN), Just(f32::INFINITY), Just(-0.0), any::<f32>()]` so special values are always sampled (proptest's `any::<f32>()` rarely hits them — the reason a plain random test is *not* reliable enough here). Kani (bounded model checker) can prove `Scalar::new` postconditions for **all** 2³² inputs — far stronger than sampling, cheap on a 20-line function. Grade M2 → **M5**.

**Q6 Enforcement:** E3 → **E5 where possible**: make `Scalar`'s field private (it is) and the only constructor total and documented — the type then *is* the proof; `#![forbid(unsafe_code)]`; Kani proof harnesses as a CI job (`cargo kani -p rustnative-types`), blocking.

**Q7 Better methods?** For a 351-line pure crate, exhaustive verification (Kani) beats sampling; for f32 functions with one argument, literally exhaustive testing (iterate all 2³² bit patterns, ~10 s in release) is even simpler and needs no extra tool — **chosen for `Scalar::new`**; Kani for the two-argument geometry ops.

**Q8 Implementation:** Task 2.1. Lands as tests first; if the ∞ defect is confirmed, the fix (map non-finite to a documented value, or clamp to `±f32::MAX`) is a separate commit with the exhaustive test as its proof.

**Q9 Limit:** with exhaustive/Kani proofs, this layer reaches the theoretical limit for its size. Residual: serialized-form stability across *future* Rust float formatting — pinned by fixture (U11c).

**Q10 Verdict:** enough after Task 2.1. Re-check triggers: any new type added; any `alloc` use; any new target in the `no-std` job.

---

## L1 — Style model (`rustnative-style`, 4 789 lines)

**Role:** the declaration vocabulary, utility classes, `app.css` parser (`src/sheet.rs:125`), value/colour/shadow parsers (`src/value.rs:292,382`, `src/color.rs:31`), design tokens, and **each backend's capability table** (`src/capability.rs`) that decides what fails the build.

**Q1 Indicators**
- *Basic:* 28 unit tests; style equivalence suite in conformance (`rustnative-conformance/tests/style_equivalence.rs`).
- *Standard:* typed errors (today `Result<_, String>` for `parse_color`, `parse_value`, `parse_shadow` — U15); `#![forbid(unsafe_code)]`.
- *Production:* **L1.1** *spelling equivalence*: typed = utility = declaration for **every** property in the vocabulary — and the suite must be generated from the vocabulary table so a new property cannot be added without its three-way case. **L1.2** *capability table truth*: for each backend, a property marked realizable is applied, and one marked unavailable fails the build for that declared target (Windows has the readback test `the_windows_capability_table_is_what_the_backend_applies`; Linux/Android/Web/TUI need theirs). **L1.3** *parser totality*: any byte string → `Ok` or an error with a span; never panic, never hang. **L1.4** colour math within ε (OKLCH ↔ sRGB has one round-trip test, `src/color.rs:349`). **L1.5** *diagnostic quality*: every error names line, column, the offending token, and (for unknown classes) the nearest valid one.
- *Edge:* **L1.6** pathological sheets: 10⁵ rules, 10⁴-deep selector nesting (if nesting exists), 1 MiB values, unterminated strings/comments at EOF, BOM, CRLF, non-UTF-8 (rejected cleanly at the `&str` boundary — callers reading files must use `from_utf8` with an error, check `rustnative-build/src/styles.rs`). **L1.7** token cycles in design tokens (`a → b → a`) are detected, not recursed.

**Q2 Coverage:** add **L1.8** *determinism of output order*: the resolved sheet and generated CSS (Web) are byte-identical across runs (no `HashMap` iteration order leaking into output) — matters for reproducible builds (U14) and for golden tests. Add **L1.9** *performance*: parsing the largest sheet in the repo + a synthetic 10 000-rule sheet within a budget key.

**Q3 Why:** this is the layer that turns a typo in `app.css` into either a build error or a silent wrong pixel on one backend. The review found utility-class target checks keyed on the *compiling OS* (finding 8, since fixed by "style targets") — exactly an L1.2 failure. HashMap-order nondeterminism would make every Web golden flaky.

**Q4 Bounds:** L1.1: 100 % of vocabulary entries have a three-way case (count = vocabulary length, asserted). L1.2: 100 % of `(backend, property)` cells have a readback or compile-fail test. L1.3: 0 crashes after 1 CPU-hour/week of fuzzing each of `sheet::parse`, `parse_value`, `parse_color`, `parse_shadow`. L1.4: ε as L0.3. L1.8: two runs → identical bytes. L1.9: ≤ 50 ms for 10 000 rules (instruction-count gate).

**Q5 Measurement:** generated equivalence tests (a `build.rs`-free approach: a test iterating `vocabulary::ALL` and asserting each spelling resolves equal — no codegen needed); per-backend capability readback tests in each backend crate; cargo-fuzz targets; a determinism test running the generator twice in one process with different `HashMap` seeds (`RandomState` is per-instance, so two instances suffice). Grade M3 → **M5**.

**Q6 Enforcement:** **E4** (fuzz smoke blocking, equivalence count assertion); **E5** for L1.8 by using `BTreeMap`/`IndexMap`-free ordered `Vec` in output paths — a `clippy::disallowed_types` for `std::collections::HashMap` *in output-producing modules* only (`sheet.rs`, `web.rs`, `token_table.rs`).

**Q7 Better?** Differential testing against a real CSS parser (e.g. `cssparser`/`lightningcss` as a dev-dep) for the declaration subset gives an oracle for free: any input both accept must parse to the same values. **Adopted for the Web-facing declaration subset** (dev-dep only, not shipped).

**Q8 Implementation:** Task 2.2 (fuzz + differential), Task 2.4 (typed errors), Task 6.2 (capability readback per backend, in wave 6).

**Q9 Limit:** a formal grammar (the declaration vocabulary as a PEG with generated parser) would remove hand-parser bugs entirely; not worth replacing a working parser — differential + fuzz reach practical equivalence.

**Q10 Verdict:** re-check when M67 (extended style range, guards, structural variants) lands — it multiplies the capability table and the equivalence suite.

---

## L2 — Syntax and build (`rustnative-markup` 2 623, `rustnative-macros` 360, `rustnative-build` 2 025, `rustnative-i18n` 801)

**Role:** one grammar for `rsx!` and `.rsx` files (`markup/src/parse.rs`, `lower.rs`, `rsx_file.rs`, `rsx_scan.rs`, `format.rs`), the proc-macro carrier, build-script codegen (icons `icon.rs`, manifests `manifest.rs`, Windows resources `rc.rs`, Android `android.rs`, styles, messages), and Fluent-subset catalogues with CLDR plural rules (`i18n/src/lib.rs:340`, `plural.rs`).

**Q1 Indicators**
- *Basic:* syntax equivalence suite (`conformance/tests/syntax_equivalence.rs`), compile-fail suite (`conformance/tests/style_ui_windows`, trybuild), source-map round-trip test (`rsx_file.rs:630`).
- *Standard:* proc macros emit `compile_error!` with spans, never panic (a panicking proc macro gives the user "proc macro panicked" with no location).
- *Production:* **L2.1** *carrier equivalence*: builder = `rsx!` = `.rsx` for every node kind and modifier, generated from the node-kind table. **L2.2** *macro hygiene*: expansions use absolute paths (`::rustnative_core::...`) and compile in a crate that shadows `Node`, `Option`, `Vec`, `Result`, `std`. **L2.3** *formatter*: `format(format(x)) == format(x)` (idempotence) and `parse(format(x)) ≡ parse(x)` (meaning-preserving) — no test today (grep for idempotence in markup: none). **L2.4** *source maps*: every diagnostic in generated code points to the `.rsx` line/column. **L2.5** *build-script outputs are valid*: generated ICO/PNG decode; generated `.rc`/manifest XML parse; generated Android XML validates against `aapt2`. **L2.6** *i18n correctness*: plural category for every CLDR locale the framework ships matches CLDR's own test data for integers 0–200 and decimals.
- *Edge:* **L2.7** recursion: 10 000 nested elements in `.rsx` → a "nesting too deep" error, not a stack overflow (rustc's own proc-macro stack is 8 MiB by default on the main thread but proc macros may run on smaller threads); **L2.8** Unicode identifiers, RTL text in attributes, `{}` in strings, raw strings, CRLF, BOM; **L2.9** build scripts are deterministic and do not touch the network or write outside `OUT_DIR` (cargo's contract).

**Q2 Coverage:** add **L2.10** *incremental correctness*: changing a `.rsx` file triggers rebuild (`cargo:rerun-if-changed` emitted for every input file, including styles and catalogues); a missing one is a stale-build bug that only shows in the dev loop. Add **L2.11** *error recovery*: one typo yields one diagnostic, not a cascade (count diagnostics in a fixture with one error).

**Q3 Why:** this layer runs inside the user's compiler. A proc-macro panic or stack overflow is a broken build with no location; a hygiene bug is an error only in *some* user crates; a wrong plural category ships a grammatically wrong UI to an entire locale and no test of the app would notice.

**Q4 Bounds:** L2.1: 100 % of node kinds × carriers (asserted count). L2.2: hygiene fixture crate compiles. L2.3: 0 failures in 10 000 proptest-generated markup trees + all repo `.rsx` files. L2.6: 100 % agreement with CLDR `plurals.xml` samples for shipped locales. L2.7: depth limit 256, error at 257, tested. L2.9: two clean builds → identical `OUT_DIR` contents. L2.10: for each input kind, a test that touches it and asserts `cargo build` re-runs the script (via `cargo build -v` output). L2.11: exactly 1 diagnostic per single-error fixture.

**Q5 Measurement:** generated equivalence cases; a `tests/hygiene/` fixture crate in conformance; proptest generator for markup ASTs (reuse `arbitrary_tree()` from `core/tests/property_tests.rs` as the `Node` side, print as markup, parse back); cargo-fuzz over `parse_markup` (via `syn::parse_str` → `TokenStream`), `rsx_scan`, `i18n::parse`; CLDR data as a fixture file (`tests/fixtures/cldr-plurals.xml`, versioned); `aapt2 link` in the Android CI job for L2.5. Grade M3 → **M5**.

**Q6 Enforcement:** E3 → **E4**; **E5** for L2.2 by a lint-like test that greps expansion goldens for unqualified paths.

**Q7 Better?** For proc macros, `trybuild` (already used) + expansion goldens (`macrotest`/`cargo expand` snapshots) are the standard. For plurals, generating the rules *from* CLDR data at build time removes hand-transcription errors entirely — **recommended** if `plural.rs` is hand-written (check in Task 2.3); otherwise test against the data.

**Q8 Implementation:** Task 2.3 (fuzz, depth limit, formatter properties, hygiene, CLDR), Task 2.6 (build-script determinism and rerun-if-changed).

**Q9 Limit:** a grammar spec with a reference parser generated from it and differential-tested against the hand parser would be the limit; not needed if fuzz + equivalence hold.

**Q10 Verdict:** re-check when M53's grammar grows or a new carrier (LSP edits — `rustnative/src/markup_edit.rs`) changes what is parsed.

---

## L3 — Core runtime (`rustnative-core`, 34 753 lines, 342 tests, 1 `unsafe`)

The core is the layer everything stands on; it is split into ten sub-layers matching its module map (`src/lib.rs:98-129`). Shared core indicators first, then each sub-layer.

### L3.0 Core-wide indicators

- **L3.0.1 Platform purity (2.4):** 0 `cfg(target_os|windows|unix)`; dependency tree free of OS bindings (U13b). *Why:* the core is checked for iOS (`apple` CI job) and wasm32 (server's `wasm32-wasip1` path); one OS import breaks every non-host target. *Measure:* grep + `cargo tree -p rustnative-core -e normal` gate. *Enforce:* E4.
- **L3.0.2 Module layering:** the module-map order is a dependency order (U13c). *Enforce:* E4 test.
- **L3.0.3 Determinism:** given a `ManualClock`, a test executor, and the same event sequence, the tree, the render log (`ComponentTree::last_render_log`), and the emitted diffs are identical across runs and across `--test-threads` settings. *Why:* every backend's correctness test assumes it. *Measure:* a "record/replay" property test: random event sequences applied twice to fresh applications, compare snapshots. *Enforce:* E4 (proptest).
- **L3.0.4 Bounded work per event:** one dispatched event renders at most the components it concerns (the invalidation contract — `docs/invalidation.md`, `tests/invalidation.rs`), and its cost is O(changed subtree), not O(tree). *Measure:* instruction-count benchmark at 1 000 vs 10 000 nodes with one changed leaf: ratio ≤ 1.5. *Enforce:* E4 budget key.
- **L3.0.5 The single `unsafe`:** documented, Miri-tested (U5b).

### L3.1 Identity, nodes, reconciliation (`identity.rs` 364, `node.rs` 2 007, `reconcile/` ~830)

- **Q1/Q3:** **L3.1.1** distinct keys never collide (property test exists: `distinct_keys_never_collide`); **L3.1.2** diff applied to the old set reconstructs the new set exactly (exists); **L3.1.3** removals deepest-first, before insertions (exists); **L3.1.4** identity survives re-render with an empty diff (exists); **L3.1.5** *new:* **moves** — reordering keyed children produces moves, not remove+insert (native objects keep focus, scroll, IME composition across a reorder; losing them is a user-visible bug); **L3.1.6** *new:* duplicate sibling keys are detected with a diagnostic naming both, in debug, and handled deterministically in release; **L3.1.7** *new:* diff complexity O(n) for keyed lists, measured at 10⁴ and 10⁵ children (a quadratic diff is invisible at test sizes); **L3.1.8** *new:* the interner (`NodeId::interned_keys()`, per-thread, never shrinks — documented in fix-all) has a bound or a test that a server render loop does not grow it per request.
- **Q4:** proptest cases ≥ 1 024 per push (today: proptest default 256), 100 000 nightly; L3.1.7: time(10⁵)/time(10⁴) ≤ 12 (allows n log n); L3.1.8: interner size after 10 000 identical server renders == after 1.
- **Q5/Q6:** the existing generator `arbitrary_tree()` limits depth/width; extend with keyed-reorder and duplicate-key strategies; set `PROPTEST_CASES` in CI env. E3 → **E4**.
- **Q7:** proptest with shrinking is the right tool; add a **model-based** test (a reference naive diff — remove all, insert all — whose *resulting tree* must equal the optimized diff's): differential against an obviously-correct oracle beats hand-written expectations.
- **Q9:** the limit is a verified diff (proof in a prover) — not justified; model-based testing reaches practical confidence.

### L3.2 Component tree and invalidation (`component/` incl. `tree.rs` 1 798, `context.rs` 754, `boundary.rs`, `effects.rs`, `invalidation.rs`)

- **L3.2.1** invalidation contract: exactly the concerned components render (tests exist, `tests/invalidation.rs`). **L3.2.2** batching: one message → one render; no render sees partial state (exists, `suites::one_message_is_one_render`). **L3.2.3** error boundaries: a panic in `view`/`update`/an effect of one subtree is contained by the nearest boundary, the rest of the window keeps working, and the panic policy (`panic.rs`) is applied — test each of view/update/effect/drop/props panics. **L3.2.4** re-entrancy: a component dispatching during its own `update`, an effect mutating state during render, a parent unmounting a child from the child's callback — each either queued deterministically or rejected with a diagnostic, **never** a `RefCell` `BorrowMutError` panic (core has 36 `RefCell<` in non-test code; each `borrow_mut` on a path reachable from user callbacks is a re-entrancy panic candidate). **L3.2.5** state flush on window close (fix-all; test exists?) and on every lifecycle exit. **L3.2.6** effect cleanup runs exactly once per effect, on unmount and on dependency change, including when the component panicked.
- **Q4:** L3.2.4: 0 panics across a property test that generates random re-entrant callback programs (dispatch, mount, unmount, set_props, spawn) over 10 000 cases.
- **Q5:** a **re-entrancy fuzzer**: a test component whose `update` interprets a random program of tree operations; run under proptest. This is the measurement that actually finds `BorrowMutError`s; reading code does not scale to 36 `RefCell`s.
- **Q6:** E4. Where a re-entrant path is found, fix by queueing (E5-ish: the API cannot express the unsafe re-entry).
- **Q9:** further: `RefCell` → a state-machine phase type (`Rendering`/`Idle`) checked at compile time for the internal paths — consider after the fuzzer reports.

### L3.3 Layout (`layout/engine.rs` 1 434, `constraints.rs` 380, grid)

- **L3.3.1** non-negative geometry (property exists); **L3.3.2** declared constraints respected (exists); **L3.3.3** scroll ranges only where content overflows (exists); **L3.3.4** *new:* **monotonicity** — increasing available width never increases a wrapped text's height; **L3.3.5** *new:* **idempotence** — laying out twice with the same input gives identical frames; **L3.3.6** *new:* **saturation** — `i32::MAX`/`u32::MAX`/0 sizes, 10⁴-deep nesting (recursion depth → iterative or bounded), `NaN`-free with L0's Scalar; **L3.3.7** *new:* **incremental = full** — an incremental relayout after a change equals a from-scratch layout of the new tree (the classic incremental-layout bug); **L3.3.8** RTL mirroring: layout(rtl) == mirror(layout(ltr)) for mirrored-safe trees; **L3.3.9** cost: O(n) per pass at 10⁴ nodes.
- **Q4:** 0 counterexamples at 1 024/push, 100 000/night; depth limit documented (e.g. 1 024) and refusal tested; L3.3.9 ratio test as L3.1.7.
- **Q5/Q7:** properties with a **differential oracle** for L3.3.7 (full layout is the oracle for incremental). Grade M5.
- **Q9:** Taffy/Yoga differential for the flex subset is possible (dev-dep) — useful if the engine claims CSS-flex semantics; **check the claim in `docs/` first** and adopt only if it does.

### L3.4 Scheduler, executor, clock, affinity (`scheduler/` ~2 240, `clock.rs`, `affinity.rs`)

- **L3.4.1** scope-bound cancellation: no task delivers after its owner unmounts (property test exists over mount/unmount/time sequences). **L3.4.2** *new:* **affinity proof unforgeable per application** — Review Focus 5: `UiThread::claim()` is public; either make it `pub(crate)`/backend-only via a sealed constructor, or bind it to an `Application` (claim returns `Err` if the application already has a UI thread). **L3.4.3** *new:* no task, waker, or timer leak: after all scopes close, the executor's live-task count is 0 (expose a debug counter). **L3.4.4** *new:* timers with `ManualClock` fire in deadline order, exactly once, including equal deadlines (FIFO) and deadlines in the past. **L3.4.5** supervision (`supervise.rs`): restart policies obey max-restarts/window; a crashing supervised task cannot starve others. **L3.4.6** the executor is fair under a spinning task (cooperative budget) — the UI thread must not freeze because one task never yields. **L3.4.7** the thread pool (multi-thread runtime feature) has a bounded thread count and named threads (`rustnative-*`, fix-all).
- **Q5:** loom for any custom waker/atomic code in `scheduler/executor.rs`/`host.rs` (U9c) — **first audit for atomics with non-SeqCst orderings**; TSan run of `tests/local_tasks.rs`; proptest for timer ordering. Grade M3 → M5.
- **Q6:** L3.4.2 to **E5** (type-level). Others E4.

### L3.5 Environment, theme, style resolution (`environment.rs` 529, `style/theme.rs` 648, `style/resolve.rs` 318)

- **L3.5.1** an environment change renders only its readers (exists). **L3.5.2** *new:* **one channel per host trait** — the review's finding 6 (reduced motion on two channels) generalizes: every `keys::*` entry has exactly one writer path from backends, and a test per backend asserts it is fed (checklist row). **L3.5.3** resolution is a pure function of (node style, theme, environment, state flags) — property test: same inputs, same `VisualStyle`; **L3.5.4** precedence order documented and tested (typed > utility? state > base? — whatever the doc says, a table-driven test per pair).
- **Q5/Q6:** a test enumerating `keys::ALL` against each backend's `host_traits` feed list (backends run it in their own suites). E4.

### L3.6 Input, gestures, animation (`input/` ~1 700, `animation/` ~2 400)

- **L3.6.1** arbitrary pointer streams never panic and always settle (property exists). **L3.6.2** easing curves hit both endpoints exactly (exists); **L3.6.3** a timed animation finishes within its duration (exists). **L3.6.4** *new:* gesture arbitration is deterministic for simultaneous recognizers (pan vs pinch vs long-press) — table-driven + property. **L3.6.5** *new:* reduced motion collapses every animation to its end state in ≤ 1 frame. **L3.6.6** *new:* animation frames do not re-render (exists on Windows only; make it a conformance suite case using the headless frame clock — today "— (no frame clock on the model)" in `docs/guarantees.md`). **L3.6.7** IME composition sequences (start/update/commit/cancel interleaved with focus loss) leave text consistent.
- **Q5:** proptest over event streams; a headless frame clock (M45 deterministic clock). E4.

### L3.7 Accessibility model (`accessibility/` ~1 100)

- **L3.7.1** every interactive node has a role and an accessible name (headless query test exists in `portable_surface.rs`). **L3.7.2** *new:* the projection is total — every `Node` kind maps to a role, asserted by iterating the node-kind table. **L3.7.3** *new:* virtual elements (charts) have stable ids across re-render and parent/child consistency. **L3.7.4** *new:* live-region announcements are rate-limited/coalesced (a counter updating at 60 Hz must not flood screen readers).
- **Q5:** an **a11y lint pass** over every example app's tree in headless (all examples, every screen reachable by the test) asserting L3.7.1 — turns every example into an accessibility test (M45 C60 intent). E4.

### L3.8 Navigation, persistence, state (`navigation/` ~840, `persistence/`, `state.rs`)

- **L3.8.1** kill-and-restore per destination (M45 C14-1; lifecycle collision suite exists in headless). **L3.8.2** *new:* **format versioning** — persisted state carries a version; fixtures from each released version load (U11c); a field added to a persisted struct does not drop previous state. **L3.8.3** *new:* **crash-consistency** — a write interrupted at any byte (simulate by truncating the file at every offset in a test) leaves either the old or the new state readable, never a corrupt store (`FileStateStore` engine moved to core in fix-all: check it writes temp + rename + fsync). **L3.8.4** deep link arriving during restoration (collision suite) — exists in headless; extend to every backend.
- **Q4:** L3.8.3: all truncation offsets of a 4 KiB record → old-or-new. **Q5:** a truncation test harness; Windows `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)` vs POSIX `rename` + directory fsync semantics differ — test per OS. E4.

### L3.9 Services, capabilities, platform seam, application (`services/mod.rs` 800, `capability.rs`, `platform.rs`, `application.rs` 1 036)

- **L3.9.1** *the seam is the only way in*: every backend capability reaches components through `ComponentContext`/environment (fix-all added capabilities to the environment); no example names a backend type (grep gate over `examples/` for `rustnative_windows::|rustnative_linux::|rustnative_android::` outside `rustnative-host`-free adoption examples, which are allow-listed). **L3.9.2** every service contract has a **conformance suite** run against each backend's implementation *and* a fake (HTTP, storage, clipboard, secure storage, notifications, share, work, documents): same inputs → same observable outcome or a documented `Unsupported`. **L3.9.3** `Application` misuse is a typed error, not a panic: after `take()` (P3 in the review: accessors panic on the empty husk) — make the husk state unrepresentable (consume `self` / typestate per `docs/conformance/typestate.md`). **L3.9.4** lifecycle reaches every window (fix-all) — test per backend.
- **Q6:** L3.9.3 → **E5** (typestate). L3.9.2 → E4 via a `services_conformance` suite in `rustnative-conformance` parametrized by a `ServiceHost` trait, like `ConformanceHost`.
- **Q7:** contract tests (one suite, many implementations) are strictly better than per-backend hand tests: a new backend gets the whole suite for free.

### L3.10 Inspection, wire, crash, panic, teardown (`inspect/` ~2 300, `wire.rs` 320, `crash.rs`, `panic.rs`, `teardown.rs`)

- **L3.10.1** the inspection protocol is versioned and every message round-trips (encode → decode = identity) — property test over generated messages; **L3.10.2** the compact decoder (`inspect/compact.rs:141`) and wire decoder are total on arbitrary bytes (fuzz); **L3.10.3** inspection is off in release unless enabled, and when on over a remote transport it requires a token (security: an inspector can read every string in the UI); **L3.10.4** crash capture: a panic on any thread produces exactly one report, chained with existing hooks (the TUI guard chains — `rustnative-tui/src/guard.rs:31-37`; verify core's capture chains too), and report writing never panics or allocates unboundedly; **L3.10.5** teardown policy table per backend (U17).
- **Q5:** proptest round-trip; fuzz; a test spawning a child that panics on a worker thread and asserting one report file. E4.

**L3 Q2 coverage additions (whole core):** aspect 9 — the core's public API is by far the largest; U11's snapshot matters most here (expect ~1 500 public items; every one becomes visible in review). Aspect 11 — render-cause tracing exists (`RenderCause`); add *task* tracing (which scope spawned a task that never completed) for leak diagnosis.

**L3 Q9 limit:** the core could go further with (a) Kani proofs of the saturating layout arithmetic helpers, (b) a TLA+/P model of the scheduler's cancellation protocol. (a) is cheap and recommended for `layout/constraints.rs` helpers; (b) only if L3.4's loom/proptest find a bug class that sampling keeps missing.

**L3 Q10:** the core is re-checked **first** in the second pass (Part E) because every finding in L4–L11 tends to trace back to a missing core contract (as findings 3, 6, 9 did in the review). Re-check triggers: any new `Event` variant, any new `keys::*`, any new service contract, any `RefCell` added on a callback path.

---

## L4 — Reference and verification (`rustnative-headless` 2 586, `rustnative-conformance` 1 161, `rustnative-web-testing` 1 020)

**Role:** the instruments every other layer is measured with. **A solid measuring instrument is a precondition of every other indicator** — a headless backend that diverges from real hosts makes every green headless test a false statement.

**Q1 Indicators**
- **L4.1** *headless fidelity*: for each conformance suite, headless and every real backend give the same verdict; where hosts differ by design, the difference is listed (focus rules, hit-testing). Today headless cannot drive `MultilineText` and records no `SurfaceCommand`/`InputRequest` (`todo.md` "Remaining"). **L4.2** *synthetic input goes through the real input path* (M45) — asserted by a test that a handler-bypass (calling `dispatch` directly) and a synthetic click through hit-testing produce identical render logs for a hit-testable node. **L4.3** *query API failure messages* list what the tree contains (M45 C60) — tested. **L4.4** *exhaustive mode* fails a test on an unasserted task/effect/message — mutation-tested (inject an unasserted effect, test must fail). **L4.5** *web-testing robustness*: the DevTools driver times out, kills the browser, and reports when Edge/Chrome is missing; never hangs CI; browser version recorded in output. **L4.6** *conformance suites are themselves tested*: each suite fails against a deliberately broken `ConformanceHost` (a "mutant host" per suite) — otherwise a suite that asserts nothing passes forever.
- *Edge:* **L4.7** headless under single core and under 64 threads (CI does single core ✔); **L4.8** golden files: one per machine class where pixels are involved (fix-all moved goldens per machine class), text goldens byte-exact everywhere.

**Q2:** add **L4.9** *suite coverage map*: every checklist row maps to a suite fn, every suite fn to a row — the claims checker (U12) enforces both directions.

**Q3:** without L4.6, the conformance crate is the single largest silent-failure risk in the project: all backends "pass" a suite that checks nothing.

**Q4:** L4.1: every suite in `suites.rs` runs on headless + each real backend (matrix count asserted); L4.6: 100 % of suites killed by their mutant host; L4.5: browser test job wall time ≤ 15 min hard timeout, 0 hangs in 200 runs.

**Q5/Q6:** mutant hosts in `rustnative-conformance/tests/mutant_hosts.rs`; cargo-mutants over `rustnative-conformance/src/suites.rs` (a surviving mutant = an assertion missing). E3 → **E4**.

**Q7:** cargo-mutants over the suites is the generic tool; mutant hosts are the domain-specific one and catch "suite passes with a host that does nothing" which cargo-mutants (mutating the suite, not the host) does not.

**Q8:** Task 3.11 (headless gaps + mutant hosts), runs **before** wave 6 (backends), because wave 6 relies on these suites.

**Q9:** the limit is headless being generated from a formal host model; not needed.

**Q10:** re-check whenever a suite or a checklist row is added.

---

## L5 — Shared platform groups (`rustnative-drawlist` 1 591, `rustnative-update` 521, `rustnative-host` 483, `rustnative-interop` 1 724)

Four small crates with very different risk; each gets its own short answer set.

### L5a — Draw list rasterizer (`rustnative-drawlist`)

**Role:** rasterizes a `DrawList` onto a host grid (the TUI today; embedded displays later), with damage tracking (`damage.rs`) and focus/hit geometry.
- **Q1:** **L5a.1** raster is total: any draw list (negative rects, huge coordinates, empty paths, 10⁶ commands) renders without panic in bounded time; **L5a.2** *damage soundness*: repainting only the damaged region yields the same grid as a full repaint (the classic damage-tracking bug: stale pixels); **L5a.3** geometry logical px ↔ cells round-trips within one cell; **L5a.4** clipping never writes outside the grid.
- **Q3:** an unsound damage tracker leaves stale glyphs on screen that no snapshot test catches unless the test compares incremental with full.
- **Q4:** L5a.2: 0 differences over 10 000 random sequences of draw-list updates (full-repaint grid as oracle). L5a.1: fuzz 1 CPU-h/week, 0 crashes.
- **Q5/Q6:** proptest with the full repaint as differential oracle; fuzz target over a serialized draw-list format. E3 → **E4**. Allows: 13 `#[allow]`s in 1 591 lines (densest in the workspace) — each becomes an `#[expect(reason)]` and is reviewed (U2).
- **Q9/Q10:** at the limit once the oracle test exists. Re-check when the embedded backend (M37) starts consuming it.

### L5b — Update manifest and decision (`rustnative-update`)

**Role:** Ed25519-signed manifest verify-before-parse, app id and expiry checks, rollout bucket, pre-release ordering (`src/lib.rs:1-20`). Security-critical; **4 tests**.
- **Q1:** **L5b.1** signature checked over the exact published bytes before parsing (✔ by design); **L5b.2** refusal on wrong app / expired / bad signature / bad hash — one test each; **L5b.3** *new:* **anti-rollback** — a validly signed manifest for a version lower than the installed one is refused unless explicitly a rollback channel (grep `downgrade|rollback` in the crate: no hits; an attacker replaying an old signed vulnerable release is the textbook update attack); **L5b.4** *new:* **key rotation** — more than one trusted key, with a rotation procedure and a test that a manifest signed by a retired key is refused after its retirement time (grep `keyring|rotation`: no hits); **L5b.5** version order is a total order consistent with SemVer 2.0 §11 (pre-release precedence, build metadata ignored) — property test against the `semver` crate as a dev-dep oracle; **L5b.6** rollout bucketing is uniform (χ² over 10⁶ synthetic install ids) and stable per install id; **L5b.7** the verifier is total on arbitrary bytes (fuzz); **L5b.8** payload hash compared in constant time is *not* needed (public data) — documented so nobody adds a false fix; **L5b.9** time source injectable (expiry tests must not depend on the wall clock).
- **Q2:** this is a supply-chain trust boundary: compare against **TUF**'s threat list (arbitrary installation, rollback, indefinite freeze, endless data, extraneous dependencies, mix-and-match, key compromise). Today covered: arbitrary installation (signature + hash), freeze (expiry), wrong app. **Not covered: rollback, endless data (download size bound — check whether `url`'s payload size is bounded by a signed `size` field; the manifest struct shows no `size`), key compromise (rotation/threshold).** Add **L5b.10** signed payload `size` and a download that stops at that size.
- **Q3:** an update channel is remote code execution by design; every TUF gap is a way for whoever controls the CDN to run code on every installation.
- **Q4:** L5b.2/3/4/10: one passing refusal test each (binary). L5b.5: 0 disagreements with `semver` in 100 000 cases. L5b.6: χ² p > 0.01. L5b.7: 0 crashes, 1 CPU-h/week. Mutation score ≥ 90 % (critical set).
- **Q5/Q6:** tests, proptest-with-oracle, fuzz, cargo-mutants. E3 → **E4**; L5b.3/L5b.10 → **E5** by making `UpdateDecision` construction require the installed version and the size.
- **Q7:** adopting TUF wholesale (`tough` crate) is the maximal option — heavier than this crate, but it is what a production updater should converge on. **Recommendation:** close the three gaps natively now (small), record TUF adoption as the Q9 limit with a decision point at M50's fleet work.
- **Q10:** re-check on any manifest field change; any new host installer (`rustnative_windows::update`, `rustnative_android::update`, and future Linux/macOS).

### L5c — Host selection (`rustnative-host`)

**Role:** chooses the backend for the compiling target and builds default `Services` (fix-all; removes per-app `cfg(target_os)` ladders).
- **Q1:** **L5c.1** a target with no backend is a **compile error naming the target**, not a run-time `UnsupportedHost` (review finding 4's fallback-to-Windows bug) — compile-fail test per unsupported target (`cargo check --target aarch64-apple-darwin -p rustnative-host` must fail with the message, in the `apple` CI job); **L5c.2** `services()` returns the same set of service kinds on every backend (missing ones as explicit `Unsupported` implementations, not absent), asserted by a test listing kinds; **L5c.3** `wants_terminal()` decision table tested (TTY, env var, flags).
- **Q4/Q6:** binary tests, E4. Today the `apple` job *checks* `rustnative-host` for macOS and iOS (`ci.yml:313,317`) — so either it compiles (and L5c.1 needs a decision: headless fallback vs compile error) or the job would fail; **Task 4.3 verifies which and records the decision in the crate docs.**
- **Q10:** re-check when macOS/iOS/iPadOS backends land.

### L5d — Library-only mode and interop (`rustnative-interop`)

**Role:** `.ril` IDL → C, C#, Rust bindings, and the C-ABI runtime the generated code calls (`runtime.rs`, 18 `unsafe`, 3 `extern`, 2 `catch_unwind`).
- **Q1:** **L5d.1** IDL parser total (fuzz) with depth limit; **L5d.2** generated code compiles: C with `-Wall -Wextra -Werror -std=c11` (gcc and clang and MSVC), C# with `dotnet build -warnaserror`, Rust with `-D warnings` — for every IDL feature (a kitchen-sink `.ril` fixture); **L5d.3** every exported function is panic-safe (`catch_unwind` → error code) — 2 `catch_unwind` for 3 `extern`s: **verify the third** (U3b); **L5d.4** ownership across the ABI: every handle has a create/destroy pair, double-destroy and use-after-destroy are detected (generation-checked handles, like Windows `handle.rs`) and return an error, never UB; **L5d.5** strings: NUL-termination, UTF-8 validation on input, ownership of returned strings documented and a matching free function; **L5d.6** ABI stability: the generated C header is snapshotted and diffed (U11); **L5d.7** thread rules across the ABI documented and enforced (calls from the wrong thread return an error).
- **Q3:** this layer hands raw pointers to code the framework did not write; every rule not enforced at the boundary becomes UB inside Rust.
- **Q4:** L5d.2: kitchen-sink compiles on 3 C compilers + dotnet (binary). L5d.4: a C test program doing double-free/use-after-free gets error codes under ASan with 0 reports. L5d.1: fuzz 0 crashes.
- **Q5/Q6:** C test harness compiled in CI (`cc` crate in a test, or a CMake step) under ASan/UBSan; Miri for the Rust side of the runtime with a Rust-written caller. E2/E3 → **E4**.
- **Q7:** `cbindgen`/`csbindgen` are the ecosystem standards; the custom generator is justified only if the IDL carries semantics they cannot (component trees, events). Keep, but test the generated output with the same rigor as a published C library.
- **Q9:** the limit is a formally specified ABI with a conformance suite in each target language; reachable incrementally.
- **Q10:** re-check on any IDL grammar change and before advertising library-only mode as stable.

---

## L6 — Native backends (`rustnative-windows` 28 435 / 783 `unsafe`, `rustnative-linux` 17 085 / 86, `rustnative-android` 14 110 / 30, `rustnative-tui` 11 060 / 57)

All four realize the core's tree on a host. They share one indicator set (L6.0, the backend contract) — measured by the same conformance suites and the same checklist (`docs/conformance/new-backend-checklist.md`) — plus per-host indicators.

### L6.0 The backend contract (all four)

- **L6.0.1 Checklist completeness:** every row the host can answer has a named, passing test; unanswerable rows are marked n/a with a reason; **0 `—` cells** for a backend called complete (U12). *Why:* the review's finding 18 class. *Measure:* the claims checker. *Bound:* Windows/Linux/Android/TUI columns 100 % (rows 23–25 owed by M67 for all — recorded, not counted).
- **L6.0.2 Capability truth:** `realized()` == the set of capabilities with passing tests (parity both ways) — review finding 5 (Android under-advertising) generalized. *Measure:* a per-backend test comparing `realized()` with a capability → test map. E4.
- **L6.0.3 Native-object lifetime:** mount/unmount × 100 cycles returns every host resource to baseline: Windows GDI/USER objects (gate exists: `gdi_and_user_objects_return_to_baseline`) **and kernel handles** (`GetProcessHandleCount` — not covered today); Linux GObject instance counts (`G_DEBUG=gc-friendly` + `GOBJECT_DEBUG=instance-count` / `g_type_get_instance_count`) and fds; Android JNI global refs (`adb shell dumpsys meminfo` / `-Xcheck:jni` global ref table) and View counts; TUI fds and termios state. *Bound:* delta 0 after warm-up. E4.
- **L6.0.4 FFI panic boundary:** every host callback (window procedures, GTK signal closures, JNI natives, signal handlers) goes through one guarded entry (U3b). *Bound:* 0 unguarded. E4 → E5 (macro).
- **L6.0.5 Thread affinity at the host boundary:** host calls only from the host's UI thread — debug assertion in each ownership module (`ThreadAffinity`), plus a test calling from a worker thread that expects the assertion (debug) / error (release).
- **L6.0.6 Error classification parity:** every ignored host result goes through the backend's classified `best_effort` and is logged with a category (Windows model). *Bound:* `let _ =` on host results → 0 (U4).
- **L6.0.7 Teardown per exit path** (U17), modelled on `rustnative-tui/src/guard.rs`.
- **L6.0.8 Fidelity and accessibility in CI through the host's own AT API** — UIA (Windows ✔), AT-SPI (Linux: `gtk/atspi_reader.rs` ✔), Android `AccessibilityNodeInfo` (device tests ✔), TUI (screen-reader-friendly mode — check plan); and the **recorded human pass** with each host's screen reader (Narrator/NVDA, Orca, TalkBack) — **owed to a person** per backend, M1 grade, cannot be automated (Q9 residual).
- **L6.0.9 Modal-loop conformance:** scheduled work and animations continue inside the host's modal loops (Windows menu/size loops ✔; OLE drag loop "not verified"; GTK has nested main loops in dialogs; Android has none) — a test per loop that exists.
- **L6.0.10 Budgets measured on CI OS** (U10): Linux memory (`VmHWM` — `RESUME.md` item 1), Android startup/memory on the emulator, TUI on the synthetic terminal.
- **L6.0.11 Host-version matrix:** each backend states its minimum host (Windows 10 1809? GTK 4.x? API level? terminal capabilities) and CI covers min and latest where runners allow (Linux distros matrix ✔: ubuntu + fedora + arch in the gate; Android API 35 only → add minSdk image; Windows only `windows-latest` → Windows 10 is the reference machine, record as owed for Server 2019/2022 runners).
- **L6.0.12 Lifecycle collision suite** (M45) runs on the real host, not just headless: process death during restoration, deep link during restoration, low-memory trim during a modal.

### L6a — Windows specifics

- **L6a.1** *Win32 handle ownership:* every `HWND`/`HFONT`/`HBRUSH`/`HICON`/COM pointer is owned by an RAII type whose `Drop` releases it; raw handle values (`isize` in `handle.rs`) never outlive their generation (generation check ✔). *Measure:* Application Verifier `Handles` + `Heaps` + `Locks` layers over the integration tests (U5d); GDI/USER/kernel-handle baselines (L6.0.3).
- **L6a.2** *DPI:* layout scales by per-monitor DPI and follows `WM_DPICHANGED` (M69 first half, fix-all). Bound: the reference screen fits at 100/125/150/175/200/250 % and on a mixed-DPI move (test with `SetThreadDpiAwarenessContext` + `AdjustWindowRectExForDpi` simulation, since CI runners have one monitor at 96 DPI — multi-monitor is **owed to the reference machine**, M2).
- **L6a.3** *message-loop re-entrancy:* window procedures re-entered by `SendMessage` during a dispatch (e.g. `SetWindowPos` inside `update` triggering `WM_SIZE`) never hit a `RefCell` double borrow. *Measure:* a re-entrancy stress test sending nested messages from inside handlers, under a debug build. This is the Windows instance of L3.2.4.
- **L6a.4** *COM apartment rules:* STA for UIA/OLE/dialogs (`framework-dialog-sta` thread renamed `rustnative-*`), every `CoInitializeEx` paired with `CoUninitialize` on the same thread; UIA providers never call back into the tree off the UI thread.
- **L6a.5** *Visual goldens per machine class* (fix-all) with a tolerance metric documented (per-pixel ΔE and % of pixels) — the previous 6.49 % failure showed the metric must distinguish "different machine" from "different rendering".
- **Q7 note:** Windows has the most `unsafe` (783). The strongest affordable push is (1) Application Verifier in CI, (2) migrating raw `windows-sys` calls behind safe wrappers in the ownership modules so the `unsafe` count *shrinks* (ratchet), (3) evaluating the `windows` crate's safe COM wrappers for UIA/D2D where they replace hand-written vtables.

### L6b — Linux specifics

- **L6b.1** GTK main-thread rule: every GTK call from the main context (`glib::MainContext::default().is_owner()` debug assertion in the ownership module).
- **L6b.2** X11 and Wayland both tested (Xvfb in CI ✔; Wayland via `weston --backend=headless` in CI — add), and the portal paths (`desktop/portal.rs`) tested against the fake bus (`desktop/fake_bus.rs` ✔) **and** a real `xdg-desktop-portal` in one CI job.
- **L6b.3** ASan/TSan over the GTK integration tests (U5c) with GLib suppressions (GLib leaks by design at exit — use `G_SLICE=always-malloc G_DEBUG=gc-friendly` and a suppression file checked in).
- **L6b.4** distro matrix ✔ (ubuntu, fedora, arch in the gate; ubuntu + fedora in CI) — bound: every distro in `toolchain/linux_distro.rs` that packaging targets is in CI.
- **L6b.5** secure storage (`services/secure_storage.rs`) behaves when no keyring daemon exists (headless servers): a clear `Unavailable`, never a hang on D-Bus activation (timeout bound: ≤ 5 s).

### L6c — Android specifics

- **L6c.1** *JNI discipline:* every native entry (20 in `jni_host/natives.rs`) is guarded (L6.0.4); local references are bounded per call (`with_local_frame` is used in one module only — 3 occurrences in `jni_host/mod.rs`; loops creating local refs without a frame overflow the local reference table); pending Java exceptions are checked after every call that can throw. *Measure:* run the device suite with **CheckJNI** on (`adb shell setprop debug.checkjni 1` or `android:debuggable` with `-Xcheck:jni`), which aborts on every JNI rule violation — the single most reliable JNI instrument there is (M5-like). *Bound:* 0 CheckJNI aborts.
- **L6c.2** *lifecycle:* configuration change, process death, `onTrimMemory` levels, multi-window/activity — the collision suite on the emulator (`tools/android-lifecycle-test.sh` ✔) — bound: every collision in the headless suite also runs on the emulator.
- **L6c.3** *capability truth* (review finding 5 — fix status: verify `capabilities.rs` now advertises what it realizes).
- **L6c.4** *device matrix:* API 35 emulator in CI; add minSdk emulator; the Redmi Note 14 physical run is **owed** (phone not attached) — M1/M2 until done.
- **L6c.5** *hwasan* build of the device tests (Q9 residual for U5) — optional.

### L6d — Terminal specifics

- **L6d.1** *restoration on every exit path* ✔ (guard table) — keep as the model; bound: each row has a test (pty, tmux, ssh jobs ✔).
- **L6d.2** *input decoder totality:* `decode.rs` `feed` on arbitrary byte streams, split at every boundary (a sequence split across two reads must decode identically to one read) — **fuzz + split-invariance property**; bound: 0 crashes, 0 split-dependence.
- **L6d.3** *width tables:* generated from Unicode 17 data (`width/generate.rs`, `rustnative-tools/unicode-17`) — test that the generated table equals a regeneration (staleness check) and agrees with a reference implementation (`unicode-width` crate as dev-dep oracle) except for documented deliberate differences (emoji presentation, East Asian ambiguous).
- **L6d.4** *emulator fidelity:* the synthetic terminal (`emulator.rs`) agrees with a real terminal for every sequence the encoder emits — differential test against `vte`'s parser (dev-dep) or tmux capture (tmux job ✔).
- **L6d.5** *output bounded under slow terminals:* a stalled pty (reader not reading) does not grow an unbounded output buffer; frames are dropped/coalesced. Bound: output buffer ≤ 2 frames.

**L6 Q2 additions:** aspect 10 (security) — native backends read clipboard, drag data, deep links, and IPC (single-instance pipes/D-Bus): each is a trust boundary (U16): deep-link/URL parsing fuzzed; single-instance channel authenticates the peer (same user SID / same UID) — **check `windows/native/single_instance.rs` and `linux/desktop/single_instance.rs`** for peer checks.

**L6 Q9 limit:** what cannot be automated: real screen-reader passes, multi-monitor DPI, physical devices, real GPU frame pacing. These stay **owed and recorded** (2.13); everything else reaches E4.

**L6 Q10:** re-check per backend after each wave-4 task and whenever a checklist row is added; the macOS/iOS/iPadOS backends inherit L6.0 verbatim.

---

## L7 — Web (`rustnative-web` 9 184 + `rn.js` 139 907 bytes, `rustnative-webgen` 6 733, `rustnative-web-macros` 22)

**Role:** semantic HTML/CSS realization, the JavaScript runtime `src/runtime/rn.js` (≈ 140 KB, hand-written) and service worker `sw.js`, client logic compiled from a Rust subset to JS (`webgen`), WebAssembly subtrees (`wasm.rs`, 8 `extern` fns).

**Q1 Indicators**
- **L7.1** *output safety (XSS):* every string reaching HTML goes through an escaper appropriate to its **context** — text, quoted attribute, URL attribute, `<script>` JSON, `<style>`. Today: two byte-identical escapers (`rustnative-web/src/html.rs:21` and `rustnative-server/src/response.rs:144`, `pub(crate)`) handle text/attribute contexts; **URL attributes are not scheme-checked** — `Control::Link` writes `href` from `self.marks.links` unchecked (`src/dom.rs:774-776`), so app data containing `javascript:` becomes a link (the strict nonce CSP blocks `javascript:` navigation, so this is defence-in-depth, not an open hole — **verify CSP is always on for rendered pages**). Bound: one escaper module; typed sinks (`SafeUrl` constructed only through a scheme allow-list `http|https|mailto|tel|relative`); 0 XSS in a fuzzed render of arbitrary strings into every node kind, checked by parsing the output with `html5ever` (dev-dep) and asserting no element/attribute was created from data.
- **L7.2** *realizer agreement* JS = Rust for every node kind ✔ (`tests/runtime.rs`); **L7.3** *client subset agreement* Rust = generated JS incl. failures ✔ (`tests/client_subset.rs`) — extend with **generated programs**: a proptest generator of subset programs (arithmetic, strings, control flow, overflow, Unicode) compared in both languages; JS's `Number` vs Rust integers (overflow, `i64` beyond 2⁵³, division by zero, `%` of negatives, string indexing by UTF-16 vs UTF-8) is precisely where hand-picked cases miss.
- **L7.4** *webgen refuses what it cannot translate faithfully* ✔ (`each_construct_outside_the_subset_is_refused`) — bound: every syntax node kind of `syn` the translator does not handle has a refusal test (enumerate `syn::Expr` variants).
- **L7.5** *JS runtime quality:* `rn.js` has no type checking and no linter in CI. Bound: `tsc --checkJs --noEmit --strict` over `rn.js` with JSDoc types (0 errors) or at least ESLint recommended (0 errors), run in CI; runtime size budget (exists in `budgets/web.toml`? — add `runtime_js_kb` if not).
- **L7.6** *browser matrix:* Chromium-family only (Edge/Chrome); Firefox and Safari "not run" (`docs/guarantees.md`). Bound: Firefox via WebDriver BiDi in CI (Linux); Safari owed (needs macOS runner + `safaridriver` — the `apple` job's `macos-latest` runner can run it: **adopt**).
- **L7.7** *hydration determinism:* server render → hydrate → event → DOM equals server render of the new state ✔ (`pages_in_a_browser`); extend to random event sequences.
- **L7.8** *virtual list range events and canvas virtual elements* (review findings 10, 11) — fixed/drafted; checklist rows filled (`RESUME.md` item 2: rows 2, 4, 6, 8–16, 19, 22–26 — owed).
- **L7.9** *wasm boundary:* the 8 `extern` fns guarded (U3b); memory growth bounded per module (host limits ✔ in serverless).
- *Edge:* **L7.10** very large DOMs (10⁵ nodes) — hydration time budget; **L7.11** CSP strict mode in every test page (no `unsafe-inline`, no `unsafe-eval`) — the generated JS must never need `eval`/`new Function` (grep gate over `rn.js` and webgen output).

**Q2:** add **L7.12** *service-worker update safety*: a new `sw.js` version never serves a mix of old HTML and new JS (cache versioning test ✔? `offline_browser.rs` — extend with a version bump mid-session).

**Q3:** the Web layer is the only one where the framework's output is executed by a third party's interpreter on behalf of untrusted users; L7.1 failures are stored-XSS in every app built on it.

**Q4:** L7.1: 0 data-created elements in 10⁶ fuzzed renders/week; L7.3: 0 disagreements in 10 000 generated programs/push; L7.5: 0 type errors; L7.6: Chromium + Firefox blocking, Safari non-blocking → blocking after 2 green weeks.

**Q5/Q6:** html5ever-based XSS oracle (M5); differential proptest (M5); `tsc --checkJs` (E4); E5 for URLs via `SafeUrl` newtype.

**Q7:** for JS runtime correctness, migrating `rn.js` to TypeScript compiled at build time is the strong option but adds a Node toolchain to the build; `tsc --checkJs` with JSDoc gives most of the safety with zero build change — **chosen**.

**Q8:** Task 5.1 (escaping + SafeUrl + XSS oracle), Task 5.2 (webgen differential), Task 5.3 (rn.js checking + browser matrix), Task 5.4 (checklist rows — continues `RESUME.md` item 2).

**Q9:** the limit is a verified compiler for the subset — no. Differential testing at scale is the practical limit.

**Q10:** re-check when the client subset grows or a new deployment mode is added.

---

## L8 — Application layer (`rustnative-components` 2 658, `rustnative-data` 4 507, `rustnative-i18n` runtime side)

**Role:** headless behaviours and composite components (`behaviour.rs`, `widgets.rs`, `chart.rs`), the data layer (query cache with revalidation, mutations with offline queue, forms, migrations, state machines, tables, history, image, work).

**Q1 Indicators**
- **L8.1** *components are accessible by construction:* every component's rendered tree passes the a11y lint (L3.7) in headless, in every state (enumerate states: disabled, error, loading, empty, focused). **L8.2** *components work in both syntaxes* ✔ (description claims; equivalence test per component). **L8.3** *charts' virtual tables* are complete (every data point reachable as a cell) and stable under data updates.
- **L8.4** *query cache semantics* (tests exist: dedup, stale-while-revalidate, retention, retry/backoff, invalidation by prefix, identity preservation) — add **properties**: for any interleaving of fetch/complete/invalidate/unmount (generated), the cache never shows a result older than one already shown (monotonic freshness), never leaks an entry past retention, and issues ≤ 1 in-flight request per key.
- **L8.5** *offline mutation queue:* durable, ordered, exactly-once *delivery attempt semantics stated* (at-least-once with idempotency keys is the honest bound) — test: crash between send and ack (truncation harness from L3.8.3) → mutation re-sent with the same idempotency key, server dedups. **L8.6** *migrations:* every migration applied to every previous schema fixture reaches the current schema; down-migrations (if offered) round-trip. **L8.7** *state machines* (`machine.rs`): unreachable states and dead transitions reported at construction (a property, not a hope). **L8.8** *forms:* validation runs on the same rules client- and server-side (Web) — one definition, tested in both.
- *Edge:* **L8.9** 10⁶-row tables (`table.rs`, `list.rs`) — filter/sort within the M54 budget (filter-demo typing latency budget ✔ in `budgets/windows.toml`); **L8.10** clock skew and time travel in retention/backoff (injected clock).

**Q2:** add **L8.11** *cancellation*: an unmounted observer's in-flight query is cancelled (or its result discarded) and never renders (ties to L3.4.1).

**Q3:** the data layer is where apps lose user data: an offline queue that reorders or drops writes, or a migration that fails on a two-versions-old schema, loses work silently.

**Q4:** L8.4: 0 property violations in 10 000 generated interleavings; L8.5: every truncation offset → no loss, no duplicate effect; L8.6: N×M fixture matrix passes; L8.1: 0 a11y lint failures across all components × states.

**Q5/Q6:** model-based proptest against a simple reference cache model (M5); truncation harness; fixture matrix. E3 → **E4**.

**Q7:** model-based testing (reference model + random op sequences, à la `proptest-state-machine`) is the standard for caches/queues and strictly stronger than the current example tests.

**Q9/Q10:** at the practical limit with model-based tests; re-check when sync (L9) and data layers share more (offline queue ↔ replicas).

---

## L9 — Server and distributed (`rustnative-server` 7 870 / 12 `unsafe`, `-server-macros` 184, `rustnative-sync` 3 569, `rustnative-durable` 1 914, `rustnative-observe` 810)

**Role:** HTTP server with typed handlers and secure defaults (sessions, CSRF, CSP, rate limiting, passkeys, OAuth, passwords, tokens), checked SQL (`query!`), jobs, pages, serverless/edge, deploy exports; CRDT sync, Live server-interactive screens, MQTT; durable workflows, actors, supervision; tracing and consent-gated export.

### L9a — Server

- **L9a.1** *secure defaults are on and tested as defaults:* a fresh `App` sets CSP with nonce, `X-Content-Type-Options`, `Referrer-Policy`, `frame-ancestors`, HSTS (when TLS), `SameSite=Lax`+`HttpOnly`+`Secure` cookies, CSRF on unsafe methods, body limit 1 MiB, rate limit — one test asserting the full header set on a default response, so a refactor cannot drop one.
- **L9a.2** *review findings stay fixed:* cache never stores `no-store`/nonce-bearing responses and varies by `Accept-Language`/`Accept`, bounded (findings 13, 14); trusted-proxy configuration for the rate limiter (15); single-use passkey challenges (16); CSRF Bearer exemption only when the bearer authenticated (P2); session key rotation/revocation (P2); user-verification flag checked (P2). Each: a regression test named after the finding.
- **L9a.3** *HTTP robustness:* hyper does framing; the framework's own parsing (forms `body.rs`, cookies, multipart, query strings, JSON, headers like `X-Forwarded-For`, `traceparent`) is total and bounded — fuzz each; slowloris/slow-body handled (read timeout per request, documented default).
- **L9a.4** *auth crypto hygiene:* argon2 parameters ≥ OWASP current minimum (m=19 MiB, t=2, p=1) asserted by test; constant-time comparisons for tokens/CSRF/MACs (`subtle`/`ct_eq` — grep for `==` on secret byte slices); session cookie AES-GCM nonces random 96-bit and never reused under one key (rotation before 2³² encryptions — document the bound); CBOR/COSE parsing in passkeys (`auth/passkey.rs`) fuzzed.
- **L9a.5** *checked SQL:* `query!` validates against the schema at compile time (server-macros) — compile-fail tests for wrong column/type; all runtime SQL parameterized (grep gate: no `format!` into SQL strings).
- **L9a.6** *deploy exports run:* the generated Compose/Kubernetes/systemd files work with the default stack (finding 17) — CI job: `docker compose up` the export of `examples/server-demo`, hit `/healthz`, write data, restart, read it back; `kubeconform` validates the Kubernetes YAML; `systemd-analyze verify` the unit.
- **L9a.7** *operational endpoints:* `/metrics` not public by default (review P3) — test.
- **L9a.8** *serverless:* invocation keeps nothing; spawned work does not outlive it ✔; deadline refusal ✔.
- **L9a.9** *ZAP/OWASP baseline:* an automated scan (`zap-baseline.py` in Docker) against `server-demo` in CI, failing on medium+ alerts.

### L9b — Sync

- **L9b.1** CRDT laws (commutative, associative, idempotent merge; convergence) ✔ (`tests/crdt.rs` lawful merges) — raise case counts (U6) and add **causal delivery perturbation**: random reorder/duplication/delay of operations between N=3–5 replicas converges to identical state.
- **L9b.2** *HLC clock* (`clock.rs`, renamed `HlcClock`): monotonic under backwards wall-clock jumps; bounded drift acceptance; property test with an injected clock.
- **L9b.3** *Live sessions:* `max_sessions` (default 1024) enforced; per-session memory bounded; wire protocol (`live.rs` `WireEvent`) versioned and fuzzed; reconnect resumes or resets deterministically.
- **L9b.4** *MQTT decoder* (`mqtt.rs:178`) fuzzed; remaining-length varint ≤ 4 bytes enforced; packet size limit.
- **L9b.5** *device desired state:* `device::decode` versioned (`Versioned<T>`) — fixtures per version.

### L9c — Durable

- **L9c.1** *replay determinism:* a diverging replay fails loudly ✔ — add a property: any workflow (generated from a small op language: activity, timer, signal, compensation) replayed from its log after a crash at **every** step yields the same result and the same side-effect sequence (exactly-once activity *results*, at-least-once *execution* — stated).
- **L9c.2** *log format versioning* with fixtures (U11c); log growth bounded by snapshotting/compaction (U8).
- **L9c.3** *supervision* (`supervise.rs`): restart intensity limits; a crash loop escalates rather than spinning (bound: ≤ N restarts per window, then stop + report).
- **L9c.4** *actors:* message ordering per actor guaranteed and tested; mailbox bounded with an overflow policy.

### L9d — Observe

- **L9d.1** *nothing leaves without consent* ✔ — extend to every exporter and every signal (traces, metrics, logs, crash reports): a test per exporter with consent off asserts zero network/file writes (use a fake transport that panics if called).
- **L9d.2** *traceparent* parsing rejects garbage ✔ — fuzz it.
- **L9d.3** *PII:* attribute allow-list for exported spans; a test that UI text and form values are never exported by default.
- **L9d.4** *overhead:* tracing disabled costs ≤ 1 % on the core benchmark (instruction count).

**L9 Q2:** add **L9.5** *dependency-level security*: `cargo audit` ✔, plus `cargo deny` advisories for `rusqlite` bundled SQLite version (bundled C code is outside cargo-audit's view — pin and track SQLite CVEs manually, recorded).

**L9 Q3:** this layer faces the internet; each indicator maps to a concrete attack (XSS, CSRF, session theft, replay, injection, DoS) or data-loss path (replay divergence, sync divergence).

**L9 Q4 summary:** all regression tests binary; fuzz 1 CPU-h/week per decoder, 0 crashes; mutation score ≥ 90 % on `auth/`, `security.rs`, `cache.rs`; ZAP 0 medium+; deploy-export job green; convergence property 0 failures in 10 000 schedules.

**L9 Q5/Q6:** E3 → **E4** throughout; E5 for SQL (compile-time checked ✔), for secrets (`Secret<T>` newtype with no `Debug`/`Display`, zeroized on drop — check whether one exists; if not, Task 8.2).

**L9 Q7:** external DAST (ZAP) complements unit tests by testing the composed defaults the way an attacker sees them; fuzzing complements both for parsers. For crypto parameters, tests asserting constants are the right tool (cheap, exact).

**L9 Q9:** limit includes a third-party penetration test and a formal threat model per deployment shape (`docs/` has threat material? — the `threat-modeler` output is a candidate deliverable); recorded as pre-1.0 requirements.

**L9 Q10:** re-check after any change under `auth/`, `security.rs`, `cache.rs`, `app.rs` request pipeline, and before any public release.

---

## L10 — Tooling (`rustnative` CLI, 16 553 lines, 159 tests)

**Role:** `create/check/build/run/dev/package/deploy/bench/inspect/lsp/emulate/doctor/crash/compliance/upgrade` — the developer-facing surface and the producer of shipped artifacts (MSIX, APK/AAB, deb/rpm/pacman/AppImage, zip), deploy exports, and the serverless emulators (`emulate/lambda.rs`, `wagi.rs`).

**Q1 Indicators**
- **L10.1** *artifacts are valid by the platform's own validator:* MSIX via `makeappx`/`signtool verify` + WACK where available; APK via `apksigner verify` + `aapt2 dump`; deb via `lintian` (0 errors); rpm via `rpmlint` (0 errors); pacman via `namcap`; AppStream metadata via `appstreamcli validate`; zip via an independent reader. Some exist in the Linux distro job (`package::linux` tests) — bound: every packager × validator in CI.
- **L10.2** *artifacts are reproducible* (U14): building the same package twice gives identical bytes (timestamps from `SOURCE_DATE_EPOCH`, sorted entries, fixed permissions) — the zip/rpm/pacman writers are hand-written (`package/zip.rs`, `linux/rpm.rs`, `linux/pacman.rs`), so this is under the project's control.
- **L10.3** *package writers are correct against readers:* fuzz/proptest the zip/rpm/pacman writers with arbitrary file trees (names with Unicode, spaces, long paths > 255, empty files, symlinks, 4 GiB+ for zip64 if supported) and read back with `zip` crate / `rpm` tooling / `bsdtar` — round-trip equality.
- **L10.4** *the CLI never leaves a half-written project or artifact:* writes to temp + rename; Ctrl+C during `package` leaves no partial output at the final path.
- **L10.5** *error UX:* every user-facing error says what, where, and what to do (`error.rs`, 125 lines); `doctor` covers every platform (fix-all ✔).
- **L10.6** *`describe --json` snapshot* ✔ (`docs/api/framework.json`, test fails on drift).
- **L10.7** *LSP:* protocol messages handled without panics on arbitrary documents (fuzz the LSP request handlers with generated documents, incl. mid-edit invalid markup); responses within 100 ms for files ≤ 5 000 lines.
- **L10.8** *emulators match the real hosts' contracts:* Lambda emulator event/response shapes validated against AWS's published JSON schemas/fixtures; WAGI against its spec; documented divergences.
- **L10.9** *dev loop:* save → reload ≤ budget (M43), state preserved, broken build shown over the page ✔ (`web_dev.rs`); file-watcher handles editor atomic-save patterns (rename-over) on all three OSes.
- **L10.10** *bench measures what budgets claim* (U10, `RESUME.md` item 1): Linux `VmHWM`; a shipping-shaped app for `artifact_size_kb`.

**Q2:** add **L10.11** *panic policy for a CLI*: `unwrap`/`expect` counts are high (64/78 raw) — a CLI may panic on programmer error, but never on user input or environment (missing SDK, bad config). Fuzz `config.rs` (`rustnative.toml` parsing) and `project` templates with arbitrary names (path traversal: a project named `../../x` must be refused — test).

**Q3:** the CLI produces the binaries users install; a non-reproducible or linter-failing package is rejected by stores and distros, and a path-traversal in `create` writes outside the target directory.

**Q4:** L10.1: 0 validator errors per format; L10.2: identical hashes for 2 builds; L10.3: 0 round-trip differences in 10 000 generated trees; L10.7: 0 crashes, p95 ≤ 100 ms.

**Q5/Q6:** validators in CI (Linux job already installs distro tooling); proptest round-trips; E3 → **E4**.

**Q7:** using the platforms' own validators as oracles is better than any self-written check — they are what stores run.

**Q9/Q10:** limit = store submission dry-runs (Microsoft Store/Play Console APIs) — owed, needs accounts. Re-check when a packager or emulator is added.

---

## L11 — Project, CI, and release

**Role:** the layer that makes every other layer's enforcement real. If CI does not run it, no indicator above is enforced.

**Q1 Indicators**
- **L11.1** *CI runs every gate the gate scripts run locally:* the local gates (`rustnative-tools/gate.sh`, `gate-linux.sh`, `gate-android.sh`) and CI should be one definition. Today they are separate scripts outside the repo (`rustnative-tools/` is not version-controlled with the framework) — so contributors cannot run the same gate. Bound: `tools/gate/` in the repo is the single source; CI calls it.
- **L11.2** *master is green:* a red master is a P0 (review finding 1). Bound: time-to-green ≤ 1 day; red-master incidents counted per month.
- **L11.3** *CI time budget:* full CI ≤ 60 min wall clock (sharding via nextest timing data, U7); nightly jobs separate.
- **L11.4** *supply chain* (U14).
- **L11.5** *release pipeline:* tagging produces artifacts, SBOM, provenance, changelog, crates.io publish in dependency order, docs; dry-run on every push to master (`cargo publish --dry-run` per crate in order; `cargo package --list` for unwanted files).
- **L11.6** *security policy:* `SECURITY.md` names a private channel (review: it names none) — GitHub private vulnerability reporting enabled.
- **L11.7** *contributor gates documented:* `CONTRIBUTING.md` lists the Linux and Android gates (review: omitted).
- **L11.8** *nightly CI layer:* Miri, sanitizers, long fuzz, mutants (weekly), flake ×20, soak (weekly), full feature powerset (weekly), coverage report.
- **L11.9** *disk/time hygiene:* CI caches keyed by lockfile + toolchain; `target` sizes bounded (local: `target\r199` deletion note in `RESUME.md`).

**Q2:** add **L11.10** *toolchain pinning*: `rust-toolchain.toml` `channel = "stable"` → a pinned version updated by Dependabot-like PRs (`renovate` supports rust-toolchain), so local and CI agree (the standing memory "CI runs newer Rust than local" is a symptom). Keep a separate *latest-stable* CI job (non-blocking) as an early warning.

**Q3:** every other layer's E4 rung is only as real as this layer's CI; a gate that exists only in a developer's tool folder is E1.

**Q4:** L11.1: 1 gate definition; L11.2: ≤ 1 day; L11.3: ≤ 60 min; L11.5: dry-run green on every push.

**Q5/Q6:** GitHub Actions; required status checks on `master` (branch protection — **user decision**, since the repo convention is direct commits to master: protection with "required checks" still allows direct pushes by admins; recommend enabling required checks for non-admin pushes). E1 → **E4**.

**Q7:** a merge queue would make red master impossible (E5 for L11.2) but conflicts with the direct-to-master workflow; **recommendation:** keep direct commits, add a pre-push hook in `tools/gate/` that runs the fast gate (fmt, clippy, changed-crate tests) — E3 locally, E4 on CI.

**Q9:** limit = hermetic, reproducible CI (Nix/Bazel). Not justified for this project size; the reproducibility check (U14) gets the verifiable part.

**Q10:** re-check whenever a CI job is added or a gate script changes.

---

# Part D — Tasks

Ten waves. **Wave 0 fixes the confirmed P0/P1 defects**, each with its reproducing test. **Wave 1 builds the instruments** (no indicator can be enforced before it is measured); waves 2–9 walk the layers bottom-up. Inside a wave, tasks touching disjoint crates can run in parallel; across waves, order is mandatory. Every task: lands non-blocking (CI `continue-on-error: true` or a ratchet at the *current* value), proves itself green on CI for one push, then flips to blocking in the same task's last commit.

**Common verification for every task** (run before each commit; from `RESUME.md` "How to work here"):

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
$env:CARGO_TARGET_DIR='target\r199'; cargo +1.99 clippy --workspace --all-targets --all-features -- -D warnings; Remove-Item Env:CARGO_TARGET_DIR
$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps; Remove-Item Env:RUSTDOCFLAGS
```

plus the crate's own tests, plus the Linux gate when a non-Windows `cfg` path changed (`wsl -d Ubuntu -- bash /mnt/e/Users/HP/Documents/Projects/rustnative-tools/gate-linux.sh`), plus `docs/api/framework.json` regeneration when public API changed.

## Wave 0 — Confirmed P0/P1 defects (before any instrument work)

> [!IMPORTANT]
> **Apply the "⚠ READ BEFORE ACTING ON ANY FINDING: patch or redesign" block at the top of this plan to every task in every wave.** A task written here as a patch becomes a targeted redesign when that block's criteria hold, for any part of the codebase, not only the six areas it names.

These were found and, where marked ✅, reproduced during the loop. They come before wave 1 because each one is either a security hole, or data loss that users hit today, or a bug that makes the instruments lie. For each, the fix lands **with the test that reproduces it** (red first), and the same commit adds the indicator named in the findings file. The order is severity, then dependency.

### Task 0.1: Live session hijack and forged state (L9-F1 ✅, L9-F2 ✅, L9-F3; U24)

**Files:** `crates/rustnative-sync/src/live.rs`; tests in `crates/rustnative-sync/tests/live.rs`.

- [ ] Write failing tests that mirror `scratchpad/verify7`:
  - (a) a second client resuming another client's session id gets a fresh session;
  - (b) `Hello { snapshot: <forged JSON> }` is refused;
  - (c) a `Click` at a disabled or hidden node never reaches `update`.
- [ ] Session ids: 128-bit `getrandom` tokens, bound to the principal that created the session (the `__Host-session` id when one exists, otherwise a per-connection secret returned in `Welcome` and required on resume).
- [ ] Snapshots: seal with AES-256-GCM (reuse `rustnative_server::auth::session` sealing), carrying session id and expiry. `Drain` carries sealed snapshots only.
- [ ] Event authorization: check the target against the last tree sent, and that it is enabled and visible, before `dispatch`.
- [ ] Per-client caps on sessions and frames (L9.4).
- [ ] Run `cargo test -p rustnative-sync`. Expect all pass, including the three new tests. Commit.

### Task 0.2: Diff blindness, and the instrument that hid it (L3-F1 ✅, L4-F1; U21)

**Files:**
- `crates/rustnative-core/src/reconcile/diff.rs` (`node_changed`)
- `crates/rustnative-headless/src/*` (apply ops)
- a new `crates/rustnative-core/tests/diff_completeness.rs`

- [ ] Write the failing test first. For each `TreeNode` field, changing only that field yields ≥ 1 `Update` op. Today it fails for `control`, `grid`, `cursor`, `foreign`, and `shared_id`.
- [ ] Make `node_changed` destructure `TreeNode` exhaustively (`let TreeNode { id: _, kind, … } = a;`), so a new field is a compile error until it is classified.
- [ ] Headless applies `TreeOp`s to its object tree, with a debug shadow rebuilt from the snapshot and compared after each settle (L4.1).
- [ ] Run `cargo test -p rustnative-core -p rustnative-headless -p rustnative-conformance`, then the Windows `native_controls` tests (select options, checkbox with an overridden name, foreign-kind swap read back from the native control). Commit.

### Task 0.3: Panics that are contained must stay contained (L6-F1 ✅, L6-F2, L3-F2 ✅, L3-F3, L3-F6; U23)

**Files:**
- `crates/rustnative-tui/src/guard.rs`
- `crates/rustnative-core/src/component/tree.rs` (`render_component`: entry restore on unwind)
- `crates/rustnative-core/src/crash.rs`

- [ ] Write the failing tests:
  - a pty test where a `ReportAndContinue` panic is followed by a keypress that is delivered in raw mode (the verify6 assertion: console mode unchanged);
  - a core test where the tree renders again after a contained render panic.
- [ ] The TUI hook restores only when the panic will end the process. The session decides this from the policy and sets a flag before `catch_unwind` returns.
- [ ] `render_component` puts the entry back on unwind (a drop guard), so L3-F2 cannot happen.
- [ ] Crash reports: write only for uncontained panics, or mark contained ones non-fatal, with rate-limiting and a bounded directory.
- [ ] Run `cargo test -p rustnative-core -p rustnative-tui` and `tools/tui-session.sh tests`. Commit.

### Task 0.4: State written at exit is saved (L3-F4, confirmed in Windows/GTK/Android by L6; L4-F2; U20)

**Files:**
- `crates/rustnative-core/src/application.rs` (`lifecycle`: flush again after delivery)
- `crates/rustnative-windows/src/native/{app,lifecycle}.rs`
- `crates/rustnative-linux/src/gtk/app.rs`
- `crates/rustnative-android/src/lifecycle.rs`
- `crates/rustnative-headless` (honest kill)

- [ ] Write the failing test: a component that writes `Persisted` in `update(Lifecycle(Terminating))` finds it in the store after `lifecycle()` returns. Today that holds in the TUI only.
- [ ] `lifecycle()` flushes before delivery and again after. `notify` (Windows) flushes after dispatch.
- [ ] Headless `kill_and_restore` stops destructors from running (`mem::forget`, or a child process).
- [ ] Commit.

### Task 0.5: Persisted-state load errors never destroy data (L3-F17, L3-F18; U20, U25)

**Files:** `crates/rustnative-core/src/persistence/persisted.rs`, `services/file_state.rs`.

- [ ] Write the failing test: a store whose `load` fails once, then `set` + `flush`. The file must still hold the original bytes.
- [ ] `StateCache` keeps `Unknown` (not `None`) after a load error, refuses to overwrite `Unknown` keys, and retries the load.
- [ ] Corrupt or undeserializable values are moved to `quarantine/` and reported.
- [ ] Commit.

### Task 0.6: URL, CSS, and header contexts (L7-F1, L7-F2, L6-F5, L9-F5; U26)

**Files:**
- `crates/rustnative-web/src/{form.rs,dom.rs,css.rs,runtime/rn.js}`
- `crates/rustnative-windows/src/services/http.rs`
- `crates/rustnative-core` (`HttpRequest` validation)
- `crates/rustnative-server/src/security.rs` (`Cookie`)

- [ ] Write the failing tests:
  - `link(…, "javascript:alert(1)")` renders `href="#"` (and `rn.js` does the same under Node);
  - a font family `a, b; } * {` leaves the stylesheet's rule count unchanged;
  - a header value with CR/LF is refused by `HttpRequest`;
  - a cookie value with `;` is refused.
- [ ] Add a `SafeUrl` type for `href`/`action`, a CSS `<family-name>` serializer, and header and cookie validation at construction.
- [ ] Commit.

### Task 0.7: Unbounded work from data (L5-F12 ✅, L5-F13, L6-F4, L3-F26, L3-F8 ✅; U22)

**Files:**
- `crates/rustnative-drawlist/src/raster.rs` (clip before iterating)
- `crates/rustnative-tui/src/decode.rs` (paste and CSI limits plus a paste timeout)
- `crates/rustnative-core/src/layout/grid.rs` (placement clamp)
- `crates/rustnative-core/src/reconcile/snapshot.rs` and the recursive walkers (depth limit returns `RenderError::TooDeep`)

- [ ] Write the failing tests:
  - rasterize a 10⁹ px ellipse, path, and stroke-rect on 80×24 in under 1 ms;
  - a paste with no end marker followed by a key, where the key is delivered within 1 s;
  - `GridPlacement { row: 1e9 }` fails fast;
  - a depth-10 000 tree on a 1 MiB thread returns `TooDeep`.
- [ ] Fix each; commit per crate.

### Task 0.8: The updater (L5-F5, L5-F6, L5-F7, L6-F7; U20, U25)

**Files:** `crates/rustnative-update/src/lib.rs`, `crates/rustnative-windows/src/update.rs`.

- [ ] Write the failing tests:
  - `v2.0.0`, `1.2.x`, `01.0.0`, and a 21-digit part are all `BadManifest`;
  - a model `current` read error is an error, not `"0"`;
  - `version = "..\\x"` cannot leave `versions/`.
- [ ] Strict SemVer parser; `write_atomically` uses fsync and a unique temp name; `stage` validates the version as a single path component.
- [ ] Commit.

### Task 0.9: `rustnative fmt` keeps comments and line endings (L10-F1, L2 formatter ✅; U25)

**Files:** `crates/rustnative-markup/src/format.rs` (comment trivia), `crates/rustnative/src/{cli.rs,markup.rs}`.

- [ ] Write the failing test: formatting a file with comments in markup preserves every comment token, and a CRLF file stays CRLF.
- [ ] Until the formatter carries trivia, `fmt` refuses (with an error naming the line) any call site whose token stream contains a comment the formatter would drop.
- [ ] Writes are atomic.
- [ ] Commit.

### Task 0.10: Crash reports are private (L3-F5, L6-F6; U27)

**Files:** `crates/rustnative-core/src/{crash.rs,application.rs}`.

- [ ] Write the failing tests:
  - a marker typed into a `TextInput` never appears in a report;
  - with no `LOCALAPPDATA`/`XDG_STATE_HOME`/`HOME`, crash capture is disabled rather than writing to `temp_dir`;
  - an id containing `..` or a separator is refused.
- [ ] Commit.

Tasks 0.11–0.24 were added by the evidence loop and are listed under E.7 ("Wave 0 additions"). They follow the same red-test-first procedure.

## Wave 1 — Instruments

### Task 1.1: Feature and target matrix (U1)

**Files:** Modify `.github/workflows/ci.yml` (new job `features`).

- [ ] **Step 1: Run locally to learn the current state**

```bash
cargo install cargo-hack --locked
cargo hack check --workspace --feature-powerset --depth 2 --no-dev-deps --exclude rustnative-windows --exclude rustnative-android
```

Expected: a list of failing combinations (or none). Record each failure as a sub-step fix in this task (one commit per crate).

- [ ] **Step 2: Add the job, non-blocking**

```yaml
  features:
    name: feature combinations (ubuntu)
    runs-on: ubuntu-latest
    continue-on-error: true   # removed in step 4
    steps:
      - uses: actions/checkout@<sha> # v4
      - uses: dtolnay/rust-toolchain@<sha> # stable
      - uses: taiki-e/install-action@<sha> # cargo-hack
        with: { tool: cargo-hack }
      - run: sudo apt-get update && sudo apt-get install -y libgtk-4-dev libsoup-3.0-dev
      - run: cargo hack check --workspace --feature-powerset --depth 2 --no-dev-deps --exclude rustnative-windows --exclude rustnative-android
```

(SHA placeholders are filled by Task 1.13, which pins every action; until then use the same tags as the rest of `ci.yml`.)

- [ ] **Step 3: Fix each failing combination** — one commit per crate; the usual fix is a missing `#[cfg(feature = ...)]` on an import or a feature that must enable another in `Cargo.toml` `[features]`.
- [ ] **Step 4: Make blocking** — delete `continue-on-error`. Add the Windows leg (`runs-on: windows-latest`, `-p rustnative-windows`) the same way.
- [ ] **Step 5: Commit** `CI: check every feature combination to depth 2`.

### Task 1.2: One ratchet script for counted patterns (U2, U3, U4, U5)

**Files:** Create `tools/ratchets/count.py`, `tools/ratchets/ratchets.toml`; modify `.github/workflows/ci.yml` (step in `lint`); modify `Cargo.toml` `[workspace.lints.clippy]`.

**Interfaces:** Produces `python3 -I tools/ratchets/count.py [--update]` — exit 1 when any count exceeds its ratchet, prints `crate pattern current ratchet`; `--update` lowers ratchets to current values (never raises). Later tasks lower values only through `--update`.

- [ ] **Step 1: Write the script** — one script, patterns declared in the TOML, test-code excluded by path and by skipping everything after a line matching `^#\[cfg\(test\)\]` at column 0:

```python
#!/usr/bin/env python3
"""Counts declared source patterns per crate and fails when a count rises.

Usage: python3 -I tools/ratchets/count.py [--update]
"""
import pathlib, re, sys, tomllib

ROOT = pathlib.Path(__file__).resolve().parents[2]
CONF = ROOT / "tools" / "ratchets" / "ratchets.toml"
SKIP_PARTS = {"tests", "benches", "device_tests", "fuzz", "target"}
SKIP_SUFFIXES = ("_integration.rs", "_tests.rs")

def library_lines(path):
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("#[cfg(test)]"):
            return
        yield line

def count(crate_dir, regex):
    total = 0
    for path in (crate_dir / "src").rglob("*.rs"):
        rel = path.relative_to(crate_dir)
        if SKIP_PARTS & set(rel.parts) or path.name.endswith(SKIP_SUFFIXES) or path.name == "tests.rs":
            continue
        total += sum(len(regex.findall(line)) for line in library_lines(path))
    return total

def main():
    update = "--update" in sys.argv
    conf = tomllib.loads(CONF.read_text(encoding="utf-8"))
    patterns = {name: re.compile(rx) for name, rx in conf["patterns"].items()}
    failed, lines_out = False, []
    for crate, limits in sorted(conf["crates"].items()):
        for name, limit in limits.items():
            current = count(ROOT / "crates" / crate, patterns[name])
            if current > limit:
                failed = True
                print(f"RISE  {crate:28} {name:12} {current:5} > {limit}")
            elif current < limit and update:
                limits[name] = current
            lines_out.append((crate, name, current, limit))
    if update:
        out = ["[patterns]"] + [f'{k} = {v!r}' for k, v in conf["patterns"].items()]
        for crate, limits in sorted(conf["crates"].items()):
            out.append(f'\n[crates."{crate}"]')
            out += [f"{k} = {v}" for k, v in limits.items()]
        CONF.write_text("\n".join(out) + "\n", encoding="utf-8")
    sys.exit(1 if failed else 0)

if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Seed `ratchets.toml` at today's counts** — patterns: `unsafe = '\bunsafe\b'`, `allow = '#\[allow\('`, `expect_attr = '#\[expect\('`, `unwrap = '\.unwrap\(\)'`, `expect = '\.expect\('`, `panic = '\b(panic|unreachable|todo|unimplemented)!'`, `let_ignore = 'let _ ='`. Run once with every limit at 99999 and `--update` to write the real values.
- [ ] **Step 3: Self-test** — add a temporary `.unwrap()` in `crates/rustnative-types/src/lib.rs`, run, expect `RISE … unwrap 1 > 0` and exit 1; remove it.
- [ ] **Step 4: CI** — in job `lint`, add `- run: python3 -I tools/ratchets/count.py`.
- [ ] **Step 5: Lints** — add to `[workspace.lints.clippy]`: `allow_attributes_without_reason = "warn"`, `let_underscore_must_use = "warn"`. Fix what `-D warnings` reports (expect a large first diff: do it crate by crate, one commit each; convert `#[allow(x)]` → `#[expect(x, reason = "…")]`).
- [ ] **Step 6: Commit** `Ratchets: counted unsafe, allows, panics, and ignored results per crate`.

### Task 1.3: `forbid(unsafe_code)` where unsafe is unused; overflow checks in test builds (U3, U5)

**Files:** `src/lib.rs` of `rustnative-types`, `-style`, `-markup`, `-macros`, `-i18n`, `-components`, `-conformance`, `-data`, `-drawlist`, `-durable`, `-headless`, `-host`, `-observe`, `-server-macros`, `-sync`, `-update`, `-web-macros`, `-web-testing` (each 0 `unsafe` in `src/`); `Cargo.toml` `[profile.test]`/`[profile.bench]`.

- [ ] **Step 1:** add `#![forbid(unsafe_code)]` to each listed `lib.rs`; build; any failure means the count was wrong — move that crate to the allow-list instead.
- [ ] **Step 2:** add to `Cargo.toml`:

```toml
[profile.test]
overflow-checks = true
[profile.bench]
overflow-checks = true
```

(`dev` already has them; this makes `cargo test --release` and benches find wrapping arithmetic too.) Decision recorded in the `arithmetic_side_effects` comment in `Cargo.toml`.
- [ ] **Step 3:** run `cargo test --workspace --release` once on Windows and once in WSL; every new overflow panic is a real bug → fix with `saturating_*`/`checked_*` and a test.
- [ ] **Step 4: Commit** `Forbid unsafe code in the 18 crates that use none; overflow checks in test and bench builds`.

### Task 1.4: Fuzzing infrastructure (U6, U16)

**Files:** Create `fuzz/Cargo.toml` (own workspace, `publish = false`, excluded from the root workspace via `exclude = ["fuzz"]`), `fuzz/fuzz_targets/*.rs`, `fuzz/corpus/<target>/` seeds; modify root `Cargo.toml` (`exclude`), `.github/workflows/ci.yml` (jobs `fuzz-smoke`, `fuzz-nightly`).

**Interfaces:** Produces one target per trust-boundary reader; later waves add targets by adding a file and a `[[bin]]` entry. Initial targets (all exist as public fns today): `style_sheet` (`rustnative_style::sheet::parse`), `style_value` (`parse_value` for each `ValueKind`), `style_color`, `style_shadow`, `i18n_catalogue` (`rustnative_i18n::parse`), `interop_idl` (`rustnative_interop::idl::parse_idl` — check visibility; if private, add a `#[doc(hidden)] pub fn fuzz_parse` behind a `fuzzing` cfg), `tui_decode` (`rustnative_tui::decode::Decoder::feed`), `mqtt_packet` (`rustnative_sync::mqtt::…::decode`), `inspect_compact` (`rustnative_core::inspect::compact::decode`), `update_manifest` (the verifier entry), `markup` (`rustnative_markup::parse_markup` over `syn::parse_str::<proc_macro2::TokenStream>`).

- [ ] **Step 1: Skeleton target** (`fuzz/fuzz_targets/style_sheet.rs`):

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = rustnative_style::sheet::parse(text);
    }
});
```

and for the split-invariance property of the terminal decoder (`fuzz/fuzz_targets/tui_decode.rs`):

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: (Vec<u8>, usize)| {
    let (bytes, split) = input;
    let split = if bytes.is_empty() { 0 } else { split % bytes.len() };
    let mut whole = rustnative_tui::decode::Decoder::default();
    let mut parts = rustnative_tui::decode::Decoder::default();
    let a = whole.feed(&bytes);
    let mut b = parts.feed(&bytes[..split]);
    b.extend(parts.feed(&bytes[split..]));
    assert_eq!(a, b, "decoding must not depend on how the input was split");
});
```

(Adjust constructor names to the real API when writing; `InputEvent` must be `PartialEq + Debug` — add the derive if missing.)

- [ ] **Step 2: Seeds** — copy every `.css`, `.ftl`, `.ril`, `.rsx` file in the repo into the matching corpus; for `tui_decode`, the escape sequences from `crates/rustnative-tui/tests`.
- [ ] **Step 3: Run each target 10 min locally (WSL)**: `cargo +nightly fuzz run style_sheet -- -max_total_time=600 -max_len=65536`. Every crash → minimize (`cargo fuzz tmin`), add the input as a regression `#[test]` in the owning crate, fix, commit per crash.
- [ ] **Step 4: CI** — `fuzz-smoke` (per push, blocking after one green week): matrix over targets, `-max_total_time=60`. `fuzz-nightly` (schedule `cron: '0 2 * * *'`): `-max_total_time=3600` per target, corpus cached with `actions/cache` keyed by target, crash artifacts uploaded.
- [ ] **Step 5: Commit** `Fuzzing: targets for every parser and decoder, smoke per push, an hour nightly`.

### Task 1.5: Property-test depth (U6)

**Files:** `.github/workflows/ci.yml` (env), existing property test files.

- [ ] **Step 1:** set `PROPTEST_CASES: 1024` in the workflow `env` block (beside `RUSTFLAGS`).
- [ ] **Step 2:** add a nightly job running `cargo test --workspace --release -- proptest` filtered tests with `PROPTEST_CASES=100000` (tests must be named or moduled so a filter selects them: put them under a `property` module in each file).
- [ ] **Step 3:** ensure `*.proptest-regressions` files are committed (one exists: `crates/rustnative-core/tests/property_tests.proptest-regressions`) and not gitignored.
- [ ] **Step 4: Commit** `Property tests: 1 024 cases per push, 100 000 nightly`.

### Task 1.6: Flake detection and quarantine (U7)

**Files:** Create `tools/ratchets/quarantine.toml`, `tools/ratchets/quarantine.py`; modify `.github/workflows/ci.yml` (job `flake-nightly`); `.config/nextest.toml`.

- [ ] **Step 1:** `quarantine.toml` format: `[[test]] name = "crate::path::test" owner = "omar" expires = 2026-11-15 reason = "…"`; `quarantine.py` fails when any entry's `expires` is past (blocking in `lint`) and prints `--skip` flags for nextest.
- [ ] **Step 2:** `.config/nextest.toml` with a `ci` profile: `retries = 0`, `fail-fast = false`, JUnit output `target/nextest/ci/junit.xml`.
- [ ] **Step 3:** `flake-nightly`: matrix over OS (ubuntu, windows), runs `cargo nextest run --workspace --profile ci` 20 times in a loop (each iteration `--test-threads=1` on odd runs, `--test-threads=64` on even runs), uploads JUnit files, and a tiny script aggregates per-test pass counts; any test < 20/20 opens/updates an issue via `gh issue create --label flaky` (needs `issues: write` permission on that job only).
- [ ] **Step 4:** grep gate in `lint`: `grep -rnE 'thread::sleep|Instant::now\(\)|SystemTime::now\(\)' crates/*/tests` compared against an allow-list file `tools/ratchets/wallclock-allow.txt` (seed with today's hits; ratchet down).
- [ ] **Step 5: Commit** `Flake detection: twenty repeated runs nightly, quarantine with owners and expiry`.

### Task 1.7: Soak harness (U8)

**Files:** Create `crates/rustnative/src/soak.rs` (a `rustnative soak <scenario> --hours N` subcommand beside `bench.rs`, reusing its process-memory measurement); modify `bench.rs` to share the measurement function; `.github/workflows/ci.yml` (job `soak-weekly`).

**Interfaces:** Consumes `bench.rs`'s memory sampler (extended in Task 9.1 to Linux `VmRSS`/`VmHWM`). Produces `rustnative soak --scenario {server,live,sync,durable,tui,headless} --hours <f64> --max-slope-pct-per-hour <f64>` exit 1 on breach, CSV of samples.

- [ ] **Step 1:** sampler: every 10 s record RSS, handle/fd count (`/proc/self/fd` count on Linux; `GetProcessHandleCount` on Windows), thread count.
- [ ] **Step 2:** slope = least-squares fit of RSS over the last 80 % of samples (skip warm-up), as % of the mean per hour.
- [ ] **Step 3:** scenarios: `server` drives `examples/server-demo` with a fixed request mix (keep-alive, form posts, page renders with distinct query strings to stress the cache bound); `live` opens/closes Live sessions at 10/s up to `max_sessions`; `sync` merges random ops between 3 in-process replicas; `durable` runs workflows to completion in a loop; `tui` drives `examples/tui-demo` on the synthetic terminal; `headless` mounts/unmounts the reference app.
- [ ] **Step 4: Unit test** the slope function on synthetic data (flat → ~0; linear growth → expected value).
- [ ] **Step 5:** `soak-weekly` (cron Sunday) runs each scenario 6 h on ubuntu, 1 h on windows; blocking for release tags only (a release workflow step checks the latest soak run's status).
- [ ] **Step 6: Commit** `Soak: memory, handle, and thread slopes over hours for every long-running scenario`.

### Task 1.8: Coverage and mutation testing (U6)

**Files:** `.github/workflows/ci.yml` (jobs `coverage`, `mutants-diff`, `mutants-weekly`), `.cargo/mutants.toml`.

- [ ] **Step 1:** `coverage` (per push, ubuntu): `cargo llvm-cov --workspace --exclude rustnative-windows --exclude rustnative-android --lcov --output-path lcov.info`; changed-line coverage via `diff-cover lcov.info --compare-branch=origin/master~1 --fail-under=80` (direct-to-master workflow: compare with the previous master commit). Non-blocking for two weeks, then blocking.
- [ ] **Step 2:** `.cargo/mutants.toml`: `exclude_globs = ["**/tests/**", "**/device_tests/**", "examples/**"]`, `timeout_multiplier = 3`.
- [ ] **Step 3:** `mutants-diff` (per push): `git diff HEAD~1 > d.diff && cargo mutants --in-diff d.diff -p <changed crates> --no-shuffle`; blocking for paths under `crates/rustnative-update/`, `crates/rustnative-server/src/auth/`, `crates/rustnative-server/src/security.rs`, `crates/rustnative-server/src/cache.rs`; report-only elsewhere.
- [ ] **Step 4:** `mutants-weekly`: whole-crate runs, one matrix leg per crate (excluding windows/android/linux GUI crates on ubuntu), results uploaded; the per-crate score table is appended to `docs/quality/mutation-scores.md` by a scheduled commit-free artifact (no bot commits to master — the file is updated by hand when reviewing).
- [ ] **Step 5: Commit** `Coverage on changed lines and mutation testing on the security-critical set`.

### Task 1.9: Persisted and wire format fixtures (U11c)

**Files:** Create `docs/formats.md` (inventory: format → owning module → version field → fixture dir); create `tests/fixtures/<format>/v1.*` in each owning crate.

Formats to inventory (from the survey): persisted component state (`rustnative-core/src/persistence`), `FileStateStore` (core engine), inspection wire (`core/src/wire.rs`, `inspect/compact.rs`), crash report (`core/src/crash.rs`), update manifest (`rustnative-update`), offline mutation queue and migrations (`rustnative-data`), sync replica/device payloads (`rustnative-sync/src/device.rs` `Versioned<T>`), Live wire (`sync/src/live.rs`), durable workflow log and actor state (`rustnative-durable`), server session cookie (`server/src/auth/session.rs`), budgets TOML schema (`budgets/SCHEMA.md`), `rustnative.toml` (`crates/rustnative/src/config.rs`), `.ril`, `framework.json`.

- [ ] **Step 1:** for each format, check whether it carries a version; if not, record "unversioned" in `docs/formats.md` and create a follow-up task in the owning layer's wave (versioning is a format change and must be its own commit with a migration).
- [ ] **Step 2:** for each versioned format, write today's representative value to `tests/fixtures/<format>/v<current>.<ext>` and a test `fixture_v<current>_still_loads` that decodes it and asserts key fields.
- [ ] **Step 3:** add to `CONTRIBUTING.md`: "changing a format = new version + new fixture; old fixtures are never edited".
- [ ] **Step 4: Commit** `Formats: an inventory, a version per format, and a fixture per version`.

### Task 1.10: The claims checker (U12, U18)

**Files:** Create `tools/ratchets/claims.py`, `docs/status.toml`; modify CI `lint`.

- [ ] **Step 1:** parse `docs/guarantees.md` and `docs/conformance/new-backend-checklist.md` for backtick spans matching `path::fn_name` or `file.rs::fn_name` or bare `fn_name` in test columns; for each, assert `fn <name>` exists in the named file (or anywhere under `crates/*/tests`, `crates/*/src` for bare names). Print unresolved references; exit 1.
- [ ] **Step 2:** checklist completeness per backend column → compare with `docs/status.toml` (`[backends.windows] state = "complete" checklist_percent = …`); a backend whose state is `complete` with any `—` cell fails.
- [ ] **Step 3:** README/BUILD_STATUS: a test that the backend list in README's "Current status" equals `docs/status.toml`'s keys with state ≠ `planned` (simple string presence check per backend name).
- [ ] **Step 4:** run; fix every unresolved reference (docs commit) before making it blocking.
- [ ] **Step 5: Commit** `Claims: every named test exists, and status follows the checklist`.

### Task 1.11: Architecture tests (U13)

**Files:** Create `crates/rustnative-conformance/tests/architecture.rs` (conformance already depends on the workspace; uses `std::process::Command` for `cargo metadata` and plain file reads — no new deps).

- [ ] **Step 1: Crate graph test**

```rust
//! Section 2's invariants as tests (`PLAN.md` Milestone 65, C129). Each
//! failure names the principle it breaks.

use std::process::Command;

fn metadata() -> serde_json::Value {
    let out = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .output()
        .expect("cargo metadata runs");
    serde_json::from_slice(&out.stdout).expect("cargo metadata prints JSON")
}

fn deps_of(meta: &serde_json::Value, krate: &str) -> Vec<String> {
    meta["packages"].as_array().unwrap().iter()
        .find(|p| p["name"] == krate).unwrap()["dependencies"].as_array().unwrap().iter()
        .filter(|d| d["kind"].is_null()) // normal dependencies only
        .map(|d| d["name"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn principle_2_4_the_core_depends_on_no_backend_and_no_os_binding() {
    let deps = deps_of(&metadata(), "rustnative-core");
    for forbidden in ["windows-sys", "windows", "gtk4", "jni", "libc", "ndk",
                      "rustnative-windows", "rustnative-linux", "rustnative-android", "rustnative-tui", "rustnative-web"] {
        assert!(!deps.iter().any(|d| d == forbidden),
            "2.4: rustnative-core must not depend on {forbidden}");
    }
}

#[test]
fn no_backend_depends_on_another_backend() {
    let meta = metadata();
    let backends = ["rustnative-windows", "rustnative-linux", "rustnative-android", "rustnative-tui", "rustnative-web"];
    for b in backends {
        let deps = deps_of(&meta, b);
        for other in backends.iter().filter(|o| **o != b) {
            assert!(!deps.iter().any(|d| d == other), "{b} must not depend on {other}; shared code belongs in a platform group");
        }
    }
}
```

(`serde_json` is already in the workspace lockfile; add it to conformance's `[dev-dependencies]` if absent.)

- [ ] **Step 2: Core purity (source)** — a test reading every `crates/rustnative-core/src/**/*.rs` and asserting no line contains `cfg(target_os`, `cfg(windows`, `cfg(unix`, `cfg(target_family`.
- [ ] **Step 3: Core module layering** — encode the module-map order from `crates/rustnative-core/src/lib.rs:100-129` as a list; for each module file, collect `use crate::<m>` / `crate::<m>::` references; fail when a module references one listed *above* it (higher), except `platform → application` (documented exception). Run it: expect violations — record each as either a real layering bug (fix in wave 3) or a documentation error in the module map (fix the doc). Start non-blocking via `#[ignore]` + a nightly `--ignored` run; un-ignore when clean.
- [ ] **Step 4: Backend ownership modules** — per backend, list the modules allowed to name the host binding crate (`windows_sys::`, `gtk4::`/`gtk::`, `jni::`, `libc::`); test that no other module does. Seed the allow-lists with today's reality (they are probably broad) and record narrowing as wave-6 work.
- [ ] **Step 5: Examples name no backend** — grep `examples/*/src/**/*.rs` for `rustnative_windows::|rustnative_linux::|rustnative_android::` with an allow-list for adoption examples (`adoption-*`, `gdi-font-diagnostic`, `android-device-tests`).
- [ ] **Step 6: Commit** `Architecture: section 2's boundaries as tests`.

### Task 1.12: Public API snapshots and sealing audit (U11)

**Files:** Create `docs/api/crates/<crate>.txt` per published crate; `tools/ratchets/sealing.py`; CI job `public-api` (nightly toolchain for rustdoc JSON).

- [ ] **Step 1:** `cargo install cargo-public-api --locked`; for each published crate: `cargo +nightly public-api -p <crate> --simplified > docs/api/crates/<crate>.txt`.
- [ ] **Step 2:** CI `public-api`: regenerate and `git diff --exit-code docs/api/crates`; the failure message says "public API changed: regenerate with … and bump the version if breaking".
- [ ] **Step 3:** `cargo semver-checks` job, enabled after the first `v0.1.0` tag (until then the snapshot diff is the gate). Record the activation in `docs/stability.md` (exists? if not, part of M52).
- [ ] **Step 4:** `sealing.py`: from the snapshot text, list `pub enum` lines not preceded by `#[non_exhaustive]` and `pub trait` lines not marked sealed (convention: a `Sealed` supertrait from a private module); output as a review list in `docs/api/sealing-audit.md`; decisions are made per item (some enums are deliberately exhaustive — e.g. `PointerPhase`); the script then gates *new* unsealed items only (ratchet by name list).
- [ ] **Step 5: Commit** `Public API: per-crate snapshots diffed in CI, and the sealing audit`.

### Task 1.13: Supply chain (U14, L11.10)

**Files:** `.github/workflows/ci.yml`, `.github/dependabot.yml`, `rust-toolchain.toml`, `deny.toml`, new `.github/workflows/release.yml`.

- [ ] **Step 1:** pin every `uses:` to a full commit SHA with the tag as a trailing comment (`pinact run` does it mechanically); add a `lint` step: `! grep -nE 'uses: [^@]+@v?[0-9]+(\.[0-9]+)*\s*$' .github/workflows/*.yml` (fails on tag-only pins).
- [ ] **Step 2:** `rust-toolchain.toml`: `channel = "1.99.0"` (the CI-current stable at the time of the task — check `rustc +stable --version` first); keep a `latest-stable` CI job, `continue-on-error: true`, as the early-warning lane.
- [ ] **Step 3:** `deny.toml` `multiple-versions = "deny"` with `skip = [...]` for the three already in `clippy.toml`'s `allowed-duplicate-crates` (`syn`, `jni-sys`, `windows-sys`), each with a reason.
- [ ] **Step 4:** `dependabot.yml`: `github-actions` and `cargo` weekly, grouped.
- [ ] **Step 5:** `release.yml` (on tag `v*`): build CLI for Windows/Linux, `cargo cyclonedx` SBOM, `actions/attest-build-provenance`, upload; a `reproducible` job builds the Linux CLI twice (different checkout paths, `SOURCE_DATE_EPOCH` from the tag commit, `RUSTFLAGS=--remap-path-prefix=$PWD=.`) and compares SHA-256; mismatch → `diffoscope` report artifact, job fails.
- [ ] **Step 6: Commit** one per step.

### Task 1.14: Miri and sanitizers (U5, U9)

**Files:** `.github/workflows/ci.yml` (job `unsafe-nightly`), `tools/sanitizers/glib.supp`.

- [ ] **Step 1: Miri** — `cargo +nightly miri test -p rustnative-core -p rustnative-interop -p rustnative-types` (types/core have pure-Rust tests; interop's runtime tests must avoid real FFI — gate those with `#[cfg_attr(miri, ignore)]`). Record which tests Miri cannot run and why.
- [ ] **Step 2: ASan** — `RUSTFLAGS="-Zsanitizer=address" ASAN_OPTIONS=detect_leaks=1 cargo +nightly test -Zbuild-std --target x86_64-unknown-linux-gnu -p rustnative-tui -p rustnative-interop`, then `-p rustnative-linux` under `tools/linux-session.sh xvfb` with `LSAN_OPTIONS=suppressions=tools/sanitizers/glib.supp`.
- [ ] **Step 3: TSan** — same with `-Zsanitizer=thread` over `-p rustnative-core -p rustnative-server -p rustnative-sync -p rustnative-observe` (the crates with locks).
- [ ] **Step 4:** all non-blocking for one week, then blocking for Miri and TSan on core; ASan on Linux GTK stays non-blocking until suppressions stabilise (decision recorded in the job comment).
- [ ] **Step 5: Windows Application Verifier** — a PowerShell step on `windows-latest`: `appverif /verify target\debug\deps\rustnative_windows-*.exe /faults` is too broad; use `appverif -enable Handles Heaps Locks -for <test exe>` then run the integration test binary; parse `appverif -export log`. Non-blocking (E2) until a clean baseline exists.
- [ ] **Step 6: Commit** `Unsafe checked by tools: Miri, ASan, TSan nightly; Application Verifier on Windows`.

### Task 1.15: The gate lives in the repository (L11.1)

**Files:** Create `tools/gate/gate.ps1`, `tools/gate/gate.sh`, `tools/gate/pre-push`; modify `CONTRIBUTING.md`.

- [ ] **Step 1:** port the logic of `rustnative-tools/gate.sh`, `gate-linux.sh`, `gate-android.sh`, `t.ps1` into `tools/gate/` (read each first; drop machine-specific paths — use `$PSScriptRoot`/`$(dirname "$0")`).
- [ ] **Step 2:** `pre-push` hook (opt-in via `git config core.hooksPath tools/gate/hooks`): fmt check + clippy + `python3 -I tools/ratchets/count.py` + tests of crates changed since `@{u}`.
- [ ] **Step 3:** CI jobs call the same scripts where they duplicate them (start with `lint`).
- [ ] **Step 4:** `CONTRIBUTING.md`: list the Linux and Android gates (review finding), the hook, and the ratchets.
- [ ] **Step 5: Commit** `The gate in the repository: one definition for contributors and CI`.

## Wave 2 — L0–L2

### Task 2.1: Value-type laws and the finite-scalar claim (L0)

**Files:** Create `crates/rustnative-types/tests/laws.rs`; maybe modify `crates/rustnative-types/src/scalar.rs:19-21`.

- [ ] **Step 1: Exhaustive test for `Scalar::new`** (runs every `f32` bit pattern; ~4·10⁹ iterations — `#[ignore]` by default, run in the nightly job with `--release --ignored`):

```rust
#[test]
#[ignore = "exhaustive: every f32; run with --release --ignored"]
fn every_f32_becomes_a_finite_scalar() {
    for bits in 0..=u32::MAX {
        let value = f32::from_bits(bits);
        let scalar = rustnative_types::Scalar::new(value);
        assert!(scalar.get().is_finite(), "Scalar::new({value:?}) = {:?} is not finite", scalar.get());
        assert!(scalar.get().to_bits() != (-0.0f32).to_bits(), "-0.0 must be normalized");
    }
}

#[test]
fn infinities_are_not_scalars() {
    assert!(rustnative_types::Scalar::new(f32::INFINITY).get().is_finite());
    assert!(rustnative_types::Scalar::new(f32::NEG_INFINITY).get().is_finite());
}
```

- [ ] **Step 2: Run** `cargo test -p rustnative-types --test laws infinities_are_not_scalars`. Expected today: **FAIL** (the constructor admits ±∞). If it passes, delete the defect note in L0/Q3 and keep the test.
- [ ] **Step 3: Fix (separate commit)** — choose and document: map ±∞ to `±f32::MAX` (keeps sign, stays finite). Minimal change:

```rust
pub fn new(value: f32) -> Self {
    if value.is_nan() || value == 0.0 {
        Self(0.0)
    } else {
        Self(value.clamp(-f32::MAX, f32::MAX))
    }
}
```

and update the doc comment to state the mapping.
- [ ] **Step 4: Law suites** — proptest `Eq`/`Hash` consistency and `Ord` totality for every type in `geometry.rs`, `color.rs`, `scalar.rs`, with special values forced via `prop_oneof!`.
- [ ] **Step 5:** check whether the crate uses `alloc`; if not, add a CI build `--no-default-features` without `alloc` as the L0.6 proof.
- [ ] **Step 6: Commit** `Types: every value-type law tested, the finite scalar made finite`.

### Task 2.2: Style parser fuzzing, determinism, and a differential oracle (L1)

**Files:** fuzz targets from Task 1.4; create `crates/rustnative-style/tests/{equivalence_count.rs,determinism.rs,differential.rs}`; `crates/rustnative-style/Cargo.toml` (`[dev-dependencies] cssparser = "…"` or `lightningcss`).

- [ ] **Step 1:** run the four style fuzz targets for 1 h each; fix every crash with a regression test.
- [ ] **Step 2:** determinism test — parse and render the largest sheet twice with two fresh `HashMap` seeds (two separate parse calls already use separate `RandomState`s) and assert byte-identical generated CSS / resolved output.
- [ ] **Step 3:** equivalence count — assert the conformance style-equivalence suite covers `vocabulary::ALL.len()` properties (expose the count if needed).
- [ ] **Step 4:** differential — for generated declaration strings in the Web-facing subset, if both `cssparser` and `rustnative_style::value::parse_value` accept, the parsed numeric values agree.
- [ ] **Step 5:** `clippy.toml` cannot scope `disallowed-types` per module; instead add `#![deny(clippy::disallowed_types)]` + a crate-level `clippy.toml`? — not supported per crate either. **Use the determinism test as the enforcement** (E4) and note E5 as infeasible with current clippy.
- [ ] **Step 6: Commit** per step.

### Task 2.3: Markup, i18n, interop parsers: depth limits, formatter laws, hygiene, CLDR (L2, Review Focus 1)

**Files:** `crates/rustnative-markup/src/parse.rs`, `crates/rustnative-markup/src/format.rs` tests, `crates/rustnative-conformance/tests/hygiene/`, `crates/rustnative-i18n/tests/cldr.rs` + `tests/fixtures/cldr-plurals.xml`, `crates/rustnative-interop/src/idl.rs`.

- [ ] **Step 1: Failing depth test** (markup):

```rust
#[test]
fn deep_nesting_is_an_error_not_a_stack_overflow() {
    let depth = 10_000;
    let source = format!("{}{}", "<Column>".repeat(depth), "</Column>".repeat(depth));
    let tokens: proc_macro2::TokenStream = source.parse().expect("tokenizes");
    let error = rustnative_markup::parse_markup(tokens).expect_err("too deep");
    assert!(error.to_string().contains("nested too deeply"), "{error}");
}
```

Run in a thread with a 2 MiB stack (`std::thread::Builder::new().stack_size(2 << 20)`) so the test fails by overflow today rather than passing by luck on an 8 MiB main stack.
- [ ] **Step 2: Implement** a depth counter threaded through `parse_element`/`parse_child` (`MAX_DEPTH = 256`), error with the span of the 257th opening tag. Same for the i18n parser (placeables/selectors nest) and the IDL parser.
- [ ] **Step 3: Formatter laws** — proptest: for generated markup ASTs, `format(format(x)) == format(x)` and `parse(format(x)) == parse(x)` (AST equality modulo spans); plus every `.rsx` in the repo.
- [ ] **Step 4: Hygiene fixture** — a test crate under `crates/rustnative-conformance/tests/hygiene/` (a trybuild *pass* case) that defines `struct Node; struct Option; mod std {}` and uses `rsx!`; must compile.
- [ ] **Step 5: CLDR** — vendor CLDR's `plurals.xml` (license Unicode-3.0, already allowed in `deny.toml`) for the shipped locales; test every sample `@integer`/`@decimal` against `plural.rs`. If `plural.rs` is hand-written, add a follow-up in this task: generate the rules from the vendored XML at build time.
- [ ] **Step 6: Commit** per step.

### Task 2.4: Typed errors in public signatures (U15)

**Files:** `crates/rustnative-style/src/{color.rs,value.rs}`, `crates/rustnative-sync/src/{device.rs,mqtt.rs}`, `crates/rustnative-server/src/{jobs.rs,db/schema.rs}`.

- [ ] **Step 1:** replace `Result<_, String>` with a per-module error enum implementing `Error + Display`, carrying the offending text span/offset. Breaking change → regenerate `docs/api/crates/*.txt` (Task 1.12) in the same commit; update callers.
- [ ] **Step 2:** ratchet: a `lint` step greps `docs/api/crates/*.txt` for `Result<.*, alloc::string::String>`; must be 0 after this task.
- [ ] **Step 3: Commit** one per crate.

### Task 2.6: Build-script determinism and rebuild triggers (L2.9, L2.10)

**Files:** `crates/rustnative-build/tests/determinism.rs`.

- [ ] **Step 1:** test: run each generator (icon, manifest, rc, android, styles, messages) twice into two temp dirs; compare bytes.
- [ ] **Step 2:** test: each generator's emitted `cargo:rerun-if-changed=` lines cover every input file it read (wrap file reads in the build helpers with a recorder in test builds, compare sets).
- [ ] **Step 3:** validate outputs: decode generated ICO/PNG with the `image` crate (dev-dep — check whether already in the lockfile), parse generated XML with `roxmltree` (dev-dep).
- [ ] **Step 4: Commit** `Build scripts: deterministic outputs, complete rebuild triggers, valid files`.

## Wave 3 — L3 core and L4 instruments

### Task 3.1: Reconciliation: moves, duplicate keys, complexity, interner (L3.1)

**Files:** `crates/rustnative-core/tests/property_tests.rs`, `crates/rustnative-core/benches/core_benchmarks.rs`, `budgets/core.toml` (new).

- [ ] **Step 1:** property: a keyed reorder of children yields only move operations for surviving keys (no remove+insert of a surviving key). Run; if the diff has no move operation type, this is a design gap → record and decide (moves matter for focus/IME preservation on native hosts).
- [ ] **Step 2:** duplicate sibling keys → deterministic outcome + debug diagnostic naming both (test both builds via `cfg(debug_assertions)`).
- [ ] **Step 3:** model-based oracle: naive diff (remove all, insert all) produces the same *resulting tree* as the real diff, for 1 024 generated tree pairs.
- [ ] **Step 4:** complexity: benchmark diff at 10⁴ and 10⁵ keyed children with one change; instruction count via `iai-callgrind` on Linux; `budgets/core.toml` keys `diff_1e4_instructions`, `diff_ratio_1e5_1e4 = { max = 12 }`.
- [ ] **Step 5:** interner growth test: 10 000 identical renders of a page in a server-shaped loop → `NodeId::interned_keys()` unchanged after the first.
- [ ] **Step 6: Commit** per step.

### Task 3.2: Re-entrancy fuzzer for the component tree (L3.2.4, L6a.3)

**Files:** Create `crates/rustnative-core/tests/reentrancy.rs`.

**Interfaces:** Produces `enum Op { Dispatch(u8), Mount(u8), Unmount(u8), SetProps(u8), Spawn(u8), Flush }` and a `Scripted` test component whose `update` executes a `Vec<Op>` given in its props, against the real `Application`/`ComponentTree` API.

- [ ] **Step 1:** write the component and a proptest over `Vec<Vec<Op>>` (a script per component in a small tree); the property: no panic (`std::panic::catch_unwind` around the whole run must return `Ok`), and the render log is identical across two runs (L3.0.3).
- [ ] **Step 2:** run with 10 000 cases; each failure → minimized `Vec<Op>` checked in as a named regression test, fix by queueing the re-entrant operation (the batching contract already defines when queued work runs).
- [ ] **Step 3:** effect cleanup property (L3.2.6): each effect's cleanup runs exactly once (count via `Rc<Cell<u32>>`), including when its component panics.
- [ ] **Step 4: Commit** per fix.

### Task 3.3: Layout properties with an incremental-vs-full oracle (L3.3)

**Files:** `crates/rustnative-core/tests/property_tests.rs` (layout section).

- [ ] **Step 1:** monotonicity (wider ⇒ wrapped text height non-increasing), idempotence, RTL mirror equality, incremental = full (apply a random edit, relayout incrementally, compare with a fresh layout of the edited tree).
- [ ] **Step 2:** saturation: sizes `0`, `u32::MAX`, `i32::MAX` in every constraint; depth 10⁴ → documented limit error or iterative success, in a 2 MiB-stack thread.
- [ ] **Step 3:** cost: 10³ vs 10⁴ nodes instruction ratio ≤ 12 (budget key).
- [ ] **Step 4:** check `docs/` for a CSS-flex semantics claim; if claimed, add a `taffy` differential for the flex subset (dev-dep).
- [ ] **Step 5: Commit** per property.

### Task 3.4: Unforgeable UI-thread proof (L3.4.2, Review Focus 5)

**Files:** `crates/rustnative-core/src/affinity.rs`, callers in each backend (`grep -rn "UiThread::claim" crates`).

- [ ] **Step 1: Decide** — option A: `UiThread::claim` becomes `pub(crate)`, and `Application` hands out the proof (`Application::ui_thread(&self) -> &UiThread`, claimed at construction on the constructing thread). `Application` is `!Send`, so the proof can then only ever be observed on its home thread. Option B: a global `OnceLock<ThreadId>` per process (breaks multi-application tests — reject). **Recommend A**; backends construct and run the application on the thread that runs their message loop. List every caller first: `grep -rn "UiThread::claim" crates examples`.
- [ ] **Step 2: Failing tests (for design A)** — a trybuild compile-fail case `crates/rustnative-conformance/tests/ui/claim_is_private.rs`:

```rust
fn main() {
    let _proof = rustnative_core::UiThread::claim(); //~ ERROR associated function `claim` is private
}
```

and a runtime test in `crates/rustnative-core/tests/affinity.rs`:

```rust
#[test]
fn the_application_issues_the_proof_for_the_thread_it_was_built_on() {
    let application = rustnative_core::Application::new(
        Probe::new(()),
        rustnative_core::Window::new("affinity", rustnative_core::Size::new(10, 10)),
    );
    assert_eq!(application.ui_thread().id(), std::thread::current().id());
}
```

(`Probe` is the minimal test component already used in that file, or the `Greeter` from `crates/rustnative-core/src/lib.rs:21-52`.) Run both: compile-fail passes today only if `claim` is already private (it is not), runtime test fails to compile (`ui_thread` missing).
- [ ] **Step 3:** implement A; deprecate the public `claim` for one release (`#[deprecated]`), then remove (public API snapshot shows it).
- [ ] **Step 4: Commit** `Affinity: the UI-thread proof is issued by the application, once`.

### Task 3.5: Scheduler leaks, timer order, fairness, loom (L3.4)

**Files:** `crates/rustnative-core/src/scheduler/{executor.rs,host.rs,mod.rs}`, `crates/rustnative-core/tests/local_tasks.rs`.

- [ ] **Step 1:** audit `scheduler/` for atomics/orderings and custom wakers: `grep -n "Ordering::\|AtomicU\|AtomicBool\|RawWaker" crates/rustnative-core/src/scheduler`. For each non-`SeqCst` ordering or `RawWaker`, write a loom test (`[dev-dependencies] loom`, `#[cfg(loom)]` paths).
- [ ] **Step 2:** debug counter of live tasks; test: after all scopes close, 0.
- [ ] **Step 3:** timer properties with `ManualClock`: random deadlines incl. equal and past → fire in (deadline, insertion) order, exactly once.
- [ ] **Step 4:** fairness: a task that never yields cannot block a timer beyond the cooperative budget (if no budget exists, record as a design decision: document "a task that never awaits blocks the UI thread" and add a debug watchdog warning after N ms — M54 responsiveness may already have one: check `component/tree/responsiveness.rs`).
- [ ] **Step 5: Commit** per step.

### Task 3.6: One channel per host trait (L3.5.2)

**Files:** `crates/rustnative-core/src/environment.rs` (expose `keys::ALL` if absent), each backend's `host_traits.rs` (tests only in this task).

- [ ] **Step 1:** test in core: every key in `keys::ALL` has exactly one documented feeder method on `Application` or is a pure environment key; the reduced-motion double channel (review finding 6) stays unified (regression test exists? if not, add: `set_motion_preference` updates `keys::REDUCED_MOTION`).
- [ ] **Step 2:** per backend, a test listing the keys its `host_traits` feeds and asserting each is actually set on a real window (headless for headless; the Windows/Linux/Android/TUI integration harnesses).
- [ ] **Step 3: Commit** `Environment: every host trait on one channel, fed by every backend that claims it`.

### Task 3.7: Frame clock on the model; reduced motion; IME (L3.6)

**Files:** `crates/rustnative-headless/src/app.rs`, `crates/rustnative-conformance/src/suites.rs`.

- [ ] **Step 1:** headless frame clock driven by `ManualClock`; conformance suite `animation_frames_render_nothing` moves from Windows-only to the suite (fills the `—` in `docs/guarantees.md`).
- [ ] **Step 2:** suite case: reduced motion → every running animation reaches its end state in ≤ 1 frame.
- [ ] **Step 3:** property: IME sequences interleaved with focus loss leave the text equal to the committed text.
- [ ] **Step 4: Commit** per step.

### Task 3.8: Every example is an accessibility test (L3.7, L8.1)

**Files:** Create `crates/rustnative-conformance/tests/a11y_lint.rs`.

- [ ] **Step 1:** for each example with a portable root component (`reference-app`, `gallery`, `filter-demo`, `data-demo`, `i18n-demo`, `kits`, `notes-shared`, `collab-notes`), mount in headless, walk the accessibility tree: every focusable/interactive node has a role and non-empty accessible name; every image has a name or is marked decorative; live regions present where `aria-live` equivalents are declared.
- [ ] **Step 2:** for `rustnative-components`, mount each component in each state (disabled, error, loading, empty, focused) and run the same lint.
- [ ] **Step 3:** live-region rate limit test (L3.7.4).
- [ ] **Step 4: Commit** `Accessibility lint over every example and every component state`.

### Task 3.9: Persistence: versions and crash consistency (L3.8)

**Files:** `crates/rustnative-core/src/persistence/*`, the core `FileStateStore` engine, `crates/rustnative-core/tests/state_resilience.rs`.

- [ ] **Step 1: Truncation harness** — write a record, then for every byte offset `k` in `0..len` simulate a crash by writing only `k` bytes of the *new* record's temp file and not renaming; and separately, renaming a truncated temp (to model a non-atomic filesystem) — assert load returns old or new, never error/corrupt. If the engine writes in place, this test fails → implement temp + `fsync` + rename (+ directory fsync on Unix; `MoveFileExW(..., MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)` on Windows).
- [ ] **Step 2:** version field + fixture (Task 1.9 convention).
- [ ] **Step 3: Commit** per step.

### Task 3.10: Service contract suites; `Application` typestate (L3.9)

**Files:** Create `crates/rustnative-conformance/src/services.rs` (a `ServiceHost` trait + suites), `crates/rustnative-conformance/tests/services_fake.rs`; modify `crates/rustnative-core/src/application.rs` (typestate for `take()`).

- [ ] **Step 1:** suites per service contract in `rustnative-core/src/services/mod.rs` (HTTP, storage, clipboard, secure storage, notifications, share, work, documents): same inputs → same observable result or `Unsupported`. Run against the core fakes first.
- [ ] **Step 2:** each backend runs the suites against its real services in wave 6 (Task 6.2).
- [ ] **Step 3:** `Application::take()` — replace the panicking husk with a consuming API (`fn into_parts(self) -> (…)`) or a typestate (`Application<Running>` / `Application<Taken>`) per `docs/conformance/typestate.md`; Android is the caller (`grep -rn "\.take()" crates/rustnative-android/src`).
- [ ] **Step 4: Commit** per step.

### Task 3.11: Measuring the instruments: headless gaps and mutant hosts (L4)

**Files:** `crates/rustnative-headless/src/{app.rs,platform.rs,services.rs}`, `crates/rustnative-conformance/tests/mutant_hosts.rs`.

- [ ] **Step 1:** headless drives `Control::MultilineText` (`type_text`/`set_text` accept it); headless records `SurfaceCommand` and `InputRequest` with query methods (`todo.md` "Remaining").
- [ ] **Step 2:** for each suite fn in `src/suites.rs`, a mutant `ConformanceHost` wrapper that breaks exactly the property the suite checks (e.g. renders everything on every event; never cancels tasks; leaks one object per mount) — the suite must fail against it:

```rust
#[test]
fn every_suite_fails_against_the_host_that_breaks_it() {
    for (suite_name, run_suite, mutant) in rustnative_conformance::mutants::ALL {
        let result = std::panic::catch_unwind(|| run_suite(&mut mutant()));
        assert!(result.is_err(), "suite {suite_name} passed against a host that breaks it: it asserts nothing");
    }
}
```

- [ ] **Step 3:** test that synthetic input through hit-testing and direct dispatch produce identical render logs (L4.2).
- [ ] **Step 4:** web-testing: hard timeout and browser-version reporting (L4.5).
- [ ] **Step 5: Commit** per step.

### Task 3.12: Inspection protocol round-trip, wire fuzzing, crash-hook chaining (L3.10)

**Files:** `crates/rustnative-core/tests/inspection.rs`, fuzz targets `inspect_compact` and a new `wire_decode`.

- [ ] **Step 1:** proptest round-trip over generated protocol messages (derive `proptest::arbitrary::Arbitrary` in a test-only helper module, not on the public types).
- [ ] **Step 2:** remote inspection requires a token; test the refusal.
- [ ] **Step 3:** child-process test: a worker-thread panic produces exactly one crash report and the previously installed hook still runs (marker file written by the previous hook).
- [ ] **Step 4: Commit** per step.

## Wave 4 — L5 shared platform groups

### Task 4.1: Draw-list damage oracle (L5a)

**Files:** `crates/rustnative-drawlist/tests/damage_oracle.rs`.
- [ ] proptest: random sequences of draw-list updates; grid after incremental (damage-only) repaint == grid after full repaint. Fuzz target `drawlist_raster` (serialized draw list → raster). Convert the 13 `#[allow]`s to `#[expect(reason)]`. Commit per item.

### Task 4.2: Update channel: rollback, rotation, size, SemVer oracle (L5b)

**Files:** `crates/rustnative-update/src/lib.rs`, `crates/rustnative-update/tests/{refusals.rs,semver_oracle.rs,bucketing.rs}`; format change ⇒ new fixture version (Task 1.9).

- [ ] **Step 1: Failing tests** — `a_signed_older_version_is_refused` (installed 1.2.0, manifest 1.1.0 validly signed → refused with `UpdateError::Downgrade`); `a_manifest_signed_by_a_retired_key_is_refused_after_its_retirement`; `a_download_longer_than_the_signed_size_is_stopped`.
- [ ] **Step 2: Implement** — decision API takes the installed version; trusted keys become a list with `not_after`; manifest gains `size: u64` (signed); the downloader in each host (`rustnative-windows/src/update.rs`, `rustnative-android/src/update.rs`) stops at `size` and verifies SHA-256. New manifest field = format version bump; old-format fixture keeps loading (field optional for old versions? **No** — a size-less manifest from a new publisher must be refused; old installed clients reading new manifests ignore the field by design, `src/lib.rs:15-18`).
- [ ] **Step 3:** SemVer property test vs `semver` crate (dev-dep); bucketing χ² test; refusal test per error variant; mutants ≥ 90 % (Task 1.8 gate).
- [ ] **Step 4:** document the TUF comparison and the remaining gap (threshold signatures) in `docs/deploy/update-rules.md`.
- [ ] **Step 5: Commit** per step.

### Task 4.3: Host selection on unsupported targets (L5c)

**Files:** `crates/rustnative-host/src/lib.rs`, `.github/workflows/ci.yml` (`apple` job).
- [ ] Determine how `rustnative-host` currently compiles on macOS/iOS (the `apple` job checks it). Decide: compile error naming the target **or** an explicit headless/terminal fallback that is documented and returns a typed error at `run`. Recommend: `compile_error!` for GUI targets without a backend, unless the app opts into `features = ["headless-fallback"]`. Test with `cargo check --target aarch64-apple-darwin` expecting failure (a `trybuild`-like script step). `services()` kind-set test (L5c.2). Commit.

### Task 4.4: Interop ABI: generated code compiles strict, handles are generation-checked, panics stay inside (L5d)

**Files:** `crates/rustnative-interop/tests/{kitchen_sink.ril,generated_c.rs,generated_cs.rs,abi_misuse.c}`, `crates/rustnative-interop/src/runtime.rs`.
- [ ] Kitchen-sink IDL; compile generated C with gcc/clang (`-Wall -Wextra -Werror -std=c11 -fsanitize=address,undefined`) in the Linux CI job and MSVC `/W4 /WX` on Windows; C# with `dotnet build -warnaserror` where `dotnet` exists (skip-with-message otherwise).
- [ ] A C program performing double-destroy / use-after-destroy / wrong-thread calls receives error codes; ASan clean.
- [ ] Verify the third `extern` in `runtime.rs` is unwind-guarded (2 `catch_unwind` for 3 `extern`s).
- [ ] Snapshot the generated C header (`tests/snapshots/kitchen_sink.h`), diffed.
- [ ] Commit per step.

## Wave 5 — L7 Web

### Task 5.1: One escaper, typed URL sinks, an XSS oracle (L7.1)

**Files:** `crates/rustnative-web/src/html.rs`, `crates/rustnative-web/src/dom.rs:771-776`, `crates/rustnative-server/src/response.rs:144` (delete, use the web one — server already depends on `rustnative-web`), `crates/rustnative-web/tests/xss_oracle.rs`, `crates/rustnative-web/Cargo.toml` (`[dev-dependencies] html5ever`/`scraper`).

- [ ] **Step 1: Failing test** — a `Link` whose address is `javascript:alert(1)` renders `href="#"` (or no link) and a debug diagnostic:

```rust
#[test]
fn a_javascript_address_never_becomes_a_link() {
    let html = render_link_with_address("javascript:alert(1)"); // helper building a page with one Control::Link
    assert!(!html.contains("javascript:"), "{html}");
}
```

- [ ] **Step 2:** `SafeUrl::parse(&str) -> Option<SafeUrl>` allowing `http`, `https`, `mailto`, `tel`, and scheme-less relative URLs (reject leading whitespace/control characters before the scheme — browsers strip them); `marks.links` stores `SafeUrl`.
- [ ] **Step 3:** delete the server's duplicate `escape_into`; route through `rustnative_web::html::escape_into`.
- [ ] **Step 4:** XSS oracle proptest: arbitrary strings (incl. `<`, `"`, `'`, backticks, `</script>`, `]]>`, U+2028, NUL) into every text/attribute-bearing node kind; parse output with html5ever; assert the element/attribute set equals the one rendered from a placeholder string. Fuzz target `web_render` with the same oracle.
- [ ] **Step 5:** test: every rendered page carries the strict nonce CSP header (no `unsafe-inline`, no `unsafe-eval`); grep gate: no `eval(`/`new Function(` in `rn.js`, `sw.js`, webgen output.
- [ ] **Step 6: Commit** per step.

### Task 5.2: Client-subset differential at scale (L7.3, L7.4)

**Files:** `crates/rustnative-web/tests/client_subset_generated.rs`, `crates/rustnative-webgen/tests/refusals.rs`.
- [ ] proptest generator of subset programs (integer ops at `i32`/`i64` boundaries incl. > 2⁵³, division/remainder by zero and of negatives, string length/indexing with non-ASCII, nested control flow); run each in Rust and as generated JS in the headless browser; outputs (incl. failure kind) equal. 10 000 cases nightly, 256 per push.
- [ ] refusal test per unhandled `syn::Expr`/`syn::Stmt`/`syn::Pat` variant (enumerate).
- [ ] Commit per step.

### Task 5.3: `rn.js` type-checked; Firefox and Safari in CI (L7.5, L7.6)

**Files:** `crates/rustnative-web/src/runtime/rn.js` (JSDoc annotations), `crates/rustnative-web/src/runtime/jsconfig.json`, `crates/rustnative-web-testing/src/lib.rs` (WebDriver BiDi for Firefox), `.github/workflows/ci.yml`.
- [ ] `npx -y typescript@5 tsc -p crates/rustnative-web/src/runtime --noEmit` with `checkJs`, `strict`; annotate until 0 errors (one commit per section of the file); CI step in `lint` (Node is preinstalled on runners).
- [ ] Firefox leg on ubuntu (`geckodriver`/BiDi); Safari leg on the `apple` job (`safaridriver --enable` requires a one-time `sudo`; available on GitHub macOS runners). Non-blocking → blocking after two green weeks.
- [ ] Budget key `runtime_js_kb` in `budgets/web.toml` if absent.
- [ ] Commit per step.

### Task 5.4: Web checklist rows (L7.8; continues `RESUME.md` item 2)

**Files:** `crates/rustnative-web/tests/*`, `crates/rustnative-server/tests/*_browser.rs`, `docs/conformance/new-backend-checklist.md`.
- [ ] One test per `—` row (2, 4, 6, 8–16, 19, 22–26 minus M67's 23–25): safe areas, `touch-action` gesture arbitration, ownership, escape hatch, panic/teardown, typed environment, commands, adaptive layout, mappers, surfaces, group decision, cursors, runtime token switching, inspection protocol in `rn.js`. Fill each cell in the same commit as its test. The claims checker (Task 1.10) then enforces it.

## Wave 6 — L6 native backends

### Task 6.1: One guarded entry for every host callback (L6.0.4, U3b, Review Focus 2)

**Files:** Create `crates/rustnative-core/src/ffi_guard.rs` (portable: `pub fn guarded<R>(name: &'static str, fallback: R, f: impl FnOnce() -> R + UnwindSafe) -> R` — catches the unwind, routes the payload to the panic policy (`panic.rs`) and crash capture, returns `fallback`); each backend's callback sites (51 `extern` fns: list with `grep -rn 'extern "\(C\|system\)" fn' crates/*/src`); create `crates/rustnative-conformance/tests/ffi_entries.rs`.

- [ ] **Step 1: Source test (failing)** — for each `extern "C"|"system" fn` in non-test sources of windows/linux/android/tui/web/interop, assert the body's first statement calls `guarded(` (or the crate's existing equivalent — Android `jni_host/mod.rs` and Windows `message_loop.rs` already have `catch_unwind` helpers: reuse those names, list them in the test).
- [ ] **Step 2:** wrap every unguarded callback; window procedures return `DefWindowProcW(...)` as fallback; JNI natives return the type's zero and leave a pending Java exception describing the panic; signal handlers must stay async-signal-safe — **do not** wrap them in `catch_unwind` (allocation); instead assert they contain no panicking calls (they should only set an atomic and write restore bytes — `rustnative-tui/src/guard.rs` model) and list them as an allow-listed exception in the test with that reason.
- [ ] **Step 3:** a test per backend triggering a panic inside a callback (e.g. a component panicking during a `WM_PAINT`-driven render) and asserting the process survives per policy and one crash report exists.
- [ ] **Step 4: Commit** one per backend.

### Task 6.2: Capability truth, service suites, and style readback per backend (L6.0.2, L3.9.2, L1.2)

**Files:** per backend: `capabilities.rs`/`platform.rs` tests; run `rustnative_conformance::services` suites; capability-table readback test modelled on `native::style_integration::the_windows_capability_table_is_what_the_backend_applies` for Linux, Android, Web, TUI.
- [ ] For each backend: `realized()` vs a `capability → test` map in the test file (both directions). Fix under/over-advertising.
- [ ] Commit per backend.

### Task 6.3: Native-object baselines beyond GDI (L6.0.3)

**Files:** `crates/rustnative-windows/src/native/guarantees_integration.rs` (add `GetProcessHandleCount`), `crates/rustnative-linux/src/gtk/guarantees.rs` (GObject instance counts via `GOBJECT_DEBUG=instance-count` + fd count), `crates/rustnative-android/src/device_tests/conformance.rs` (JNI global-ref count via CheckJNI dump, View count), `crates/rustnative-tui/tests` (fd count, termios restored).
- [ ] 100 mount/unmount cycles, warm-up 10, delta 0. Commit per backend.

### Task 6.4: Windows specifics (L6a)

- [ ] Application Verifier baseline (Task 1.14 step 5) → fix each report.
- [ ] Re-entrancy stress test (nested `SendMessage` during dispatch) — the Windows instance of Task 3.2.
- [ ] DPI matrix test at 100–250 % via per-thread DPI awareness simulation; mixed-DPI move owed to the reference machine (record).
- [ ] COM init/uninit pairing test per thread that initializes COM.
- [ ] Visual-golden metric documented (ΔE + % pixels) in `tests/goldens/README.md`.
- [ ] `unsafe` ratchet reduction: identify the top 5 files by `unsafe` count and move raw calls behind safe wrappers in the ownership module; lower the ratchet.
- [ ] Commit per item.

### Task 6.5: Linux specifics (L6b)

- [ ] Main-context debug assertion in the ownership module(s).
- [ ] Wayland leg (`weston --backend=headless-backend.so` + `WAYLAND_DISPLAY`) in `linux-backend` CI job.
- [ ] Real `xdg-desktop-portal` leg (one job) beside the fake bus.
- [ ] Secure storage without a keyring: `Unavailable` within 5 s (test with no `gnome-keyring` running).
- [ ] Single-instance peer check (same UID) — test with a socket from another UID if the CI container allows (`sudo -u nobody`), else record as owed.
- [ ] Commit per item.

### Task 6.6: Android specifics (L6c)

- [ ] CheckJNI on for the emulator device suite (`adb shell setprop debug.checkjni 1` before launch in `tools/android-device-test.sh`); fix every abort.
- [ ] Local-reference frames around every loop that creates local refs (audit `jni_host/` and services); test with a 10 000-item list realization under CheckJNI.
- [ ] minSdk emulator leg in CI; physical device run recorded as owed until done.
- [ ] Verify capability advertising (review finding 5) via Task 6.2.
- [ ] Commit per item.

### Task 6.7: Terminal specifics (L6d)

- [ ] Split-invariance fuzz (Task 1.4 target) → fix.
- [ ] Width table staleness (regenerate and diff) and `unicode-width` differential with documented exceptions.
- [ ] Emulator differential against `vte` (dev-dep) for every sequence the encoder emits.
- [ ] Stalled-pty test: output buffer ≤ 2 frames.
- [ ] Commit per item.

## Wave 7 — L8 application layer

### Task 7.1: Model-based tests for the query cache and the offline queue (L8.4, L8.5, L8.11)

**Files:** `crates/rustnative-data/tests/{query_model.rs,offline_model.rs}`.
- [ ] Reference model: a `BTreeMap<Key, (value, fetched_at, observers)>` with the documented rules; random op sequences (fetch, complete(ok/err), invalidate(prefix), observe, unobserve, advance clock) applied to both; assert monotonic freshness, ≤ 1 in-flight per key, retention collection, cancelled observers never render.
- [ ] Offline queue: crash at every point between persist/send/ack (truncation harness from Task 3.9) → no loss, idempotency key reused, server-side dedup test in `rustnative-server`.
- [ ] Commit per step.

### Task 7.2: Migrations matrix, state-machine validation, forms parity (L8.6–L8.8)

- [ ] Fixture per schema version; apply all migrations from each; compare final schema.
- [ ] `machine.rs`: construction reports unreachable states/dead transitions (failing test first).
- [ ] Forms: one rule definition evaluated client (generated JS) and server (Rust) on generated inputs — differential (reuses Task 5.2's harness).
- [ ] Commit per step.

## Wave 8 — L9 server and distributed

### Task 8.1: Secure defaults as one asserted set; review findings as named regressions (L9a.1, L9a.2, L9a.7)

**Files:** `crates/rustnative-server/tests/secure_defaults.rs`.
- [ ] One test: default app response headers and cookie attributes equal the documented set (exact list).
- [ ] One named test per review finding 13–16 + P2 server items (`finding_13_cache_never_replays_a_nonce`, …) — some exist under other names: find them (`grep -rn "nonce\|no-store\|forwarded\|challenge" crates/rustnative-server/tests`) and rename or reference them in a comment table at the top of the file.
- [ ] Commit.

### Task 8.2: Server parsers fuzzed; timeouts; crypto parameters; constant time (L9a.3, L9a.4)

- [ ] Fuzz targets: form body, cookies, multipart (if present), query string, `X-Forwarded-For` parsing, `traceparent`, passkey CBOR/COSE (`auth/passkey.rs`), session cookie decode.
- [ ] Per-request read timeout test (slow body).
- [ ] Test asserting argon2 params ≥ m=19456 KiB, t=2, p=1.
- [ ] Grep gate: `==` on token/CSRF/MAC byte slices → replace with constant-time comparison (`subtle` is likely already in the tree via `aes-gcm`; verify in `Cargo.lock`, else implement a 5-line constant-time eq).
- [ ] `Secret<T>` newtype (no `Debug`/`Display`, zeroize on drop) for keys and tokens if none exists.
- [ ] Commit per step.

### Task 8.3: Deploy exports run; ZAP baseline (L9a.6, L9a.9)

**Files:** `.github/workflows/ci.yml` (job `deploy-exports`, ubuntu with Docker).
- [ ] Export `examples/server-demo` to Compose; `docker compose up -d`; `curl /healthz`; write via a form; `docker compose restart`; read back; `kubeconform` the Kubernetes export; `systemd-analyze verify` the unit.
- [ ] `zaproxy/zap-stable` baseline scan against the running Compose app; fail on medium+.
- [ ] Commit.

### Task 8.4: Sync convergence under perturbation; HLC; Live; MQTT (L9b)

- [ ] proptest: 3–5 replicas, random ops, random delivery schedule with reorder/duplicate/delay → identical final states.
- [ ] HLC property with an injected clock that jumps backwards.
- [ ] Live: `max_sessions` refusal test; per-session memory bound in the soak (`live` scenario); `WireEvent` fixture + fuzz.
- [ ] MQTT fuzz; varint length ≤ 4; packet size limit test.
- [ ] Commit per step.

### Task 8.5: Durable replay from every crash point; log versioning; supervision limits; actor mailboxes (L9c)

- [ ] Small workflow op language + generator; crash after every step; replay; same result and side-effect sequence.
- [ ] Log fixture per version; compaction bound in the `durable` soak scenario.
- [ ] Crash-loop escalation test; bounded mailbox overflow-policy test.
- [ ] Commit per step.

### Task 8.6: Observe: consent per exporter, PII allow-list, overhead (L9d)

- [ ] Fake transport that panics if called, per exporter, consent off.
- [ ] Default span attributes contain no UI text / form values (test with a page containing a marker string).
- [ ] Instruction-count overhead ≤ 1 % with tracing disabled (budget key).
- [ ] Commit per step.

## Wave 9 — L10 tooling and L11 release

### Task 9.1: Budgets measured where they apply (U10, L10.10; continues `RESUME.md` item 1)

**Files:** `crates/rustnative/src/bench.rs`, `budgets/*.toml`.
- [ ] Linux memory: read `VmHWM` from `/proc/<pid>/status` for the child (find by `/proc/*/comm`, truncated to 15 chars, e.g. `web-notes-lambd`) while alive.
- [ ] `MISS` (unmeasured key not `optional`) fails `--check`.
- [ ] Shipping-shaped app for `artifact_size_kb` (a release build of `examples/reference-app`, stripped) instead of the bench harness.
- [ ] Commit per step.

### Task 9.2: Packages validated by the platforms' own tools; reproducible; writers round-trip (L10.1–L10.3)

- [ ] CI: `lintian` (deb), `rpmlint` (rpm), `namcap` (pacman, arch container), `appstreamcli validate`, `apksigner verify` + `aapt2 dump badging` (android job), `signtool verify /pa` + `makeappx unpack` (windows job).
- [ ] Two builds → identical bytes per format (`SOURCE_DATE_EPOCH`, sorted entries, fixed modes/uids).
- [ ] proptest round-trip of zip/rpm/pacman writers vs independent readers (`zip` crate, `rpm` crate or `rpm2cpio | cpio -t`, `bsdtar -tvf`).
- [ ] Commit per step.

### Task 9.3: CLI input hardening; LSP fuzzing; emulator contract fixtures (L10.4, L10.7, L10.8, L10.11)

- [ ] `rustnative create ../../x` refused (test); temp+rename for every artifact write; Ctrl+C test (spawn, kill mid-package, assert no file at final path).
- [ ] Fuzz `config.rs` parsing; fuzz LSP handlers with generated documents; p95 latency test on a 5 000-line `.rsx`.
- [ ] Lambda/WAGI emulator request/response fixtures from the providers' published examples.
- [ ] Commit per step.

### Task 9.4: Release pipeline dry-run on every push; security policy (L11.5–L11.7)

- [ ] `cargo publish --dry-run` per crate in dependency order (script derives order from `cargo metadata`); `cargo package --list` reviewed for stray files (a test asserting no `target/`, `*.log`, `*.stackdump` in any package list; `*.stackdump` is already gitignored).
- [ ] `SECURITY.md`: GitHub private vulnerability reporting as the channel (user enables it in repository settings — **user action**).
- [ ] Commit per step.

---

# Part E — The second pass (Q10: ask again from the beginning)

Pass one was written layer by layer. Pass two re-asked Q1–Q10 across layers, looking for indicators that fall *between* layers or that the per-layer framing hid. Each gap found is added here with its owner; the tasks above are amended by reference (the owning task gains the step).

## E.1 Gaps found by re-asking Q1/Q2 (missing indicators)

| # | Gap | Why it matters (Q3) | Bound (Q4) | Owner |
|---|---|---|---|---|
| E1 | **Events to targets that cannot receive them.** A backend can deliver a click for a node that is disabled, hidden, or already unmounted (race between host and tree). Today each backend filters (or not) on its own. | One host race → `update` runs for a disabled button; on the Web/Live path a *malicious client* sends exactly this on purpose (E3). | The core refuses events whose target is absent, disabled, or hidden — in `Application::dispatch`, one place — with a debug diagnostic; a conformance case per backend. | Task 3.2 (new step: property "no event reaches `update` for an absent/disabled target") |
| E2 | **`panic = "abort"` in the user's profile.** Error boundaries and FFI guards rely on unwinding; under `abort` they cannot catch. | Users set `panic = "abort"` for size; the documented subtree-isolation guarantee silently becomes "whole process dies". | Behaviour under `abort` documented in `docs/guarantees.md`; one CI build of `reference-app` with `panic = "abort"` proving crash capture still writes a report (it runs in the panic hook, before abort). | Task 3.12 (new step) |
| E3 | **Live/server-interactive input is untrusted input.** `rustnative-sync/src/live.rs` `WireEvent`s come from a browser the server does not control: events for nodes never rendered, disabled, or in another session; malformed text lengths. | Server-side state change bypassing UI rules — the Live equivalent of a missing authorization check. | Every `WireEvent` validated against the session's current rendered tree (E1's core check covers part); size limits per field; fuzz target `live_wire`. | Task 8.4 (new step) |
| E4 | **`#[server]` functions are public HTTP endpoints.** Generated by `rustnative-web-macros`; each is callable by anyone, not only by the island that calls it. | Authorization and CSRF must be per function; an unauthenticated server function is an open API. | A test enumerating every `#[server]` fn in examples asserting it goes through CSRF and the app's auth extractor; argument deserialization fuzzed. | Task 8.1 (new step) |
| E5 | **Developer-tool network exposure.** Dev server (`crates/rustnative/src/dev.rs`), inspector transport, LSP: if bound to `0.0.0.0` or without an `Origin`/`Host` check, a web page the developer visits can drive them (DNS rebinding). | Code execution or source disclosure on the developer's machine. | Loopback-only by default; reload/inspector WebSocket checks `Host` ∈ {localhost, 127.0.0.1, [::1]} and `Origin`; token for non-loopback. Test each. | Task 9.3 (new step) |
| E6 | **SQLite under more than one process.** Deploy exports used `replicas: 2` with SQLite (review finding 17, fixed in exports); jobs, durable, and actors share the file. | `SQLITE_BUSY` errors under load or divergent replicas. | WAL mode + `busy_timeout` asserted at open; a test with two processes writing the same DB for 10 s: 0 errors, 0 lost writes. | Task 8.3 (new step) |
| E7 | **Time: DST, leap days, time zones in schedules.** `rustnative-server/src/jobs.rs:103` parses schedule expressions; durable timers; data retention. | A cron job that runs twice or never on DST change; retention off by an hour. | Property tests over schedule expressions across DST transitions in ≥ 3 zones (fixed tz database fixture); "runs exactly once per scheduled instant". | Task 8.5 (new step) |
| E8 | **Vendored data provenance.** CLDR plurals (Task 2.3), Unicode 17 width data (`rustnative-tools/unicode-17` → `rustnative-tui/src/width/table.rs`), subset fonts (`rustnative-web/src/fonts.rs`). cargo-deny sees crates, not data files. | License compliance and staleness (a Unicode update changes widths). | `docs/vendored-data.md`: file → source URL → version → license → regeneration command; a test that regenerating produces the committed file (staleness). | Task 1.9 (new step; same inventory style as formats) |
| E9 | **Per-node memory cost.** App-level `resident_memory_mb` exists; a regression of +200 bytes per `Node` is invisible until a 10⁵-row screen. | Memory budgets on Android/embedded. | `size_of::<Node>()`, `size_of::<Event>()` asserted (exact, updated deliberately); bytes/node for a 10⁴-node tree as a budget key. | Task 3.1 (new step) |
| E10 | **Macro expansion time.** `rsx!` runs in every user build. | Compile-time regressions are user-visible on every save (dev loop budget M43). | Expansion of a 2 000-element `rsx!` ≤ budget (`budgets/core.toml` `rsx_expand_2000_ms`), measured by `cargo build -Z timings`-free method: a test timing `parse_markup` + lowering in-process. | Task 2.3 (new step) |
| E11 | **Claimed target triples.** U1 checks crates × features but the list of *targets* is implicit in CI jobs. Windows on ARM64 and Linux aarch64 are common user targets. | A claimed-but-unchecked target breaks on first use. | `docs/status.toml` lists every claimed target triple; a CI step runs `cargo check --target <t>` for each (with `rustup target add`); unclaimed targets stated as unsupported. | Task 1.1 (new step) |
| E12 | **Instruments that cannot fail.** Every gate added in wave 1 is itself code; a gate that silently always passes (wrong path, regex never matching after a refactor) is worse than none because it certifies. | False confidence: the class of failure L4.6 addresses for suites, applied to every gate. | **Gate liveness:** each gate has a canary — a committed known-bad input it must reject (the ratchet self-test, a fake doc for the claims checker with a missing fn, a fuzz target with a planted crash under `cfg(fuzzing_canary)`, a mutant host per suite). A weekly job runs all canaries and fails if any gate *passes* its canary. | New **U19** below; Task 1.2/1.10/1.11 (new step each) |
| E13 | **CI cost bound.** Wave 1 adds nightly Miri, sanitizers, 11+ fuzz targets × 1 h, ×20 flake runs, weekly soak and mutants. | Runner minutes are finite; an over-budget nightly gets disabled, which silently removes E4 enforcement. | Nightly wall time ≤ 6 h total across jobs; weekly ≤ 12 h; tracked in `docs/quality/ci-budget.md`; when exceeded, reduce per-target fuzz time before removing any job. | Task 1.15 (new step) |
| E14 | **Gate removal policy.** Pass one defines how gates are added, not how a noisy one is handled. | Without a rule, a flaky gate gets deleted in frustration. | A gate is never deleted to unblock: it is quarantined (Task 1.6 file) with owner + expiry, like a test. | Task 1.6 (new step) |

**New universal indicator U19 — Gate liveness.** *Why:* E12. *Bound:* 100 % of gates have a canary; the weekly canary job is green (meaning every gate rejected its canary). *Instrument:* `tools/ratchets/canaries/` + weekly job. *Grade/rung:* M4/E4. *Why this method:* it is the only way to know an E4 gate is still E4 after the code around it moves.

## E.2 Re-asking Q3/Q4 (are the reasons specific and the bounds real?)

- **Bounds that were judgment calls, now justified or flagged:**
  - 80 % changed-line coverage, 70 %/90 % mutation scores, 1 024 proptest cases: industry-typical starting points, **not** derived from data. Each gets a review date (three months after its gate turns blocking) at which the bound is moved to the observed p10 of passing changes if that is higher. Recorded in `docs/quality/bounds-review.md`.
  - Soak slope 1 %/h over 6 h: chosen so that a 100 MB process growing 1 MB/h passes (no measurable leak in a day) but 10 MB/h fails. Revisit when the first real soak data exists.
  - Depth limit 256 for markup, 128 for other recursive parsers: the deepest `.rsx` in the repo should be measured (Task 2.3 step 1 prints it); the limit must be ≥ 4× that.
- **Reasons that were generic in pass one and are now specific:** U9's lock ordering — pass one said "deadlocks are found in production"; the specific risk is core's 28 `Mutex<|RwLock<` combined with callbacks into user code while held (a user `update` dispatching while the scheduler holds its queue lock). Task 3.5 gains a step: **no user callback is invoked while any core lock is held** — enforced by a debug lock-depth counter asserted at every user-callback entry.

## E.3 Re-asking Q5 (is each measurement reliable enough, or is there a better one?)

| Indicator | Pass-one instrument | Weakness found | Better/added instrument |
|---|---|---|---|
| U3 panics | grep counts | Counts `unwrap` in strings/comments; misses panics in macros and deps | Keep grep as the ratchet (cheap, stable); **rely on fuzz/proptest for the real search**; for pure decoders, `#[no_panic]` attribute (crate `no-panic`) on the decode fn in a release-mode test build — a link-time proof where it applies (Task 1.4 addition for `inspect::compact::decode`, `mqtt` varint) |
| U6 coverage | llvm-cov line coverage | Lines, not branches; generated code inflates | Branch coverage (`--branch`, nightly) reported alongside; mutation score remains the gate for critical code |
| U8 growth | RSS slope | Allocator may not return memory to the OS (glibc), hiding or faking growth | Add heap-level counting in soak builds: a counting `#[global_allocator]` wrapper (test-only feature) reporting live bytes — exact, allocator-independent |
| U10 perf | instruction counts | Counts miss cache/memory effects and syscalls | Keep instruction counts for regressions (low noise); keep wall-time budgets on the reference machine for absolute claims (the two answer different questions) |
| L4.1 headless fidelity | same verdict per suite | Same verdict ≠ same behaviour | Add a *trace* comparison for the reference app: render log + event log from headless vs each real backend over one scripted session, equal modulo documented host differences |
| L6.0.8 accessibility | AT API readback | Readback proves the tree, not usability | The human screen-reader pass stays owed (M1); nothing automated replaces it — stated, not hidden |
| L7.1 XSS | html5ever oracle | Parser differentials between html5ever and browsers (mXSS) | Run the same oracle in the real browser for a sample (parse via `DOMParser` in headless Edge through web-testing) nightly |

## E.4 Re-asking Q6/Q7 (pushed to the extreme? best method?)

Indicators still below the highest feasible rung after pass one, and the move:

- **U4 (ignored results) E4 → E5:** introduce `#[must_use = "…"]` on every backend's result/handle types *and* make `best_effort` the only function that accepts a `Result` and returns `()` in backend crates — then `let _ =` stands out in review as the only way around it. Already in U4; promoted to a Task 6.x step per backend (6.4–6.7 each gain "convert the remaining `let _ =` on host results").
- **L3.2.4 (re-entrancy) E4 → E5 candidate:** after Task 3.2 reports, if failures cluster around `RefCell` in `component/tree.rs`, replace with a phase-typed API (`&mut Tree<Idle>` vs `&Tree<Rendering>`) for the internal paths. Decision point recorded in Task 3.2.
- **L5b (update) E4 → E5:** `UpdateDecision::new(installed: Version, manifest: VerifiedManifest)` where `VerifiedManifest` is only constructible by the verifier — the "verify before use" order becomes unrepresentable to get wrong. Added to Task 4.2 step 2.
- **L7.1 (URLs) — already E5** via `SafeUrl`.
- **U13 (architecture) E4 → partial E5:** dependency direction for the core is enforced by Cargo itself once backends are never listed in core's `Cargo.toml`; the test exists to stop an edit, and that is the ceiling — Cargo has no "forbidden dependency" field. Accepted.
- **U7 (flakiness):** the best known method beyond repeated runs is **deterministic simulation** (all time, randomness, and scheduling injected) — the core already has `ManualClock` and a test executor; the remaining nondeterminism sources are real threads in backends and the tokio multi-thread pool. Extending determinism to the server tests (tokio `start_paused = true` for time-based server tests) is added to Task 1.6 as a step.

## E.5 Re-asking Q8 (implemented safely?)

- **Risk: wave 1 floods master with red.** Every new gate lands non-blocking, seeded at today's values (ratchets) or `continue-on-error`, and flips only after one green push. The direct-to-master workflow makes this rule mandatory, not optional.
- **Risk: format changes during hardening break users.** Every format change (Tasks 3.9, 4.2, 8.4, 8.5) goes through Task 1.9's version + fixture rule *before* the change lands.
- **Risk: public API churn from typed errors and typestate (Tasks 2.4, 3.4, 3.10).** Pre-1.0, breaking changes are allowed but must be visible: the public-API snapshot (Task 1.12) lands *before* those tasks, so each break is a reviewed diff with a changelog line.
- **Risk: tool availability on Windows.** cargo-fuzz, Miri sanitizers, cachegrind are Linux-first. All such jobs run on ubuntu runners or WSL locally; Windows gets Application Verifier instead. Documented in Task 1.14.
- **Risk: CI time.** E13's budget.

## E.6 Re-asking Q9 (can each layer go further?)

What remains between this plan's target rungs and the theoretical limit, per layer, and whether to pursue it:

| Layer | Further step | Pursue? |
|---|---|---|
| L0 | Kani proofs of all geometry ops | **Yes** (cheap; Task 2.1 optional step) |
| L1/L2 | Grammar spec + generated reference parser for differential | Only if fuzzing keeps finding parser bugs after 3 months |
| L3 | TLA+/P model of scheduler cancellation; Kani for layout arithmetic helpers | Kani **yes** for `layout/constraints.rs` helpers; TLA+ only on a bug class sampling misses |
| L4 | Headless generated from a formal host model | No |
| L5b | Full TUF (`tough`), threshold signatures | **Decide at M50 fleet work**; recorded in `docs/deploy/update-rules.md` |
| L5d | Formal ABI spec + per-language conformance suites | When library-only mode is advertised as stable |
| L6 | Screen-reader passes, multi-monitor DPI, physical devices, real GPUs | **Owed to people/hardware** (2.13) — scheduled, never assumed |
| L7 | Verified client-subset compiler | No; differential at scale is the practical limit |
| L9 | Third-party penetration test; per-deployment threat models | **Before any public release** (pre-1.0 requirement) |
| L10 | Store submission dry-runs (Microsoft Store, Play) | When accounts exist |
| L11 | Hermetic builds (Nix/Bazel) | No; reproducibility check covers the verifiable part |

## E.7 Verdict (Q10): the loop record

**Correction to the first draft.** The first draft of this section said a third pass "did not surface indicators of a new kind". That was wrong: it was a desk pass over the plan, not over the code. Q10 was meant as a loop, and the loop is now run over the code itself:

- **Coverage ledger.** [`layer-solidity/ledger.md`](layer-solidity/ledger.md) lists all 721 source and test files (205 477 lines) with a status of `unread`, `skimmed`, or `read`.
- **Per-layer evidence.** Each layer gets a findings file. A finding cites file:line, and a scratch program reproduces it where that is feasible, outside the repository (`verify` through `verify7`: L0–L3 values and core, drawlist raster, TUI console mode, Live hijack).
- **Termination.** The loop ends only when **(a)** every ledger row is `read` (or `skimmed` for tests) and **(b)** one complete pass of Q1–Q10 over all layers produces no new finding *and* no new indicator.

### Iteration 1 (2026-10-15): result

| Measure | Value |
|---|---|
| Files read in full | 223 (64 770 lines) |
| Files skimmed or targeted | 61 (30 187 lines) |
| Untouched | 437 files, about 54 % of lines (see the queue below) |
| Findings | About 190 across L0–L11, including **3 P0** (L3-F1 diff blindness ✅, L9-F1 Live hijack ✅, L9-F2 forged state ✅) and about 30 P1 |
| Reproduced by running code (✅) | 25 |
| Refuted | 1 (L2: the formatter's handling of `margin=-4`) |
| New universal indicators | 9 (U20–U28), from defect shapes that recurred across layers |
| New layer indicators | About 70 (the "Indicators added" sections of each findings file) |

**Is this enough, or must the questions be asked again?** Both termination conditions fail:
- (a) fails: about 54 % of lines are unread.
- (b) fails: iteration 1 produced new universal indicators, and new indicators appeared late in the pass. L9 alone produced two P0s, and L6's first targeted read produced a P1 by running code.

So the loop continues. Q9 ("can it go further?") is answered *yes* for every layer, as each findings file's indicator list shows.

### Iteration 2 queue (in risk order: untrusted input and authority first, then data, then presentation)

1. **L9 remainder:**
   - `server/src/{app,web,request,response,serverless,deploy,jobs,admin,push,cache,config,db/*,auth/passkey}.rs`
   - `sync/src/{replica,mqtt,crdt,bus,device}.rs`
   - `durable/*`, `observe/*`, `server-macros`
2. **L6 remainder:**
   - Windows: `message_loop`, `integration`, `input/*`, `uia/*`, `rendering/{accessibility,styling,native_controls}`, `graphics/d2d`, `services/*`
   - Linux: `gtk/*`, `desktop/tray`, `services/*`
   - Android: `registry`, `input/*`, `accessibility`, `jni_host/mod`, `rendering/*`, `services/*`
   - TUI: `model`, `paint`, `edit`, `emulator`, `harness`, `term/*` (full), `width/table`
3. **L7 remainder:** `rn.js` (full), `client.rs`, `dom.rs`, `wasm.rs`, `capability.rs`, `request.rs`, `pwa.rs` + `sw.js`, `export.rs`, and `webgen` `expr`/`items`/`pat`/`ty`/`methods` (full).
4. **L8:** `components/*` and the rest of `data/*`.
5. **L10:** the rest of the CLI, especially `package/*` (signing, manifests), `deploy`, `emulate/*`, `generate`, `project/templates`, `doctor`.
6. **L0–L5 re-pass:** re-ask Q1–Q10 with U20–U28 in hand. Each new universal indicator is a lens the first pass did not have. For example, U25 (atomic writes) and U27 (environment) were not looked for in L2's build scripts.

After iteration 2, the same two conditions are checked. Iteration 3 runs only if they fail again.

### Iteration 2 (in progress): result so far

Iteration 2 has reached about 50.5 % of lines touched (231 files read in full, 77 skimmed). New findings:

| ID | Sev | Finding |
|---|---|---|
| L9-F7 | **P1** | The MQTT broker has no authentication or ACL, and its client-id takeover is open. |
| L9-F12 | **P1** | One peer can poison every replica's clock through HLC `observe`. |
| L6-F12 | **P1** | A screen-reader action that panics aborts the process on Windows; the same panic from a mouse click is contained. |
| L9-F8 | P2 | Server: no slowloris or render deadlines; upgrades bypass rate limiting. |
| L9-F9 | P2 | A cached public route can store and replay a personalised page. |
| L9-F10 | P2 | MQTT allocates whatever packet length the client declares, and its queues are unbounded. |
| L6-F13 | P2 | 4 of 13 Windows callbacks have no panic guard. |
| L6-F14 | P3 | Any same-session process can inject a deep link through `WM_COPYDATA`. |
| L10-F5 | P3 | The PFX password is passed in `signtool`'s argv. |

Status changes: D5 confirmed, D8 mostly refuted, D7 refuted. Positive references to copy: GTK's single guarded entry (L6-F15), passkey's depth-limited CBOR reader (L9-F14), the durable workflow design, TUI caret clamping, and `Html::trusted(&'static str)`.

Neither termination condition holds yet:
- (a) fails: about 49 % of lines are untouched.
- (b) fails: three new P1s appeared in iteration 2.

**Wave 0 additions** (same red-test-first procedure as tasks 0.1–0.10):
- **Task 0.11:** add authN, ACL, and TLS to the MQTT broker, and bound its packets and queues (L9-F7, L9-F10).
- **Task 0.12:** reject HLC stamps beyond the drift bound (L9-F12).
- **Task 0.13:** route Windows UIA providers and the 4 unguarded window procedures through one guarded `enter` (L6-F12, L6-F13, L6.6).
- **Task 0.14:** add server phase timeouts and connection caps, and route upgrades through the security pipeline (L9-F8).
- **Task 0.15:** cache only responses whose handler read no session or cookie (L9-F9).

### Iteration 2 (session 3): result

Session 3 worked the queue in risk order: the L9 remainder, the L6 remainder, L7, L8, L10, then a re-pass of L0–L5 with U20–U28. The findings are in each layer's file: L9-F15–F42, L6-F16–F35, L7-F6–F10, L8-F6–F10, L10-F6–F8, L2-RP1–RP3, and L3-RP1. The scratch programs `verify8`–`verify11` are outside the repository.

| Measure | Value |
|---|---|
| Ledger rows touched | 721 of 721: 435 `read` (108 944 lines), 286 `skimmed` (96 533 lines) |
| Source files only `skimmed` | 158 (about 63 000 lines): L6 74 (24 269), L7 29 (17 437), L10 43 (16 233), L8 12 (4 963) |
| New P1 | 9 (table below) |
| New P2 | 26: L9-F16, F17, F18, F28 ✅, F30, F31, F32, F38, F39; L6-F17, F18, F19, F22, F24, F26, F28, F30, F31, F33; L7-F7, F8; L8-F7, F8, F9; L10-F7 |
| Reproduced by running code this session | verify8 (sync authority, restart, MQTT QoS 2, WNS), verify9 (the Windows echo), verify11 (`VersionedStore`) |
| Not completed | verify10 (a GDI handle count while rendering) hung and was stopped. L6-F26 stands as confirmed by reading; the measurement moves to Task 6.4 |
| New universal indicators | 5 (U29–U33) and the U24 extension |
| L0–L5 re-pass | 4 new P3s (L2-RP1–RP3, L3-RP1) and no new P0–P2. The i18n build check and the runtime use the same parser (`core/src/i18n.rs:63` re-exports `rustnative_i18n`), so there is no divergence there |

| ID | Sev | Finding |
|---|---|---|
| L9-F15 ✅ | **P1** | The WNS access token is sent to a client-supplied channel URI, and Web Push posts to any URL (SSRF). |
| L9-F27 ✅ | **P1** | Sync has no authorization: any client reads and overwrites any document, and the client's `base` and `stamp` win conflicts. |
| L9-F29 | **P1** | Durable operations are an IDOR: any caller with an id reads or cancels the operation. |
| L9-F37 ✅ | **P1** | The MQTT broker never releases QoS 2 packet ids, so later "exactly once" messages are acknowledged and dropped. |
| L6-F16 | **P1** | Windows `open_url` passes any string to `ShellExecuteW`, including files, programs, and UNC paths. Only the TUI validates. |
| L6-F21 ✅ | **P1** | Windows re-entrancy aliases `&mut Runtime`; a programmatic set echoes as a user event and swallows the next keystroke. |
| L7-F6 | **P1** | The service worker's offline queue replays `/_fn` calls with no idempotency, no expiry, and under the next user's session. |
| L8-F6 ✅ | **P1** | `VersionedStore` loses data on rollback and stores non-JSON values as `null`. |
| L10-F6 | **P1** | The auth kit makes the first sign-up an administrator. |

**Is this enough, or must the questions be asked again?** Both termination conditions still fail:
- (a) fails: every row is touched, but 158 source files are only `skimmed`, and the condition requires `read` for source files.
- (b) fails: session 3 produced 9 new P1s and 5 new universal indicators, and the L0–L5 re-pass with the new lenses still found new (P3) items.

**Wave 0 additions** (same red-test-first procedure as tasks 0.1–0.10):
- **Task 0.16:** push targets come from an allow-list of provider hosts, and the WNS token is sent only to WNS hosts (L9-F15; U24, U26).
- **Task 0.17:** sync authorizes every read and write per document, and the server assigns `base` and `stamp` (L9-F27; U24).
- **Task 0.18:** operations record their owner and check it on every access (L9-F29; U24 extension).
- **Task 0.19:** the MQTT broker releases a QoS 2 id on `PUBREL`, and a test cycles 65 536 publishes from one client (L9-F37; L9.16).
- **Task 0.20:** `open_url` on every backend takes a `SafeUrl` (http, https, mailto, and declared schemes), as the TUI already does (L6-F16; U26, U28). **Done** in `fd3a789`: every backend's `open_url` calls `UrlPolicy::check`; confirmed in iteration 3.
- **Task 0.21:** Windows gets one re-entry guard: re-entrant calls are queued, and programmatic sets are muted (L6-F21; U31).
- **Task 0.22:** offline calls carry an idempotency key, an owner, and an expiry, and sign-out clears the queue and the page cache (L7-F6, L7-F7; U33).
- **Task 0.23:** on rollback, `VersionedStore` leaves data of an unknown newer version untouched, and it refuses values it cannot encode instead of storing `null` (L8-F6; U20, U25).
- **Task 0.24:** the auth kit has no implicit administrator; bootstrap is an explicit, one-time grant issued from the CLI (L10-F6).

### Iteration 3 queue

1. **A full read of every `skimmed` source file**, listed with line counts in `rustnative-tools/layer-solidity/skimmed-src.txt` (outside the repository). In risk order:
   - L7: the rest of `rn.js`, then `client.rs`, `dom.rs`, `wasm.rs`, and the `webgen` translator (`expr`, `items`, `pat`, `ty`, `methods`).
   - L10: the CLI, especially `package/*`, `deploy`, `emulate/*`, `generate`, and `project/templates`.
   - L6: Android's remaining modules (about 35), all TUI source, Windows `uia/provider`, `uia/properties`, `input/pointer`, and `graphics/d2d`, and GTK rendering.
   - L8: the remaining `components/*`.
2. **Measure what reading could not:** the GDI handle count over many colours (L6-F26, Task 6.4), and L6-F16 and L7-F6 with a running program.
3. **Re-pass every layer with U29–U33 as lenses**, as session 3 did for L0–L5 with U20–U28.

After iteration 3, the same two conditions are checked again.

### Iteration 3 (2026-10-21): result

Iteration 3 read the 158 source files that were only `skimmed` (L7, L10, L6, then L8), ran the two measurements that reading could not settle, and re-passed every layer with U29–U33 as lenses. The findings are in each layer's file: L7-F11–F18, L10-F9–F19, L6-F36–F48, L8-F11–F16, L9-F43–F46, and L3-RP2–RP3. The scratch programs `verify12` and `verify13` are outside the repository, in `rustnative-tools/layer-solidity/`.

| Measure | Value |
|---|---|
| Ledger | 730 rows (208 284 lines), all touched: 608 `read` (176 737 lines) and 122 `skimmed` (31 547 lines), every one of them a test. The 9 rows added are the files the merged fixes created (`core/src/secret.rs`, `core/src/services/url_policy.rs`, `server/src/listen.rs`, `sync/src/listen.rs`, and 5 tests), and the line counts were refreshed from the working tree |
| Source rows | every `src` row is `read`. 35 in-crate modules compiled only under `#[cfg(test)]` (GTK and Windows `*_integration.rs`, `harness.rs`, `test_support.rs`, `graphics/tests.rs`, `fake_bus.rs`, `capability_tests.rs`) were relabelled `test`. The TUI `harness.rs` (public) and Android `device_tests/` (behind the `device-tests` feature) ship, so they were read in full; that read found L6-F46–F48 |
| New P1 | 4 (table below) |
| New P2 | 16: L7-F12 ✅, F13, F14; L10-F10 ✅, F11 ✅, F12, F13; L6-F36, F37 ✅, F38, F39, F47 ✅; L8-F12 ✅, F13 ✅; L9-F44; L3-RP2 ✅ |
| New P3 | 24: L7-F15–F18; L10-F14 ✅, F15 ✅, F16–F19; L6-F40 ✅, F41–F46, F48; L8-F14 ✅, F15, F16; L9-F45, F46; L3-RP3 |
| Reproduced by running code | verify12 (CLI `db rollback`, `tokens import`, `upgrade`, `doctor`; LSP drift; form decoding; idle keep-alive slots; the service-worker replay in Chromium), verify13 (TUI paint and virtual realize scaling, `Debug` cache keys, `BearerAuth`, the invalidation race, `DataTable` sort, `ListSelection` anchor, the UI task pool, the Android device-suite verdict) |
| Measurements owed from the queue | L7-F6 ✅: a call queued offline under `user=alice` was delivered under `user=bob`. L6-F16: fixed in `fd3a789`; every backend's `open_url` checks `UrlPolicy`, and the TUI and core tests pass. L6-F26 (GDI handles) needs Windows and stays with Task 6.4 |
| Fixed since iteration 2 | L6-F16 (fd3a789), L9-F8 (3485eb2, which introduced L9-F43), L9-F10 (4e916b8), L9-F16 (20fc4fa, 5633bfa), L3-F19 (268034b) |
| New universal indicators | 2 (U34, U35). U20, U22, U26, U29, U32, and U33 gained proving findings |
| New layer indicators | 14: L7.8–L7.11, L10.10–L10.12, L6.7–L6.8, L8.7–L8.9, L9.18, L3.4.13 |
| U29–U33 re-pass | L0–L5: L3-RP2 (P2) and L3-RP3 (P3); the inspector token is now private. L9: L9-F43 (P1), F44, F45, F46. L6–L8 and L10: their open U29–U33 items (L6-F14, L6-F21, L7-F6, L8-F8) are unchanged, and the new items are recorded in each layer's iteration-3 section |

| ID | Sev | Finding |
|---|---|---|
| L7-F11 ✅ | **P1** | Form and query decoding is quadratic in the field count and runs on the server's async workers, so one large form stalls a worker. |
| L10-F9 ✅ | **P1** | `rustnative db rollback --to <name>` with a name that is not applied reverses every migration and exits 0. |
| L8-F11 ✅ | **P1** | Sorting a `DataTable` column that mixes numbers and text panics in `render`: the comparator is not a total order. |
| L9-F43 ✅ | **P1** | Idle keep-alive connections hold the server's connection slots forever, so 1 024 quiet sockets lock every other client out (a regression from 3485eb2). |

**Is this enough, or must the questions be asked again?**
- (a) **holds**: every source row is `read`, and only tests remain `skimmed`.
- (b) **fails**: iteration 3 found 4 new P1s, 2 new universal indicators, and 14 layer indicators, and the U29–U33 re-pass of L0–L5 still found a P2 (L3-RP2).

**Wave 0 additions** (same red-test-first procedure as tasks 0.1–0.10):
- **Task 0.25:** the server and the deploy splitter set an idle keep-alive timeout and a total header deadline, and a fill-and-wait test holds every slot with quiet keep-alive connections and asserts a new client is served (L9-F43; U34). The MQTT broker caps keep-alive (L9-F44).
- **Task 0.26:** form and query decoding is linear and bounded by field count and size, and a 10⁵-field body is refused within a fixed time (L7-F11; U22).
- **Task 0.27:** `db rollback --to` refuses a name that is not an applied migration and exits non-zero; `--to` with no match reverses nothing (L10-F9; U20).
- **Task 0.28:** `DataTable` sorts with a total order over mixed cells (numbers before text, then by value), and a property test sorts arbitrary mixed columns without panicking (L8-F11; L8.7).

### Iteration 4 queue

1. **A full Q1–Q10 pass over every layer** with U34 and U35 as new lenses. Condition (a) now holds, so this pass decides (b).
2. **Skimmed tests:** for each of the 126 `skimmed` test files, check that the tests named by the Wave 0 tasks exist or are owed, and read in full any test the findings files cite as evidence.
3. **Owed measurement:** L6-F26 on Windows (Task 6.4), driven from a timer with a hard timeout.
4. **Re-run the reproductions after fixes land:** each ✅ finding's scratch program, once its task merges, must fail before the fix and pass after it.

After iteration 4, the same two conditions are checked again.

### Iteration 4 (2026-10-21): result

Iteration 4 was a fresh Q1–Q10 pass over every layer, run as eight parallel reviews (L0–L2, L3, L4–L5 with the CI scripts, Windows and Android, GTK and the terminal, L7, L8 with L10, and L9), each reading its layer's findings first so that only new defects were reported. P0/P1 candidates and the reviewers' main claims were checked against the code before recording. The findings are in each layer's "Iteration 4" section. The scratch programs are in `rustnative-tools/layer-solidity/verify14/`.

| Measure | Value |
|---|---|
| New P1 | 1: L6-F54 |
| New P2 | 20: L1-F21 ✅; L2-F29 ✅, F30 ✅; L3-RP4 ✅; L4-F17 ✅; L5-F22 ✅, F23 ✅; L6-F49 ✅, F50 ✅, F55, F56, F57, F58, F59; L7-F19 ✅, F20 ✅, F21 ✅; L9-F47 ✅, F49; L10-F20 ✅ |
| New P3 | 20: L1-F22 ✅; L2-F31, F32, F33; L3-RP5 ✅, RP6 ✅; L4-F18; L5-F24 ✅; L6-F51, F52, F53, F60; L7-F22 ✅, F23, F24; L9-F48 ✅, F50; L10-F21 ✅, F22, F23 |
| Extended | L6-F47 (adb failures and the missing completion record), L9-F45 (invalid-JSON lines and no write timeout) |
| Layers with nothing new | L8 (`rustnative-components`, `rustnative-data`) and L0 (`rustnative-types`) |
| New universal indicators | 2: U36 (every oracle has a positive witness) and U37 (every check covers the indirect form). U22, U26, and U29 gained proving findings |
| New layer indicators | 16: L1.19; L2.24–L2.25; L3.8.10, L3.4.14; L4.8; L5.9; L6.9–L6.11; L7.12–L7.14; L9.19–L9.20; L10.13 |
| Fix checks | 20fc4fa and 5633bfa (control API) are complete. 3485eb2 still carries L9-F43 |
| Partly covered this pass | `rustnative-markup` `lower.rs`, `rsx_file.rs`, `rsx_scan.rs`, `parse.rs` were compared against existing findings only; iteration 5 reads them in full |

| ID | Sev | Finding |
|---|---|---|
| L6-F54 | **P1** | Leaking an `ExternalLoop` or embed root with safe `mem::forget` leaves Windows windows holding a dangling `&mut Application` (use after free on the next loop). |

**Is this enough, or must the questions be asked again?**
- (a) **holds**: every source row is `read`.
- (b) **fails**: iteration 4 found 1 new P1, 20 P2s, 20 P3s, 2 universal indicators, and 16 layer indicators. The rate is falling (iteration 3 found 4 P1s), but every layer except L0 and L8 still yielded something.

**Wave 0 additions** (same red-test-first procedure as tasks 0.1–0.10):
- **Task 0.29:** `start_external` and `embed` take a closure (`with_external(&mut app, |host_loop| …)`) or own the `Application`, so no safe code can outlive the borrow; a test `mem::forget`s every public guard type (L6-F54; L6.11, U5).

### Iteration 5 queue

1. A fresh Q1–Q10 pass over every layer, with U36 and U37 as new lenses, again as parallel reviews that read the findings files first.
2. Read `rustnative-markup`'s `lower.rs`, `rsx_file.rs`, `rsx_scan.rs`, and `parse.rs` in full.
3. Owed measurement: L6-F26 on Windows (Task 6.4). Windows and Android findings from iteration 4 (L6-F54–F60) are confirmed by reading only and need a device run.

After iteration 5, the same two conditions are checked again.

### Iteration 5 (2026-10-21): result

Iteration 5 was a fresh Q1–Q10 pass over every layer, again as eight parallel reviews that read each layer's findings first, with U36 and U37 as new lenses. Each review favoured the code earlier passes had covered least, and `rustnative-markup`'s `lower.rs`, `rsx_file.rs`, `rsx_scan.rs`, and `parse.rs` were read in full. P1 claims and the main P2 claims were checked against the code, and their reproductions' output files were read, before recording. The findings are in each layer's "Iteration 5" section; the scratch programs are in `rustnative-tools/layer-solidity/verify15/`.

| Measure | Value |
|---|---|
| New P1 | 2: L3-RP7 ✅ (layout), L9-F51 ✅ (durable workflows) |
| New P2 | 23: L2-F34 ✅; L3-RP8 ✅, RP9 ✅, RP10 ✅, RP11 ✅; L4-F19 ✅; L6-F61 ✅, F62 ✅, F63 ✅, F67, F68, F69, F70, F71; L7-F25 ✅, F26 ✅, F27 ✅, F28 ✅; L9-F52 ✅, F53 ✅, F54 ✅; L10-F24 ✅, F25 ✅ |
| New P3 | 32: L2-F35–F40 ✅; L3-RP12 ✅, RP13; L4-F20 ✅, F21 ✅, F22 ✅; L6-F64 ✅, F65, F66, F72–F75; L7-F29–F32 ✅, F33; L8-F17 ✅, F18; L9-F55, F56, F57; L10-F26 ✅, F27 ✅, F28 ✅, F29 |
| Layers with nothing new | L0, L1, L5 |
| New universal indicator | 1: U38 (what is planned, shown, or declared is computed by the same function that acts). U22 and U30 gained proving findings; L4.9 sharpens U36 (validate each oracle against an injected real failure) |
| New layer indicators | 14: L2.26–L2.27; L3.4.15, L3.9.9, L3.2.9; L4.9; L6.12–L6.13; L7.15–L7.16; L9.21–L9.22; L10.14–L10.15 |
| Not reproducible here | Windows and Android findings (L6-F54–F60, F66–F75) are confirmed by reading; they need a device run |

**Is this enough, or must the questions be asked again?**
- (a) **holds**: every source row is `read`.
- (b) **fails**: iteration 5 found 2 new P1s, 23 P2s, 32 P3s, 1 universal indicator, and 14 layer indicators. P1s per iteration fell from 9 (iteration 2) to 4, 1, and 2, but every pass still finds new P2s in most layers, so the loop has not converged.

**Wave 0 additions** (same red-test-first procedure as tasks 0.1–0.10):
- **Task 0.30:** the layout sharing pass uses each child's resolved size (`Fixed` included), and a property test asserts siblings never overlap and the sharing pass agrees with placement (L3-RP7; U38, L3.4.15).
- **Task 0.31:** workflow runners claim an execution with a lease before replaying it, a journal-key conflict means "another runner recorded this step", and every status change is compare-and-set on `status = 'running'` (L9-F51; U30, L9.21).

### Standing triggers

The method is also periodic. **Re-run Part A's ten questions for a layer when:**

1. a milestone touching that layer completes (the `PLAN.md` milestone list is the trigger list);
2. a new backend starts (it inherits L6.0 verbatim and adds its host section);
3. any bug escapes to a user or to master CI — ask "which indicator should have caught this, and why didn't it?" and add or tighten one (the post-incident question is the most productive source of new indicators);
4. a gate's canary (U19) fails or a gate is quarantined;
5. before any public release (all layers, full pass);
6. quarterly otherwise, starting with L3 (the core), because review findings in upper layers trace back to missing core contracts.

---

## E.8 Where the loop stopped (2026-10-21)

The loop was stopped on request after iteration 5, before condition (b) held. This section is the summary of every iteration; Appendix 4 lists every finding.

| Iteration | Method | New P0 | New P1 | New P2 | New P3 | New universal indicators |
|---|---|---|---|---|---|---|
| 1 | Layer-by-layer reading, first deep pass (223 files read in full) | 3 | about 30 | about 160 P2 and P3 together | — | U20–U28 |
| 2 | Remaining source, and a re-pass of L0–L5 with U20–U28 | 0 | 9 (session 3) plus the session-2 P1s in E.7 | 26 (session 3) | recorded per layer | U29–U33, U24 extension |
| 3 | The 158 skimmed source files read in full, measurements, U29–U33 re-pass | 0 | 4 | 16 | 24 | U34–U35 |
| 4 | Fresh pass over every layer, eight parallel reviews | 0 | 1 | 20 | 20 | U36–U37 |
| 5 | Fresh pass over every layer, eight parallel reviews | 0 | 2 | 23 | 32 | U38 |

**Totals across the register (Appendix 4):** 397 rows — P0 3, P1 45, P2 171, P3 154, and 24 strengths or refuted candidates. Universal indicators U1–U38 (U19 is the gate canary in Part B). Five findings are fixed by merged commits: L6-F16, L9-F8, L9-F10, L9-F16, and L3-F19.

**Termination status:**
- (a) holds: 730 ledger rows (208 284 lines); 608 `read`, 122 `skimmed`, and every `skimmed` row is a test.
- (b) does not hold: the last pass still found new P1s. The trend in P1s (9, 4, 1, 2) suggests the severe defects are thinning, while P2s and P3s keep appearing at a steady rate in the backends (L6), web (L7), server (L9), and tooling (L10).

**Every P0 and P1 recorded, with its status:**

| ID | Sev | Iter | Status | Finding |
|---|---|---|---|---|
| L3-F1 | P0 | 1 | ✅ open | The diff cannot see several node fields, so native objects go stale. |
| L9-F1 | P0 | 1 | ✅ open | Live sessions can be hijacked by guessing their id. |
| L9-F2 | P0 | 1 | ✅ open | The resume snapshot is client-supplied and trusted. |
| L0-F1 | P1 | 1 | open | `Scalar` claims to be "a finite `f32`" but `Scalar::new` passes `±∞` through. Any consumer relying on finiteness (division, interpolation, layout rounding) can |
| L1-F1 | P1 | 1 | open | Breakpoint precedence differs between the Web and every native backend. |
| L1-F2 | P1 | 1 | open | `rtl:`/`ltr:` precedence differs too. |
| L10-F1 | P1 | 1 | open | `rustnative fmt` deletes the user's comments and rewrites files non-atomically. |
| L10-F6 | P1 | 2 | open | The generated sign-up makes the first account to register an administrator. |
| L10-F9 | P1 | 3 | ✅ open | `rustnative db rollback <db> --to <name>` with a name that is not applied reverses every migration and exits 0. |
| L11-F1 | P1 | 1 | open | No dynamic-analysis job. |
| L2-F1 | P1 | 1 | ✅ open | `.rsx` compiler panics |
| L2-F13 | P1 | 1 | ✅ open | Unlisted languages silently get English plural rules |
| L2-F2 | P1 | 1 | ✅ open | `pub mod child;` in a `.rsx` file produces invalid Rust |
| L2-F3 | P1 | 1 | ✅ open | `rustnative fmt` deletes comments inside markup. |
| L3-F17 | P1 | 1 | open | A transient read error destroys saved state. |
| L3-F19 | P1 | 1 | fixed (268034b) | The inspector's token is written to a shared temp directory |
| L3-F2 | P1 | 1 | ✅ open | After an uncontained render panic, the tree is permanently broken. |
| L3-F3 | P1 | 1 | open | Containment gaps. |
| L3-F30 | P1 | 1 | open | Every render pays O(whole tree), measured. |
| L3-F33 | P1 | 1 | open | Wire keys address any component |
| L3-F37 | P1 | 1 | open | `FileDocuments::write` is not atomic |
| L3-F4 | P1 | 1 | open | State written in reaction to `Lifecycle::Terminating`/`Suspending` is never flushed by `lifecycle()` |
| L3-F5 | P1 | 1 | open | Crash reports contain the whole UI tree with every text-input value |
| L3-RP7 | P1 | 5 | ✅ open | Fixed sizes are left out of main-axis space sharing and out of a container's natural size, so "fixed sidebar or header plus fill content" overflows on every backend. |
| L4-F1 | P1 | 1 | open | Headless rebuilds its realized tree from the full snapshot, not by applying diff ops. |
| L4-F2 | P1 | 1 | open | `terminate_and_relaunch` / `kill_and_restore` drop the app gracefully |
| L5-F12 | P1 | 1 | ✅ open | Ellipse and path fills iterate over their whole bounds before clipping. |
| L5-F5 | P1 | 1 | open | Version parsing accepts garbage silently, which feeds downgrade and freeze decisions. |
| L6-F1 | P1 | 1 | ✅ open | A *caught* panic hands the terminal back while the app keeps running. |
| L6-F12 | P1 | 2 | open | Whether a panic is survivable depends on the input device. |
| L6-F16 | P1 | 2 | fixed (fd3a789) | On Windows, `SystemService::open_url` runs whatever it is given. |
| L6-F2 | P1 | 1 | open | After a contained panic, the TUI loops on the broken tree |
| L6-F21 | P1 | 2 | ✅ open | Writing a text field re-enters the window procedure while the runtime is mutably borrowed, so two `&mut Runtime` coexist. |
| L6-F54 | P1 | 4 | open | Leaking an `ExternalLoop` (or the `embed` root) in safe code leaves windows holding a dangling `&mut Application`. |
| L7-F1 | P1 | 1 | open | Link and form URLs are never scheme-checked. |
| L7-F11 | P1 | 3 | ✅ open | Form and query decoding is quadratic in the number of fields, and it runs on the server's async workers. |
| L7-F6 | P1 | 2 | ✅ open | The offline call queue replays `/_fn/` POSTs without idempotency, without expiry, and under whoever is signed in later. |
| L8-F11 | P1 | 3 | ✅ open | Sorting a `DataTable` column that mixes numbers and text panics in `render`. |
| L8-F6 | P1 | 2 | ✅ open | `VersionedStore` destroys data on a rollback, and silently nulls any value that is not JSON. |
| L9-F12 | P1 | 2 | open | One peer can poison every replica's clock forever. |
| L9-F15 | P1 | 2 | ✅ open | Push requests go to whatever URL the subscriber supplied, with the server's credentials attached. |
| L9-F27 | P1 | 2 | ✅ open | Sync has no authorization and trusts the client's conflict metadata. |
| L9-F29 | P1 | 2 | open | Long-running operations are readable and cancellable by anyone who can count. |
| L9-F3 | P1 | 1 | open | Live events bypass disabled and hidden state. |
| L9-F37 | P1 | 2 | ✅ open | The MQTT broker silently drops "exactly once" messages once a client reuses a packet id. |
| L9-F43 | P1 | 3 | ✅ open | Idle keep-alive connections hold `max_connections` slots forever, so 1 024 idle sockets lock out the server. |
| L9-F51 | P1 | 5 | ✅ open | Two runners of one `LocalEngine` execute the same step twice, and the loser compensates the work and marks a successful execution failed. |
| L9-F7 | P1 | 2 | open | The MQTT broker has no authentication, no authorization, and no transport security. |

**Remediation strategy:** targeted redesign where the criteria in the "READ BEFORE ACTING ON ANY FINDING" block at the top of this plan hold, patches elsewhere. That block is binding, and its six areas are examples, not a limit: any part may be redesigned when the criteria hold.

**To resume the loop:** run iteration 6 as a fresh pass over every layer with U38 as a new lens (HANDOVER.md has the procedure), confirm the Windows and Android findings on devices, measure L6-F26 on Windows, and fix the Wave 0 tasks red-test first, rerunning each finding's scratch program before and after its fix.

---

# Appendix 1 — Candidate defects found while planning (to confirm in the named task)

Status after loop iteration 1: **confirmed** (by reading, or ✅ by running), **refuted**, or **open** (not yet examined in the loop).

| # | Candidate | Evidence | Confirm in | Status |
|---|---|---|---|---|
| D1 | `Scalar::new` admits ±∞ although documented as "a finite `f32`" | `crates/rustnative-types/src/scalar.rs:3,19-21` (maps only `NaN` and `0.0`) | Task 2.1 | **Confirmed ✅** (L0-F1) |
| D2 | No recursion-depth limit in any recursive parser (markup, i18n, IDL) | grep `MAX_DEPTH\|max_depth` → no hits; recursive `parse_element`/`parse_child` in `crates/rustnative-markup/src/parse.rs:74,206` | Task 2.3 | **Confirmed** for style `calc`/`color-mix` (L1) and tree depth (L3-F8 ✅); markup, i18n, and IDL remain to be fuzzed |
| D3 | `UiThread::claim()` is public and unconditional, so the affinity proof can be minted on any thread | `crates/rustnative-core/src/affinity.rs:27-29` | Task 3.4 | **Confirmed** by reading (it proves "a thread", not "the UI thread") |
| D4 | Web `Link` href written from page data without a scheme check (`javascript:` → link; strict CSP mitigates) | `crates/rustnative-web/src/dom.rs:774-776` | Task 0.6 | **Confirmed** (L7-F1). The default CSP mitigates it; static export does not. |
| D5 | Two byte-identical HTML escapers (web, server) — divergence risk | `crates/rustnative-web/src/html.rs:21`, `crates/rustnative-server/src/response.rs:144` | Task 5.1 | **Confirmed (iteration 2).** `server/src/response.rs:143` duplicates `web/src/html.rs:21` character for character today. Merge them into one exported escaper. The server's `Html::trusted` accepting only `&'static str` is an E5 pattern to keep. |
| D6 | Update channel has no anti-rollback, no key rotation, no signed size | `crates/rustnative-update/src/lib.rs` (no `downgrade\|rollback\|keyring`; `UpdateManifest` has no `size`) | Task 0.8 | **Confirmed** and widened (L5-F5–F8, L6-F7) |
| D7 | `rustnative-interop` runtime: 3 `extern` fns, 2 `catch_unwind` — one export may be unguarded | `crates/rustnative-interop/src/runtime.rs` | Task 4.4 / 6.1 | **Refuted.** `runtime.rs` has no exports; every generated shim wraps its body in `runtime::guard` (`generate.rs` `rust_service_shims`). `last_error` and `string_free` cannot panic. |
| D8 | Android JNI local-reference frames used in one module only (3 sites) | `crates/rustnative-android/src/jni_host/mod.rs` | Task 6.6 (CheckJNI will show it) | **Mostly refuted (iteration 2).** Every JNI call goes through `jni_host::with_env`, which wraps it in `with_local_frame(64)`, so references are released per call. The residue is P3: loops such as `Arg::Strs` and `read_strings` create one local reference per element without `DeleteLocalRef`. With `minSdk 26`, ART's local table grows instead of aborting, so a 10 000-item list costs only a memory spike for one call. |
| D9 | Toolchain unpinned (`channel = "stable"`) while CI and local drift | `rust-toolchain.toml` | Task 1.13 | **Confirmed** (L11-F3) |
| D10 | Actions pinned by mutable tag | `.github/workflows/ci.yml:35-39,…` | Task 1.13 | **Confirmed** (L11-F4) |
| D11 | Milestone 65 (fuzzing, sanitizers, coverage, mutation, semver, architecture tests) unimplemented although Tier 1 "continuous" | `docs/ecosystem-analysis/parity-matrix.md:358-359`; no fuzz/miri/sanitizer config anywhere | Wave 1 | **Confirmed** (L11-F1) |
| D12 | Public signatures return `Result<_, String>` | `rustnative-style` `color.rs:31`, `value.rs:292,382`; `rustnative-sync` `device.rs:196`, `mqtt.rs:178`; `rustnative-server` `jobs.rs:103`, `db/schema.rs:82` | Task 2.4 | Open (iteration 2) |
| D13 | Security-critical and data-critical crates with very few tests: `rustnative-update` 4, `rustnative-i18n` 3, `rustnative-types` 1, `rustnative-interop` 7 | test-attribute counts per crate | Tasks 2.1, 2.3, 4.2, 4.4 | **Confirmed.** The defects in exactly these crates (L0-F1, L2 i18n ✅, L5-F5–F8, L5-F18–F20) were all untested. |
| D14 | Gates live outside the repository (`rustnative-tools/`) | `RESUME.md` "How to work here" | Task 1.15 | Open |

# Appendix 2 — Traceability (indicator → task)

| Indicators | Tasks |
|---|---|
| U1 | 1.1, E11 |
| U2, U3, U4, U5 counts | 1.2, 1.3 |
| U3b FFI | 6.1 |
| U5 soundness tools | 1.14 |
| U6 | 1.4, 1.5, 1.8 |
| U7 | 1.6 |
| U8 | 1.7, per-layer bound inventories |
| U9 | 3.4, 3.5, 1.14 (TSan) |
| U10 | 9.1, 3.1, 3.3, 8.6 |
| U11 | 1.9, 1.12 |
| U12, U18 | 1.10, 5.4, 6.2 |
| U13 | 1.11 |
| U14 | 1.13 |
| U15 | 2.4 |
| U16 | 1.4, 2.3, 8.2 |
| U17 | 6.1, 6.3, per-backend teardown |
| U19 | E12 canaries in 1.2, 1.10, 1.11 |
| L0 | 2.1 |
| L1 | 2.2, 2.4, 6.2 |
| L2 | 2.3, 2.6 |
| L3.1–L3.10 | 3.1–3.10, 3.12 |
| L4 | 3.11 |
| L5a–d | 4.1–4.4 |
| L6.0, L6a–d | 6.1–6.7 |
| L7 | 5.1–5.4 |
| L8 | 7.1, 7.2, 3.8 |
| L9a–d | 8.1–8.6 |
| L10 | 9.1–9.3 |
| L11 | 1.13, 1.15, 9.4 |

# Appendix 3 — Self-review against the request

- *"Every layer"*: 12 layers covering all 29 crates (each crate appears in exactly one layer's heading; `rustnative-i18n` appears in L2 for its parser and L8 for runtime use), plus CI/release as L11. Examples are covered as test subjects (Tasks 3.8, 8.3, 9.1) rather than as a layer, because they are consumers, not foundations.
- *"Basic, standard, production, edge"*: each layer's Q1 is split along those four depths, and the universal set U1–U19 holds the cross-cutting ones.
- *"All aspects"*: the fourteen-aspect grid is the completeness check; Part E.1 lists the gaps the grid exposed on the second pass.
- *"Why, specifically"*: every Q3 names a failure in a named file or a review finding.
- *"Bounds"*: numeric or binary per indicator; judgment-call bounds carry a review date (E.2).
- *"Measure at every facet; reliable enough?"*: grades M0–M5 per indicator; E.3 replaces the weak instruments.
- *"Enforce, push to the extreme; best methods?"*: rungs E0–E5 per indicator; E.4 promotes the remaining ones; Q7 per layer names the alternative considered.
- *"Implement safely"*: Part D's non-blocking-first rule, format/version rule, and API-snapshot-first ordering (E.5).
- *"Limit or further?"*: Q9 per layer and E.6.
- *"Enough or ask again?"*: E.7's verdict and re-check triggers.
- *"Don't change any code"*: this document is the only file written; no source, test, CI, or configuration file was modified.

# Appendix 4 — Findings register (all iterations)

Every finding the evidence loop recorded, iterations 1–5, generated from the layer files in `layer-solidity/`, where each row has its full description and file:line evidence. **Sev** is the severity; "—" marks a strength or a refuted candidate the loop recorded so that it is not re-examined. **Iter** is the iteration whose section holds the row. **Status**: ✅ reproduced by running code; *read* confirmed by reading; *fixed* closed by the named commit.

Totals: 397 rows — P0 3, P1 45, P2 171, P3 154, strengths or refuted 24.

## L0 — Value types

7 rows: P1 1, P2 1, P3 3, — 2. Source: `layer-solidity/L0-types.md`.

| ID | Sev | Iter | Status | Finding |
|---|---|---|---|---|
| L0-F1 | P1 | 1 | read | `Scalar` claims to be "a finite `f32`" but `Scalar::new` passes `±∞` through. Any consumer relying on finiteness (division, interpolation, layout rounding) can |
| L0-F2 | P3 | 1 | read | Module doc and crate description say "totally ordered scalars", but `Scalar` implements only `PartialEq`/`Eq`/`Hash` — total *equality*, no `PartialOrd`/`Ord`. |
| L0-F3 | P3 | 1 | read | `From<u32> for Color` silently discards the top byte. `Color::from(0x336699CC)` written by someone expecting `0xRRGGBBAA` yields `rgb(0x66,0x99,0xCC)` — a wrong |
| L0-F4 | P2 | 1 | read | `Size` is `u32` but `Rect`/`Point`/`SizeMode::Fixed`/`EdgeInsets` are `i32`: negative widths, negative fixed sizes, and negative insets are representable and th |
| L0-F5 | P3 | 1 | read | `serde` field names are the public field names (`start`/`end`, `red`…); renaming a field silently changes every persisted/wire format that embeds these types (c |
| L0-F6 | — | 1 | — | `Scalar` has no `serde` impl — good: a derived `Deserialize` would bypass `Scalar::new` (deserializing `NaN`/`-0.0` raw and breaking `Eq`). Must stay this way, |
| L0-F7 | — | 1 | — | `#![no_std]` with no `extern crate alloc`: allocation is structurally impossible — L0.6 is already E5. `#![deny(missing_docs)]` already on. |

## L1 — Style model

22 rows: P1 2, P2 7, P3 11, — 2. Source: `layer-solidity/L1-style.md`.

| ID | Sev | Iter | Status | Finding |
|---|---|---|---|---|
| L1-F1 | P1 | 1 | read | Breakpoint precedence differs between the Web and every native backend. |
| L1-F2 | P1 | 1 | read | `rtl:`/`ltr:` precedence differs too. |
| L1-F3 | P2 | 1 | read | Both diverge from the claimed vocabulary. |
| L1-F4 | P2 | 1 | read | An unterminated `/*` silently swallows the rest of the file. |
| L1-F5 | P3 | 1 | read | One stray `)` (paren depth goes negative) or an unterminated quote makes `statement_end` treat the rest of the file as one statement → a single misleading error |
| L1-F6 | P2 | 1 | read | Silent shadowing. |
| L1-F7 | P3 | 1 | read | `@custom-variant` names are not validated (utility names are: `:438-446`): an empty name or one with spaces/colons is accepted and unusable. |
| L1-F8 | P2 | 1 | read | A single-token `--x: initial` is lost at build output. |
| L1-F9 | P2 | 1 | read | `@custom-variant x (@media (min-width: -1px))` becomes an always-true variant |
| L1-F10 | P3 | 1 | read | Non-finite number text is accepted. |
| L1-F11 | P3 | 1 | read | Out-of-range percentages accepted |
| L1-F12 | P2 | 1 | read | Unbounded recursion |
| L1-F13 | P3 | 1 | read | Capability-table text has embedded runs of spaces |
| L1-F14 | P3 | 1 | read | `StyleError::line_column` panics on a range that is not a char boundary of the source passed (public fields, public fn); and it counts *chars*, whereas LSP posi |
| L1-F15 | P3 | 1 | read | `DeclarationSet::from_sources` documents "one entry per declaration" but does not check it; `tokens::declaration_set` guards it, a hand call does not. |
| L1-F16 | P3 | 1 | read | Design-token import writes JSON string values raw into `app.css` (`--x: {value};`): a value containing `;`, `}`, `*/`, or a newline corrupts or injects into the |
| L1-F17 | P3 | 1 | read | must be checked in L7 for any path where token values or families arrive at run time |
| L1-F18 | P3 | 1 | read | `font-[900x]` silently becomes a font *family* when the weight parse fails; `_` is always a space in arbitrary values, so a family with an underscore cannot be |
| L1-F19 | — | 1 | — | The vendored theme's SHA-256 is recorded (`VENDORED.md`) but nothing verifies it. |
| L1-F20 | — | 1 | — | Output ordering is deterministic (all maps `BTreeMap`/`BTreeSet`; groups in first-appearance order) — L1.8 is structurally held. Token resolution has a cycle bo |
| L1-F21 | P2 | 4 | ✅ | A token whose value is another token skips the build-time type check. |
| L1-F22 | P3 | 4 | ✅ | Design-token import merges colliding names without a warning. |

## L2 — Syntax and build

43 rows: P1 4, P2 19, P3 18, — 2. Source: `layer-solidity/L2-syntax-build.md`.

| ID | Sev | Iter | Status | Finding |
|---|---|---|---|---|
| L2-F1 | P1 | 1 | ✅ | `.rsx` compiler panics |
| L2-F2 | P1 | 1 | ✅ | `pub mod child;` in a `.rsx` file produces invalid Rust |
| L2-F3 | P1 | 1 | ✅ | `rustnative fmt` deletes comments inside markup. |
| L2-F4 | P2 | 1 | ✅ | The `.rsx` compiler rewrites string literals |
| L2-F5 | P2 | 1 | read | Scanner and parser can disagree |
| L2-F6 | P2 | 1 | read | No recursion bound |
| L2-F7 | P2 | 1 | read | Hygiene: internal bindings are call-site-visible. |
| L2-F8 | P2 | 1 | read | `rustnative.toml` errors are silently ignored by the style macros and `compile_styles` |
| L2-F9 | P2 | 1 | read | Provenance/capability check can skip declarations |
| L2-F10 | P2 | 1 | read | Host vs target confusion in `embed_resources` |
| L2-F11 | P2 | 1 | read | Wrong rebuild trigger for the icon |
| L2-F12 | P2 | 1 | ✅ | i18n subset gaps that fail confusingly |
| L2-F13 | P1 | 1 | ✅ | Unlisted languages silently get English plural rules |
| L2-F14 | P2 | 1 | read | No per-locale category completeness check |
| L2-F15 | P2 | 1 | read | No bidi isolation of placeables |
| L2-F16 | P3 | 1 | read | Locale fallback is case-sensitive on the full tag and script-unaware: `pt-br` misses `pt-BR`; `zh-Hant-TW` falls to `zh` (Simplified) before the source. Decimal |
| L2-F17 | P2 | 1 | read | Generated message functions can fail to compile |
| L2-F18 | P2 | 1 | read | Android `versionCode` is not monotonic with versions |
| L2-F19 | P2 | 1 | read | Android manifest sets `android:allowBackup="true"` with no `dataExtractionRules`/`fullBackupContent`: persisted state and the SQLite stores are included in clou |
| L2-F20 | P3 | 1 | read | Silent value mangling: `rc`/manifest versions map unparsable or > 65535 parts to 0 (`1.70000.0` → `1.0.0.0`); ICO accepts any bytes after the PNG signature (IHD |
| L2-F21 | P2 | 1 | read | Build output written with ignored errors |
| L2-F22 | P3 | 1 | read | `embed_assets` embeds and serves every file including dotfiles (`.env`, `.DS_Store`); `content_type` lacks `wasm` (streaming instantiation requires `application |
| L2-F23 | P2 | 1 | read | `sdk::tests::the_override_is_used_when_it_points_at_a_real_file` calls `unsafe { set_var/remove_var }` claiming no other reader — but `cargo test` runs tests on |
| L2-F24 | P3 | 1 | read | Compile-time costs: `element_spec` rebuilds the whole element table (24 elements × ~25 `AttrSpec` vectors) on every lookup, several lookups per element; style m |
| L2-F25 | P3 | 1 | read | Absolute build-machine paths are written into generated code: `#[path = "/abs/…"]` (`rsx_file.rs:379-381`), `include_str!`/`include_bytes!` of absolute paths (m |
| L2-F26 | P3 | 1 | read | Proc-macro2 `span-locations` keeps a thread-local source map that grows with every `TokenStream::from_str`; no call to `proc_macro2::extra::invalidate_current_t |
| L2-F27 | — | 1 | — | *Refuted:* the formatter keeps `margin=-4` intact (span join works). |
| L2-F28 | — | 1 | — | Duplicate attributes are refused; non-literal `class=` is a compile error with a reason; unknown attributes suggest the nearest; emitted paths are absolute (`:: |
| L2-RP1 | P3 | 2 | read | `compile_rsx` ignores a failed write of the generated Rust. |
| L2-RP2 | P3 | 2 | read | The build helpers default a missing `CARGO_MANIFEST_DIR`/`OUT_DIR` to the empty path. |
| L2-RP3 | P3 | 2 | read | The `rustnative-macros` style-vocabulary cache is keyed by path and mtime. A long-lived macro server (rust-analyzer) keeps a stale vocabulary across two edits m |
| L2-F29 | P2 | 4 | ✅ | Term references are never checked, and terms do not fall back across locales. |
| L2-F30 | P2 | 4 | ✅ | Term expansion costs exponential time and memory. |
| L2-F31 | P3 | 4 | read | The generated `messages.rs` writes `.ftl` content into Rust without encoding it for that context. |
| L2-F32 | P3 | 4 | read | `compile_styles` does not watch a style file that does not exist yet. |
| L2-F33 | P3 | 4 | read | An empty `[style] targets` turns off every capability check. |
| L2-F34 | P2 | 5 | ✅ | A left shift by an identifier is a compile error in any `.rsx` file. |
| L2-F35 | P3 | 5 | ✅ | A deleted or renamed `.rsx` file keeps compiling from stale output. |
| L2-F36 | P3 | 5 | ✅ | The `mod` rewrite writes file-system paths into Rust string literals unescaped. |
| L2-F37 | P3 | 5 | ✅ | A function-pointer type replaces the enclosing function's context. |
| L2-F38 | P3 | 5 | ✅ | Attribute expressions are not evaluated in the order written. |
| L2-F39 | P3 | 5 | ✅ | The scanner overflows the stack on deeply nested plain Rust. |
| L2-F40 | P3 | 5 | ✅ | The "needs a context" error points at the root element when the component is inside an `if`, `match`, `for`, or fragment. |

## L3 — Core runtime

61 rows: P0 1, P1 10, P2 34, P3 13, — 3. Source: `layer-solidity/L3-core.md` and its continuation files.

| ID | Sev | Iter | Status | Finding |
|---|---|---|---|---|
| L3-F1 | P0 | 1 | ✅ | The diff cannot see several node fields, so native objects go stale. |
| L3-F2 | P1 | 1 | ✅ | After an uncontained render panic, the tree is permanently broken. |
| L3-F3 | P1 | 1 | read | Containment gaps. |
| L3-F4 | P1 | 1 | read | State written in reaction to `Lifecycle::Terminating`/`Suspending` is never flushed by `lifecycle()` |
| L3-F5 | P1 | 1 | read | Crash reports contain the whole UI tree with every text-input value |
| L3-F6 | P2 | 1 | read | Every panic writes a crash report — including contained ones. |
| L3-F7 | P2 | 1 | ✅ | `rsx!` and the builder disagree on out-of-range values |
| L3-F8 | P2 | 1 | ✅ | Stack depth |
| L3-F9 | P2 | 1 | ✅ | Index-based moves |
| L3-F10 | P2 | 1 | read | Persisted-state keys depend on `std::any::type_name` |
| L3-F11 | P2 | 1 | read | The "framework bug" assertion is reachable |
| L3-F12 | P2 | 1 | read | Events reach disabled/hidden nodes |
| L3-F13 | P2 | 1 | read | composed |
| L3-F14 | P3 | 1 | read | `render_pass` stops after 3 passes silently when the tree has not settled (provider/consumer oscillation) — no diagnostic. `apply_queued_window_commands` loops |
| L3-F15 | P3 | 1 | read | `NodeId` derives `Serialize/Deserialize` although "a `NodeId` means something only on the thread that made it"; any persisted/transmitted `NodeId` aliases a dif |
| L3-F16 | — | 1 | — | Interning removes hash-collision risk by construction; allocators fail loudly on exhaustion; `Tabs::new` clamps; duplicate node keys are a `RenderError`, not a |
| L3-F17 | P1 | 1 | read | A transient read error destroys saved state. |
| L3-F18 | P2 | 1 | read | Schema change silently resets state |
| L3-F19 | P1 | 1 | fixed (268034b) | The inspector's token is written to a shared temp directory |
| L3-F20 | P2 | 1 | read | Inspection is honoured by release builds |
| L3-F21 | P2 | 1 | read | Panics inside spawned tasks vanish. |
| L3-F22 | P2 | 1 | read | A self-waking local task freezes the UI thread |
| L3-F23 | P2 | 1 | read | CPU work runs on the async workers |
| L3-F24 | P2 | 1 | read | No back-pressure on task results |
| L3-F25 | P2 | 1 | read | The backend waker runs while the scheduler's mutex is held |
| L3-F26 | P2 | 1 | read | Grid placement from data can exhaust memory |
| L3-F27 | P3 | 1 | read | Layout measures each child subtree 2–4 times per ancestor level (`preferred_height` in the sum pass and again per child; `child_width` computed twice) with no p |
| L3-F28 | P3 | 1 | read | `ComponentContext::deferred` panics if a key is reused with another type (a user-triggerable panic, not a `RenderError`); `RenderError` messages name opaque `Co |
| L3-F29 | — | 1 | — | `Constraints` keeps `min ≤ max` and non-negative bounds in its constructors *and* through `serde(from)`; `FileStateStore` writes temp → `fsync` → rename → direc |
| L3-F30 | P1 | 1 | read | Every render pays O(whole tree), measured. |
| L3-F31 | P2 | 1 | read | The stack cost is documented (≈ 0.58 KiB per level: 579 KiB at depth 1 000) and the page concludes "well under the default 1 MiB main-thread stack on Windows" — |
| L3-F32 | P2 | 1 | read | Failure traces never reach the compact form |
| L3-F33 | P1 | 1 | read | Wire keys address any component |
| L3-F34 | P2 | 1 | read | Server-rendered trees can instantiate host content |
| L3-F35 | P2 | 1 | read | Grant path checks are lexical |
| L3-F36 | P2 | 1 | read | The HTTP contract has no timeout, size limit, redirect policy, or header validation |
| L3-F37 | P1 | 1 | read | `FileDocuments::write` is not atomic |
| L3-F38 | P2 | 1 | read | A saved navigation stack can crash the application at restore |
| L3-F39 | P2 | 1 | read | Deep links reach every route |
| L3-F40 | P2 | 1 | read | `Store` freezes after a panic inside `update` |
| L3-F41 | P2 | 1 | read | Session recordings leak what redaction is meant to hide |
| L3-F42 | P2 | 1 | read | Representable-invalid controls reach backends |
| L3-F43 | P2 | 1 | read | Virtual lists are O(item count), not O(visible) |
| L3-F44 | P2 | 1 | read | Release builds change behaviour from environment variables |
| L3-F45 | P3 | 1 | read | Spring easing accepts zero damping (never settles → frames forever) and inherits `Scalar`'s ±∞ (L0-F1) → `NaN` progress → element snaps to 0; `Transform2D::inve |
| L3-F46 | P3 | 1 | read | Accessibility relationships (`labelled_by`, `described_by`, `controls`) are scoped to the declaring component, so a field in a child component cannot be labelle |
| L3-F47 | P3 | 1 | read | Shortcut labels are Windows-only (`Ctrl`, `Win`) on every host; `Shortcut::matches` compares the produced character, so on a Cyrillic or Arabic layout `Ctrl+C` |
| L3-F48 | — | 1 | — | `ImageData::rgba` validates dimensions with checked multiplication; the surfaces queue is bounded (256, drop-oldest); wire command names are leaked to `'static` |
| L3-RP1 | P3 | 2 | read | The inspectable-store registry is pruned only when it is read. |
| L3-RP2 | P2 | 3 | ✅ | The UI thread's task pool has no per-pass budget, so a task that yields by waking itself freezes the window for good. |
| L3-RP3 | P3 | 3 | read | The wire command-name table saturates for good. |
| L3-RP4 | P2 | 4 | ✅ | Every window with the same root component type shares one persisted-state key space, so one window's saved state overwrites another's. |
| L3-RP5 | P3 | 4 | ✅ | Concurrent `FileStateStore::save` calls on one key share one temporary file. |
| L3-RP6 | P3 | 4 | ✅ | `HostExecutor` keeps a timer entry for every poll of every sleep, including dropped ones, until its deadline. |
| L3-RP7 | P1 | 5 | ✅ | Fixed sizes are left out of main-axis space sharing and out of a container's natural size, so "fixed sidebar or header plus fill content" overflows on every backend. |
| L3-RP8 | P2 | 5 | ✅ | A menu-bar command ignores focus, so it runs on a different component than its shortcut and its displayed state. |
| L3-RP9 | P2 | 5 | ✅ | An RTL locale with a Unicode `-u-` extension is laid out left to right. |
| L3-RP10 | P2 | 5 | ✅ | Wire round trips lose input interest. |
| L3-RP11 | P2 | 5 | ✅ | A remote flag removed upstream is never forgotten. |
| L3-RP12 | P3 | 5 | ✅ | `perf::percentile` is not the nearest rank it claims. |
| L3-RP13 | P3 | 5 | read | A development panic dialog names the `.rsx` file with the Rust column. |

## L4 — Reference and verification

22 rows: P1 2, P2 12, P3 7, — 1. Source: `layer-solidity/L4-reference.md`.

| ID | Sev | Iter | Status | Finding |
|---|---|---|---|---|
| L4-F1 | P1 | 1 | read | Headless rebuilds its realized tree from the full snapshot, not by applying diff ops. |
| L4-F2 | P1 | 1 | read | `terminate_and_relaunch` / `kill_and_restore` drop the app gracefully |
| L4-F3 | P2 | 1 | read | `click()` on a checkbox/toggle/radio/control node does nothing, silently. |
| L4-F4 | P2 | 1 | read | `settle` stops after 1000 iterations silently |
| L4-F5 | P2 | 1 | read | `range_changes` rebuilds the virtual-list `ExtentCache` from estimates on every settle |
| L4-F6 | P2 | 1 | read | The `lifecycle()` result is discarded, and `realize` ignores snapshot errors |
| L4-F7 | P2 | 1 | read | `preview_goldens` writes a missing golden and passes |
| L4-F8 | P2 | 1 | read | The conformance driver is narrow. |
| L4-F9 | P2 | 1 | read | Equivalence suites test one typical value per name. |
| L4-F10 | P2 | 1 | read | Style equivalence is checked inside core only. |
| L4-F11 | P2 | 1 | read | Layout conformance uses one reference screen with headless metrics. |
| L4-F12 | P3 | 1 | read | The cancellation property covers only `effects.spawn` after `sleep`. |
| L4-F13 | P2 | 1 | read | Browser tests skip silently unless `RUSTNATIVE_BROWSER_REQUIRED` is set, and CI never sets it |
| L4-F14 | P3 | 1 | read | The web-testing `StaticServer` parses `Content-Length` with `unwrap_or(0)` and reads lines without bound (test-only). `Connection.events` grows without bound ac |
| L4-F15 | P3 | 1 | read | The wire round-trip test excludes canvas, class, style, input, transition, and spread from its generator, so the fields most likely to change shape are the ones |
| L4-F16 | — | 1 | — | `doc_parity` enforces two-syntax and two-spelling docs per section (with explicit exemptions). `doc_claims` requires performance words to cite `budgets/`. `ever |
| L4-F17 | P2 | 4 | ✅ | The shared suite `scrolling_renders_nothing` passes on a backend whose scrolling does not work. |
| L4-F18 | P3 | 4 | read | The tmux check "a resize is followed" cannot fail when the app ignores the resize. |
| L4-F19 | P2 | 5 | ✅ | The "no Content Security Policy" browser assertions cannot fail. |
| L4-F20 | P3 | 5 | ✅ | The missing-docs guard checks the attribute's text, not the lint level in effect. |
| L4-F21 | P3 | 5 | ✅ | Headless exhaustive mode is silently dropped after `kill_and_restore` and `relaunch_with_deep_link`. |
| L4-F22 | P3 | 5 | ✅ | `tools/linux-session.sh` leaks its Xvfb or Weston server. |

## L5 — Shared platform groups

24 rows: P1 2, P2 10, P3 9, — 3. Source: `layer-solidity/L5-groups.md`.

| ID | Sev | Iter | Status | Finding |
|---|---|---|---|---|
| L5-F1 | P2 | 1 | read | `AppIdentity.id` is never validated |
| L5-F2 | P3 | 1 | read | `wants_terminal()` treats any argv `--tui` as the switch. |
| L5-F3 | P3 | 1 | read | Android `install_crash_capture` discards its result (`let _ =`). Linux capture is panic-only: there is no native-signal or abort capture, while Windows writes m |
| L5-F4 | P3 | 1 | read | `the_default_services_cover_every_contract_the_host_offers` asserts only 7 of the 13 to 15 services (share, work, push, commerce, printing, serial, and locale a |
| L5-F5 | P1 | 1 | read | Version parsing accepts garbage silently, which feeds downgrade and freeze decisions. |
| L5-F6 | P2 | 1 | read | The model payload's installed version defaults to `"0"` on any read error |
| L5-F7 | P2 | 1 | read | `write_atomically` does not fsync |
| L5-F8 | P2 | 1 | read | One pinned key, with no rotation and no revocation. |
| L5-F9 | P3 | 1 | read | The model `name` from the manifest is joined into a path unvalidated |
| L5-F10 | P3 | 1 | read | `bucket()` maps a corrupt `installation-id` to bucket 0, so that installation always gets every rollout first. `pin()` stores the version, but `check` only test |
| L5-F11 | — | 1 | — | The signature is verified over the exact published bytes before parsing, so no re-serialization ambiguity exists. App-id binding and expiry refusal are both pre |
| L5-F12 | P1 | 1 | ✅ | Ellipse and path fills iterate over their whole bounds before clipping. |
| L5-F13 | P2 | 1 | ✅ | `StrokeRect` builds its whole perimeter in a `BTreeMap` before clipping |
| L5-F14 | P3 | 1 | read | "Rectangles never overlap" is not what the code guarantees. |
| L5-F15 | P3 | 1 | read | `GridRect::union` and `intersect` subtract without saturation (`right - x`), so a debug build can overflow on extreme rectangles. `Region::row_spans` allocates |
| L5-F16 | P2 | 1 | read | Drawn-host focus is plain preorder with no scope |
| L5-F17 | — | 1 | — | Damage coalescing is bounded (`MAX_RECTS` = 16) and loses nothing (tested). Grid rounding has a single, documented rule. Alpha and premultiplied images are hand |
| L5-F18 | P2 | 1 | read | `[thread = any]` is documented as "serialized by the handle table" but returns `BUSY` under contention. |
| L5-F19 | P2 | 1 | read | Identifiers are not checked against target-language keywords. |
| L5-F20 | P3 | 1 | read | Event handlers cannot read the instance. |
| L5-F21 | — | 1 | — | Every FFI entry runs inside `catch_unwind` and returns a status. Handles are 64-bit and never reused (no ABA). Thread affinity is checked on every owner-only ca |
| L5-F22 | P2 | 4 | ✅ | Any rectangle wider than 2³¹ grid units is not drawn in release builds and panics in debug builds. |
| L5-F23 | P2 | 4 | ✅ | Every generated app panics at start when any command-line argument is not valid UTF-8. |
| L5-F24 | P3 | 4 | ✅ | Interop doc comments go into generated code unescaped. |

## L6 — Native backends

75 rows: P1 6, P2 37, P3 30, — 2. Source: `layer-solidity/L6-backends.md`.

| ID | Sev | Iter | Status | Finding |
|---|---|---|---|---|
| L6-F1 | P1 | 1 | ✅ | A *caught* panic hands the terminal back while the app keeps running. |
| L6-F2 | P1 | 1 | read | After a contained panic, the TUI loops on the broken tree |
| L6-F3 | P2 | 1 | read | `frame()` runs outside every panic boundary. |
| L6-F4 | P2 | 1 | read | A bracketed paste with no end marker swallows all later input. |
| L6-F5 | P2 | 1 | read | WinHTTP header injection. |
| L6-F6 | P2 | 1 | read | The crash directory is derived from an unvalidated id and falls back to the shared temp dir. |
| L6-F7 | P2 | 1 | read | The Windows updater uses the signed `version` as a directory name and calls `remove_dir_all` on it. |
| L6-F8 | P3 | 1 | read | mitigates L3-F22 in the TUI only |
| L6-F9 | P3 | 1 | read | Linux Secret Service items are found by `{application: <id>, name}` attributes. Secret Service has no per-application isolation, so any process on the session c |
| L6-F11 | P2 | 1 | read | Keyboard shortcuts break on non-Latin layouts on GTK but not on Windows. |
| L6-F10 | — | 1 | — | Terminal output cannot carry an injected escape: controls are their own grapheme clusters (UAX #29 GB4/GB5) with width 0, and `Grid::print` drops zero-width clu |
| L6-F12 | P1 | 2 | read | Whether a panic is survivable depends on the input device. |
| L6-F13 | P2 | 2 | read | 4 of 13 Windows callbacks have no panic boundary |
| L6-F14 | P3 | 2 | read | Any same-session process can inject a deep link |
| L6-F15 | — | 2 | — | GTK already implements L6.6. |
| L6-F16 | P1 | 2 | fixed (fd3a789) | On Windows, `SystemService::open_url` runs whatever it is given. |
| L6-F17 | P2 | 2 | read | Reading the clipboard trusts another process to have NUL-terminated its text. |
| L6-F18 | P2 | 2 | read | The "isolated worker" can read every file the user can and talk to the network. |
| L6-F19 | P2 | 2 | read | Image decoding allocates whatever the file declares. |
| L6-F20 | P3 | 2 | read | Service honesty (U18, U20, U28). |
| L6-F21 | P1 | 2 | ✅ | Writing a text field re-enters the window procedure while the runtime is mutably borrowed, so two `&mut Runtime` coexist. |
| L6-F22 | P2 | 2 | read | Characters outside the BMP are dropped from custom text targets. |
| L6-F23 | P3 | 2 | read | Loop details. |
| L6-F24 | P2 | 2 | read | Drag and drop: no panic boundary, and an unbounded read of the source's text. |
| L6-F25 | P3 | 2 | read | Input and UIA details. |
| L6-F26 | P2 | 2 | read | Every background or border color ever painted leaks one GDI brush for the life of the process. |
| L6-F27 | P3 | 2 | read | Rendering details. |
| L6-F28 | P2 | 2 | read | The framework has no masked (password) text input. |
| L6-F29 | P3 | 2 | read | Windows details. |
| L6-F30 | P2 | 2 | read | GTK has no shutdown or logout path that saves state. |
| L6-F31 | P2 | 2 | read | `native_surface` hands out raw handles that another thread can outlive. |
| L6-F32 | P3 | 2 | read | GTK details. |
| L6-F33 | P2 | 2 | read | Certificate pinning breaks every request with a body on Android. |
| L6-F34 | P3 | 2 | read | Android details. |
| L6-F35 | P3 | 2 | read | A duplicate node key fails differently on each backend. |
| L6-F36 | P2 | 3 | read | Android's drop data and home-screen widgets use U+0001 as an in-band field separator in text other apps or users supply. |
| L6-F37 | P2 | 3 | ✅ | GTK keys its control-measure cache with `format!("{control:?}")`, and an image's `Debug` prints every pixel. |
| L6-F38 | P2 | 3 | read | `Event::WindowCloseRequested` says "the window remains open until the application actually removes it", but every desktop backend closes it anyway. |
| L6-F39 | P2 | 3 | read | Any app on the device can switch on inspection in a release build. |
| L6-F40 | P3 | 3 | ✅ | The terminal backend's per-frame work grows faster than the tree. |
| L6-F41 | P3 | 3 | read | Input details that differ from the other backends. |
| L6-F42 | P3 | 3 | read | Things that only grow. |
| L6-F43 | P3 | 3 | read | Rendering and accessibility details. |
| L6-F44 | P3 | 3 | read | Embedded content skips `UrlPolicy`. |
| L6-F45 | P3 | 3 | read | Unix terminal teardown leaves a window for a stray write. |
| L6-F46 | P3 | 3 | read | The public TUI test harness can pass a test that should fail. |
| L6-F47 | P2 | 3 | ✅ | The Android device suite passes when a run stalls twice. |
| L6-F48 | P3 | 3 | read | Device-suite details |
| L6-F49 | P2 | 4 | ✅ | The kitty keyboard protocol is never switched on, even on a terminal that answers the probe. |
| L6-F50 | P2 | 4 | ✅ | Each stop and continue writes the enter sequences twice, so the title and kitty-flag stacks gain an entry per cycle. |
| L6-F51 | P3 | 4 | read | A Secret Service prompt blocks the GTK main thread for up to 300 s. |
| L6-F52 | P3 | 4 | read | Portal requests can wait forever. |
| L6-F53 | P3 | 4 | read | On the Windows console, a focus change during a paste corrupts the pasted text. |
| L6-F54 | P1 | 4 | read | Leaking an `ExternalLoop` (or the `embed` root) in safe code leaves windows holding a dangling `&mut Application`. |
| L6-F55 | P2 | 4 | read | Windows notifications stop working after the blocking thread that created their window retires. |
| L6-F56 | P2 | 4 | read | Any same-user process can squat on, hang, or intercept the single-instance channel. |
| L6-F57 | P2 | 4 | read | Any Android app can forge a widget, notification, tile, or shortcut action. |
| L6-F58 | P2 | 4 | read | Shared items reach the app unfiltered ("Dirty Stream"). |
| L6-F59 | P2 | 4 | read | Android self-update reports success and never installs. |
| L6-F60 | P3 | 4 | read | Unpinned WinHTTP requests follow cross-host redirects with the caller's headers. |
| L6-F61 | P2 | 5 | ✅ | In the terminal, a child of an `Overflow::Visible` container inside a scroll container ignores the scroll viewport, so clicks on what is visible go to a control scrolled out of view. |
| L6-F62 | P2 | 5 | ✅ | Terminal label text, text-field and select underline fills, and the caret use the node's rectangle, not its clip. |
| L6-F63 | P2 | 5 | ✅ | A terminal list box with a selection cannot be wheel-scrolled. |
| L6-F64 | P3 | 5 | ✅ | Tabs take zero cells in the terminal. |
| L6-F65 | P3 | 5 | read | Terminal scroll containers draw their scroll bar over the content's last column |
| L6-F66 | P3 | 5 | read | A custom text target cannot place the IME candidate window. |
| L6-F67 | P2 | 5 | read | Android `finishComposingText` throws away the word being composed. |
| L6-F68 | P2 | 5 | read | After process death, an Android secondary window's task comes back as the primary window. |
| L6-F69 | P2 | 5 | read | An Android native surface is destroyed with no event. |
| L6-F70 | P2 | 5 | read | The Windows canvas keeps the DPI it was created at. |
| L6-F71 | P2 | 5 | read | The Windows tray icon is gone for good after Explorer restarts. |
| L6-F72 | P3 | 5 | read | Android PendingIntents alias and notification ids repeat. |
| L6-F73 | P3 | 5 | read | Android virtual accessibility ids are list positions |
| L6-F74 | P3 | 5 | read | IME delivery differs across backends. |
| L6-F75 | P3 | 5 | read | Smaller Windows and GTK details. |

## L7 — Web

33 rows: P1 3, P2 14, P3 15, — 1. Source: `layer-solidity/L7-web.md`.

| ID | Sev | Iter | Status | Finding |
|---|---|---|---|---|
| L7-F1 | P1 | 1 | read | Link and form URLs are never scheme-checked. |
| L7-F2 | P2 | 1 | read | A font family containing a comma is emitted raw into the stylesheet. |
| L7-F3 | P3 | 1 | read | Attribute names are never validated. |
| L7-F4 | P2 | 1 | read | The client-subset translator has scenario-only differential tests. |
| L7-F5 | — | 1 | — | One escaper covers text and attribute values; `json_for_script` escapes `<`, `>`, `&`, U+2028, and U+2029. Inline `<style>` neutralises `</` and carries a nonce |
| L7-F6 | P1 | 2 | ✅ | The offline call queue replays `/_fn/` POSTs without idempotency, without expiry, and under whoever is signed in later. |
| L7-F7 | P2 | 2 | read | The service worker caches personalised pages that the server marked `no-store`. |
| L7-F8 | P2 | 2 | read | Server-interactive mode ships the root component's inspector state to every browser. |
| L7-F9 | P3 | 2 | read | Web runtime details. |
| L7-F10 | P3 | 2 | read | Translator and image details. |
| L7-F11 | P1 | 3 | ✅ | Form and query decoding is quadratic in the number of fields, and it runs on the server's async workers. |
| L7-F12 | P2 | 3 | ✅ | The client subset compiles ordinary Rust into JavaScript that means something else, and it raises no error. |
| L7-F13 | P2 | 3 | read | The server's responsive-image store never evicts anything. |
| L7-F14 | P2 | 3 | read | The WebAssembly subtree's service bridge corrupts bytes. |
| L7-F15 | P3 | 3 | read | Native client components never unsubscribe from topics. |
| L7-F16 | P3 | 3 | read | Browser runtime details. |
| L7-F17 | P3 | 3 | read | Render and translator details. |
| L7-F18 | P3 | 3 | read | Doc comments with mojibake. |
| L7-F19 | P2 | 4 | ✅ | Streamed pages can give islands each other's state and module. |
| L7-F20 | P2 | 4 | ✅ | Hoisted statements change evaluation order in compiled client code. |
| L7-F21 | P2 | 4 | ✅ | `move` closures capture by reference in compiled client code. |
| L7-F22 | P3 | 4 | ✅ | Three more numeric divergences. |
| L7-F23 | P3 | 4 | read | `sw.js` differs from process to process, so every restart looks like a new build. |
| L7-F24 | P3 | 4 | read | Precached pages are never served offline. |
| L7-F25 | P2 | 5 | ✅ | A form submitted without JavaScript sends only text inputs, checkboxes, and buttons. |
| L7-F26 | P2 | 5 | ✅ | All radios of one component form one browser radio group. |
| L7-F27 | P2 | 5 | ✅ | Going back to a history entry the runtime did not create leaves the previous page on screen. |
| L7-F28 | P2 | 5 | ✅ | Converting a whole-number float to an integer wraps in JS where Rust saturates. |
| L7-F29 | P3 | 5 | ✅ | Struct-update syntax evaluates `..base` before the explicit fields. |
| L7-F30 | P3 | 5 | ✅ | Horizontal virtual lists never advance under `dir=rtl`. |
| L7-F31 | P3 | 5 | ✅ | Moving a keyed item up loses keyboard focus. |
| L7-F32 | P3 | 5 | ✅ | Clicking a checkbox's or radio's label text sends `Click` to the parent container. |
| L7-F33 | P3 | 5 | read | The browser sends `Composition` events for native text inputs |

## L8 — Application layer

18 rows: P1 2, P2 7, P3 8, — 1. Source: `layer-solidity/L8-app.md`.

| ID | Sev | Iter | Status | Finding |
|---|---|---|---|---|
| L8-F1 | P2 | 1 | read | A truncated disk-cache write breaks an image permanently. |
| L8-F2 | P2 | 1 | read | Image memory and disk are bounded by time only. |
| L8-F3 | P3 | 1 | read | Disk-cache keys are 64-bit FNV of the URL. |
| L8-F4 | P3 | 1 | read | `chart.rs` has no `is_finite` or NaN handling. A NaN or infinite data point flows into layout and drawing as is; together with L5-F12, an infinite value makes t |
| L8-F5 | — | 1 | — | `Retry` retries only idempotent requests and only on transport failure, with bounded attempts. Query back-off saturates (`checked_shl`, `saturating_mul`). Image |
| L8-F6 | P1 | 2 | ✅ | `VersionedStore` destroys data on a rollback, and silently nulls any value that is not JSON. |
| L8-F7 | P2 | 2 | read | A load error empties the offline mutation queue and persisted tables, and the next write makes it permanent. |
| L8-F8 | P2 | 2 | read | The native offline mutation queue has the same replay flaws as the web one (L7-F6). |
| L8-F9 | P2 | 2 | read | Autosave overwrites changes made outside the application. |
| L8-F10 | P3 | 2 | read | L8-F4 is partly refuted. |
| L8-F11 | P1 | 3 | ✅ | Sorting a `DataTable` column that mixes numbers and text panics in `render`. |
| L8-F12 | P2 | 3 | ✅ | `BearerAuth` sends the access token to whatever URL a request names, and endpoint parameters are spliced into the path unescaped. |
| L8-F13 | P2 | 3 | ✅ | An invalidation that arrives while a fetch for the key is in flight is lost. |
| L8-F14 | P3 | 3 | ✅ | `ListSelection::set_count` clamps `focused` and `selected` but not `anchor`. |
| L8-F15 | P3 | 3 | read | Component details. |
| L8-F16 | P3 | 3 | read | Data-layer details. |
| L8-F17 | P3 | 5 | ✅ | The chart's screen-reader text invents values. |
| L8-F18 | P3 | 5 | read | `Toast` matches an expiring timer to its message by text |

## L9 — Server and distributed

57 rows: P0 2, P1 9, P2 19, P3 22, — 5. Source: `layer-solidity/L9-server.md`.

| ID | Sev | Iter | Status | Finding |
|---|---|---|---|---|
| L9-F1 | P0 | 1 | ✅ | Live sessions can be hijacked by guessing their id. |
| L9-F2 | P0 | 1 | ✅ | The resume snapshot is client-supplied and trusted. |
| L9-F3 | P1 | 1 | read | Live events bypass disabled and hidden state. |
| L9-F4 | P2 | 1 | read | The Live socket is unauthenticated, with only coarse resource bounds. |
| L9-F5 | P3 | 1 | read | `Cookie` `Display` writes `name=value` without validating either. A value containing `;` adds attributes (`; Domain=…`), though `HeaderValue` blocks CR/LF. The |
| L9-F6 | — | 1 | — | HTTP security is strong by default. |
| L9-F7 | P1 | 2 | read | The MQTT broker has no authentication, no authorization, and no transport security. |
| L9-F8 | P2 | 2 | fixed (3485eb2; see L9-F43) | The server has no header-read timeout, no connection limit, and no handler or render deadline. |
| L9-F9 | P2 | 2 | read | A cached "public" route can serve one visitor's personalised page to everyone. |
| L9-F10 | P2 | 2 | fixed (4e916b8) | MQTT reads allocate whatever length the client declares |
| L9-F11 | — | 2 | — | Admin routes are behind `.authorized::<P, Pol>()`, with table and column names from the model and bound values. Path and form extraction parse defensively, and |
| L9-F12 | P1 | 2 | read | One peer can poison every replica's clock forever. |
| L9-F13 | P3 | 2 | read | Durable and DB details. `db::open` ignores a failed `journal_mode=WAL` (`let _ =`, U4). `schema.rs` formats table names into `PRAGMA table_info("{name}")` (deve |
| L9-F14 | — | 2 | — | The durable workflow design is sound |
| L9-F15 | P1 | 2 | ✅ | Push requests go to whatever URL the subscriber supplied, with the server's credentials attached. |
| L9-F16 | P2 | 2 | fixed (20fc4fa, 5633bfa) | The local deployment's control API is open to any web page. |
| L9-F17 | P2 | 2 | read | Row policies can be bypassed through their own `columns` argument. |
| L9-F18 | P2 | 2 | read | The job queue is unsafe with more than one worker, and one panicking job stops the queue. |
| L9-F19 | P3 | 2 | read | Cron semantics differ from cron. |
| L9-F20 | P3 | 2 | read | `RUSTNATIVE_DEV` changes a release server. |
| L9-F21 | P3 | 2 | read | Silent defaults in the auth and config paths. |
| L9-F22 | P3 | 2 | read | OAuth and OIDC are half done. |
| L9-F23 | P3 | 2 | read | Serverless and proxy edge cases. |
| L9-F24 | P3 | 2 | read | `query!` checks against a different schema than the one that runs (U21). |
| L9-F25 | P3 | 2 | read | API-tooling details. |
| L9-F26 | — | 2 | — | Password hashing is Argon2id in PHC form with a random salt. Bearer verification is constant-time HMAC-SHA256 with expiry. OAuth uses PKCE S256 and a constant-t |
| L9-F27 | P1 | 2 | ✅ | Sync has no authorization and trusts the client's conflict metadata. |
| L9-F28 | P2 | 2 | ✅ | A sync server restart makes every client blind. |
| L9-F29 | P1 | 2 | read | Long-running operations are readable and cancellable by anyone who can count. |
| L9-F30 | P2 | 2 | read | Actor storage turns read errors into empty state. |
| L9-F31 | P2 | 2 | read | One panicking or undecodable event stalls the event queue for good. |
| L9-F32 | P2 | 2 | read | The OTLP exporter buffers without bound. |
| L9-F33 | P3 | 2 | read | Telemetry carries URLs and raw paths. |
| L9-F34 | P3 | 2 | read | Sync and device details. |
| L9-F35 | P3 | 2 | read | The edge actor adapter accepts any message for any actor id from any requester, and offers no authentication hook. `LocalActorSystem::ask` can lose a message th |
| L9-F36 | — | 2 | — | `supervise` catches panics and follows the policy's backoff. The edge alarm is taken once. Consent for telemetry defaults to off, and nothing leaves without it |
| L9-F37 | P1 | 2 | ✅ | The MQTT broker silently drops "exactly once" messages once a client reuses a packet id. |
| L9-F38 | P2 | 2 | read | A partially prerendered shell is cached by path, from the first visitor's props, without bound. |
| L9-F39 | P2 | 2 | read | A failed saga compensation is never retried. |
| L9-F40 | P3 | 2 | read | Database layer details. |
| L9-F41 | P3 | 2 | read | Pipeline and admin details. |
| L9-F42 | P3 | 2 | read | MQTT and CRDT details. |
| L9-F43 | P1 | 3 | ✅ | Idle keep-alive connections hold `max_connections` slots forever, so 1 024 idle sockets lock out the server. |
| L9-F44 | P2 | 3 | read | An MQTT client that asks for keep-alive 0 holds a broker slot for 24 hours. |
| L9-F45 | P3 | 3 | read | The inspector's pre-token deadline is per line, and blank lines are skipped, so 16 unauthenticated clients hold every inspector slot. |
| L9-F46 | P3 | 3 | read | Listener and URL policy details. |
| L9-F47 | P2 | 4 | ✅ | An MQTT client that reconnects while its old connection is still open is disconnected, loses its subscriptions, and has its will published while it is online. |
| L9-F48 | P3 | 4 | ✅ | Resuming a Live session while the old socket is still attached disconnects both. |
| L9-F49 | P2 | 4 | read | The durable queues never delete finished rows and have no index on the claim predicate, so every claim scans the whole history. |
| L9-F50 | P3 | 4 | read | An `OrSet` replica that starts fresh re-issues dots that were already removed. |
| L9-F51 | P1 | 5 | ✅ | Two runners of one `LocalEngine` execute the same step twice, and the loser compensates the work and marks a successful execution failed. |
| L9-F52 | P2 | 5 | ✅ | A step's failure is not journaled, so a workflow that handles a step error re-runs the step on every replay and then diverges. |
| L9-F53 | P2 | 5 | ✅ | A workflow waiting for a signal is replayed from the top on every tick. |
| L9-F54 | P2 | 5 | ✅ | The HTTP rate limiter is keyed by the full IPv6 address, so one client has 2⁶⁴ buckets. |
| L9-F55 | P3 | 5 | read | `TracedHttp::within` stores the parent span on the shared service, not per request |
| L9-F56 | P3 | 5 | read | `fire_alarms` deletes by actor id only |
| L9-F57 | P3 | 5 | read | `query!` cannot select a column named with a Rust keyword. |

## L10 — Tooling

29 rows: P1 3, P2 9, P3 16, — 1. Source: `layer-solidity/L10-L11-cli-ci.md`.

| ID | Sev | Iter | Status | Finding |
|---|---|---|---|---|
| L10-F1 | P1 | 1 | read | `rustnative fmt` deletes the user's comments and rewrites files non-atomically. |
| L10-F2 | P2 | 1 | read | The LSP measures columns in Unicode scalars, not UTF-16 code units. |
| L10-F3 | P3 | 1 | read | `serve_static_on` serves one connection at a time (one slow client stalls the preview server). Paths are not percent-decoded, so an exported file whose name con |
| L10-F5 | P3 | 1 | read | The PFX password reaches `signtool` as a command-line argument |
| L10-F4 | — | 1 | — | Dev, preview, and emulator servers bind loopback by default. The static server refuses `..`, `\`, and `:`. Request heads are capped at 64 KiB. The dev error pag |
| L10-F6 | P1 | 2 | read | The generated sign-up makes the first account to register an administrator. |
| L10-F7 | P2 | 2 | read | The commerce kit's routes are keyed by a buyer named in the path and are public. |
| L10-F8 | P3 | 2 | read | CLI details. |
| L10-F9 | P1 | 3 | ✅ | `rustnative db rollback <db> --to <name>` with a name that is not applied reverses every migration and exits 0. |
| L10-F10 | P2 | 3 | ✅ | The `.rsx` LSP proxy loses edits for good once a document stops lowering. |
| L10-F11 | P2 | 3 | ✅ | `rustnative tokens import` replaces a style file it cannot read with only the tokens block. |
| L10-F12 | P2 | 3 | read | `rustnative update keygen` writes the update-signing secret key world-readable, and overwrites an existing key. |
| L10-F13 | P2 | 3 | read | `rustnative dev --remote` sends the token and every rebuilt executable in cleartext, and the agent serves one connection at a time with no timeout. |
| L10-F14 | P3 | 3 | ✅ | The `upgrade` codemod rewrites look-alike types and skips macro bodies, and reports neither. |
| L10-F15 | P3 | 3 | ✅ | `rustnative doctor` fails on every machine that is not Windows. |
| L10-F16 | P3 | 3 | read | Packaging details. |
| L10-F17 | P3 | 3 | read | The local Lambda and WAGI emulators diverge from their hosts in ways that hide bugs. |
| L10-F18 | P3 | 3 | read | Dev loop, generators, and `db`. |
| L10-F19 | P3 | 3 | read | Smaller CLI details. |
| L10-F20 | P2 | 4 | ✅ | `rustnative package tui` ships packages that cannot start, for every app made from the project template. |
| L10-F21 | P3 | 4 | ✅ | `db migrate` and `db rollback` silently create a database at a mistyped path and exit 0. |
| L10-F22 | P3 | 4 | read | `rustnative build --pgo` passes paths through `RUSTFLAGS` and drops the project's own rustflags. |
| L10-F23 | P3 | 4 | read | Smaller CLI details. |
| L10-F24 | P2 | 5 | ✅ | `rustnative compliance` reports accessibility "passed" when every accessibility test fails. |
| L10-F25 | P2 | 5 | ✅ | `rustnative test`, `run`, `build`, `check`, and `dev` delete every stdout line that is valid JSON. |
| L10-F26 | P3 | 5 | ✅ | `rustnative i18n lint` checks only the direct literal form. |
| L10-F27 | P3 | 5 | ✅ | `rustnative doctor` reports Android ready when the build will fail. |
| L10-F28 | P3 | 5 | ✅ | `rustnative add` corrupts a CRLF `Cargo.toml`. |
| L10-F29 | P3 | 5 | read | The dev loop and the LSP ignore `[style] file`. |

## L11 — Project, CI, and release

6 rows: P1 1, P2 2, P3 2, — 1. Source: `layer-solidity/L10-L11-cli-ci.md`.

| ID | Sev | Iter | Status | Finding |
|---|---|---|---|---|
| L11-F1 | P1 | 1 | read | No dynamic-analysis job. |
| L11-F2 | P2 | 1 | read | Skip-if-absent instruments are never required in CI |
| L11-F3 | P2 | 1 | read | `rust-toolchain.toml` says `channel = "stable"` (unpinned), and CI uses `dtolnay/rust-toolchain@stable`. A new stable release can change lints and break `-D war |
| L11-F4 | P3 | 1 | read | Third-party actions are pinned by mutable tags (`actions/checkout@v4`, `Swatinem/rust-cache@v2`, `taiki-e/install-action@cargo-deny`, `dtolnay/rust-toolchain@st |
| L11-F5 | P3 | 1 | read | `windows-cross-wine` is disabled. macOS runs `cargo check` only (TUI tests "owed to a Mac"). Real-cursor hover and two Android signing tests are `--skip`ped on |
| L11-F6 | — | 1 | — | `-D warnings` applies to rustc and rustdoc. Clippy runs with `--all-features` on Linux and Windows. An MSRV job exists. `cargo deny` (licenses, sources, yanked) |

