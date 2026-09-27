# Concepts I — execution, rendering, state, and composition (L1–L4)

The archetype documents analyse *who* made which root choices. This catalogue
analyses *what they invented along the way*: the ideas that started in one
framework family and became an expectation everywhere. An archetype can be
abandoned by its users while a concept it introduced becomes table stakes, so
concepts are analysed independently of the archetypes that carry them.

The catalogue is ordered bottom-up like everything else in this directory, and
split into four documents:

| Document | Layers | Concepts |
| --- | --- | --- |
| **this one** | L1–L4 — scheduling, rendering model, state, composition | C01–C21 |
| [`concepts-app.md`](concepts-app.md) | L3, L5, L6 — UI system, data and sync, server and API, distributed execution, platform surfaces | C22–C54 |
| [`concepts-delivery.md`](concepts-delivery.md) | L7–L9 — engineering loop, build and packaging, isolation and security, ecosystem | C55–C72 |
| [`concepts-embedded.md`](concepts-embedded.md) | L0–L8 on constrained and device targets — hardware description, scheduling, memory, storage, fleets, robotics, edge inference | C73–C92 |
| [`concepts-toolchain.md`](concepts-toolchain.md) | L7–L9 — the layer between a repository and an installed, updatable application: release train, diagnostics, profiles and credentials, submission, update channels and fingerprinting, pipelines, native-module authoring, universal routing, launch assets, playground, pairing, escape ladder, artifact attribution | C93–C105 |

Every concept gets the same treatment:

- **Introduced by** — the archetypes (see the platform documents) that
  originated or popularized it. Named by archetype identifier only; the naming
  rule in [`README.md`](README.md) applies here as everywhere.
- **Mechanism** — what it actually does, in terms precise enough to implement.
- **Strengths / Weaknesses** — of the concept itself, independently of any one
  implementation.
- **Opportunities / Threats** — for RustNative specifically: what our root
  choices let us do better, and what happens if we ignore it.
- **Position** — scored as in [`method-and-stack.md`](method-and-stack.md):
  Met, Partial, Planned, or Absent, with evidence.
- **Requirements** — falsifiable, identified `Cnn-k`, tagged by target, and
  scheduled in [`gap-plan.md`](gap-plan.md).

A concept is included when at least one of these is true: users of mature
frameworks now expect it; it removes a bug class; or it is a mechanism our root
choices let us do strictly better than the archetype that introduced it. A
concept is *rejected* explicitly — with the reason written down — when it
conflicts with `PLAN.md` section 2. Rejection is also a decision a future
session must be able to find.

---

# Part A — Scheduling and the rendering model

## C01 — Interruptible rendering and priority lanes

**Introduced by.** W1 (its later generations), M1.

**Mechanism.** Not all updates are equal. An update caused by typing must
reach the screen within one frame; an update caused by filtering ten thousand
rows in response to that typing may take several. The scheduler assigns every
update a priority (a *lane*); rendering work is split into units that can be
paused between frames, abandoned if a higher-priority update invalidates it,
and resumed or restarted later. Developers mark work as non-urgent (a
*transition*), or ask for a *deferred value* — the previous value keeps being
shown until the new one is ready — and the framework keeps the urgent path
responsive while the slow path catches up. Pending transitions are observable,
so the UI can show that fresher content is on its way without blanking what is
already there.

**Strengths.** Input latency decoupled from render cost. Removes an entire
class of hand-written debouncing. Makes "keep showing the old content while the
new content loads" a framework behaviour rather than per-screen code. A clean,
explainable model for *why* the UI stayed responsive.

**Weaknesses.** Rendering must be free of side effects, because it can run and
be discarded — so the model is only as safe as the discipline that keeps
effects out of render. Tearing: two parts of the screen can briefly reflect
different versions of external state unless the framework provides a consistent
read. Debugging "why did this render twice" becomes harder. Retrofitted onto a
framework that did not start with it, the migration was long and painful.

**Opportunities.** RustNative's render is already a pure function from state to
tree (`PLAN.md` 2.8), effects are already separated (Milestone 19), and tasks
are already structured (Milestone 18) — which are precisely the preconditions
this concept needed and the archetype had to retrofit. Rust can additionally
*enforce* purity at the boundary: render receives shared references to state,
so a render that mutates cannot compile. Priority can be attached to the
message that caused the update, which fits the existing message model rather
than adding a second API.

**Threats.** Without it, the first large data-heavy application built on the
framework will stutter on input, and "native framework feels slower than a web
page" is a verdict that sticks.

**Position.** **Absent.** The scheduler (Milestone 17) orders tasks; it has no
notion of update priority, and a tree pass is not interruptible.

**Requirements.**

