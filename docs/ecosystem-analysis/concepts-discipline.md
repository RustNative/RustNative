# Concepts VIII — engineering discipline (L7–L9)

Continues [`concepts-core.md`](concepts-core.md) (format, inclusion rule,
scoring). Requirement identifiers are `Cnn-k`.

These are the practices that keep a framework's *own* promises true as it
grows, and that its users inherit. Every one of them is invisible in a demo and
decisive after the second year — which is exactly why the archetypes that have
them rarely advertise them, and the ones that do not are found out slowly.

---

## C128 — Public API surface review and version enforcement

**Introduced by.** W9, D1, D5, and the language ecosystems with formal
compatibility rules.

**Mechanism.** The public surface is an artifact: it is extracted, committed,
and diffed on every change, so a pull request shows exactly what it adds,
changes, and removes. A tool decides whether the diff is compatible with the
declared version bump and fails the build when it is not. Types are sealed by
default — non-exhaustive enums, private fields, opaque handles — so that
additive change stays additive rather than becoming a breaking change by
accident.

**Strengths.** The stability policy becomes mechanical rather than aspirational;
reviewers see API change as its own diff; accidental breakage is caught before
release, not by users.

**Weaknesses.** Surface snapshots are noisy at first and need discipline to
keep meaningful; sealing everything makes some legitimate uses awkward.

**Opportunities.** Milestone 52 publishes a stability policy with a deprecation
window and ships codemods, and `rustnative describe --json` already emits a
machine-readable description of elements, attributes, capabilities, and events.
That description is most of an API snapshot: diffing it per release, and adding
the crate-level public-item surface beside it, turns the policy into a build
gate — and gives capability-package authors a compatibility signal that is
computed rather than claimed (`C71`, `C93`).

**Threats.** A framework that breaks a minor release once loses the benefit of
having a policy at all.

**Position.** **Partial** — a stability policy, codemods, and a machine-readable
description exist; no snapshot diff, no compatibility gate, no sealing policy.

**Requirements.**

- `C128-1` `[X]` A committed public-API snapshot — crate items plus the
  `describe` surface — diffed in CI, with the diff failing the build when it
  exceeds what the declared version bump allows.
- `C128-2` `[X]` A written sealing policy (non-exhaustive enums, opaque
  handles, sealed traits) applied to the public surface, so additive change
  stays additive.

## C129 — Architecture boundaries as tests

**Introduced by.** W9 and D5's large-codebase practice; the module systems of
W7 and M4 enforce a weaker version structurally.

**Mechanism.** The layering a project believes in is written down as rules —
which module may depend on which, what may not reach across a boundary, what
may not appear in a public signature — and a test fails when a change violates
them. It is the architectural equivalent of a lint, and it is what stops a core
crate from quietly acquiring a platform dependency.

**Strengths.** The architecture stays true under time pressure and staff
turnover; violations are caught in the pull request that introduces them, with
the rule named.

**Weaknesses.** Rules ossify if nobody is allowed to change them; too many
rules produce noise.

**Opportunities.** `PLAN.md` 2.4 is exactly such a rule — the core must never
depend on a platform API — and it is currently enforced by review and by the
crate graph rather than by a test that says so. Making section 2's invariants
executable (core has no platform dependency; no global mutable state on the
render path; backends reach the host only through their ownership module) turns
the plan's principles into gates, which is the same move Milestone 41 made for
guarantees.

**Threats.** Low externally; internally, this is how a framework with a clean
architecture keeps one.

**Position.** **Absent** — the invariants are stated in prose and honoured by
practice.

**Requirements.**

- `C129-1` `[X]` The section 2 invariants as executable boundary tests — core
  free of platform dependencies, backend host access confined to its ownership
  module, no global mutable state on the render path — each failure naming the
  principle it breaks.

## C130 — Test health

**Introduced by.** the engineering practice around every large archetype;
M8 and W9's test services make it a product.

**Mechanism.** Tests are treated as a system with its own reliability: flaky
tests are detected by repetition, quarantined automatically with an owner and
an expiry rather than deleted or endured, suites are sharded and run in
parallel with timing history, coverage is reported with a gate on new code
rather than on the whole repository, and correctness is pressure-tested beyond
examples — property tests, fuzzing on parsers and protocol code, mutation
testing on critical logic, and sanitizers on the unsafe surface.

**Strengths.** A suite people trust, which is the difference between a gate and
a ritual; parsers and unsafe code exercised by machines rather than by
imagination; regression risk concentrated where it matters.

**Weaknesses.** Fuzzing and mutation testing are expensive; quarantine can
become a graveyard without expiry; coverage gates invite gaming.

**Opportunities.** The verification gate already runs the suites, Milestone 45
adds the headless backend and deterministic time, and `X-L7-6` makes async
reproducible — which is the precondition that makes flakiness *detectable*
rather than assumed. The framework's own surfaces that most warrant fuzzing are
already identified: the markup parser and its source maps (Milestone 53), the
style-sheet parser, the update manifest and package readers, and every FFI
ownership module.

