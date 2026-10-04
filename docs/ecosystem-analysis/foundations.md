# Foundations: the root layers every framework is built from

Read this before the platform documents. It analyses the *strategies available
at the bottom of the stack* — substrate, execution, realization, identity,
layout, text, input, state, effects, accessibility, styling, and build — and
what each one permanently allows and forbids above it.

The platform documents then treat concrete archetypes as paths through these
choices. When one of them is called strong or weak at layer 7, the reason is
almost always a choice made here, at layer 0, 1, or 2.

For each root strategy: the mechanism, who picks it, its strengths, its
weaknesses, **the ceiling it imposes** (the part that cannot be fixed by
effort above it), RustNative's position, and what follows for us.

Requirement tags: `[X]` cross-cutting, `[W]` web, `[D]` desktop, `[M]` mobile,
`[E]` embedded/RTOS, `[T]` terminal.

---

# L0 — Substrate: language, runtime, compilation, FFI

Everything about startup, memory floor, artifact size, and *where type errors
surface* is decided here. No amount of tooling above moves it.

## F0.1 — Interpreted / just-in-time dynamic runtime

**Mechanism.** Source or bytecode shipped to the target, compiled or
interpreted at runtime, dynamically typed, garbage-collected, with a large
standard runtime present before application code runs.

**Strengths.** Fastest edit-to-running loop physically possible — no compile
step at all; code is data, so hot replacement, live inspection, and
introspection-driven tooling are nearly free; enormous package ecosystems;
lowest barrier to first contribution.

**Weaknesses.** The runtime is present in every artifact: a memory floor per
process, a startup cost before the first line of user code, and a size that
dominates small applications. Types are checked — if at all — by a separate
tool that erases before execution, so every boundary (network, storage, foreign
calls, plugin) is unchecked at the point it matters. Garbage collection
introduces pauses that are bounded only statistically, which is exactly the
wrong property for frame-paced UI and for hard-real-time targets. Concurrency
is usually single-threaded by design, so CPU-bound work blocks the UI unless
manually offloaded.

**Ceiling.** Cannot reach deterministic destruction, cannot reach sub-millisecond
cold start, cannot be deployed to memory-constrained or bare-metal targets, and
cannot make compile-time guarantees about anything crossing a boundary. These
are not maturity problems; they are the substrate.

**Follows for us.** Every advantage this substrate has is at L7 (the loop) and
L9 (the ecosystem), and both are *reachable by us*. Every advantage we have is
at L0–L1 and unreachable by them. The strategic implication is blunt: our L7
gap is the one that must close, because it is the only one where the trade is
symmetrical.

- `X-L0-1` `[X]` Publish and enforce the numbers this substrate cannot match:
  cold start, resident memory, artifact size, and frame-time distribution
  including worst case — per target, in CI, with regressions failing the build.

## F0.2 — Managed virtual machine with ahead-of-time compilation

**Mechanism.** Statically typed language on a garbage-collected virtual
machine, with an ahead-of-time compilation mode that produces a native image
with a trimmed runtime.

**Strengths.** Strong static typing with mature tooling; ahead-of-time
compilation removes much of the startup and memory disadvantage; excellent
profilers, debuggers, and long-lived operational tooling; genuine multithreading.

**Weaknesses.** Ahead-of-time mode restricts reflection and dynamic loading,
which much of its own ecosystem depends on, so the fast path and the
ecosystem path are in tension. Garbage collection remains, so pause behaviour
remains. The runtime is trimmed, not removed.

**Ceiling.** Deterministic destruction and no-allocation paths remain out of
reach, which closes hard-real-time and most bare-metal targets.

**Follows for us.** This is the most credible substrate competitor: it is
closing the startup and footprint gap deliberately. Competing on those numbers
alone is not durable — the durable difference is *deterministic* lifetime and
destruction, which is a correctness property, not a performance one.

- `X-L0-2` `[X]` State native-object lifetime as a guarantee, not an
  implementation note: a host object's destruction point is defined by the
  tree, and a leak is a test failure (leak diagnostics exist in `PLAN.md`
  section 9 as "eventually" and must become a gate).

## F0.3 — Compiled native with a garbage collector

**Mechanism.** Statically typed, compiled to a native binary, with a runtime
and collector linked in. Single-binary deployment, fast startup, good
throughput.

**Strengths.** Single-artifact deployment; small enough images for per-request
hosts; simple concurrency primitives; fast builds.

**Weaknesses.** Pause behaviour remains; no zero-cost abstraction over
lifetimes, so foreign-object ownership is manual and error-prone; type systems
in this family are usually deliberately modest, so illegal states are harder to
make unrepresentable.

**Ceiling.** Same as F0.2 for real-time and bare-metal, with less compile-time
expressiveness.

**Follows for us.** This substrate is a direct neighbour on the server and
loses to us at L4 (expressiveness) and at hard-real-time targets. It beats us
at build speed and learning curve — both of which are addressable.

- `X-L0-3` `[X]` Build-time budget: a clean build and an incremental build time
  per target, tracked like any other budget, because compile time is the tax
  our substrate pays and it is the one developers feel hourly.

## F0.4 — Compiled native with ownership-based memory management

**Mechanism.** Statically typed, compiled, no collector; lifetimes and
ownership checked at compile time; destruction is deterministic and its point
is known statically.

**Strengths.** No runtime floor, no pauses, deterministic destruction (which
maps exactly onto host-object ownership and foreign resources), data-race
freedom for shared state, zero-cost abstraction, one artifact, and the widest
possible target range — the same core can reach a browser sandbox and a
microcontroller. Illegal states can be made unrepresentable rather than
merely discouraged.

