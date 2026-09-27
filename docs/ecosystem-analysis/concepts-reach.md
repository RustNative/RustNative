# Concepts IX — reach: enterprise, shell, constrained networks, and the field (L6–L8)

Continues [`concepts-core.md`](concepts-core.md) (format, inclusion rule,
scoring). Requirement identifiers are `Cnn-k`.

The last band of the survey: the mechanisms that decide whether an application
can be *deployed into the places applications actually run* — a managed
corporate fleet, a desktop shell, a metered network, a browser under changing
privacy rules, or ten thousand devices in the field.

---

## C133 — Enterprise distribution and managed configuration

**Introduced by.** D1, D3, M1, M2 under their enterprise programmes; W9's
customers are the reason those programmes exist.

**Mechanism.** Applications distributed outside public stores — internal
tracks, private channels, enterprise signing — and configured by a management
system rather than by the user: an administrator pushes a configuration
document that the application reads at startup (server URL, tenant, feature
restrictions, single sign-on realm), and reports compliance back. Related
controls follow: kiosk or single-application mode, restrictions on copy and
paste between managed and personal applications, per-application network
policy, and offline licence validation.

**Strengths.** The only way into most large organizations; removes per-customer
build variants, because configuration replaces compilation; administrators
get the controls their security policy requires.

**Weaknesses.** Per-platform management protocols; testing needs a management
server; configuration schemas must stay compatible across versions.

**Opportunities.** Typed, layered configuration (`W-EP-1`) and build profiles
(`C95`) are most of the mechanism already: a managed-configuration provider is
one more configuration layer, with the schema published so administrators know
what they may set — and generated from the same declarations, so it cannot
drift. That is a cheap step into a market segment where a framework's
architecture is rarely the deciding factor and its manageability always is.

**Threats.** Enterprise buyers do not evaluate frameworks that cannot be
managed; the requirement arrives as a checklist from someone who will not
negotiate.

**Position.** **Absent** — layered typed configuration exists; no managed
provider, published schema, or enterprise distribution path.

**Requirements.**

- `C133-1` `[D]` `[M]` A managed-configuration layer per host, with a published
  schema generated from the application's configuration declarations, plus
  compliance reporting and kiosk or single-application mode where the host
  offers one.
- `C133-2` `[D]` `[M]` Enterprise and internal distribution paths in the
  release tooling (Milestone 59), including offline licence validation.

## C134 — Desktop shell integration

**Introduced by.** D1, D2, D5 — the accumulated conventions of desktop
operating systems.

**Mechanism.** The ways a desktop application participates in the system
outside its own window: file-type associations and protocol handlers with their
registration and verification, login and startup items, single-instance
behaviour with second-launch handoff (opening a file in the running instance
rather than starting a second one), global hotkeys, taskbar and dock badges and
progress, system search indexing of the application's own content, quick-look
style previews, power and sleep notifications, display hot-plug, and
fast-user-switching behaviour.

**Strengths.** An application that behaves like part of the system rather than
a visitor; double-clicking a document does the right thing; the application is
findable from the system's own search.

**Weaknesses.** Registration differs per host and per packaging format, and
gets subtly wrong when an application is installed for one user versus all
users; some pieces require elevated installation.

**Opportunities.** Milestone 23 has dialogs and menus, Milestone 24 has window
lifecycle, packaging already registers protocol schemes, and Milestone 57's
surface vocabulary has tray extras and jump lists. Association registration,
single-instance handoff, login items, global hotkeys, and power events are the
remaining desktop conventions, and they are the ones that make a desktop
application feel installed rather than merely present.

**Threats.** Desktop users notice these immediately, and their absence is the
usual evidence for "this is a ported mobile application" (`desktop.md` D10).

**Position.** **Partial** — protocol schemes are registered in packaging;
associations, handoff, login items, hotkeys, badges, indexing, and power events
are not portable contracts.

**Requirements.**

- `C134-1` `[D]` File-type associations and protocol handlers declared in
  project configuration and registered by packaging, with per-user and
  per-machine installation handled and verified.
- `C134-2` `[D]` Single-instance behaviour with second-launch handoff, login
  and startup items, global hotkeys, taskbar and dock badges and progress, and
  power, sleep, and display-change events as portable contracts.

## C135 — Delta and bandwidth-aware updates

**Introduced by.** M8's update service, D7's desktop updaters, and E6's
firmware tooling — independently, because they all hit the same wall.

**Mechanism.** Ship the difference, not the artifact: binary patches computed
against known previous versions, partial and resumable downloads, update fetch
deferred to unmetered networks or charging, bandwidth ceilings, and a fallback
to the full artifact when no patch chain applies. On devices, the patch must
also fit alongside the running image, which constrains how large a delta may be.

**Strengths.** Updates that complete on poor networks and constrained storage;
dramatically lower egress costs at fleet scale; higher update adoption, which
is a security property as much as a product one.

**Weaknesses.** Patch generation and verification add build complexity; a patch
chain must be reasoned about (which versions can reach which); a corrupt patch
must fail safely rather than partially.

