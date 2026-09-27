# Web Milestones A–K Implementation Plan (the Web backend)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan phase-by-phase. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Web backend — `PLAN.md` "Web backend — Web milestones A–K" — in full: Rust on the server, HTML/CSS and compile-time generated JavaScript in the browser, WebAssembly for the subtrees that opt in, in all three deployment modes (client-side, server-rendered, serverless), plus every item another milestone recorded as owed to the Web track.

**Architecture:** One new backend crate, `rustnative-web`, owns the browser's realization of the tree: the node→element mapping (B), the layout and style→CSS mapping (C), the HTML renderer (H), the client-component model and its runtime (A, D, F, G), browser services (E), WebAssembly subtrees, the service worker and manifest (I), and static export (J). A translator library, `rustnative-webgen`, compiles the client subset of Rust to JavaScript and is driven by the proc-macro crate `rustnative-web-macros` (`#[client]`, `#[server]`). `rustnative-server` depends on `rustnative-web` to serve pages, islands, and modules under its existing security pipeline, gains streaming bodies, and gains feature gates so its request pipeline builds for `wasm32-wasip1`; its serverless adapters (a function runtime and an edge/WASI runtime) run the same `AppService::handle`. The CLI builds, runs, tests, packages, and deploys all three modes. `rustnative-core` gains only portable widenings: builds for `wasm32-*`, time read through the host clock on every path, a typed extension slot in `Services`, and capability answers the browser needs.

**Tech Stack:** Rust 2024 (MSRV 1.85); `syn`/`quote`/`proc-macro2` (translator); `serde`/`serde_json`; `hyper`/`http`/`http-body-util` (server, streaming bodies); `tokio-tungstenite` (headless-browser driver over the DevTools protocol, and the live mode); `wasmi` (the local WASI/edge emulator, with fuel for CPU budgets); hand-written JavaScript runtime modules (no npm toolchain, no bundler); `wasm32-unknown-unknown` (browser subtrees, no `wasm-bindgen`: the framework emits its own glue) and `wasm32-wasip1` (the edge sandbox). Verified in headless Microsoft Edge (Chromium) through the DevTools protocol and with Node for unit tests of generated JavaScript.

**Spec:** `PLAN.md` §1, §2 (2.2, 2.3, 2.4, 2.5, 2.9, 2.13, 2.14), the Web backend section (milestones A–K), §9 (browser tests, equivalence); `docs/ecosystem-analysis/web.md` (`W-*`), `concepts-core.md` (`C05`, `C06`, `C07`, `C13-2`), `concepts-app.md` (`C33`, `C41`, `C42`, `C43`, `C44`, `C46`), `concepts-delivery.md` (`C58-3`, `C69-1`); the Web-owed lines of `BUILD_STATUS.md` (Milestones 40, 42, 46, 49, 50, 51, 55, 56).

## Scope decision (2026-09-27)

The user's instruction: *build the full web backend, making sure it covers everything in the docs.* Therefore:

- **In scope:** Web milestones A–K, every bullet; section 11's additions to H, J, and K; the Web halves other milestones recorded as owed (custom-element export `C43-1`, HTML language alternates `C41-2`, per-route metadata updated client-side `C41-1`, the web loading path `C42-1`…`C42-4`, web budgets, the permissions policy `C69-1`, the browser client for server-interactive mode and the browser render modes `C07-1`/`C33`, the web client half of server functions, static/function/edge deployment adapters with local emulators, serverless adapters for event handlers `C44`, an edge adapter for actors `C46`).
- **Verification bar (2.13):** everything runs on this machine. Browser behaviour is verified in headless Microsoft Edge (Chromium 1xx) over the DevTools protocol; Firefox and Safari are **not** available here and are recorded as owed, never assumed. Real cloud deployments (a managed function platform, an edge provider) are verified against their documented wire contracts through local emulators that enforce the same limits; a deploy to a real account is owed. Hardware-bound browser capabilities (Bluetooth, camera, sensors) are implemented and capability-answered; their end-to-end behaviour is recorded as unverified where the headless browser has no device.

## Global Constraints

- Everything in the 39–58 plan's Global Constraints still holds (core platform-free, MSRV 1.85, `deny(missing_docs)`, clippy pedantic clean, `cargo deny` clean, capabilities advertised only once realized, both syntaxes and both style spellings for anything new, the verification gate before each commit).
- `rustnative-core` gains **no** browser, DOM, or JavaScript knowledge. It gains: a `threads` feature (default on) that owns the Tokio multi-thread executor, so a `wasm32` build takes the core without it; time read through `Executor::now`/`Services::clock` on every framework path; `Services::with_extension`/`extension`; and new `Capability` variants. No `cfg(target_os)`/`cfg(target_arch)` item is added to the core.
- No JavaScript runs on the server. The browser receives HTML, CSS, JavaScript emitted at compile time (by `#[client]`) or shipped as the framework's fixed runtime, and `.wasm` modules for opted-in subtrees. Client and server exchange serializable data only.
- Strict CSP stays on. Nothing the framework emits needs `'unsafe-inline'` or `'unsafe-eval'`; styles are classes in stylesheets or CSSOM rules, never `style` attributes; a page with a WebAssembly subtree adds exactly `'wasm-unsafe-eval'`.
- A route with no interactive subtree ships no JavaScript; one with no WebAssembly subtree ships no WebAssembly (`W-IS-1`, selective attachment).
- One definition, two realizations, one test: wherever the same mapping exists in Rust (the HTML renderer, layout→CSS, number formatting) and in the JavaScript runtime (the DOM patcher, the class computation, `rn.fmt`), a test runs both over the same inputs and compares.