- `C01-1` `[X]` Update priorities attached to the message or state change that
  causes them, with at least three classes: immediate (input feedback), normal,
  and deferrable.
- `C01-2` `[X]` Interruptible reconciliation for deferrable updates — work
  split at component boundaries, yielding to the host between frames, and
  discarded when superseded — with a consistency guarantee that one frame never
  mixes two versions of the same state.
- `C01-3` `[X]` A deferred-value and pending-transition primitive, so a
  component can keep showing previous content while new content is prepared and
  can render a "stale" indication.
- `C01-4` `[X]` Render purity enforced by type, not by convention: render
  receives only shared access to state, and the documentation states why.

## C02 — Visibility- and lifecycle-aware work

**Introduced by.** M1, M2, W3 (offscreen deprioritization), W14's data caches.

**Mechanism.** Work that nobody can see should not run at full priority, and
work for a backgrounded application should often not run at all. Frameworks
tie the collection of data streams to a lifecycle *state* (started, resumed)
rather than to creation and destruction, so collection pauses when the screen
is hidden and resumes when it returns. Offscreen subtrees (hidden tabs, items
scrolled away, a backgrounded window) are rendered at the lowest priority or
kept in a frozen state, and refreshing data resumes on return.

**Strengths.** Battery life, data use, and CPU headroom, for free. Stops the
common bug where a hidden screen keeps polling. Preserves state of hidden tabs
without paying for their updates.

**Weaknesses.** Two lifetimes to reason about — existence and activity — and
bugs where the developer used the wrong one. Frozen subtrees need a clear rule
for what happens to their pending work.

**Opportunities.** RustNative has scope-bound tasks tied to *existence*
(Milestone 18) and a lifecycle model (Milestones 13, 24, 30); adding an
*activity* dimension to scopes — a scope that can be suspended and resumed —
is a natural extension, and makes the pause behaviour a property of the scope
rather than something each effect implements.

**Threats.** Mobile hosts actively penalize applications that work in the
background, and review and energy reports surface it.

**Position.** **Absent.** Scopes are cancelled at unmount; there is no
suspended state.

**Requirements.**

- `C02-1` `[X]` Suspendable task scopes: a scope may be suspended and resumed by
  the framework according to visibility and host lifecycle, with the rule for
  in-flight work documented (complete, cancel, or defer) per task kind.
- `C02-2` `[X]` Offscreen subtrees — hidden tabs, collapsed panels,
  backgrounded windows — retained with state and rendered at the lowest
  priority, per `C01`.
- `C02-3` `[M]` `[D]` Data revalidation on return to foreground, as a declared
  policy of the data layer (`C30`).

## C03 — Change-detection strategies

**Introduced by.** Several web archetypes over successive generations; D1, D3,
D5, M1.

**Mechanism.** Every framework must answer *how does it know something
changed?* Five answers exist and they differ at the root:

- *dirty checking* — compare every watched value against its previous value on
  a trigger;
- *asynchronous interception* — patch every asynchronous entry point so any
  callback completion triggers a check;
- *explicit notification* — objects raise change events (observable
  properties);
- *setter-triggered re-render* — calling a state setter schedules the owning
  component;
- *read tracking* — reading a value during render subscribes the reader
  (signals, snapshot observation).

**Strengths.** Each is a real point on a trade-off curve: interception is
invisible to the developer; notification is explicit and efficient; setters are
simple and local; read tracking is precise.

**Weaknesses.** Interception checks far too much and makes performance
unpredictable (the archetype that relied on it spent years migrating away);
dirty checking scales with watch count; notification leaks subscriptions;
read tracking leaks tracking rules into the mental model.

**Opportunities.** RustNative uses setter-triggered invalidation of the owning
component plus reactive invalidation for effects and resources (Milestones 3,
19). That is the right root, and ownership makes it better than elsewhere: the
set of things that can change a component's state is statically known, so
over-invalidation can be *tested* (`X-L3-1`). The concept to extract is
documentation and naming: developers arriving from each of the other four
strategies need to be told, explicitly, which one this is and what it implies.

**Threats.** Developers bring the expectations of the strategy they know —
most commonly that mutating a value somewhere "just updates" the screen — and
file the difference as a bug.

**Position.** **Partial** — implemented, not stated as a contract.

**Requirements.**

- `C03-1` `[X]` A published change-detection contract naming the strategy, what
  triggers invalidation, what does not, and a comparison table against the
  other four strategies for developers arriving from them.

## C04 — Positional memoization and compiler-inferred skipping

**Introduced by.** M1, W2.

**Mechanism.** The framework remembers values and child results by their
*position in the call structure* of the render function (a slot table), so a
re-render can skip any call whose inputs are unchanged. A compiler plugin
infers which types are stable (immutable, or observable when they change), and
marks calls with only stable arguments as skippable, without the developer
writing memoization.