**Opportunities.** Milestone 50 ships signed, verified, atomically activated
updates with rollback, and Milestone 59 adds fingerprint gating and channels.
Deltas are the efficiency layer over that: the content-hash naming already used
for embedded assets is the right substrate, and the same mechanism serves
desktop updates, mobile over-the-air payloads, firmware A/B slots, and model
assets (`C89`).

**Threats.** On metered or rural networks, an update that does not complete is
an update that never happened, and the fleet stays vulnerable.

**Position.** **Absent** — full packages only.

**Requirements.**

- `C135-1` `[X]` Binary delta updates with a documented patch-chain policy,
  resumable fetch, verification before activation, and full-artifact fallback.
- `C135-2` `[X]` Network-aware update policy — unmetered only, charging only,
  bandwidth ceiling, deferral window — declared per channel and per host.

## C136 — Offline-first on the web

**Introduced by.** W3, W4, and the web platform's own service-worker model.

**Mechanism.** A worker between the page and the network implements declared
caching strategies per route and asset class (cache-first, network-first,
stale-while-revalidate), precaches the application shell, queues failed
mutations for background synchronization, and manages its own updates —
including the awkward case of a new worker taking over while old pages are
open. Storage is quota-bound and evictable, so persistence must be requested
and the eviction case handled.

**Strengths.** A web application that starts offline and survives a flaky
network; installability; the same mutation-queueing model as native offline
support.

**Weaknesses.** Worker lifecycle and update semantics are a classic source of
"why am I seeing an old version" bugs; quota eviction is silent.

**Opportunities.** Web milestone I already plans service workers, and `X-DATA-2`
plans offline mutation queueing. The concept to import is *declared strategies
per route* generated from the route table (`C100`) rather than hand-written
worker code, plus an explicit update choreography and a quota policy — so the
common failure modes are designed out rather than debugged.

**Position.** **Planned** — Web milestone I names service workers and offline
applications; strategies, update choreography, and quota policy are not
specified.

**Requirements.**

- `C136-1` `[W]` Caching strategies declared per route and asset class,
  generating the worker rather than hand-writing it, with precaching of the
  shell and background synchronization of queued mutations.
- `C136-2` `[W]` Worker update choreography — activation, claiming open pages,
  and a user-visible "reload for the new version" path — plus a storage quota
  and eviction policy.

## C137 — Navigation performance on the web

**Introduced by.** W3, W4, and the browser platform's navigation features.

**Mechanism.** Making the *next* navigation instant: speculative prefetch and
prerender of likely destinations by declared rules, compatibility with the
browser's back/forward cache (which requires avoiding the patterns that
disqualify a page), scroll restoration on history navigation, view transitions
between routes (`C25`), and — the accessibility half — announcing route changes
to assistive technology, which single-page applications routinely forget.

**Strengths.** Navigation that feels immediate without a heavier client;
back and forward that behave like the browser's own; screen-reader users who
know the page changed.

**Weaknesses.** Speculation costs bandwidth and can trigger side effects if a
destination is not idempotent; back/forward-cache disqualifiers are easy to
introduce accidentally.

**Opportunities.** The router (Milestone 30, `C100`) knows the route graph and
the data layer knows what each route needs (`C29`, `C30`), so speculation rules
can be *derived* rather than hand-written, and prefetch can warm both the
document and its data. Route-change announcements belong to the accessibility
model and cost almost nothing to get right — and are a conformance item
(`X-L5-1`) rather than an optional polish.

**Position.** **Absent** — the web track plans routing and prefetch on intent
(`C42-3`) but not speculation rules, back/forward-cache compatibility, or route
announcements.

**Requirements.**

- `C137-1` `[W]` Declared speculation rules derived from the route graph, with
  side-effect-free prerendering and a bandwidth policy.
- `C137-2` `[W]` Back/forward-cache compatibility as a tested property, scroll
  restoration per history entry, and route-change announcements to assistive
  technology.

## C138 — Storage partitioning and the third-party context

**Introduced by.** the browser platform, and felt by every W-archetype.

**Mechanism.** Browsers now partition storage and cookies by top-level site,
restrict third-party contexts, require explicit access requests for embedded
content, and demand cross-origin isolation headers before granting powerful
capabilities such as shared memory and precise timers. An application embedded
in another site, or one that embeds others, must be designed for partitioning
rather than discovering it when a browser changes default.

**Strengths.** Better privacy defaults for users; capabilities gated on
isolation are safer to grant.

**Weaknesses.** Long-standing patterns (third-party sessions, shared login
iframes, cross-site analytics) break; the rules differ per browser and keep
moving.

**Opportunities.** Cross-origin isolation is already a requirement
(`C69-1`), and the framework generates the document — so it can set the right
headers, declare partitioned cookies, and *test* the embedded case rather than
leaving each application to discover it. The custom-element export (`C43-1`)
makes RustNative components embeddable in other sites, which makes the
partitioned context our problem rather than someone else's.

