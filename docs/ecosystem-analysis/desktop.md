# Desktop archetypes — Windows, macOS, Linux

Read [`foundations.md`](foundations.md) first. Each archetype below is analysed
bottom-up from its root choices (substrate, loop ownership, realization) to its
ecosystem, in the order of [`method-and-stack.md`](method-and-stack.md).

RustNative's desktop targets are the shipped Windows backend and `PLAN.md`
Milestones 33 (macOS) and 34 (Linux). Tags: `[D]` desktop, `[X]`
cross-cutting.

---

## D1 — The current-generation first-party native toolkit

**Root (L0–L2).** The host vendor's own current UI stack: a managed or
host-language substrate, the host's message loop, and a retained tree of host
widget objects composited by the system (F2.1). Declarative markup or a
declarative language surface sits on top of that retained tree.

**Semantics and model (L3–L4).** Two-pass measure/arrange layout (F3.2), the
host's own text stack (F3.3), styling through the vendor's appearance system,
and a data-binding or observation model as the state mechanism (F4.1, observer
or signal variants).

**Integration (L5–L6).** Best in class, by construction: accessibility,
automation, input methods, locale formatting, right-to-left mirroring, high
contrast, text scaling, and every system convention are the same code the host
uses for its own applications. L6 varies — first-party stacks usually ship
persistence and often a reactive data layer, but rarely routing or a data
cache.

**Loop and ship (L7–L8).** Vendor tooling: visual designers, live previews,
profilers integrated with the OS, and first-party packaging and store paths.
Iteration speed ranges from excellent (with a preview canvas) to mediocre.

**Project (L9).** Vendor documentation, vendor support, vendor roadmap — and
vendor deprecation. This archetype's history is a sequence of superseded
stacks, each leaving applications stranded.

**Strengths.** Perfect host fidelity; zero accessibility work; system settings
honoured for free; first-class packaging and store acceptance; deep OS
integration available immediately when the OS ships it.

**Weaknesses.** One platform only. Vendor churn: the "recommended" stack
changes every several years, and migration is the application's problem.
Language lock-in. Interop with other languages ranges from adequate to hostile.

**Opportunities for RustNative.** This archetype is what our L2 choice makes us
comparable to — the same host objects, the same accessibility, the same
conventions — while being portable, which none of them are. It is also the
correct *fidelity reference*: our conformance target is the host's own
first-party applications, not our other backends (`X-L2-1`). Their vendor churn
is our stability argument, provided we actually have a stability policy
(`W-MF-8`).

**Threats.** On its own platform this archetype is unbeatable on fidelity and
on time-to-first-feature for a single-platform team, and a large share of
desktop software is single-platform.

**Concepts introduced here.** `C15` environment down, preferences up; `C18`
modifier chains, attached properties, value precedence; `C19` lookless and
headless controls; `C20` the command model; `C22` adaptive layout; `C25`
shared-element transitions; `C26` model/view, proxies, identity snapshots;
`C27` document-based architecture; `C28` host content controls; `C49` surfaces
beyond the main window; `C55` live previews and catalogues; `C56` round-trip
visual designers. Each is analysed on its own merits, independently of this
archetype, in [`concepts-app.md`](concepts-app.md),
[`concepts-core.md`](concepts-core.md),
[`concepts-delivery.md`](concepts-delivery.md).

**What we must ship.**

- `D-FP-1` `[D]` Per-backend fidelity conformance against the host's own
  first-party applications: control appearance, focus visuals, scroll physics,
  text rendering, context-menu and drag conventions, and system settings
  (contrast, reduced motion, text scale, colour scheme).
- `D-FP-2` `[D]` Support the host's *current* control set, with a documented
  policy for newer controls that appear in later OS versions and a graceful
  path on older ones.

---

## D2 — The legacy first-party native stack

**Root (L0–L2).** The host's original C-level API: a message pump the
application drives directly, window handles as the unit of UI, and painting by
message. The application *is* the loop.

**Semantics and model (L3–L4).** No framework-level layout — geometry is
computed and assigned. No state model. Identity is the handle. Everything above
L2 is the application's own invention.

**Integration (L5–L6).** Complete access to everything the OS has, earliest,
with no abstraction in between. Accessibility exists but must be wired up
manually.