**Strengths.** Memoization without ceremony; skipping is the default rather
than an optimization the developer must remember; large trees re-render in time
proportional to what changed.

**Weaknesses.** Stability inference is invisible and brittle — one unstable
type in a parameter list silently disables skipping for a whole subtree, and
teams end up reading compiler reports to find out why. Position-based identity
confuses developers when control flow changes the call structure.

**Opportunities.** RustNative uses explicit keys for identity (Milestone 2,
`PLAN.md` 2.7) rather than call position, which avoids the second weakness.
For the first, Rust's type system can express stability honestly —
`PartialEq` and immutability are visible in the type — so skipping can be
decided from the type rather than inferred by a plugin, and a component whose
props are not comparable can be reported at compile time instead of silently
never skipping.

**Threats.** Without skipping, large component trees re-render wholesale, and
the invalidation contract (`X-L3-1`) is met only by pushing memoization work
onto developers.

**Position.** **Partial** — component props exist (Milestone 15); skipping on
equal props is not specified.

**Requirements.**

- `C04-1` `[X]` Automatic skipping of a component whose props are equal to the
  previous render's, decided from the props type, with a compile-time or
  debug-time report when a component's props cannot participate.
- `C04-2` `[X]` The inspector (Milestone 44) shows, per component, whether it
  rendered or was skipped in the last update and why.

## C05 — Server components

**Introduced by.** W3.

**Mechanism.** Some components run *only* on the server. They may access the
database, the filesystem, and secrets directly; they never ship code to the
client; and their output is serialized as a tree payload — not HTML, but the
framework's own tree format — that the client merges into its tree. Client
components nested inside receive serialized props. The boundary is declared per
module.

**Strengths.** Zero client code for everything that is not interactive. Data
access colocated with the component that needs it, with no API layer in
between. Streaming falls out naturally, because the payload can be sent in
order of readiness. Secrets cannot leak to the client by construction.

**Weaknesses.** Two kinds of component with different rules, and the boundary
between them is a common source of confusion and errors. Props crossing the
boundary must be serializable, which is a runtime check in the archetype's
substrate. Caching behaviour interacts with everything and is hard to predict.
Tooling (inspectors, error reporting) must understand both sides.

**Opportunities.** Several advantages are available to us and not to the
originator. The boundary can be enforced by the type system: a server-only
component's props must implement a serialization trait, and a server-only
dependency cannot be named in client code because it is not compiled into the
client target. The tree payload format already exists in effect — our `Node`
is the tree — so the payload is the reconciler's own input, not a third
representation. And because the client uses the same reconciler, merging a
server subtree is ordinary reconciliation.

**Threats.** This concept is now how the dominant archetype expects
server-rendered applications to be written; a server mode without it will be
judged as the previous generation.

**Position.** **Absent.** Web milestone H renders whole trees on the server;
it has no notion of components that exist only there.

**Requirements.**

- `C05-1` `[W]` Server-only components whose code is excluded from the client
  build, whose props across the boundary are checked for serializability at
  compile time, and whose output is sent as tree payload merged by the ordinary
  reconciler.
- `C05-2` `[W]` A compile-time error, not a runtime one, when server-only
  dependencies are reachable from client code.

## C06 — Partial prerendering

**Introduced by.** W3.

**Mechanism.** A route's static shell — everything that does not depend on the
request — is prerendered at build time and served instantly from a cache or
edge; the dynamic parts are marked as holes and streamed into the shell in the
same response.

**Strengths.** Static-site first-byte performance for routes that are mostly,
but not entirely, static — which is most routes. No choice between static and
dynamic at route granularity.

**Weaknesses.** Requires pending-representation boundaries everywhere dynamic
data is read; mistakes silently make whole routes dynamic. Caching semantics
become harder to explain.

**Opportunities.** Pending-representation boundaries are already planned for
streaming (`W-MF-3`); the static/dynamic split can be *inferred* rather than
declared because the tree records which subtrees read request-scoped state
(per-request state is already a Web milestone H requirement).

**Threats.** Low on its own — but it is the current expectation for
content-heavy applications and will be asked for.

**Position.** **Absent.**

**Requirements.**

- `C06-1` `[W]` A route may be served as a prerendered shell with streamed
  dynamic holes, with the split determined by which subtrees read
  request-scoped state, and the inspector able to show which parts of a route
  are static.

## C07 — Per-component render modes

**Introduced by.** W3, and the server-side-interactive archetype (W16).

