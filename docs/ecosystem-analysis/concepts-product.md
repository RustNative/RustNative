# Concepts VII — trust, compliance, and product operations (L6, L8, L9)

Continues [`concepts-core.md`](concepts-core.md) (format, inclusion rule,
scoring). Requirement identifiers are `Cnn-k`.

Everything here is a requirement someone outside the engineering team imposes:
a platform's review process, a regulator, a security questionnaire, a growth
team, or a privacy officer. Frameworks that ignore this band are adopted for
prototypes and abandoned at the point of shipping a commercial product, which
is the most expensive moment to lose a user.

---

# Part A — Identity, integrity, and compliance

## C120 — Platform sign-in and account lifecycle obligations

**Introduced by.** M1 and M2 under store rules; W7 and W9 on the server side.

**Mechanism.** Hosts impose account rules, not just APIs. An application
offering third-party sign-in must usually also offer the platform's own; an
application with accounts must offer an in-application path to *delete* the
account and its data; sessions must be revocable across devices; sign-in on
devices without keyboards (televisions, appliances) needs a device
authorization flow; and credentials should come from the platform credential
manager, which now brokers passwords, passkeys, and federated identity through
one sheet.

**Strengths.** Sign-in that users trust and can complete quickly; deletion and
revocation paths that satisfy both stores and privacy law; fewer bespoke
authentication screens.

**Weaknesses.** Rules differ per store and change; deletion has to reach every
system that holds the data, which is a product problem, not a framework one.

**Opportunities.** Milestone 49 already has authentication and passkeys
(`C52-2`). The concept to adopt is the *lifecycle obligations*: the framework's
authentication contract should make account deletion, session revocation, and
platform-broker sign-in first-class, and the generated compliance evidence
(`W-EP-3`) should list which of them an application implements — turning a
review checklist into something the build can answer.

**Threats.** A missing deletion path is a store rejection, and a rejection at
submission time is the worst possible moment to discover a framework gap.

**Position.** **Partial** — authentication, sessions, and passkeys exist; the
platform-broker path, device flow, revocation, and deletion obligations do not.

**Requirements.**

- `C120-1` `[X]` Account lifecycle in the authentication contract: in-product
  deletion with a documented propagation path, session revocation across
  devices, and a device authorization flow for keyboard-less hosts.
- `C120-2` `[M]` `[D]` Sign-in through each host's credential broker where one
  exists, covering passwords, passkeys, and federated identity.

## C121 — Application integrity and anti-abuse attestation

**Introduced by.** M1, M2, and the platform integrity services; W15 and W9 on
the server side.

**Mechanism.** A server cannot trust a client, so hosts offer *attestation*: a
signed statement from the platform that this request comes from a genuine,
unmodified build of your application on a genuine device. Around it sit replay
protection (nonces), device-binding of tokens, rooted-or-emulated signals, and
a server-side policy that decides what to do with a failed check — block,
degrade, or observe.

**Strengths.** Abuse, credential stuffing, and cheating become much harder
without punishing honest users; a policy that can start in observe mode and
tighten with evidence.

**Weaknesses.** Attestation excludes legitimate users on unusual devices;
signals are probabilistic; over-reliance produces support load; it is
per-platform and unavailable on desktop and embedded in the same form.

**Opportunities.** The framework's job is the *contract and the policy shape*,
not the verdict: attestation as a capability with an honest per-host answer, a
server-side verification service in Milestone 49, and a documented policy
ladder (observe, degrade, block) so applications do not hard-fail users by
default. Our capability model is already the right vehicle for "this host
cannot attest".

**Threats.** Consumer applications with payments or competition are expected to
have this; without it they carry the abuse themselves.

**Position.** **Absent.**

**Requirements.**

- `C121-1` `[X]` A platform-attestation capability with nonce-based replay
  protection, verified by the server model, and a documented observe/degrade/
  block policy ladder with honest answers where a host has no attestation.

## C122 — Consent, tracking, and regional compliance

**Introduced by.** M1, M2, W3, and the regulators behind them.

**Mechanism.** Before an application may track a user across contexts it must
ask, in the host's own prompt; before it may process personal data in some
regions it must collect consent with a lawful basis, store the record, and
honour withdrawal; some categories (children, health, finance) impose extra
rules; data may be required to stay in a region; store listings require privacy
labels, content ratings, and export declarations. Most of these have
machine-checkable artifacts.

**Strengths.** Compliance as declared configuration rather than folklore;
consent state available to the application so it can degrade features rather
than break; evidence generated for reviews and audits.

**Weaknesses.** Rules differ per jurisdiction and change; consent interfaces
are product surfaces the framework should not dictate; over-collection of
consent annoys users.

