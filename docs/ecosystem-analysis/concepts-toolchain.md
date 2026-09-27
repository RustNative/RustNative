# Concepts V — the toolchain and service layer (L7–L9)

Continues [`concepts-core.md`](concepts-core.md) (format, inclusion rule,
scoring). Requirement identifiers are `Cnn-k`.

This document exists because one archetype — the managed toolchain layer
analysed as `mobile.md` M8 — turned out, on a second and closer reading, to
carry more distinct mechanisms than any other single archetype in this
analysis. It is not a UI framework. It is the layer between a repository and an
installed, updatable application, and it won its position by owning that layer
completely: project generation, dependency compatibility, diagnostics, device
pairing, cloud builds, credentials, store submission, update channels,
compatibility gating, pipelines, and a shareable playground.

That matters here for a specific reason. Every other archetype in this analysis
competes with RustNative on something the framework *is*. This one competes on
everything that happens **around** the framework — and it is the archetype most
often credited, by its own users, with why they chose the stack at all. A
framework that matches it on architecture and loses to it on the toolchain
loses the evaluation anyway.

The concepts below are ordered the way a project meets them: what you install
and upgrade (C93, C94), what you build and sign (C95, C96, C101), what you ship
and update (C97, C98, C105), how you extend and route (C99, C100), and how you
try and share it (C102, C103, C104).

---

## C93 — The versioned release train

**Introduced by.** M8; approached differently by D1, D3, and W9, whose vendors
ship a platform version rather than a library version.

**Mechanism.** The platform has *one* version number, not one per package. A
release pins a curated set of libraries to each other and to the host versions
they support, so "we are on version N" fully determines which library versions
are compatible, which host versions are supported, and which language and
toolchain versions are required. Installing a dependency resolves the version
compatible with the project's platform version rather than the newest published
one. Releases follow a predictable cadence, each with a support window and a
deprecation policy, and an upgrade command moves the whole set at once,
reporting per-dependency what it changed and what it cannot.

**Strengths.** Removes the combinatorial compatibility problem that dominates
assembled stacks: a team on a supported release has a *supported combination*,
not a lucky one. Upgrades become one decision per period rather than dozens
per week. Documentation, examples, and support conversations can name a
version and be precise. The vendor gets one matrix to test rather than N.

**Weaknesses.** Anything outside the train lags it; a library that ships fixes
faster than the train is held back by it. Teams that need one newer package
must leave the train or pin around it. A long support window is an ongoing
maintenance cost, and a short one forces upgrades the team did not want.

**Opportunities.** RustNative is already one product across many crates, and
`docs/policy/stability.md` already sets semantic versioning, a deprecation
window, an MSRV policy, and the rule that a breaking change ships with a
codemod (Milestone 52). What is missing is the *train*: one platform version
that the crates, the CLI, the capability packages, the templates, and the
supported host versions are all stated against, and an `add` that resolves to
the compatible version rather than the newest. Because our capability packages
already carry a framework version range (`C71-1`), the resolution data exists —
it simply is not used as a train.

**Threats.** Without it, a team that adds three capability packages and a
backend upgrade discovers compatibility by building. That is exactly the
experience the archetype eliminated, and teams that have had it once do not
tolerate it again.

**Position.** **Partial** — stability policy, codemods, MSRV, and per-package
version ranges exist; a single platform version, compatible-version resolution,
and a supported-host matrix do not.

**Requirements.**

- `C93-1` `[X]` One platform version covering the crates, the CLI, the
  templates, and the capability-package index, stated in `rustnative.toml` and
  reported by `rustnative describe`.
- `C93-2` `[X]` A supported-host matrix per platform version — operating-system
  versions, device APIs, browser baselines, toolchain and MSRV — published and
  checked by `doctor` (`C94`).
- `C93-3` `[X]` `rustnative add` resolves the version compatible with the
  project's platform version, and refuses an incompatible package with the
  reason (extends `C71-1`).
- `C93-4` `[X]` `rustnative upgrade` moves the whole train, runs the codemods,
  and prints a per-dependency report of what moved, what is held back, and why.

## C94 — Project and environment diagnostics