**Weaknesses.** Compile times are the worst of any substrate here; the learning
curve is real; UI trees are graph-shaped and graphs fight ownership, so
identity, parent links, and callbacks need deliberate design (this is precisely
why `PLAN.md` uses stable IDs and a framework-owned tree rather than
parent-pointer nodes); hot code replacement is hard, because the thing that
makes it easy elsewhere — a dynamic runtime — is exactly what was removed;
ecosystem breadth for UI and application concerns is thin.

**Ceiling.** Live code replacement and introspection-driven tooling must be
*engineered*, not inherited. If they are not engineered, this substrate's L7
story stays worse than every competitor's, permanently.

**RustNative's position.** This is our substrate. Milestones 1–32 have already
paid the graph-versus-ownership cost and kept the benefits.

- `X-L0-4` `[X]` Treat the L7 consequences of this substrate as first-class
  engineering, not as a limitation to be explained: state-preserving reload,
  live tree inspection, and runtime introspection must be designed *into* the
  runtime rather than bolted on (see `F7.*`).

## F0.5 — The foreign-code boundary

**Mechanism.** Every framework that is not written in its host's own language
crosses a boundary. The shapes in use: serialized asynchronous message bridges,
handle-and-reference models with attachment rules, direct interoperability with
the host's object model, imported sandbox functions, and plain C calling
conventions.

**Strengths and weaknesses by shape.**

- *Serialized asynchronous bridge.* Simple, safe, and debuggable; but it is a
  throughput ceiling, forces asynchrony onto operations the host performs
  synchronously, and makes per-frame work impossible. Frameworks built this way
  spend years engineering around it.
- *Handle and reference model.* Fast, but ownership, thread attachment, and
  reference lifetime become the developer's problem, and mistakes are
  use-after-free or leaks rather than exceptions.
- *Direct host-object-model interoperability.* Best fidelity and lowest cost;
  requires per-host unsafe code and careful memory-management convention
  matching.
- *Sandbox imports.* Safe and fast within the sandbox; every capability must be
  explicitly imported, so absence is visible rather than surprising.
- *C calling convention.* Universal, minimal, and unopinionated; carries no
  ownership information at all, so the rules must be documented and enforced by
  hand.

**Ceiling.** A framework whose boundary is a serialized bridge can never have
synchronous host measurement, per-frame host mutation, or first-class host
escape hatches — the three things `PLAN.md` treats as fundamental (2.2, 2.6,
`IntrinsicMeasurer`).

**RustNative's position.** Direct interoperability with per-host unsafe
confined to the backend crate, which is the correct choice and the expensive
one.

- `X-L0-5` `[X]` One ownership module per backend, with the host's memory
  convention written down and asserted in tests — the rule
  `rustnative-windows` already follows, applied to every backend before its
  first control is realized.
- `X-L0-6` `[X]` A documented, safe escape-hatch contract: how application code
  obtains a host object, what it may do with it, what invalidates it, and what
  the framework guarantees afterwards.

---

# L1 — Execution: loop, threads, scheduling, async, time

## F1.1 — Who owns the loop

**Mechanism.** Three arrangements exist: the host owns the loop and the
framework runs inside it; the framework owns the loop and pumps the host; or
the framework runs its own loop on a thread and communicates with the host's.

**Strengths and weaknesses.** Host-owned is the only arrangement that behaves
correctly during host-driven modal operations — resize, drag, menu tracking,
system dialogs, and the lifecycle callbacks mobile hosts impose. Framework-owned
loops are simpler to reason about and are usually adopted by self-drawing
archetypes, which then re-discover, per platform, that the host takes the loop
away during modal interaction. Separate-thread arrangements need every host
call marshalled and generally produce the worst latency.

**Ceiling.** A framework that does not run inside the host's loop cannot behave
natively during host-driven modality, and users perceive exactly that as
"not a real application".

**RustNative's position.** Host-owned, with scheduler wake-ups delivered into
the host loop — already specified per backend in `PLAN.md` section 8.

- `X-L1-1` `[X]` A conformance test per backend: the application continues to
  render, animate, and process scheduled work during host-driven modal
  operations (resize, menu tracking, native dialogs, drag loops).

## F1.2 — Thread affinity

**Mechanism.** Almost every host requires UI mutation on one specific thread.
Frameworks differ in whether that is enforced by types, by assertion, or by
documentation.

**Ceiling.** Where affinity is only documented, violations become intermittent
production crashes that are impossible to reproduce.

- `X-L1-2` `[X]` Thread affinity enforced by the type system wherever the
  substrate allows, and by a debug assertion everywhere else; never by prose
  alone.

## F1.3 — Async execution model

**Mechanism.** Multithreaded work-stealing runtimes; single-threaded
cooperative runtimes; frame-driven loops with no async at all; and
callback/promise chains.

**Strengths and weaknesses.** Work-stealing runtimes maximize throughput and
require every task to be safely movable between threads, which is exactly the
constraint that breaks on single-threaded hosts (browsers, many embedded
targets, per-request sandboxes). Single-threaded cooperative runtimes match
those hosts natively and leave throughput on the table. Frameworks that hard-wire
one runtime discover the other class of host late and expensively.

**RustNative's position.** `PLAN.md` 2.4 records this exact lesson: the
scheduler was found hard-wired to one runtime and widened to an `Executor`
contract. The remaining work — a non-`Send` executor seam for single-threaded
hosts — is already named in the long-range roadmap and is a prerequisite for
web, embedded, and per-request targets.

- `X-L1-3` `[X]` A single-threaded executor profile with non-`Send` tasks,
  proven by the same test suite that covers the multithreaded profile.

## F1.4 — Task lifetime: unstructured versus structured

**Mechanism.** Either background work is started freely and cancelled by
convention, or every task belongs to a scope that cancels it deterministically
when the scope ends.