**Threats.** A flaky gate is worse than no gate, because it teaches people to
re-run rather than to read.

**Position.** **Partial** — a full gate with unit, integration, golden, and
conformance suites exists; flakiness detection, quarantine, sharding, coverage
gating, fuzzing, mutation testing, and sanitizer runs do not.

**Requirements.**

- `C130-1` `[X]` Flakiness detection by repeated runs, automatic quarantine
  with an owner and an expiry, and timing history used for sharding.
- `C130-2` `[X]` Coverage reported and gated on changed code, with the gate's
  rationale documented.
- `C130-3` `[X]` Continuous fuzzing of the parsers, manifest and package
  readers, and protocol decoders; property tests for reconciliation, layout,
  and merge functions; sanitizer runs over the unsafe surface of every backend.

## C131 — Dependency hygiene, provenance, and reproducibility

**Introduced by.** W9, E7, and the supply-chain practice that followed the
ecosystem attacks of the last decade.

**Mechanism.** Dependency updates arrive as automated, tested pull requests
rather than as a quarterly panic; advisory databases gate the build; licences
are checked; the dependency set can be vendored or mirrored so a build does not
depend on a registry being up; releases carry signed provenance describing what
built them from which source; and the build is reproducible, so an independent
rebuild produces the same artifact.

**Strengths.** Security response measured in hours; supply-chain questions in
enterprise reviews answered with artifacts rather than assurances; a build that
can be reconstructed years later, which regulated and embedded customers
require.

**Weaknesses.** Update automation is noise unless the suite is trustworthy
(`C130`); reproducibility is a constant fight against timestamps, paths, and
parallelism.

**Opportunities.** `deny.toml` already gates advisories and licences, and
Milestone 51 generates a software bill of materials and licence report.
Provenance attestation, a mirrored dependency set, and a reproducibility check
in CI complete the chain — and reproducibility is a claim the embedded and
regulated markets (E3, E7) ask for by name.

**Threats.** One compromised dependency in a framework is a compromise of every
application built on it; this is the risk that scales worst with adoption.

**Position.** **Partial** — advisory and licence gates and a bill of materials
exist; update automation, mirroring, provenance, and a reproducibility check do
not.

**Requirements.**

- `C131-1` `[X]` Automated dependency update proposals gated by the full suite,
  with advisory and licence checks as build gates.
- `C131-2` `[X]` Signed build provenance for every released artifact, plus a
  mirrored or vendored dependency set for offline and regulated builds.
- `C131-3` `[X]` A reproducibility check in CI: an independent rebuild produces
  a bit-identical artifact, with any unavoidable variance documented.

## C132 — Release health gating

**Introduced by.** M1, M2, and the crash-reporting services around them; W3's
hosting platforms do the same for deployments.

**Mechanism.** A staged rollout is *watched*: crash-free session rate,
crash-free user rate, startup failure rate, and responsiveness (hang and
frame-drop rates) are compared against the previous release for the cohort that
has the new one. Thresholds are declared; breaching one halts the rollout
automatically, and a worse breach reverts it. The same signals gate an update
channel (`C97`) and a feature flag (`C50`), so a bad feature is disabled without
a release.

**Strengths.** Regressions are caught by the first percent of users rather than
by the last; rollback becomes a policy rather than a decision made under
pressure at midnight; feature-level and release-level control use one set of
signals.

**Weaknesses.** Thresholds need history to be meaningful; noisy cohorts cause
false halts; it requires production telemetry, with everything `C122` implies.

**Opportunities.** Milestone 50 already does staged rollout by stable bucket
with rollback on repeated failure, and Milestone 51 adds crash capture and
metrics. Release health is the loop between them — and with `C126-2`'s declared
guardrail metrics, the thresholds have definitions rather than folklore. For
device fleets (`C84`) the same mechanism governs firmware rollout, where the
cost of a bad release is highest.

**Threats.** Shipping to everyone at once is how frameworks acquire their worst
incident stories, and users blame the framework as readily as the application.

**Position.** **Partial** — staged rollout and per-installation rollback exist;
health signals do not gate them.

**Requirements.**

- `C132-1` `[X]` Declared release-health thresholds over guardrail metrics,
  evaluated per rollout cohort, halting a staged rollout on breach and
  reverting on a severe one — for application updates, firmware updates, and
  feature flags alike.

---

## Part summary

Three of these make existing promises mechanical rather than aspirational: the
API snapshot gate (`C128`) enforces the stability policy Milestone 52
published, boundary tests (`C129`) enforce the architecture section 2 states,
and release-health gating (`C132`) closes the loop on the staged rollouts
Milestone 50 already ships.

The other two are the price of being trusted at scale: a test suite whose
failures mean something (`C130`), and a supply chain whose artifacts can be
audited and reproduced (`C131`). Both are asked about by exactly the buyers who
never read a benchmark.