**Introduced by.** M8's diagnostic command; W9's health checks and W7's system
checks are the server-side equivalents.

**Mechanism.** One command answers "is this project in a state that can
build, run, and ship?" It checks the toolchain and platform SDKs, the
dependency set against the release train, the project configuration against its
schema, native configuration for drift, the signing and credential material,
and known-bad combinations — then prints each failure with the command that
fixes it. It runs in CI as a gate and locally as a first-aid command.

**Strengths.** Turns a class of confusing build failures into an explanation;
makes support conversations short; catches an incompatible dependency before it
is compiled rather than after.

**Weaknesses.** Diagnostics rot if they are not maintained alongside the
things they check; false positives train people to ignore them.

**Opportunities.** `rustnative doctor --install` already installs what `rustup`
can (Milestone 43). Extending it into a full project check is cheap and pays
back on every backend we add, because each new backend brings a new toolchain,
a new SDK, and a new set of confusing first-run errors. A doctor that
understands the release train (`C93`) and the capability-package manifests
(`C71`) can diagnose compatibility rather than merely presence.

**Threats.** Low as a competitive threat; high as an adoption tax if absent,
because the first failure a new user hits is usually environmental.

**Position.** **Partial** — `doctor --install` covers toolchain installation;
dependency, configuration, credential, and drift checks do not exist.

**Requirements.**

- `C94-1` `[X]` `rustnative doctor` checks toolchains and platform SDKs,
  dependency compatibility against the release train, configuration against its
  schema, generated-native drift (`C104`), and credential material, printing
  the fixing command for each failure.
- `C94-2` `[X]` A machine-readable doctor report and a CI mode that fails the
  build on findings above a declared severity.

## C95 — Build profiles and managed credentials

**Introduced by.** M8's build service.

**Mechanism.** Named *build profiles* — development, preview, production, and
whatever a team invents — each declare target, optimization, configuration
variant, environment values, secrets, update channel, and distribution intent.
One command builds a named profile for a named target. Signing material is
*managed*: created, stored, and used by the build service, so a developer signs
a store build without ever holding the key, and a lost laptop is not a lost
signing identity. Secrets are attached to profiles and injected at build time,
never committed.

**Strengths.** Reproducible, named builds instead of remembered flag
combinations; onboarding without distributing signing keys; environment
variants (staging, production) without code changes; rotation and revocation of
credentials as an operation rather than an archaeology project.

**Weaknesses.** Key custody by a service is a trust decision, and some
organizations cannot make it; profiles multiply; secret handling adds a moving
part to every build.

**Opportunities.** `rustnative package windows --sign` exists, and typed
configuration and per-invocation secret reads are already required
(`W-EP-1`, `W-DP-3`). Profiles are the missing shape that ties target,
configuration variant, secrets, update channel, and distribution together — and
they are the natural unit for the pipelines in `C98` and the channels in
`C97`. Our position on custody can be *stronger* than the archetype's: profiles
that work identically against a local key store, an organization's own vault,
or a hosted service, with the choice stated in configuration rather than
implied by using the product.

**Threats.** A framework whose release process is a document of manual steps
loses to one whose release process is a command, regardless of runtime merit.

**Position.** **Absent** — local signing exists; profiles, environments, and
managed credentials do not.

**Requirements.**

- `C95-1` `[X]` Named build profiles in `rustnative.toml` binding target,
  configuration variant, environment values, secrets, update channel, and
  distribution intent, with `rustnative build --profile <name>`.
- `C95-2` `[X]` A credential contract with at least three backings — local
  store, external vault, remote service — so signing material can be held
  where the organization requires, with rotation and revocation documented.
- `C95-3` `[X]` Secrets resolved per build from the profile, never written into
  the artifact, with a build-time check that proves it (extends `W-DP-3`).

## C96 — Store submission as a command

**Introduced by.** M8's submission service.

**Mechanism.** Uploading a build to each host's store, with credentials,
metadata, release notes, phased release percentage, and review status handled
by one command and reported back. Metadata and screenshots live in the
repository; the submission is an artifact of the pipeline, not a browser
session.

**Strengths.** Removes the last manual, error-prone, per-platform step;
repeatable releases; review status visible from the terminal and CI.