**Strengths and weaknesses.** Unstructured is simpler to offer and produces an
entire bug class: work that completes after the thing that started it is gone,
writing into freed or stale state. Most archetypes manage this with cleanup
callbacks and dependency rules — that is, with developer discipline.
Structured scopes remove the class.

**Ceiling.** A framework without scope-bound tasks cannot claim
"no state mutation after unmount" as a guarantee; it can only recommend it.

**RustNative's position.** Structured scopes tied to component lifetime
(Milestone 18), which is a genuine differentiator and is currently documented
as an internal detail rather than as a guarantee.

- `X-L1-4` `[X]` State scope-bound cancellation as a public guarantee with a
  conformance test: no task observes or mutates state after its owner unmounts,
  on every target.

## F1.5 — Time and pacing

**Mechanism.** Animation and scheduling need a clock and a frame signal. The
choices are the host's display link / frame clock, a timer, or a busy loop.

**Ceiling.** A framework using timers rather than the host frame signal cannot
be smooth across variable refresh rates, power-saving states, or background
throttling — and on embedded and terminal targets, a process-wide monotonic
clock may not exist at all.

**RustNative's position.** Per-backend frame pacing is specified; a host clock
abstraction is named in the long-range roadmap and still outstanding.

- `X-L1-5` `[X]` A host clock contract replacing direct process-wide time
  access in the core, with a deterministic test clock as the same contract's
  second implementation.

---

# L2 — Realization: what a widget physically is

This is the irreversible layer. Six strategies exist.

## F2.1 — Retained host-native widget tree

**Mechanism.** The framework creates and mutates the host's own UI objects.

**Strengths.** Free fidelity: appearance, animation curves, scroll physics,
text rendering, right-click and long-press conventions, contrast and text-scale
settings, and — most importantly — the host's accessibility and automation
layers work because the objects are real. Host updates improve the application
without a release. Memory and startup are shared with the host's own
implementation.

**Weaknesses.** Per-host work for every control; controls that differ in
capability between hosts; version differences within a host; host controls that
resist deep customization; and a real risk of lowest-common-denominator design
if the framework flattens their differences.

**Ceiling.** Pixel-identical cross-platform output is impossible, and pursuing
it defeats the strategy. Visual differentiation is bounded by what the host
allows.

**RustNative's position.** This is our L2 choice, and `PLAN.md` section 1
explicitly forbids flattening host differences.

- `X-L2-1` `[X]` A per-backend fidelity conformance checklist — appearance,
  system settings, conventions, text, scroll physics, focus visuals — verified
  against the host's own first-party applications, not against our other
  backends.

## F2.2 — Retained self-drawn scene

**Mechanism.** The framework owns a scene graph and renders it with the GPU,
implementing every control itself.

**Strengths.** Identical output everywhere; total control over visual design
and animation; one implementation of every control; no host-control
limitations; consistent behaviour across host versions.

**Weaknesses.** The accessibility tree must be synthesized and bridged per
platform, and it is always behind; system settings must be re-implemented one
by one; text input, IME, selection, and caret behaviour must be rebuilt per
host and are famously the hardest part; artifact size carries a renderer and
usually a font stack; and every host release is a chase.

**Ceiling.** This strategy can approach host behaviour but cannot inherit it.
The gap is permanent and is largest exactly where it is least visible in a demo
and most visible in an audit: assistive technology, input methods, and system
settings.

**Follows for us.** This is our sharpest competitive argument, and it must be
made *empirically* rather than rhetorically.

- `X-L2-2` `[X]` A published comparison methodology: the same reference
  application realized natively versus self-drawn, evaluated with each host's
  own screen reader, input methods, automation tooling, and accessibility
  settings, with results and method both public.

## F2.3 — Immediate mode

**Mechanism.** No retained UI objects. The entire interface is re-emitted every
frame; widget identity is derived from call-site or hashed identifiers, and
interaction state lives in a side table.

**Strengths.** Extremely simple mental model; no synchronization between
application state and UI state, because there is no UI state; trivial to embed
in a render loop; tiny; excellent for tools, debug overlays, and
graphics-adjacent software.

**Weaknesses.** Continuous redraw costs power; accessibility is effectively
absent because there is no persistent object to expose; text input and IME are
minimal; layout is usually manual; scaling to large interfaces is poor.

**Ceiling.** No retained tree means no host accessibility, no host automation,
and no per-element state for assistive technology. This is not fixable within
the strategy.

**Follows for us.** The *ergonomics* — describe the UI every time, let the
framework reconcile — are what developers actually like, and a retained
framework with good reconciliation offers them without the ceiling. Worth
stating plainly, because this archetype's simplicity is genuinely attractive.

- `X-L2-3` `[X]` A developer-facing statement and example of describe-the-whole-UI
  authoring on a retained realization, with identity and reuse shown
  explicitly (this is `PLAN.md` 2.7 and 2.8 made visible rather than assumed).

## F2.4 — Document/markup host

**Mechanism.** The framework emits a semantic document tree that the host
(a browser engine) lays out, styles, and renders.

**Strengths.** The host provides layout, text shaping, bidirectional handling,
accessibility mapping, input methods, printing, zoom, find-in-page, and user
stylesheets — an enormous amount of correctness for free. Semantics are
expressible and inspectable. Progressive enhancement is possible.

**Weaknesses.** Layout semantics are the host's, not the framework's, so a
portable layout model must be mapped onto them rather than executed; the
cascade can be fought by the application; and the engine's cost is fixed.

**Ceiling.** Full control over layout timing and geometry is impossible; the
framework negotiates with the engine rather than commanding it.

**RustNative's position.** This is the Web backend's realization (Web
milestones B and C), and the mapping — not the emitting — is where the
difficulty lives.

- `W-L2-4` `[W]` A documented, tested mapping from the portable layout model to
  the host's own layout mechanisms, with the cases where the mapping is
  approximate named explicitly rather than discovered by users.

