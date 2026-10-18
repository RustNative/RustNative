# How the engineering standards were derived

`STANDARDS.md` is the result of a question loop run over the whole
repository in two rounds. This page records the loop: its questions, the
evidence each round ran on, what each pass found, and why it stopped. A
proposal to change the standards answers one of the same questions
(`STANDARDS.md` section 38).

## The questions

Each domain was taken through nine questions, in this order:

| # | Question | What it produces |
|---|---|---|
| Q1 | **What?** What does this domain mean for this framework specifically? | The scope |
| Q2 | **Why?** What breaks, and for whom, if it is not held? | The rationale |
| Q3 | **Covers enough?** Does the scope reach every crate, backend, language, target, and lifecycle stage? | Gaps in scope |
| Q4 | **Explore more?** What adjacent concern does this one touch that has no home yet? | New domains or standards |
| Q5 | **How to enforce?** Which mechanism holds each standard? | The enforcement |
| Q6 | **Why these methods?** Why this rung of the ladder (E1–E6) and not another? | The rung, justified |
| Q7 | **Is this enforcement enough?** Can the standard be broken with every mechanism green? | Holes in enforcement |
| Q8 | **Are there better ways?** Can a higher rung hold it: a type, a denied lint, a generated artifact? | Promotions |
| Q9 | **Ask again from the beginning?** Did this pass find anything at all? | Another pass, or the stop |

Q9 makes the sequence a loop. Every finding of a pass (a new domain, a
scope gap, an enforcement hole, a promotion, a defect) went into the next
pass as input. The loop stopped at the first pass in which all nine
questions, for every domain, produced nothing new.

## Two rounds, and why the second was needed

The first round (passes 1–4) asked the questions against what the
repository *says*: every tracked file was read by script for known risk
patterns, and the configuration, policy, and security documents were read
in full. It produced 31 domains and stopped when a pass found nothing new.

Its stop was premature, and the reason is a lesson the standards now
encode. A scan by pattern can only find what its patterns name, so Q7 ("is
this enforcement enough?") was answered by reading enforcement, not by
testing it. The second round (passes 5–9) asked the same questions against
what the repository *does*. It compiled, linted, ran, fuzzed, and attacked
the code, and read the security-relevant modules in full. It found the
most serious defects of either round, two new domains, about forty new
standards, and two mistakes in the first round's own claims. That is the
origin of ladder rule 5 ("evidence is executed").

## The evidence

### Round 1: what the repository says

At commit `c2a2e67`:

- **Every tracked file, by script.** Each of the 1,076 files was measured
  for `unsafe` and `SAFETY:` comments, panicking calls, suppressions with
  and without reasons, crate attributes, printing, statics, process exits,
  threads and sleeps, `transmute`, host `cfg`s, documentation and
  `forbid(unsafe_code)` declarations, tests and `#[ignore]` reasons,
  unbounded channels, lossy casts, credential-like literals, plain-HTTP
  hosts, traceability tags, and text hygiene (encoding, line endings,
  final newlines, control bytes).
- **Every manifest and configuration file in full:** 60 `Cargo.toml` files,
  `Cargo.lock`, `clippy.toml`, `deny.toml`, `rustfmt.toml`,
  `rust-toolchain.toml`, `.gitignore`, `.gitattributes`, the CI workflow,
  and `budgets/`.
- **Every governance and policy document in full**, `PLAN.md` sections 2
  and 9 and Milestone 65, and the tests that already police the
  repository's documentation (`doc_claims.rs`, `doc_parity.rs`,
  `describe.rs`).
- **External references,** checked on 2026-10-18: WCAG 2.2 and the WCAG 3
  draft, SLSA v1.1/v1.2, `cargo-semver-checks`, OpenSSF Scorecard, the EU
  Cyber Resilience Act's dates, Contributor Covenant 3.0, Rust 1.81's
  `#[expect]` and `allow_attributes_without_reason`, and the rust-lang/rust
  LLM policy of August 2026.

### Round 2: what the repository does

- **Compilation and extended lints on three targets.** The CI clippy gate
  ran on Linux and as cross-checks for `x86_64-pc-windows-msvc` and
  `aarch64-linux-android`, on Rust 1.97 and 1.99.1. About 60 additional
  lints ran over every library and binary, among them `missing_docs`,
  `allow_attributes_without_reason`, `string_slice`,
  `let_underscore_must_use`, `map_err_ignore`,
  `significant_drop_in_scrutinee`, `multiple_unsafe_ops_per_block`,
  `mem_forget`, `panic`, `as_conversions`, `print_stderr`, and
  `non_send_fields_in_send_ty`. Results were counted per lint and per
  crate, and the high-risk sites were read one by one.