**Loop and ship (L7–L8).** Debuggers and system-level tooling are excellent;
there is nothing framework-shaped to inspect. Deployment is a single binary
with no runtime dependency, which is still the lowest-friction desktop
distribution that exists.

**Strengths.** Unmatched longevity — binaries built decades ago still run;
minimal footprint; total OS access; no runtime dependency.

**Weaknesses.** Everything above L2 is hand-built; high defect rates in
exactly the places that are hard (text, accessibility, high-DPI, input
methods); no portability; a tiny and shrinking talent pool.

**Opportunities for RustNative.** This archetype is the substrate our Windows
backend already sits on, which means our escape hatch reaches the full OS
surface (`X-L0-6`). It is also the population most likely to adopt us: teams
maintaining these applications need modernization without rewriting, and
*embedding* a RustNative subtree inside an existing host window is the adoption
path that makes that possible.

**Threats.** None competitively — but its longevity sets the expectation that a
desktop binary keeps working for a decade, and that is a stability bar we
inherit.

**Concepts introduced here.** `C20` the command model; `C56` round-trip visual
designers. Each is analysed on its own merits, independently of this archetype,
in [`concepts-core.md`](concepts-core.md),
[`concepts-delivery.md`](concepts-delivery.md).

**What we must ship.**

- `D-LG-1` `[D]` Host-window embedding: realize a RustNative tree into a
  caller-supplied host window or view, so it can be hosted inside an existing
  application (= `X-INTEROP-1` for desktop).
- `D-LG-2` `[D]` The reverse direction: a foreign host object adopted as a leaf
  of our tree, laid out and clipped by our layout model.

---

## D3 — The managed cross-platform retained toolkit

**Root (L0–L2).** A managed runtime (F0.2) with either per-OS native controls
behind one API, or its own drawn controls, depending on the family member.
Loop ownership is the host's; realization is the interesting axis and the
families differ on it — which is why members of this archetype behave very
differently on L5 despite sharing an API.

**Semantics and model (L3–L4).** Declarative markup with data binding, a
measure/arrange layout model, and a styling system with themes.

**Integration (L5–L6).** Where members realize into host controls, fidelity and
accessibility are good; where they draw their own, both are re-implementations
with the permanent lag described in F2.2. Persistence, navigation, and
dependency injection are usually provided.

**Loop and ship (L7–L8).** Good tooling, hot reload of the markup layer, and
per-platform packaging with real store support. Startup and memory carry the
managed runtime.

**Project (L9).** Large ecosystems, strong enterprise support, and — again —
vendor churn between successive cross-platform attempts.

**Strengths.** One codebase across desktop and mobile; strong data binding;
mature enterprise tooling; genuine store-ready packaging for every target.

**Weaknesses.** The managed runtime's floor on startup, memory, and artifact
size; per-platform behavioural divergence that surfaces late; abstraction
leaks where the shared API cannot express a platform's control; and the
deep-customization ceiling of whichever realization was chosen.

**Opportunities for RustNative.** Our substrate removes the runtime floor
outright, and our capability model (`PLAN.md` 2.5) is a better answer to
platform divergence than a lowest-common-denominator API. Their data binding is
genuinely good ergonomics worth learning from — in our terms, that is the
shared-state contract (`X-L4-1`) plus forms (`W-SF-7`).

**Threats.** For enterprise line-of-business software this archetype is the
default, and it is default because of tooling and support, not technology.

**Concepts introduced here.** `C16` scoped dependency injection; `C18` modifier
chains, attached properties, value precedence; `C19` lookless and headless
controls; `C23` platform-adaptive components; `C24` per-property native
mappers; `C55` live previews and catalogues; `C56` round-trip visual designers.
Each is analysed on its own merits, independently of this archetype, in
[`concepts-app.md`](concepts-app.md), [`concepts-core.md`](concepts-core.md),
[`concepts-delivery.md`](concepts-delivery.md).

**What we must ship.**

- `D-MC-1` `[X]` A declarative two-way binding ergonomics pass over the
  component API: binding a control's value to a state cell should be one
  expression, with validation and dirty state included (depends on `X-L4-1`,
  `W-SF-7`).

---

## D4 — The self-drawing cross-platform toolkit

**Root (L0–L2).** Any substrate; the decisive choice is F2.2 — the framework
owns a scene graph and draws every control itself with the GPU. Often it owns
the loop as well.

**Semantics and model (L3–L4).** Its own layout model, its own styling, its own
text stack (or a bundled shaper), and its own animation system. Complete
control, complete responsibility.