## F2.5 — Cell grid

**Mechanism.** The host addresses a grid of character cells with attributes;
everything is drawn into it.

**Strengths.** Universally available, remotely accessible, extremely cheap, and
a host with real conventions of its own.

**Weaknesses.** Geometry is cell-granular; measurement is display width rather
than pixels; capabilities vary per terminal and must be detected; no overlapping
windows; accessibility belongs to the terminal.

**Ceiling.** Sub-cell geometry and host-provided accessibility do not exist.
Honest capability reporting is the only correct response.

**RustNative's position.** Specified in detail (Milestone 38), including the
crucial points: conversion at the backend boundary, display-width measurement,
and terminal restoration as a correctness requirement.

## F2.6 — Framebuffer / display list

**Mechanism.** A draw list is consumed by a display driver; there is no host UI
system at all.

**Strengths.** Works where nothing else does; bounded and predictable cost;
full control.

**Weaknesses.** Everything — text, input, focus, any notion of a control — is
the framework's responsibility; memory and CPU budgets are hard limits;
accessibility services typically do not exist.

**Ceiling.** The capability model must be genuinely honest here or the
framework lies to its users. This is the strongest test of whether capabilities
are real (`PLAN.md` Milestone 37 makes the same argument).

**RustNative's position.** The draw-list path from Milestone 29 is the
substrate for both the terminal and embedded targets — one mechanism, two
hosts, which is the right economy.

---

# L3 — Semantic UI: identity, reconciliation, layout, text, styling

## F3.1 — Identity and reconciliation strategies

**Mechanism, five variants.**

- *Full re-render with tree diffing.* Compare new description to old, emit
  minimal mutations. Cost is proportional to tree size; correctness depends on
  keys.
- *Keyed diffing over lists.* The same, with explicit identity for sequences —
  the difference between reusing a host object and destroying it.
- *Fine-grained reactive binding.* State cells are wired directly to the
  elements that depend on them; no tree comparison happens at all. Cost is
  proportional to the change; the cost moves into subscription bookkeeping and
  into rules about when reads are tracked.
- *Compile-time emitted updates.* A build step analyses the description and
  emits direct mutation code. Minimal runtime; debugging happens in generated
  code.
- *Whole-fragment replacement.* The server returns new markup for a region and
  it replaces the old. Trivial; loses local state and focus unless explicitly
  preserved.

**Ceiling per variant.** Diffing cannot beat the change-proportional variants on
update cost for small changes in large trees; reactive binding cannot avoid
subscription-leak and tracking-rule bugs; compile-time emission cannot avoid
opaque debugging; fragment replacement cannot preserve client state.

**RustNative's position.** Keyed diffing with stable identity and host-object
reuse (Milestones 2 and 4), plus the transient-state rule (2.10) that removes
the highest-frequency updates from the tree path entirely. That combination is
the right answer, but the *proof* is missing: nothing currently makes
over-invalidation a test failure.

- `X-L3-1` `[X]` A documented invalidation contract plus tests that fail when a
  change re-renders more of the tree than the contract allows.
- `X-L3-2` `[X]` Render-cause tracing: for any update, report the state,
  prop, resource, or effect that caused it, with a component path.

## F3.2 — Layout algorithm lineages

**Mechanism, five lineages.**

- *Two-pass measure/arrange.* Parent asks children for a desired size under
  constraints, then places them. Predictable, intrinsic-size-aware, and the
  basis of most native toolkits.
- *Flow and box model with inline formatting.* Document lineage: block and
  inline boxes, margin behaviour, baselines, floats. Extremely capable for
  text-centric content; complex and full of historical rules.
- *One-dimensional and two-dimensional line layout.* Flexible box and grid
  algorithms — now the lingua franca of UI layout and what most developers
  actually think in.
- *Constraint solvers.* Relationships between edges solved globally. Very
  expressive; performance and debuggability degrade with constraint count, and
  over-constrained systems fail in ways that are hard to explain.
- *Fixed/absolute placement.* Coordinates or cells. Trivial; does not survive
  text scaling, translation growth, or variable displays.

**The failure mode all of them share.** Layout that is correct for the
developer's language, text size, and display, and wrong for a translated
string, a user who enlarged text, or a mirrored right-to-left locale. This is
the single most common production defect in cross-platform UI, and it is a
layer-3 property.

**RustNative's position.** A portable measure/constrain model with intrinsic
measurement delegated to the host (Milestones 6–9), which is the correct
lineage for a native-realization framework.

- `X-L3-3` `[X]` Right-to-left mirroring as a layout-model property — start/end
  rather than left/right throughout, with mirroring applied by the core and
  verified per backend.
- `X-L3-4` `[X]` A layout conformance suite run at several text scales, with
  pseudo-localized strings (length growth and accented forms), asserting no
  clipping, no overlap, and no lost interactive targets.
- `X-L3-5` `[X]` Layout explanation in the inspector: for any node, why it has
  the geometry it has — which constraint, which intrinsic measurement, which
  parent decision.

## F3.3 — The text pipeline

**Mechanism.** Between a string and pixels lie: encoding and grapheme cluster
segmentation, font selection and fallback, shaping (ligatures, marks, complex
scripts), bidirectional reordering, line breaking (including dictionary-based
breaking for scripts without spaces), justification, hinting and subpixel
rendering, and caret/selection geometry. A framework either uses the host's
stack or bundles its own.

**Strengths and weaknesses.** Using the host's stack gives correct complex
scripts, correct fallback for every installed font, correct caret behaviour,
and matching rendering — for free, per host, forever. Bundling a shaper gives
identical rendering everywhere and inherits responsibility for every script,
every fallback chain, and every input method interaction.

**Ceiling.** A bundled stack that does not implement a script *cannot render
that language*, and this is where self-drawing archetypes accumulate their
longest-lived defects. A host-stack framework instead inherits per-host
differences in metrics, which is a far cheaper problem.