- **Tools:** `cargo deny`, `cargo audit`, `cargo machete`, `shellcheck`,
  `zizmor`, `actionlint`, and Miri (nightly 2026-10-17).
- **The test suite,** run workspace-wide three times, once with this
  machine's `RUST_BACKTRACE=1` and twice without, plus the GTK backend's
  suite under Xvfb, before and after installing GStreamer's base plug-ins.
- **Fuzzing.** A scratch crate drove eleven parsers and decoders with 2.5
  million generated inputs: MQTT packets, the terminal input decoder (raw
  bytes and an escape-sequence alphabet), the colour, value, and shadow
  parsers, the style sheet parser, the `rsx!` front end, the update
  manifest, version comparison, and the terminal's URL check.
- **Probes,** each a small program running the real code against a
  suspected defect: cross-site requests to the deploy control API,
  non-ASCII URLs, the inspection token's file mode, oversized MQTT headers
  under a memory limit, and accept-loop behaviour under a descriptor limit.
- **Reading in full:** the server's `security.rs`, sessions, tokens,
  passwords, passkeys, OAuth, body and request handling, the accept loop,
  the deploy adapter and its control API, the update verifier, the live
  session upgrade path, the inspection server, the URL openers of every
  backend, the HTML escapers, the SQL builders, the job queue, the HTTP
  clients and certificate pins, the Java bridge's exception handling, the
  signal handlers, the Windows user-data accessors, and all 47
  host-callable `extern` functions.

---

## Round 1

### Pass 1: the seed list

The seed list came from the request: stability, reliability, security,
performance, and developer experience. Each was taken through Q1–Q9.

- **Stability.** Q1: the framework never takes down the process it lives
  in. Q3: the word mixed three things, crash-freedom (STB), API stability
  (CMP), and determinism (COR), so they were split. Q7: `clippy::panic` and
  `indexing_slicing` are allowed. Q8: promote `panic`, scan for
  `process::exit`.
- **Reliability.** Q1: correct behaviour under failure. Q4: idempotency,
  offline, lifecycle, and data integrity (DAT) had no home. Q7: the MQTT
  broker's allocation before any check was noticed by reading.
- **Security.** Q1: secure defaults, untrusted input, cryptography,
  authorization, code loading, secrets, disclosure. Q4: privacy (PRV) and
  supply chain (SUP) split out. Q7: a bare-name `LoadLibraryW`. No fuzzing.
- **Performance.** Q1: declared budgets per target. Q7: a budget can be
  raised in the change that regresses it. Q8: CODEOWNERS on `budgets/`,
  an idle-wakeups key.
- **Developer experience.** Q3: it covered application developers and left
  out contributors (VCS, CI, GOV) and documentation (DOC). Q4: diagnostics,
  configuration, Cargo features, the CLI as a stable interface,
  observability.

Q3 and Q4 grew the list to 31 domains. Q9: yes.

### Pass 2: every domain

Each of the 31 domains was taken through Q1–Q9 against the round-1
evidence. Pass 2 added source hygiene (control bytes in three doc
comments), every non-Rust language, environment mutation, generated
artifacts, configuration and features, energy, regulatory obligations, and
AI-assisted contributions. Q9: yes.

### Pass 3: the standards as a whole

The questions were asked of the standards themselves. That added the
waiver process, the process for changing the standards, the ratchet rule,
and the second-catch rule. Q9: yes.

### Pass 4: the first stop

No domain produced a new finding against the round-1 evidence, and the
loop stopped. Round 2 shows the stop was a limit of the evidence, not of
the questions.

---

## Round 2

### Pass 5: Q7 asked with tools