**Mechanism.** Each component chooses where it runs and how it becomes
interactive: static (server-rendered, never interactive), server-interactive
(state on the server, events and diffs over a persistent connection),
client-interactive (code shipped to the browser), or automatic (start
server-interactive for instant interactivity, switch to client-interactive once
the client module has downloaded).

**Strengths.** The right cost per component instead of per application;
instant interactivity without a large client payload; progressive
transition from server to client.

**Weaknesses.** State does not survive a mode switch unless explicitly
persisted; mode interactions are subtle; developers must know which mode a
component is in to know what it can access.

**Opportunities.** The three deployment modes of the Web track are currently
*application-wide*. Mode-per-subtree is a generalization of the same mechanism,
and the one-tree-one-reconciler design makes the switch a transfer of state
between two instances of the same reconciler rather than a change of
framework.

**Threats.** Medium: without it, applications choose between large client
payloads and a server round-trip on every interaction.

**Position.** **Absent.** Modes are chosen per build (Web milestone J).

**Requirements.**

- `C07-1` `[W]` Render mode selectable per subtree — static, server-interactive
  (see `C33`), client-interactive, automatic — with state transfer on mode
  switch specified and tested.

---

# Part B — State

## C08 — Derived state and memoized selectors

**Introduced by.** W14's state containers, W2, D5, M1.

**Mechanism.** Values computed from state are declared as derivations rather
than stored: a derivation caches its result and recomputes only when its inputs
change. Selectors read a slice of a large store so a component re-renders only
when its slice changes. Stores are *normalized* — entities stored once by
identity and referenced elsewhere — so an update touches one place.

**Strengths.** A single source of truth with no duplicated, drifting copies;
cheap recomputation; minimal re-renders from large stores.

**Weaknesses.** Memoized selectors with hand-written equality are a bug source;
normalization adds ceremony for small applications.

**Opportunities.** Rust can express derivations as pure functions over borrowed
state with equality from the type, and the invalidation contract can include
them, so "derived value recomputed although its inputs were equal" is a test
failure rather than a performance mystery.

**Threats.** Without derivations, applications store computed values and they
drift — a correctness bug, not just a performance one.

**Position.** **Absent** as a primitive.

**Requirements.**

- `C08-1` `[X]` A derived-value primitive: a pure function of state, cached,
  recomputed only when its inputs change by value, participating in the
  invalidation contract and visible in the inspector.
- `C08-2` `[X]` Slice subscription for shared state (`X-L4-1`), so a component
  observing part of a shared store is invalidated only by changes to that part.

## C09 — Transactional snapshot state

**Introduced by.** M1.

**Mechanism.** State lives in versioned snapshots (multi-version concurrency
control). A mutation happens inside a snapshot and becomes visible to others
only when the snapshot is applied, atomically. Readers always see a consistent
version. Background threads can prepare changes in their own snapshot and
apply them, with conflicts detected and merged by policy.

**Strengths.** Consistency without locks; batching for free (many writes, one
apply, one render); safe concurrent preparation of state; the foundation for
the consistency guarantee that interruptible rendering (`C01`) needs.

**Weaknesses.** A sophisticated runtime mechanism; conflict policies are
subtle; memory overhead for retained versions.

**Opportunities.** RustNative already forbids mutation from workers
(`PLAN.md` 2.12) and funnels results through the scheduler, which gives
consistency by serialization. The transferable part is *batching and atomic
apply*: several state changes produced by one message become visible together,
in one render. Ownership also makes the "prepare off-thread, apply on the UI
thread" pattern expressible as moving a value, which is cheaper and simpler
than a version store.

**Threats.** Without atomic batching, intermediate states render — the
familiar "flash of inconsistent UI".

**Position.** **Partial.** Updates are serialized through the scheduler; the
batching and atomicity guarantee is not stated.

**Requirements.**

- `C09-1` `[X]` A stated batching guarantee: all state changes caused by one
  message become visible in the same render, and no render observes a partial
  set of them.
- `C09-2` `[X]` A move-based "prepare off the UI thread, apply atomically"
  pattern for large state updates, documented with an example.

## C10 — Statecharts and explicit state machines

**Introduced by.** D5 (declarative state machines in its UI layer), and a
long line of UI-logic libraries in the web ecosystem.

**Mechanism.** UI logic is modelled as a hierarchical state machine: explicit
states, events, guarded transitions, entry and exit actions, parallel and
nested states, history. The machine is data, so it can be visualized, tested
exhaustively, and simulated.

**Strengths.** Impossible states are impossible; complex flows (checkout,
onboarding, media players, connection management) become auditable; machines
can be visualized for non-engineers.

**Weaknesses.** Verbose for simple cases; a second modelling language alongside
the component model; libraries that interpret machines at runtime lose type
checking.