**Weaknesses.** Store APIs change and break; per-store metadata rules are
extensive; phased release semantics differ.

**Opportunities.** Packaging already generates store-acceptable artifacts on
Windows (Milestone 32) and plans to on every other host. Submission is the step
after, and it is the difference between "produces an artifact" and "ships".
Store metadata in the repository also feeds compliance evidence (`W-EP-3`) and
the privacy manifests already generated for mobile.

**Threats.** Teams that have automated submission will not go back; teams that
have not do not know what they are missing — so this converts evaluations
mostly for experienced teams, which are the ones with budget.

**Position.** **Absent.**

**Requirements.**

- `C96-1` `[M]` `[D]` `rustnative submit <target> --profile <name>`: upload,
  metadata and release notes from the repository, phased-release percentage,
  and review status reported back, for each host with a store.

## C97 — Update channels, runtime compatibility, and native fingerprinting

**Introduced by.** M8's update service; the fingerprinting mechanism is
specific to it and is the part most worth taking.

**Mechanism.** Updates are published to a *branch*; installed builds subscribe
to a *channel*; the mapping between them is changed at will, so promoting a
release is repointing a channel rather than rebuilding. Crucially, an update is
only delivered to a build it is *compatible* with, and compatibility is decided
by a **runtime version**: either a declared string, or — the interesting
variant — a **fingerprint computed by hashing every input that affects the
native side of the build** (dependencies, native configuration, permissions,
generated projects). If an application's native inputs change, its fingerprint
changes, and old updates stop being offered to it automatically. Every build
also carries an embedded copy of the code it shipped with, so a failed update
falls back to something known good, and adoption is measured per release.

**Strengths.** The fingerprint removes the worst failure mode of over-the-air
updates — shipping application code to a build whose native side cannot run it —
without asking developers to maintain a compatibility string by hand. Channels
separate "what is built" from "who gets it". Embedded fallback makes rollback
instant and offline-safe. Adoption metrics turn a release into something
observable rather than assumed.

**Weaknesses.** Fingerprint inputs must be complete or the guarantee is false;
an over-sensitive fingerprint forces store releases for harmless changes.
Channel mappings are powerful enough to cause mistakes at scale.

**Opportunities.** Milestone 50 already ships signed manifests, staged rollout
by stable bucket, version pinning, atomic activation, and rollback on repeated
failure — the hard security parts. What is missing is exactly the part that
makes updates *safe to automate*: a compatibility fingerprint over native
inputs, channels decoupled from builds, an embedded fallback payload, and
adoption reporting. For RustNative the fingerprint is more tractable than for
the archetype, because the native inputs are already declared: the crate
dependency graph, `rustnative.toml`, the generated manifest and entitlements,
and the capability packages with their grants. A fingerprint over exactly those
is computable at build time and verifiable at update time.

**Threats.** Update mechanisms that lack compatibility gating eventually ship a
payload that bricks a cohort. One such incident costs more trust than the
feature earns.

**Position.** **Partial** — signed, staged, pinned, rollback-capable desktop
updates exist; channels, fingerprint gating, embedded fallback, and adoption
reporting do not.

**Requirements.**

- `C97-1` `[X]` A compatibility fingerprint computed from declared native
  inputs — dependency graph, project configuration, generated manifests and
  entitlements, capability packages and their grants — recorded in every build
  and in every update manifest, with delivery refused on mismatch.
- `C97-2` `[X]` Update *channels* decoupled from builds: a build subscribes to
  a channel, a release is published to a branch, and promotion repoints the
  channel without rebuilding.
- `C97-3` `[X]` An embedded fallback payload in every build, so a failed or
  withdrawn update returns to a known-good state offline.
- `C97-4` `[X]` Adoption reporting per release — how many installations are on
  which version, with the privacy policy of `X-OBS-1` applied.

## C98 — Pipelines defined in the repository

**Introduced by.** M8's workflow service, and the continuous-integration
ecosystem generally; the specific idea taken here is *framework-aware* jobs.

