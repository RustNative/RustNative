# RustNative — Master Development Plan

## 1. Vision

Build a Rust-first application framework that lets developers write the application model, state, components, layout, and business logic once while using the operating system's native primitives instead of emulating an operating system on top of a custom renderer.

The long-term target platforms are:

- Windows
- macOS
- Linux
- Android
- iOS
- Web (Rust on the server; HTML, CSS, and compile-time generated JavaScript
  in the browser, with WebAssembly for the subtrees that opt into it, over the
  browser DOM/Web APIs), in every deployment mode a web application is written
  in: client-side, server-rendered, and serverless
- Terminal user interfaces, on the desktop operating systems and on embedded
  Linux consoles
- Embedded Linux
- RTOS / selected bare-metal embedded targets

No target on that list is a port of another one. Each gets a backend of the
same shape — native host objects, native text measurement, native input,
native accessibility, native services, its own toolchain and packaging — and
the order they are built in follows what the core contracts are ready for and
what hardware the project can verify on, not which platform is considered more
important.

The framework follows the philosophy of driving real native host controls from a portable application runtime, but that runtime is Rust rather than a scripting language and its boundary to the host is compiled rather than serialized:

```text
Application
    │
    ├── shared Rust state / business logic
    ├── component tree
    ├── declarative UI tree
    ├── layout / events / scheduling
    └── platform services
             │
             ▼
      platform adapter
             │
             ▼
      native OS APIs
```

That application model has two authoring surfaces, not one. A tree can be written with the builder API or in markup — directly, as an expression anywhere in a `.rsx` source file, or inside the `rsx!` macro in an ordinary `.rs` file — and the two are spellings of the same tree rather than two frameworks bolted together: the markup form expands to the builder form at compile time and adds nothing to the runtime. Both are supported natively, both reach the entire API, and neither is treated as the primary one (2.9).

Styling is arranged the same way. One resolved style model sits under the tree — theme tokens, component defaults, per-node overrides, state variants — and it has two spellings: typed style properties, and a utility-class vocabulary written in the styling language the largest population of UI developers already uses. The classes are compiled to those same properties at build time, so no cascade, no selector, and no class string reaches a backend, and a style a backend cannot realize is a diagnostic rather than a silent omission (2.14).

The framework must not become a lowest-common-denominator abstraction that hides the unique capabilities of each operating system. Portable semantics should be consistent, while platform-specific capabilities remain available through explicit capability APIs and native escape hatches.

---

## 2. Core architectural principles

### 2.1 Rust owns application semantics

Rust owns:

- application state;
- component state;
- props;
- messages and callbacks;
- declarative UI trees;
- reconciliation;
- scheduling;
- task ownership;
- layout semantics;
- cross-platform service contracts.

### 2.2 The OS owns the native UI

The framework should use native host controls and platform services wherever a native equivalent exists.

What the host *is* differs per target, and that difference is the point:

```text
Windows   HWND + common controls
macOS     NSView + AppKit controls
Linux     the chosen toolkit's native widgets
Android   the android.view.View hierarchy
iOS       UIView + UIKit controls
Web       semantic DOM elements
Terminal  the terminal's own cell grid and input protocols
Embedded  the display and input the device actually has
```

The Windows backend, for example, creates real Win32 `HWND`s. Every other backend should realize the tree in its own host's terms rather than painting a facsimile of an operating-system UI.

### 2.3 A target is first-class or it is not a target

Desktop, mobile, Web, terminal, and embedded targets are planned as peers. It is not acceptable to build the desktop platforms first and then emulate them elsewhere — neither in a generic canvas-based browser runtime nor in a text-mode imitation of a window manager. Each backend is written against the primitives its host actually offers, and each one is allowed to be different where its host is different.

**Web.** The Web backend should use browser-native primitives wherever appropriate:

- DOM elements for semantic controls and containers;
- CSS for browser-native layout and visual mechanics where the framework can map its layout semantics safely;
- browser event listeners and event propagation;
- browser focus and selection APIs;
- browser accessibility semantics through HTML/ARIA;
- Web APIs for storage, networking, clipboard, notifications, media, sensors, permissions, and other capabilities;
- Rust on the server for every line of application code, serverful or serverless;
- in the browser, HTML, CSS, and JavaScript the framework generates at compile time from the application's client logic by default;
- WebAssembly for the subtrees that opt into it, where client logic needs the full language — heavy computation, rich offline behaviour, or logic such as sync merge functions that must be the same Rust in the browser as everywhere else.

**Terminal.** A terminal backend should use the terminal's own primitives: the cell grid, its text attributes and colour depth, its key and mouse reporting protocols, its resize notifications, its alternate screen and cursor control, and — where the terminal offers one — its clipboard escape sequence. A terminal is a real host with real conventions, not a canvas for drawing fake title bars and shadows.

**Embedded.** An embedded backend should express what the device has and nothing more. Targets without a window manager do not grow one; targets without accessibility services do not pretend to have them. The capability model (2.5) is what makes that honest rather than lossy.

Every adapter therefore follows the same shape:

```text
Rust application
    ↓
rustnative-core
    ↓
rustnative-<platform>        rustnative-web        rustnative-tui
    ↓                           ↓                     ↓
native OS APIs             HTML + generated JS    terminal I/O
                           (+ opt-in WASM)
    ↓                           ↓                     ↓
native controls            DOM / CSS / Web APIs   cells / key + mouse
                                                  protocols
```

The framework must also explicitly account for each host's constraints, and design for them up front rather than discovering them at integration time:

- **browser**: the single-threaded main-thread model for DOM access, asynchronous Web APIs, browser lifecycle, URL/history/navigation, page visibility, storage quotas, user-gesture restrictions, attaching client code to server-rendered markup, and browser security boundaries;
- **terminal**: cell-granular geometry, no overlapping native windows, text-only measurement, colour and capability differences between terminals, input that arrives as escape sequences, and accessibility that belongs to the terminal rather than to the application;
- **mobile**: process lifecycle and reclaim, configuration changes, and permission prompts;
- **embedded**: constrained memory, fixed displays, and the absence of a general-purpose OS.

### 2.4 Platform-independent core, platform-specific adapters

`rustnative-core` must never depend on Windows, macOS, Android, iOS, GTK, AppKit, UIKit, WinUI, JNI, Objective-C, browser/DOM bindings, terminal or terminfo libraries, or other platform APIs.

Platform crates implement the contracts exposed by the core. When a host needs something the contracts cannot express, the contract is widened portably — as `Executor` was, when the scheduler was found hard-wired to one Tokio runtime — rather than an `#[cfg]` for that host being added to the core.

### 2.5 Capabilities over platform conditionals

Prefer capability contracts such as:

```text
HasClipboard
HasCamera
HasNotifications
HasBluetooth
HasStorage
```

over application-wide checks such as `if platform == Android`.

### 2.6 Native escape hatch

A serious cross-platform framework must allow advanced applications to access native platform objects and APIs when the portable abstraction is insufficient.

The escape hatch must be explicit and isolated so platform-specific code does not leak into the portable core.

### 2.7 Stable identity is fundamental

Framework node IDs and component keys provide stable identity across rerenders. Native objects must be reused when identity remains stable.

### 2.8 Declarative tree is the source of truth

The application produces a declarative tree. Native objects are a realization of that tree, not an independent source of truth.

### 2.9 One tree, two syntaxes

The tree 2.8 describes has two spellings, and both are first-class ways to write an application:

- a **builder syntax** — `Node::column(...)` and the `with_*` modifiers, which is ordinary Rust: values, method chains, iterators, and functions that return `Node`;
- a **markup syntax** — elements with typed attributes, nested children, and Rust expressions in braces, written as an expression wherever Rust expects one.

The markup syntax has one grammar and two carriers:

- **`.rsx` files.** A `.rsx` file is Rust with one more kind of expression: an element. Markup is written directly wherever an expression is valid — a `let` initializer, a return value, a closure body, a match arm, an argument — with no wrapper around it. This is the same arrangement as markup-extended source files elsewhere, and it has the same mechanics: the host language's compiler does not accept the extension, so a compile step lowers the file to plain Rust before `rustc` sees it (`rustnative_build::compile_rsx()`, run from the build script).
- **The `rsx!` macro.** The same markup, delimited, inside any `.rs` file. It needs no build step, so it is how markup appears in a crate that has none, in a `.rs` module that wants one markup-shaped subtree, and in runnable documentation examples, which `rustdoc` compiles as plain Rust.

```text
   builder syntax                   markup syntax
                             ┌───────────┴───────────┐
   Node::column(..)       .rsx file               .rs file
                          <Column ..>             rsx! { <Column ..> }
          │                  │                       │
          │                  │ compile_rsx()         │
          │                  │ wraps it in rsx!      │
          │                  └──────────┬────────────┘
          │                             │ rsx! expands to
          └──────────► builder calls ◄──┘
                             │
                   Node — the same tree
```

The carriers share everything but the delimiter. `compile_rsx()` does exactly one thing to a `.rsx` file: it finds each markup expression and wraps it in `rsx!`, leaving every other byte of the file where it was. Parsing, lowering, and diagnostics therefore exist once, in the macro, and a `.rsx` file cannot accept anything the macro rejects or mean anything the macro would not. The grammar is identical by rule as well as by implementation: any element can move between a `.rsx` file and an `rsx!` call without being rewritten.

Neither syntax is a layer over the other. The markup form is a compile-time front end that expands to builder calls and nothing else: it introduces no node kind, no runtime type, no allocation, no indirection, and no capability of its own. Everything downstream — reconciliation, layout, realization, accessibility, animation, virtualization — sees one tree and cannot tell which syntax or carrier produced it, because by construction there is nothing to tell.

Three rules follow, and they are the whole of the principle:

- **Capability equality.** Anything one syntax can express, the other can express. Every attribute is a builder method; every builder method is reachable from markup; `{ }` splices any Rust expression into markup, and `..expr` applies any builder chain to a markup element. A feature is not finished when it works in one syntax.
- **No compromise in either direction.** Neither syntax is narrowed to keep the other reachable. Markup gets the constructs markup is good at — nesting that matches the tree, conditional and repeated children, fragments, component elements with typed props, and in `.rsx` files, markup as a plain expression with no wrapper. The builder API gets the constructs an API is good at — composition through ordinary functions, iterator pipelines, conditional chaining, extension traits. A translation between them is not required to be literal, and idiomatic code in each will not look like the other.
- **Equal standing.** Documentation, examples, project templates, the component library, and the conformance suites carry both. Neither is "the real one" with the other as sugar, and neither is presented as the default a developer should prefer.

Equality of this kind decays unless it is tested, so it is: an equivalence suite asserts that both spellings of every documented node kind and modifier produce equal `Node` values (Milestone 41), and the markup diagnostics — reported against the `.rsx` file or the `rsx!` call the developer actually wrote — are held to the same bar as a compiler's (Milestone 53).

### 2.10 Transient interaction state is not a rerender

Examples such as scrolling, focus transitions, pointer movement, and other high-frequency native interaction should update native runtime state directly when possible instead of rebuilding the entire component tree.

### 2.11 Layout is platform-independent

The core calculates semantic geometry, constraints, clipping, intrinsic measurement requests, and coordinate spaces. A platform backend applies the resulting geometry to native objects and supplies native measurement capabilities.

A backend whose host does not address space in pixels — a terminal, which addresses it in character cells — converts at its own boundary and reports its measurements in the same unit it converts to, so the core keeps one geometry model rather than one per host.

### 2.12 Async work must respect component lifetime

Background work must never mutate component state directly from a worker. Results return through the framework scheduler/event loop, and task lifetime must be tied to component lifetime through structured task scopes.

### 2.13 Hardware availability changes the order, not the plan

The project does not currently have a macOS machine or an iOS device, so Milestones 33 and 36 cannot be built and verified here yet. That is a scheduling fact, not a scope decision: macOS and iOS remain fully planned platforms, specified at the same depth as the rest, and the core contracts are designed against their documented APIs (AppKit/UIKit view hierarchies, Core Text measurement, `NSAccessibility`/`UIAccessibility`, the Apple toolchains) so that nothing in the portable layer has to be renegotiated when the hardware arrives.

Two rules keep that honest, and they apply to every platform the project cannot exercise on the machine in front of it:

- a backend advertises a `Capability` only once it actually realizes it, never because the plan says it will (the Windows backend already asserts this about itself);
- `BUILD_STATUS.md` states what was verified and on what. Work reasoned through but not run is recorded as exactly that, and a platform is called supported only after it runs on real hardware.

### 2.14 One style model, two spellings

Milestone 21 established that a node's visual style is *resolved* in the core — theme token, component default, node override, and interaction state merged into concrete values — before any backend sees it. That resolved style is the model, and the only thing a backend is ever handed. Everything above it is a way of writing into it:

```text
   style properties                    utility classes
   .with_style(..) / style={..}        .with_class(classes!("..")) / class=".."
          │                                   │  resolved at build time
          └────────────► declarations ◄───────┘
                              │  tokens stay references
                              ▼
                    resolved style — what a backend applies
```

A **declaration** is a property and a value in the vocabulary the web platform made universal — lengths, colours, the arithmetic and colour functions over them, and references to named tokens. It is the shared value language of both spellings, and it is deliberately the *declaration* half of that platform's styling model and not the other half: there are no selectors, no specificity, no cascade, and no inheritance beyond the text properties that inherit everywhere. A style belongs to the node that declares it, which is what keeps resolution deterministic (Milestone 21) and independent of where a node happens to sit in the tree.

Four rules follow, and they are the whole of the principle:

- **Vocabulary equality.** Every utility class resolves to style properties reachable from both syntaxes, and every style property has a spelling in the utility vocabulary or is documented as having none. A class that does not resolve is a compile error naming the property it was looking for, rather than a class silently dropped — which is how utility vocabularies normally fail.
- **No second engine.** The utility layer is a compile-time front end, the way markup is (2.9). It introduces no runtime type, no matching step, no style storage, and no capability of its own: `rustnative expand` shows the style properties a class string became, and an equivalence suite asserts that the two spellings resolve to equal values.
- **Tokens are references, not constants.** A token-valued declaration resolves through the theme at style-resolution time, so a theme change, a colour-scheme change, or a user's own palette is a re-resolution rather than a rebuild; values declared fixed are folded at build time and stated to be fixed. This is also the boundary 2.2 needs — semantic tokens may map to host appearance, while absolute brand values are applied as given.
- **Capability honesty, per property.** Rounded corners, shadows, gradients, and transforms exist on some hosts and not others, and a terminal cell has neither a radius nor a shadow. Each backend declares, per style property, whether it realizes it, approximates it, or cannot express it, and an application that declares one its target cannot realize is told at build time (2.5, 2.13). A backend never silently ignores a style, and never abandons a native control to owner-drawing in order to satisfy one — that trade is exactly what 2.2 exists to refuse.

Units follow 2.11 rather than the web's: the core keeps one geometry model in logical pixels, `rem` is that model's root text size scaled by the host's text setting, and a backend whose host addresses space differently — points, density-independent pixels, character cells — converts at its own boundary with a documented mapping and rounding rule.

---

# 3. Completed milestones

The following milestones are implemented in the current codebase. The current production backend is Windows/Win32; every other target in section 1 is planned and not yet implemented.

## Milestone 1 — Framework foundation and native Windows label

Implemented:

- Cargo workspace;
- `rustnative-core` crate;
- `rustnative-windows` crate;
- example application crate;
- `Platform` abstraction;
- native Win32 window creation;
- native Win32 `STATIC` label;
- Windows-specific unsafe/FFI code isolated in the Windows backend.

## Milestone 2 — Stable node identity + native-object ownership + UI tree

Implemented:

- declarative `Node` tree;
- stable `NodeId` keys;
- native object registry;
- `NodeId -> NativeObject -> HWND` ownership;
- duplicate-ID detection;
- native object reuse based on stable identity;
- logical containers separated from native ownership.

## Milestone 3 — Application state + initial reconciliation

Implemented:

```text
State -> view() -> Node tree -> renderer -> native objects
```

State updates rebuild the declarative tree while stable IDs preserve native controls.

## Milestone 4 — Components + events + proper tree diffing

Implemented:

- `Component` abstraction;
- component `view()` and `update()`;
- semantic framework events;
- explicit tree snapshots;
- structural tree diffing;
- insert/update/remove/move operations;
- stable node identity during updates.

## Milestone 5 — Native container ownership + hierarchical layout

Implemented:

- native `Column` containers;
- native parent/child HWND hierarchy;
- nested containers;
- recursive layout;
- native reparenting for moved nodes;
- hierarchical event propagation.

## Milestone 6 — Deterministic layout architecture

Implemented:

- layout as a separate phase after reconciliation;
- deterministic sibling ordering;
- explicit coordinate-space contract;
- child rectangles expressed relative to native parents;
- layout result separated from native application.

## Milestone 7 — General layout model

Implemented:

- `Auto` sizing;
- `Fixed` sizing;
- `Fill` sizing;
- alignment;
- padding;
- margins;
- gaps;
- column layout;
- row layout foundations.

## Milestone 8 — Intrinsic measurement + `Row` + layout invalidation

Implemented:

- platform `IntrinsicMeasurer` contract;
- native Windows text measurement;
- `Row` container;
- horizontal flow;
- layout invalidation detection;
- bounded intrinsic measurement support.

## Milestone 9 — Constraints + text wrapping

Implemented:

- min/max width constraints;
- min/max height constraints;
- bounded text measurement;
- multiline height calculation;
- Windows text wrapping using native GDI measurement/drawing APIs;
- constraint-aware final geometry.

## Milestone 10 — Overflow, clipping, and scrolling

Implemented:

- `Visible`, `Clip`, and `Scroll` overflow semantics;
- scroll ranges;
- scroll offsets;
- clipping regions;
- proper native viewport/content-host architecture;
- stable child coordinates during scrolling;
- scroll updates that do not rerender the component tree;
- native event-loop wheel handling;
- regression protections against scroll/layout feedback loops.

The native scroll hierarchy is:

```text
Scrollable container
└── Viewport HWND
    └── Content HWND
        ├── child HWND
        ├── child HWND
        └── ...
```

## Milestone 11 — Focus, keyboard input, and accessibility semantics

Implemented:

- framework focus model;
- focus gained/lost events;
- keyboard abstraction independent of Win32 virtual-key constants;
- key modifiers;
- keyboard event routing;
- Tab / Shift+Tab focus traversal foundations;
- accessibility roles;
- accessibility names;
- accessibility descriptions;
- focusability semantics;
- native controls continue to provide their normal platform accessibility behavior.

A full custom Windows UI Automation provider is intentionally still future work.

## Milestone 12 — Text input controls + controlled component state

Implemented:

- native Windows `EDIT` control;
- `TextInput` node;
- controlled value model;
- `TextChanged` framework event;
- native-to-Rust-to-native synchronization;
- suppression of feedback loops caused by programmatic native value updates;
- preservation of stable native input identity.

## Milestone 13 — Component composition + lifecycle

Implemented:

- reusable child components;
- explicit mount/update/unmount lifecycle;
- `ComponentHost` ownership primitive;
- lifecycle stability across rerenders;
- lifecycle replacement semantics;
- child state persistence across parent rerenders.

Key invariant:

```text
rerender != unmount + remount
```

## Milestone 14 — Framework-managed component tree

Implemented:

- framework-owned child component entries;
- stable keyed child identity;
- automatic child creation/reuse/removal;
- automatic lifecycle transitions;
- automatic event ownership routing;
- `NodeId -> ComponentId` ownership mapping;
- framework-managed component lifetime.

Application code no longer needs to manually route every child event or manually maintain child component hosts for ordinary composition.

## Milestone 15 — Component props / parent-to-child data flow

Implemented:

- typed `Component::Props`;
- `new(props)`;
- `props()`;
- `set_props()`;
- `props_changed()`;
- typed `child_with_props(...)` construction;
- prop equality checks;
- prop updates without remounting;
- child state preservation across prop updates.

Invariant:

```text
same component key + changed props
    !=
component replacement
```

## Milestone 16 — Child-to-parent communication / callbacks / shared channels

Implemented:

- typed `Callback<M>`;
- cloneable callback endpoints;
- shared framework-managed message queues;
- child-to-parent message delivery;
- parent message handling through the component tree;
- callback-driven parent rerenders;
- child state preservation during callback-driven updates.

This completes the initial two-way data-flow model:

```text
parent props -> child state -> callback/message -> parent state
```

## Milestone 17 — Async tasks + framework scheduler

Implemented:

- `Scheduler`;
- `TaskId`;
- `TaskHandle`;
- async task spawning;
- task completion queue;
- framework message delivery for task results;
- `SleepFuture` foundation;
- Windows native event-loop wake-up using a private `WM_APP` message;
- completion pumping on the event-loop thread;
- explicit task cancellation;
- ignoring completions targeting components that no longer exist.

Worker threads do not mutate component state directly.

## Milestone 18 — Structured task scopes

Implemented:

- persistent component-owned `TaskScope`;
- `ComponentContext::task_scope()`;
- `ComponentContext::spawn()` using the component scope;
- task ownership tied to managed component lifetime;
- task scope persistence across rerenders and prop updates;
- automatic cancellation when a component is removed;
- cancellation occurring before `unmounted()`;
- explicit `TaskHandle::cancel()` for early cancellation;
- completed task results ignored after component removal.

Current structured-task invariant:

```text
component rerender      -> task survives
props update            -> task survives
component unmount      -> outstanding tasks cancel
worker completion       -> result re-enters framework scheduler
```

## Milestone 19 — Effects + reactive invalidation

Implemented:

- keyed, dependency-aware effects declared from `ComponentContext`;
- effects committed after the declarative tree render;
- cleanup functions on dependency change, removal, and tree teardown;
- effect-owned task scopes, separate from the component-wide task scope;
- automatic cancellation of tasks started by obsolete effects;
- stable effects across rerenders when dependencies have not changed;
- duplicate effect-key detection within a component render.

Effect lifecycle:

```text
state / props change
      ↓
dependency change
      ↓
cleanup previous effect + cancel its tasks
      ↓
run replacement effect
```

## Milestone 20 — Resource and service system

Implemented:

- application-owned `Services` registry available from `ComponentContext`;
- typed asynchronous contracts for HTTP, storage, clipboard, file dialogs, and system operations;
- portable request/response and error types;
- deterministic `MemoryStorage` and `MemoryClipboard` implementations for tests and previews;
- explicit service injection through `Application::with_services`;
- service sharing across all window component roots.

## Milestone 21 — Theme + styling system

Implemented:

- theme colors, typography, spacing, and radius tokens;
- component defaults for labels, buttons, text inputs, and containers;
- node-level visual-style overrides;
- `disabled` as a first-class node flag (`Node::disabled`), realized natively
  as `EnableWindow` and excluded from Tab/Shift+Tab focus traversal;
- deterministic theme/override resolution;
- state-aware style variants for normal, hover, focus, pressed, and disabled
  controls, defined in the theme model;
- `TreeSnapshot::from_node_with_theme` resolves every node's style (theme
  default merged with override, in its `Normal`/`Disabled` state) before it
  reaches a backend — mirroring how layout geometry is already fully
  resolved in the core rather than left to each backend to compute;
- Windows realization: native fonts (`CreateFontIndirectW` + `WM_SETFONT`)
  and colors (`WM_CTLCOLORSTATIC`/`WM_CTLCOLOREDIT`/`WM_CTLCOLORBTN` for
  controls, `WM_ERASEBKGND` for containers), with GDI resources owned and
  freed per node;
- live Windows hover, pressed, and focus state repaints, using
  `TrackMouseEvent`/`WM_MOUSELEAVE` plus native focus synchronization, without
  rebuilding the declarative component tree.

The resolved style this milestone produces is the model 2.14 describes, and it
is what the second spelling compiles into: Milestone 58 adds the declaration
vocabulary and the utility classes above this resolution step, not beside it.
Nothing there gives a node a style this milestone cannot already resolve.

## Milestone 22 — Platform capability abstraction

Implemented:

- portable `Capability` enumeration (including `Menus`) and
  `PlatformCapabilities` discovery set;
- capability reporting through every platform adapter;
- Windows capability declarations for its available system integration
  surface, kept honest: a capability is only advertised once this backend
  actually realizes it (`capabilities_only_advertise_realized_backend_features`
  asserts this for every `Capability` variant);
- explicit `Platform::native_extension()` escape hatch for backend-specific APIs.

## Milestone 23 — Native dialogs, menus, and system integration

Implemented:

- portable async contracts for clipboard, notifications, URL launching, and file dialogs;
- file-dialog request model with open/save/folder selection and filters;
- system-service contract usable by native backends for notifications and URL launching;
- capability flags for file dialogs, system appearance, drag-and-drop, system sharing, and window management;
- a portable `MenuBar`/`MenuItem` model (actions, submenus, separators,
  enabled/checked state) attached to a `Window` via `Window::with_menu`, and
  `Event::MenuAction` for selection, routed like a window-lifecycle event to
  the window's root component;