**Integration (L5–L6).** The archetype's permanent weakness. The accessibility
tree is synthesized and bridged per platform, and it lags. System settings are
re-implemented one at a time. Text input, IME composition, selection, and caret
behaviour must be rebuilt per host and are where the longest-lived defects
live.

**Loop and ship (L7–L8).** Often excellent loops (drawn UI is easy to hot
reload) and self-contained artifacts that carry a renderer and a font stack.

**Strengths.** Pixel-identical output everywhere; total design freedom; one
implementation of every control; immunity to host control limitations and host
version differences; strong animation and graphics stories.

**Weaknesses.** Everything in F2.2's ceiling: accessibility permanently behind,
system settings ignored or re-implemented, input methods hard, artifact size
inflated, and every host release a chase. Users perceive the result as
"not a real application" in ways they cannot always articulate — scroll
physics, focus behaviour, menu conventions, text selection.

**Opportunities for RustNative.** This is the archetype our L2 choice is
*aimed* at, and the argument must be made empirically rather than rhetorically:
the same reference application, native versus self-drawn, evaluated with each
host's own screen reader, input methods, automation tooling, and accessibility
settings, with method and results published (`X-L2-2`). Their design-freedom
advantage is also partly answerable — our draw-list path (Milestone 29) can
host bespoke visuals inside a natively realized tree, which is the combination
neither pure archetype offers.

**Threats.** Design-led teams choose this archetype for brand control and will
not trade it for fidelity. We should not pretend to win that argument; we
should win the one about accessibility, input methods, and system integration —
and offer the draw-list path where brand control is non-negotiable.

**Concepts introduced here.** `C19` lookless and headless controls; `C23`
platform-adaptive components. Each is analysed on its own merits, independently
of this archetype, in [`concepts-app.md`](concepts-app.md),
[`concepts-core.md`](concepts-core.md).

**What we must ship.**

- `D-SD-1` `[D]` A documented hybrid pattern: custom-drawn subtrees inside a
  natively realized tree, with accessibility semantics still supplied by the
  portable model for the drawn region.
- `D-SD-2` `[X]` The published fidelity/accessibility comparison methodology
  and its results (= `X-L2-2`).

---

## D5 — The compiled cross-platform toolkit with its own object system

**Root (L0–L2).** Compiled native substrate with a bolted-on object system
(introspection, signal/slot connections, a code generator) to supply what the
language lacks. Realization is per-platform styled drawing in the modern
members, with host integration hooks.

**Semantics and model (L3–L4).** A very complete stack: layouts, models and
views, its own string and container types, a declarative UI language with a
scripting runtime, animation, and a property/binding system.

**Integration (L5–L6).** Deep and broad: accessibility bridges, input methods,
printing, networking, databases, multimedia, and serial/industrial protocols.
This archetype ships more L6 than any other desktop family.

**Loop and ship (L7–L8).** Mature designers, profilers, and per-platform
deployment tooling; a long history of commercial support.

**Project (L9).** Decades of documentation and a large industrial installed
base, especially in embedded, medical, industrial control, and automotive —
which is exactly the territory `PLAN.md` Milestone 37 targets. Licensing is a
recurring decision point for commercial users.

**Strengths.** Breadth no other desktop archetype matches; the same stack from
workstation to embedded device; strong industrial credibility; a declarative UI
layer with real designer tooling.

**Weaknesses.** A parallel object system and code generator layered on the
language; heavy build and large artifacts; its own types and idioms that
propagate through the application; and the usual self-drawing trade where its
modern UI layer draws rather than realizes host controls.

**Opportunities for RustNative.** Rust gives compile-time expressiveness with
no bolted-on object system or code generator — the mechanism this archetype
needed and we do not. The strategic lesson is its *reach*: one stack from
desktop to embedded, which is exactly RustNative's intended span and a strong
argument to make deliberately rather than incidentally. Its breadth also shows
what industrial buyers actually require — serial and fieldbus protocols,
printing, charting, and database access — none of which appear in our plan.

**Threats.** In industrial and embedded procurement this archetype is the
incumbent, with certification precedents and long-term support contracts we
cannot match for years.