## Verification gate

The workspace gate (`rustnative-tools/gate.sh`) plus, for this track:

```bash
cargo check -p rustnative-core --target wasm32-unknown-unknown --no-default-features --features markup
cargo check -p rustnative-web --target wasm32-unknown-unknown
cargo check -p rustnative-server --target wasm32-wasip1 --no-default-features --features web
cargo test -p rustnative-web -p rustnative-webgen -p rustnative-web-macros     # includes Node and Edge tests
```

Browser tests locate a Chromium-family browser (`RUSTNATIVE_BROWSER`, else Edge, else Chrome, at their standard install paths). With none, each browser test prints `skipped: no browser` and passes — and `BUILD_STATUS.md` records where they last ran. Node-based tests do the same for `node`.

## Crates

| Crate | Kind | Milestones | Depends on |
|---|---|---|---|
| `rustnative-web` | library (backend) | A B C D E F G H I J | core, style, serde |
| `rustnative-webgen` | library (translator) | A | syn, quote, proc-macro2, rustnative-markup, rustnative-style |
| `rustnative-web-macros` | proc-macro | A H | rustnative-webgen |
| `rustnative-server` (changed) | library | H J K | + rustnative-web; features `serve`, `db`, `web` |
| `rustnative-cli` (changed) | binary | J K | + rustnative-web, wasmi |

`rustnative-web` does not depend on `rustnative-server`; the server depends on it. The client-side modes (static export) need no server crate.

---

## Phase 0 — Core readiness for the browser and the sandbox

**Design.**
1. **`threads` feature.** `tokio`'s `rt-multi-thread` moves behind `rustnative-core/threads` (default on). `TokioExecutor` exists only with it. `Scheduler::new()` keeps its current default with `threads`; without it the default is `HostExecutor` — a single-threaded, run-until-stalled executor whose tasks are polled by `ComponentTree::pump_tasks` (through a new `Executor::run_ready` hook, default no-op) and whose `sleep` resolves against its clock when the host pumps. This is the executor a browser subtree and a WASI request use.
2. **Time from the host.** `Application::dispatch_to_window`/`pump_tasks_for`, `ComponentTree::deliver_deferred_slice`, and `inspect::answer` read time from the scheduler's executor (`Executor::now`) instead of `Instant::now`. `SystemClock` stays for native hosts; a `wasm32-unknown-unknown` host supplies its own clock and executor, so no framework path reaches `std::time::Instant` there.
3. **Extensions.** `Services::with_extension::<T>(Arc<T>)` and `extension::<T>() -> Option<Arc<T>>`: typed, per-`Services` values (request-scoped services, the island registry of a page render).
4. **Capabilities.** New variants: `Http`, `WebSocket`, `IndexedStorage`, `OfflineCache`, `FileSystemAccess`, `Microphone`, `Sensors`, `Downloads`, `History`, `BackgroundWorkers`, `ServiceWorker`, `Installable`. Added to `Capability::ALL`, `rustnative describe`, and the committed `docs/api/framework.json`.
5. **wasm builds.** `getrandom` for the inspection token moves behind a feature the browser build does not take (the inspection server is a native-only feature: `inspect-server`, default on). Verified: `cargo check -p rustnative-core --target wasm32-unknown-unknown --no-default-features --features markup` and the same for `wasm32-wasip1`.

**Files:** `crates/rustnative-core/{Cargo.toml, src/scheduler/{executor.rs, mod.rs, host.rs}, src/application.rs, src/component/tree/responsiveness.rs, src/inspect/{answer.rs, server.rs, mod.rs}, src/services/mod.rs, src/capability.rs}`, `crates/rustnative/src/describe.rs`, `docs/api/framework.json`, Windows/headless capability tests.

**Produces:**
```rust
pub struct HostExecutor;              // HostExecutor::new(clock: Arc<dyn Clock>)
impl Executor for HostExecutor { fn run_ready(&self); .. }
pub trait Executor { fn run_ready(&self) {} /* new, default no-op */ }
impl Services { pub fn with_extension<T: Send + Sync + 'static>(self, v: Arc<T>) -> Self; pub fn extension<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> }
```