**RustNative's position.** Host measurement behind `IntrinsicMeasurer` — the
right choice. What is missing is the explicit statement that shaping, bidi,
breaking, and caret geometry are *host* responsibilities on every backend, and
the tests that prove each backend actually delegates them.

- `X-L3-6` `[X]` A text conformance suite per backend: complex scripts,
  bidirectional mixed text, grapheme clusters and emoji sequences, font
  fallback, line breaking without spaces, and caret/selection positions — run
  against the host's own stack.

## F3.4 — Styling and theming models

**Mechanism.** Cascading stylesheets with inheritance and specificity; inline
style objects per element; utility class vocabularies; design-token systems
resolved at build time; or the host's own appearance system.

**Strengths and weaknesses.** Cascade is powerful and famously hard to reason
about at scale. Inline objects are local and explicit and lose reuse. Utility
vocabularies are fast and consistent and hostile to host-native appearance.
Token systems are the current best practice for crossing the design/engineering
boundary — and they are the piece a native-realization framework must map to
host appearance rather than to fixed values.

**Ceiling.** A styling system that expresses absolute values cannot honour host
appearance; a system that expresses only semantic roles cannot express a brand.
Both must exist, with the boundary explicit.

**The split inside the cascading model, which is the part worth taking.** The
cascading stylesheet model is two separable things: a *declaration* vocabulary —
property names, value and unit syntax, arithmetic and colour functions, named
custom properties — and a *resolution* mechanism, the selector matching,
specificity, and inheritance that decide which declarations reach which element.
The first is the most widely known styling vocabulary there is and costs a
parser. The second is an engine, and for a framework that already resolves style
deterministically per node it is a *second* engine competing with the first, on
every host, forever — the same trade a self-drawing renderer makes at L2 and the
same one a framework makes when it delegates layout to a host that also lays
out.

The utility-vocabulary archetype is interesting precisely because it already
took that split. A utility class is a fixed set of declarations attached to the
element that names it: no descendant rules, no specificity contests, variants
that are element state rather than tree position. It is therefore portable to a
non-browser host in a way a stylesheet is not — the ecosystem around
`mobile.md` M3 compiles a utility vocabulary straight down to that archetype's
per-element style objects, with no cascade anywhere in the result. The opposite
experiment is on record too: `desktop.md` D5 accepts a stylesheet dialect over
its widgets, and applying one moves those widgets onto the dialect's own
painting path — affordable for an archetype that draws its controls anyway, and
exactly the trade a host-native framework exists to refuse.

**Ceiling of the utility half.** Two, both real. Unknown classes are silently
dropped in the archetype's own tooling, so a typo is a styling bug found by
eye; and a vocabulary that can express what the host cannot realize either
lies, or forces the framework to draw its own controls to keep the promise. A
per-property capability answer is the only honest resolution, and it must be a
build-time answer, because a style that silently disappears at run time is
indistinguishable from a layout bug.

**The ceiling on the other side.** The obvious response to a host that cannot
realize a property is to leave the property out of the vocabulary, so that
every host looks the same. That is the lowest-common-denominator trade at the
styling layer: the narrowest host sets the range for all of them, the richer
hosts are styled below what they offer, and the vocabulary a developer arrives
with fails on its most ordinary classes. The capability answer resolves this
too, provided it is used in both directions — every host answers every
property, and an application states where a difference between its targets is
acceptable. The archetype's own conditional-support variant is the right
spelling for that statement, if what it asks is the framework's capability table
rather than a browser's feature query. One condition makes the arrangement
testable: a reference backend that realizes the whole range, so the equivalence
suite can reach every property whichever hosts realize it.

**The ceiling of the relational half.** Some of what selectors are used for is
not about the cascade at all — position among siblings, a parent styling its
children, a hovered container restyling its contents, a component responding to
its container's size. Each is a relationship between named nodes, and each can
be resolved by a bounded lookup in a framework that owns the tree, without a
matcher. Positional relationships need only the child lists a reconciler
already holds. State- and size-dependent ones need a dependency index that
keeps invalidation bounded, which is a permanent cost, and in a component model
with typed state they mostly repeat what a prop already expresses. An arbitrary
selector, finally, is a search over the tree and cannot be made a lookup; the
only honest place for it is an explicit escape hatch on the one host that
speaks it.

**RustNative's position.** A theme system exists (Milestone 21) and resolves
per node, which is the right substrate; a token pipeline, a declaration
vocabulary, a utility spelling, a documented semantic-versus-absolute boundary,
and per-backend style capability answers do not exist.

- `X-L3-7` `[X]` A design-token pipeline into the theme system, with a
  documented token schema and an explicit split between semantic roles (mapped
  to host appearance) and absolute brand values (applied as-is).
- `X-L3-12` `[X]` Vocabulary equality between style spellings: every utility
  class resolves to typed style properties reachable from both authoring
  syntaxes, every property has a utility spelling or a documented note that it
  has none, proven by an equivalence suite; an unresolvable class is a compile
  error naming what was expected, never a silently dropped class.
- `X-L3-13` `[X]` A declaration vocabulary without a cascade, stated as a rule
  rather than as an omission: values, units, arithmetic and colour functions,
  and named token references, resolved at build time, with no selectors, no
  specificity, and no inheritance beyond the text properties that inherit
  everywhere.
- `X-L3-14` `[X]` Token-valued declarations resolved at style-resolution time
  rather than folded at build time, so a theme, colour-scheme, or palette change
  re-resolves and re-applies to existing host objects without a rebuild and
  without a tree pass.
- `X-L3-15` `[X]` A per-backend style capability table: every property declared
  realized, approximated (with the approximation documented), or unavailable,
  asserted by conformance rather than described, with an application's use of an
  unavailable property failing the build.