**Concepts introduced here.** `C10` statecharts; `C18` modifier chains,
attached properties, value precedence; `C20` the command model; `C26`
model/view, proxies, identity snapshots; `C27` document-based architecture;
`C56` round-trip visual designers; `C66` generated bindings from interface
descriptions. Each is analysed on its own merits, independently of this
archetype, in [`concepts-app.md`](concepts-app.md),
[`concepts-core.md`](concepts-core.md),
[`concepts-delivery.md`](concepts-delivery.md).

**What we must ship.**

- `D-CT-1` `[X]` A stated and demonstrated one-stack span: the same application
  model from desktop to embedded, with an example that is genuinely built for
  both (this is the Milestone 37/38 argument made visible).
- `D-CT-2` `[D]` Industrial service contracts where they are portable —
  printing, serial/device I/O, and database access — behind capability
  contracts.

---

## D6 — The thin native wrapper toolkit

**Root (L0–L2).** A compiled substrate wrapping each host's own widgets behind
one API (F2.1 per platform), with no drawing of its own.

**Strengths.** True host fidelity and accessibility for free; small artifacts;
long-lived and stable; conceptually simple.

**Weaknesses.** The API is the intersection of what all hosts offer, so it is
lowest-common-denominator by construction; layout and styling are thin;
modern visual design is hard; the ecosystem is small.

**Opportunities for RustNative.** This archetype demonstrates both that our L2
choice works and *how it fails if we are careless*: the intersection trap is
precisely what `PLAN.md` section 1 forbids, and the capability model plus the
escape hatch are the defences. Those defences should be tested rather than
assumed — a capability that no backend advertises is a design smell worth
detecting.

**Threats.** Minimal; the archetype is a cautionary example more than a
competitor.

**Concepts introduced here.** `C23` platform-adaptive components. Each is
analysed on its own merits, independently of this archetype, in
[`concepts-app.md`](concepts-app.md).

**What we must ship.**

- `D-TW-1` `[X]` A portable-surface review gate: when a control or service is
  added to the portable API, the design record must state what each backend
  does when its host differs, and any flattening must be justified in writing.

---

## D7a — The bundled-engine web shell

**Root (L0–L2).** A complete browser engine plus a server-side dynamic runtime,
shipped inside the application. Realization is the document tree (F2.4) inside
an embedded engine; the loop is the engine's.

**Semantics and model (L3–L4).** The web stack entirely, plus a privileged
process for host access.

**Integration (L5–L6).** Accessibility, input methods, and internationalization
come from the engine and are good. System conventions do not — menus, scroll
physics, window behaviour, and appearance are approximations unless
hand-built. Host access goes through a privileged bridge with an explicit
security boundary.

**Loop and ship (L7–L8).** The web loop and the web ecosystem, which is the
whole point. Artifacts start at roughly a hundred megabytes and resident memory
at hundreds; startup is slow; each application ships and must update its own
engine, which is a security-patching obligation most teams do not honour.

**Strengths.** Web skills and the entire web ecosystem applied to desktop; one
UI codebase across desktop platforms and the browser; a consistent, fully
controllable design; extremely fast time to first release.

**Weaknesses.** Footprint and startup, measured in multiples of every other
archetype; memory cost that makes multiple such applications untenable on
modest machines; per-application engine patching as a security liability; weak
host conventions; a privileged bridge that is a genuine attack surface.

**Opportunities for RustNative.** Footprint and startup are the arguments, and
they are arguments users already make themselves. The deeper one is security:
a compiled application with a capability-scoped service layer has a far smaller
attack surface than a bundled engine with a privileged bridge, and this is the
kind of claim procurement understands. Their genuine advantage — designers and
web developers being immediately productive — is answerable only through our
own L7 and L9 work, not by argument.

**Threats.** This archetype wins on time-to-market and on hiring, and both
matter more than footprint to most teams shipping their first version.

**Concepts introduced here.** `C67` multi-process isolation; `C68` capability
as authorization. Each is analysed on its own merits, independently of this
archetype, in [`concepts-delivery.md`](concepts-delivery.md).

**What we must ship.**

- `D-WS-1` `[X]` Published desktop footprint budgets — artifact size, resident
  memory, cold start — with a reference application measured per platform in CI
  (= `X-L0-1`).
- `D-WS-2` `[X]` A stated desktop threat model: what the service layer exposes,
  how capabilities are scoped, and what an escape hatch can reach.

---

## D7b — The system-webview shell