**Acceptance tests:** the core checks for both wasm targets without `threads`; a `ComponentTree` built on `HostExecutor` + `ManualClock` delivers a spawned task's message and a delayed one after the clock moves, with `pump_tasks` as the only driver; dispatch with inspection on and an injected clock records the injected duration; the headless and Windows capability tests still hold (neither advertises a new web variant).

---

## Phase 1 — Web milestones B and C: DOM realization, layout and style mapping, the HTML renderer

**Design.**
- **`WebNode`** — the browser's form of a node: `{ k, t, …content, cls: [..], a: {aria..}, d (disabled), h (hidden), c: [children] }`, serde JSON. It is the *one* format every browser path uses: the HTML renderer renders it, the JavaScript runtime realizes and patches it, generated client views build it, WebAssembly subtrees send it, and the live mode sends it. `WebNode::from_node(&Node, &mut StyleSheet)` is the only conversion from the core tree.
- **Element mapping (B)**, one table (`dom::element`), documented in `docs/web/dom-mapping.md`: Label→`<span>` (a heading role→`<h1>`–`<h6>`), Button→`<button type="button">`, TextInput→`<input>`, multiline→`<textarea>`, Column/Row→`<div>` (a `List` role with every child a `ListItem`→`<ul>`/`<li>`), Grid→`<div>` with CSS grid, checkbox/toggle→`<label><input type="checkbox">` (toggle adds `role="switch"`), radio→`<input type="radio">`, slider→`<input type="range">`, spinner→`<input type="number">`, progress→`<progress>`, select→`<select>`, list box→`<select size>`, date picker→`<input type="date">`, separator→`<hr>`, link→`<a>`, image→`<img>` with reserved intrinsic size, tab bar→`role="tablist"` of `role="tab"` buttons, canvas→inline `<svg>` rendered from the draw list, a native surface/foreign kind→`<div>` with its accessible name, Dialog role→`<dialog>`. Every element of an interactive subtree carries `data-k` (its key in its tree); root-owned keyed nodes outside islands keep `id` (Milestone 49 compatibility).
- **Layout→CSS (C)**, `css::layout`: Column/Row are flex containers; `SizeMode::Fixed/Fill/Auto` → `width/height`, `flex: 1 1 0`, content sizing; constraints → `min-*`/`max-*`; margins and padding as logical properties (`margin-inline-start`), so `dir="rtl"` mirrors with no application code; `Alignment` → `align-items`/`align-self`; `Overflow` → `overflow` (scroll containers); grid tracks → `grid-template-columns` with `fr`/`auto`/px; opacity, transforms (matched geometry uses the Web Animations API); viewport: `html, body` fill the viewport, the root fills `body`. Each distinct layout is one atomic class `l<fnv64>` over a canonical text; the JavaScript runtime computes the same class from the same canonical text (`rn.layoutClass`), tested against Rust.
- **Style→CSS (C)**: the theme's tokens → `:root{--…}` custom properties (dark tokens under `@media (prefers-color-scheme: dark)`), so a theme change is one re-resolution; per-kind theme defaults → `.rn-label{…}` etc.; each `DeclarationSet` → `.d<fnv64>` rules, with state variants as pseudo-classes, breakpoints as `@media (min-width)`, `dark:` as a colour-scheme query, `rtl:` as `:dir(rtl)`, `motion-reduce:` as `prefers-reduced-motion`; typed `VisualStyle` overrides → `.v<fnv64>`. `rustnative_style::WEB` answers every property `Realized` (shadows included) and `WEB_UNITS` records 1 logical px = 1 CSS px and `rem` = the root font size, which follows the browser's text-size setting.
- **HTML renderer (H's core)**: `html::render(&WebNode) -> String` with escaping that cannot be bypassed (text and attributes through one escaper); `html::Document` assembles head (metadata, `lang`, `dir`, language alternates), the stylesheet (nonce'd inline or linked), the page data island, and module scripts. `rustnative_server::render::page` is re-implemented on it, so one mapping remains.

**Files:** create `crates/rustnative-web/{Cargo.toml, src/lib.rs, src/node.rs, src/dom.rs, src/html.rs, src/css/{mod.rs, layout.rs, style.rs, sheet.rs}, src/svg.rs, src/hash.rs}`, `docs/web/{dom-mapping.md, layout-mapping.md}`; modify `crates/rustnative-style/src/capability.rs` (WEB, WEB_UNITS), `crates/rustnative-server/{Cargo.toml, src/render.rs}`.

**Acceptance tests:** every node kind and control renders to its element with its accessible name/role (golden per kind, both syntaxes); escaping of every text and attribute path; layout canonical text → CSS for every `LayoutStyle`/`ColumnStyle`/`RowStyle`/`GridStyle` field; declaration sets with each variant kind → CSS; the WEB capability table answers every property; the Milestone 49 render tests still pass through the new renderer; in Edge, a page of every node kind has the accessibility tree the framework's `AccessibilityTree` describes (roles and names, via `Accessibility.getFullAXTree`); in Edge, `dir="rtl"` mirrors a row's start/end padding.