**Mechanism.** Build, test, submit, and update jobs are declared in the
repository, versioned with the code, triggered by version-control events, and
run with the framework's own understanding of targets, profiles, and channels —
so a job is "build the production profile for these two targets, run the
device matrix, submit, then publish an update to this channel", not a hundred
lines of shell.

**Strengths.** Release engineering becomes reviewable code; the same pipeline
runs locally and remotely; framework-aware jobs remove the per-project glue
that rots.

**Weaknesses.** Yet another workflow format; overlaps with whatever CI the team
already runs, which they will not replace.

**Opportunities.** The pragmatic position is the one already taken for
deployment (`W-DP-1`): own the *jobs*, not the runner. `rustnative` should
expose each release step as a command that any CI can call, and ship generated
pipeline descriptions for the common CI systems from the profiles in `C95` —
so the framework is the source of truth and the team's existing CI is the
executor.

**Threats.** Building a competing CI product would be a scope error with no
payoff.

**Position.** **Absent.**

**Requirements.**

- `C98-1` `[X]` Every release step available as a scriptable `rustnative`
  command with machine-readable output: build a profile, run a test matrix,
  package, sign, submit, publish an update, promote a channel, roll back.
- `C98-2` `[X]` Generated pipeline descriptions for at least two widely used CI
  systems, derived from the declared profiles, refreshed by `rustnative
  generate pipeline`.

## C99 — Autolinking and declarative native modules

**Introduced by.** M8's module system and autolinking; M3's code generation;
D5's meta-object system is the older relative.

**Mechanism.** Two halves. *Autolinking*: adding a dependency that contains
native code is enough — the build discovers it, links it, and merges its native
configuration, with no manual project edits. *Declarative native modules*: the
author of a native module writes a small declaration — name, functions,
properties, events, view types — in each host language, and the binding layer
between the portable API and the host language is generated from it, with types
checked on both sides, rather than hand-written marshalling.

**Strengths.** Native extension stops being an expert activity; the
declaration is the documentation; the boundary is typed on both sides;
consumers never edit a native project to add a capability.

**Weaknesses.** The declaration language is another thing to learn; generated
bindings constrain what can cross; debugging spans generated code.

**Opportunities.** RustNative has the pieces and not the ergonomics:
capability packages with per-backend code and grants (`C71`, Milestone 52),
`rustnative add`, `bindgen` for host languages (`C66`), and one ownership module
per backend (`X-L0-5`). What is missing is the authoring experience — a
declaration from which per-backend glue, the portable service trait, the
manifest contributions (`C63-2`), and the test doubles are all generated — and
the guarantee that adding such a package requires no native project edit.
Because our boundary is compiled rather than serialized, the generated glue can
be checked at compile time on both sides, which the archetype cannot do.

**Threats.** The size of a framework's native-module ecosystem is a leading
indicator of its adoption, and ecosystems grow where authoring is easy.

**Position.** **Partial** — packages, grants, and bindings exist; a native
module authoring declaration with generated glue and automatic native
configuration merging does not.

**Requirements.**

- `C99-1` `[X]` A native-module declaration — functions, properties, events,
  view types — from which the portable trait, the per-backend glue, the
  manifest contributions, and a headless test double are generated, with the
  boundary type-checked on both sides.
- `C99-2` `[X]` Autolinking: adding a capability package to the dependency
  graph links its native code and merges its native configuration with no
  project edits, verified by a test that adds a package and builds.

## C100 — One router for every target

**Introduced by.** M8's router; W3 and W4 for the web half.

**Mechanism.** One file- or table-based router serves native navigation *and*
web URLs from the same definitions: typed routes and parameters, nested
layouts, deep links and universal links generated into the native
configuration, a static export for the web, and server routes in the same tree
when the deployment has a server. A link is one construct, whether it becomes a
push on a navigation stack or a URL.

**Strengths.** One mental model across native and web; deep links that are
configured rather than hand-written; shareable URLs on every target that has
them; static export and server rendering without a second router.

**Weaknesses.** Native navigation and URL navigation genuinely differ (stacks,
tabs, modals, back semantics), so the abstraction leaks at the edges; typed
routes demand generation or macro machinery.