**Opportunities.** Rust enums with exhaustive matching are a statechart's core
already, checked at compile time; what is missing is guidance and a small
amount of support — entry/exit effects bound to the scope model, and
visualization from the type.

**Threats.** Low as an absence; high as an opportunity missed, because this is
a place where Rust is visibly better than every other substrate in the
analysis.

**Position.** **Absent** as documented practice.

**Requirements.**

- `C10-1` `[X]` A documented state-machine pattern using enums, with entry and
  exit effects tied to task scopes, so leaving a state cancels its work.
- `C10-2` `[X]` Optional generation of a state diagram from a state-machine
  type, for documentation and the inspector.

## C11 — Reducer-effect architecture with exhaustive testing

**Introduced by.** M2's disciplined architecture libraries, W14's state
containers, and the unidirectional-message lineage (F4.1 variant 2).

**Mechanism.** State changes only through a reducer: `(state, action) → (new
state, effects)`. Effects are values describing work, executed by the runtime,
whose results come back as actions. External dependencies (clock, network,
storage, randomness) are injected and overridable. A test store sends actions
and must assert *every* state change and *every* effect — an unasserted effect
fails the test.

**Strengths.** Total testability, including of asynchronous behaviour; time
travel and replay become straightforward; dependencies are explicit; composition
of features through scoped reducers.

**Weaknesses.** Boilerplate; exhaustiveness is punishing for large features;
performance overheads if every change flows through one root.

**Opportunities.** The message model already exists (Milestone 16) and
structured scopes already execute work. The test half is the valuable half:
with the deterministic clock and executor (`X-L7-6`) a test harness can observe
every task spawned by a component and require the test to account for it —
the exhaustive-effect guarantee, without requiring the reducer shape
everywhere.

**Threats.** Teams arriving from this architecture will judge the framework's
testability by exactly this property.

**Position.** **Absent** as a testing capability.

**Requirements.**

- `C11-1` `[X]` An exhaustive test mode: a component test fails if a task,
  effect, or outgoing message occurred that the test did not assert.
- `C11-2` `[X]` Dependency overrides — clock, network, storage, randomness,
  and any application service — per test, through the service contracts
  (Milestone 20), with no global mutable state.

## C12 — Reactive streams

**Introduced by.** M1, M2, D1, and the reactive-extensions lineage.

**Mechanism.** Values over time as composable streams with operators (map,
filter, debounce, throttle, combine, flat-map, retry). Two kinds: *cold*
streams start work per subscriber; *hot* streams share one source among
subscribers (latest-value state streams, broadcast event streams). Backpressure
lets a slow consumer signal demand so producers do not overwhelm it. Schedulers
decide which thread each stage runs on.

**Strengths.** Declarative handling of time-based behaviour (search-as-you-type,
sensor fusion, reconnect logic); composition; explicit concurrency.

**Weaknesses.** Steep learning curve; operator soup; debugging stack traces
through operator chains; subscription leaks when lifetime is not bound.

**Opportunities.** Rust's async streams already exist in the language
ecosystem. The framework's contribution is lifetime: a stream collected by a
component is bound to that component's scope (and suspended with it, `C02`), so
subscription leaks are impossible rather than discouraged. Backpressure is also
native to pull-based async streams.

**Threats.** Without first-class stream integration, developers wire streams
through ad-hoc tasks and reinvent lifetime binding badly.

**Position.** **Partial** — async tasks exist; stream collection bound to
component scope is not a documented primitive.

**Requirements.**

- `C12-1` `[X]` Stream collection as a component primitive, bound to the
  component's scope, suspended with it, and delivering values as messages.
- `C12-2` `[X]` Documented hot/cold semantics for framework-provided streams
  (state, events, sensors), with backpressure behaviour stated per stream.

## C13 — Navigation as state, and the URL as typed state

**Introduced by.** M2's architecture libraries, M1, W3, and type-safe routing
libraries in W14.

**Mechanism.** Two related ideas. First, navigation is *derived from state*:
the stack of screens, the presented sheet, the selected tab are fields in the
application state (often an enum per destination), and changing the state
navigates. Deep links and restoration become "construct this state". Second,
on the web, the URL — path *and* query parameters — is treated as typed,
validated state with defaults, so filters, sorting, and pagination live in the
URL, are shareable, and survive reload, with type errors caught at compile
time.

**Strengths.** Navigation is testable without a host; deep links and state
restoration are the same mechanism; impossible navigation states (a sheet over
a screen that does not exist) cannot be constructed; shareable URLs for free.

**Weaknesses.** Tension with host navigation controllers that own their own
stack; animations must be derived from state diffs.

