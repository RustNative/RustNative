# Concepts VI — the visual pipeline, interaction depth, and media (L2–L6)

Continues [`concepts-core.md`](concepts-core.md) (format, inclusion rule,
scoring). Requirement identifiers are `Cnn-k`.

The first four concept documents analysed the ideas that *shape* a framework:
how it renders, holds state, composes, fetches, deploys. This one analyses the
ideas that decide whether an application built with it feels like it belongs on
the device — the frame pipeline, scrolling and gesture physics, text editing,
and the media and file surfaces every real application eventually needs.

These are the concepts most often missing from a framework's *plan* and most
often decisive in its *reviews*. A user cannot name "scroll anchoring" or
"audio focus", but they can tell that a list jumps, that music stops when a
notification arrives, or that the keyboard covers the field they are typing in.

---

# Part A — The frame pipeline

## C106 — Compositing layers, damage, and overdraw

**Introduced by.** D1, M1, M2, and every host compositor; W1's engine exposes
the same machinery through style hints.

**Mechanism.** Between a widget tree and the screen sits a compositor. Subtrees
are promoted to *layers* that the GPU can transform, fade, and scroll without
re-rasterizing their contents; the framework declares which subtrees are worth
promoting and which must be repainted. *Damage* — the union of changed
rectangles — bounds the repaint. *Overdraw* analysis shows where the same pixel
is painted several times, which is the usual explanation for a slow screen that
looks simple.