**Opportunities.** Milestone 30 already has `Route`/`Router`, Milestone 47 adds
navigation as typed state and typed query parameters (`C13`), and the Web track
plus Milestone 49 add server routes and per-route rendering. The missing piece
is the *unification*: one route table that also emits each host's deep-link
configuration (`C63-2`), the web's static export map, and the server's route
table — so a route is declared once and every target's plumbing is generated.

**Threats.** Teams building for native and web will otherwise maintain two
routing models and accept the drift, which is the outcome the archetype
eliminated.

**Position.** **Partial** — one router exists; deep-link configuration
generation, static export mapping, and the native/web unification do not.

**Requirements.**

- `C100-1` `[X]` One route table from which native navigation, web URLs, the
  server route table, and each host's deep-link and universal-link
  configuration are all derived.
- `C100-2` `[W]` Static export of the route tree for hosts that serve files
  only, sharing the prerendering path.

## C101 — Generated launch assets

**Introduced by.** M8's asset generation; every first-party mobile toolchain
has a partial version.

**Mechanism.** One source image and a few declared colours produce every
artifact each host demands: application icons at every density and shape
(including adaptive, monochrome, and themed variants), launch screens, store
graphics, notification icons, and favicons — generated at build time, checked
against each store's rules, and never committed as dozens of files.

**Strengths.** Removes a tedious, error-prone, per-release chore; guarantees
the rules are met; a rebrand is one file.

**Weaknesses.** Generated output is sometimes worse than hand-tuned art at the
smallest sizes; per-host shape rules change.

**Opportunities.** Packaging already takes one icon on Windows. Generating the
full set per backend from a declared source, and validating it against the
host's rules, is a small, well-bounded feature that visibly improves the first
release — and it is the kind of thing whose absence is discovered at submission
time, at the worst moment.

**Threats.** Low individually; collectively, this class of chore is what makes
"shipping with framework X" feel heavy.

**Position.** **Absent** — a single icon is embedded on Windows; nothing is
generated.

**Requirements.**

- `C101-1` `[X]` Launch assets — icons in every required density and shape,
  launch screens, and store graphics — generated per backend from one declared
  source, validated against each host's rules at package time.

## C102 — The shareable playground

**Introduced by.** M8's browser playground.

**Mechanism.** A hosted editor runs a small application in the browser or on a
paired device, from a link. Documentation examples are runnable in place; bug
reports arrive as a link that reproduces the problem; answers to questions are
demonstrated rather than described.

**Strengths.** Removes the setup cost from evaluation, teaching, and support —
the three moments where a framework is most often abandoned. Turns every
documentation page into a place to experiment.

**Weaknesses.** Needs hosting and sandboxing; can only run what the sandbox
supports, so it teaches a subset; keeping it current with releases is ongoing
work.

**Opportunities.** This is more reachable for us than it looks. The Web track
compiles the same application model to the browser, and the headless backend
plus previews (`C55`) already render components without a host. A playground
that compiles a snippet to the web target and renders it — with a link that
carries the snippet, and an option to open the same snippet as a project — is
the evaluation path with the lowest possible friction, and it doubles as the
reproduction format for bug reports.

**Threats.** Evaluation friction is where a compiled framework is most
vulnerable: the archetype's first experience is a scanned code and a running
application, ours is a toolchain install.

**Position.** **Absent.**

**Requirements.**

- `C102-1` `[X]` A shareable playground that compiles a snippet to the web
  target and runs it, with a link that carries the snippet and an "open as a
  project" path.
- `C102-2` `[X]` Documentation examples runnable in the playground, generated
  from the same sources as the guides and doc tests, so they cannot drift.

## C103 — Device pairing for the development loop

**Introduced by.** M8's development client and pairing flow.

**Mechanism.** A running development server advertises itself; a device joins
by scanning a code or entering a short address; transport falls back from the
local network to a relay when the network forbids direct connections; several
devices can attach at once and receive the same updates; the connection
survives the device sleeping and reconnecting.

**Strengths.** Removes the cable-and-configuration step from on-device work;
makes testing on several devices at once ordinary; works from a coffee shop
network and through corporate firewalls.

**Weaknesses.** A relay is a security surface and a dependency; discovery on
hostile networks is fiddly.