**Opportunities.** `W-EP-3` already generates compliance evidence, and
Milestone 51 has a threat model and privacy manifests. The addition is a
*consent contract*: a typed consent state in the environment (`C15`) that
services and the data layer respect — analytics that does not emit without
consent, telemetry that honours withdrawal, storage that knows its region —
plus a tracking-authorization capability per host. Making consent a value the
framework enforces is stronger than documenting that applications should check
it.

**Threats.** Regulatory findings are existential for commercial users;
frameworks that make compliance hard get replaced in regulated industries.

**Position.** **Partial** — privacy manifests and telemetry opt-in exist; a
consent contract, tracking authorization, residency, and age gating do not.

**Requirements.**

- `C122-1` `[X]` A typed consent contract in the environment — purposes,
  lawful basis, grant and withdrawal, and an auditable record — enforced by
  telemetry, analytics, and the data layer rather than checked by convention.
- `C122-2` `[M]` `[W]` Tracking authorization as a per-host capability, with
  store privacy labels, content ratings, and export declarations generated from
  the same declarations.
- `C122-3` `[X]` Data-residency declarations that the storage and deployment
  adapters can enforce and report.

## C123 — Data portability, backup, and device transfer

**Introduced by.** M1, M2, D1; the regulatory half comes from data-protection
law.

**Mechanism.** Two directions. *Platform backup*: application data is backed up
and restored with the device, which means deciding what is included, what must
be excluded (credentials, caches, device-bound identifiers), and behaving
correctly when a restore lands on a different device — including
device-to-device transfer. *Portability*: the user can export their data in a
machine-readable form and request deletion, with the export produced by the
application rather than by a support engineer.

**Strengths.** Users keep their data when they replace a device — a retention
feature in disguise; regulatory requests answered by a command; restore bugs
(duplicated accounts, stale device tokens) avoided by design.

**Weaknesses.** Backup inclusion rules are per-host and easy to get wrong in
the dangerous direction — backing up a secret; restore paths are hard to test.

**Opportunities.** Milestone 30's persistence and Milestone 47's schema
migration are the substrate. The missing contract is *classification*: every
persisted store declares whether it is backed up, device-bound, or excluded —
which also answers the restore-on-new-device case and feeds the export and
deletion endpoints that `C120-1` requires. The lifecycle conformance suite
(`M-OB-2`) can then test restore onto a different device identity.

**Threats.** "It lost my data when I got a new phone" is a review killer, and
export/deletion requests are a legal obligation.

**Position.** **Absent** — persistence exists with no backup classification,
export, or transfer story.

**Requirements.**

- `C123-1` `[X]` Backup classification per persisted store — included,
  excluded, device-bound — honoured by each host's backup system and verified
  by a restore-onto-a-different-device test.
- `C123-2` `[X]` User data export and deletion as framework-supported
  operations over the declared stores, usable by the account-deletion path.

---

# Part B — Product operations

## C124 — Deferred deep links, attribution, and link verification

**Introduced by.** M8, M3, and the growth tooling around M1 and M2.

**Mechanism.** A link should survive installation: a user taps a link, installs
the application, and lands on the content the link named — which requires the
link to be recoverable after install (*deferred* deep linking) and the platform
association files to be published and verified, or the operating system will
open a browser instead. Attribution ties an install back to its source with the
platform's privacy-preserving mechanisms, and link association is verified in
CI rather than discovered in production.

**Strengths.** Campaigns, referrals, and shared content actually work;
association misconfiguration — an extremely common and silent failure — is
caught by a test.

**Weaknesses.** Attribution is privacy-sensitive and restricted; deferred
matching is imperfect; association files must be served correctly from a domain
the team may not control.

**Opportunities.** Milestone 30's deep links and `C100`'s route-derived
association files are the substrate. Adding verification (fetch the published
association file in CI and check it against the generated configuration) and a
deferred-link handoff contract closes the loop, and both are cheap next to the
cost of the failure mode.

**Threats.** Growth teams choose stacks partly on this, and a broken universal
link is invisible to engineering and obvious to marketing.

**Position.** **Partial** — deep links exist; association generation,
verification, deferred links, and attribution do not.

**Requirements.**

- `C124-1` `[M]` `[W]` Platform association files generated from the route
  table and verified in CI against what the domain actually serves.
- `C124-2` `[M]` A deferred deep-link contract: the first launch after install
  can recover the link that caused it, using each host's privacy-preserving
  mechanism.

## C125 — In-product guidance

**Introduced by.** M1, M2, and the product layers around them.