**Opportunities.** Milestone 30 already has `Route`/`Router` and persistence.
Making navigation a projection of typed state, and the router a reconciler
between that state and the host's navigation stack, is exactly the
"declarative tree is the source of truth" principle (2.8) applied to
navigation. Rust enums make each destination and its parameters a type.

**Threats.** Navigation bugs — back-stack corruption, lost state on deep link —
are among the most-reported defects in mobile applications; a framework that
does not structurally prevent them inherits them.

**Position.** **Partial** — typed routes exist (Milestone 30); navigation state
as the source of truth for the host stack, and typed query state, are not
specified.

**Requirements.**

- `C13-1` `[X]` Navigation state as a typed value from which the host
  navigation stack is reconciled; deep links, restoration, and programmatic
  navigation all construct that value.
- `C13-2` `[W]` Typed query/search parameters with validation and defaults,
  round-tripped through the URL, compile-time checked at use sites.
- `C13-3` `[X]` Per-destination state scoping: state that survives while its
  destination is on the stack and is released when it leaves (see `C14`).

## C14 — State scoped to the right lifetime

**Introduced by.** M1, M2.

**Mechanism.** Three lifetimes, not one: the *view* (destroyed on
configuration changes such as rotation), the *destination* (lives while the
screen is on the navigation stack, survives configuration changes), and the
*process* — with a saved-state handle that writes the destination's essential
state into the host's restoration bundle so it survives process death.

**Strengths.** Configuration changes stop losing state; process death stops
losing user input; each piece of state lives exactly as long as it should.

**Weaknesses.** Three lifetimes are hard to teach; the saved-state size limit
is a trap.

**Opportunities.** RustNative's components are not destroyed on configuration
changes (the tree is reconciled, not rebuilt), which removes the first
lifetime's problem entirely. What remains is destination scope (`C13-3`) and
selective saved state bound to Milestone 30's restoration.

**Threats.** Process-death data loss is a top review complaint on mobile.

**Position.** **Partial** — restoration exists; per-destination scope does not.

**Requirements.**

- `C14-1` `[M]` `[X]` A declared saved-state subset per destination, written to
  the host's restoration mechanism, with a size budget and a test that kills
  and restores the process.

---

# Part C — Composition and dependencies

## C15 — Environment propagation down, preferences up

**Introduced by.** M1, D1, W1, D3.

**Mechanism.** Values flow *down* the tree implicitly — theme, locale, layout
direction, size class, text scale, the current service instances — and any
descendant reads them without every intermediate passing them as props; a
subtree can override a value for its descendants. Values also flow *up*: a
descendant publishes a *preference* (a title for the enclosing navigation bar,
a preferred size, an anchor for an overlay) that an ancestor reads and
combines.

**Strengths.** Removes prop threading; makes theming, localization, and
dependency injection one mechanism; makes host traits (dark mode, size class)
available everywhere consistently; upward preferences solve the "child decides
something the parent renders" problem without callbacks.

**Weaknesses.** Implicit dependencies are harder to see; overuse turns into
global state with extra steps; changes to a high-level value can invalidate
large subtrees.

**Opportunities.** Milestone 20 provides services and Milestone 21 provides
theme; the concept unifies them. Rust can make environment reads typed (a key
is a type) and the invalidation precise (only readers of a changed key
re-render). This is a Tier 0 item because every backend feeds host traits into
the environment and every component library control reads from it.

**Threats.** If each backend invents its own way of exposing host traits, the
portable model fragments.

**Position.** **Partial** — theme and services exist as separate mechanisms;
no general, overridable, typed environment; no upward preferences.

**Requirements.**

- `C15-1` `[X]` A typed, subtree-overridable environment, carrying theme,
  locale, layout direction, text scale, size class, colour scheme, reduced
  motion, and services, with invalidation limited to readers of a changed key.
- `C15-2` `[X]` Host traits delivered into the environment by every backend,
  so a component never queries the host directly for them.
- `C15-3` `[X]` Upward preferences: a typed channel for a descendant to publish
  a value that an ancestor reduces and reads during layout or render.

## C16 — Hierarchical dependency injection with lifecycle scopes

**Introduced by.** W9, W7, M1, M2, D3.

**Mechanism.** Dependencies are declared, not constructed; a container
resolves them, with scopes matching lifetimes (application, window,
destination, request); tests replace any dependency. The modern generation
resolves the graph at compile time, failing the build on a missing binding and
paying nothing at runtime. Convention-based auto-configuration assembles
defaults when a capability is present (`C40`).

**Strengths.** Testability; separation of construction from use; lifetime
correctness when scopes match component lifetimes.

**Weaknesses.** Container magic, reflection costs, runtime resolution errors in
the older generation; ceremony.