- `X-L3-16` `[X]` A documented unit mapping per host — the host's own unit, how
  a root-relative unit follows the host's text setting, and the rounding rule —
  with a conformance case per backend, including the cell-quantized one where
  the mapping is coarsest.
- `X-L3-17` `[X]` A style range not cut to the common set: a property enters
  the vocabulary when a host can realize it, every backend answers it —
  separately for a native control and for a box the framework owns — and the
  reference backend realizes all of it, so no property is untestable.
- `X-L3-18` `[X]` Explicit enhancement: capability and target conditions in
  both spellings, decided against the realizing backend's capability table at
  resolution rather than by a feature query; the application's declared targets
  checked at build time, so an unguarded property a declared target cannot
  realize fails the build; approximations reported and refusable.
- `X-L3-19` `[X]` Style motion lowered onto the framework's own timelines, so a
  class-spelled transition is interrupted, cancelled, and skipped under reduced
  motion by the same rules as a typed one, on every host that runs frames.
- `X-L3-20` `[X]` Structural conditions — position among siblings, children
  styled by their parent — resolved from the reconciler's child lists, counted
  over logical items in virtualized collections, and invalidated by a child
  list change alone.
- `X-L3-21` `[X]` State- and size-dependent relationships — ancestor groups,
  earlier siblings, children's state, container size — as typed relationships
  resolved by bounded lookup through a dependency index, never by a matcher,
  with invalidation held by the over-invalidation suite. Optional: the
  component model already expresses each through props or the environment.
- `X-L3-22` `[X]` Host style passthrough confined to one host that speaks a
  styling language of its own, carried uninterpreted, outside the equivalence
  suite, and reported as unverified wherever it appears. Optional: the escape
  hatch for style, not a styling channel.

## F3.5 — The authoring surface

**Mechanism, six variants.** Every framework has to decide what a developer
physically types to describe a tree.

- *Builder or fluent API in the host language.* Constructors and chained
  modifiers. Nothing new to learn, the full language available, and every tool
  the language already has — formatter, completion, go-to-definition, error
  messages — works on day one, because none of it knows anything special is
  happening.
- *A markup-extended source dialect.* Whole source files in a superset of the
  host language in which an element is one more kind of expression, written
  wherever an expression may go, with no wrapper. The text's shape matches the
  tree's shape, and this is the form the largest population of UI developers
  alive already writes every day. The host compiler does not accept the
  dialect, so a compile step lowers each file first — and everything the host
  toolchain gave for free must be given back through that step: a source map,
  diagnostics rewritten onto the dialect file, a language-server layer that maps
  positions both ways, and a formatter that understands both halves of the file.
- *Markup inside a delimited host-language construct.* The same element
  grammar inside a macro or equivalent, in ordinary source files. No compile
  step and no source map: the construct's tokens keep their real positions, so
  errors land where they were written under the plain toolchain. It costs a
  delimiter around every markup region, and it opts out of the host language's
  formatting and much of its editor assistance inside the delimiters.
- *A separate template file compiled against the code.* Strong separation and
  designer-editable artifacts, at the price of a second language with its own
  scoping and its own type story, and a boundary where type information is
  easiest to lose.
- *A brace-based declarative DSL.* Nesting expressed with the host language's
  own block syntax. Much cheaper to implement than elements; loses the
  attribute/child distinction and tends to accumulate ad-hoc conventions in
  place of a grammar.
- *Generated code from a visual designer.* Fastest start of any variant; the
  generated artifact becomes the real source of truth, merges badly, and
  constrains everything above it.

**Strengths and weaknesses.** The builder variant's strength is that it is free
and complete by construction: there is no surface that can lag, because there
is no surface. Its weakness is that deeply nested structure stops looking like
structure. The two markup carriers share the opposite strength and divide the
cost differently. The dialect is the more natural to write and read, and the
more familiar, and it pays in tooling: its quality is exactly the quality of
its source mapping. The delimited construct is cheaper and exact under the
plain toolchain, and it pays in ergonomics: a wrapper around every region, and
a formatter and editor that stop at the delimiter unless someone extends them.

**The failure modes, and there are two.** The first is shared by every
framework that offers *two* surfaces: they rarely stay equal. The second
surface ships a release later, the documentation settles on whichever one the
maintainers prefer, examples stop being written in both, a feature lands in one
and is "coming soon" in the other, and the lagging surface ends up with worse
errors, no formatter, and a smaller API. This is worse than offering one,
because the lagging surface is advertised as a choice and is actually a trap —
a developer discovers the inequality after the codebase is written in it.
Multiple archetypes across three of the platform families in this analysis
demonstrate exactly this decay.

The second belongs to the dialect alone: a compile step without complete
source mapping. Errors that point into generated code, an editor that cannot
resolve a name inside an element, a formatter that mangles one half of the
file — each makes the most pleasant surface to write the least pleasant one to
debug, and each is the default outcome, because the host toolchain will never
know the dialect exists.

**Ceiling per variant.** A dialect cannot be better than its source map and its
language-server layer, because every host tool sees only the lowered file; a
delimited markup construct cannot exceed the diagnostics and formatting its own
implementers build, because it declined the language's; a builder surface
cannot make nesting visible; a template file cannot fully recover the type
information it crosses a boundary to lose; a designer-generated surface cannot
survive being hand-edited. And no framework can keep two surfaces — or two
carriers of one surface — equal by intention, only by a test that fails when
they are not.

**RustNative's position.** `PLAN.md` 2.9 commits to the builder variant and to
markup as peers, and carries markup both ways: as a dialect (`.rsx` files) for
code that is mostly UI, and as a delimited construct (`rsx!`) for markup inside
ordinary source, for crates without a build step, and for runnable API
documentation. Equality is defined structurally rather than aspirationally, at
both boundaries:

- between the syntaxes, markup is a compile-time front end that emits builder
  calls and nothing else, so it cannot carry a capability the builder form
  lacks, and an equivalence suite asserts that both spellings of every node
  kind and modifier produce equal values;
- between the carriers, the dialect's compile step does nothing but wrap each
  markup expression in the macro and leave every other byte in place, so there
  is one parser, one lowering, and one set of diagnostics, and a grammar rule —
  no bare text between tags — keeps anything from being expressible in one
  carrier and not the other.

That design answers the first failure mode. The second is answered only by
building the source-map tooling at the same depth as the syntax, which is why
Milestone 53 treats diagnostics remapping, the language-server proxy, and the
formatter as part of the syntax rather than as follow-up work. None of it is
built yet.

- `X-L3-8` `[X]` Capability equality between authoring surfaces, proven by an
  equivalence suite covering every node kind and every modifier through every
  carrier, run in CI, and failing when a new constructor or modifier lands in
  only one of them.
- `X-L3-9` `[X]` Diagnostics from markup held to the host compiler's quality —
  spans on the offending attribute or element rather than on a macro
  invocation or a lowered file, unknown attributes naming what was expected,
  type errors reported against the attribute's own span — proven by a
  compile-failure suite run through every carrier rather than by inspection.
- `X-L3-10` `[X]` Tooling parity for markup: formatting of markup regions,
  completion, hover and go-to-definition that resolve an attribute to the
  method it calls, and a command that prints the lowered builder form. A
  surface with worse tooling is not an equal surface regardless of what its
  capability table says.
- `X-L3-11` `[X]` A markup-extended source dialect whose compile step is
  invisible in use: every diagnostic — from markup and from ordinary host code
  alike — reported at the dialect file's own position, a language-server layer
  mapping positions in both directions, a whole-file formatter, and source-map
  round trips under test. Where the plain host toolchain cannot be made to
  report dialect positions, the limitation is documented rather than
  discovered.

---

# L4 — Application model: state, effects, lifetime

## F4.1 — State and reactivity strategies

**Mechanism, five variants.**

- *Mutation with observers.* Objects notify listeners. Simple, and produces
  unbounded update graphs and subscription leaks.
- *Unidirectional message loop.* All change flows through messages into a
  single update function. Exceptionally predictable and testable, at the cost
  of verbosity and central coupling.
- *Local component state with explicit setters.* The mainstream compromise:
  local by default, lifted when shared, with re-render triggered by the setter.
- *Reactive signal graphs.* Values with automatic dependency tracking; precise
  updates, with tracking rules that leak into the mental model.
- *Immutable snapshots with structural sharing.* Whole-state versions; easy
  time travel and undo; allocation-heavy.

**Ceiling.** Observer mutation cannot give predictable update ordering; message
loops cannot avoid boilerplate; signal graphs cannot avoid tracking rules.

**RustNative's position.** Local component state plus props and message
callbacks, with ownership preventing the shared-mutable-state failures the
first variant has. What is missing is the *explicit* shared-state story: how
two distant components observe the same data without lifting it to the root.

- `X-L4-1` `[X]` A first-class shared-state contract (scoped, typed, observable
  without global mutable state), with defined update ordering and the same
  lifetime discipline as component state.
- `X-L4-2` `[X]` Undo/redo and time-travel support built on a documented state
  history contract, because it is nearly free to offer here and is a recurring
  application requirement.

## F4.2 — Effects and their lifetime

**Mechanism.** Effects run on mount, on change, and on unmount. Variants
differ in how dependencies are declared (explicit lists, automatic tracking, or
scope-based), and whether cleanup is guaranteed.

**Ceiling.** Where cleanup is by convention, leaks and post-unmount writes are
permanent bug classes. Where dependencies are declared by hand, stale-capture
bugs are permanent.

**RustNative's position.** Effects with reactive invalidation (Milestone 19)
over structured scopes (Milestone 18) — a stronger position than most
archetypes. It needs to be *provable*, per `X-L1-4`.

## F4.3 — Error containment

**Mechanism.** Either a failure anywhere terminates the application, or
subtrees have boundaries that contain failure and present a fallback.

**Ceiling.** Without boundaries, a framework cannot honestly claim resilience,
and per-target panic behaviour becomes an accident. On embedded and terminal
targets, an unhandled failure can leave hardware or a terminal in an unusable
state (`PLAN.md` Milestone 38 already treats terminal restoration this way).

**RustNative's position.** Not specified anywhere — a genuine gap.

- `X-L4-3` `[X]` An error-boundary model: a subtree may fail, be contained,
  present a fallback, and be retried, with the failure reported through a
  diagnostic channel.
- `X-L4-4` `[X]` A per-target panic and teardown policy, including host and
  hardware restoration, verified by a test that panics deliberately.

---

# L5 — Host integration: accessibility, input, internationalization

## F5.1 — Accessibility architecture

**Mechanism.** Three arrangements: host objects carry accessibility natively; a
synthesized tree is bridged per platform; or none exists.

**Ceiling.** Synthesized trees lag the host's semantics permanently, and
"virtual" elements are where they lag most. Frameworks with no retained tree
cannot participate at all.

**RustNative's position.** A portable accessibility model bridged per backend
(Milestone 26) on top of real host objects — the strongest available
arrangement. The gap is verification: it must be exercised with each host's own
assistive technology and asserted in CI where possible.

- `X-L5-1` `[X]` Automated accessibility assertions in CI per backend (roles,
  names, states, focus order, live regions), plus a recorded manual pass with
  each host's own screen reader before a backend is called complete.

## F5.2 — Input, gestures, and text entry

**Mechanism.** Hit testing and routing (capture/bubble), pointer unification
across mouse, touch, pen, and trackpad, gesture recognition and
disambiguation against host-provided recognizers, keyboard handling with
modifiers and shortcuts, and input methods for composed text.