**Position.** **Partial** — isolation headers and secure cookie defaults are
required; partitioning, storage-access requests, and embedded-context testing
are not.

**Requirements.**

- `C138-1` `[W]` Partitioned-by-default storage and cookie handling with an
  explicit storage-access request path, and an embedded-context test that runs
  the component inside a third-party frame.

## C139 — Time synchronization and calibration

**Introduced by.** E10, E11, and E3's deployments.

**Mechanism.** Devices need to agree on time and on themselves. Network time
synchronization with drift compensation, monotonic versus wall-clock
distinction for scheduling, and — on devices with no real-time clock or no
network — a documented behaviour for "the time is unknown". Beside it,
per-device calibration: sensor offsets, display and touch calibration, and
factory data stored durably, surviving updates and factory resets.

**Strengths.** Logs that can be correlated across a fleet; schedules that hold
across reboots; sensors that read correctly on each individual unit rather than
on the reference unit.

**Weaknesses.** Time synchronization on constrained networks is fiddly; a
calibration store is one more thing to version and protect.

**Opportunities.** The host-clock contract (`X-L1-5`) already separates time
from the core, which is the right seam: a device profile adds synchronized and
unknown-time states, and the state store (`C80`) gains a calibration partition
that updates must preserve. `foundations.md` already notes that the physical
world needs a calibration knob; this is where it lives.

**Position.** **Absent.**

**Requirements.**

- `C139-1` `[E]` A device time contract: synchronized, drifting, and unknown
  states distinguished, with monotonic scheduling unaffected by wall-clock
  corrections.
- `C139-2` `[E]` A calibration and factory-data store that survives updates and
  selective factory reset, versioned like any other persisted schema.

## C140 — Field diagnostics and remote support

**Introduced by.** E2, E3, E10, and every product that has had to debug a
device in a customer's building.

**Mechanism.** Getting evidence off a device that cannot be attached to a
debugger: structured logs buffered locally with a ring policy and uploaded when
connectivity allows, crash and watchdog reports captured with enough context to
be actionable, an authenticated remote console or shell with a documented
threat model, a support bundle command that collects configuration, versions,
and recent logs, and a fingerprint (`C97-1`) in every report so the exact build
is known.

**Strengths.** Field failures diagnosed without a site visit; support requests
that arrive with evidence; reports that identify the build precisely.

**Weaknesses.** Remote access is a serious attack surface; logs may contain
personal data and need the redaction of `C127`; buffering costs flash writes.

**Opportunities.** Milestone 51 has crash capture, structured logging, and
tracing; `E-RS-2` has a reduced diagnostic channel for constrained targets;
`C127` has redaction. Field diagnostics is the packaging of all three for
devices: offline buffering, a support bundle, and a gated remote console.

**Position.** **Partial** — crash capture and a constrained diagnostic channel
exist; offline buffering, support bundles, and remote console do not.

**Requirements.**

- `C140-1` `[E]` Offline-buffered diagnostics with a ring policy and
  opportunistic upload, a `support-bundle` command collecting configuration,
  versions, fingerprint, and recent logs, and an authenticated, capability-
  gated remote console with a documented threat model.

## C141 — Manufacturing and provisioning at scale

**Introduced by.** E4, E10, and the production lines behind every shipped
device.

**Mechanism.** The steps between a built image and a sellable unit: factory
flashing with per-unit data (serial number, keys, certificates, regional
configuration), secure-element or one-time-programmable provisioning, a
manufacturing test mode with fixtures and pass/fail records, first-boot
commissioning, and factory reset that clears user data while preserving
identity and calibration.

**Strengths.** A repeatable line rather than a procedure; devices that arrive
with unique identities and certificates, which is the precondition for fleet
security; test records that satisfy quality audits.

**Weaknesses.** Deeply hardware- and vendor-specific; mistakes are shipped in
hardware and cannot be patched.

**Opportunities.** This is the far end of the toolchain milestone: `rustnative`
already builds and signs images (`C81`), and the fingerprint (`C97-1`) already
identifies builds. Provisioning contracts — per-unit data injection, a test
mode built from the same application, and a factory-reset policy that respects
the calibration store (`C139-2`) — complete the path from repository to a unit
in a box, which is the embedded equivalent of store submission (`C96`).

**Position.** **Absent.**

**Requirements.**

- `C141-1` `[E]` Provisioning contracts: per-unit data and credential injection
  at flash time, a manufacturing test mode built from the same application, and
  a factory-reset policy that clears user data while preserving identity and
  calibration.

---

## Part summary

This band is where "every target is first-class" (`PLAN.md` 2.3) is tested
hardest, because each concept belongs to a place the framework has not been
yet: a managed corporate fleet (`C133`), a desktop shell (`C134`), a metered
network (`C135`), a partitioned browser (`C138`), or a production line
(`C141`).

Two of them are the same mechanism seen from different ends and should be built
as one: delta, bandwidth-aware updates (`C135`) and field diagnostics (`C140`)
are what make a fleet maintainable, whether the fleet is phones, desktops, or
devices in a building.