**Opportunities.** `rustnative dev-agent` already exists with a token and a
two-phase deploy, verified over loopback (Milestone 43). The remaining work is
the pairing experience and the transports: advertisement on the local network,
a short-code or scannable pairing, a relay fallback, and fan-out to several
attached devices — all of which apply equally to phones, boards, and remote
desktops, which is broader than the archetype's own scope.

**Threats.** Device iteration speed is compared directly in evaluations, and
pairing friction is counted as part of it.

**Position.** **Partial** — an authenticated remote host exists over loopback;
discovery, pairing, relay, and multi-device fan-out do not.

**Requirements.**

- `C103-1` `[M]` `[E]` `[D]` Local-network advertisement and a short-code or
  scannable pairing for the development host, with an authenticated relay
  fallback, documented threat model, and fan-out to several attached devices.

## C104 — The two-way escape ladder for generated projects

**Introduced by.** M8, whose earliest version made the escape one-way and whose
current version does not — the correction is the lesson.

**Mechanism.** Generated native projects (`C63`) can be *materialized* on
demand: written out, committed, and edited by hand when a requirement cannot be
expressed as configuration. The valuable half is the return path — a diff
between the materialized project and what the generator would produce now, so a
team can see exactly what it owns, adopt upstream changes selectively, and move
back to generation when the special case is gone or becomes expressible.

**Strengths.** Removes the fear that makes teams refuse generation entirely;
makes "we had to eject" a reversible state rather than a permanent fork; the
drift diff is a maintenance tool in its own right.

**Weaknesses.** Two supported modes to test; the drift diff is only as good as
the generator's determinism.

**Opportunities.** We are committing to generated native projects on every
backend (`C63-1`). The archetype's history says the *escape and return* path is
what makes that commitment acceptable to teams with unusual requirements — and
our generator's inputs are already declarative, so a deterministic regeneration
and a drift report are straightforward. This also gives `doctor` (`C94-1`)
something precise to check.

**Threats.** Without a return path, one unusual requirement removes a team from
every future upgrade — and they tell others.

**Position.** **Absent.**

**Requirements.**

- `C104-1` `[X]` `rustnative eject <target>` materializes the generated native
  project, and `rustnative diff-native <target>` reports the drift between a
  materialized project and current generation, with a documented path back.

## C105 — Artifact composition inspection

**Introduced by.** M8's bundle inspector; W14's bundle analysers; E7's image
manifests.

**Mechanism.** A tool answers "what is in this artifact, and why": size
attributed per crate, per asset, and per dependency; what pulled in each item;
what changed since the last build; and which declared budget each contributes
to.

**Strengths.** Size regressions become attributable instead of mysterious; an
accidental heavyweight dependency is visible on the commit that added it; asset
bloat is separated from code bloat.

**Weaknesses.** Attribution is approximate after optimization and inlining;
tooling must be maintained per target format.

**Opportunities.** Milestone 42 already enforces artifact-size budgets in CI. A
budget that fails without attribution tells a developer that something grew,
not what — and the answer is exactly what makes the budget actionable. The
build already knows the crate graph and the embedded-asset table (`embed_assets`
hashes every file), so most of the data exists.

**Threats.** Low as a competitive threat; high as a source of avoidable
frustration when a budget fails.

**Position.** **Absent.**

**Requirements.**

- `C105-1` `[X]` `rustnative bundle explain <artifact>`: size attributed per
  crate, asset, and dependency, with the delta against a baseline and the
  budget each contributes to.

---

## What this archetype teaches, in one paragraph

Every mechanism above sits between the repository and the installed
application, and none of them is a UI feature. The archetype's own users
describe its value as "it removes the parts of mobile development nobody wants
to do" — project files, signing, submission, compatibility, device setup. That
is a *complete* claim to a layer, and it is the layer where this project is
currently strongest on desktop (Milestones 43, 50, 52 on Windows) and weakest
everywhere else, because the device backends that need it most are the ones not
yet built. The sequencing consequence is stated in
[`gap-plan.md`](gap-plan.md) under Milestone 59: the toolchain contracts should
be defined while there is one backend to test them against, so the device
backends inherit them instead of each inventing a release process.