**Opportunities.** Services (Milestone 20) plus the environment (`C15`) are a
compile-time dependency system without a container. The transferable parts are
*scoping* (a service instance per window, per destination, per request) and
*override in tests* (`C11-2`).

**Threats.** Teams arriving from managed server platforms expect scoped
dependencies and will build ad-hoc singletons if there are none — which is the
global mutable state section 9 forbids.

**Position.** **Partial** — services exist; scoping per window, destination,
and request is not specified.

**Requirements.**

- `C16-1` `[X]` Service scopes per application, window, destination, and
  request, with construction and disposal bound to the scope's lifetime and
  missing services reported at compile time where the type system allows.

## C17 — Actors and supervision

**Introduced by.** The persistent-connection server archetype (W16) and its
runtime lineage; D5's threading model; M1's supervisor scopes.

**Mechanism.** Units of state and behaviour (actors, processes) communicate
only by message, own their state exclusively, and fail independently. A
*supervisor* watches its children and applies a restart strategy (restart
one, restart all, escalate) when they fail — "let it crash" instead of
defensive error handling everywhere. Coroutine libraries adopted the smaller
version: a *supervisor scope* in which one child's failure does not cancel its
siblings. The server runtime of this lineage also upgrades code in a running
system without dropping state.

**Strengths.** Fault isolation and recovery as architecture rather than as code;
massive concurrency; systems that run for years; a simple answer to "what
happens when this fails?".

**Weaknesses.** Message-only communication has overhead and can hide flow;
restart strategies can mask real bugs; hot upgrade is operationally complex.

**Opportunities.** Components with scope-bound tasks are already actor-shaped:
exclusive state, message delivery through the scheduler. What is missing is the
*supervision* half — the policy for a failing task or subtree — which is what
error boundaries (`X-L4-3`) should be built on rather than inventing a second
mechanism. The same model serves long-lived server and device processes
(`C45`, `C87`).

**Threats.** Without supervision, a panic in a background task either kills the
application or is silently lost — both unacceptable on servers and devices.

**Position.** **Absent** — structured scopes cancel on unmount; failure policy
is unspecified.

**Requirements.**

- `C17-1` `[X]` Supervision policies on task scopes and component subtrees —
  isolate, restart with backoff, escalate — with error boundaries (`X-L4-3`)
  defined as a supervision policy on a subtree.
- `C17-2` `[X]` A failure in one child task never cancels siblings unless the
  scope's policy says so, with the default stated and tested.

## C18 — Modifier chains, attached properties, and value precedence

**Introduced by.** M1 (modifier chains), D1 and D3 (attached properties and a
property system with value precedence and inheritance).

**Mechanism.** Three related ideas about how properties reach an element.
*Modifier chains* apply ordered wrappers (padding, then background, then
click handling) where order is meaningful and visible. *Attached properties*
let a parent define properties that its children carry (a grid row, a dock
edge) without the child's type knowing about the parent. A *property system*
resolves a property's value from ordered sources — animation, local value,
style trigger, style, inherited value, default — so the answer to "why is this
blue?" is deterministic and inspectable.

**Strengths.** Composable and ordered styling; parent-specific layout data
without coupling; deterministic, explainable value resolution; inheritance of
text properties and similar values down the tree.

**Weaknesses.** Order-sensitive modifiers surprise newcomers; property
precedence systems are powerful and opaque in equal measure.

**Opportunities.** The builder API's `with_*` chain is already a modifier chain,
and markup attributes lower onto it (`PLAN.md` 2.9). The concepts to adopt are
*layout data owned by the container* (attached properties, typed per container)
and *a documented precedence order* for values that can come from several
places (theme, style, local, animation), surfaced by the inspector's layout
explanation (`X-L3-5`).

**Threats.** Without a stated precedence order, style bugs become
"works on one backend, not another".

**Position.** **Partial** — chains exist; precedence and container-owned layout
data are not specified.

**Requirements.**

- `C18-1` `[X]` Container-owned, typed layout data for children, expressible in
  both syntaxes.
- `C18-2` `[X]` A documented precedence order for property values (animation,
  local, style, theme, inherited, default), shown per property by the
  inspector.

## C19 — Lookless controls and headless behaviour components

**Introduced by.** D1 and D3 (templated controls), and headless component
libraries in W14.

**Mechanism.** A control's *behaviour and semantics* (state, keyboard handling,
focus management, accessibility roles) are separated from its *appearance*.
Templated controls let the appearance be replaced wholesale while the behaviour
stays; headless component libraries ship only the behaviour and accessibility,
leaving all visual output to the application.

**Strengths.** Brand freedom without re-implementing hard behaviour; one
accessible implementation of a combobox or date picker, however it looks;
design systems built on proven behaviour.