**Ceiling.** A framework that re-implements gesture recognition rather than
cooperating with the host's produces conflicts users perceive as bugs — most
visibly in scrolling, text selection, and system edge gestures. A framework
without real input-method integration cannot be used in the languages that need
it, which is most of the world's users.

**RustNative's position.** Advanced input (Milestone 25) and per-backend IME
specified; the cooperation rule is stated for one mobile host and should be
universal.

- `X-L5-2` `[X]` A documented gesture arbitration contract with host
  recognizers, per backend, with conflict cases enumerated and tested.

## F5.3 — Internationalization and localization

**Mechanism.** Message catalogues with plural and gender categories, locale-aware
number, date, currency, and unit formatting, collation, bidirectional text,
locale-aware casing, and a translator workflow with context and screenshots.

**Ceiling.** String concatenation and format placeholders cannot express plural
or grammatical-gender rules; applications built that way are re-engineered
rather than translated.

**RustNative's position.** **Absent from the plan entirely.** This is the
largest unnoticed gap in the current documentation: every archetype at every
level of maturity has an answer, and a framework without one is not viable for
commercial software.

- `X-L5-3` `[X]` A localization system: typed message catalogues with plural
  and gender categories, compile-time-checked placeholders, locale-aware
  formatting delegated to host facilities where they exist, runtime locale
  switching, and an extraction/merge workflow for translators.
- `X-L5-4` `[X]` Pseudo-localization built into the dev loop, wired into the
  layout conformance suite (`X-L3-4`).

---

# L7 — The engineering loop, as a root problem

Placed here rather than later because, for our substrate (F0.4), the loop is
not a convenience layer — it is the structural disadvantage, and it must be
engineered at the same depth as the runtime.

## F7.1 — Iteration speed

**Mechanism.** Dynamic substrates replace code live; compiled substrates must
either restart, reload a dynamic library, or re-run a description. The
achievable target for a compiled framework is: rebuild only the changed crate,
reload the application, and restore application state from a serialized
snapshot so the developer stays where they were.

**Ceiling.** True per-function live replacement is not available to us. Fast
rebuild plus state-preserving restart is, and it is close enough in practice if
the numbers are good.

- `X-L7-1` `[X]` A development loop that rebuilds and restarts with application
  state preserved, targeting a declared and enforced wall-clock budget; working
  on-device for mobile and embedded targets, not only on the development
  machine.
- `X-L7-2` `[X]` Optional dynamic-library reload for the application crate
  where the platform supports it, behind the same command.

## F7.2 — Inspection

**Mechanism.** A live view of the declarative tree, the realized host objects,
and the mapping between them; editable state and props; layout overlays with
explanations; event and render tracing; task and scope inspection; host-object
leak detection.

**Ceiling.** Dynamic substrates get much of this from reflection; we must
expose it deliberately from the runtime. Not doing so leaves our worst pillar
permanently worst.

- `X-L7-3` `[X]` An inspection protocol exposed by the runtime — tree, state,
  layout, events, tasks, host objects — consumable by an external tool over a
  transport that works on-device and remotely.
- `X-L7-4` `[X]` An inspector client built on that protocol, shipped with the
  CLI, working against every backend including terminal and embedded targets.

## F7.3 — Testing

**Mechanism.** Logic tests; component tests driving the real tree with a
headless backend; interaction tests dispatching synthetic input; golden tests
over realized output; end-to-end tests against a real host; and deterministic
control of time and asynchrony.

**Ceiling.** Without a headless backend, component testing requires hardware,
which caps test coverage at whatever CI can host — the exact problem `PLAN.md`
2.13 identifies for platforms we cannot verify on.

- `X-L7-5` `[X]` A headless reference backend that realizes the tree into an
  inspectable model, so component, interaction, and golden tests run anywhere.
- `X-L7-6` `[X]` A deterministic test clock and executor (the second
  implementation of `X-L1-5` and `X-L1-3`) so async behaviour is reproducible.
- `X-L7-7` `[X]` Synthetic input dispatch through the real input path, not a
  test-only shortcut.

---

# What the foundations imply

Reading the ceilings together produces five conclusions that the platform
documents then apply target by target.

1. **Our root choices (F0.4, F1.4, F2.1, F3.3) are correct and defensible, and
   three of them are *permanently* unavailable to most competitors.** They
   should be stated as guarantees with conformance tests behind them, because
   an unproven guarantee is marketing and a proven one is a moat.

2. **Our root choices also hand us one structural disadvantage — the
   engineering loop (F7) — and no competitor has to fix it for us.** It is the
   only layer where effort changes the ranking, so it deserves milestone-level
   investment rather than a "tooling" bullet under future work.

3. **Two whole layers are missing from the plan rather than incomplete in it:**
   localization (F5.3) and error containment (F4.3), with shared state (F4.1)
   and the data layer close behind. These are not polish; they are the reason
   commercial applications choose a framework.

4. **Everything above L2 must be verified per host, not per framework.** The
   fidelity argument (F2.1) is only worth making if it is measured against the
   host's own applications and its own assistive technology — which is a
   testing commitment, not a design one.

5. **Offering two authoring surfaces (F3.5) is an adoption advantage that
   decays into a liability unless equality is enforced by a test.** The
   familiar surface is how developers from the largest UI population arrive at
   all; the builder surface is what the host language gives for free and what
   programmatic composition needs. Keeping both is worth doing and cheap to get
   wrong, and every archetype that got it wrong got it wrong the same way — by
   documenting one and letting the other lag. Carrying the familiar surface as
   a source dialect adds a second obligation of the same kind: the compile step
   must be invisible in use, which is a tooling commitment made at the same
   time as the syntax, not after it.