**Strengths.** Scrolling and transitions at display rate without repainting
content; predictable cost; a diagnostic vocabulary ("this screen has four
layers and triple overdraw") that developers can act on.

**Weaknesses.** Layer promotion trades memory for speed and is easy to overdo —
promoted layers are texture memory, and too many of them is its own slowdown.
Promotion heuristics differ per host.

**Opportunities.** On host-native backends the host compositor does this, which
is free correctness — but the framework still decides *what it asks for*:
whether a transform animates a layer or invalidates a subtree, whether a
scroll region is a host scroller or a repaint. The draw-list path (Milestone 29)
and the terminal and embedded backends already have damage tracking
(`E-GUI-1`); the missing piece is one portable vocabulary — promote, damage,
opaque — that every backend maps onto its host's mechanism, plus overdraw and
layer counts in the inspector.

**Threats.** A framework that repaints where the host would have composited
loses the comparison on exactly the interactions users notice most.

**Position.** **Partial** — host scrolling and animation exist on Windows;
damage tracking exists on the draw-list path; no portable layer vocabulary and
no compositing diagnostics.

**Requirements.**

- `C106-1` `[X]` A portable layer and damage vocabulary — promote, opaque,
  damage region — mapped by each backend onto its host's compositing
  mechanism, with the fallback stated where a host has none.
- `C106-2` `[X]` Layer count, promotion reasons, damage regions, and overdraw
  reported by the inspection protocol and visualizable as an overlay.

## C107 — Frame pacing, refresh rate, and input-to-pixel latency

**Introduced by.** M1, M2, D1, and the platform frame-pacing libraries that
exist because getting this wrong is so common.

**Mechanism.** The application presents frames in step with the display: it
learns the refresh interval (which now varies — 60, 90, 120 Hz, and adaptive),
schedules work to finish before the deadline, and hands the frame to the
compositor early enough to be shown on the intended vsync. Input is timestamped
at arrival, so the pipeline can be measured end to end — touch to photon — and
frames that miss their deadline are attributed to the work that overran. A
hang detector notices when the main loop stops responding at all.

**Strengths.** Smoothness becomes a measured property rather than an
impression; variable-refresh displays are used correctly instead of being
fought; jank has a cause with a name; hangs are detected in production rather
than reported as "it froze".

**Weaknesses.** Pacing interacts with power management — pacing to the highest
rate drains batteries, and thermal throttling changes the deadline underneath
you. Latency measurement needs host timestamps that not every host provides.

**Opportunities.** Milestone 27's `Timeline` and Milestone 54's priorities are
the scheduling half; what is missing is the *pacing* half — a frame deadline
derived from the host's own display signal, a declared frame budget per target
(Milestone 42 has frame-time distribution but not deadline attribution), and
input timestamps carried from the host through the event model so
input-to-present latency is measurable rather than estimated. The
embedded rule that frame pacing stops when idle (`C76-1`) is the same contract
seen from the low-power end.

**Threats.** "Feels sluggish" is unarguable and unfixable without measurement,
and it is the most common subjective complaint about non-native frameworks.

**Position.** **Partial** — a timeline and frame-time budgets exist; refresh
rate, deadline scheduling, input timestamps, latency measurement, and hang
detection do not.

**Requirements.**

- `C107-1` `[X]` Frame pacing against the host's display signal, including
  variable refresh rate, with the current rate available as an environment
  value and a documented interaction with power and thermal state.
- `C107-2` `[X]` Input events timestamped at host arrival, with
  input-to-present latency measured per backend and budgeted (Milestone 42).
- `C107-3` `[X]` Frame-deadline attribution — which component, task, or service
  call overran — and a hang detector that reports an unresponsive loop through
  the diagnostic channel.

## C108 — Colour management and display capability

**Introduced by.** D1, M1, and the professional media tools that forced the
issue.

**Mechanism.** Colour is interpreted, not absolute: assets carry a colour
space, displays advertise a gamut (standard, wide, high dynamic range), and the
framework converts correctly, blends in a linear space where the host expects
it, and honours per-display profiles when a window moves between monitors.
Related display capabilities — pixel density, dynamic range, refresh rate — are
queryable rather than assumed.

**Strengths.** Brand colours that match across devices; images that do not
shift; correct blending in gradients and shadows; HDR content where the display
supports it.

**Weaknesses.** Wide-gamut correctness is subtle and invisible until someone
compares two screens; conversion costs; host support differs.

**Opportunities.** The theme system (Milestone 21) and token pipeline
(`X-UI-2`) define colours; nothing states the space they are in. Declaring it
once, converting at the backend boundary, and exposing display capability as
environment values (`C15`) is small work that prevents a whole class of
"the brand colour is wrong on this laptop" reports — and it is a prerequisite
for any serious design-tool integration.

**Threats.** Design-led teams notice immediately; everyone else notices when a
screenshot is compared with a design.

**Position.** **Absent** — colours are host-native RGB values with no declared
space; display capability is not queryable.

**Requirements.**

- `C108-1` `[X]` A declared colour space for tokens and assets, converted at
  each backend boundary, with linear blending where the host expects it.
- `C108-2` `[X]` Display capability — density, gamut, dynamic range, refresh
  rate — as environment values, updated when a window changes display.

## C109 — Material effects and backdrop rendering

**Introduced by.** D1, M1, and the design languages that made translucency a
system idiom.

**Mechanism.** Surfaces that sample what is behind them — blurred sidebars,
translucent bars, frosted sheets — plus elevation, shadow, and tint systems
that the host defines. Hosts expose these as materials rather than as filters,
because they are sampled from the compositor's own buffers and follow system
settings such as "reduce transparency" and "increase contrast".

**Strengths.** Native visual identity for free; correct behaviour under
accessibility settings; effects that would be expensive to re-implement.

**Weaknesses.** Availability and appearance differ per host and per version;
hand-rolled equivalents look wrong and cost more.

**Opportunities.** This is the fidelity argument at its most visible: a
host-native backend can *ask for the host's material* where a self-drawing
framework must approximate it. It needs a portable vocabulary of surface roles
(chrome, sheet, popover, sidebar) that map to each host's materials, with an
honest fallback and reduce-transparency handling built in.

**Threats.** An application without host materials looks dated on hosts where
they are the idiom, and this is judged in screenshots.

**Position.** **Absent** — solid backgrounds and theme colours only.

**Requirements.**

- `C109-1` `[X]` Surface-role materials mapped to each host's own material
  system, honouring reduce-transparency and contrast settings, with a declared
  fallback where a host has none.

## C110 — Offscreen rendering and snapshot capture

**Introduced by.** D1, M1, M2, and every visual-testing tool.

**Mechanism.** Rendering a subtree to an image without showing it: thumbnails,
share images, PDF export, print preview, golden tests, and the snapshots hosts
request for the application switcher. The inverse also matters — marking
content as *secure* so the host excludes it from screenshots, recordings, and
the switcher.

**Strengths.** One mechanism serves testing, sharing, printing, and export;
secure-content flags satisfy a real compliance requirement in finance and
health applications.

**Weaknesses.** Offscreen rendering must produce what the screen would, which
is harder than it sounds once host controls are involved; secure flags differ
per host and are easy to forget on one surface.

**Opportunities.** Golden tests (`X-L7-8`) and previews (`C55`) already need
this on the headless backend; extending it to real backends gives share images,
export, and print (`D-CT-2`) from one contract — and the secure-content flag is
a small addition with a large compliance payoff.

**Position.** **Partial** — headless golden capture exists; no capture on real
backends, no secure-content marking.

**Requirements.**

- `C110-1` `[X]` Subtree capture to an image or document on every backend,
  shared with golden tests, share images, export, and print.
- `C110-2` `[X]` A secure-content marker that excludes a subtree from host
  screenshots, recordings, and switcher previews, answered as a capability
  where a host cannot.

---

# Part B — Interaction depth

## C111 — Scroll systems

**Introduced by.** M1, M2, D1, and the web platform; the archetypes differ in
how much of it they own.

**Mechanism.** Scrolling is a system, not a gesture: nested and coordinated
scrolling (a list that scrolls a collapsing header before itself), snap points,
sticky and floating headers, pull-to-refresh, over-scroll effects, scroll
anchoring so content loading above does not jump, scroll restoration on
navigation, programmatic scroll-into-view for focus, and scroll-driven
animation where progress is a position rather than a clock.

**Strengths.** These behaviours are what "native feel" means in practice on
touch hosts; each one is individually small and collectively decisive.

**Weaknesses.** Every host implements them slightly differently, and
coordination between nested scrollers is where frameworks most often produce
subtly wrong behaviour.

**Opportunities.** Milestone 10 gives overflow and scrolling, Milestone 28 adds
virtualization with a scroll anchor, and Milestone 27 gives timelines. The gap
is the *system*: nested scroll coordination, snap, sticky, pull-to-refresh,
over-scroll, restoration, and scroll-driven progress as portable contracts that
each backend maps to its host's scroller — rather than as things applications
rebuild per screen.

**Threats.** This is the single most common place a cross-platform application
is identified as one.

**Position.** **Partial** — scrolling, virtualization, and a scroll anchor
exist; the coordination system does not.

**Requirements.**

- `C111-1` `[X]` Nested scroll coordination with a documented contract for
  which scroller consumes a gesture and when, mapped to each host's mechanism.
- `C111-2` `[X]` Snap points, sticky and collapsing headers, pull-to-refresh,
  and over-scroll behaviour as portable contracts with host-native realization.
- `C111-3` `[X]` Scroll restoration per navigation entry, and scroll-driven
  animation progress available to the timeline.

## C112 — Keyboard avoidance and inset choreography

**Introduced by.** M1, M2 — and it is the omission that most reliably produces
a one-star review.

**Mechanism.** When a soft keyboard, toolbar, or system panel appears, the
layout must move the focused field into view, animate in step with the host's
own animation curve and duration, respect safe areas and gesture insets
simultaneously, and restore on dismissal — including when the keyboard resizes
(prediction bar, emoji panel, floating keyboards) or when a second window is
present.

**Strengths.** Forms that work; animations that match the system rather than
racing it; correct behaviour on devices with cutouts and gesture bars.

**Weaknesses.** The host events are intricate and differ per version; naive
implementations either jump or lag by a frame.

**Opportunities.** Safe areas and cutouts are already planned as layout
properties (Milestone 39). Keyboard and system-panel insets belong to the same
contract, with the host's animation curve driving the framework's timeline —
which is a genuinely better position than re-animating with a guessed curve.

**Threats.** Every mobile framework is judged on this within minutes of the
first form.

**Position.** **Absent.**

**Requirements.**

- `C112-1` `[M]` `[D]` System insets — keyboard, toolbars, panels — as layout
  properties, animated with the host's own curve and duration, composed with
  safe areas and gesture insets.
- `C112-2` `[X]` Automatic scroll-into-view for the focused field, with a
  documented policy and an override.

## C113 — Gesture physics and cross-application drag

**Introduced by.** M1, M2, D1.

**Mechanism.** Velocity tracking from the pointer stream, fling deceleration
curves that match the host, rubber-banding at bounds, interruptible and
redirectable gestures (catching a moving card mid-flight), and drag-and-drop
that crosses application boundaries with typed payloads, file promises, and
drop-target feedback.

**Strengths.** Motion that matches the rest of the system; drag-and-drop that
works with the host's other applications rather than only inside ours.

**Weaknesses.** Curves are host-specific and undocumented in places;
cross-application drag involves per-host type identifiers and security rules.

**Opportunities.** Milestone 25 gives advanced input and Milestone 27 gives
animation; the missing contracts are velocity and deceleration derived from
host conventions, gesture interruption as a first-class state, and a typed
cross-application drag payload model that reuses the clipboard's type
vocabulary.

**Position.** **Partial** — gestures and animation exist; physics conventions
and cross-application drag do not.

**Requirements.**

- `C113-1` `[X]` Velocity tracking, host-conventional deceleration, rubber
  banding, and interruptible gestures as portable contracts.
- `C113-2` `[D]` `[M]` Cross-application drag and drop with typed payloads,
  file promises, and drop-target feedback.

## C114 — The text editing surface

**Introduced by.** D1, M1, M2; the hardest part of every self-drawing
archetype (`foundations.md` F2.2).

**Mechanism.** Beyond entering characters: selection handles and the
magnifier, the host's edit menu and smart paste, spell checking and
autocorrection with their underlines, dictation, text replacement and
autofill (including one-time codes), undo through the host's own gesture, and
input traits — capitalization, keyboard type, return-key label, secure entry,
content type for autofill.

**Strengths.** Users get the text behaviour they have everywhere else; password
managers and autofill work; dictation works; accessibility of editing works.

**Weaknesses.** Only fully available when the host's own text control is used —
which is precisely the architectural argument for host-native realization.

**Opportunities.** Milestone 12's text input and the per-backend IME work
already realize host text controls. What is missing is the *declaration
surface*: input traits, content types for autofill, secure entry, and the edit
menu as portable properties, so an application gets the host's full text
behaviour without per-backend code.

**Threats.** Autofill and password-manager compatibility is a common reason
applications are rated badly on mobile, and it is invisible until users
complain.

**Position.** **Partial** — text input and IME exist; traits, autofill content
types, secure entry, edit menu, and dictation are not portable contracts.

**Requirements.**

- `C114-1` `[X]` Input traits and autofill content types as portable text-field
  properties, including secure entry and one-time-code fields.
- `C114-2` `[X]` The host's edit menu, selection handles, spell checking, and
  dictation used rather than reimplemented, with capability answers where a
  host lacks one.

## C115 — Non-pointer navigation

**Introduced by.** D1 and M1 on television and accessibility surfaces.

**Mechanism.** Directional (spatial) navigation for remotes, D-pads, and
keyboards: focus moves by geometry rather than by tree order, with predictable
rules, focus engagement for scrollable regions, and a visible focus treatment
sized for a screen watched from three metres away. The same machinery serves
switch access, voice control (every actionable element needs a spoken label),
and full keyboard access on desktop.

**Strengths.** One mechanism opens television, automotive, accessibility, and
keyboard-power-user surfaces at once.

**Weaknesses.** Geometric focus resolution has awkward cases (overlapping,
scrolled-out, and sparse layouts) that need overrides.

**Opportunities.** Milestone 11's focus model and Milestone 26's accessibility
tree are the substrate; spatial resolution, focus engagement, and overrides are
an addition to the focus contract rather than a new system — and they convert
directly into the companion-device surfaces of Milestone 57.

**Position.** **Partial** — tab-order focus exists; spatial navigation and
engagement do not.

**Requirements.**

- `C115-1` `[X]` Spatial focus navigation with documented resolution rules,
  per-node overrides, focus engagement for scrollable regions, and a focus
  treatment scaled by input class.

---

# Part C — Media, capture, and files

## C116 — Audio session and media controls

**Introduced by.** M1, M2, D1.

**Mechanism.** Audio is a shared, arbitrated resource. An application declares
a session category (playback, recording, ambient), requests focus, and handles
losing it — ducking under a navigation prompt, pausing for a call, resuming
after. Playback publishes metadata and transport controls to the lock screen,
notification shade, and wearables; routing follows headphones, car systems, and
casting targets; background playback is a declared capability with its own
rules.

**Strengths.** Applications that behave correctly with everything else on the
device; system-level controls users expect; background audio that survives the
screen locking.

**Weaknesses.** Category and focus semantics differ per host and are full of
special cases; getting them wrong is audible.

**Opportunities.** `C28` gives media playback as a host control; the session
layer around it is the part that makes it behave. A portable session contract —
category, focus, interruption, routing, now-playing metadata — with per-host
mapping is what turns "we can play video" into "we behave like a media
application".

**Position.** **Absent** — media playback is planned as a host content control;
no session, focus, or transport-control contract.

**Requirements.**

- `C116-1` `[X]` An audio session contract: category, focus request and loss,
  ducking and interruption handling, route changes, and background playback as
  a capability.
- `C116-2` `[M]` `[D]` Now-playing metadata and transport controls published to
  each host's system surfaces.

## C117 — Capture pipelines and privacy choreography

**Introduced by.** M1, M2, D1.

**Mechanism.** Camera, microphone, and screen capture as pipelines rather than
one-shot calls: device selection, preview, configuration (resolution, frame
rate, stabilization, torch), capture, and disposal — each tied to permission
state and to the host's privacy indicators (the light or chip that tells the
user a sensor is live). Screen capture adds host consent flows and per-window
selection.