**Mechanism.** The surfaces that teach and retain: first-run onboarding, feature
introductions and what's-new screens tied to versions, contextual coaching
marks, empty states that explain rather than apologize, in-application messaging
targeted by the same rules as feature flags, and the host's own review-request
API — used correctly, at a moment of success, within the quota the host allows.

**Strengths.** Adoption of new features without a release note nobody reads;
ratings requested at the right moment rather than at random; empty states that
convert.

**Weaknesses.** Easy to overdo; interruptive patterns are resented; targeting
rules need the same care as flags.

**Opportunities.** Feature flags and remote configuration (`C50`) already give
targeting, the component library (Milestone 48) gives the surfaces, and version
history is known to the update system. The concept to adopt is that these are
*framework-supported patterns with quotas and rules*, not application
improvisation — especially the review request, which hosts rate-limit and
whose misuse is a common rejection reason.

**Position.** **Absent.**

**Requirements.**

- `C125-1` `[X]` Guidance surfaces — onboarding, what's-new tied to version,
  coaching marks, targeted in-product messages — in the component library,
  driven by flags and version state, with frequency caps.
- `C125-2` `[M]` `[D]` The host's review-request API behind a contract that
  enforces its quota and a documented "moment of success" policy.

## C126 — A typed analytics and metric contract

**Introduced by.** W14's analytics layers, M8's insight service, W9's metric
conventions.

**Mechanism.** Events are declared as types with typed properties, versioned
and documented in the repository; the schema is generated into the analysis
layer so dashboards and warehouses cannot drift from the code; metrics —
activation, retention, conversion, and *guardrail* metrics such as crash-free
sessions and slow-start rate — are defined once with their computation, and the
experiment system (`C50`) reads the same definitions.

**Strengths.** Analytics that survives refactoring; no "what does this event
mean?" archaeology; experiments and dashboards measuring the same thing;
consent (`C122`) enforceable at the emit point because emission goes through a
typed contract.

**Weaknesses.** Schema governance is work; over-instrumentation is a cost and a
privacy risk.

**Opportunities.** Observability (Milestone 51) covers operational telemetry
but not product analytics, and the two are usually conflated to the detriment
of both. A typed event contract that shares transport with telemetry, respects
consent, and generates its own schema documentation is a small addition with a
large credibility payoff for commercial adopters.

**Position.** **Absent.**

**Requirements.**

- `C126-1` `[X]` Typed, versioned analytics events with generated schema
  documentation, emission gated by the consent contract, sharing transport and
  batching with telemetry.
- `C126-2` `[X]` Metric definitions — including guardrail metrics such as
  crash-free sessions, slow starts, and frame-drop rate — declared once and
  read by both dashboards and the experiment system.

## C127 — Privacy-safe session diagnostics

**Introduced by.** the diagnostic layers around M1, M2, and W1.

**Mechanism.** Reconstructing what a user did before a failure, without
recording their data: a structured event trail (navigation, interactions,
network results, state transitions) rather than pixels, with automatic
redaction of text and image content, sampling, an explicit opt-in, and local
retention with upload only on a crash or an explicit report.

**Strengths.** Support and debugging without guesswork; the "what did the
user's tree look like when it broke?" question (`X-OBS-1`) answered with
history rather than a single snapshot; far smaller privacy exposure than screen
recording.

**Weaknesses.** Trails still leak if redaction is incomplete; storage and
upload costs; a genuine temptation to record too much.

**Opportunities.** Record and replay (`C61`) already exists for development on
Windows, with redaction rules. A production-safe subset — sampled, redacted,
consent-gated, attached to crash reports — is mostly a policy and packaging
exercise on top of a mechanism that exists, and it is a differentiator against
frameworks whose equivalents are pixel recorders.

**Position.** **Partial** — development-time record and replay exists;
production capture, sampling, and consent gating do not.

**Requirements.**

- `C127-1` `[X]` A production diagnostic trail: sampled, redacted by default,
  consent-gated, retained locally, and attached to crash reports — sharing the
  recording format with `C61` so a report can be replayed.

---

## Part summary

Two of these are *gates rather than features*: account lifecycle (`C120`) and
consent with regional compliance (`C122`) decide whether an application can be
published and operated at all in the markets its owners care about, and both
are currently answered by documentation rather than by contracts the framework
enforces.

Two are *retention in disguise*: backup and transfer (`C123`) and deferred
links (`C124`) fail silently, are invisible to engineering, and are noticed
immediately by users and growth teams.

And two — the typed analytics contract (`C126`) and privacy-safe diagnostics
(`C127`) — are places where a framework that already owns typed messages,
consent, and a redacting recorder can offer something the assembled stacks
cannot: instrumentation that cannot drift from the code, and a failure trail
that is data rather than a video of someone's screen.