---

## Phase 2 — Web milestone A: the client subset, its translator, and the runtime

**Design.**
- **Authoring.** A client component is an ordinary Rust module:
  ```rust
  #[rustnative_web::client]
  pub mod counter {
      use rustnative_core::{Event, Node, NodeId, rsx};
      use rustnative_web::Effects;
      #[derive(Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
      pub struct Counter { pub count: i32 }
      impl Counter {
          pub fn update(&mut self, event: Event, fx: &mut Effects<()>) { … }
          pub fn view(&self) -> Node { rsx! { … } }
      }
  }
  // `counter::Island` is a `Component` whose props are `Counter` — `<counter::Island key="c" count={3} />`.
  ```
  The macro leaves the Rust as written (so on Windows, headless, and the server it *is* the component), implements `ClientLogic` for the state type (its name, its JavaScript module, its CSS for the declaration sets it uses, its source-map line table, the server functions it calls), and defines `pub type Island = rustnative_web::Client<State>`.
- **The subset** (`docs/web/client-subset.md`): state of plain serializable types defined in the module (structs, enums with unit/tuple/struct variants, `Option`, `Vec`, `String`, `bool`, `char`, integers up to 32 bits, `f64`, `f32` rounded with `Math.fround`; 64-bit integers only as opaque values, rejected at render if outside ±(2⁵³−1)); `let`, assignment and compound assignment, arithmetic, comparison, `if`/`else`, `match` (literals, ranges, variants, `Option`/`Result`, bindings, guards), `while`/`loop`/`for` over ranges and collections, early `return`, `let … else`; `format!` (`{}`, width, fill, alignment, sign, precision); string and collection methods from a fixed list; closures inside iterator adapters and `retain`; inherent methods and free functions in the module; `rsx!` and the builder API for every node kind and modifier the mapping covers; `classes!`/`class=`; events (`Click`, `TextChanged`, `Toggled`, `ValueChanged`, `SelectionChanged`, `DateChanged`, `TabSelected`, `KeyDown`/`KeyUp`, `FocusGained`/`FocusLost`, `PointerDown/Move/Up/Cancel`, `Wheel`, `Composition`, `Clipboard`, `Lifecycle`); effects through `Effects<M>`.
- **Agreement rules**: integer arithmetic is checked on both sides — the macro emits the Rust as `checked_*` that panics on overflow (a debug build's behaviour, now in every build) and the JavaScript throws; integer division truncates; `as` casts saturate like Rust; `f64` display follows Rust's `Display` (no exponent, `inf`, `NaN`, `-0`), precision uses round-half-to-even on the exact binary value (implemented with `BigInt`); `len()` is UTF-8 bytes, `chars().count()` is code points, string ordering is by code point; `trim` is Rust's `White_Space`.
- **Diagnostics**: anything outside the subset is a compile error at that expression naming the three ways forward — `#[server]`, a WebAssembly subtree (`rustnative_web::wasm_subtree!`), or hand-written JavaScript (`Effects::js("module", "function", args)`).
- **Effects** (`Effects<M>`): `call::<F: ServerFn>(input, reply)`, `navigate(url)`, `back()`, `focus(key)`, `copy(text)`, `store(key, value)`/`load(key, reply)`, `notify(title, body)`, `share(..)`, `download(name, text)`, `publish(topic, value)`/`subscribe(topic, reply)` (page-shared state between islands), `js(module, function, args, reply)`, plus the capability bindings of milestone E. Natively each is carried out through `Services` from the generated `Component::render`; in the browser, through the runtime.
- **Runtime** (`rn.js`, hand-written, embedded): reads the page data island, imports each island's module, attaches (verifying the server's DOM against the client view: a mismatch is reported with its path as `rn:mismatch` and repaired by the client render — a bug report, never silent), delegates events at the island root, runs `update`, batches renders into one microtask, and patches the DOM keyed by `data-k`, preserving focus, selection, and scroll.

**Files:** create `crates/rustnative-webgen/{Cargo.toml, src/lib.rs, src/ty.rs, src/check.rs, src/emit.rs, src/expr.rs, src/stmt.rs, src/pattern.rs, src/view.rs, src/format.rs, src/diag.rs}`, `crates/rustnative-web-macros/{Cargo.toml, src/lib.rs}`, `crates/rustnative-web/src/{client.rs, effects.rs, islands.rs, runtime/{rn.js, fmt.js}}`, `docs/web/client-subset.md`, `crates/rustnative-webgen/tests/{subset.rs, js/…}`, `crates/rustnative-web-macros/tests/ui/*.rs` (compile-fail suite).