**Strengths.** Applications that can implement scanning, video calling, and
screen sharing without leaving the framework; correct permission and indicator
behaviour, which is a review requirement.

**Weaknesses.** Large per-host surfaces; hardware variation; privacy rules
change between OS versions.

**Opportunities.** Permission states (Milestone 39) and host content controls
(`C28`) exist. Capture is where they combine, and it is the capability most
often cited as a reason to drop to native code. A pipeline contract with an
honest capability surface — and a preview that is a real host view — is
achievable without owning the whole media stack.

**Position.** **Absent** — camera is a named capability with no contract.

**Requirements.**

- `C117-1` `[X]` Capture pipeline contracts for camera, microphone, and screen
  capture: device enumeration and selection, configuration, preview, capture,
  disposal, and permission and privacy-indicator behaviour per host.

## C118 — Privacy-preserving pickers and file providers

**Introduced by.** M1, M2, D1 — a direct response to storage permissions being
too coarse.

**Mechanism.** Instead of granting an application access to all photos or all
files, the *host* presents a picker and returns only what the user chose, often
without any permission prompt at all. The same model covers document pickers,
security-scoped bookmarks for continued access, file providers that expose an
application's own documents to other applications, drag-in payloads, and file
promises for deferred content.

**Strengths.** Less permission friction, less risk, better review outcomes; the
picker is the host's, so it stays current.