- Windows realization: `WindowsFileDialogs` (open/save via
  `GetOpenFileNameW`/`GetSaveFileNameW`, folder picking via
  `SHBrowseForFolderW`, both COM-apartment-initialized per call since shell
  extensions are COM-backed), real notifications via `Shell_NotifyIconW`
  (add-then-immediately-delete a tray icon so a one-shot balloon leaves no
  permanent tray presence), and a native menu bar (`CreateMenu`/
  `CreatePopupMenu`/`AppendMenuW`/`SetMenu`, `WM_COMMAND` routed to
  `Event::MenuAction` via a per-window command-id → `NodeId` table).

Drag-and-drop, system sharing, and system-appearance-change notifications
remain portable contracts/flags only — not realized, and correspondingly not
advertised as supported capabilities — preserving the core's platform
independence for the parts still deferred. A menu's contents are static at
window-creation time, matching a `Window`'s title and size, which are
likewise not yet reactively updatable after the window opens.

## Milestone 24 — Window lifecycle + multi-window support

Implemented:

- stable `WindowId` identities and a primary-window convention;
- multiple independently owned window/component roots;
- open/close operations and optional modal-parent relationship;
- per-window view, render, dispatch, task, service, and theme ownership;
- portable resize, move, close-request, and presentation-change events;
- window state for position, size, visibility, normal/minimized/maximized/fullscreen presentation;
- opening and closing windows at **runtime**, from inside a running
  component, not just before the platform's event loop starts:
  `ComponentContext::windows()` returns a `WindowRequests` handle
  (`.open(component, window, modal_parent)` / `.close(id)`) that queues a
  deferred `WindowCommand`, applied by `Application` right after the
  requesting window's own dispatch/task-pump finishes — so a component never
  mutates the window registry while it is itself mid-render;
- Windows realization: a `WindowRegistry` that syncs live native windows
  against `Application::window_ids()` after every dispatch and task pump,
  creating a new `HWND` for a window the application gained and — for one
  the application lost — posting that window itself a `WM_CLOSE` rather than
  destroying it inline. The deferral matters: a `Runtime` can be mid-dispatch
  (and so borrowed by a caller further up the native call stack) at the
  exact moment its own window is asked to close, and tearing it down inline
  would free memory that caller still holds a reference to. Every `Runtime`
  — closed or not — is kept alive until the whole native event loop returns,
  so this is sound even when a window closes itself.

## Milestone 25 — Advanced input system

Implemented:

- portable payloads in `rustnative-core::input`: `PointerEvent` (mouse,
  touch, pen; per-contact ids; buttons; modifiers; pressure; monotonic
  timestamps), `WheelDelta` (1/120-notch lines, or pixels), `Gesture`,
  `Composition`, `ClipboardAction`, `DragData`/`DropEffect`, and gamepad
  `GamepadState`/`GamepadInput`, plus `KeyUp`, navigation/function keys, and
  the meta modifier;
- **opt-in delivery**: a node declares `InputInterest` (pointer, wheel,
  gestures, drop target, gamepad) and only interested nodes — found by
  walking up from the node under the pointer — receive those streams, so a
  moving mouse is never a rerender storm;
- portable **gesture recognition** (`GestureRecognizer`: tap, long press,
  pan, two-finger pinch) driven from the pointer stream with an injected
  clock, and portable **gamepad diffing** (`GamepadPoller` over a
  `GamepadSource`) — logic that exists once for every future backend;
- deferred **input requests** (`ComponentContext::input()` →
  `InputRequests`): pointer capture/release and drag-feedback answers,
  scoped to framework-wide node ids and applied by the backend after the
  dispatch that made them;
- Windows realization: mouse (all five buttons, hover enter/leave,
  horizontal wheel), touch and pen through `WM_POINTER*` with implicit
  per-contact capture (touch-promoted mouse messages recognized and
  skipped), mouse capture on the top-level window with lost capture
  delivered as `PointerCancel`, IMM32 composition for focusable custom
  containers, Ctrl/Shift clipboard shortcuts plus `WM_CLIPBOARDUPDATE`,
  an OLE `IDropTarget` per window (files and text; source-allowed effects
  negotiated), and `XInput` controllers polled only while a node asks and
  delivered only to the active window;
- `Capability::{DragAndDrop, Touch, Pen, Gamepad, Ime}` advertised.

Deliberately not done: mouse input is not moved into the pointer stack
(`EnableMouseInPointer` is process-global and changes standard-control
behavior); Windows' own `WM_GESTURE` is not used (it is mutually exclusive
with `WM_POINTER`).

## Milestone 26 — Full accessibility bridge

Implemented:

- a portable model in `rustnative-core::accessibility`: 29 roles (including
  headings with levels), text and range values, checked/expanded/selected/
  read-only/required/busy states, declared actions (`AccessibleActionKind`)
  and invoked ones (`AccessibleAction`, delivered as
  `Event::AccessibilityAction`), labelled-by/described-by/controls
  relationships scoped per component like node keys, live regions,
  position-in-set for virtualized children, stable automation ids, and
  **virtual elements** — semantic children with no native object of their
  own, for custom-drawn content;
- `AccessibilityTree`, the portable projection every backend needs:
  role-less structure flattened away, relationships resolved to nodes that
  exist, and accessible names computed (explicit name, then the labelling
  node, then visible text) with label cycles terminated;
- Windows: real UI Automation server-side providers for every realized node
  (containers answer `WM_GETOBJECT` directly; `BUTTON`/`EDIT`/`STATIC` are
  subclassed), merged with each control's native proxy so only what the
  model states is overridden; fragment roots and fragments for virtual
  elements; Invoke, Value, RangeValue, Toggle, ExpandCollapse,
  SelectionItem, and ScrollItem patterns that turn into component events;
  property-changed, structure-changed, and live-region events raised from a
  posted message, never inside a runtime borrow; providers disconnected
  when their node or element goes away. The MSAA annotations from the
  standards-audit pass remain, for MSAA-only clients.

Planned platform targets not built here consume the same portable model when
their backends exist: macOS, Linux, Android, and iOS through their own
accessibility APIs (Milestones 33–36), and the Web through HTML/ARIA (Web
milestone D). The terminal is the exception the capability model exists for —
accessibility there belongs to the terminal, and Milestone 38's backend says
so rather than advertising a bridge it cannot provide.

---

## Milestone 27 — Animations and transitions

Implemented:

- a portable animation model in `rustnative_core::animation`: animatable
  properties (position, size, translation, opacity, background, foreground),
  typed animated values, `Transition` (a duration and easing curve, or a real
  mass/stiffness/damping spring, either with a start delay), `Animation`
  (explicit from/to, repeat counts, autoreverse, fill mode) and `Timeline`,
  the platform-free evaluator that turns elapsed time into per-property
  frames;
- **transitions**, declared on a node (`Node::with_transition`): when a
  rendered value changes, the backend animates from the previous value to the
  new one instead of jumping, then releases the property back to the tree;
- **interruption**: retargeting a running animation continues from the value
  and velocity it had, so a reversed drag or a second click bends the motion
  rather than restarting it;
- **cancellation**: per node and property, per owning component (a component
  that unmounts cancels its own animations), and per window;
- **frame scheduling**: one process-wide driver thread paced by `DwmFlush`,
  posting — never sending — a private frame message, coalesced per window, and
  asleep on a condition variable whenever nothing animates;
- **reduced-motion preference support**: the system preference
  (`SPI_GETCLIENTAREAANIMATION`) read at startup and tracked through
  `WM_SETTINGCHANGE`; each animation declares whether it is skipped or still
  run when motion is reduced;
- **declarative animation state**: nothing about a frame reaches the
  component tree — `Event::AnimationFinished` is the only thing a component
  hears, and it may ignore it.

Frames do not rerender. A frame applies the changed property to the native
object alone (`SetWindowPos` for geometry, a layered-window alpha for
opacity, an invalidation for colours); the tests assert the component's
render count does not move while values animate.

---

## Milestone 28 — Virtualized lists and large data sets

Implemented, reusing identity, layout, scrolling, and components rather than
a second list runtime — a virtual list is an ordinary scrollable column or
row (`Node::virtual_list`) that also declares how many items it logically
has:

- **visible-range calculation**: `VirtualRange::compute` from the scroll
  offset, viewport, per-item extents, and overscan (counted in items);
  `Event::VisibleRangeChanged` reaches the component only when the range
  changes, so scrolling inside a range renders nothing;
- **recycling/reuse**: on Windows, a render that removes rows of a virtual
  list and inserts others parks the removed `HWND`s and realizes the new rows
  on them (`rendering::pool`), whole item subtrees included; anything not
  reused is destroyed before the render ends;
- **stable item keys**: rows keep ordinary node keys, so a row that stays in
  range keeps its native window; `Node::with_item_index` says which item a
  row realizes, independent of its position among realized siblings;
- **incremental measurement**: `ItemExtent::Estimated` lists measure the rows
  they realize, report them as `LayoutResult::measured_items`, and record
  them in an `ExtentCache` (a Fenwick tree: `O(log n)` offsets and lookups);
  the backend lays out once more when a measurement moved an offset, then
  settles;
- **scroll position preservation**: `ScrollAnchor` holds the item at the top
  of the viewport across a render, so items inserted or removed above it do
  not move what is on screen;
- **efficient insertion/removal/reordering**: the existing keyed diff; only
  rows that entered or left the range are inserted or removed.

Virtual-list items announce their position in the whole list to assistive
technology ("5,012 of 100,000") automatically.

---

## Milestone 29 — Graphics / custom rendering escape hatch

Implemented as two explicit, isolated paths; ordinary UI is still native
controls:

- **custom canvas**: `Node::canvas(key, DrawList, layout)`. A `DrawList` is a
  retained, portable display list — fills, strokes, rounded rectangles,
  ellipses, lines, paths (lines, quadratic and cubic Béziers), text,
  RGBA images, and nested transform / clip / opacity scopes — held in the
  node tree and diffed like any node (cheap clones, pointer-equality fast
  path). On Windows a canvas is its own child window painted by Direct2D
  (DirectWrite for text); clips and opacity are Direct2D layers, so a
  rotated clip clips exactly; a lost device (`D2DERR_RECREATE_TARGET`) is
  recovered on the next paint from the draw list;
- **custom drawing regions**: `DrawList::hit_region(id, rect)` declares
  regions tested under the transforms and clips in force where they were
  declared; pointer input on a canvas carries the region it landed in
  (`PointerEvent::region`);
- **GPU surface access**: `Node::native_surface(key, layout)` is a bare child
  window the framework positions and sizes and never paints.
  `Event::SurfaceResized { surface, size, scale_factor }` reports its size;
  `rustnative_windows::native_surface(surface)` returns a `SurfaceHandle`
  implementing `raw-window-handle` 0.6's `HasWindowHandle`/`HasDisplayHandle`,
  which wgpu, ash, and glutin accept directly;
- **integration with platform compositors**: both paths are real child
  windows composed by DWM like every other control.

Not built: shader-backed drawing inside a `DrawList`. An application that
needs shaders owns a native surface and brings its own GPU API, which is
what that path is for.

---

## Milestone 30 — Persistence and navigation

Implemented on the managed component tree, not beside it:

- **navigation stacks**: `NavigationStack<R>` is data in the showing
  component's state; each entry's screen is that component's keyed child
  (`EntryId::key`), and every entry is rendered with all but the top one
  `Node::hidden`, so pushing never remounts the screens below and popping
  finds them as they were. Screens navigate with a `Navigator`, which sends
  `NavigationCommand`s through an ordinary child-to-parent `Callback`;
- **routes**: `Route` patterns (`/users/:id`, trailing `*rest`) with
  percent-decoding, typed parameters, and `build` for the reverse; a
  `Router` of named routes;
- **deep links**: `Event::DeepLink` to the primary root, `Application::open_url`,
  and on Windows the launch URL from the command line plus single-instance
  handoff (`WindowsPlatform::with_app_id`): a second launch forwards its
  URL through `WM_COPYDATA` to the running instance and exits;
- **state restoration / persistent state**: `StateStore` (sync; `MemoryStateStore`
  in core, crash-safe `FileStateStore` on Windows), set on `Services` so it
  is present from the first render; `ComponentContext::persisted` keyed by
  the component's key path, which is stable across runs; navigation stacks
  are `Serialize` and restore the same way; the primary window's placement
  is saved on close and restored on open;
- **lifecycle-aware storage**: writes are buffered and flushed after 1.5 s
  of quiet, before `Lifecycle::Suspending` / `Terminating` are delivered
  (`WM_POWERBROADCAST`, `WM_QUERYENDSESSION`), and when the last window
  closes;
- **tab/navigation containers**: `Node::tab_bar` realized as the system
  `SysTabControl32` (`Event::TabSelected`, controlled selection), with every
  tab's content kept mounted and hidden unless selected; `Node::hidden`
  removes a subtree from layout, focus traversal, and the accessibility
  tree while keeping its native objects and state.

---

# 6. Rendering and interaction milestones

All three (Milestones 27–29) are complete; see section 3.

## Milestone 31 — Developer CLI and project tooling

Implemented as `crates/rustnative` (package `rustnative-cli`, binary `rustnative`), which
orchestrates the native toolchains rather than replacing them:

```text
rustnative new <name> [--path DIR] [--framework-path DIR]
rustnative build <platform> [--release]
rustnative run   <platform> [--release]
rustnative check [platform]
rustnative test  [cargo test arguments...]
rustnative doctor [--json]
```

- **project creation**: `rustnative new` writes a project that compiles — `src/main.rs`,
  `Cargo.toml`, `rustnative.toml`, `README.md`, `.gitignore` — depending either on
  published framework versions or, with `--framework-path`, on a checkout;
- **project metadata**: `rustnative.toml` holds the application's identity (the same
  id its saved state, single-instance mutex, and package use), its display
  name, version, publisher, description, icon, and URL schemes. Every
  validation failure names the field it is about, and an unknown field is
  refused rather than ignored;
- **platform commands**: every platform on the roadmap is recognized; the
  ones without a backend fail with "no backend for `<platform>` yet — see
  PLAN.md, Milestone N" and exit code 3, never a silent build for Windows.
  Exit codes are part of the interface: 2 usage, 3 no backend, 4 a missing
  toolchain, 1 everything else;
- **`rustnative doctor`**: rustc against the MSRV, Cargo, git, the MSVC build tools
  (through `vswhere`), the newest installed Windows SDK and its `rc`, `mt`,
  `makeappx`, and `signtool`, plus per-platform readiness — as a table or,
  with `--json`, for a script.

The toolchains it drives, and the ones it will: Windows uses Cargo with the
MSVC build tools and the Windows SDK; macOS and iOS will use Xcode and the
Apple SDKs, Linux the system compiler, Android Gradle with the SDK and NDK,
the Web Cargo for the server plus the framework's client JavaScript emitter and
bundling (and, for its serverless mode, the target its host runtime expects),
the terminal Cargo alone, and embedded targets their own toolchains through
Cargo.

---

# 7. Application architecture milestones

## Milestone 32 — Packaging and deployment (Windows)

Implemented for the platform that has a backend; the other formats
(macOS bundles, Linux packages, APK/AAB, iOS bundles, Web bundles and their
three deployment modes, plain terminal binaries, firmware images) belong to
their backends' milestones.

- **Resource bundling and manifests**: a new `rustnative-build` crate, run
  from an application's `build.rs` (the `rustnative new` template wires it, and the
  example uses it). It reads `rustnative.toml` and produces the executable's icon
  (a `.png` wrapped into a PNG-compressed `.ico`, so no image library is
  needed), its `VERSIONINFO`, and its application manifest — per-monitor V2
  DPI awareness, Common Controls v6, the `supportedOS` entries layered
  child windows need, UTF-8 as the active code page, and long-path
  awareness — compiled with the SDK's `rc.exe` and linked in. Without the
  SDK the build carries on with a warning rather than failing;
- **Windows executable packaging**: `rustnative package windows --format zip|msix|all`.
  The portable zip is reproducible (sorted entries, fixed timestamps and
  attributes, stored rather than compressed) and carries a `SHA256SUMS`;
- **Installer packaging**: an MSIX — a generated `AppxManifest.xml` whose
  identity, publisher, version, logos, and `uap:Protocol` entries come from
  the same `rustnative.toml` the application's own identity does, laid out and
  packed with `makeappx`;
- **Signing**: `--sign <pfx> [--password-env VAR]` runs `signtool sign /fd
  SHA256`. The password is read from the named environment variable rather
  than the command line; `rustnative` checks the publisher is an X.500 name before
  building, and `signtool` makes the real comparison against the
  certificate's subject.

---

# 8. Platform backends

The Windows backend is the first production-oriented backend. New backends should be added only after the core contracts are stable enough to avoid duplicating accidental Windows assumptions. "First" is an order, not a ranking: every backend below is specified to the same depth and finished by the same definition.

A backend is complete when, in its own host's terms, it realizes:

- native host objects with stable identity and reuse;
- native text measurement behind the portable `IntrinsicMeasurer` contract;
- native input — keyboard, pointer or touch, focus, and IME where the host has one;
- the portable accessibility model (Milestone 26) through the host's accessibility API, or an honest statement that the host has none;
- the service contracts, advertising only the capabilities it genuinely realizes;
- its toolchain and packaging in `rustnative`;
- its own tests, run against the real host.

Nothing in this section concerns the authoring syntax. Both spellings of the
tree (2.9) lower to the same `Node` before a backend is reached, so no backend
implements either one, no backend can behave differently under one of them, and
a backend author never encounters the markup syntax while writing a backend.
What a backend does owe is documentation: its own examples are written in both,
like everything else.

Styling is the same in one direction and different in the other. Both style
spellings (2.14) resolve before a backend is reached, so a backend never sees a
class string either — but style, unlike syntax, does reach the host, so every
backend owes two things about it: its per-property capability answer, and its
unit mapping with the rounding rule (Milestone 58). Those belong in the list
above, beside its measurement and accessibility obligations.

Section 11 adds the gates that apply to every backend from the second one
onward, and they are part of this definition rather than separate work:

- it passes the conformance suites of Milestone 41 — fidelity against the
  host's own first-party applications, text (complex scripts, bidirectional
  ordering, clusters, fallback, breaking, caret geometry), layout under text
  scaling and pseudo-localization, accessibility assertions plus a recorded
  screen-reader pass, modal-operation behaviour, and host-object lifetime with
  no leaks;
- it meets its declared budgets from Milestone 42, enforced in CI;
- it answers the inspection protocol of Milestone 44, in reduced form where the
  host cannot carry the full one;
- it supports the embedding directions of Milestone 40 that its host permits —
  our tree inside a foreign host object, and a foreign host object inside our
  tree;
- it implements the portable-surface obligations of Milestone 39 — mirroring,
  safe areas, permission states, gesture arbitration, panic and teardown — or
  answers each as an honest capability;
- it runs the developer loop of Milestone 43 on its own host, on-device where
  the host is a device;
- it passes Milestone 61's interaction conformance — scroll coordination,
  system insets and keyboard avoidance, gesture physics, the host's text
  editing surface, and non-pointer navigation — because a backend without them
  is not finished whatever else it realizes;
- it releases through Milestone 59's toolchain layer — a named build profile, a
  credential it does not require the developer to hold, submission where the
  host has a store, and updates gated by the native-input fingerprint — rather
  than a release process invented for that backend.

Two of these — macOS and iOS — cannot be verified on the project's current hardware (2.13). They remain fully planned, and each is finished when it runs on an Apple machine, not before.

## Milestone 33 — macOS backend

Native AppKit interoperability through Rust Objective-C bindings, with the unsafe surface isolated the way `rustnative-windows` isolates Win32:

- `NSWindow` per window root, an `NSView` hierarchy for containers, and AppKit controls (`NSButton`, `NSTextField`, `NSScrollView`/`NSClipView` for scrolling) as the realized objects;
- Core Text measurement behind `IntrinsicMeasurer`, including bounded and wrapped measurement;
- the responder chain translated into the portable event model: keys and modifiers, mouse and trackpad, momentum scrolling, `NSTextInputClient` for IME, pressure input, and `NSPasteboard` drag-and-drop;
- accessibility through the `NSAccessibility` protocols, driven by the portable `AccessibilityTree`, with virtual elements as accessibility elements and posted notifications for property, structure, and live-region changes;
- system integration: the macOS menu bar and its application-menu conventions, `NSOpenPanel`/`NSSavePanel`, user notifications, `NSWorkspace` URL launching, clipboard, appearance/dark-mode tracking, and the reduce-motion display setting;
- animation frames paced by `CADisplayLink`, evaluated by the same portable `Timeline`;
- run-loop integration for scheduler wake-ups on the main thread;
- packaging: Xcode toolchain, `.app` bundle and `Info.plist` generated from `rustnative.toml`, code signing, notarization, and a distributable image;
- sandboxing and entitlements expressed as capability answers rather than build flags the application maintains by hand, and window and state restoration driven through the host's own restoration mechanism (Milestone 39);
- embedding in both directions: our tree realized into a caller-supplied `NSView`, and an `NSView` adopted as a leaf of our tree, laid out and clipped by our layout model (Milestone 40);
- locale formatting, collation, and right-to-left mirroring delegated to the host's own facilities rather than reimplemented (Milestone 46);
- the host's document architecture — autosave, versions, recent documents, per-document undo — behind the portable document model (Milestone 48), and menu-bar extras, dock menus, and widgets from the surface vocabulary (Milestone 57);
- verification requires a macOS machine (2.13).

## Milestone 34 — Linux backend

Start with one supported native toolkit and keep the backend pluggable so additional Linux native backends can be added later. Potential backend families include GTK or another native toolkit, depending on final architectural decisions.