**Acceptance tests:** a corpus of subset programs is translated and run in Node against their Rust originals (same inputs, same outputs, JSON-compared) — arithmetic, overflow, division, casts, float formatting (a property test over random `f64`s against Rust's `Display` and `{:.N}`), string functions over non-ASCII input, collections, matching, closures; a compile-fail suite for each rejected construct, each naming the three ways forward; an island in Edge updates on click, input, and toggle, and its DOM after each step equals the server's rendering of the state the runtime reports; a render counter shows one patch per microtask regardless of how many events arrive in it.

---

## Phase 3 — Web milestone H: server rendering, attachment, streaming, and the server seam

**Design.**
- **Pages.** `rustnative_web::Page` — a head (`Head`, typed metadata with social cards and structured data, `C41-1`), a root component, and a render strategy (`Static`, `Server`, `Streamed`, `ClientOnly` — `W-MF-2`, declared per route in the one route table). `ServerApp::page(pattern, handler)` serves it; the handler is an ordinary typed handler (extractors, server-side data loading) returning a `Page`.
- **Per-request render.** A `ComponentTree` per request on its own thread with the request's `Services` (extensions carry the request: path, query, session, CSP nonce, host limits) and a `HostExecutor`; the renderer renders, pumps the request's tasks until quiescent or its deadline, and renders again — a render may await, deterministically. Nothing process-wide is on this path. The request's task scope is cancelled when the response (the last streamed chunk) is sent.
- **Islands.** `Client<S>`'s `render` registers `(owner, module, key, state)` in the render's island registry (a `Services` extension); `WasmSubtree<C>` does the same with kind `wasm`; a `Subtree` in a render mode registers `live`/`auto`. The HTML renderer marks each island root (`data-rn-i`), and the document writes the page data island and a module script only if there is at least one island — selective attachment decided from the tree (`W-MF-4`, `W-IS-1`).
- **Server functions from the browser.** `#[server]` on an `async fn` in a shared crate defines the `ServerFn` (path, input, output) and, with the crate's `server` feature, the handler registration; the generated client call (`fx.call::<F>`) posts JSON to `/_fn/<path>` with the request-forgery token from the page data. `docs/web/wire-format.md` documents the envelope and versioning (a `x-rn-fn-version` header; the server answers `409` with the version it speaks, and the runtime reloads). Server-only code behind the shared crate's `server` feature is unreachable from client code at compile time (`C05-2`, compile-fail test).
- **Streaming.** `rustnative_web::Pending` wraps a subtree with a fallback: the renderer flushes the shell with the fallback in place, keeps pumping, and streams each resolved subtree as a `<template>` plus a nonce'd one-line fill call. `rustnative-server` gains streamed response bodies.
- **Partial prerendering (`C06-1`).** A component that reads request state (`rustnative_web::request(&context)`) is recorded as dynamic; a route marked `Page::partial()` has its static shell rendered once and cached, and its dynamic holes streamed per request; `GET /_rn/explain?path=…` (development) and the inspector show which parts are static.
- **Progressive enhancement (`W-IS-3`).** Buttons inside a `Form` node render as a real `<form method="post">` with the request-forgery field, so submissions work without JavaScript; links are real links; navigation works without the runtime.
- **Render modes in the browser (`C07-1`, `C33`).** `Static`, `ServerInteractive` (the runtime's live client speaks `rustnative-sync`'s frames over WebSocket and patches from the trees it receives, with local echo for typing and reconnection with the session id), `ClientInteractive` (generated JavaScript or WebAssembly), and `Auto` (live until the client module has loaded, then the module takes over from the live session's last state snapshot).
- **Server-only components (`C05-1`)** render in pages and are merged by the reconciler; their code is behind the server feature.

**Files:** create `crates/rustnative-web/src/{page.rs, render.rs, stream.rs, pending.rs, request.rs, live.rs, runtime/live.js}`, `crates/rustnative-web-macros/src/server.rs`, `crates/rustnative-server/src/{web.rs, body.rs}`; modify `crates/rustnative-server/src/{app.rs, response.rs, lib.rs, functions.rs, head.rs}`, `crates/rustnative-sync/src/live.rs` (browser frames), `docs/web/wire-format.md`, `docs/web.md`.

**Acceptance tests:** a route with no island ships no `<script>`; an island route ships exactly its modules; a page whose root spawns a data task renders the loaded data deterministically across 50 runs; a `Pending` subtree's fallback is flushed before its data resolves (chunk timing observed on a real socket) and replaced in Edge; the request scope is cancelled when the response ends (a task that would outlive it is cancelled — test); a server function called from an island in Edge returns and updates the island, and is refused without the token; a form works with JavaScript disabled (Edge with script disabled); the partial route serves its cached shell and streams its hole; `Auto` switches to the client with the live state carried over, in Edge; a server-interactive subtree survives a dropped socket.

---

## Phase 4 — Web milestones D, F, G, and E: input, scheduling, navigation, and browser services

**Design.**
- **D.** The runtime maps DOM events to `Event`s: `click`, `input`, `change`, `keydown`/`keyup` (with `KeyCode` and modifiers), `compositionstart/update/end`, `pointerdown/move/up/cancel` (with pointer capture when the handler asks), `focusin`/`focusout`, `wheel`, `copy`/`cut`/`paste`; `Tab` order is DOM order, with `tabindex` only where the accessibility model makes a node focusable that HTML does not; ARIA only where no HTML element carries the semantics (`docs/web/dom-mapping.md` lists which). An event reaches the server only when the handler it triggers is a server call.
- **F.** Updates are batched to one microtask; effects run after the patch; a WebAssembly subtree may run in a Web Worker (`worker: true`) with its tree and events crossing a serializable message bridge; DOM work stays on the main thread.
- **G.** URL routing with the core `Router` on both sides (the server matches the request path; the runtime matches the URL): client-side navigation fetches the next page's body fragment and swaps it (keeping the runtime), `pushState`/`popstate`, deep links (every route is a real URL), typed query parameters (`C13-2`: `Query<T>` on the server, `fx.navigate` with `Route::build`), `visibilitychange`→`Lifecycle::Suspending/Resuming`, `online`/`offline` (an `Effects` subscription), `pagehide`→island state saved to `sessionStorage` and restored on return, and `persist` islands kept in `localStorage`.
- **E.** Each capability is an `Effects` binding in the runtime and, where the core has a service contract, a Rust implementation over the browser API for WebAssembly subtrees: fetch (`HttpService`), WebSocket, Web Storage (`StorageService`), IndexedDB, Cache Storage, Clipboard (`ClipboardService`), Notifications (`SystemService::notify`), File System Access, Web Share, geolocation, media devices, Web Bluetooth, sensors, permissions (`PermissionService`), downloads, history, service workers, and Web Workers. `rn.caps()` answers each per browser, security context, and permission; the page data carries the server's view (`HostLimits`, the permissions policy the response set).

**Files:** `crates/rustnative-web/src/{runtime/{events.js, nav.js, caps.js}, capability.rs, browser/{mod.rs, services.rs}}`, `docs/web/capabilities.md`.

**Acceptance tests (Edge):** typing with IME composition events produces the same final `TextChanged`; Tab moves through the islands' controls in DOM order; pointer capture keeps delivering moves outside the element; back and forward restore each page and its island state; a deep link renders the right route server-side and attaches; `document.visibilityState` changes deliver the lifecycle events; storage, clipboard (with the permission granted through the protocol), and fetch bindings round-trip; `rn.caps()` answers `false` for Bluetooth and sensors in the headless browser, and the application can ask.

---

## Phase 5 — Web milestone A (continued) and F: WebAssembly subtrees

**Design.** `rustnative_web::wasm_subtree!(Component, "name")` in a crate built for `wasm32-unknown-unknown` exports `rn_alloc`, `rn_init(state)`, `rn_dispatch(event)`, `rn_pump()`, `rn_view()` over linear memory; imports are the framework's own (`rn_now`, `rn_timer`, `rn_log`, and the service bindings), so no `wasm-bindgen` is needed and the glue is the framework's. Inside, a `ComponentTree` with `HostExecutor` over the browser clock renders the component; its view goes out as `WebNode` JSON and is patched by the same runtime. On the server the same component renders natively inside `WasmSubtree<C>` in its own tree, so node identities match the browser's. A `#[client]` component can be built either way, which is what the JavaScript-versus-WebAssembly equivalence test uses. The worker variant runs the module in a `Worker`.

**Files:** `crates/rustnative-web/src/{wasm.rs, browser/{export.rs, executor.rs, imports.rs}, runtime/{wasm.js, worker.js}}`, `examples/web-notes/subtree/` (a CRDT-merge subtree, `C32-2` in the browser).

**Acceptance tests:** the example subtree builds for `wasm32-unknown-unknown`; in Edge it loads lazily (only on a page that has it), attaches to the server markup without a mismatch, handles events, runs a timer through the host clock, and runs in a worker; the same `#[client]` component as generated JavaScript and as WebAssembly produces identical DOM after the same event sequence.

---

## Phase 6 — Web milestone I: service workers and offline applications

**Design.** The build writes `sw.js` (versioned by the build hash): precache of the build's asset manifest, network-first pages with an offline fallback, cache-first hashed assets, a queue of server-function calls made offline replayed through Background Sync where available (and on `online` otherwise), and an update flow (the new worker waits; the runtime offers `rn:update`; accepting calls `skipWaiting` and reloads with island state kept). `manifest.webmanifest` from `[web.pwa]` in `rustnative.toml` (name, icons, colours from theme tokens, display, start URL); both linked only when configured.

**Files:** `crates/rustnative-web/src/{pwa.rs, runtime/sw.js}`, `docs/web/offline.md`.

**Acceptance tests (Edge):** the worker installs and controls the page; with the network emulated offline, a visited page and its islands still load; a server call made offline is delivered after reconnecting, once; deploying a new build shows the update and applying it keeps island state; `Page.getAppManifest` reports a valid, installable manifest.

---

## Phase 7 — Web milestone J: packaging, testing, and deployment tooling

**Design.**
- `rustnative build web --mode client|server|serverless [--host lambda|wagi]`: builds the application (native for `server`/`lambda`, `wasm32-wasip1` for `wagi`), builds every WebAssembly subtree, and — for `client` — runs the application's export entry point (`rustnative_web::run` in `--export <dir>`) to write every static route, the assets, `_headers` (the security headers for static hosts), the service worker and manifest, the sitemap, and the budget report.
- `rustnative run web`, `rustnative dev web` (rebuild and restart on save; the page reconnects over `/_rn/dev` and reloads keeping island state; build errors shown as an in-page overlay, `C58-3`), `rustnative test --browser` (the headless-browser harness: `rustnative_web::testing::Browser`, with accessibility queries over the browser's accessibility tree), `rustnative package web` (a single artifact: the server binary with embedded assets, `W-HM-2`).
- **Source maps** (development only): each generated module's map is built from the `line!()`/`file!()` of every translated statement, recorded by the macro; served beside the module in development builds.
- **Splitting and budgets**: one module per client component, loaded only by pages that contain it; `budgets/web.toml` sets per-route JavaScript+WebAssembly bytes and a startup budget measured on a throttled profile (CPU ×4, a slow network) independent of total application size; `rustnative bench --target web --check` and the J tests enforce them. User-centric metrics (`C42-4`) — largest contentful paint, interaction-to-next-paint, cumulative layout shift — are measured in the browser and budgeted, with zero layout shift for framework-controlled content (images carry their intrinsic size).
- **The web loading path (`C42`)**: an image pipeline (`rustnative_web::image`: resizing to responsive widths, WebP output, `srcset`/`sizes`, `loading="lazy"`, `fetchpriority`, reserved dimensions), font optimization (subsetting to the characters the build's text uses, `preload`, and metric-adjusted `@font-face` fallbacks), and route prefetch on hover and viewport entry bounded by `navigator.connection.saveData` and a per-page budget.
- **Custom elements (`C43-1`)**: `rustnative_web::custom_element::<S>("tag-name")` emits a module defining a custom element over a client component: typed attributes and properties map to state fields, the component's messages surface as DOM `CustomEvent`s, and it renders in light DOM so accessibility relationships (labels, `aria-*` ids) keep working.
- **Deploy (`W-MF-6`, `W-DP-1..2`)**: adapters in `rustnative_server::deploy` for a static host (the export directory plus `_headers`; emulated by `rustnative serve static`), a per-request function runtime (AWS Lambda's runtime API and its HTTP event format; emulated by `rustnative serve lambda`, which implements the runtime API), and an edge/WASM runtime (WAGI — CGI over WASI, as Spin and wagi run it; emulated by `rustnative serve wagi`, a `wasmi` host enforcing memory, fuel-metered CPU, and response-size limits). Each supports preview, staged promotion, and rollback through the existing `TrafficSplitter` locally, and exports its infrastructure description (a SAM template for the function runtime, a `spin.toml` for the edge runtime).

**Files:** `crates/rustnative/src/{web.rs, cli.rs, platform.rs, deploy.rs, bench.rs, dev.rs}`, `crates/rustnative-web/src/{export.rs, run.rs, image.rs, fonts.rs, element.rs, testing/{mod.rs, cdp.rs, a11y.rs}, runtime/{dev.js, prefetch.js, metrics.js}}`, `crates/rustnative-server/src/deploy.rs`, `budgets/web.toml`, `docs/web/deploy.md`.

**Acceptance tests:** each `build web` mode produces its artifact from the example; the static export serves through `rustnative serve static` and passes the browser suite; the per-route budget fails the build when a module is inflated; the startup measurement is unchanged when ten unrelated heavy routes are added; the source map resolves a thrown error's generated line to the Rust line; the custom element works inside a plain HTML page with no other RustNative code, and its event reaches the page; images get `srcset`, dimensions, and WebP variants; a font is subset; prefetch fires on hover and not under Save-Data.

---

## Phase 8 — Web milestone K: serverless and edge

**Design.**
- `rustnative-server` features: `serve` (hyper server, the Tokio listener, the local deploy adapter), `db` (SQLite: data, jobs, admin, row policies), `web` (pages). The request pipeline (`AppService::handle`), routing, security, sessions, tokens, server functions, and pages build for `wasm32-wasip1` with `--no-default-features --features web`.
- **Two runtime shapes.** `rustnative_server::serverless::lambda::run(app)` (a native binary: the runtime API loop, API Gateway v2 / function-URL events, and SQS-shaped event batches with `batchItemFailures` for event handlers, `C44`), and `rustnative_server::serverless::wagi::run(app)` (a `wasm32-wasip1` module: request from CGI variables and stdin, response to stdout). Both use a `HostExecutor` on the invocation's one thread — no Tokio runtime is created — and read configuration and secrets from the environment per invocation.
- **Stateless by construction**: a test builds the app, handles two requests, and asserts no state crosses them except through a service; a process-wide `OnceLock` on the render path fails a test that scans the path's statics.
- **Host limits as capabilities (`W-SL-2`)**: `HostLimits { deadline, memory, filesystem, response_bytes, payload_bytes }` in the request's services, filled by each adapter from its host (Lambda's deadline header, the WAGI emulator's configured limits), and asked through `rustnative_web::request(&context).limits()`.
- **Budgets (`W-SL-1`, `W-ED-2`)**: cold start, memory, and artifact size for both shapes measured in CI (`rustnative bench --target serverless`), and a per-route CPU budget in fuel under the edge emulator, with the documented strategy for a route that exceeds it (stream its shell and serve the hole from the long-lived server or a function).
- **Edge actors (`C46`)**: an edge adapter for `rustnative-durable`'s actors in the WAGI shape — one instance per actor id routed by the host, storage through a WASI-preview1 key-value import that the emulator provides (and a documented mapping to a real edge provider's storage), so the collaborative-session test runs on the local implementation and on the edge adapter.
- **The equivalence test (`W-MF-5`)**: one application, the same state, rendered client-side (static export), server-rendered, serverless (Lambda emulator), and serverless-edge (WAGI emulator), and with a subtree run as generated JavaScript and as WebAssembly — in Edge, the normalized DOM of each is identical, before and after the same interaction sequence. Promoted to a guarantee in `docs/guarantees.md`.

**Files:** `crates/rustnative-server/{Cargo.toml, src/lib.rs, src/serverless/{mod.rs, lambda.rs, wagi.rs, limits.rs}}`, `crates/rustnative/src/emulate/{mod.rs, lambda.rs, wagi.rs, static_host.rs}`, `crates/rustnative-durable/src/edge.rs`, `examples/web-notes/` (the application of Milestone 49's done-when), `crates/rustnative-web/tests/equivalence.rs`, `budgets/serverless.toml`.

**Acceptance tests:** both shapes build; the example answers through each emulator; a Lambda invocation past its deadline is refused cleanly; the WAGI emulator stops a route over its fuel budget and a module over its memory ceiling; no work outlives an invocation (a spawned task is cancelled at response); the equivalence test passes across all modes; the M49 done-when holds — `examples/web-notes` serves authenticated, database-backed, job-processing traffic, and its UI runs client-side, server-rendered, and serverless without modification (the database-backed parts in the serverless shapes through a data service over HTTP, since SQLite is not available in the edge sandbox — stated, not hidden).

---

## Phase 9 — Documentation, status, and the rest of the project

- `docs/web.md` (the guide: three modes, both syntaxes and both style spellings in every example), `docs/web/*.md` (mapping, subset, wire format, capabilities, offline, deploy), `docs/security/threat-model-web.md`, `docs/guarantees.md` (the equivalence guarantee), README's platform table and quick start, `docs/server.md`/`docs/sync.md`/`docs/durable.md`/`docs/deploy.md`/`docs/i18n.md`/`docs/interop/adoption-ladder.md` owed lines, `docs/conformance/new-backend-checklist.md` web column.
- `rustnative new --web` template; `Platform::Web.backend()` = `rustnative-web`.
- `PLAN.md`: a status note on each Web milestone and on the milestones whose owed items this track delivers; `BUILD_STATUS.md`: one entry per phase, with what was verified, on what, and what is owed (other browsers, real cloud accounts, device-bound capabilities).

## Execution order and commits

```text
Phase 0 → 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9
```

Each phase ends with the gate and a commit to `master` (no branch), with its `BUILD_STATUS.md` entry and `PLAN.md` note. Phases 1–2 may land as one commit if the translator's tests need the renderer.

## Self-review

- **Spec coverage:** every bullet of Web milestones A–K appears above as a design item with a test, or as an explicitly unverifiable item (other browsers, device hardware, real cloud accounts) recorded in `BUILD_STATUS.md` with the reason. Section 11's additions (typed seam, selective attachment, streaming, mismatch as a non-category, server-only components, partial prerendering, render mode per subtree; splitting and budgets, deploy adapters, single artifact) are in Phases 3, 7, and 8. The owed Web halves of Milestones 40 (`C43`), 42 (web budgets, `C42-4`), 46 (language alternates), 49 (browser client, serverless), 50 (static/function/edge adapters, `C42`), 51 (permissions policy), 55 (browser live client, render modes), and 56 (event handlers, edge actors) are in Phases 3, 7, and 8.
- **Placeholder scan:** none; where a design choice depends on a signature not yet written (the exact `Effects` binding list per capability), it is fixed at the start of that phase and recorded here.
- **Type consistency:** `WebNode` is the one browser tree format across renderer, runtime, generated views, WebAssembly subtrees, and live mode; `ClientLogic`/`Client<S>`/`Effects<M>` are the one client-component contract across generated JavaScript, WebAssembly, and native targets.