**Weaknesses.** Replacing a template can silently break accessibility or focus
visuals; headless components need a lot of styling to be usable.

**Opportunities.** On host-native backends the appearance is the host's, which
is the point (2.2). But the draw-list path (Milestone 29), the terminal, and
embedded targets draw their own controls — and custom components in every
application need proven behaviour. A headless behaviour layer (focus, keyboard,
selection, accessibility semantics) usable by drawn controls, and by
application-authored composite controls on native backends, is the correct
shape, and it is exactly what the hybrid pattern (`D-SD-1`) needs.

**Threats.** Without it, every drawn control and every custom composite
reimplements keyboard and accessibility behaviour, and gets it wrong.

**Position.** **Absent.**

**Requirements.**

- `C19-1` `[X]` A headless behaviour layer for composite controls — list
  selection, combobox, menu, tabs, tree, grid navigation, date entry — carrying
  focus, keyboard, and accessibility semantics independent of appearance.
- `C19-2` `[X]` Drawn controls on the terminal, embedded, and custom-drawing
  paths built on that layer rather than reimplementing it.

## C20 — The command model

**Introduced by.** D1, D3, D5, and desktop editors generally.

**Mechanism.** An action — save, undo, copy, toggle bold — is a first-class
object with a name, an icon, a shortcut, an enabled state, and a checked state.
Menus, toolbars, context menus, keyboard shortcuts, and command palettes all
bind to the same command, so disabling it disables every surface at once, and
the shortcut shown in a menu is always the shortcut that works. Commands route
through the focus chain, so "copy" means copy *in the focused element*.

**Strengths.** Consistency across surfaces; discoverability (palettes, menus);
correct enabled states; a natural home for undo integration and for
accessibility (every action named).

**Weaknesses.** Indirection for simple applications; routing through focus can
be confusing.

**Opportunities.** Milestone 23 has native menus; Milestone 25 has advanced
input; neither shares a command concept. The command model is Tier 0 because
it shapes how menus, shortcuts, toolbars, and system-level surfaces (`C49` —
jump lists, dock menus, tray menus) bind on every backend.

**Threats.** Menus and shortcuts that disagree are one of the clearest signals
of a non-native application on desktop.

**Position.** **Absent.**

**Requirements.**

- `C20-1` `[X]` A portable command type — identity, label, icon, shortcut,
  enabled, checked — bound by menus, toolbars, context menus, shortcuts, and
  host-level surfaces, with enablement updated through ordinary state.
- `C20-2` `[D]` Command routing through the focus chain, matching each host's
  responder or routing conventions.
- `C20-3` `[X]` A command palette component driven by the same registry.

## C21 — Typestate

**Introduced by.** E5 (the Rust embedded ecosystem), and type-driven design
generally.

**Mechanism.** An object's state is encoded in its type, so operations valid
only in some states are only callable in those states: a pin configured as
output has a `set_high` method and an input does not; a validated form value
has a different type from a raw one; a connected client has methods a
disconnected one lacks. Transitions consume the old value and return a new
type.

**Strengths.** Misuse becomes a compile error; no runtime checks; APIs document
themselves.

**Weaknesses.** Type complexity; poor fit for state that changes at runtime in
ways the program cannot know statically (which is most UI state).

**Opportunities.** Typestate fits framework *APIs* rather than application
state: a host object handle that is only usable while realized, a form value
that must be validated before submission (`W-SF-7`), a capability that must be
checked before the service is obtained. Each converts a class of runtime error
into a compile error, which is this substrate's native advantage.

**Threats.** Low — but each place it is not used is a runtime error class
kept by choice.

**Position.** **Partial** — used in places; not a stated design rule.

**Requirements.**

- `C21-1` `[X]` A design rule and review checklist: where an API has states
  with different valid operations and the state is statically knowable, encode
  it in the type; applied first to validated form values, capability-gated
  services, and escape-hatch host handles.

---

## Part summary

The concepts in this part fall into three groups.

**Concepts our root choices make cheaper or better than for their
originators:** interruptible rendering (`C01`), statecharts (`C10`), typestate
(`C21`), actor-style supervision (`C17`), server components (`C05`), and
stream lifetime (`C12`). Each had to be retrofitted or enforced by discipline
elsewhere; here the preconditions — pure render, separated effects, structured
scopes, exhaustive enums, ownership — already exist.

**Concepts that are Tier 0 because they shape every backend and every
control:** the environment (`C15`) and the command model (`C20`).

**Concepts that are expectations rather than differentiators:** derived state
(`C08`), atomic batching (`C09`), navigation as state (`C13`), scoped lifetimes
(`C14`, `C16`), exhaustive effect testing (`C11`), skipping (`C04`), and
visibility-aware work (`C02`).