**Root (L0–L2).** The same document realization, but using the *host's* webview
rather than bundling an engine, with a compiled native host process.

**Strengths.** A fraction of D7a's artifact size; engine patched by the OS; a
compiled host process with a small native surface; the web ecosystem retained
for UI.

**Weaknesses.** Engine behaviour differs per platform and per OS version, which
returns a compatibility-matrix problem the web had largely solved; host
conventions are still approximations; and the UI is still a web page inside a
native frame, so fidelity, accessibility semantics, and system settings inherit
D7a's position rather than D1's.

**Opportunities for RustNative.** This archetype is our nearest philosophical
neighbour — compiled host, native shell, small artifact — and the difference is
exactly one layer: they realize into a webview, we realize into host controls.
That makes it the clearest possible statement of our value: same footprint
argument, plus real host controls, real accessibility, and no per-version
engine matrix. It also proves that a compiled process with a web-facing
capability layer is an accepted architecture, which de-risks the shape of our
own service model.

**Threats.** It is a credible, actively improving competitor that already wins
the footprint argument against D7a, so we cannot win on footprint alone; the
differentiator must be L2 and L5.

**Concepts introduced here.** `C67` multi-process isolation; `C68` capability
as authorization. Each is analysed on its own merits, independently of this
archetype, in [`concepts-delivery.md`](concepts-delivery.md).

**What we must ship.**

- `D-WV-1` `[D]` A documented comparison on the axes that actually separate us:
  realization, accessibility, system settings, engine-version exposure, and
  update responsibility.

---

## D8 — Immediate-mode GUI libraries

**Root (L0–L2).** No retained UI objects; the interface is re-emitted every
frame into a draw list, with identity hashed from call sites (F2.3). Usually
embedded in an existing render loop.

**Strengths.** Minimal integration cost inside a graphics application; trivial
mental model; excellent for tools, debug overlays, and editors; tiny.

**Weaknesses.** F2.3's ceiling — no retained object means no host accessibility
and no automation; continuous redraw costs power; text input and IME are
minimal; layout is largely manual; large interfaces scale poorly.

**Opportunities for RustNative.** Two. First, the ergonomic lesson: developers
like describing the whole UI every time, and a retained framework with good
reconciliation gives them that without the ceiling — worth demonstrating
explicitly (`X-L2-3`). Second, the use case: debug overlays and developer tools
*inside* an application are exactly what our inspector needs (`X-L7-4`), and
the draw-list path can host them.

**Threats.** None in application software; it is dominant in its own niche and
should stay there.

**What we must ship.**

- `D-IM-1` `[X]` An in-application diagnostics overlay on the draw-list path —
  tree, layout, events, frame cost — available on every backend (feeds
  `X-L7-4`).

---

## D9 — Low-level windowing and graphics APIs

**Root (L0–L2).** Not UI frameworks at all: window and input abstraction, and
the host's graphics APIs. They are the floor everything else stands on.

**Strengths.** Complete control; the only path for games, simulation, and media
software; direct access to GPU capability.

**Weaknesses.** No UI model of any kind; everything above L2 is the
application's.

**Opportunities for RustNative.** These are not competitors but *dependencies
and neighbours*. Two obligations follow. First, our custom-drawing escape hatch
should be able to hand a region to code using the host's graphics API directly,
rather than only accepting our draw list — that is what makes RustNative usable
for applications with a rendering core and a native UI shell. Second, our
embedded and terminal targets already use a draw-list path, so the same seam
serves three purposes.

**Threats.** None.

**What we must ship.**

- `D-GX-1` `[D]` A surface-handoff escape hatch: a tree node that owns a
  host-native rendering surface, with documented lifetime, resize, DPI, and
  present semantics, laid out and clipped by our layout model.

---

## D10 — The mobile framework brought to the desktop

**Root (L0–L2).** A mobile UI stack running on a desktop host through a
compatibility layer, or a mobile-first declarative framework extended with
desktop affordances.

**Strengths.** Code reuse from an existing mobile application; fast path to a
desktop presence; the vendor's own accessibility and text stacks come along.

**Weaknesses.** Desktop conventions arrive late and partially — menu bars,
multiple windows, keyboard-first workflows, pointer affordances, window
management, and drag-and-drop are the recurring gaps. Users identify these
applications immediately, and the reason they can is that the *application
model* was designed for one host class and stretched to another.