Pass 5 re-asked Q7 ("can the standard be broken with every mechanism
green?") for every domain by running something that would break it.

| Domain | What was run | What it found |
|---|---|---|
| CI, SUP | The CI clippy gate on Rust 1.97 and 1.99.1 | It fails on 1.97 and passes on 1.99.1 because `rust-toolchain.toml` says `stable` (F-37). |
| CI, QLT, MEM | Extended lints on Linux, then on the Windows and Android targets | 38 reasonless suppressions and 42 multi-operation `unsafe` blocks exist only in `#[cfg(windows)]` code, which a Linux pass never compiles. |
| API | `missing_docs` workspace-wide | 19 hits, almost all in examples and generated code. The nine "ungated" crates are documented. **The round-1 claim was corrected.** |
| NET, SEC | Cross-site `text/plain` POSTs to the deploy control API | 200 and 200, and the attacker's revision took 100% of traffic (F-01). |
| NET, STB | 5-byte MQTT headers declaring 256 MiB, under a 3 GiB limit | 16 connections aborted the process (F-02). |
| NET | Client connections under a 48-descriptor limit | `serve()` returned `EMFILE` after 19 connections and stopped serving (F-03). |
| SEC | `stat` on the published inspection endpoint file | Mode `644` in a `755` directory under `/tmp`, holding the token (F-08). |
| COR, TST | The URL checker with non-ASCII input, then 300,000 generated inputs | `"mailto:€"` panics, and 5 distinct panics were found (F-05). |
| TST, SEC | 2.2 million inputs to ten other parsers and decoders | No panics. **The parsers are robust, which is a finding too:** it makes fuzz targets cheap to adopt. |
| MEM | Miri over four crates, then the core | No undefined behaviour in the four. In the core, Miri stops at `parking_lot_core` 0.9.12's `futex` call, a dependency's argument type, after the identity and layout tests pass. |
| SUP | `cargo deny`, `cargo audit`, `cargo machete` | Clean advisories and licences. Four crates declare unused dependencies, and two duplicates are unexplained. |
| SUP | `zizmor` on the workflow | 43 tag-pinned actions, 13 persisted credentials, 2 floating images (F-35). |
| QLT | `shellcheck` | 15 findings. |
| TST | The full test suite | The first run exhausted a 39 GB disk allowance (TST-16). Three clean runs gave identical results: 1,183 passed, 0 failed, 12 ignored, no flakes. |
| TST | The suite with `RUST_BACKTRACE=1`, this machine's default | Two terminal tests fail because the backtrace scrolls the message they look for off the screen (F-40). |
| REL | The GTK backend's tests under Xvfb | GTK's media module aborted the whole test binary when GStreamer's `playbin` was missing (F-41). |
| CRY, REL | The GTK backend's tests behind this machine's HTTPS proxy | The pin test failed: with no TLS handshake to inspect, the pin check never ran, and the proxy's `403` came back as the pinned host's response (F-42). The test's own skip logic would have hidden this on an offline runner. |

Q9: yes, and the defects were serious enough to restart every domain from
Q1 with the code read in full.

### Pass 6: Q1–Q4 again, with the code read in full

Reading the security-relevant modules, rather than grepping them, changed
the answers to Q3 and Q4 for several domains.

- **Q3 for SEC: does it cover developer tooling?** No. The deploy control
  API, the dev agent, and the inspection server are developer tools, and
  they held the critical and high findings (F-01, F-08, F-09). None had a
  threat-model row. SEC-2 was widened to cover tooling.
- **Q4 for SEC: what has no home?** Two whole areas. Listeners became a
  domain of their own (**NET**): loopback as a non-boundary, size before
  allocation, deadlines, accept loops, one choke point per check, IPv6
  rate-limit keys, graceful shutdown, hostile-peer suites, outbound
  limits. Cryptography and signing became **CRY**: vetted primitives (three
  hand-written constant-time comparisons), typed keys (any-length token
  keys), purpose binding (no associated data, no audience), rotation (one
  pinned update key), SPKI pins with backups (whole-certificate pins),
  timestamped signatures (none), and strict verification.
- **Q3 for NAT: does capability honesty cover services?** No. Capability
  tables cover UI. A service contract's *behaviour* differed by backend:
  `open_url` validates on one backend of five (F-04), and HTTP timeouts and
  limits differ (F-24). NAT-7 was added.
- **Q4 for ARC: what does the duplication of security code imply?** HTML
  escaping exists twice, constant-time comparison three times, durable
  writes twice with different guarantees, and the origin check at one of
  two entry points. ARC-12 was added: one implementation per primitive.
- **Q4 for STB: the 47 host-callable functions.** Enumerating them all
  showed the main entry points guarded and about 15 secondary callbacks
  not (F-31). It also showed a model signal handler. STB-2 was sharpened,
  and STB-8 (allocation size) and STB-9 (signal safety) were added.
- **Q4 for REL:** the job queue's single-instance recovery (F-33) gave
  REL-8. The Android Play check failing open (F-17) and the Linux pin
  check failing open (F-42) gave REL-9, stated as a principle: a security
  check accepts on a positive verdict, never on the absence of a negative
  one. A host plug-in whose absence aborts the process (F-41) extended
  REL-4.
- **Q4 for CON and OBS:** a waker and an exporter called under locks
  (F-30) gave CON-9 and OBS-6.
- **Q4 for MEM and RES:** a `mem::forget` that leaks a D-Bus subscription
  per notification (F-19) gave MEM-10 and RES-6. The Windows user-data
  accessors gave MEM-9, a standard written from a positive example.
- **Q4 for API:** sign-in in the framework's own template skips the step
  its docs require (F-16), giving API-10 and SEC-15. 220 of 257 public
  enums exhaustive, `Node` among them, sharpened API-4.
- **Q4 for DOC:** three rustdoc claims are false and untested (F-07, F-23,
  F-25). The repository's "no claim without a test" rule had reached
  Markdown only, so DOC-7 was added.
- **Q4 for DX and CI:** 18 undocumented environment variables gave DX-4. A
  contributor's inability to reproduce CI's verdict gave DX-8, SUP-11, and
  CI-9.
- **Q4 for TST:** every reproduced defect came from a test that played the
  attacker, which the suite had none of, so TST-14 was added. Miri being
  clean gave TST-15, the disk exhaustion gave TST-16, and the backtrace-dependent
  failures gave TST-17 (hermetic tests).

Q9: yes.

### Pass 7: Q5–Q8 for the new and changed standards

For each standard added or changed in pass 6, Q5 named its mechanism and
Q6 its rung. Q7 asked whether the mechanism could pass with the defect
present, and Q8 looked for a higher rung.

- **Types where the defect class allows them (E1).** These are
  `Unverified<T>` (SEC-16), typed SQL identifiers (SEC-17), typed keys
  (CRY-2), a pin type requiring two SPKI digests (CRY-5), a `PathComponent`
  (DAT-6), a `Json<T>` that refuses other content types and a loopback
  builder that cannot be built without a token (NET-1), and a one-call
  `sign_in` (API-10). A test can be skipped; a type cannot.
- **Lints where clippy already has the check (E2).** The 11 lint
  promotions in `STANDARDS.md` section 37, item 8, each with its measured
  count, so the cost of turning it on is known.
- **Tests that attack (E3)** where neither types nor lints reach: the
  hostile-peer suites (NET-8), fault injection for fail-closed decisions
  (REL-9), two-instance tests (REL-8), file-mode tests (SEC-14), and the
  boundary scan over `extern` functions (STB-2).
- **Q7 for those tests: can they pass with the defect present?** Only if
  they run on the wrong target. That made CI-9 ("every check runs on every
  target that compiles the code") a precondition of the others.

Q9: yes, CI-9 was new.

### Pass 8: the standards as a whole, again

- **Corrections.** Two round-1 claims were wrong. "No first-party parser
  bounds its recursion depth" was refuted by the passkey CBOR reader's
  16-level limit. "Nine crates lack documentation" became "nine crates
  are documented but not gated". Both were corrected in the standards, and
  the findings register keeps a gap (F-32) where the retired claim was, so
  the correction is visible.
- **Positive findings belong in the standards.** The well-made parts (the
  signal handler, the user-data accessors, the JNI guard, the CSRF check,
  the verify-then-parse update path, the parsers that survived fuzzing)
  are named as models. A standard with a model in the codebase is easier
  to follow than one without.
- **The register.** Findings were given IDs and a severity, and each
  standard cites the findings that break it. A contributor can then see
  the rule's cost here, and a fixed finding leaves the register in the
  change that fixes it.
- **Ladder rule 5.** "Evidence is executed" was added to the ladder: a
  reading proposes a finding and a run confirms or retires it.

Q9: yes, the rule and the register were new.

### Pass 9: the stop

Pass 9 took all 33 domains and the cross-cutting rules through Q1–Q9 once
more, against both rounds' evidence and the standards as written.

- **Q1–Q2:** every domain had a scope and a rationale.
- **Q3:** every crate, backend, target (including the `cfg`-gated
  Windows, Android, and wasm code), non-Rust language, developer tool,
  listener, and lifecycle stage was covered by at least one domain.
- **Q4:** no adjacent concern without a home was found.
- **Q5–Q6:** every standard named its mechanism and rung.
- **Q7:** every hole found by running the code was either closed by an
  existing mechanism or listed as owed with its finding.
- **Q8:** every promotion found was recorded in the ordered list of
  owed mechanisms.
- **Q9:** no new findings, so the loop stopped.

## What would restart the loop

The loop restarts whenever its inputs change: a new crate, backend,
target, language, listener, format, or developer tool; a new external
requirement; a waiver that keeps recurring; or a defect that reaches a
release, which shows that Q7 was answered wrongly somewhere. A restart
uses round 2's method: compile, lint every target, run, fuzz, attack, and
read in full.