- native windows, containers, and controls from the chosen toolkit, with the toolkit behind an internal seam so a second one can be added without touching the core;
- Pango (or the toolkit's equivalent) text measurement;
- toolkit/GDK input translated into the portable model: keys through XKB, pointer, touch and pen, scrolling, and IME through the platform input method;
- both display servers, with their differences reported rather than hidden — client-side decorations, the absence of a global menu bar, and Wayland's restrictions on window placement become capability answers;
- accessibility through AT-SPI2, driven by the portable model;
- system services through the desktop portals where they exist: file dialogs, notifications, URL launching, and clipboard;
- animation frames from the toolkit's frame clock;
- packaging: the system compiler, a desktop entry and icon theme, and at least one distribution format (AppImage, Flatpak, or a native package);
- a desktop-environment conformance matrix — the conventions that differ between environments are answered per environment rather than assumed — and mixed-DPI multi-monitor scaling that survives a monitor configuration change at runtime (Milestone 39);
- embedding in both directions against the chosen toolkit's widget type (Milestone 40);
- locale formatting, collation, and mirroring delegated to the host's own facilities (Milestone 46);
- tray integration through the desktop's status-notifier protocol where the environment provides one, answered as a capability where it does not (Milestone 57);
- verification on real Linux sessions under both display servers.

Implemented (`rustnative-linux`; guide: `docs/linux.md`):

- **GTK 4** behind an internal toolkit seam (`toolkit::Toolkit`; `ToolkitKind::Gtk4`), with the toolkit-independent half (`desktop`: session and display-server detection, the freedesktop services over GDBus) reusable by a second toolkit;
- every node kind realized as a GTK widget, laid out by the portable engine in a custom container (`RnLayout`), measured through GTK and Pango on prototype widgets with the answers cached until fonts or theme change; styles compiled to one display-wide style sheet, applied only where a node differs from the desktop's theme (`rustnative_style::LINUX`, `LINUX_UNITS`);
- input from GTK's controllers: keys (XKB keyvals, unshifted for shortcuts), pointer, touch, pen, wheel, gestures with scroll-versus-pan arbitration, hover, cursors, IME through `GtkIMMulticontext`, drag-and-drop through `GtkDropTarget`, command shortcuts in the capture phase;
- both display servers, answered per session: `WindowPlacement` on X11, `ServerSideDecorations` on X11 and Plasma's Wayland, no `GlobalMenuBar` (GTK 4 exports only an application-wide menu); the conventions of each desktop in `docs/linux/desktop-matrix.md`;
- accessibility through GTK's `GtkAccessible`, verified over AT-SPI2 itself (`docs/linux/accessibility.md`), with a canvas' virtual elements as accessible objects;
- services: clipboard, file dialogs (the FileChooser portal or GTK's chooser), URL launch (OpenURI portal or GIO), notifications (server or portal, with clicks), crash-safe state files under `$XDG_STATE_HOME`, glibc locale formatting, Secret Service storage, libsoup HTTP with certificate pins checked at the handshake, GTK printing, termios serial ports, portal-gated permissions in a sandbox, gdk-pixbuf decoding and GIO/UPower conditions, and push/billing answered as unavailable with the reason;
- animation from GTK's frame clock; virtual lists with anchoring and widget recycling; canvas drawing with cairo and Pango; native surfaces as Wayland subsurfaces or X11 child windows, told their scale when it changes (mixed DPI); host content (`GtkVideo`);
- menu bars (`GMenu` and `GtkPopoverMenuBar`, command-bound items following their commands), lifecycle (logind sleep and shutdown, GIO's memory monitor, idle flush), window placement restored, one instance per application id over `org.freedesktop.Application` with launch URLs handed on, a StatusNotifierItem tray where a tray host runs, and `LauncherEntry` progress;
- inspection (realized widgets, the census, the overlay), per-property mappers, the preview catalogue, and embedding both ways: `start_external` and `embed` into a GTK host, `register_foreign` adopting the host's widgets (`examples/adoption-gtk`);
- tooling: `rustnative build|run|package linux` (a `.deb`, a tarball, and an AppImage through `appimagetool`, each with the desktop entry, AppStream metadata, and icon), a `doctor` row, a template that runs on Windows and Linux, and `budgets/linux.toml`;
- verified on WSLg's Wayland and Xwayland sessions and on Xvfb with XTest input, on Ubuntu 24.04 (GTK 4.14) and on Kali Rolling (GTK 4.22), and in CI's `linux-backend` job.

## Milestone 35 — Android backend

Native Android view hierarchy, lifecycle integration, and a disciplined JNI boundary:

- an `Activity` hosting a native `View`/`ViewGroup` hierarchy and platform widgets as the realized objects;
- JNI/FFI confined to one ownership module, with explicit global-reference and thread-attachment rules, mirroring how the Windows backend confines raw handles;
- `Paint`/`StaticLayout` text measurement;
- input: `MotionEvent` translated to the portable pointer model, the soft keyboard and `InputMethodManager` for IME, hardware keys, and system back/predictive back mapped onto the navigation model from Milestone 30;
- the activity and process lifecycle mapped onto the existing lifecycle and state-restoration contracts, including configuration changes and process death;
- accessibility through `AccessibilityNodeInfo`, with virtual elements as a virtual view hierarchy, verified with TalkBack;
- system services: scoped storage, clipboard, notification channels, the share sheet, and the runtime permission model expressed as capability answers;
- scheduler wake-ups through the main `Looper`;
- packaging: Gradle with the SDK and NDK, a manifest generated from `rustnative.toml`, signing, and APK/AAB output;
- the runtime permission model expressed as the portable permission states of Milestone 39 — not-asked, granted, limited, denied, permanently-denied — with the host's own request flow behind them;
- safe areas, display cutouts, foldable hinges, split-screen, and configuration changes handled as layout-model properties (Milestone 39);
- constrained background work, and asset loading with decode, caching, and lifetime-bound cancellation (Milestone 47);
- embedding in both directions against `View`/`ViewGroup`, plus the library-only mode in which the application model is linked into an existing Android application with no UI dependency (Milestone 40);
- over-the-air update support within the host's rules, with signing, staged rollout, rollback, and version pinning (Milestone 50), fingerprint-gated channels and an embedded fallback payload (Milestone 59), and store-delivered dynamic feature and asset packs (Milestone 50);
- its release path — build profile, managed credential, store submission, update channel, generated launch assets, and device pairing for the development loop — taken from Milestone 59 rather than invented here;
- home-screen widgets, quick-settings tiles, share targets, remote push, store billing, and keystore-backed secure storage from the surface and product-service contracts (Milestone 57);
- verification on an emulator and on a physical device, including the lifecycle conformance suite of Milestone 45 — process death and restoration, configuration change, deep-link entry during restoration, and low-memory trim — and a low-end device profile in the budget matrix (Milestone 42).

## Milestone 36 — iOS backend

Native UIKit interoperability, sharing the Objective-C interop and Core Text work with Milestone 33 wherever the two platforms genuinely agree, and not pretending they agree where they do not:

- `UIWindow`/`UIViewController` roots, a `UIView` hierarchy, UIKit controls, and `UIScrollView` for scrolling;
- the scene and application lifecycle mapped onto the portable lifecycle and state-restoration contracts;
- input: touch through the portable pointer model, `UIGestureRecognizer` coexisting with the framework's own recognizers rather than duplicating them, `UITextInput` for IME, hardware keyboards, and pencil pressure;
- Core Text measurement;
- accessibility through `UIAccessibility`, with virtual elements as custom accessibility elements, verified with VoiceOver;
- system services: the share sheet, document picker, notifications, clipboard, and URL schemes and universal links feeding the existing deep-link model;
- multiple windows treated as an iPadOS capability, not an assumption — `Capability::MultipleWindows` answers it;
- packaging: Xcode, `Info.plist` and entitlements from `rustnative.toml`, code signing, IPA output, and TestFlight distribution;
- the runtime permission model expressed as the portable permission states of Milestone 39, including limited grants, with the host's own request flow behind them;
- safe areas, display cutouts, split-screen and external displays, and dynamic type as layout-model properties (Milestone 39);
- embedding in both directions against `UIView`, plus the library-only mode for linking the application model into an existing iOS application (Milestone 40);
- over-the-air update support within the host's rules — which forbid more here than elsewhere, so the milestone states what is permitted rather than assuming parity with other targets (Milestone 50), fingerprint-gated (Milestone 59);
- its release path — build profile, managed credential, store submission, update channel, generated launch assets, and device pairing — taken from Milestone 59 rather than invented here;
- privacy manifests and data-use declarations generated from the build rather than hand-maintained (Milestone 51);
- widgets, live activities, share and action extensions, remote push, store billing, and keychain-backed secure storage from the surface and product-service contracts (Milestone 57), each extension a separate target generated from `rustnative.toml` (Milestone 50);
- verification requires Apple hardware and a developer account (2.13), and includes the lifecycle conformance suite of Milestone 45.

## Milestone 37 — Embedded backends

Separate embedded platform profiles from desktop/mobile assumptions.

Initial targets should distinguish:

- embedded Linux;
- RTOS-backed targets;
- selected bare-metal systems.

Capability-oriented design is critical here because embedded targets will not implement desktop concepts such as windows or accessibility.

- realization goes through the draw-list path from Milestone 29 rather than native controls, because these hosts have none: a display driver consumes a `DrawList`, and the same portable model produces it;
- embedded Linux may instead reuse the Milestone 34 toolkit backend where a full graphical session exists; the profiles differ, and the plan keeps them named separately for that reason;
- input arrives as buttons, rotary encoders, touch panels, or a serial console, and is mapped onto the portable key and pointer model;
- a portable-core subset must be defined before the RTOS and bare-metal profiles start: `rustnative-core` currently requires `std` and an `Executor` whose tasks are `Send`, so the identity, node, reconcile, and layout layers need a `no_std`-capable profile and a single-threaded executor. That split is core work, stated here so it is not discovered during a backend;
- resource discipline: bounded allocation, no thread pool, and a frame budget the device can actually meet;
- packaging: firmware or image output through each target's own toolchain, driven by `rustnative`, including a consumable recipe or package for at least one embedded Linux build system so our application is integrated the way anything else on the device is (Milestone 50);
- **guest integration rather than ownership**: the runtime must be drivable from an existing vendor project's `main` and initialization, owning neither startup, clocks, interrupts, nor the loop, and the `Executor` contract must be implementable by an existing embedded async executor rather than replacing it (Milestone 40, Milestone 52);
- peripheral access, HAL traits, and probe tooling consumed from the existing ecosystem rather than reimplemented — a stated non-duplication policy, because the cost of splitting a small ecosystem exceeds anything a parallel stack would gain (Milestone 52);
- damage-tracked partial redraw on the draw-list path, with dirty-region coalescing shared with the terminal backend's cell damage tracking rather than implemented twice (Milestone 44's overlay and Milestone 41's conformance both depend on it);
- power, watchdog, and allocation obligations: no periodic wake without a pending deadline, documented wake sources and a measured idle current figure on a reference board, watchdog integration with a safe-state panic path, and a bounded-allocation mode in which allocation failure is a handled outcome rather than an abort (Milestone 51);
- firmware update as a first-class capability: signed images, A/B slots, verification, automatic rollback on failed boot, and staged fleet rollout, delegating to an existing bootloader rather than writing one (Milestone 50);
- a constrained text profile that declares which scripts it supports, so the limitation is stated rather than discovered (Milestone 48);
- framework display, input, and storage drivers that take ownership of the ecosystem's typed peripherals rather than raw register handles, so hardware typestate guarantees survive into our layer (`C74-1`); a display driver contract with asynchronous region transfer, so DMA- or programmable-I/O-backed drivers overlap transfer with the next frame's work (`C91-1`); and, for embedded Linux, a direct-to-display profile on the draw-list path with no window system, for appliance-like boot-to-UI times (`C83-1`);
- declared RAM, flash, and frame budgets per device class, enforced in CI (Milestone 42);
- verification on device, plus a host-side simulator — the same draw list rendered on a development machine with simulated display size, colour depth, and input — so most of the logic is testable without hardware (Milestone 45).

## Milestone 38 — Terminal (TUI) backend

A `rustnative-tui` backend realizing the same application model onto a terminal. In scope: Windows, macOS, and Linux desktop terminals, and embedded Linux consoles — local, over SSH, or on a serial line. Deliberately out of scope: Android, iOS, and the browser. A terminal emulator running inside those is the emulator's application, not a platform target of this framework.

- **the terminal is the host**: the Windows console API in virtual-terminal mode, and `termios` plus VT sequences on Unix. Alternate screen, cursor control, the terminal's own colour depth (16/256/true colour, detected rather than assumed), text attributes, bracketed paste, focus reporting, and resize notification (`SIGWINCH` or console events);
- **drawn, not native**: this is the one target where 2.2's native host object is the terminal's own cell grid. Drawing goes through the existing draw-list path quantized to cells, so the framework does not gain a second rendering runtime; a "control" is a drawn widget with the same portable semantics, identity, and events as everywhere else;
- **geometry in cells**: the conversion happens at the backend boundary (2.11), and measurement is Unicode display width — grapheme clusters, wide East Asian characters, combining marks, and emoji presentation — never byte or character counts;
- **input**: keys with modifiers and function keys (including terminals that cannot report some combinations, which is a capability answer, not a bug), paste, and mouse click/drag/wheel through the SGR protocol where the terminal reports it. No pen, no touch, no gamepad;
- **focus and traversal** reuse the portable focus model unchanged, including Tab/Shift+Tab and `disabled`;
- **accessibility belongs to the terminal**: the backend's obligation is a readable, correctly ordered screen and honest capability reporting, not a bridge it cannot provide. Where a terminal exposes an announcement mechanism, live regions from the portable model map onto it;
- **capabilities, advertised honestly**: no `MultipleWindows`, no native `Menus` or `FileDialogs`, `Clipboard` only where OSC 52 is available, `Notifications` only where the terminal implements them, no `SystemAppearance`, no `Touch`/`Pen`/`Gamepad`;
- **style, at cell granularity**: colour is the terminal's own depth and text attributes; corner radius, shadows, gradients, and transforms are declared unrealizable rather than approximated (2.14). Lengths convert with this backend's documented pixel-per-cell ratio per axis, a nonzero spacing never rounding to nothing, and the conversion table is a conformance case (Milestone 58) rather than a constant chosen in one place;
- **scheduling and redraw**: a single-threaded loop with input and redraw decoupled, damage tracking so only changed cells are written, coalesced frames, and a full redraw on resize. Animations run through the same `Timeline`, paced to a sane terminal frame rate, and respect a reduced-motion setting;
- **terminal restoration is a correctness requirement**: raw mode, the alternate screen, and the cursor are restored on exit, on signal, and on panic. A crashed application must not leave a terminal unusable;
- **tooling**: `rustnative run tui` and `rustnative build tui` — a new platform value in the CLI, built with Cargo alone — plus a deterministic harness that drives a synthetic terminal of a given size and asserts the resulting cell grid, so most of the backend is testable without a TTY;
- **section 11's gates in reduced form**: the inspection protocol answered over a side channel rather than a second screen, since the terminal is already the output surface (Milestone 44); the cell-damage tracking above shared with the embedded draw-list path rather than duplicated (Milestone 37); locale-aware display width and mirroring from the localization work (Milestone 46); and the synthetic-terminal harness counted as one of the headless test backends (Milestone 45);
- **why it belongs in this plan at all**: same components, same state, same layout, same identity, one small capability set. A target this constrained is the strongest test that the capability model is real rather than decorative.

## Web backend — Web milestones A–K

The Web track is lettered rather than numbered because it predates the numbering and because `rustnative` names no milestone for `web`. It is one backend like the others; it is longer because the browser is not one deployment target but three.

### Web deployment modes

The same application, the same components, and the same tree must be deployable in every mode a web application is actually written in, chosen at deployment time rather than by rewriting:

```text
client-side      pages are rendered at build time; the host serves static HTML, CSS,
                 and generated JavaScript, and server calls go to a separate backend
server-rendered  a long-lived Rust server renders HTML per request; the browser runs
                 the generated JavaScript against it
serverless       the same render runs per request in a function or edge runtime,
                 with nothing kept between requests
```

In every mode the division is the same: all application code that runs on a server is Rust, and the browser receives HTML, CSS, and JavaScript generated at compile time, plus a WebAssembly module for each subtree that opts into one. The two sides communicate only through serializable data.

Which milestones each mode needs:

```text
client-side      A B C D E F G I J
server-rendered  the above, plus H
serverless       the above, plus H and K
```

An application that renders identically in all three is the test that the modes are genuinely one target (see Web milestone K).

### Web milestone A — Browser host and client code generation

Build a dedicated `rustnative-web` adapter that:

- splits an application's logic into **client logic**, which runs in the browser, and **server logic**, which is Rust on the server;
- defines client logic as a restricted subset of Rust — plain serializable state (numbers, strings, booleans, lists, plain structs), assignment, arithmetic, `if`/`match`, string formatting, showing and hiding, class toggles, input binding, list insertion and removal, and navigation — which covers toggles, tabs, counters, form validation, filtering, dialogs, and optimistic updates;
- translates client logic into JavaScript at compile time through a procedural macro, once per build rather than per request, so the emitted code is a cacheable static asset and a strict Content Security Policy needs no per-request nonces;
- reports code outside the subset as a compile error that names the three ways forward: mark it `#[server]`, which makes it an explicit server call; mark the subtree as a WebAssembly subtree, which runs it as full Rust in the browser; or supply hand-written JavaScript through the native escape hatch;
- compiles each WebAssembly subtree into its own lazily loaded module over the single-threaded executor seam and the host clock, attached to the same server-rendered markup through the same node identities, and sharing the page's client state with the generated JavaScript as the same serializable data;
- declares client state with serializable types and emits a matching JavaScript shape from the same Rust definition, so the browser and the server share one schema;
- embeds each page's initial client state in the rendered HTML as JSON, together with the element hooks the generated code attaches to;
- owns browser-side initialization and lifecycle, and integrates with the browser event loop and microtask/task model;
- creates and tracks DOM handles without exposing browser types to `rustnative-core`.

The client and the server exchange serializable data and nothing else, and no JavaScript runs on the server. The subset is kept deliberately small, because every construct in it is a translation the framework maintains for as long as it exists, and because a WebAssembly subtree is always available for what the subset does not express. Within the subset, numeric and string semantics that differ between Rust and JavaScript — 64-bit integers, overflow, UTF-8 versus UTF-16 indexing, float formatting — are restricted or wrapped so both sides agree on the same data. On every other target the same client logic is ordinary Rust and runs natively, so the subset constrains only what the browser receives, and a route with no WebAssembly subtree pays nothing for the option.

**Status (2026-09-28): implemented** on Chromium engines. `#[rustnative_web::client]` translates client logic to JavaScript at build time and refuses what is outside the subset with the three ways forward; the same logic runs natively with checked arithmetic, so both fail alike (`docs/web/client-subset.md`, `rustnative-web/tests/client_subset.rs`). See `BUILD_STATUS.md`.

### Web milestone B — Native DOM realization

Map framework nodes to semantic DOM elements rather than a canvas renderer:

```text
Button  → <button>
TextInput → <input>/<textarea>
Label    → <span>/<p>/<label>
Column   → <div>
Row      → <div>
Dialog   → <dialog> or an appropriate semantic composition
```

The adapter must retain stable node identity and native DOM ownership just like the desktop backends retain native HWND ownership. The server renderer, the generated client code, and any WebAssembly subtree address nodes through the same stable identities, so the markup one produces is exactly what the others attach to.

**Status (2026-09-28): implemented** on Chromium engines. Every node kind is its semantic element, checked under a strict policy in Edge (`docs/web/dom-mapping.md`, `rustnative-web/tests/browser_dom.rs`). See `BUILD_STATUS.md`.

### Web milestone C — CSS/layout integration

Introduce a deliberate mapping between the framework layout model and browser layout mechanics. The framework must define which semantics are computed in Rust and which are delegated to CSS, avoiding two competing layout engines for the same subtree.

This includes:

- intrinsic measurement;
- constraints;
- wrapping;
- overflow and scrolling;
- clipping;
- transforms;
- viewport sizing;
- responsive/container sizing;
- device-pixel-ratio and viewport changes;
- right-to-left mirroring, expressed as the layout model's own start/end
  semantics and applied through the host's mechanisms rather than by the
  application (Milestone 39, Milestone 46);
- the mapping itself, documented and tested, with every case where it is
  approximate named explicitly rather than discovered by users — this is the
  one backend whose host owns layout, so the negotiation is the deliverable.

Visual style is part of the same negotiation, and this is the one backend whose
host speaks the declaration vocabulary natively. It is therefore also the
backend most able to drift, so the rule is explicit: the stylesheet this backend
serves is generated from the *resolved* style of Milestone 58's compiler — the
same declarations every other backend applies — and never by running a
third-party styling toolchain over the sources at build time. Class strings stay
a compile-time authoring concept and are not a styling channel in the emitted
DOM; browser CSS is how this host applies the result, in the same sense that
`WM_CTLCOLOR*` is how Windows applies it. Theme tokens map to custom properties
so a runtime theme change stays one re-resolution here as well, and any property
the browser expresses differently from the native backends is named in the
mapping document rather than left to diverge with a version bump.

**Status (2026-09-28): implemented** on Chromium engines. Layout and styles become CSS classes compiled at build time; `rustnative_style::WEB` is the capability table (`docs/web/layout-mapping.md`). See `BUILD_STATUS.md`.

### Web milestone D — Browser input/focus/accessibility

Integrate:

- pointer and mouse events;
- keyboard events;
- composition/IME events;
- text input and selection;
- pointer capture;
- touch and pointer cancellation;
- focus/blur;
- browser Tab order;
- clipboard events;
- native HTML accessibility;
- ARIA roles/properties/states where a semantic HTML element is insufficient.

The framework accessibility model must map to real browser semantics rather than emulate accessibility behavior itself. Browser input is handled in the browser, by the generated client code or by the WebAssembly subtree that owns the node; an event reaches the server only when the handler it triggers is a server call.

**Status (2026-09-28): implemented** on Chromium engines. Pointer, wheel, composition, clipboard, keyboard, and focus reach components as the same events (`docs/web/capabilities.md`, `input_navigation_and_services_in_a_browser`). See `BUILD_STATUS.md`.

### Web milestone E — Browser services/capabilities

The capability system must include browser implementations for, where supported:

- HTTP/fetch;
- WebSocket;
- Web Storage;
- IndexedDB;
- Cache Storage;
- Clipboard;
- Notifications;
- File System Access;
- Web Share;
- geolocation;
- camera/microphone/media devices;
- Bluetooth/Web Bluetooth where available;
- sensors where available;
- permissions;
- downloads;
- URL/history/navigation;
- service workers;
- web workers.

Capability availability must be explicit because browser support varies by browser, security context, permissions, user gesture, and deployment mode. Client logic reaches these capabilities through bindings the code generator emits; server logic reaches its own services in Rust.

**Status (2026-09-28): implemented** on Chromium engines. Every capability is an effect, answered where the browser has it and opened in `Permissions-Policy` only when declared (`docs/web/capabilities.md`). See `BUILD_STATUS.md`.

### Web milestone F — Async/runtime scheduling

The scheduler must support browser constraints without blocking the browser main thread. Generated client code and WebAssembly subtrees run on the browser's event loop and task model, and DOM operations must remain on the browser thread where required.

Long-running CPU work that must stay in the browser runs in a WebAssembly subtree, with a path to Web Workers running that module when the capability is available and a serializable message bridge back into the page; otherwise it belongs on the server, reached through a server call.

**Status (2026-09-28): implemented** on Chromium engines. `HostExecutor` runs component tasks on the page's loop and the host clock; islands batch one patch per microtask (`pages_in_a_browser`). See `BUILD_STATUS.md`.

### Web milestone G — Routing, navigation, persistence, and browser lifecycle

Add browser-native lifecycle concepts:

- URL routing;
- History API integration;
- back/forward navigation;
- deep links;
- page visibility;
- online/offline state;
- before-unload/pagehide/unload boundaries where applicable;
- persistence and restoration;
- storage-backed application state.

**Status (2026-09-28): implemented** on Chromium engines. Client navigation with prefetch, history with scroll and state restored, `Persisted<S>`, and lifecycle events (`docs/web/capabilities.md`). See `BUILD_STATUS.md`.

### Web milestone H — Server-rendered HTML, client attachment, and progressive enhancement

The framework should support server-rendered HTML as a deployment mode, with the generated client code — and any WebAssembly subtree — attaching to that markup in the browser rather than re-rendering it. This requires deterministic tree identity and an ownership model under which the server's markup and the client's code agree on every node.

This milestone owns the server half of the Web target, and the pieces it adds are shared by both server-side modes (Web milestone K runs exactly this render in a serverless host):

- an **HTML renderer** over the existing tree and style resolution, with no platform event loop and no native objects — the same `view()` that produces DOM produces markup;
- **server-side data loading**: a render may await, and the renderer must be able to render a tree whose data arrives asynchronously, with a deterministic result;
- **typed server functions**: a call from client logic to a Rust function marked `#[server]` that exists only on the server, carried over HTTP as serializable data and delivered through the same message/callback model, so client-side and server-rendered applications share one way of reaching the server;
- **per-request state**: no process-wide mutable state on the render path; services, storage, and session data are constructed for the request;
- **static generation** as the degenerate case: rendering the same pages at build time for hosts that serve only files, reusing this renderer rather than a separate one.

The Web roadmap therefore includes:

```text
Rust server/runtime
      ↓
HTML output + initial client state as JSON
      ↓
browser loads page and the generated JavaScript
(and, lazily, the module of each WebAssembly subtree)
      ↓
client code attaches to the existing DOM
      ↓
local updates in the browser; server calls exchange JSON
```

Attachment must preserve semantic HTML, keep the page accessible before the client code loads, and detect/handle server-client tree mismatches deterministically.

Section 11 adds four requirements to this milestone, each of which exists
because the dominant archetype in this space manages the corresponding problem
rather than eliminating it:

- **the server seam is typed end to end**: a server function has one
  definition, is compile-time checked at both call sites, has its client-side
  shape emitted from that same definition rather than written by hand, and has
  a documented wire format and versioning story;
- **selective attachment**: only subtrees that need interactivity receive
  client code, decided by the framework from the tree rather than by
  annotation, so a route with no interactive subtree ships no JavaScript at all,
  and one with no WebAssembly subtree ships no WebAssembly;
- **streaming**: a subtree may declare a pending representation and the
  renderer flushes the shell before it resolves, which is what makes per-request
  hosts' time limits survivable;
- **mismatch as a non-category**: because one tree definition produces both
  the server's markup and the client code that attaches to it, divergence is a
  bug in one code path rather than a disagreement between two. The cross-mode
  equivalence test in Web milestone K is promoted to a stated guarantee, and a
  progressive-enhancement baseline — forms, navigation, and submissions that
  work before and without the client code — is part of this milestone rather
  than an aspiration.

Three further concepts from the survey belong to this milestone's render path
rather than to the server model:

- **server-only components**: components that exist only on the server, whose
  code never reaches the client, whose boundary props are checked for
  serializability at compile time, and whose output is tree payload merged by
  the ordinary reconciler (specified in Milestone 49, carried by this renderer);
- **partial prerendering**: a route served as a prerendered static shell with
  its dynamic parts streamed into the same response, the split determined by
  which subtrees read request-scoped state rather than declared by hand, and
  shown by the inspector (`C06-1`);
- **render mode per subtree** rather than per build — static, server-rendered,
  client-interactive, and the server-interactive mode of Milestone 55 — so the
  three deployment modes are the application-wide case of a per-subtree choice.

The server *application* model — authentication, a data layer with
compile-time-checked queries, migrations, durable background jobs,
secure-by-default request handling, and a generated administrative surface —
is Milestone 49, and it builds on this render path rather than duplicating it.

**Status (2026-09-28): implemented** on Chromium engines. Pages render per request — whole, streamed, or partially prerendered — with islands, forms and links that work without JavaScript, typed server functions, server-only components, and `Live` server-interactive subtrees with the automatic hand-over (`docs/web.md`, `docs/web/wire-format.md`). See `BUILD_STATUS.md`.

### Web milestone I — Service workers and offline applications

Support service-worker-backed application architectures for offline caching, background synchronization where available, update/version management, and installable Progressive Web Apps.

From the concept survey, three mechanisms this milestone owns rather than
leaves to applications:

- **declared caching strategies per route and asset class** (`C136-1`) —
  cache-first, network-first, stale-while-revalidate — generating the worker
  from the route table rather than asking each application to hand-write one,
  with the shell precached and queued mutations synchronized in the background;
- **worker update choreography and storage policy** (`C136-2`): activation,
  claiming open pages, a user-visible path to the new version, and a declared
  quota and eviction policy, because "why am I seeing an old version" and
  silent eviction are this mechanism's two classic failures;
- **partitioned-by-default storage and cookies** (`C138-1`) with an explicit
  storage-access request path, and an embedded-context test that runs a
  component inside a third-party frame — which is our problem the moment a
  component is exported as a custom element (`C43-1`).

**Status (2026-09-28): implemented** on Chromium engines. `Pwa`: a manifest, icons, and a service worker that precaches, serves pages network-first, queues calls offline and replays them, and updates on the person's say (`docs/web/offline.md`). See `BUILD_STATUS.md`.

### Web milestone J — Web packaging, testing, and deployment

The CLI/package system must eventually support:

- client JavaScript emission from the application's client logic;
- WebAssembly builds and their JavaScript glue for WebAssembly subtrees, one lazily loaded module per subtree;
- emission of the shared serializable schema for client state and server calls;
- static assets;
- source maps from the generated JavaScript and WebAssembly back to the Rust they came from — development tooling only, never needed at runtime;
- development server;
- browser hot reload/development workflow;
- production bundling;
- server-rendered and serverless deployment;
- PWA manifests;
- service-worker packaging;
- browser test execution;
- accessibility testing;
- feature/capability detection.

The deployment mode is a build option, not a different application:

```text
rustnative build web --mode client
rustnative build web --mode server
rustnative build web --mode serverless [--host <adapter>]
```

Web support is complete only when an application can be developed, tested, packaged, deployed, and updated through the same framework tooling rather than merely emitted as HTML and JavaScript.

Section 11 adds to this milestone:

- **navigation performance**: speculation rules derived from the route graph
  with side-effect-free prerendering and a bandwidth policy (`C137-1`);
  back/forward-cache compatibility as a tested property, scroll restoration per
  history entry, and route-change announcements to assistive technology, which
  is a conformance item rather than a polish item (`C137-2`);
- **client code splitting by route**, with lazily fetched subtrees, a
  per-route payload budget that counts WebAssembly modules with the generated
  JavaScript, and a startup budget that is independent of total
  application size — measured on a throttled low-end device profile
  (Milestone 42);
- **`rustnative deploy`** with a documented adapter contract covering at least
  one static host, one long-lived server, one per-request function runtime, and
  one edge/WASM runtime, each with a local emulator; plus preview deployments,
  staged rollout, and one-command rollback, and export of standard
  infrastructure descriptions so existing pipelines can consume our output
  instead of being replaced (Milestone 50);
- **single-artifact deployment** as a supported shape: server binary with
  embedded assets and no separate asset pipeline to operate (Milestone 50).

**Status (2026-09-28): implemented** on Chromium engines. Static export, source maps, custom elements, responsive images, font subsets, runtime metrics, the dev loop, and `build`/`run`/`dev`/`package web`, `serve static`, `test --browser`, `bench --target web` with `budgets/web.toml`. See `BUILD_STATUS.md`.

### Web milestone K — Serverless and edge deployment

The serverless mode runs Web milestone H's renderer per request in a host that keeps nothing between requests, and it is a distinct milestone because those hosts impose constraints a long-lived server does not:

- **two runtime shapes**, both of which the render path must build for: a native binary invoked per request (managed function runtimes, through a runtime adapter) and a WASM sandbox (edge/worker runtimes, and `wasm32-wasip1` hosts). This is server code compiled to WebAssembly for the host, not code sent to the browser;
- **the WASM sandbox shape runs on the shared core seams**: these sandboxes drive futures on one thread, so the render path uses the single-threaded executor seam rather than `Send` tasks; and `std::time::Instant` is unavailable on the sandboxed WASM targets, so time reaches the framework through the host clock rather than from the standard library;
- **stateless by construction**: nothing durable lives in the process. Anything that must outlive a request goes through a service — storage, HTTP, or a database — and the state store on that path is per request;
- **cold start and binary size are correctness-adjacent**: no process-wide lazily created runtime on this path (the shared Tokio runtime is exactly what must not be reached for), a single-threaded executor, and a size/startup budget measured in CI like any other regression;
- **no work outliving the response**: a request owns a task scope bounded by the response, cancelled the way a component's scope is cancelled at unmount. A task that survives the response is a bug, not a background job;
- **streaming responses**: the HTML renderer should be able to emit chunks so a response starts before the whole tree is rendered, which is what makes these hosts' time limits survivable;
- **configuration and secrets from the environment**, read per invocation, never cached in a global;
- **one route table**: `Route`/`Router` from Milestone 30 matches the request path server-side and the URL client-side, rather than a second routing model for the server;
- **host limits surfaced as capabilities**: execution timeouts, memory ceilings, absent or ephemeral filesystems, and response-size limits are answers the application can ask for, not surprises in production;
- **tooling**: host adapters for at least one function runtime and one edge/WASM runtime, plus a local emulator so the serverless path is runnable and debuggable without deploying;
- **the equivalence test**: one application, the same state, rendered client-side, server-rendered, and serverless, and with a subtree's client logic run as generated JavaScript and as WebAssembly, must produce the same DOM. That test is what keeps the three modes one target instead of three codebases.

This milestone is request-shaped. The per-invocation discipline above — nothing
durable in the process, a scope bounded by the invocation, configuration read
per invocation — applies unchanged to event-triggered invocations and to
durable workflows, which are Milestone 56; revisions, traffic splitting, and
resource bindings for these hosts are in Milestone 50.

**Status (2026-09-28): implemented** on Chromium engines. `rustnative_server::serverless::run` answers AWS Lambda's runtime API and WAGI; the pipeline builds for `wasm32-wasip1`; `serve lambda` and `serve wagi` emulate the hosts with their limits (fuel per route, a memory ceiling); `examples/web-notes` runs in every shape, and one application in the same state has the same DOM in every mode (`the_ui_is_the_same_in_every_mode`). Owed: response streaming on a function runtime, and real provider accounts (`docs/web/deploy.md`). See `BUILD_STATUS.md`.

---

# 9. Cross-cutting quality milestones

These should advance continuously rather than waiting for the end.

## Testing

Maintain:

- core unit tests;
- syntax equivalence tests: both spellings of every documented node kind and
  modifier, asserted to produce equal `Node` values (2.9), with the
  markup side compiled through both carriers (`.rsx` files and `rsx!`), plus
  expansion goldens, a compile-failure suite for the markup diagnostics, and
  `.rsx` compiler tests for disambiguation, context inference, and source-map
  round trips;
- style equivalence tests: both spellings of every documented style property,
  asserted to resolve to equal values (2.14), plus the class and declaration
  diagnostics, the colour and unit conversions, and each backend's style
  capability table;
- reconciliation tests;
- layout tests;
- scheduler tests;
- lifecycle tests;
- component-tree tests;
- platform integration tests, run against each backend's real host;
- browser tests for the Web backend, and a rendered-output equivalence test
  across its three deployment modes;
- synthetic-terminal tests for the terminal backend, asserting the cell grid;
- end-to-end example applications.

Every backend should be testable without its hardware for most of its logic, and untestable-without-hardware work should be recorded as such (2.13) rather than assumed to pass.

Milestone 45 is what makes that sentence true rather than aspirational, and it
adds to the list above: a headless reference backend that realizes the tree
into an inspectable model, synthetic input dispatched through the real input
path rather than a test-only shortcut, a deterministic test clock and executor,
golden and visual regression tests over realized output, a lifecycle
conformance suite, and a device and emulator matrix in CI. Milestone 41 adds
the conformance suites — fidelity, text, layout under scaling and
pseudo-localization, accessibility, modal operation, invalidation, and
lifetime — that each backend must pass to be called complete.

## Correctness boundaries

- keep `unsafe` localized;
- document every FFI ownership rule;
- minimize global mutable state;
- make task cancellation deterministic;
- keep native object lifetime explicit;
- preserve stable IDs across rerenders;
- never mutate component state from worker threads.

Milestone 41 converts each of these from a practice into a guarantee with a
named test behind it, and Milestone 39 adds the boundaries that are currently
unwritten: thread affinity enforced by the type system where the substrate
allows and by a debug assertion everywhere else, one ownership module per
backend with the host's memory convention asserted in tests, a documented
escape-hatch contract, subtree error boundaries, and a per-target panic and
teardown policy that leaves the host — and on embedded targets the hardware —
in a safe state.

## Performance

Measure and eventually optimize:

- tree diff cost;
- layout cost;
- native object creation/destruction;
- text measurement;
- event dispatch;
- scheduler overhead;
- scrolling;
- virtualized list performance;
- startup and binary size.

"Eventually optimize" is no longer sufficient on its own: Milestone 42 turns
this list into declared per-target budgets — cold start, resident memory,
artifact size, frame-time distribution including worst case, input latency,
per-route client payload, edge CPU time per route, embedded RAM and flash, and
build time — measured in CI on every commit, on a low-end reference profile
where one exists, with a regression failing the build. Every comparative
performance claim this project makes must be traceable to a number in that
budget file.

## Tooling and diagnostics

Eventually add:

- tree inspection;
- component ownership diagnostics;
- native-object leak diagnostics;
- scheduler/task inspection;
- layout overlays;
- event tracing;
- accessibility inspection;
- platform capability diagnostics.

This list is Milestone 44 and is no longer deferred. The substrate that makes
the rest of this plan possible — compiled, no dynamic runtime — is also the
substrate that does not hand these capabilities over for free, so they are
engineered deliberately: one inspection protocol exposed by the runtime,
answered by every backend, carried over a transport that works locally,
on-device, and remotely, with an inspector client in the CLI, an
in-application overlay on the draw-list path, and a reduced deferred-formatting
form for constrained targets. Milestone 43 covers the other half of the same
problem — the edit-to-running loop — for the same reason.

---

# 10. End-state architecture

The intended final architecture is:

```text
                       Application
                           │
                    Component Runtime
                           │
        ┌──────────────────┼──────────────────┐
        │                  │                  │
     Components          Scheduler          Services
        │                  │                  │
        ├── props          ├── tasks          ├── HTTP
        ├── state          ├── scopes         ├── storage
        ├── effects        └── wakeups        ├── clipboard
        ├── lifecycle                         └── platform APIs
        └── messages
        │
        ▼
          builder syntax ──┬── markup syntax (.rsx files, rsx!)
                           │
                           ▼
                  Declarative UI Tree
                           │
                    Reconciliation
                           │
                       Layout
                           │
                    Native realization
                           │
  ┌─────────┬─────────┬────┴────┬─────────┬─────────┐
  ▼         ▼         ▼         ▼         ▼         ▼
Windows   macOS     Linux    Android     iOS       Web
  │         │         │         │         │         │
  └─────────┴─────────┴─────────┴─────────┴─────────┘
                           │
              ┌────────────┴────────────┐
              ▼                         ▼
          Terminal                  Embedded
       via capability subsets, not via a reduced application model
```

The Web branch ends in three deployment shapes — client-side, server-rendered, and serverless — from the same tree; the terminal and embedded branches realize that tree onto a cell grid or a display driver. None of them is a reduced version of the application model: they differ in capabilities, not in semantics.

The two authoring surfaces sit at the top of that picture and end at the same place. A builder-written application and a markup-written one produce the same declarative tree, reach the same backends, and are indistinguishable from the reconciler down (2.9). A single application may use both — a screen written in markup can call a function that assembles a subtree with the builder API, and a markup element can be spliced into a builder chain — because there is only one node type between them.

The two style spellings converge in the same way, one step lower: typed
properties and utility classes both become declarations, declarations resolve
against the theme, and native realization receives concrete values with no
record of which spelling produced them (2.14).

The final framework should feel like a native application framework first and a cross-platform abstraction second: one Rust application model, native operating-system behavior, explicit platform capabilities, and strong compile-time/lifetime guarantees wherever Rust can provide them.

## Long-range roadmap

Everything through Milestone 32 is complete (section 3), and so is the core work shared by the remaining targets (the host clock, the single-threaded executor seam, and the `no_std` value types in `rustnative-types` — 2026-09-23). What remains, in the order the backlog is currently expected to be taken up — subject to 2.13, since the order follows hardware availability and contract readiness rather than importance:

```text
core work shared by the remaining targets
  (no_std-capable core subset, non-Send executor seam, host clock)
        ↓
Tier 0 — the markup syntax, the style spellings, portable-surface
         obligations, and interoperability (53, 58, 39, 40)
  everything that costs once now and once per backend afterwards
        ↓
platform backends, each finished by section 8's definition
  macOS · Linux · Android · iOS · Embedded · Terminal
        ↓
Web backend, milestones A–K
  client-side → server-rendered → serverless
        ↓
Tier 2 — the application layer (46–48, 54, 60, 62, 63)
  localization · state, resilience, data · components and tokens ·
  responsiveness under load · the visual pipeline · media, capture,
  and files · trust and compliance
        ↓
Tier 3 — the server model, deployment and updates, operations, the
  project around the framework, reconciliation beyond the screen,
  durable and event-driven execution, surfaces beyond the main
  window, product operations, and reach (49–52, 55–57, 64, 66)
        ↓
one application, every target, the same semantics
```

Tier 1 (Milestones 41–45, 59, 61, and 65) — guarantees and conformance suites,
budgets, the developer loop, inspection, test infrastructure, the toolchain and
service layer, interaction depth, and engineering discipline — runs
continuously across all of the above and gates each backend's completion rather
than following it.

The cross-cutting work in section 9 — testing, correctness boundaries, performance, tooling, and diagnostics — advances alongside all of it rather than after it, and section 11's Milestones 41–45 are what turn that section's "eventually" list into gates a backend must pass.

---

# 11. Production-parity milestones (39–66)

Sections 1–10 specify the framework's architecture and its targets. They do not
specify the accumulated answers a mature framework is expected to have —
localization, an asynchronous data layer, an inspector, a fast edit-to-running
loop, an update path after ship, error containment, a stability policy. A
framework that is architecturally right and missing those is not chosen, and
none of them is backend work: they would still be missing on the day every
backend in section 8 is finished.

The milestones below close that gap. They come out of the standing analysis in
`docs/ecosystem-analysis/`, which examines the framework families this project
is measured against from their substrate choices upward — no product or vendor
is named there, deliberately, so future work is anchored on mechanisms rather
than on imitation — and scores this codebase layer by layer against them.

## How these interleave with sections 8 and 9

Milestone numbers are identities, not an order; section 8 already establishes
that convention. The sequencing is:

```text
core work shared by the remaining targets
  (no_std-capable core subset, non-Send executor seam, host clock)
        ↓
Tier 0   Milestones 53, 58, 39, 40  before the second backend exists
        ↓
Tier 1   Milestones 41–45, 59,      continuous; gates each backend's completion
         61, 65
        ↓                           (folded into section 8's definition of done)
Tier 2   Milestones 46–48, 54,      before any public release
         60, 62, 63
        ↓
Tier 3   Milestones 49–52, 55–57,   with and after the Web track
         64, 66
```

One rule binds the order, and it is the reason Tier 0 exists at all:
**anything that is a per-backend obligation lands before the second backend
does.** A layout property, a capability shape, a conformance suite, an
embedding contract, or an authoring surface every later example and template is
written in costs once when there is one backend and once per backend
afterwards. This is section 2.4's argument — widen the contract rather than add
a conditional — applied to schedule instead of to structure.

Tier 1 is not a phase. It advances continuously the way section 9 does, and
section 8's definition of a complete backend now includes passing it.

## Traceability

Each milestone below lists the requirement identifiers it satisfies. Those
identifiers are defined in `docs/ecosystem-analysis/` — in `foundations.md` for
the root-layer ones (`X-L0-*` through `X-L7-*`) and in the platform documents
for the rest (`W-*` web, `D-*` desktop, `M-*` mobile, `E-*` embedded) — and
`parity-matrix.md` scores the codebase against them. Concept requirements
(`Cnn-k`) come from the concept catalogue — `concepts-core.md`,
`concepts-app.md`, `concepts-delivery.md`, and `concepts-embedded.md` — which
analyses the ideas the competing framework families introduced independently of
the families themselves, including the ones deliberately rejected, and
`concepts-toolchain.md`, which covers the layer between a repository and an
installed, updatable application; `concepts-surface.md`, `concepts-product.md`,
`concepts-discipline.md`, and `concepts-reach.md`, which cover the frame
pipeline and interaction depth, the obligations stores and regulators impose,
the practices that keep the framework's own promises true, and the mechanisms
that reach managed fleets, desktop shells, constrained networks, and the
field. The
identifiers are stable so that a later session can check a milestone against
the analysis that produced it without re-deriving the argument.

---

## Tier 0 — before the second backend

Listed first in this tier are Milestones 53 and 58, whose numbers are later
than their peers' for the reason stated above: numbers are identities, not an
order. They are also the two authoring milestones, and they are here for the
same argument — what a developer types is what every later example, template,
and conformance case is written in.

## Milestone 53 — The markup syntax

2.9 states that the declarative tree has two spellings and that both are
first-class. One of them exists. This milestone builds the other, and it is
Tier 0 for the same reason the rest of Tier 0 is: the authoring surface is what
every later example, template, tutorial, doc test, component-library entry, and
conformance case is written in. Writing that corpus once and retrofitting a
second syntax through it costs more than every other item in this tier
combined, and the cost grows with each backend, each milestone, and each page
of documentation.

The syntax is a compile-time front end and nothing else. It is therefore
entirely backend-independent: no backend implements it, none is affected by
it, and a backend author never encounters it.

Satisfies: `X-L3-8`, `X-L3-9`, `X-L3-10`, `X-L3-11`.

### Why a compile step, and why the macro as well

Markup-extended source files work the same way in every language that has
them: the host compiler does not understand the extension, so a tool that owns
the parse lowers the file to the host language first, and the tooling around
it — errors, the editor, the formatter — maps what the host compiler reports
back to what the developer wrote. That is not a workaround peculiar to Rust; it
is the whole mechanism, and the tooling half is most of the work. A `.rsx` file
is done properly only when a developer never has to look at the lowered file,
and this milestone is scoped so that they do not.

The macro stays, and not as a fallback. It is the only carrier that needs no
build step and no tooling beyond the compiler, so it is the right one for a
crate without a build script, for one markup-shaped subtree in a `.rs` module,
and for runnable API documentation, which `rustdoc` compiles as plain Rust. It
is also the single implementation both carriers share: the `.rsx` compiler
wraps and delegates, and does not parse markup a second way.

### The grammar

One grammar, identical in both carriers, owned by a `rustnative-markup` library
crate that the proc macro, the build-script compiler, and the CLI all link:

- **elements are node kinds**: `Column`, `Row`, `Label`, `Button`,
  `TextInput`, `Canvas`, `Surface`, `TabBar`, `VirtualList` — one element per
  `Node` constructor, added to in the same commit that adds a constructor;
- **attributes are builder methods.** `key` is the constructor's key and stays
  explicit in both syntaxes, because a key is semantic (2.7) and nothing may
  infer it. `LayoutStyle` and `ColumnStyle`/`RowStyle` fields are flattened
  into attributes (`width`, `height`, `margin`, `align_self`, `constraints`,
  `padding`, `gap`, `align_items`, `overflow`); every `with_*` modifier is an
  attribute of the same name without the prefix (`accessibility`, `style`,
  `input`, `opacity`, `transition`, `item_index`); `disabled` and `hidden` are
  present-means-true flags. Values are `name="literal"` or `name={expr}`, and
  an attribute accepts exactly the type its builder method accepts;
- **structural constructs in child position**: nested elements, `{expr}` for
  any `Node` or `IntoIterator<Item = Node>`, `if`/`else`, `match`, `for`, and
  `<>…</>` fragments for a branch that yields several children. These are the
  constructs markup is good at, and they are why the markup form is not a
  transliteration of the builder form;
- **component elements**: `<Screen key="home" navigator={nav.clone()} />`
  lowers to `ComponentContext::child_with_props`, with the props struct built
  from the attributes, so a missing or misspelled prop is a type error at the
  element;
- **`..expr`** applies any `FnOnce(Node) -> Node`, which is how an
  application's own extension-trait modifiers, which the grammar has never
  heard of, stay reachable from markup;
- **no bare text.** Text is an attribute (`text="…"`) or a braced expression,
  never loose characters between tags. A macro receives Rust tokens, and loose
  prose — one apostrophe is enough — is not a valid token stream. A `.rsx` file
  could lift that limit, since its compiler owns the parse, and deliberately
  does not: the moment one carrier accepts something the other cannot, an
  element can no longer move between them unchanged, and "one grammar" stops
  being true. The rule has a second benefit — every `.rsx` file is a valid Rust
  token stream, which is what lets its compiler reuse the host language's own
  lexer rather than maintain one.

### Carrier 1 — `.rsx` files

- **markup is an expression.** In a `.rsx` file an element may appear wherever
  Rust accepts an expression — a `let` initializer, a function's tail, a
  `return`, a closure body, a match arm, a call argument, a struct field, an
  array element — with no wrapper. Everything else in the file is ordinary
  Rust, and a `.rsx` file with no markup in it is a `.rs` file with a different
  extension;
- **the disambiguation rule, stated once.** In expression-start position, `<`
  followed by an identifier or by `>` begins an element; a qualified path
  (`<T>::item`, `<T as Trait>::item`) is recognized by the `::` or `as` that
  follows and stays Rust. `<` after an expression is always a comparison, and
  `<` in type position is always generics. The file is parsed by a Rust
  expression parser extended with that one primary expression, not scanned
  with token heuristics, so the rule is exact rather than usually right;
- **the component context is found, not written.** A component element needs
  the `ComponentContext` it composes through. A macro cannot see the function
  it is called in, so `rsx!` takes the context explicitly (below). The `.rsx`
  compiler can, so it uses the enclosing function's `ComponentContext`
  parameter; a function with none, or with more than one, is a compile error at
  the component element, and the fix is the macro form with an explicit
  context — which is valid in a `.rsx` file, since a `.rsx` file is Rust;
- **the lowering is a wrap.** `rustnative_build::compile_rsx()`, one line in the
  build script beside `embed_resources()`, compiles every `.rsx` file under
  `src/` into `OUT_DIR`, emitting it byte for byte except that each markup
  expression becomes `::rustnative_core::rsx!(…)` around the original text, with
  the inferred context supplied. Lines never move, so the map from the lowered
  file back to the source is a column offset on the lines that contain markup,
  recorded in a source map beside the output;
- **modules.** A `.rsx` module is declared with `rsx_mod!(inbox);`, which
  expands to the module that includes the lowered file, so the module tree
  reads the way it would with `mod inbox;`. `mod` declarations inside a `.rsx`
  file are rewritten by the compiler to explicit paths, so a `.rsx` module can
  have `.rs` and `.rsx` children alike. A crate root stays `.rs`, because Cargo
  hands it to `rustc` directly;
- **incremental and cached.** Each file is recompiled only when it changes,
  `rerun-if-changed` is emitted per file, and the cost sits in Milestone 42's
  build-time budget like any other part of the build;
- **diagnostics land on the `.rsx` file.** `rustnative build`, `check`, and
  `test` run Cargo with structured diagnostics and rewrite every span that
  falls in a lowered file — errors from the markup and ordinary Rust errors
  alike — back through the source map, so the developer is shown the file,
  line, and column they wrote. Plain `cargo build` still works and reports
  positions in the lowered file, `OUT_DIR/rsx/<the source's relative path>.rs`,
  so the source is named by its path; that is the one
  place the compile step shows through, it is stated here rather than
  discovered, and it is why the CLI is the documented way to build a `.rsx`
  project;
- **editor support is a proxy, not a fork.** `rustnative lsp` presents `.rsx`
  files to the editor and forwards to the Rust language server over the lowered
  files, mapping positions in both directions through the same source map —
  completion, hover, go-to-definition, rename, and diagnostics included. It
  owns only what is markup: element and attribute completion, and hover that
  names the builder method an attribute calls;
- **formatting.** `rustnative fmt` formats a `.rsx` file whole: the Rust in it
  through `rustfmt`, the markup in it with the same markup formatter the
  `rsx!` carrier uses, so one project has one style.

### Carrier 2 — the `rsx!` macro

- **`rsx!`**, in a `rustnative-macros` proc-macro crate that is a thin shell
  over `rustnative-markup`, re-exported from `rustnative-core` behind a
  default-on `markup` feature — default-on so it is not a second-class opt-in,
  and a feature so the constrained profiles of Milestone 37 can drop a
  proc-macro dependency they cannot afford. `.rsx` files lower to `rsx!`, so
  turning the feature off turns off both carriers together;
- **explicit context.** A tree containing component elements names its context
  with a leading `in context,` — `in` is a reserved word, so it cannot collide
  with an element name — and a tree without component elements omits it;
- **spans are native.** Tokens handed to a proc macro keep their source
  positions, so errors inside `rsx!` point at the attribute or element in the
  `.rs` file with no remapping, under plain `cargo` as well as under the CLI;
- **`rsx!` evaluates to a `Node`**, so any builder chain applies to a markup
  tree directly and any markup tree is a builder expression.

### Shared by both carriers

- **diagnostics at compiler quality**: spans that point at the offending
  attribute or element rather than at the macro call or the lowered file, an
  unknown attribute that names the builder method it was looking for, a type
  mismatch reported against the attribute's own span, and an unclosed or
  mismatched element reported at the opening tag. Held by a compile-failure
  suite run through both carriers, not by inspection;
- **`rustnative expand`** prints the builder form a markup tree lowers to, from
  either carrier;
- **the equivalence suite**: for every node kind and every modifier, the
  builder spelling and the markup spelling are asserted to produce equal `Node`
  values, with each markup case compiled once from a `.rsx` file and once
  through `rsx!`. `Node` already derives `PartialEq`, so this is a direct
  assertion, and a new constructor or modifier without both spellings fails the
  suite (Milestone 41 owns it thereafter);
- **compiler tests for the `.rsx` carrier**: the disambiguation rule, context
  inference and both of its error cases, byte-for-byte preservation outside
  markup, source-map round trips for every diagnostic position, and module
  paths;
- **expansion goldens**, so that a change to the lowering is a visible diff
  rather than a silent one, and so that the "expands to builder calls and
  nothing else" claim of 2.9 is checkable;
- **`rustnative new --syntax builder|markup`**, with no default. The generator
  does not pick a side on the developer's behalf; the markup template is
  written in `.rsx` files with `compile_rsx()` already in its build script, and
  both templates are the same application;
- **the documentation obligation**: every example in `README.md`, `PLAN.md`,
  and the guides exists in both syntaxes, the markup side written as it appears
  in a `.rsx` file; every runnable doc example on a public API exists in both,
  the markup side through `rsx!` because that is what `rustdoc` compiles. A
  public API gains both or neither. This roughly doubles the doc-test count,
  which is the intended cost.

**Done when** the equivalence suite covers every node kind and modifier
through both carriers, the compile-failure suite covers every diagnostic above
through both carriers, a diagnostic from a `.rsx` file is reported at its
source position by `rustnative build` and by the editor, `rustnative fmt` and
`rustnative expand` work on both carriers, both project templates build and
run, and no documented example exists in only one syntax.

**Status (2026-09-24): implemented** — `crates/rustnative-markup` (parser,
element table, lowering, formatter, `.rsx` compiler and source map),
`rsx!` in `crates/rustnative-macros`, `.rsx` compilation through
`rustnative_build::compile_rsx`, diagnostics remapped by the CLI,
`rustnative expand`/`fmt`/`lsp`, both project templates, and the
equivalence, compile-failure, and documentation-parity suites in
`crates/rustnative-conformance`. Unverified: the LSP proxy against a live
rust-analyzer (tested with a scripted server). See `BUILD_STATUS.md`.

**Depends on** nothing. Like Milestone 39, it is deliberately early.

## Milestone 58 — The style spellings

2.14 states that the resolved style is the model and that it has two spellings.
One of them exists: Milestone 21's typed properties, theme tokens, and state
variants. This milestone builds the other — a declaration vocabulary and the
utility classes above it — and lands the per-backend obligations that come with
it.

It is Tier 0 for two reasons, one per half. The utility vocabulary is an
authoring surface, so Milestone 53's argument applies to it unchanged: the
component library, the templates, the guides, and every screenshot in them are
written in a style spelling, and retrofitting the second one through that corpus
costs more later than it does now. The other half is a genuine per-backend
obligation: the style capability table and the unit mapping (2.14) are something
each backend must answer, and answering them once against one backend is what
keeps them from being negotiated six more times.

Satisfies: `X-L3-12`, `X-L3-13`, `X-L3-14`, `X-L3-15`, `X-L3-16`, and the
resolution half of `X-L3-7`; concepts `C22-4`.

### The layers, and what is deliberately not here

```text
utility classes      p-4  bg-primary  hover:bg-primary/90  md:flex-row  dark:…
        │            compiled at build time; unknown class = error
        ▼
declarations         padding: 1rem;  background: var(--color-primary);
        │            values, units, calc(), colour functions, token references
        ▼
typed style properties  ── the same values a builder chain or a markup
        │                  attribute sets directly (2.9)
        ▼
resolved style       theme + default + override + state (Milestone 21)
        ▼
backend              concrete values, per-property capability answered
```

Each layer lowers into the one below it, which is what makes vocabulary
equality (2.14) true by construction rather than by maintenance. What is *not*
in this stack is the rest of the web's styling model: no selector matching, no
specificity, no cascade, no descendant or sibling rules, and no stylesheet whose
text decides which nodes it applies to. A framework that adopted those would own
a second engine competing with its own resolution, on every host, forever — the
same trade Web milestone C refuses for layout.

### The declaration vocabulary

A small, exactly specified subset of the web's value syntax, owned by a
`rustnative-style` library crate and parsed at build time:

- **values and units**: lengths (`px`, `rem`, `em`, `%`, and the viewport and
  container units where a host can answer them), colours, angles, numbers, and
  keywords, each admitted only where the typed property accepts it;
- **`calc()` and its arithmetic**, because the utility layer above generates it;
- **colour functions** — `rgb()`, `hsl()`, `oklch()`, and `color-mix()`, the
  last two because the v4 default palette and its opacity modifiers are defined
  in them — evaluated at build time and converted per backend: 8-bit sRGB for
  GDI, the terminal's own depth, the host's colour type elsewhere, with
  out-of-gamut values clipped by one documented rule rather than per backend;
- **custom properties as token references** (`var(--color-primary)`), which is the
  representation 2.14's third rule needs: a declaration that still points at a
  token when it reaches resolution;
- **no selectors, at any point.** A declaration block is attached to a node by
  the code that writes it, never matched against the tree.

Every property in the vocabulary maps to exactly one typed style or layout
property, and the table is part of the crate rather than folklore: adding a
style property adds its declaration name, its utility spelling, and its
per-backend capability answer in the same commit (CONTRIBUTING).

### The utility vocabulary

The utility layer is the reason the declaration layer is worth having, and the
vocabulary is not invented here. The compatibility target is **Tailwind CSS
v4**, named explicitly because a compatibility claim without a version is not
one, and chosen because its configuration is itself CSS — a theme block of
custom properties — which is the half of the styling language this framework
already needs:

- **utilities**: one class, one set of declarations. Spacing is computed rather
  than tabulated (`p-13` is as valid as `p-4`), colours come from the theme's
  token namespaces, and every utility resolves to typed properties or fails;
- **state variants** — `hover:`, `focus:`, `focus-visible:`, `active:`,
  `disabled:` — map onto Milestone 21's existing state variants, not onto a new
  mechanism;
- **scheme and size variants**: `dark:` on the host's colour scheme, and
  `sm:`/`md:`/`lg:` on the size classes and breakpoints Milestone 39 puts in the
  portable layout model (`C22-1`), so one vocabulary answers to the layout model
  rather than to device guesses (`C22-4`);
- **arbitrary values** (`w-[37px]`, `bg-[oklch(0.62_0.19_259)]`) through the
  declaration parser, which is what keeps the vocabulary from being a closed
  list;
- **class strings are literals**, resolved where they are written — in
  `.with_class(classes!("…"))` on a builder chain (the macro is what checks
  the string at compile time), in `class="…"` on a markup element
  (the same `with_` rule the markup grammar already applies to modifiers), and in
  the component library's own props. A computed class name is a compile error
  naming the rule, rather than a string that silently styles nothing;
- **an unknown or unsupported class is a compile error** with the span of the
  class inside the string, naming the nearest property it was looking for. This
  is the one place the framework deliberately diverges from the vocabulary it
  implements, where dropping unknown classes silently is the normal behaviour
  and the most common way a utility codebase rots.

### The style file

One file per project — `app.css` by default, read by
`rustnative_build::compile_styles()` in the build script beside `compile_rsx()` —
written in the subset of the v4 directives that survive without a cascade:

- **`@theme`** defines the token namespaces (colour, spacing, radius, text,
  font, shadow, breakpoint, easing), which become theme tokens in Milestone 21's
  model. Values stay references so a theme switch is a re-resolution (2.14);
  values marked inline are folded at build time. A namespace can be cleared and
  replaced, so a project is never stuck with a default palette;
- **`@utility`** and **`@apply`** for project-defined utilities built from
  declarations;
- **`@custom-variant`** for conditions the framework can actually evaluate —
  state, colour scheme, size class, pointer coarseness, reduced motion — and a
  named error for one it cannot;
- **the default theme** ships vendored from the upstream project under its MIT
  licence, pinned to a stated v4 version, with the pin recorded where a reader
  looks for it and updated deliberately;
- **refused, with a diagnostic that says why**: directives that load JavaScript
  plugins or a JavaScript configuration file. This framework's build is Cargo
  and a build script; a Node toolchain is not a dependency it will acquire for
  styling, and a plugin ecosystem built on selectors could not be honoured
  anyway. The browser reset shipped by the upstream project is likewise not
  applicable: a native backend has no browser defaults to reset, and its
  starting point is the theme.

### What each backend owes

- **a style capability table**: for every property in the vocabulary, realized,
  approximated (with the approximation documented), or unavailable — the
  per-property form of 2.5, and the reason an application is told at build time
  rather than shown nothing at run time;
- **a unit mapping**: the host's own unit, how `rem` follows the host's text
  setting, and the rounding rule, with a conformance case asserting the table
  (`X-L3-16`). The terminal's pixel-per-cell ratio is the sharpest case and is
  stated in Milestone 38;
- **runtime token resolution**: a theme, colour-scheme, or text-scale change
  re-resolves and re-applies style to the existing native objects, with no tree
  rebuild — the same path Milestone 21's state variants already take (2.10).

### Deferred deliberately, and named so they are not assumed

- **container-relative variants** (`@container`), which depend on a parent's
  resolved size and therefore on a second pass after layout. The layout model
  can answer them (`C22-1`); the ordering work is not in this milestone;
- **relational variants** — a style keyed to an ancestor's or sibling's state —
  which are selector matching under another name and would need the machinery
  2.14 refuses. If they are ever added, they arrive as an explicit, typed
  relationship between nodes, not as a matcher;
- **arbitrary raw properties** with no typed equivalent, which would break
  vocabulary equality in the direction that matters.

### Shared with Milestone 53

- **one implementation, three callers**: `rustnative-style` owns the vocabulary,
  the parse, the lowering, and the diagnostics; the `rsx!` macro,
  `rustnative_build::compile_styles()` in the build script beside
  `compile_rsx()`, and the CLI link it, exactly as they link
  `rustnative-markup`;
- **diagnostics at compiler quality**, spanned to the class or declaration the
  developer wrote — in a `.rsx` file, inside `rsx!`, in a builder call, and in
  `app.css` — held by a compile-failure suite rather than by inspection;
- **`rustnative expand`** prints the style properties a class string lowers to,
  the same way it prints the builder form of a markup tree;
- **the equivalence suite** gains style cases: the utility spelling and the
  typed-property spelling of every documented style produce equal resolved
  values, and a property added to one without the other fails the suite
  (Milestone 41 owns it thereafter);
- **the documentation obligation**: styling examples appear in both spellings
  wherever they appear in both syntaxes, and the component library's entries
  carry both. A style property gains both spellings or neither.

**Done when** the equivalence suite covers every documented style property in
both spellings, an unknown class and an unrealizable property each fail the
build with a spanned diagnostic, `app.css` drives the theme with a live token
switch on Windows, `rustnative expand` shows the lowering, every backend's
capability table and unit mapping are recorded, and no documented style example
exists in only one spelling.

**Status (2026-09-24): implemented** — `crates/rustnative-style` (declaration
vocabulary, Tailwind CSS v4.1.13 utilities over the vendored default theme,
`app.css`, capability tables and unit mappings), `classes!`/`styles!` and
markup `class=`/`style=`, resolution in the core against theme tokens and the
environment, `compile_styles()`/`app_theme!()`, `rustnative expand --classes`,
and the Windows table read back from native objects. Deviation: the builder
spelling is `.with_class(classes!("…"))`. Owed by the deferred backends:
their own tables and unit mappings. See `BUILD_STATUS.md`.

**Depends on** Milestone 53 for the attribute surface it shares, and on
Milestone 39 for the size classes its responsive variants key to. Neither
dependency is deep enough to sequence it after them.

## Milestone 39 — Portable-surface obligations

Three families of competing framework fail in the same place: a portable API
shaped by the first host it was written for, which then has to be widened
under pressure once per additional host. This project is currently at exactly
the moment where that is cheap — one backend exists — and it is the last such
moment.

Satisfies: `D-PT-1`, `D-PT-2`, `D-TW-1`, `X-L3-3`, `M-OB-1`, `M-OB-3`,
`X-L5-2`, `X-L4-4`, `X-L1-2`, `X-L0-5`, `X-L0-6`; concepts `C15-1`–`C15-3`,
`C20-1`, `C20-2`, `C21-1`, `C22-1`, `C22-2`, `C24-1`, `C24-2`, `C49-1`,
`C65-1`, and the shape of `C68-1`.

- **a desktop-class affordance audit of the portable API** — window
  management, menus, shortcut maps, pointer hover and cursors, drag-and-drop,
  and multi-window state — with each one expressed as a capability that a host
  may answer negatively, rather than as a feature other backends are expected
  to imitate;
- **a reverse audit**: every place the current portable API encodes a Windows
  assumption, resolved by widening the contract (2.4) rather than by adding a
  conditional, and recorded so the next backend does not rediscover it;
- **right-to-left as a layout-model property**: start/end rather than
  left/right throughout the model, mirroring applied by the core, and the
  backend applying the host's own mirroring where it has one. This is a change
  to the layout model's vocabulary and is therefore the single most expensive
  item in this milestone to defer;
- **safe areas, display cutouts, foldable hinges, and split-screen** as layout
  model properties rather than per-application code, because two of the planned
  backends cannot render correctly without them;
- **permission states as a capability enum** — not-asked, granted, limited,
  denied, permanently-denied — with a portable request flow and a documented
  per-host mapping. A boolean capability cannot express what mobile hosts
  actually return;
- **a gesture arbitration contract** describing how the framework's own
  recognizers coexist with a host's, with the conflict cases enumerated — the
  scroll, text-selection, and system-edge conflicts are the ones users
  perceive as bugs;
- **thread affinity enforced** by the type system where the substrate allows
  and by a debug assertion everywhere else, never by prose;
- **one ownership module per backend**, with the host's memory-management
  convention written down and asserted in tests — the rule `rustnative-windows`
  already follows, made a precondition for every future backend;
- **a documented escape-hatch contract**: how application code obtains a host
  object, what it may do with it, what invalidates it, and what the framework
  guarantees afterwards (2.6 currently states the principle and nothing more);
- **a per-target panic and teardown policy**, including host and hardware
  restoration, verified by a test that panics deliberately. Milestone 38
  already treats terminal restoration as a correctness requirement; this
  generalizes that position to every host.

From the concept survey (`docs/ecosystem-analysis/concepts-*.md`), seven structural decisions, each of which fixes how every later
backend is written and therefore belongs here rather than later:

- **a typed environment** (`C15`): values that flow down the tree implicitly
  and can be overridden per subtree — theme, locale, layout direction, text
  scale, size class, colour scheme, reduced motion, posture, and service
  instances — fed from host traits by every backend so no component queries
  the host directly, with invalidation limited to readers of a changed key;
  plus *upward preferences*, a typed channel for a descendant to publish a
  value an ancestor reduces during layout or render;
- **a command model** (`C20`): an action with identity, label, icon, shortcut,
  enabled and checked state, bound by menus, toolbars, context menus,
  shortcuts, and host-level surfaces, routed through the focus chain by each
  host's own convention — so a disabled command is disabled everywhere and a
  menu's shortcut is always the shortcut that works;
- **adaptive-layout vocabulary** (`C22`): size classes per axis and
  container-relative decisions computed by the portable layout model from the
  constraints it already has, with posture and hinge geometry delivered as
  environment values;
- **per-property native mappers** (`C24`): every backend applies properties
  to host objects through a per-control table of typed appliers, which
  applications may extend or replace globally or per instance — the escape
  hatch at the granularity applications actually need, and a structure every
  backend is written in from the start;
- **the surface vocabulary** (`C49-1`): widget, live activity, tile,
  extension, instant application, companion device, tray or menu-bar extra,
  jump list, and taskbar progress as capabilities, so every backend answers
  them honestly from its first day (realization is Milestone 57);
- **platform-group crates** (`C65`): shared crates for groups of hosts that
  genuinely agree — Apple hosts, draw-list hosts — each defining trait
  contracts its members implement, decided before a group's second member is
  written so shared code is never copied between backends;
- **the shape of capability grants** (`C68`): 2.5's capabilities answer *does
  this host have it?*; nothing yet answers *may this part of the application
  use it?* Services become obtainable only through a scoped grant — paths,
  origins, windows, packages — so third-party packages receive only what they
  declare. The shape is fixed here because retrofitting it breaks every
  application that relied on ambient access; enforcement is Milestone 51;
- and **typestate as a design rule** (`C21`) for framework APIs whose states
  permit different operations, applied first to validated form values,
  capability-gated services, and escape-hatch host handles.

**Done when** a new-backend conformance checklist exists, the Windows backend
passes it, and every item on it names the test that proves it.

**Status (2026-09-24): implemented** on Windows and the headless backend;
the checklist is `docs/conformance/new-backend-checklist.md`, the
invalidation contract `docs/invalidation.md`, and the audits beside the
checklist. Owed by the deferred backends: their own checklist columns —
real safe areas and hinges, competing host gesture recognizers, and the
permission states Windows never produces. See `BUILD_STATUS.md`.

**Depends on** nothing. It is deliberately first.

## Milestone 40 — Interoperability and incremental adoption

Of the four structural asymmetries identified in the analysis, three favour
this project and one does not: accumulated ecosystem. It cannot be closed by
building faster, and the only strategy that has ever worked against it is being
usable *inside* what already exists. This is Tier 0 because embedding
constrains how a backend realizes its root — retrofitting it means rewriting
each backend's realization layer.

Satisfies: `X-INTEROP-1`, `D-LG-1`, `D-LG-2`, `M-AS-1`, `M-AS-2`, `M-MP-1`,
`M-MP-2`, `D-GX-1`, `M-EN-1`, `W-CL-2`, `W-MS-1`, `E-HAL-1`; concepts `C43-1`,
`C66-1`.

- **embedding, inward**: the tree realized into a caller-supplied host object —
  a window, a view, or a document node — that the framework did not create and
  does not own, participating correctly in that host's sizing, lifecycle, and
  teardown;
- **embedding, outward**: a foreign host object adopted as a leaf of our tree,
  measured through `IntrinsicMeasurer`, laid out and clipped by our layout
  model, and destroyed on the same rules as any other realized object;
- **library-only mode**: the application model, state, scheduler, services, and
  data layer compiled into an existing native application with no UI dependency
  at all, exposed through a generated typed interface per host language. This
  is the lowest-risk first step a team can take, and the one competing
  archetype that grew this way proves it works;
- **guest-runtime mode**: the runtime driven from someone else's `main` and
  initialization, owning neither startup, clocks, interrupts, nor the loop —
  required for vendor embedded SDKs, and the same mechanism that lets a
  server-rendered application mount as a handler inside an existing HTTP
  service;
- **host rendering-surface handoff**: a tree node that owns a host-native
  rendering surface, with documented lifetime, resize, DPI, and present
  semantics, laid out and clipped by our layout model — the shape an
  application with a rendering core and a native UI shell actually needs;
- **a documented adoption ladder** — library, then embedded subtree, then full
  application — with a worked example at each rung, because an adoption story
  that is not demonstrated is not an adoption story.

From the concept survey (`docs/ecosystem-analysis/concepts-*.md`):

- **one interface description for the library-only surface** (`C66`), with
  ownership and threading annotated, from which bindings for every supported
  host language are generated and tested — never written by hand per language;
- **export of a component as a web custom element** (`C43`), with typed
  attributes and properties, events surfaced as DOM events, and accessibility
  relationships preserved: the web rung of the adoption ladder, letting a
  RustNative component live in any existing page without that page adopting
  anything else.

**Done when** a sample pre-existing application on each shipped backend hosts a
RustNative subtree, a sample RustNative application hosts a foreign control,
and both are under test.

**Status (2026-09-24): implemented** on Windows — library-only mode
(`crates/rustnative-interop`: the `.ril` description, C/C#/Rust generation,
`rustnative bindgen`), embedding inward (`WindowsPlatform::embed`) and
guest-runtime mode (`start_external`), embedding outward (`Node::foreign`,
`register_foreign`), the surface hand-off contract with `WM_DPICHANGED`, and
the three-rung adoption ladder under test. Owed: the web custom element (Web
milestone B) and embedding on the deferred backends. See `BUILD_STATUS.md`.

**Depends on** Milestone 39's escape-hatch and ownership contracts.

---

**Web (2026-09-28): delivered** with the Web track — the web rung of the adoption ladder: a client component as a custom element (`C43-1`, `rustnative-web/tests/element.rs`).

## Tier 1 — continuous, and gating each backend's completion

## Milestone 41 — Guarantees and conformance suites

The project's strongest claims — native fidelity, deterministic lifetime, no
work after unmount, host-quality text and accessibility — are currently
implementation properties rather than guarantees. An unproven guarantee is
marketing; a tested one is the difference between this framework and the
archetypes that must re-implement what it inherits. This milestone converts
each claim into a suite.

Satisfies: `X-L3-1`, `X-L3-2`, `X-L3-4`, `X-L3-6`, `X-L3-8`, `X-L3-12`,
`X-L3-15`, `X-L2-1`, `X-L2-2`, `X-L1-1`, `X-L1-4`, `X-L0-2`, `X-L5-1`,
`W-FG-1`, `D-FP-1`, `D-FP-2`, `M-FP-1`, `D-WV-1`, `D-SD-2`, `X-L2-3`; concepts
`C09-1`.

- **syntax equivalence as a standing guarantee** (2.9): the equivalence suite
  Milestone 53 creates — builder against markup, with the markup compiled from
  a `.rsx` file and through `rsx!` — moves here and stays here, so that a node kind or
  modifier added later in one syntax and not the other fails the build rather
  than quietly making one surface smaller than the other. This is the only
  mechanism that keeps two authoring surfaces equal over years;
- **style equivalence, on the same terms** (2.14): the utility spelling and the
  typed-property spelling of every documented style resolve to equal values,
  and each backend's style capability table is asserted rather than described —
  a property the table calls realized is checked against what the host actually
  applies, and one it calls unavailable fails the build when an application
  declares it;
- **an invalidation contract**: a document stating exactly which subtrees
  re-render for each kind of change, and tests that fail when a change
  re-renders more of the tree than the contract allows;
- **render-cause tracing**: for any update, the framework can report the state,
  prop, resource, or effect responsible, with a component path — the diagnostic
  that competing archetypes provide through reflection and this one must
  provide deliberately;
- **the transient-state fast path as a contract** (2.10): text entry, scroll
  offset, animated values, and list windows mutate host objects with no tree
  pass, stated as a guarantee with tests rather than left as a practice;
- **scope-bound cancellation as a public guarantee**: no task observes or
  mutates state after its owner unmounts, proven per target rather than
  recommended;
- **native-object lifetime as a guarantee**, with leak detection as a CI gate
  rather than a future diagnostic;
- **modal-operation conformance**: the application continues to render,
  animate, and process scheduled work during host-driven resize, menu tracking,
  native dialogs, and drag loops — the behaviour that separates an application
  that runs inside the host's loop from one that merely coexists with it;
- **fidelity conformance per backend**, measured against the host's own
  first-party applications and never against our other backends: control
  appearance, focus visuals, scroll physics, text rendering, context-menu and
  drag conventions, and system settings — contrast, reduced motion, text
  scale, colour scheme — honoured without application code;
- **text conformance per backend**: complex scripts, bidirectional mixed text,
  grapheme clusters and emoji sequences, font fallback, line breaking in
  scripts without spaces, and caret and selection geometry, all exercised
  against the host's own stack so that delegation is proven rather than
  assumed;
- **layout conformance**: the same screens at several text scales and with
  pseudo-localized strings — length growth, accented forms, mirrored
  direction — asserting no clipping, no overlap, and no lost interactive
  targets;
- **accessibility assertions in CI** per backend — roles, names, states, focus
  order, live regions — plus a recorded manual pass with each host's own
  assistive technology before that backend is called complete;
- **the published comparison**: one reference application realized natively and
  compared against the self-drawing and embedded-engine approaches on
  assistive-technology support, input methods, automation, and system settings,
  with the methodology published alongside the results. The fidelity argument
  is only worth making if it is measured.

From the concept survey (`docs/ecosystem-analysis/concepts-*.md`): **the batching guarantee** (`C09`) as a tested property — every
state change caused by one message becomes visible in the same render, and no
render ever observes a partial set of them.

**Done when** every guarantee above names a test, and no backend is called
complete without passing the suite.

**Status (2026-09-24): implemented** on Windows and the headless backend —
every guarantee names its test in `docs/guarantees.md`; the shared suites
run on both backends through `ConformanceHost`; layout, fidelity, text, leak,
and modal-loop conformance run on Windows. Owed: a person's recorded
Narrator pass, the drag-loop case, the comparison's other columns, and the
deferred backends' columns. See `BUILD_STATUS.md`.

**Depends on** Milestone 45 for the headless backend the suites run on.

## Milestone 42 — Budgets

Every comparative performance claim this project can make is an assertion until
it is measured. The archetypes it beats on footprint are beaten only with
numbers, and the substrate advantage that makes those numbers possible is
squandered if nothing defends it against regression.

Satisfies: `X-L0-1`, `X-L0-3`, `W-RS-1`, `W-RS-2`, `W-SL-1`, `W-ED-2`,
`M-BR-2`, `D-WS-1`, `E-GUI-4`, `W-SH-1`, `M-OB-4`; concepts `C42-4`, `C62-1`,
`C62-2`, `C83-2`.

- **a budget file per target**, holding declared numbers rather than
  aspirations: cold start, resident memory, artifact size, frame-time
  distribution including the worst case, input latency, per-route client
  payload, per-request cold start, edge CPU time per route, embedded RAM and
  flash, container image size, and build time — clean and incremental;
- **measured in CI on every commit**, on a low-end reference device or profile
  wherever one exists, because the low end is the one that decides whether an
  application is usable;
- **a regression fails the build**, the same way a failing test does;
- **the published numbers are the measured numbers**: documentation quotes the
  budget file rather than adjectives;
- **build time is a budget too** — it is the tax this substrate pays and the
  one developers feel hourly. The markup and style compile steps are inside it,
  not beside it: a `.rsx` file and a class string cost compile time, and the
  budget is where that cost stays visible rather than being absorbed silently.

From the concept survey (`docs/ecosystem-analysis/concepts-*.md`), three kinds of budget the list above misses:

- **user-centric web metrics** (`C42-4`): largest content paint, interaction
  responsiveness, and layout shift, with zero layout shift as the target for
  framework-controlled content;
- **a startup phase model** (`C62`) traced on every backend — process start,
  runtime ready, first frame, first content, interactive — with each phase
  budgeted separately so a regression is attributable, and optional
  profile-guided release builds generated from a scripted startup run;
- **boot-to-first-frame** (`C83-2`) on embedded reference boards.

**Done when** a budget file exists per shipped target, CI enforces it, and no
performance claim in the project's documentation lacks a number behind it.

**Depends on** Milestone 45 for reproducible measurement harnesses.

**Status (2026-09-24): implemented** for Windows and headless:
- budget files in `budgets/`;
- `rustnative bench --check` enforced in CI;
- the startup phase model and a PGO build;
- a documentation test that no performance claim lacks a budget.

Web, edge, embedded, and device keys are owed with their backends. See
`BUILD_STATUS.md`.

**Web (2026-09-28): delivered** with the Web track — the web budgets (`budgets/web.toml`, `C42-4`) and the serverless ones (`budgets/serverless.toml`).

## Milestone 43 — The developer loop

The substrate chosen in section 2 gives this project almost every root-layer
advantage in the analysis, and exactly one structural disadvantage: the
edit-to-running loop. Frameworks on dynamic runtimes get live replacement and
introspection for free. This one has to engineer them — and the loop is the
pillar developers cite most when choosing between two otherwise equivalent
frameworks, so leaving it unaddressed loses evaluations before anything else is
examined.

Satisfies: `X-L7-1`, `X-L7-2`, `X-L3-10`, `X-L3-11`, `M-BR-4`, `E-PR-1`;
concepts `C55-1`, `C55-2`, `C56-1`, `C57-1`, `C58-1`–`C58-3`, `C59-1`, `C90-1`.

- **rebuild and restart with application state preserved**: state serialized
  before teardown and restored into the new process, so the developer stays
  where they were, against a declared wall-clock budget held in Milestone 42's
  budget file;
- **incremental rebuild that does not rebuild the world**, with the crate
  boundaries arranged so an application-level change recompiles the
  application;
- **optional dynamic-library reload** for the application crate on platforms
  that support it, behind the same command, as an upgrade rather than a second
  workflow;
- **on-device**: the loop works against a connected phone, a board, or a
  remote host — not only on the development machine. This is where the
  archetypes with the best loops are weakest, and where an embedded target
  makes the difference most visible;
- **board and device quickstart**: one command from a supported board to a
  running application, with flashing, logging, and restart included;
- **a measured first-hour target**: project creation to running on a device in
  three commands or fewer, documented and tested as a number rather than
  claimed;
- **editor assistance that does not stop at a syntax boundary**: in a `.rsx`
  file through `rustnative lsp`, and inside `rsx!` in a `.rs` file,
  completion offers the element's attributes, hover shows the builder method an
  attribute calls, go-to-definition reaches it, and a diagnostic is anchored to
  the attribute the developer wrote rather than to a macro call or a lowered
  file. A developer who chooses the
  markup syntax must not get a worse loop for it (2.9), and the same applies in
  reverse — neither surface is allowed to become the one with tooling.
  Style spellings are held to the same bar (2.14): completion and hover inside a
  class string, naming the properties a class sets and the token a value
  references, and a diagnostic anchored to the class rather than to the string
  around it;
- **theme editing without a rebuild**: a change to `app.css`'s token values
  re-resolves and re-applies style to the running application, because tokens
  are references rather than constants (Milestone 58). A change that adds or
  removes a utility is a rebuild, and the loop says which of the two happened
  rather than leaving it to be inferred from the delay;

From the concept survey (`docs/ecosystem-analysis/concepts-*.md`), the mechanisms that make the loop competitive rather than merely
fast:

- **previews and a catalogue** (`C55`): a component rendered in isolation from
  a preview declaration — in either syntax — across a configuration matrix
  (theme, locale and pseudo-locale, text scale, size class, direction,
  contrast), browsable in a catalogue shipped with the CLI, on the development
  machine's backend and on the headless backend;
- **development builds** (`C59`) per device target, loading the application
  crate as a separately rebuilt unit so a change reaches a phone or board
  without reinstalling or re-signing;
- **development services** (`C58`) derived from declared resource bindings and
  provisioned locally when absent, a continuous test mode running affected
  tests on save, and an in-application error overlay on every backend linking
  to source positions, `.rsx` positions included;
- **`rustnative generate`** (`C57-1`) for components with their preview and
  test, screens wired into navigation, services, and server resources, in both
  syntaxes;
- **a structural editing API** (`C56`) on the Milestone 53 language server —
  insert, move, set attribute, preserving formatting and comments — so a visual
  designer can be built on the markup tooling without a model of its own;
- **on-demand installation** (`C90`) of toolchains and board support.

**Done when** the loop's wall-clock time is in the budget file and is met on
every shipped backend, including at least one device target.

**Depends on** Milestone 39 (teardown policy) and the state contracts in
Milestone 47 for what "preserved" means.

**Status (2026-09-24): implemented** on Windows (local and remote host):
- `rustnative dev` applies live theme edits, and rebuilds and restarts with
  state preserved, within the `dev_loop_restart_ms` budget;
- the first-run target is budgeted;
- previews, the catalogue, and preview goldens;
- `rustnative generate`;
- editor assistance in both syntaxes, with structural editing;
- development resources, the error dialog, and `doctor --install`.

Owed: device targets (Milestones 35–37) and dynamic-library reload. See
`docs/developer-loop.md` and `BUILD_STATUS.md`.

## Milestone 44 — Inspection and diagnostics

Section 9's tooling list, made concrete and no longer deferred. Same reasoning
as Milestone 43: a compiled framework does not inherit introspection, so it
must expose it.

Satisfies: `X-L7-3`, `X-L7-4`, `X-L3-5`, `D-IM-1`, `E-RS-2`; concepts `C04-2`,
`C18-2`, `C24-2`, `C61-1`–`C61-3`, `C88-1`, `C92-1`.

- **one inspection protocol exposed by the runtime**, covering the declarative
  tree, the realized host objects and the mapping between them, state and props
  (readable and, where safe, editable), layout with a per-node explanation of
  *why* a node has the geometry it has, the same explanation for style — which
  token, which component default, which override, which state variant, and
  which class if the node was styled by one (2.14) — event and render tracing,
  task and scope inspection, and host-object lifetimes;
- **a transport that works locally, on-device, and remotely**, because the
  targets that most need inspection are the ones with no second screen;
- **an inspector client shipped with the CLI**, working against every backend;
- **an in-application overlay on the draw-list path** — tree, layout, events,
  frame cost — for hosts where an external client cannot attach, reusing
  Milestone 29's path rather than introducing a second renderer;
- **a reduced form for constrained targets** using deferred host-side
  formatting over a debug probe or serial line, so an embedded build pays
  almost nothing for diagnostics it can still answer;
- **capability and service diagnostics**: what this host advertises, what it
  refused, and why.

From the concept survey (`docs/ecosystem-analysis/concepts-*.md`):

- **record, replay, and time travel** (`C61`): input, messages, and service
  responses recorded with redaction rules; deterministic replay on the headless
  backend and on the originating one; stepping backwards through state history
  in the inspector; and conversion of a recording into a regression test.
  Message-only state change plus a replaceable clock and executor make this
  attainable here in a way frameworks with ambient mutation cannot match;
- device recordings that include sensor and service inputs and replay in the
  host-side simulator (`C88`);
- per-component render-or-skip reasons (`C04-2`), property-value provenance by
  precedence level (`C18-2`), and active mapper customizations (`C24-2`) in the
  protocol;
- scheduler, task, frame, and input events emitted to existing embedded trace
  formats and debugger kernel-awareness where available (`C92`).

**Done when** every backend answers the protocol, including terminal and
embedded in reduced form, and the inspector is part of the shipped CLI.

**Depends on** Milestone 39's audit for stable capability vocabulary.

**Status (2026-09-24): implemented** on Windows and the headless backend.
- One protocol is answered by both backends, over a token-authenticated
  transport.
- `rustnative inspect` is shipped.
- The overlay is drawn on the canvas path.
- Recordings replay deterministically and become tests.
- `inspect::compact` is the reduced form.

Owed with their backends: the terminal and embedded reduced forms, and the
`C88`/`C92` device items. See `docs/inspection.md` and `BUILD_STATUS.md`.

## Milestone 45 — Test infrastructure

Section 2.13 caps verification at the hardware this project has. A headless
backend lifts most of that cap for everything above the realization layer, and
it is the prerequisite for testing the application layer built in Tier 2 —
which is otherwise untestable without one machine per target.

Satisfies: `X-L7-5`, `X-L7-6`, `X-L7-7`, `X-L7-8`, `M-OB-2`, `M-OB-4`,
`E-GUI-2`; concepts `C11-1`, `C11-2`, `C14-1`, `C55-3`, `C60-1`, `C60-2`,
`C82-1`.

- **a headless reference backend** that realizes the tree into an inspectable
  model, so component, interaction, and golden tests run on any machine;
- **synthetic input dispatched through the real input path**, never a test-only
  shortcut, so what the test exercises is what the user exercises;
- **a deterministic test clock and executor** — the second implementations of
  the host-clock and executor contracts — so asynchronous behaviour is
  reproducible;
- **golden and visual regression tests** over realized output, per backend
  where the host allows capture;
- **a lifecycle conformance suite**: process death and restoration,
  configuration changes, deep-link entry arriving *during* restoration, and
  low-memory trim — the collisions, not just the individual events, because the
  collisions are what break in production;
- **a device and emulator matrix in CI**, including at least one low-end
  profile, running Milestone 42's budgets;
- **a host-side device simulator** for display-bearing embedded profiles: the
  same draw list rendered on a development machine with simulated display size,
  colour depth, and input.

From the concept survey (`docs/ecosystem-analysis/concepts-*.md`):

- **a test query API over the portable accessibility tree** (`C60`) — by role,
  accessible name, label, and state — used by component, interaction, and
  end-to-end tests on every backend, so tests survive refactoring and every UI
  test is also an accessibility check; a lookup failure lists what the tree does
  contain;
- **an exhaustive test mode** (`C11`) in which a task, effect, or outgoing
  message the test did not assert fails the test, with per-test overrides of
  clock, network, storage, randomness, and any application service through the
  service contracts;
- **every preview doubling as a golden test** (`C55-3`), so previews cannot rot;
- **a kill-and-restore test** for each destination's saved state (`C14-1`);
- **a hardware-in-the-loop runner** (`C82`) executing the suites on emulators,
  the simulator, and connected boards through debug probes, whose per-board
  results are the only basis on which a board is called supported (2.13).

**Done when** component, interaction, golden, and lifecycle tests for the full
application layer run on a machine that has none of the target hardware.

**Status (2026-09-23): implemented** as `crates/rustnative-headless` — the
headless reference backend, `HeadlessApp`'s synthetic input through
hit-testing and host focus rules, the accessibility query API, goldens and
Windows `PrintWindow` visual goldens, exhaustive mode, and the lifecycle
collision suite — plus `Lifecycle::LowMemory`, realized on Windows. Owed by
Milestone 37: the host-side device simulator and hardware-in-the-loop runner;
by Milestones 35–37: the device and emulator matrix. See `BUILD_STATUS.md`.

**Depends on** nothing outside the core; it should start early because
Milestones 41, 42, 46, 47, and 48 all consume it.

---

## Milestone 59 — The toolchain and service layer

Milestones 43, 50, and 52 built most of the layer between a repository and an
installed application: the developer loop with previews and generators, signed
staged updates with rollback, deployment adapters with revisions and traffic
splitting, capability packages with grants, the stability policy with codemods,
and a machine-readable description of the framework. All of it on Windows.

This milestone finishes that layer and fixes its contracts before the device
backends arrive, because a release process invented once per backend is the
most expensive kind of duplication this plan can still incur. It is the one
competitive surface where none of the root-layer advantages in section 2 help:
the mechanisms below are independent of substrate, and a framework that answers
"how do I ship this?" with a document of manual steps loses to one that answers
with a command.

Satisfies: `C93-1`–`C93-4`, `C94-1`, `C94-2`, `C95-1`–`C95-3`, `C96-1`,
`C97-1`–`C97-4`, `C98-1`, `C98-2`, `C99-1`, `C99-2`, `C100-1`, `C100-2`,
`C101-1`, `C102-1`, `C102-2`, `C103-1`, `C104-1`, `C105-1`, and the unfinished
halves of `C59-1`, `C63-1`, `C63-2`, `C64-1`, `C64-2`, `C90-1`.

- **the release train** (`C93`): one platform version covering the crates, the
  CLI, the templates, and the capability-package index, reported by
  `rustnative describe`; a published supported-host matrix per version —
  operating-system versions, device APIs, browser baselines, toolchain and
  MSRV — checked by `doctor`; `rustnative add` resolving the version compatible
  with the project's platform version rather than the newest published one; and
  `rustnative upgrade` moving the whole train, running the codemods, and
  printing what moved, what is held back, and why;
- **diagnostics** (`C94`): `rustnative doctor` extended from toolchain
  installation to the whole project — platform SDKs, dependency compatibility
  against the train, configuration against its schema, generated-native drift,
  and credential material — printing the fixing command for each finding, with
  a machine-readable report and a CI mode that fails above a declared severity;
- **profiles and credentials** (`C95`): named build profiles in
  `rustnative.toml` binding target, configuration variant, environment values,
  secrets, update channel, and distribution intent, built with `rustnative
  build --profile`; a credential contract with three backings — local store,
  external vault, remote service — so signing material lives where the
  organization requires rather than where a vendor prefers, with rotation and
  revocation documented; and secrets resolved per build, never written into the
  artifact, proven by a build-time check;
- **submission** (`C96`): `rustnative submit <target> --profile <name>` for each
  host with a store, with metadata, release notes, and phased-release
  percentage declared in the repository and review status reported back;
- **compatibility and channels** (`C97`): a fingerprint computed from the
  declared native inputs — the crate dependency graph, `rustnative.toml`, the
  generated manifests and entitlements, and the capability packages with their
  grants — recorded in every build and every update manifest, with delivery
  refused on mismatch; update channels decoupled from builds, so promotion
  repoints a channel instead of rebuilding; an embedded fallback payload in
  every build, so a withdrawn or failed update returns to a known-good state
  offline; and adoption reporting per release under Milestone 51's telemetry
  policy. This is the half of over-the-air updating that Milestone 50 did not
  build, and it is the half that makes it safe to automate;
- **pipelines** (`C98`): every release step a scriptable command with
  machine-readable output — build a profile, run a test matrix, package, sign,
  submit, publish an update, promote a channel, roll back — plus generated
  pipeline descriptions for at least two widely used CI systems, refreshed by
  `rustnative generate pipeline`. The framework owns the jobs; the team's own CI
  remains the runner, as with deployment (`W-DP-1`);
- **extension ergonomics** (`C99`): a native-module declaration — functions,
  properties, events, view types — from which the portable trait, the
  per-backend glue, the manifest contributions, and a headless test double are
  generated, with the boundary type-checked on both sides, which a serialized
  bridge cannot do; and autolinking, so adding a capability package with native
  code links it and merges its native configuration with no project edit;
- **one router for every target** (`C100`): native navigation, web URLs, the
  server route table, and each host's deep-link and universal-link
  configuration derived from one route table, with a static export map for
  file-only hosts;
- **the chore layer**, whose absence is discovered at the worst moment:
  launch assets — icons at every density and shape, launch screens, store
  graphics — generated per backend from one declared source and validated
  against each host's rules (`C101`); a shareable playground that compiles a
  snippet to the web target and runs it from a link, with documentation
  examples generated from the same sources as the guides so they cannot drift
  (`C102`); development-host discovery, scannable pairing, a relay fallback
  with a documented threat model, and fan-out to several attached devices
  (`C103`); `rustnative eject` and `rustnative diff-native`, so a team that
  must hand-edit a generated native project can see its drift and find the way
  back (`C104`); and `rustnative bundle explain`, attributing artifact size per
  crate, asset, and dependency with a delta against a baseline, so a failing
  size budget says what grew (`C105`).

**Done when** a device backend can be released end to end with no manual step:
`doctor` clean, a named profile built and signed with a credential the
developer never handled, submitted to its store, an update published to a
channel and refused by a build whose fingerprint does not match, rolled back to
the embedded payload, and the whole run reproducible from a generated pipeline
description.

**Depends on** Milestones 43, 50, and 52, each of which it extends rather than
repeats, and on the backend whose release it is exercising.

## Milestone 61 — Interaction depth: scrolling, gestures, and text

The interactions a user cannot name and notices immediately. Scrolling is a
system rather than a gesture, a keyboard that covers the field being typed into
is the most reliable one-star review in mobile software, and the text editing
surface is where every self-drawing framework accumulates its longest-lived
defects (`foundations.md` F2.2). This milestone is Tier 1 because a backend
that does not have these is not finished, whatever else it realizes.

Satisfies: `C111-1`–`C111-3`, `C112-1`, `C112-2`, `C113-1`, `C113-2`,
`C114-1`, `C114-2`, `C115-1`.

- **scroll systems** (`C111`): nested scroll coordination with a documented
  contract for which scroller consumes a gesture and when; snap points, sticky
  and collapsing headers, pull-to-refresh, and over-scroll behaviour as
  portable contracts realized by each host's own scroller; scroll restoration
  per navigation entry; and scroll position available to the timeline as
  animation progress;
- **system insets** (`C112`): keyboard, toolbars, and panels as layout
  properties composed with safe areas and gesture insets, animated with the
  host's own curve and duration rather than a guessed one, plus automatic
  scroll-into-view for the focused field with a documented policy and an
  override;
- **gesture physics** (`C113`): velocity tracking, host-conventional
  deceleration, rubber banding at bounds, and gestures that can be interrupted
  and redirected mid-flight; and cross-application drag and drop with typed
  payloads, file promises, and drop-target feedback;
- **the text editing surface** (`C114`): input traits and autofill content
  types as portable text-field properties — including secure entry and
  one-time-code fields, which is what makes password managers work — with the
  host's own edit menu, selection handles, magnifier, spell checking, and
  dictation used rather than reimplemented, and capability answers where a host
  has none;
- **non-pointer navigation** (`C115`): spatial focus navigation with documented
  resolution rules and per-node overrides, focus engagement for scrollable
  regions, and a focus treatment scaled by input class — one mechanism serving
  televisions, remotes, switch access, voice control, and full keyboard access.

**Done when** the interaction conformance suite passes on every shipped
backend: a form whose field stays visible as the keyboard animates, a
collapsing header driven by a nested scroller, an interrupted fling, a text
field that autofills a one-time code, and a screen fully operable by directional
input alone.

**Depends on** Milestones 10, 11, 12, 25, and 27, whose mechanisms it composes,
and Milestone 39's inset and layout vocabulary.

## Milestone 65 — Engineering discipline

The practices that keep this plan's own promises true as the codebase grows,
and that every application built on it inherits. Each one converts something
currently held by review and habit into something a build fails on.

Satisfies: `C128-1`, `C128-2`, `C129-1`, `C130-1`–`C130-3`, `C131-1`–`C131-3`,
`C132-1`.

- **the public API as an artifact** (`C128`): a committed snapshot — crate
  items beside the `describe` surface — diffed in CI, failing the build when
  the diff exceeds what the declared version bump allows, plus a written
  sealing policy (non-exhaustive enums, opaque handles, sealed traits) so
  additive change stays additive. This is what makes Milestone 52's stability
  policy mechanical rather than aspirational;
- **architecture boundaries as tests** (`C129`): section 2's invariants
  executable — the core free of platform dependencies (2.4), each backend's
  host access confined to its ownership module, no global mutable state on the
  render path — each failure naming the principle it breaks;
- **test health** (`C130`): flakiness detected by repeated runs and quarantined
  automatically with an owner and an expiry, timing history driving sharding,
  coverage reported and gated on changed code, continuous fuzzing of the markup
  and style parsers, manifest and package readers, and protocol decoders,
  property tests over reconciliation, layout, and merge functions, and
  sanitizer runs over every backend's unsafe surface;
- **supply chain** (`C131`): automated dependency update proposals gated by the
  full suite, advisory and licence checks as build gates, signed build
  provenance for every released artifact, a mirrored or vendored dependency set
  for offline and regulated builds, and a reproducibility check that an
  independent rebuild is bit-identical, with any unavoidable variance
  documented;
- **release health gating** (`C132`): declared thresholds over the guardrail
  metrics of `C126-2` — crash-free sessions, startup failures, hang and
  frame-drop rates — evaluated per rollout cohort, halting a staged rollout on
  breach and reverting on a severe one, for application updates, firmware
  updates, and feature flags alike.

**Done when** a breaking change cannot reach a minor release, a platform
dependency cannot reach the core, a flaky test is quarantined with an owner
rather than endured, an artifact can be rebuilt bit-identically, and a rollout
halts itself on a health breach.

**Depends on** Milestone 45 (deterministic tests), Milestone 50 (rollouts), and
Milestone 52 (policy and description).

## Tier 2 — before any public release

## Milestone 46 — Internationalization and localization

The largest single omission in this plan as it previously stood: the word does
not appear in it. Every competing archetype at every maturity level has an
answer, a framework without one is not viable for commercial software, and
retrofitting it touches every string, every layout, and every backend — which
is why it is the first Tier 2 milestone rather than a late one.

Satisfies: `X-L5-3`, `X-L5-4`; concepts `C41-2`.

- **typed message catalogues** with plural categories and grammatical gender,
  because placeholder substitution and string concatenation cannot express
  either, and applications built on them are re-engineered rather than
  translated;
- **compile-time-checked placeholders**: a message and its arguments are
  checked together, which is a capability the dynamic-substrate archetypes
  cannot offer, and which reaches both authoring surfaces identically — a
  message reference is an ordinary expression in a builder call and in a markup
  attribute, so a literal string left untranslated is the same lint in both;
- **extraction that reads both syntaxes**: the string extractor walks builder
  calls, `.rsx` files, and `rsx!` trees alike, because a catalogue that misses half the
  application's strings depending on how a screen was written is worse than no
  extractor;
- **locale-aware formatting delegated to host facilities** where they exist —
  numbers, dates, currencies, units, collation, and casing — rather than
  reimplemented per backend, consistent with the text-stack position in 2.2;
- **bidirectional text and mirroring** joined up with Milestone 39's layout
  work, so a mirrored locale is a layout-model outcome and not an application
  concern. Both style spellings are expressed in the same terms: the spacing and
  alignment utilities are start/end rather than left/right, so a screen styled
  with classes mirrors with the layout model instead of against it (2.14);
- **runtime locale switching** that updates the realized tree, including
  re-measurement, without restarting the application;
- **a translator workflow**: extraction, merge, context, and a way to see the
  string in place;
- **pseudo-localization in the dev loop**, wired into Milestone 41's layout
  conformance suite so growth and mirroring defects fail a test rather than
  surfacing in a translated build.

From the concept survey (`docs/ecosystem-analysis/concepts-*.md`): locale delivered through the environment (`C15`) so a switch
invalidates only its readers, and locale-aware web routing with generated
language alternates (`C41-2`).

**Done when** an example application ships in at least one right-to-left and
one plural-rich locale, switching at runtime, with the layout suite passing
under pseudo-localization on every backend.

**Depends on** Milestone 39 (mirroring) and Milestone 41 (conformance).

**Status (2026-09-24): implemented** on Windows and the headless backend.
`examples/i18n-demo` ships in English, Arabic, and Polish, switching at
runtime. The layout suite passes under the pseudo-locale on both backends.
See `docs/i18n.md` and `BUILD_STATUS.md`.

**Web (2026-09-28): delivered** with the Web track — HTML language alternates, `lang`, and `dir` on web routes (`C41`).

## Milestone 47 — State, resilience, and data

Four absences that together account for most of what an application actually
does between its UI and its host: shared state, error containment, the
asynchronous data layer, and forms. The data layer in particular is the
most-used third-party layer in every competing ecosystem and is portable to
every target — it is not a web feature.

Satisfies: `X-L4-1`, `X-L4-2`, `X-L4-3`, `X-DATA-1`, `X-DATA-2`, `X-DATA-3`,
`W-SF-7`, `M-FP-2`, `M-FP-3`, `D-MC-1`; concepts `C08-1`, `C08-2`, `C09-2`,
`C10-1`, `C10-2`, `C12-1`, `C12-2`, `C13-1`–`C13-3`, `C14-1`, `C16-1`, `C17-1`,
`C17-2`, `C29-1`, `C29-2`, `C30-1`, `C30-2`, `C31-1`, `C31-2`, `C34-1`–`C34-3`,
`C36-1`, `C87-1`.

- **a shared/scoped state contract**: typed, observable, scoped to a subtree
  rather than global, with defined update ordering and the same lifetime
  discipline as component state — the answer to "two distant components need
  the same data" that currently requires lifting everything to the root;
- **a state history contract** giving undo/redo, which is nearly free once
  updates are ordered and is a recurring application requirement;
- **error boundaries**: a subtree may fail, be contained, present a fallback,
  and be retried, with the failure reported through Milestone 44's diagnostic
  channel rather than lost;
- **the asynchronous data layer**: typed queries keyed by identity, declared
  cache lifetimes, request deduplication, background revalidation, retries with
  backoff, pagination and incremental loading, optimistic updates with
  rollback, and invalidation that composes with the component lifecycle and
  with structured task scopes;
- **offline mutation queueing** with an explicit conflict policy, on every
  target that has durable storage;
- **schema migration for persisted state**, with up and down paths and a dry
  run, so Milestone 30's persistence survives an application update;
- **one validation and forms model shared by client and server**: one schema,
  one set of error messages, checked at compile time, with two-way binding
  ergonomics, dirty tracking, and a submission lifecycle;
- **constrained background work** as a portable service contract — network,
  charging, and deadline constraints — because both mobile hosts impose it and
  neither can be worked around;
- **asset and image loading** with decode, downscale, memory and disk caching,
  and cancellation tied to component lifetime.

From the concept survey (`docs/ecosystem-analysis/concepts-*.md`), the details that separate an application layer from a feature list:

- **derived values** (`C08`) — pure functions of state, cached, recomputed only
  when their inputs change by value, visible in the inspector — and slice
  subscription so a component observing part of a shared store is invalidated
  only by that part;
- **a move-based "prepare off the UI thread, apply atomically" pattern**
  (`C09-2`) for large updates;
- **a state-machine pattern on enums** (`C10`) whose entry and exit effects are
  tied to task scopes, so leaving a state cancels its work, with optional
  diagram generation from the type;
- **stream collection as a component primitive** (`C12`), bound to the
  component's scope, suspended with it, delivering values as messages, with
  hot/cold and backpressure semantics stated for every framework stream;
- **navigation as typed state** (`C13`) from which the host navigation stack is
  reconciled — deep links, restoration, and programmatic navigation all
  construct that value — with typed query parameters on the web and state scoped
  to a destination's time on the stack;
- **a declared saved-state subset per destination** (`C14`), written to the
  host's restoration mechanism under a size budget;
- **service scopes** (`C16`) per application, window, destination, and request,
  with construction and disposal bound to the scope;
- **supervision** (`C17`): error boundaries are a supervision policy on a
  subtree — isolate, restart with backoff, escalate — and the same policies
  apply to task scopes, with the stated default that a child's failure does not
  cancel its siblings;
- **query results as an exhaustive state type** (`C29`) — loading, empty,
  failure, success, stale-while-refreshing — that components must match, and
  colocated data requirements batched by an ancestor into one request, each
  component receiving only its own slice;
- **stale-while-revalidate, specified** (`C30`): separate freshness and
  retention lifetimes; revalidation on focus, reconnect, mount, and interval as
  declared policy; hierarchical key invalidation; prefetch; cancellation when
  unobserved; retention-based collection; structural sharing so unchanged parts
  of new results keep their identity;
- **live queries over local storage** (`C31`) usable as virtualized list
  sources, and a documented repository pattern in which the network writes to
  local storage and the UI reads from it, with a gap-filling paging source;
- **an HTTP interceptor chain** (`C34`) — authentication and token refresh,
  retry, logging, caching — certificate pinning as declared policy, and declared
  endpoint interfaces producing typed clients;
- **a changeset type in the forms model** (`C36`): typed casting, per-field
  errors, storage constraint violations mapped back to their fields, and a
  validated output type distinct from raw input;
- **a long-running operation primitive** (`C87`) — goal, progress stream,
  cancellation, pre-emption, result — bound to a task scope and shown by
  standard progress components.

**Done when** an example application demonstrates cached, deduplicated,
paginated data with optimistic updates and offline queueing on at least two
backends, and a subtree failure is contained and retried without restarting the
application.

**Depends on** Milestone 45 (deterministic async in tests) and Milestone 20's
service contracts.

**Status (2026-09-25): implemented** on Windows and the headless backend.
`examples/data-demo` shows cached, deduplicated, paginated data with
optimistic updates and a durable offline queue. A failing subtree is
contained and retried without restarting. The same machinery runs on
native controls in `native::data_integration`. See `docs/data.md` and
`BUILD_STATUS.md`.

## Milestone 48 — Components, tokens, and visualization

Primitives are not a component set, and the path from a design system to
running UI is how applications are actually built. This is also where the
framework's realization strategy has to prove it can serve design-led teams
without abandoning host fidelity.

Satisfies: `X-UI-1`, `X-UI-2`, `X-VIZ-1`, `D-SD-1`, `E-GUI-3`; concepts
`C18-1`, `C19-1`, `C19-2`, `C20-3`, `C22-3`, `C23-1`, `C25-1`, `C26-1`–`C26-3`,
`C27-1`, `C28-1`, `C28-2`.

- **a component library** covering the controls applications need, realized
  natively per backend, with the accessibility semantics of each one
  documented and asserted, and with every component usable — and documented —
  as a builder call and as a markup element (2.9). A component library that is
  comfortable in only one syntax would silently make that syntax the default;
- **a design-token pipeline** feeding the existing theme system, with a
  documented token schema and an explicit split between semantic roles — mapped
  to host appearance — and absolute brand values applied as given. A token
  system that can only express absolute values cannot honour a host; one that
  can only express roles cannot express a brand. The pipeline's output is
  Milestone 58's token file rather than a format of its own, so a design system
  exported from a design tool and a theme written by hand are the same artifact,
  and both spellings read from it;
- **charting and data visualization** built on Milestone 29's draw-list path,
  with an accessible alternative for every visual encoding rather than an
  image with a label;
- **a documented hybrid pattern**: custom-drawn subtrees inside a natively
  realized tree, with the portable accessibility model still supplying
  semantics for the drawn region — the combination neither a pure native nor a
  pure self-drawing approach offers;
- **a constrained text profile** for embedded targets that declares which
  scripts it supports, so the limitation is stated rather than discovered.

From the concept survey (`docs/ecosystem-analysis/concepts-*.md`):

- **a headless behaviour layer** (`C19`) for composite controls — list
  selection, combobox, menu, tabs, tree, grid navigation, date entry — carrying
  focus, keyboard, and accessibility semantics independently of appearance, on
  which the terminal, embedded, and custom-drawn controls and applications' own
  composites are built instead of reimplementing that behaviour;
- **container-owned typed layout data** for children (`C18-1`), in both
  syntaxes;
- **a per-host idiom table** for every control (`C23`), including behavioural
  differences such as button order, dismissal, and destructive-action placement;
- **an adaptive navigation component** switching between bottom bar, rail, and
  sidebar by size class, and **a command palette** on the command registry
  (`C22-3`, `C20-3`);
- **matched-geometry transitions** (`C25`) keyed by a declared shared identity
  across navigation and state changes, using the host's own transition
  mechanism where one exists and respecting reduced motion;
- **list infrastructure** (`C26`): sort, filter, and group as non-copying views
  over a source; identity-diffed animated insert, delete, and move; and
  sectioned compositional layout (list, grid, carousel per section, with
  headers) on the virtualization of Milestone 28;
- **a document model** (`C27`): open, save, save as, revert, autosave, dirty
  state, recent documents, per-document undo bound to the command model,
  external-change detection, and one window per document, each mapped to the
  host's convention;
- **host content controls** (`C28`): embedded web content, media playback with
  system media controls and picture-in-picture, and camera preview as
  capability-guarded nodes, plus the documented pattern for adding more.

**Done when** an example application is built entirely from the component
library and a token set, and renders host-appropriately on every shipped
backend.

**Depends on** Milestone 41 (fidelity and accessibility conformance) and
Milestone 46 (mirroring and text growth).

**Status (2026-09-25): implemented** on Windows and the headless backend.
`examples/gallery` is built only from `rustnative-components` and one token
set. It renders on Windows as the host's controls, in the host's accent.
Web content (WebView2), the system media controls, and picture-in-picture
are owed, as is rendering on the other backends. See `docs/components.md`
and `BUILD_STATUS.md`.

---

## Milestone 54 — Responsiveness under load

Nothing elsewhere in this plan covers what happens when a render is
*expensive*. Every mature framework family reached the same answer —
prioritized, interruptible, visibility-aware work — and each had to retrofit it
onto a render path that was not pure or effects that were not separated. Here
the preconditions already exist: render is a pure function of state (2.8),
effects are separated (Milestone 19), and tasks are structured (Milestone 18).
It is Tier 2 because the first data-heavy application will expose its absence,
and the scheduler contract is cheaper to extend before Milestone 47 builds the
application layer on it.

Satisfies: `C01-1`–`C01-4`, `C02-1`–`C02-3`, `C03-1` (the contract), `C04-1`,
`C75-1`, `C76-1`.

- **update priorities** attached to the message or state change that causes
  them — immediate for input feedback, normal, and deferrable — rather than
  introduced as a second API;
- **interruptible reconciliation** for deferrable updates: work split at
  component boundaries, yielding to the host between frames, discarded when
  superseded, with the guarantee that one frame never mixes two versions of the
  same state;
- **deferred values and pending transitions**, so a component keeps showing
  previous content while new content is prepared, and can render that it is
  stale;
- **render purity enforced by type**: render receives shared access to state
  only, so a render that mutates does not compile;
- **suspendable task scopes** driven by visibility and host lifecycle — a scope
  can be suspended and resumed, with the rule for in-flight work (complete,
  cancel, or defer) stated per task kind — and offscreen subtrees (hidden tabs,
  collapsed panels, backgrounded windows) retained with their state at the
  lowest priority;
- **skipping by props equality**: a component whose props equal the previous
  render's is skipped, decided from the props type, with a report when a
  component's props cannot participate;
- **on constrained targets**, the executor running as one task at a declared
  priority inside a static-priority system and never above the application's
  real-time work, and frame pacing that stops completely when no animation,
  input, or pending work exists.

**Done when** a reference application filtering a large data set keeps input
latency within its Milestone 42 budget while the filtered view updates, on at
least two backends; a hidden screen performs no periodic work; and an embedded
reference board shows no periodic wake when idle, by measured current.

**Depends on** Milestone 45 (deterministic scheduling in tests) and the
host-clock and single-threaded executor core work.

**Status (2026-09-25): implemented** on Windows and the headless backend.
`examples/filter-demo` filters 200 000 rows. Keystrokes stay within the
Milestone 42 budget on both backends (`rustnative bench`), and a hidden
screen does no periodic work. An idle Windows window receives no messages.
The embedded measurement is owed with Milestone 37. See
`docs/responsiveness.md` and `BUILD_STATUS.md`.

## Milestone 60 — The visual pipeline

Everything between the tree and the photons. The framework currently reasons
about layout and animation but not about *frames*: which subtrees the host
composites, what a frame's deadline is on a display whose refresh rate varies,
what colour space a token is in, and how a subtree becomes an image for a test,
a share sheet, or a printer.

Satisfies: `C106-1`, `C106-2`, `C107-1`–`C107-3`, `C108-1`, `C108-2`,
`C109-1`, `C110-1`, `C110-2`.

- **compositing and damage** (`C106`): a portable vocabulary — promote, opaque,
  damage region — that each backend maps onto its host's compositor, with the
  fallback stated where a host has none, and with layer counts, promotion
  reasons, damage regions, and overdraw reported by the inspection protocol and
  drawable as an overlay;
- **frame pacing and latency** (`C107`): frames paced against the host's
  display signal including variable refresh rate, with the current rate as an
  environment value and a documented interaction with power and thermal state;
  input timestamped at host arrival so input-to-present latency is measured
  rather than estimated and budgeted in Milestone 42; frame-deadline
  attribution naming the component, task, or service call that overran; and a
  hang detector reporting an unresponsive loop through the diagnostic channel;
- **colour and display capability** (`C108`): a declared colour space for
  tokens and assets, converted at each backend boundary with linear blending
  where the host expects it, and display capability — density, gamut, dynamic
  range, refresh rate — as environment values that update when a window changes
  display;
- **materials** (`C109`): surface-role materials (chrome, sheet, popover,
  sidebar) mapped to each host's own material system, honouring
  reduce-transparency and contrast settings, with a declared fallback — the
  fidelity argument at its most visible, because a self-drawing framework can
  only approximate what a host-native one can request;
- **capture** (`C110`): subtree rendering to an image or document on every
  backend, shared by golden tests, share images, export, and print, plus a
  secure-content marker that excludes a subtree from host screenshots,
  recordings, and switcher previews.

**Done when** frame-deadline attribution and input-to-present latency are in
the budget file on two backends, a brand colour matches across a
standard-gamut and a wide-gamut display, a sheet uses the host's material and
degrades correctly under reduce-transparency, and a subtree marked secure is
absent from a screenshot.

**Depends on** Milestone 27 (timelines), Milestone 42 (budgets), Milestone 44
(inspection), and Milestone 54 (priorities).

## Milestone 62 — Media, capture, and files

The services applications reach for when they stop being screens and start
being products — and the most common reason a team drops out of a portable
framework into per-platform code.

Satisfies: `C116-1`, `C116-2`, `C117-1`, `C118-1`, `C119-1`.

- **audio sessions** (`C116`): a session contract — category, focus request and
  loss, ducking and interruption, route changes, background playback as a
  capability — so an application behaves correctly alongside everything else on
  the device, plus now-playing metadata and transport controls published to
  each host's lock screen, notification surface, and wearables;
- **capture pipelines** (`C117`): camera, microphone, and screen capture as
  pipelines rather than one-shot calls — device enumeration and selection,
  configuration, preview through a real host view, capture, disposal — each
  tied to the permission states of Milestone 39 and to the host's own privacy
  indicators and consent flows;
- **privacy-preserving pickers** (`C118`): photo and document pickers that
  return only what the user chose, scoped-access bookmarks with renewal, file
  providers that expose the application's documents to other applications, and
  file promises for deferred content — the correct answer to scoped storage,
  and one that usually removes a permission prompt entirely;
- **background transfer** (`C119`): resumable uploads and downloads that
  continue when the application is backgrounded or terminated, with progress
  reporting, metered and low-data policy, and completion delivered as an entry
  point into the application, mapped to each host's own transfer service.

**Done when** an example application records audio while honouring focus loss,
scans a code through the camera pipeline with correct privacy indicators,
imports a photo without requesting library permission, and completes a large
upload after being backgrounded and losing the network.

**Depends on** Milestone 39 (permission states), Milestone 47 (data layer), and
Milestone 48 (host content controls).

## Milestone 63 — Trust: identity lifecycle, integrity, consent, and portability

The obligations imposed by stores, regulators, and security reviewers. None of
them is a feature a user asks for; each of them can stop a release. They are
Tier 2 because the first commercial application built on this framework will
meet all four before it ships, and because retrofitting consent and backup
classification after data exists is far more expensive than declaring them.

Satisfies: `C120-1`, `C120-2`, `C121-1`, `C122-1`–`C122-3`, `C123-1`,
`C123-2`.

- **account lifecycle** (`C120`): in-product account deletion with a documented
  propagation path, session revocation across devices, and a device
  authorization flow for keyboard-less hosts, all in the authentication
  contract rather than left to each application; and sign-in through each
  host's credential broker where one exists, covering passwords, passkeys, and
  federated identity;
- **integrity** (`C121`): platform attestation as a capability with
  nonce-based replay protection, verified by the server model, with a
  documented observe, degrade, and block policy ladder and honest answers where
  a host cannot attest — so anti-abuse is a policy decision rather than a
  hard-coded exclusion of unusual devices;
- **consent and regional compliance** (`C122`): a typed consent contract in the
  environment — purposes, lawful basis, grant and withdrawal, and an auditable
  record — *enforced* by telemetry, analytics, and the data layer rather than
  checked by convention; tracking authorization as a per-host capability with
  store privacy labels, content ratings, and export declarations generated from
  the same declarations; and data-residency declarations the storage and
  deployment adapters can enforce and report;
- **portability and backup** (`C123`): every persisted store classified as
  included, excluded, or device-bound, honoured by each host's backup system
  and verified by a restore-onto-a-different-device test; and user data export
  and deletion as framework-supported operations over those stores, which is
  what makes the account-deletion path above implementable.

**Done when** an example application deletes an account end to end, refuses to
emit analytics without consent, carries generated privacy labels, and survives
a restore onto a different device with its credentials excluded and its data
intact.

**Depends on** Milestone 49 (authentication and the server model), Milestone 51
(telemetry and compliance evidence), and Milestone 47 (persistence and
migration).

## Tier 3 — with and after the Web track

## Milestone 49 — The server application model

The largest uncontested opening identified anywhere in the analysis: a
batteries-included application backend, in a compiled statically typed
language, attached to a native UI story, effectively does not exist. The
archetype that owns this space pays for it with a runtime floor and
production-surfaced type errors; the compiled archetypes that could take it
deliberately refuse to provide anything above routing.

Satisfies: `W-SF-1` through `W-SF-6`, `W-MF-1`, `W-EP-1`, `W-MS-1`; concepts
`C05-1`, `C05-2`, `C35-1`, `C35-2`, `C37-1`, `C38-1`, `C39-1`, `C39-2`,
`C40-1`, `C41-1`, `C52-2`, server-side `C54-1`.

- **a server application model** built on the *same* component, scheduler, and
  service contracts as the client — typed request handling, middleware,
  sessions, and error pages — rather than a second framework wearing the same
  name;
- **authentication and authorization**: session and token strategies, password
  and passkey handling, provider federation, and a policy model expressed in
  the type system rather than in runtime checks;
- **a data layer**: compile-time-checked queries, migrations with up and down
  paths and a dry run, connection pooling, and transaction scoping tied to
  request lifetime exactly as task scopes are tied to component lifetime;
- **durable background jobs and scheduled work**: queues that survive restart,
  retries with backoff, idempotency keys, and an inspection interface that
  answers Milestone 44's protocol;
- **secure-by-default request handling**: request-forgery protection, a strict
  content security policy, secure cookie defaults, rate limiting, request-size
  limits, and output escaping that cannot be accidentally bypassed. These are a
  documented checklist, not research, and shipping without them is a defect
  rather than a missing feature;
- **a generated administrative surface** derived from the schema and the policy
  model — a feature most teams need and nobody enjoys building;
- **typed server functions** with one definition checked at both call sites
  (specified in Web milestone H, completed here against a real backend);
- **layered typed configuration**: defaults, files, environment, and secrets,
  validated at startup, with no global mutable state and no secret cached in a
  process-wide value;
- **mountability inside an existing HTTP service**, sharing its listener,
  middleware, and configuration, so adoption does not require replacing a
  running service.

From the concept survey (`docs/ecosystem-analysis/concepts-*.md`):

- **server-only components** (`C05`) whose code is excluded from the client
  build, whose boundary props are checked for serializability at compile time,
  whose output is tree payload merged by the ordinary reconciler, and whose
  server-only dependencies cause a compile error, not a runtime one, if
  reachable from client code;
- **the Rust server ecosystem's established service and middleware
  abstraction** (`C37`) as the foundation, with typed extractors for framework
  values (session, principal, request-scoped services) — not a private
  pipeline, which would split the ecosystem this project most needs to join;
- **an API schema derived from handler and server-function types** (`C35`),
  with generated documentation, request validation, generated clients for other
  languages, and contract tests that fail the build when a published version
  breaks;
- **migrations generated from model changes** (`C38`), with rename prompts,
  attached data migrations, a dry run, and squashing;
- **data-layer authorization policies** (`C39`) declared next to the schema and
  enforced for queries and subscriptions alike, with a fixture-driven test
  harness; and client-side adapters for existing hosted backends, so a team can
  adopt RustNative's UI without changing its backend;
- **feature-driven defaults** (`C40`) for pool, health, metrics, and tracing,
  with a build-time report of what was configured and why;
- **typed per-route web metadata** (`C41-1`) — head, social cards, structured
  data — rendered server-side, updated client-side, with generated sitemaps and
  build-time validation;
- **passkey sign-in** (`C52-2`) and **server-side push sending** (`C54-1`).

**Done when** an example application serves authenticated, database-backed,
job-processing traffic from one codebase whose UI runs client-side,
server-rendered, and serverless without modification.

**Depends on** Web milestone H, Milestone 47 (data and forms), Milestone 40
(mounting).

**Status (2026-09-25): implemented** for the server and the Windows client.
`examples/server-demo` serves authenticated, SQLite-backed, job-processing
traffic. `examples/server-client`, a Windows application, consumes it
through typed server functions and a server-only component, from one
codebase (`examples/notes-shared`). The same view is also served as a page.
The Web track's shapes (browser client, serverless) are owed with Web H and
K. See `docs/server.md` and `BUILD_STATUS.md`.

**Web (2026-09-28): delivered** with the Web track — the browser client and the serverless shapes: `examples/web-notes` serves authenticated, database-backed, job-processing traffic whose UI runs client-side, server-rendered, and serverless without modification.

## Milestone 50 — Deployment, updates, and fleet operations

Shipping once is packaging; shipping repeatedly is a different problem, and
this plan currently has no answer to it on any target. Mobile teams treat
over-the-air updates as non-negotiable; a device fleet that cannot be updated
is a liability rather than a product.

Satisfies: `W-MF-6`, `W-MF-7`, `W-DP-1`, `W-DP-2`, `W-DP-3`, `W-SL-2`,
`W-SL-3`, `W-SL-4`, `W-ED-1`, `W-ED-3`, `W-SH-1`, `M-BR-1`, `E-MW-1`, `E-MW-2`,
`E-BL-1`, `W-IS-1`, `W-IS-3`, `W-HM-1`, `W-HM-2`; concepts `C42-1`–`C42-3`,
`C47-1`, `C47-2`, `C48-1`, `C53-1`, `C63-1`, `C63-2`, `C64-1`, `C64-2`,
`C81-1`, and model payloads of `C89-1`.

- **deployment adapters as a stable documented contract**, covering at least
  one static host, one long-lived server, one per-request function runtime, and
  one edge/WASM runtime, each with a local emulator that enforces the same
  limits locally as the host does in production;
- **`rustnative deploy`**, plus export of standard infrastructure descriptions
  so teams that already own their pipeline consume our output instead of
  replacing their tooling;
- **preview deployments, staged rollout, and one-command rollback**;
- **per-request host discipline**: execution deadline, memory ceiling,
  filesystem availability and durability, and payload limits exposed as
  capabilities; request-scoped task scopes cancelled at response, with a test
  proving no work outlives it;
- **edge storage and cache contracts** that expose each platform's distinctive
  consistency, expiry, and placement semantics rather than flattening them into
  a lowest common denominator (section 1 forbids the flattening);
- **incremental regeneration and response caching** with inspectable
  invalidation: cache state is queryable, never inferred;
- **container and embedded-Linux package output**, with a declared image-size
  budget, so orchestrated and on-premises deployment are ordinary rather than
  special;
- **mobile over-the-air updates** within each host's rules: signed payloads,
  staged rollout, rollback, version pinning, and a written statement of what
  each host permits and forbids — the rules differ per host and pretending
  otherwise is how applications get rejected;
- **firmware update** with A/B slots, image verification, automatic rollback on
  failed boot, and staged fleet rollout, delegating to an existing bootloader
  rather than writing one;
- **static and progressive shapes**: a route with no interactive subtree ships
  no client JavaScript at all, and forms, navigation, and submissions work
  before and without the client code;
- **single-artifact deployment**: a server binary with embedded assets and no
  separate asset pipeline to operate.

From the concept survey (`docs/ecosystem-analysis/concepts-*.md`):

- **native project files are generated build outputs** (`C63`) on every
  backend — never hand-edited, regenerated on upgrade — with a typed
  configuration-plugin hook through which capability packages declare
  permissions, entitlements, manifest entries, and native dependencies;
- **a shared build cache** for local and CI builds, and **remote build and
  signing** for targets that cannot be built locally (`C64`) — with 2.13
  preserved: a remote build is not a verification;
- **immutable revisions and percentage traffic splitting** (`C47`) in the
  adapter contract, and the rule that nothing unique is initialized outside a
  per-invocation scope, so snapshot-restore hosts are safe;
- **resource bindings** (`C48`) declared in project metadata, injected as typed
  service handles, and used by adapters to derive infrastructure and
  least-privilege permissions;
- **the web loading path** (`C42`): an image pipeline (resizing, modern formats,
  responsive sources, lazy loading, priority hints, reserved intrinsic
  dimensions), font subsetting with preload and metric-adjusted fallbacks, and
  route and data prefetch on hover and viewport entry under a data-use policy;
- **store-delivered dynamic asset and feature packs** (`C53`) driven from
  project metadata;
- **multi-image firmware build and signing** (`C81`) with anti-rollback
  versioning and a documented key-management procedure;
- **model assets** (`C89`) as a versioned payload type in the mobile and
  firmware update paths, with compatibility checked before activation.

**Done when** the same application can be deployed in every shape section 8
names, and updated after deployment on every target whose host permits it.

**Depends on** Web milestones J and K, Milestone 32's packaging, Milestone 42's
budgets.

**Status (2026-09-26): implemented for Windows and the long-lived server.**
This covers:

- immutable revisions with preview, percentage promotion, and rollback
  (`rustnative deploy local`);
- container, Kubernetes, and systemd export;
- single-artifact embedded assets, and tag-invalidated response caching;
- signed, staged desktop updates with automatic rollback;
- MSIX capabilities, and `.appinstaller` updates.

Static-host, per-request-function, and edge adapters are owed with Web
milestones J and K. Mobile and firmware updates are owed with Milestones
35–37. See `docs/deploy.md` and `BUILD_STATUS.md`.

**Web (2026-09-28): delivered** with the Web track — static, function, and edge deployment targets with their emulators, preview, promotion, and rollback, and `deploy export sam|spin`.

## Milestone 51 — Observability, security, and compliance

What makes a framework acceptable to the people who never read a benchmark:
the reviewer, the auditor, the operator on call. This is also where the
embedded profiles' non-negotiable obligations live, because a device that
cannot be diagnosed or recovered is not shippable.

Satisfies: `X-OBS-1`, `W-EP-2`, `W-EP-3`, `D-WS-2`, `E-SF-1`, `E-SF-2`,
`E-BL-2`, `E-OB-1`, `E-OB-2`, `E-OB-3`, `E-IOT-1`, `E-AI-1`, `E-RB-1`,
`D-CT-2`; concepts `C67-1`, enforcement of `C68-1`, `C69-1`, `C70-1`, `C77-1`,
`C77-2`, `C78-1`, `C80-1`, `C80-2`, and accelerator capabilities of `C89-1`.

- **crash capture with per-target symbolication**, structured logging, metrics,
  and tracing that spans the client/server boundary in one trace;
- **tree state at failure**: the inspection protocol's model captured with a
  crash, so "what did the user's tree look like when it broke?" is answerable;
- **opt-in, privacy-respecting telemetry** with a published data policy and no
  collection by default;
- **a stated threat model per target**: what the service layer exposes, how
  capabilities are scoped, what an escape hatch can reach, and what the update
  path trusts;
- **generated compliance evidence**: dependency inventory and licences, a
  software bill of materials, accessibility conformance output from Milestone
  41's suites, and privacy and permission manifests per target — generated from
  the build, never hand-maintained;
- **a stated certification posture** for safety-adjacent embedded work: what is
  and is not claimed, which practices are already in place (bounded allocation,
  no hidden global state, documented worst-case behaviour, coverage evidence),
  and what a future certification effort would require, with requirement
  traceability maintained as build output rather than prose;
- **embedded operational obligations**: power-aware scheduling with no periodic
  wake without a pending deadline and a measured idle current figure, watchdog
  integration with a safe-state panic path verified by a deliberate hang and a
  deliberate panic, and a bounded-allocation mode in which allocation failure
  is a handled outcome;
- **integration service contracts** for the device ecosystems this framework
  sits inside: messaging, provisioning and device identity, bounded-latency
  inference kept off the frame path with cancellation tied to component
  lifetime, and node-graph transports mapped onto the portable message model —
  each delegating protocol implementation to existing libraries rather than
  reimplementing it;
- **portable industrial services** where they genuinely are portable: printing,
  serial and device I/O, and database access, behind capability contracts.

From the concept survey (`docs/ecosystem-analysis/concepts-*.md`):

- **capability grants enforced** (`C68`): services obtainable only through a
  scoped grant, and third-party packages receiving only the grants they
  declare;
- **an optional isolated worker process** (`C67`) with a typed message
  boundary and per-backend sandbox restrictions, for applications that host
  untrusted content, with crash recovery reported to the supervising scope;
- **web security primitives on by default** (`C69`): nonce-based content
  security policy, subresource integrity, typed restrictions on dangerous
  sinks, cross-origin isolation, and permissions policy;
- **instrumentation through the vendor-neutral tracing and metrics standard**
  (`C70`) — renders, tasks, service calls, requests, jobs — with its semantic
  conventions, an application-chosen exporter, and trace context propagated
  from client to server;
- **pools, arenas, and high-water-mark reports** (`C77`) for framework
  structures on embedded targets, with capacities set from measurement;
- **the framework running unprivileged or supervised** (`C78`) in a
  memory-protected domain or as a supervised process, recovering its state
  after a supervised restart;
- **a power-loss-resilient state store** (`C80`) with Milestone 30's atomicity
  contract restated for power loss and verified by power-cut testing, and
  partition layouts generated from project metadata with encryption where the
  hardware supports it;
- **accelerator availability as capability answers** (`C89`).

**Done when** a crash on any shipped target produces a symbolicated report with
tree state, and every artifact ships with generated compliance evidence.

**Depends on** Milestone 44 (protocol), Milestone 50 (artifacts).

**Status (2026-09-26): implemented for Windows and the server.** This
covers:

- tracing across the client and the server, with consent-gated export;
- crash reports carrying the tree at failure and a minidump
  (`rustnative crash`);
- grants enforced at each request;
- a low-integrity, job-limited isolated worker;
- a checksummed state store that survives a process killed mid-save;
- printing and serial ports;
- accelerator answers;
- `rustnative compliance`.

The embedded obligations are owed with the embedded backends. See
`docs/observability.md` and `BUILD_STATUS.md`.

**Web (2026-09-28): delivered** with the Web track — the permissions policy from declared capabilities (`C69-1`) and the Web threat model (`docs/security/threat-model-web.md`).

## Milestone 52 — The project around the framework

The layer that decides whether anything above gets a second project. Two items
here are direct answers to the loudest complaints about the incumbent
archetypes — migration pain, and ecosystem lock-in — and one is a response to
how a growing share of code is now written.

Satisfies: `W-MF-8`, `X-DOC-1`, `X-DOC-2`, `X-ECO-1`, `X-ECO-2`, `E-RS-3`,
`E-PR-2`, `D-CT-1`, `E-RS-1`, `E-EC-1`, `E-EC-2`, `E-HAL-2`, `E-K-3`; concepts
`C03-1`, `C57-2`, `C57-3`, `C71-1`, `C71-2`, `C73-1`, `C79-1`.

- **a published stability and deprecation policy** with a support window, plus
  automated migration for every breaking change. The archetype that dominates
  its space is complained about for exactly this, and the complaint is
  answerable;
- **task-oriented guides and a published generated API reference**, with a
  runnable example per subsystem and per supported board — every one of them in
  both syntaxes, presented side by side rather than in separate builder and
  markup editions, so that neither becomes the documented path and the other
  the appendix (2.9) — and, where an example styles something, in both style
  spellings on the same terms (2.14);
- **a third-party capability package contract**: a community-authored service
  or control implementing a portable contract with per-backend code,
  discoverable and versioned, usable without forking the framework. Without
  this there is no ecosystem, and the one asymmetry running against this
  project stays open;
- **a machine-readable description of the framework** — component and service
  contracts, capabilities, events, layout semantics, the element/attribute
  vocabulary of the markup syntax with the builder method each attribute calls,
  and the utility vocabulary with the style properties each class sets — so
  code-generating tools produce correct code rather than plausible code, in
  whichever syntax and spelling they are asked for. This is increasingly decisive and is
  cheap to maintain if it is generated from the same source as the API
  reference;
- **a stated non-duplication policy toward the embedded ecosystem**: the
  `Executor` contract demonstrated against an existing embedded async
  executor, peripheral access and HAL traits consumed rather than reimplemented,
  probe tooling reused, subsystem capability mapping for at least one
  configuration-driven RTOS ecosystem, and its board description consumed by
  `rustnative` rather than duplicated;
- **documented core cost characteristics**: stack, heap, and worst-case timing
  for the core's hot paths, measured rather than estimated;
- **the one-stack span demonstrated**: a single application genuinely built for
  a desktop and for a device, because the claim that the same model spans both
  is the project's strongest differentiator and the easiest to disbelieve.

From the concept survey (`docs/ecosystem-analysis/concepts-*.md`):

- **codemods** (`C57-2`) shipped with every breaking release and run by
  `rustnative upgrade`, tested against a corpus of example applications — the
  mechanism that makes the stability policy a promise kept by tooling rather
  than by users' labour — and **feature kits** (`C57-3`) generating working,
  tested authentication, commerce, and administration;
- **compatibility metadata** (`C71`) in every capability package — supported
  backends, required grants, framework version range — checked by `rustnative`
  and indexed, with package contributions scoped to what the package declares
  and never registered globally;
- **a published change-detection contract** (`C03-1`) naming the strategy this
  framework uses, what triggers invalidation and what does not, with a
  comparison for developers arriving from the four other strategies in use;
- **board metadata consumed from existing hardware descriptions** (`C73`) and
  an executor and clock adapter for a standard RTOS interface (`C79`);
- **the rejected concepts** recorded in `docs/ecosystem-analysis/
  concepts-delivery.md` (`C72`) kept current — among them any visual designer
  that writes a separate format or generated code as the source of truth
  (`C56-2`) — so a declined idea is found rather than re-proposed.

**Done when** a third party can ship a capability package, a team can upgrade
across a breaking change with tooling, and the span example runs on both ends
of the target range.

**Depends on** everything above it, which is why it is last.

**Status (2026-09-26): implemented for Windows.** This covers:

- the stability policy, with codemods run by `rustnative upgrade`;
- the capability-package contract, with `add` and `search` and a sample
  package;
- feature kits;
- `rustnative describe --json`;
- measured core costs, the change-detection contract, and guides;
- the span example on the desktop and at a device's screen size.

The device end of the span, and the embedded adapters, are owed with
Milestone 37. See `BUILD_STATUS.md`.

---

## Milestone 55 — Reconciliation beyond the screen: real time and sync

Three ideas the concept survey surfaced are this framework's own core idea —
declare desired state, reconcile reality towards it — applied somewhere other
than a UI tree: local-first data replicated between devices and a server, a
server-held UI tree reconciled into a browser over a persistent connection, and
a device fleet reconciled towards a desired configuration. Each is how the
framework family that owns it wins; none of those families owns the mechanism
that unifies all three.

Satisfies: `C07-1`, `C32-1`–`C32-3`, `C33-1`–`C33-3`, `C84-1`, `C85-1`,
`C85-2`.

- **a sync service**: local-first reads and writes, background replication,
  server push, partial replication, and a declared conflict policy per
  collection (server authority, last-writer-wins, or a merge function), with at
  least one adapter; optional conflict-free replicated types for collaborative
  text, lists, maps, and counters, whose merge functions are property-tested
  for commutativity, associativity, and idempotence, and which run in the
  browser as a WebAssembly subtree so every client merges with the same Rust;
  and schema versioning for replicated data across clients at different
  application versions;
- **a server-interactive mode**: per-connection component trees on the server,
  events over a persistent connection, reconciler-produced diffs applied on the
  client by the ordinary reconciler, reconnection with state recovery,
  deployment draining, and optimistic client hooks for latency-sensitive
  interactions;
- **render mode per subtree** — static, server-interactive, client-interactive
  (generated JavaScript or WebAssembly), or automatic (server-interactive until
  the client code arrives) — with state
  transfer on a mode switch specified and tested;
- **channels and presence** as service contracts usable by every mode;
- **device desired state**: a typed desired/reported contract reconciled on the
  device, with a conflict policy and offline catch-up, sharing machinery with
  the sync service; messaging with explicit delivery-guarantee, retained-value,
  last-will, and persistent-session semantics; and a mapping between typed
  application state and standard device data models, with commissioning
  delegated to existing stacks.

**Done when** one collaborative example works offline on two devices and
converges; one server-interactive example survives a reconnect and a deploy
without losing state; and one device example converges to a desired
configuration after a period offline.

**Depends on** Milestone 47 (data layer), Milestone 49 (server model), and Web
milestone H.

**Status (2026-09-26): implemented** for Windows and the headless backend.
- `examples/collab-notes` converges after offline edits on two devices.
- `examples/live-counter` survives a reconnect and a deploy without losing
  state.
- `examples/device-desired` converges over MQTT after being offline.

The browser client is owed with Web milestone H. See `docs/sync.md` and
`BUILD_STATUS.md`.

**Web (2026-09-28): delivered** with the Web track — the browser live client and the render modes, with the automatic hand-over (`C07`, `C33`).

## Milestone 56 — Durable and event-driven execution

Web milestone K is request-shaped, yet most per-invocation workloads are events,
and business processes need execution that survives restarts. Rust async
functions are already state machines and structured scopes already model a
step's lifetime, so the determinism durable execution depends on can be
enforced by type — a guarantee the dynamic substrates can only lint for.

Satisfies: `C17-1` (server and device processes), `C44-1`, `C44-2`, `C45-1`,
`C45-2`, `C46-1`, `C87-1` (across the client/server boundary).

- **event handlers** as a serverless entry point: a standard event envelope,
  batching, partial-failure reporting, retries, dead-letter routing,
  idempotency keys and deduplication, and the same invocation-bounded task scope
  as requests;
- **durable workflows**: steps with recorded results, replay on restart,
  durable timers, external signals, human-approval waits, and compensation,
  with versioning rules for in-flight executions — and non-deterministic
  operations reachable only through the workflow context, so a
  non-deterministic workflow does not compile; at least one engine adapter;
- **stateful actors**: identity, single-instance serialized execution, private
  durable storage, and alarms, with an edge adapter and a single-process local
  implementation for development;
- **supervision** for long-lived server and device processes, and long-running
  operations whose progress and cancellation cross the client/server boundary.

**Done when** a workflow killed mid-step completes with each step executed
exactly once; an event handler processes a batch with partial failures
correctly; and an actor-backed collaborative session runs on the local
implementation and on one edge adapter.

**Depends on** Web milestone K and Milestone 49.

**Status (2026-09-26): implemented** on the local implementation.

- A killed workflow completes with each step executed exactly once
  (`examples/workflow-crash`, which uses a real process abort).
- An event batch with partial failures is settled event by event.
- An actor-backed collaborative session runs on `LocalActorSystem`.

The edge adapter is owed with Web milestone K. See `docs/durable.md` and
`BUILD_STATUS.md`.

**Web (2026-09-28): delivered** with the Web track — event handlers as a function's entry point (`events::sqs_batch`, `C44`) and actors on an edge host (`edge::serve`, `C46`).

## Milestone 57 — Surfaces beyond the main window, and product services

Cross-platform frameworks are most often abandoned at the moment an application
needs a widget, an extension, push, purchases, or secure storage and must drop
to native code to get them. The vocabulary for these surfaces lands in
Milestone 39 so every backend answers it from the start; this milestone
realizes it.

Satisfies: `C49-2`, `C49-3`, `C50-1`, `C51-1`, `C52-1`, `C54-1`.

- **widgets and tray or menu-bar extras** realized from a restricted subset of
  the portable tree on the backends that have them, with data shared with the
  main application through a declared store; **share and action extensions**
  receiving typed payloads; and every remaining surface in the Milestone 39
  vocabulary answered honestly per backend;
- **remote push**: registration, token rotation, topics, rich and actionable
  notifications whose actions arrive as messages, and background delivery;
- **commerce**: catalogue, purchase, entitlements, restoration, and
  subscription state through each host store's own billing, with server-side
  receipt validation in the server model;
- **secure storage**: a contract backed by each host's protected store, with
  capability answers for hardware backing and biometric gating;
- **feature flags and remote configuration**: typed flags with compiled
  defaults, caching, offline behaviour, environment integration and
  invalidation, and local overrides for development and tests.

**Done when** a reference mobile application ships a widget, a share extension,
push with actions, an in-application purchase, secure token storage, and a
remotely toggled feature, with no application-authored native code.

**Depends on** Milestone 39 (vocabulary and grants), the relevant backends, and
Milestone 49 (receipt validation and push sending).

**Status (2026-09-26): implemented for Windows.** This covers:

- the tray icon and its menu, the jump list, taskbar progress, and
  notifications, answered through `Event::SurfaceAction`;
- secure storage (Credential Manager with DPAPI);
- typed feature flags with remote configuration;
- honest `Unavailable` answers for push and Store billing, which need
  package identity.

The mobile reference application is owed with Milestones 35 and 36. See
`docs/surfaces.md` and `BUILD_STATUS.md`.

## Milestone 64 — Product operations

The mechanisms a product team needs after the application works: links that
survive installation, guidance that teaches, instrumentation that does not
drift, and a failure trail that can be read without recording the user's
screen.

Satisfies: `C124-1`, `C124-2`, `C125-1`, `C125-2`, `C126-1`, `C126-2`,
`C127-1`.

- **links that survive install** (`C124`): platform association files generated
  from the route table (`C100-1`) and *verified in CI* against what the domain
  actually serves — a silent, common failure otherwise — plus a deferred
  deep-link contract so the first launch after installation can recover the
  link that caused it, through each host's privacy-preserving mechanism;
- **in-product guidance** (`C125`): onboarding, what's-new tied to version
  state, coaching marks, and targeted in-product messages in the component
  library, driven by feature flags and version history with frequency caps; and
  the host's review-request API behind a contract that enforces its quota and a
  documented policy for the moment it is asked;
- **a typed analytics contract** (`C126`): versioned events with typed
  properties and generated schema documentation, emission gated by the consent
  contract, sharing transport and batching with telemetry; and metric
  definitions — activation, retention, conversion, and the guardrail metrics
  release health depends on — declared once and read by both dashboards and the
  experiment system;
- **privacy-safe diagnostics** (`C127`): a production diagnostic trail that is
  sampled, redacted by default, consent-gated, retained locally, and attached
  to crash reports, sharing its format with the development recorder (`C61`) so
  a production report can be replayed on the headless backend.

**Done when** a link installs the application and lands on its content, a
what's-new screen appears exactly once per version, an analytics event cannot
be emitted without consent or without matching its schema, and a crash report
carries a replayable trail with no user text in it.

**Depends on** Milestone 44 (recording), Milestone 51 (telemetry), Milestone 57
(flags), and Milestone 59 (route-derived configuration).

## Milestone 66 — Reach: enterprise, shell, constrained networks, and the field

The last band of the ecosystem survey: the mechanisms that decide whether this
framework can be deployed where applications actually run — a managed corporate
fleet, a desktop shell, a metered network, and a production line.

Satisfies: `C133-1`, `C133-2`, `C134-1`, `C134-2`, `C135-1`, `C135-2`,
`C139-1`, `C139-2`, `C140-1`, `C141-1`.

- **enterprise** (`C133`): a managed-configuration layer per host with a
  published schema generated from the application's own configuration
  declarations, compliance reporting, and kiosk or single-application mode
  where the host offers one; plus enterprise and internal distribution paths in
  Milestone 59's release tooling, including offline licence validation. This is
  the segment where manageability decides the purchase and architecture rarely
  does;
- **desktop shell integration** (`C134`): file-type associations and protocol
  handlers declared in project configuration and registered by packaging, with
  per-user and per-machine installation handled and verified; single-instance
  behaviour with second-launch handoff; login and startup items; global
  hotkeys; taskbar and dock badges and progress; and power, sleep, and
  display-change events as portable contracts;
- **constrained networks** (`C135`): binary delta updates with a documented
  patch-chain policy, resumable fetch, verification before activation, and
  full-artifact fallback; and a network-aware update policy — unmetered only,
  charging only, bandwidth ceiling, deferral window — declared per channel and
  per host, because an update that does not complete is an update that never
  happened;
- **the field** (`C139`, `C140`, `C141`): a device time contract distinguishing
  synchronized, drifting, and unknown states with monotonic scheduling
  unaffected by wall-clock corrections; a calibration and factory-data store
  that survives updates and selective factory reset; offline-buffered
  diagnostics with a ring policy, a `support-bundle` command, and an
  authenticated, capability-gated remote console with a documented threat
  model; and provisioning contracts for per-unit data and credential injection
  at flash time, a manufacturing test mode built from the same application, and
  a factory-reset policy that preserves identity and calibration.

**Done when** an administrator can configure a deployed application without a
rebuild, a double-clicked document opens in the running instance, an update
completes over a metered link as a delta, and a device in the field can be
diagnosed and reset without a site visit.

**Depends on** Milestone 59 (release tooling), Milestone 50 (updates),
Milestone 51 (diagnostics), and the backends whose hosts each item belongs to.

## What these milestones do not change

Nothing in section 11 overrides sections 1–2. The architecture is unchanged:
Rust owns application semantics, the OS owns the native UI, every target is
first-class, the core stays platform-independent, capabilities replace platform
conditionals, a backend advertises only what it genuinely realizes, the
declarative tree has two equal spellings that produce the same tree, and style
is resolved in the core before a backend sees it. These
milestones exist because that architecture is necessary and not sufficient —
they are what turns a correct framework into a chosen one.

The differentiation argument that results, stated so it can be checked rather
than assumed:

1. **host-native realization on every target, from one application model** —
   proven by Milestone 41's conformance suites and published comparison, not
   asserted;
2. **no runtime tax anywhere on the range**, from a browser sandbox to a
   microcontroller — proven by Milestone 42's budgets;
3. **correctness properties that are guarantees rather than conventions** —
   scope-bound cancellation, deterministic native-object lifetime, and typed
   boundaries where others serialize — proven by Milestones 41 and 49;
4. **one seam count of one**: UI, state, layout, routing, persistence, data,
   forms, services, packaging, and deployment versioned as one product with one
   stability policy — Milestones 46 through 52;
5. **adoptable inside what already exists** — Milestone 40, the only clause
   that addresses the asymmetry running against this project, and therefore
   the one that must not slip;
6. **reconciliation as a general capability rather than a UI technique** — the
   same mechanism drives the UI tree, local-first data sync, server-interactive
   UI over a persistent connection, and device fleets — Milestone 55, whose
   done-when criteria are what test whether the claim holds.