**Opportunities for RustNative.** `PLAN.md`'s insistence that no target is a
port of another (2.3) is precisely the defence against this failure, and this
archetype is the evidence that the failure is real rather than theoretical.
The concrete obligation is that desktop-class affordances must be portable
*concepts* with honest capability answers — multiple windows, menus, keyboard
shortcut maps, pointer hover and cursors, drag-and-drop — rather than
Windows-shaped features other backends imitate.

**Threats.** Low as a competitor; high as a cautionary pattern we could fall
into from the other direction, by treating the shipped Windows backend as the
shape of the portable model.

**Concepts introduced here.** `C22` adaptive layout; `C23` platform-adaptive
components. Each is analysed on its own merits, independently of this
archetype, in [`concepts-app.md`](concepts-app.md).

**What we must ship.**

- `D-PT-1` `[X]` A desktop-class affordance audit of the portable API — window
  management, menus, shortcut maps, hover and cursors, drag-and-drop,
  multi-window state — each expressed as a capability rather than assumed.
- `D-PT-2` `[X]` A reverse audit before each new backend: which parts of the
  portable API encode a Windows assumption, resolved by widening the contract
  rather than by conditional code (`PLAN.md` 2.4).

---

## Per-host obligations the archetypes make visible

Gathered here because they are what Milestones 33 and 34 will be judged on, and
because the archetypes above earn their fidelity precisely by handling them.

**Windows.** High-DPI and per-monitor scaling with runtime changes; light/dark
and accent colour tracking; the host's modern control set alongside the classic
one; touch and pen; the shell integration users expect (jump lists, taskbar
progress, notifications, file associations); installer, signing, and store
packaging; and accessibility verified with the host's own screen readers.

**macOS.** Menu bar conventions and the application menu; window restoration
and state; full-screen and stage-style window management; trackpad gestures and
momentum scrolling; services and share; appearance and accent tracking;
sandboxing and entitlements; notarization; and accessibility and text input via
the host's own stacks. `PLAN.md` Milestone 33 covers most of this already; the
gaps are sandboxing/entitlements as capability answers and window restoration.

**Linux.** Two display servers with genuinely different capabilities — window
placement, global menus, client-side decorations, screen capture, and input
grabs differ, and the correct response is capability answers rather than
emulation; portal-mediated services; multiple desktop environments with
different conventions; theme and icon-theme integration; and at least one
distribution format with a story for the others. `PLAN.md` Milestone 34 states
the display-server position correctly; what is missing is desktop-environment
conformance and a scaling story for mixed-DPI multi-monitor sessions.

- `D-WIN-1` `[D]` Per-monitor DPI change handled live, verified on a mixed-DPI
  multi-monitor session.
- `D-WIN-2` `[D]` Framework-owned boxes decorated through the host's own
  compositor — shadows, anti-aliased corners, gradients, fades — without
  capturing or owner-drawing a native control, and with the limits of mixing
  composed content and native child windows (the airspace problem) answered as
  capability rather than discovered.
- `D-WIN-3` `[D]` A recorded decision, with written criteria, on whether the
  backend realizes the host's newer control set (whose composition has no
  airspace limit) alongside or instead of the classic one — taken as a
  control-set decision under `D-FP-2`, not as a styling one.
- `D-MAC-1` `[D]` Sandbox and entitlement requirements expressed as
  capabilities, with window and state restoration implemented.
- `D-LIN-1` `[D]` Desktop-environment conformance matrix and mixed-DPI
  multi-monitor scaling, both verified under each display server.

---

## Summary: the desktop opening, ranked

1. **Fidelity plus portability (`D-FP-1`, `X-L2-2`).** The combination D1 and
   D4 each hold half of. It only counts if it is measured against first-party
   applications and published.
2. **Footprint and security against web shells (`D-WS-1`, `D-WS-2`).** The
   argument users already make themselves; ours to lose by not measuring.
3. **Embedding and incremental adoption (`D-LG-1`, `D-LG-2`, `D-GX-1`).** The
   only realistic path into the large installed base of existing desktop
   applications, and the one thing the whole competitive set is weak at.
4. **One stack from desktop to embedded (`D-CT-1`).** The reach argument, with
   exactly one incumbent, in markets that buy on longevity.
5. **Desktop-class affordances as portable concepts (`D-PT-1`, `D-PT-2`).**
   Cheap now, extremely expensive to retrofit after three backends exist.