**Weaknesses.** Scoped access is easy to lose (bookmarks expire, URIs become
stale); the model differs per host in detail.

**Opportunities.** Milestone 23 already has native file dialogs. Extending to
photo pickers, scoped bookmarks, file providers, and promises makes storage
access a capability with an honest surface rather than an all-or-nothing
permission — and it is the correct answer to mobile scoped storage, which is
otherwise a per-backend mess.

**Position.** **Partial** — file dialogs exist on Windows; pickers, scoped
bookmarks, providers, and promises do not.

**Requirements.**

- `C118-1` `[X]` Photo and document pickers, scoped-access bookmarks with
  renewal, file providers, and file promises as portable contracts with
  per-host mapping.

## C119 — Background transfer

**Introduced by.** M1, M2 — because hosts kill processes and networks fail.

**Mechanism.** Uploads and downloads that continue when the application is
backgrounded or terminated, resume after interruption, respect metered and
low-data settings, report progress, and deliver completion as an entry point
into the application — with the host's own transfer service doing the work.

**Strengths.** Large transfers that actually finish; correct behaviour on
constrained networks; no foreground-only assumptions.

**Weaknesses.** Host services impose their own lifecycles and limits, and
debugging them is unpleasant.

**Opportunities.** Milestone 47's data layer and `M-FP-2`'s constrained
background work are the neighbours; background transfer is the third piece and
the one that turns "we can fetch" into "we can ship a file-heavy application".
It also pairs with delta updates (`C135`) on constrained networks.

**Position.** **Absent.**

**Requirements.**

- `C119-1` `[X]` A background transfer contract: resumable uploads and
  downloads, progress reporting, metered and low-data policy, and completion
  delivered as an entry point, mapped to each host's transfer service.

---

## Part summary

Three of these — keyboard avoidance (`C112`), scroll systems (`C111`), and
the text editing surface (`C114`) — are the concepts most likely to decide
whether users describe an application as native, and all three are currently
**Partial** or **Absent** in a plan that is otherwise strong on the
architecture beneath them.

Three more — materials (`C109`), capture (`C117`), and pickers (`C118`) — are
where host-native realization pays a dividend a self-drawing framework cannot
match, provided the contracts exist to ask for them.

And two — frame pacing with latency measurement (`C107`) and compositing
diagnostics (`C106`) — turn "feels smooth" from an opinion into a number, which
is the same move Milestone 42 made for startup and size.
