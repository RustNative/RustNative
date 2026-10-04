# RustNative

A Rust-first, native-control-oriented cross-platform application framework.

The project is being developed around one core idea:

> **Write application semantics once in Rust; let each operating system provide the actual native UI and platform services.**

This is intentionally closer to the architectural philosophy of frameworks that drive real native host controls from a portable runtime than to a custom-rendered toolkit. The framework does not paint an imitation of every operating system. It maintains a declarative Rust UI/component model and realizes that model through native platform objects.

That model is written in one of two syntaxes, and they are peers. A **builder
syntax** of constructors and `with_*` modifiers, and a **markup syntax** —
elements, attributes, and nested children written directly as expressions in
`.rsx` files, the way anyone arriving from JSX or TSX would expect, or inside
the `rsx!` macro in an ordinary `.rs` file. Neither syntax wraps the other:
markup expands to the builder form at compile time, so both produce the same
tree, reach the same API, and cost the same at runtime. See
[Two syntaxes](#two-syntaxes).

## Current status

The working backends are Windows/Win32, Linux (GTK 4 on Wayland and X11; see
[`docs/linux.md`](docs/linux.md)), and the Web (semantic HTML and CSS,
in the browser, on a server, as a function, or on an edge host; see
[`docs/web.md`](docs/web.md)), with the headless reference backend for
tests. The framework core is designed to remain platform-independent so macOS, Android, iOS, terminal, and embedded targets are added as separate adapters, each planned to the same depth: native host objects, native measurement, native input, native accessibility, its own toolchain and packaging.

Two notes on what "planned" means here. macOS and iOS are fully planned platforms that this project has no hardware to build or verify on yet, so their milestones are specified and designed for but not started — order follows hardware, not priority. And a backend advertises a capability only once it genuinely realizes it, so "planned" never reaches an application as a claim of support.

Milestones 39–58 are built on the Windows backend (every milestone and
tier except the other backends, which come later and are recorded as owed in
`BUILD_STATUS.md`). Done so far: **Milestone 52 — the project around the framework** (a stability policy kept by codemods in `rustnative upgrade`, third-party capability packages with `rustnative add`/`search`, feature kits, `rustnative describe --json`, measured core costs, and guides; `docs/guides/README.md`), **Milestone 57 — surfaces beyond the main window, and product services** (the tray icon and menu, jump list, taskbar progress, and notifications answered as events; secure storage; typed feature flags with remote configuration; `docs/surfaces.md`), **Milestone 51 — observability, security, and compliance** (one trace across client and server with consent-gated export, crash reports with the tree at failure, grants enforced per request, a sandboxed isolated worker, power-loss-safe state, and generated compliance evidence; `docs/observability.md`), **Milestone 50 — deployment, updates, and fleet operations** (immutable revisions with preview, staged promotion, and rollback; container and systemd export; single-artifact assets; response caching; signed, staged Windows updates with automatic rollback; `docs/deploy.md`), **Milestone 56 — durable and event-driven execution** (replayed workflows with exactly-once steps, timers, signals and compensation; event handlers with partial-failure batches and dead letters; single-writer actors; supervision; long operations across the boundary; `docs/durable.md`), **Milestone 55 — reconciliation beyond the screen** (local-first sync with conflict policies and CRDTs, server-interactive screens that survive reconnects and deploys, channels and presence, and device desired state over MQTT; `docs/sync.md`), **Milestone 49 — the server application model** (a `tower` service on the client's own contracts: typed handlers, secure defaults, sessions, passkeys, checked SQL, durable jobs, typed server functions, server-only components, and an OpenAPI contract; `docs/server.md`), **Milestone 48 — components, tokens, and visualization** (native controls, a component library in both syntaxes, design-token import with host-following roles, accessible charts, grids, matched geometry, list views, documents, and host content; `docs/components.md`), **Milestone 54 — responsiveness under load** (message priorities, deferred values computed off the UI thread, pure components, and suspension of hidden screens; `docs/responsiveness.md`), **Milestone 47 — state, resilience, and data** (scoped stores read by slice, error boundaries with supervision, a query cache with offline mutations, forms, migrations, and WinHTTP with certificate pinning; `docs/data.md`), **Milestone 46
— internationalization** (typed message catalogues with CLDR plurals and
gender, runtime locale switching, host formatting, the translator's
workflow; `docs/i18n.md`), **Milestone 43
— the developer loop**, **Milestone 42
— budgets** (a budget file per shipped target, enforced in CI), **Milestone 44
— inspection and diagnostics** (one protocol every backend answers, the
`rustnative inspect` client, an in-app overlay, record and replay), **Milestone
41 — guarantees and conformance suites**, **Milestone 40 — interoperability
and incremental adoption**, **Milestone 58 — the style spellings** (utility
classes and declarations over typed styles, `app.css`, per-backend capability
tables), **Milestone 53 — the markup syntax**, **Milestone 39 —
portable-surface obligations**, and **Milestone 45 — test infrastructure** (the
headless reference backend), after Milestone 32's packaging and the milestones
before it. See `BUILD_STATUS.md` for what each pass
verified, what it found while doing so, and what is still open.

## Architecture

```text
                       Application
                           │
                    Component Runtime
                           │
        ┌──────────────────┼──────────────────┐
        │                  │                  │
     Components          Scheduler          Services
        │                  │                  │
        ├── state          ├── tasks          └── future platform APIs
        ├── props          ├── scopes
        ├── effects        └── wakeups
        ├── lifecycle
        └── messages
        │
        ▼
        builder syntax  ──┬──  markup syntax (.rsx files, rsx!)
                          │
                          ▼
                  Declarative UI Tree
                           │
                       Tree Diff
                           │
                         Layout
                           │
                    Native realization
                           │
              ┌────────────┴────────────┐
              ▼                         ▼
          rustnative-core        rustnative-windows
                                          │
                                          ▼
                                    Windows / Win32
```

The runtime also carries theme/style data and portable capability discovery;
platform-specific APIs remain behind explicit service and native-extension
boundaries. Style is resolved in the core the same way the tree is built there,
and it has two spellings of its own — typed properties and utility classes —
that meet before any backend sees them (see [Styling](#styling)).

## Two syntaxes

A tree can be written two ways. Both are part of the framework, both reach all
of it, and neither is a wrapper over the other.

**Builder** — ordinary Rust: constructors, `with_*` modifiers, method chains,
and functions that return `Node`.

```rust
// inbox.rs
fn view(&self) -> Node {
    Node::column_with_layout(
        "root",
        [
            Node::label("title", "Inbox"),
            Node::button("compose", "Compose").disabled(self.offline),
        ],
        LayoutStyle::new().width(SizeMode::Fill),
        ColumnStyle::new().padding(EdgeInsets::all(24)).gap(12),
    )
}
```

**Markup** — the same tree as elements, written directly as an expression in a
`.rsx` file.

```rust
// inbox.rsx
fn view(&self) -> Node {
    <Column key="root" width={SizeMode::Fill} padding={EdgeInsets::all(24)} gap={12}>
        <Label key="title" text="Inbox" />
        <Button key="compose" text="Compose" disabled={self.offline} />
    </Column>
}
```

Those two produce the same `Node` — not an equivalent one, the same one, and
the test suite asserts it with `==`.

### Where markup can be written

A `.rsx` file is Rust with one more kind of expression. An element can go
anywhere Rust accepts an expression — a function's tail, a `let`, a `return`,
a closure body, a match arm, an argument — with nothing around it, the way
markup is written in the markup-extended source files of other languages.
Everything else in the file is ordinary Rust.

The same markup can also be written inside the `rsx!` macro in any `.rs` file:

```rust
// any .rs file
let header = rsx! { <Label key="title" text="Inbox" /> };
// …which is the same `Node` as the builder call
let header = Node::label("title", "Inbox");
```

These are two carriers of one grammar, not two syntaxes. Any element moves
between a `.rsx` file and an `rsx!` call without a character changing, and they
are one implementation: building a `.rsx` file wraps each markup expression in
`rsx!` and leaves every other byte where it was, so a `.rsx` file cannot accept
anything the macro rejects or mean anything it would not.

They exist because each is the right tool somewhere:

- **`.rsx` files** are for code that is mostly UI — a screen, a component, a
  view module — where markup is simply how the file is written;
- **`rsx!`** is for markup inside a `.rs` file, for crates without a build
  script, and for runnable API documentation, which `rustdoc` compiles as plain
  Rust.

### How `.rsx` files build

Rust's compiler does not understand markup, just as a JavaScript engine does
not; in both cases a compile step lowers the file first, and the tooling maps
everything back. For a RustNative project that step is one line of the build
script, beside the one that embeds resources:

```rust
// build.rs
fn main() {
    rustnative_build::embed_resources();
    rustnative_build::compile_rsx(); // every .rsx file under src/
}
```

and a `.rsx` module is declared with `rsx_mod!(inbox);` where a `.rs` module
would use `mod inbox;`. The crate root stays `main.rs` or `lib.rs`.

The lowered files live in `target/` and nobody should need to open them:

- `rustnative build`, `check`, and `test` report every error — from markup or
  from ordinary Rust — at the `.rsx` file, line, and column you wrote;
- `rustnative lsp` gives editors completion, hover, go-to-definition, rename,
  and diagnostics in `.rsx` files, by forwarding to the Rust language server
  and mapping positions both ways;
- `rustnative fmt` formats a `.rsx` file whole — its Rust through `rustfmt`,
  its markup with the same formatter `rsx!` uses;
- `rustnative expand` prints the builder form any markup lowers to.

Plain `cargo build` still works; it reports positions in the lowered file,
whose source map (`<file>.rs.map`, beside it) names the source. That is the one place the compile step shows
through, which is why the CLI is the documented way to build a `.rsx` project.
`rsx!` has no such seam — a proc macro keeps its tokens' real positions, so its
errors land on the `.rs` file under plain `cargo` too.

### Why the two syntaxes cannot drift apart

Markup is a compile-time front end that expands to builder calls and nothing
else. It adds no node kind, no runtime type, no allocation, no indirection, and
no capability of its own. Everything downstream — reconciliation, layout,
native realization, accessibility, animation, virtualization — sees one tree
and cannot tell which syntax produced it, because there is nothing to tell.

That is what makes the guarantees below structural rather than a promise
somebody has to keep:

- **Every attribute is a builder method.** `key` is the constructor's key, and
  it stays explicit in both syntaxes because a key is semantic and nothing may
  infer it. `LayoutStyle` and `ColumnStyle`/`RowStyle` fields flatten into
  attributes — `width`, `height`, `margin`, `align_self`, `constraints`,
  `padding`, `gap`, `align_items`, `overflow`. Every `with_*` modifier is an
  attribute of the same name without the prefix. `disabled` and `hidden` are
  present-means-true flags.
- **Every builder method is reachable from markup.** Beyond the attribute
  mapping, `..expr` applies any `FnOnce(Node) -> Node`, which is how an
  application's own extension-trait modifiers — which the grammar has never
  heard of — stay available.
- **Every markup tree is a builder expression.** An element evaluates to a
  `Node`, so a builder chain applies to it directly, and `{expr}` splices any
  `Node` or iterator of nodes into markup.
- **A feature is not finished in one syntax.** A new node kind or modifier
  lands with both spellings and an equivalence case — compiled through a `.rsx`
  file and through `rsx!` — or the build fails.

Text is always an attribute or a braced expression, never loose characters
between tags. A macro receives Rust tokens, and loose prose is not valid Rust
tokens; a `.rsx` file could allow it, and deliberately does not, because the
moment one carrier accepts something the other cannot, markup stops moving
between them unchanged.

### Each at full strength

Neither form is a transliteration of the other. Markup gets the constructs
markup is good at — structure that mirrors the tree, conditional and repeated
children, fragments, component elements with typed props:

```rust
// inbox.rsx
fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
    <Column key="inbox" gap={8}>
        <FolderHeader key="header" title={self.folder.name.clone()} unread={self.unread} />
        if self.messages.is_empty() {
            <Label key="empty" text="Nothing here" />
        } else {
            for message in self.messages.iter().filter(|m| m.matches(&self.query)) {
                <Row key={message.id.to_string()} gap={12} ..{ |row| row.unread(message.unread) }>
                    <Label key="from" text={message.from.clone()} />
                    <Label key="subject" text={message.subject.clone()} />
                </Row>
            }
        }
    </Column>
}
```

`<FolderHeader/>` is a component element. `title` and `unread` are that
component's props, so a missing or misspelled one is a compile error at the
attribute, and it composes through `render`'s `ComponentContext` — the `.rsx`
compiler finds that parameter itself. Inside `rsx!`, which cannot see the
function it is in, the context is named instead: `rsx! { in context, … }`.

The builder API gets the constructs an API is good at — composition through
ordinary functions, iterator pipelines, conditional chaining, and extension
traits that read as part of the framework:

```rust
// inbox.rs
fn message_row(message: &Message) -> Node {
    Node::row_with_layout(
        message.id.to_string(),
        [
            Node::label("from", message.from.clone()),
            Node::label("subject", message.subject.clone()),
        ],
        LayoutStyle::new(),
        RowStyle::new().gap(12),
    )
    .unread(message.unread) // the application's own extension trait
}

fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
    Node::column_with_layout(
        "inbox",
        iter::once(context.child_with_props(
            "header",
            FolderHeaderProps { title: self.folder.name.clone(), unread: self.unread },
            FolderHeader::new,
        ))
        .chain(match self.messages.len() {
            0 => vec![Node::label("empty", "Nothing here")],
            _ => self.messages.iter().filter(|m| m.matches(&self.query)).map(message_row).collect(),
        }),
        LayoutStyle::new(),
        ColumnStyle::new().gap(8),
    )
}
```

Because there is one node type between them, an application may use both. A
`.rsx` screen can splice in a subtree assembled by a builder function, and a
builder chain can be applied to an element — which is what the navigation
example further down actually does.

### How this repository reads

Every example below appears in both syntaxes, labelled **Builder** and
**Markup**, with the markup written as it appears in a `.rsx` file; in a `.rs`
file the same markup goes inside `rsx! { … }`. The order inside each pair is
alphabetical and means nothing else. Neither syntax is the default, and the
CLI's project templates require you to choose (`rustnative new --syntax
builder|markup`) rather than choosing for you.

**Status.** The builder syntax is implemented and verified through Milestone 32
and is what the examples in `examples/` are written in. The markup syntax —
`.rsx` files and `rsx!` alike — is specified at implementable depth and is
**Milestone 53**; the markup in this repository is that specification and does
not compile yet. `PLAN.md` 2.9 states the contract it is held to, and
`BUILD_STATUS.md` records its status the same way it records everything else
this project has not yet run.

## Crate boundaries

```text
RustNative/
├── .github/
│   └── workflows/
│       └── ci.yml
├── Cargo.toml
├── Cargo.lock
├── .gitignore
├── .gitattributes
├── rust-toolchain.toml
├── rustfmt.toml
├── clippy.toml
├── deny.toml
├── LICENSE-APACHE
├── LICENSE-MIT
├── SECURITY.md
├── CONTRIBUTING.md
├── PLAN.md
├── BUILD_STATUS.md
├── README.md
│
├── crates/
│   ├── rustnative-core/
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs              (module map + flat public re-exports)
│   │   │   ├── identity.rs          (NodeId/ComponentId/WindowId + key interning)
│   │   │   ├── event.rs
│   │   │   ├── node.rs
│   │   │   ├── component/           (Component trait, context, effects, tree)
│   │   │   ├── reconcile/           (snapshot + diff)
│   │   │   ├── layout/              (geometry, constraints, measure, engine)
│   │   │   ├── style/               (theme + the two style phases)
│   │   │   ├── scheduler/           (Scheduler, TaskScope, pluggable Executor)
│   │   │   ├── services/            (service contracts + in-memory impls)
│   │   │   ├── capability.rs
│   │   │   ├── menu.rs
│   │   │   ├── window.rs
│   │   │   ├── panic.rs             (component-panic policy)
│   │   │   ├── application.rs
│   │   │   └── platform.rs
│   │   ├── tests/
│   │   │   ├── component_lifecycle.rs
│   │   │   └── property_tests.rs
│   │   └── benches/
│   │       └── core_benchmarks.rs
│   │
│   ├── rustnative-markup/             (Milestone 53: the markup grammar, once)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── parse/                (elements, attributes, children,
│   │       │                          control flow, fragments, spreads)
│   │       ├── vocabulary.rs         (element -> constructor,
│   │       │                          attribute -> builder method)
│   │       ├── lower.rs              (element tree -> builder calls)
│   │       ├── rsx_file.rs           (.rsx: find markup expressions, wrap
│   │       │                          them in rsx!, record the source map)
│   │       ├── source_map.rs         (lowered position <-> .rsx position)
│   │       └── diagnostics.rs        (spans and messages)
│   │
│   ├── rustnative-macros/             (Milestone 53: `rsx!`, a thin proc-macro
│   │   │                              shell over rustnative-markup,
│   │   │                              re-exported from rustnative-core)
│   │   ├── Cargo.toml
│   │   ├── src/lib.rs
│   │   └── tests/
│   │       ├── equivalence.rs        (builder, .rsx, and rsx! spellings,
│   │       │                          asserted equal)
│   │       ├── rsx_files/            (the same cases as .rsx files)
│   │       ├── expansion/            (expansion goldens)
│   │       └── ui/                   (compile-failure suite, both carriers)
│   │
│   ├── rustnative-style/              (the style vocabulary, once — Milestone 58)
│   │   ├── Cargo.toml
│   │   ├── VENDORED.md               (the pinned Tailwind v4 theme)
│   │   ├── vendor/                   (tailwind-theme-4.1.13.css, licence)
│   │   └── src/
│   │       ├── model.rs              (properties, values, conditions,
│   │       │                          DeclarationSet)
│   │       ├── value.rs, color.rs    (lengths, calc(), colour functions,
│   │       │                          the gamut rule)
│   │       ├── vocabulary.rs         (classes and declarations -> model)
│   │       ├── sheet.rs              (app.css: @theme, @utility, @apply,
│   │       │                          @custom-variant)
│   │       ├── token_table.rs        (run-time tokens)
│   │       ├── capability.rs         (per-backend tables, unit mappings)
│   │       └── tokens.rs             (model -> Rust, for the macros)
│   │
│   ├── rustnative-interop/            (library-only mode — Milestone 40:
│   │                                  the .ril description, C/C#/Rust
│   │                                  generators, the shims' runtime)
│   │
│   ├── rustnative-build/              (build-script helpers: resources,
│   │                                  `compile_rsx()`, `compile_styles()`)
│   │
│   └── rustnative-windows/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs              (module map + flat public re-exports)
│           ├── error.rs             (Error + NativeContext + Win32Category)
│           ├── platform.rs          (WindowsPlatform: the Platform impl)
│           ├── ffi.rs               (shared string/memory helpers)
│           ├── services/            (clipboard, dialogs, notifications, system)
│           └── native/              (the Win32 window backend)
│               ├── context.rs       (the only place a raw handle becomes a reference)
│               ├── win32.rs         (Win32 return-value classification)
│               ├── message_loop.rs  (window class, message loop, WNDPROC)
│               ├── runtime.rs       (per-window Runtime + WindowRegistry)
│               ├── container.rs     (container WNDPROC)
│               ├── registry.rs      (NodeId <-> native object, both directions)
│               ├── rendering/       (realization, controls, styling,
│               │                     accessibility, scrolling)
│               ├── input/           (keys, focus, pointers/capture/gestures,
│               │                     IME, clipboard events, OLE drop target,
│               │                     XInput controllers)
│               ├── measure.rs       (GDI text measurement)
│               ├── menu.rs          (MenuBar -> HMENU, with RAII)
│               ├── user_data.rs     (typed GWLP_USERDATA accessors)
│               ├── window_handles.rs
│               ├── harness.rs       (test-only bounded message pump)
│               ├── integration.rs   (test-only native scenarios)
│               └── input_integration.rs (test-only advanced-input scenarios)
│
├── examples/
│   ├── hello-label/
│   │   ├── Cargo.toml
│   │   └── src/main.rs
│   └── gdi-font-diagnostic/         (standalone GDI handle-lifetime probe)
│       ├── Cargo.toml
│       └── src/main.rs
│
└── tools/
    ├── windows-cross-test.sh        (mingw + Wine cross-compile/run)
    └── win_probes/                  (C ground-truth probes for Win32 behavior)
```

### `rustnative-core`

Contains the portable runtime and UI model:

- component model;
- keyed identity;
- props;
- lifecycle;
- callbacks/messages;
- framework events;
- declarative nodes;
- tree snapshots/diffing;
- native-object-independent layout;
- constraints;
- intrinsic measurement contracts;
- overflow/clipping/scroll semantics;
- focus/keyboard abstractions;
- advanced input: pointer/touch/pen events with capture, wheels, portable
  gesture recognition, IME composition, clipboard actions, drag-and-drop, and
  gamepad diffing, all opt-in per node;
- accessibility semantics;
- scheduler;
- task scopes;
- dependency-aware effects and effect-owned task scopes.
- injected async services and deterministic mock services;
- theme tokens, component styles, and node-level visual overrides;
- platform capabilities and native-extension escape hatch;
- multiple independently owned window roots and window lifecycle state.

It must not import Windows or any other operating-system API.

### The markup crates: `rustnative-markup`, `rustnative-macros`, and `compile_rsx`

The markup syntax is three thin pieces over one implementation, all planned for
Milestone 53:

- **`rustnative-markup`** owns the grammar, once: one element per `Node`
  constructor — `Column`, `Row`, `Label`, `Button`, `TextInput`, `Canvas`,
  `Surface`, `TabBar`, `VirtualList` — plus component elements, which lower to
  `ComponentContext::child_with_props`; one attribute per builder method or
  style field, carrying that method's exact type; the structural constructs
  (nested children, `{expr}`, `if`/`else`, `match`, `for`, `<>…</>`
  fragments, `..expr` spreads); the lowering to builder calls; and the
  diagnostics. It also knows how to read a `.rsx` file: a Rust parser extended
  with one expression form, which finds each markup expression, wraps it in
  `rsx!`, and records the source map.
- **`rustnative-macros`** is `rsx!`: a proc-macro shell over
  `rustnative-markup`, re-exported from `rustnative-core` behind a default-on
  `markup` feature — on by default so the syntax is not a second-class opt-in,
  and a feature so the constrained embedded profiles can drop a proc-macro
  dependency they cannot afford.
- **`rustnative_build::compile_rsx()`** runs that wrapping step for every
  `.rsx` file from the build script, and `rustnative` uses the same source map
  to put every diagnostic, editor position, and formatting edit back on the
  `.rsx` file.

Diagnostics are held to compiler quality by a compile-failure suite run
through both carriers: the span points at the offending attribute or element —
never at the macro call or the lowered file — an unknown attribute names the
builder method it was looking for, and a type mismatch is reported against the
attribute's own span.

None of it touches the runtime. Markup emits builder calls, so it cannot add
behaviour the builder syntax does not already have, and the `equivalence`
tests are what keep that true as the node API grows.

### The style crate: `rustnative-style`

Arranged like the markup crates for the same reason — one implementation,
several callers:

- **`rustnative-style`** owns the declaration vocabulary (values, units,
  `calc()`, colour functions and the one gamut rule, token references), the
  utility-class table compatible with Tailwind CSS v4.1.13 over its vendored
  default theme, the `app.css` directives that survive without a cascade
  (`@theme`, `@utility`, `@apply`, `@custom-variant`), the diagnostics, and
  every shipped backend's capability table and unit mapping.
- **`classes!` and `styles!`** (in `rustnative-macros`, re-exported by
  `rustnative-core`) compile a class string or declaration block into a
  `DeclarationSet` in a `static`; markup's `class="…"` and `style="…"` lower to
  them. **`rustnative_build::compile_styles()`** compiles `app.css` into the
  theme `app_theme!()` includes, and `rustnative expand --classes` prints a
  lowering.
- **`rustnative-core`** re-exports the model (`style::decl`) and resolves it:
  after every render, and whenever the theme or environment changes, each
  node's declarations are folded into its typed properties.

It emits typed style properties and nothing else, so — like markup — it cannot
give a node a style the typed spelling could not already produce. There is no
matcher, no stylesheet, and no class string at run time.

### Interoperability: embedding both ways, and library-only mode

Milestone 40's adoption ladder, each rung with a worked example under test
(`docs/interop/adoption-ladder.md`):

- **library-only mode** — `rustnative-interop`: one interface description
  (`.ril`) with ownership and threading annotated, from which the C header, C#
  bindings, and Rust implementation shims are generated
  (`rustnative bindgen`); `examples/adoption-library` is driven by a C and a C#
  program under test;
- **embedding inward** — `WindowsPlatform::embed(parent, application)` realizes
  the tree inside a window the host owns, driven by the host's loop
  (`examples/adoption-subtree`, a `windows-sys`-only program); without a parent
  it is **guest-runtime mode** (`start_external`);
- **embedding outward** — `Node::foreign` / `<Foreign>` adopts a control the
  framework did not write, from a factory registered with `register_foreign`
  (`examples/adoption-foreign`, the system month calendar and date picker);
- **the rendering-surface hand-off** — `Node::native_surface`'s lifetime,
  resize, DPI, and present contract, in `docs/interop/surface-handoff.md`.

### `rustnative-windows`

Owns Windows-specific implementation details:

- Win32 window creation;
- native `HWND` ownership;
- native controls;
- Win32 event translation;
- native text measurement;
- native focus APIs;
- advanced input (`WM_POINTER*`, mouse capture, IMM32, OLE drag-and-drop,
  `XInput`);
- native scrolling/viewport implementation;
- Windows event-loop wakeups;
- the accessibility bridge (`WS_TABSTOP`, and MSAA dynamic annotation for
  name/description/role);
- platform-specific unsafe code.

This is the only place where Windows APIs should leak into the framework implementation.

Two modules inside `native/` exist specifically to keep the unsafe surface
small, and are worth reading before anything else there:

- `native::context` is the **only** place a raw Win32 handle or stored raw
  pointer becomes a Rust reference. The lifetime argument that makes every
  such conversion sound is stated there once, rather than re-derived in a
  `SAFETY` comment at each call site.
- `native::win32` classifies Win32 return values — `must_succeed`,
  `best_effort`, `informational`, `ignored_by_contract` — so that ignoring
  one is a deliberate, readable decision rather than an omission.

## Current component data flow

The runtime now supports both directions of typed data flow:

```text
Parent
  │
  │ typed Props
  ▼
Child
  │
  │ Callback<Message>
  ▼
Parent
```

A changed prop updates an existing keyed component rather than recreating it. A child callback sends a typed message through a framework-managed channel and the parent rerenders through normal reconciliation.

## Current asynchronous model

Async work follows:

```text
Component
   │
   ▼
TaskScope
   │
   ▼
Scheduler
   │
   ▼
Worker/task execution
   │
   ▼
Completion queue
   │
   ▼
Native event-loop wakeup
   │
   ▼
Component message/update
   │
   ▼
Render → diff → layout → native update
```

Each framework-managed component owns one persistent `TaskScope`. Effects use a
separate scope per keyed dependency run, so replacing an effect cancels only the
work owned by that effect.

### Lifetime rules

- Rerendering does not cancel tasks.
- Prop changes do not cancel tasks.
- Explicit `TaskHandle::cancel()` cancels a task early.
- Removing a component cancels all outstanding tasks in its scope.
- Cancellation occurs before `unmounted()`.
- Completed results for removed components are ignored.
- Effects run after rendering, are retained for unchanged dependencies, and run
  cleanup before replacement or component removal.

## Native object identity

The framework keeps stable Rust IDs separate from native handles:

```text
NodeId
  ↓
NativeObjectRegistry
  ↓
NativeObject
  ↓
HWND
```

A stable node identity means an existing native control can be updated in place instead of destroyed/recreated.

## Planned platform backends

Each target gets its own adapter over the same core, and each is planned to
the same depth. The full specifications are in [`PLAN.md`](PLAN.md), section 8.

```text
                    rustnative-core
                          │
  ┌─────────┬─────────┬───┴─────┬─────────┬─────────┬─────────┐
  ▼         ▼         ▼         ▼         ▼         ▼         ▼
windows    macos     linux    android    ios       web       tui
 Win32     AppKit    toolkit   View     UIKit    DOM/CSS    cells
                                                             │
                                                        embedded
                                                     display drivers
```

**macOS** (Milestone 33) — `NSWindow`/`NSView` and AppKit controls, Core Text
measurement, the responder chain, `NSAccessibility`, the macOS menu bar, and
`.app` bundling with signing and notarization.

**Linux** (Milestone 34, **built**: [`docs/linux.md`](docs/linux.md)) — GTK 4
behind a toolkit seam, Pango measurement, AT-SPI2 accessibility, desktop
portals and the freedesktop services, both Wayland and X11 sessions with
their differences reported as capabilities, and `.deb`, tarball, and
AppImage packaging.

**Android** (Milestone 35) — a native `View` hierarchy over a disciplined JNI
boundary, the activity/process lifecycle mapped onto the existing lifecycle
and restoration contracts, `AccessibilityNodeInfo`, and Gradle/AAB packaging.

**iOS** (Milestone 36) — `UIView`/UIKit, the scene lifecycle, `UIAccessibility`,
universal links into the existing deep-link model, and Xcode packaging.

**Web** (Web milestones A–K, **built**: [`docs/web.md`](docs/web.md)) — a
`rustnative-web` adapter using semantic DOM elements rather than a canvas, in
all three deployment modes, chosen at build time from one application:

```text
rustnative-core → rustnative-web → Rust on the server
                                      ↓
                    HTML / CSS / compile-time generated JavaScript
                    (+ WebAssembly for subtrees that opt in)
                                      ↓
                    DOM / browser events / Web APIs

client-side      pages built ahead of time; the host serves files
server-rendered  a Rust server renders HTML per request; generated JS attaches
serverless       the same render per request in a function or edge runtime
```

All application code on a server is Rust. By default the browser receives
HTML, CSS, and JavaScript generated at compile time from client logic written
in a restricted subset of Rust. A subtree that needs the full language — heavy
computation, rich offline behaviour, sync merge functions — opts into
WebAssembly and runs as full Rust in a lazily loaded module; other code outside
the subset is a compile error unless it is marked as a server call. The two
sides exchange serializable data only.

The scope covers client code generation, opt-in WebAssembly subtrees, and
browser lifecycle, DOM ownership, CSS/layout integration, browser focus, text
input, pointer, touch, keyboard and IME, HTML/ARIA accessibility,
fetch/WebSocket/storage capabilities, Web Workers, URL/history routing, server
rendering with client attachment and typed server functions, service workers
and PWAs, serverless and edge deployment, and the browser development,
testing, and bundling tooling.

**Terminal** (Milestone 38) — a `rustnative-tui` adapter realizing the same
tree onto a terminal's cell grid: the Windows console in virtual-terminal
mode, `termios` and VT sequences elsewhere, Unicode-width text measurement,
key and mouse protocols, damage-tracked redraw, and terminal state restored
even on panic. Desktop terminals and embedded Linux consoles, local or over
SSH — not Android, iOS, or the browser.

**Embedded** (Milestone 37) — embedded Linux, RTOS, and selected bare-metal
profiles, realized through the draw-list path rather than native controls,
with a `no_std`-capable core subset defined before the constrained profiles
start.

## Layout model

The layout engine is platform-independent and supports:

- `Auto`;
- `Fixed`;
- `Fill`;
- minimum/maximum constraints;
- alignment;
- padding;
- margins;
- spacing/gaps;
- `Column`;
- `Row`;
- intrinsic measurement requests;
- wrapped text measurement;
- overflow/clipping;
- scroll ranges and offsets.

The core returns geometry; the platform backend applies it to native controls.

For scrolling, the Windows backend uses:

```text
Scrollable container
└── Viewport HWND
    └── Content HWND
        ├── native child
        ├── native child
        └── ...
```

Scroll position is transient runtime state and does not itself trigger a component rerender.

## Styling

Style is resolved in the core before a backend sees it. A theme supplies tokens
and per-component defaults, a node may override them, and the interaction state
— normal, hover, focus, pressed, disabled — selects a variant. What reaches
Windows is concrete values applied to real controls through `WM_CTLCOLOR*`,
`CreateFontIndirectW`, and `WM_ERASEBKGND`; what reaches any other backend is
the same values in its own terms.

That resolved style has two spellings, the way the tree itself has two syntaxes.

**Typed properties** (Milestone 21):

```rust
// builder
Node::column("card", children)
    .with_style(VisualStyle::new().background(Color::rgb(0x2b, 0x7f, 0xff)).border_radius(8))
    .with_state_style(ControlState::Hovered, VisualStyle::new().background(Color::rgb(0x15, 0x5d, 0xfc)))
```

```rust
// card.rsx
<Column key="card" style={VisualStyle::new().background(Color::rgb(0x2b, 0x7f, 0xff)).border_radius(8)}>
    {children}
</Column>
```

**Utility classes** (Milestone 58) — the same style, in the vocabulary of
Tailwind CSS v4.1.13:

```rust
// builder
Node::column("card", children).with_class(classes!("bg-blue-500 hover:bg-blue-600 rounded-lg"))
```

```rust
// card.rsx
<Column key="card" class="bg-blue-500 hover:bg-blue-600 rounded-lg">
    {children}
</Column>
```

or as declarations — `styles!("background-color: var(--color-blue-500)")`, or
`style="…"` with a string in markup.

Classes are compiled where they are written. `bg-blue-500` becomes a background
declaration pointing at the `--color-blue-500` token, `p-4` becomes
`calc(var(--spacing) * 4)` of padding, `hover:` becomes the node's hover state
style, and the result is the same typed properties the first spelling sets — so
no class string, selector, or cascade exists at run time, and
`rustnative expand --classes "…"` prints exactly which properties a class
string sets and what they resolve to. A class that does not resolve is a
compile error naming the nearest one; a computed class string is a compile
error that says why; and a class setting a property the target's backend cannot
realize (a shadow, on Windows) is a compile error for that target, at the
class. The builder spelling takes the macro — `.with_class(classes!("…"))` — for
exactly that reason: a plain string could not be checked until run time.

Tokens come from one file per project, compiled by the build script
(`rustnative_build::compile_styles()`) and applied with
`application.set_theme(rustnative_core::app_theme!())`:

```css
/* app.css */
@theme {
  --color-primary: oklch(0.62 0.19 259);
  --radius-lg: 8px;
}

@utility card {
  @apply bg-primary rounded-lg p-4;
}

@custom-variant touch (@media (pointer: coarse));
```

The default theme — Tailwind v4's, vendored and pinned — is underneath; a
namespace can be cleared (`--color-*: initial`). `@import "tailwindcss"`,
`@plugin`, `@config`, selectors, and `@media` blocks are refused with a
diagnostic saying why: the theme ships with the framework, there is no
JavaScript toolchain, and nothing is matched against the tree.

Token values stay *references* through resolution, so switching theme, colour
scheme, palette, or text size re-resolves the existing native objects instead
of rebuilding the tree — on Windows, a live `WM_SETTINGCHANGE` to dark mode
restyles every `dark:` class on the controls already on screen. State and
condition variants — `hover:`, `focus:`, `active:`, `disabled:`, `dark:`,
`sm:`/`md:`/`lg:`, `rtl:`, `motion-reduce:`, `pointer-coarse:` — map onto
mechanisms the framework already has (state styles and the environment) rather
than adding new ones.

What the framework takes from that vocabulary is the declaration half: property
names, values, units, colour functions, and token references. What it
deliberately does not take is the cascade — no selectors, no specificity, no
descendant rules — because a style decided by matching rules against the tree
cannot be resolved deterministically, and a second styling engine competing
with the framework's own resolution is the thing every native-realization
framework has regretted. Where a style genuinely depends on the tree — a row's
position, a parent styling its children — it names that relationship as a typed
condition the core resolves, with no matching step.

Styles are capability-checked per host, like everything else here. Each backend
answers, per property, whether it realizes, approximates, or cannot express it
(`Platform::style_capabilities`), and records how it maps units
(`Platform::unit_mapping`). Windows realizes colours, fonts, layout, and
opacity; approximates a container's border (a one-pixel frame) and corner
radius (a window region) while native controls keep their system shape; and
cannot draw shadows. The headless backend, being a model, realizes everything.
`BUILD_STATUS.md` records the tables.

The vocabulary is not cut down to what every host shares, because that would
style the browser, a GTK desktop, and every host to come no further than the
narrowest backend goes. Its range grows past the portable one (Milestone 67,
planned): border widths and styles, gradients, transforms, filters, motion on
the framework's own animation timelines, text detail, and control parts. Every
backend answers each of them — once for a native control, once for a box the
framework owns, since a host may decorate a container but never redraws a
control — and the application writes down where its targets may differ:

```text
rounded-lg supports-[box-shadow]:shadow-lg not-supports-[box-shadow]:border
web:backdrop-blur-sm        odd:bg-gray-50        *:px-4
```

A guard asks the capability table of the backend the tree is realized on, not
the browser's `@supports`, and exists in the typed spelling too. The targets an
application declares (`[style] targets` in `rustnative.toml`) are all checked at
build time, so an unguarded property one of them cannot realize still fails the
build, naming that target. Structural variants such as `odd:` and `*:` are
resolved by the core from the reconciler's own child lists. Relationships on
another node's state (`group-hover:`, `peer-checked:`), container queries, and
raw host styling are specified as typed relationships and a single-host escape
hatch (Milestone 68) and are optional: a component already passes its state to
its children. `docs/styling.md` is the reference, including what the range
looks like on each host and what bounds it on Windows (Milestone 69: scaling
first, then composition for the boxes the framework owns).

## Advanced input

Beyond clicks, keys, text, and focus — which every node receives — a node opts
into the higher-frequency streams it actually wants:

**Builder**

```rust
Node::column("canvas", children)
    .with_input(InputInterest::new().pointer().gestures().wheel())
```

**Markup**

```rust
<Column key="canvas" input={InputInterest::new().pointer().gestures().wheel()}>
    {children}
</Column>
```

A backend delivers those streams only to interested nodes, walking up from
whatever is under the pointer to the nearest one that asked. A component that
needs every sample of a drag, even outside its bounds, captures the pointer
through a deferred request:

```rust
fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
    self.input = Some(context.input()); // keep for `update`
    self.view()
}
// in `update`, on `Event::PointerDown { pointer, .. }`:
// self.input.capture_pointer("canvas", pointer.pointer_id());
```

Gesture recognition (tap, long press, pan, pinch) and gamepad state diffing
live in `rustnative-core`, not in a backend, so every platform agrees on what a
tap is. The Windows backend realizes the rest natively: `WM_POINTER*` for
touch and pen, top-level mouse capture with lost capture reported as
`PointerCancel`, IMM32 composition for focusable custom containers, an OLE
`IDropTarget` per window, clipboard shortcuts and `WM_CLIPBOARDUPDATE`, and
`XInput` controllers polled only while some node asks for them.

## Focus, keyboard, and accessibility

The core exposes semantic input events instead of Win32 virtual-key constants. The Windows backend translates native input to those events.

The portable accessibility model (`rustnative_core::accessibility`) covers
roles (29, including headings with levels), names, descriptions, text and
range values, checked/expanded/selected/read-only/required/busy states,
declared and invoked actions, labelled-by/described-by/controls
relationships, live regions, position-in-set for virtualized children,
automation ids, focusability, and *virtual elements* — semantic children with
no native object of their own, for custom-drawn content.

Every node kind defaults to the role and focusability that describe it — a
button is an activatable control and a keyboard stop, a label is static text
and is not — so the model carries real information without an application
filling it in node by node. `Node::with_accessibility` replaces it wholesale
where that default is wrong.

The Windows backend realizes it as **UI Automation server-side providers**
(`native::uia`): every realized node's window answers `WM_GETOBJECT`, and its
provider is merged with the control's native proxy so only what the model
states is overridden. Virtual elements are fragments; control patterns
(Invoke, Value, RangeValue, Toggle, ExpandCollapse, SelectionItem,
ScrollItem) turn into `Event::AccessibilityAction` for the component to
honor; property, structure, and live-region changes are announced.
Focusability is also realized as `WS_TABSTOP`, and MSAA-only clients still get
names and roles through the Dynamic Annotation API.

Every UIA test runs a real `IUIAutomation` client on another thread, the way a
screen reader in another process reaches the application.

## Animations and transitions

A value can change over time without the component tree knowing. Two ways in:

A transition — whenever this node's opacity changes, animate to the new value
over 120 ms rather than jumping to it.

**Builder**

```rust
Node::column("panel", [])
    .with_opacity(if pressed { 0.6 } else { 1.0 })
    .with_transition(
        AnimatedProperty::Opacity,
        Transition::new(Duration::from_millis(120)),
    )
```

**Markup**

```rust
<Column
    key="panel"
    opacity={if pressed { 0.6 } else { 1.0 }}
    transition={(AnimatedProperty::Opacity, Transition::new(Duration::from_millis(120)))}
/>
```

An explicit animation is requested from the component rather than described on
a node, so it is the same call in either syntax: a spring back to the laid-out
position, starting 24 px to the right of it.

```rust
context.animations().animate(
    "panel",
    Animation::new(
        AnimatedProperty::Translation,
        AnimatedValue::Offset(Point::new(0, 0)),
        Transition::spring(220.0, 14.0, 1.0),
    )
    .from(AnimatedValue::Offset(Point::new(24, 0))),
);
```

`rustnative_core::animation::Timeline` evaluates both, and is entirely
platform-free: it turns elapsed time into a per-property value and handles
repeats, autoreverse, fill modes, and — for springs — carries the current
velocity into a retarget, so interrupting a motion continues it rather than
restarting it. A finished animation releases its property, and the node goes
back to exactly what the rendered tree says.

**A frame is not a render.** The backend applies the frame's value to the
native object it belongs to and nothing else: `SetWindowPos` for geometry, a
layered-window alpha for opacity, an invalidation for colours. A component
hears only `Event::AnimationFinished`, and may ignore it; the animation tests
assert that render counts do not move while values animate.

Frames come from one process-wide driver thread paced by `DwmFlush` and
posted — never sent — to each animating window, coalesced so a busy UI thread
drops frames rather than queueing them. With nothing animating, that thread
sleeps on a condition variable and the application costs nothing.

Animations are cancelled per node and property, per component (unmounting
cancels a component's own animations), and per window. The system's
reduced-motion preference is read at startup and tracked live; each animation
says whether it is skipped (jump straight to the target) or still run when
motion is reduced.

## Virtualized lists

A list of a hundred thousand items realizes a screenful of native windows:

**Builder**

```rust
Node::virtual_list(
    "rows",
    VirtualListStyle::new(100_000, ItemExtent::Fixed(24)),
    self.range.indices().map(|index| {
        Node::label(format!("row-{index}"), format!("Row {index}")).with_item_index(index)
    }),
)
```

**Markup**

```rust
<VirtualList key="rows" count={100_000} extent={ItemExtent::Fixed(24)}>
    for index in self.range.indices() {
        <Label key={format!("row-{index}")} text={format!("Row {index}")} item_index={index} />
    }
</VirtualList>
```

The component renders only `self.range`, which it learns from
`Event::VisibleRangeChanged`. That event arrives when scrolling (or a resize,
or newly measured item sizes) moves the range, and never otherwise: a
scroll inside the realized range is the same native viewport transform any
scrollable container uses. A virtual list is sized by its parent (give it
`Fill` or a fixed size), and its content length is every item's, so the
scroll range covers the whole list.

Items are either `ItemExtent::Fixed`, which costs nothing per item, or
`ItemExtent::Estimated`, in which case the rows that are realized are
measured and every later offset moves with what was learned
(`ExtentCache`, a Fenwick tree). Row keys that name the *data* rather than
the index keep the item a person is looking at still when items are
inserted above it (`ScrollAnchor`). On Windows, rows that scroll out hand
their native windows to rows that scroll in, so scrolling end to end creates
a screenful of controls once.

## Custom drawing

Ordinary UI is native controls. For the parts of an application that are
pictures, there are two explicit escape hatches.

A **canvas** draws a portable display list with the platform's 2D API
(Direct2D on Windows):

A draw list is built the same way in either syntax — it is data, not tree
structure — and only the node differs.

```rust
let chart = DrawList::new()
    .fill_rect(RectF::new(0.0, 0.0, 240.0, 90.0), Paint::color(Color::rgb(245, 246, 250)))
    .push_transform(Transform2D::translation(20.0, 10.0))
    .fill_rect(RectF::new(0.0, 0.0, 36.0, 70.0), Paint::color(Color::rgb(90, 140, 230)))
    .hit_region(1, RectF::new(0.0, 0.0, 36.0, 70.0))
    .pop();
```

**Builder**

```rust
Node::canvas("chart", chart, LayoutStyle::new().width(SizeMode::Fixed(240)).height(SizeMode::Fixed(90)))
    .with_input(InputInterest::new().pointer())
```

**Markup**

```rust
<Canvas
    key="chart"
    draw_list={chart}
    width={SizeMode::Fixed(240)}
    height={SizeMode::Fixed(90)}
    input={InputInterest::new().pointer()}
/>
```

A draw list is data in the node tree: an unchanged list costs nothing, and
a changed one redraws that canvas alone. Pointer input on a canvas carries
the hit region it landed in (`PointerEvent::region`), tested under the same
transforms and clips the drawing used.

A **native surface** is a bare window the framework lays out and never
paints, for an application's own GPU renderer:

**Builder**

```rust
Node::native_surface("viewport", LayoutStyle::new().width(SizeMode::Fill).height(SizeMode::Fill))
```

**Markup**

```rust
<Surface key="viewport" width={SizeMode::Fill} height={SizeMode::Fill} />
```

Reaching the handle happens in `update`, away from the tree, so it is the same
either way:

```rust
if let Event::SurfaceResized { surface, size, .. } = event {
    let handle = rustnative_windows::native_surface(surface); // raw-window-handle 0.6
}
```

## Navigation and persistence

Screens are components; a navigation stack is data in the component that
shows them.

The stack itself is an ordinary value and `NavigationStack::view` an ordinary
function, so what differs between the two forms is only how each entry's screen
is spelled — which is also the smallest honest illustration of mixing them.

**Builder**

```rust
struct App { stack: NavigationStack<String> }
// type Message = NavigationCommand<String>;

fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
    let navigator = Navigator::new(context.callback());
    self.stack.view("stack", |entry| {
        context.child_with_props(entry.id().key(), ScreenProps { navigator: navigator.clone() }, Screen::new)
    })
}
fn message(&mut self, command: NavigationCommand<String>) { self.stack.apply(command) }
```

**Markup**

```rust
struct App { stack: NavigationStack<String> }
// type Message = NavigationCommand<String>;

fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
    let navigator = Navigator::new(context.callback());
    self.stack.view("stack", |entry| <Screen key={entry.id().key()} navigator={navigator.clone()} />)
}
fn message(&mut self, command: NavigationCommand<String>) { self.stack.apply(command) }
```

`<Screen/>` is a component element: `navigator` is one of `ScreenProps`'
fields, checked at the attribute, and the element lowers to exactly the
`child_with_props` call the builder form writes out, through `render`'s
`context` — which the `.rsx` compiler finds on its own, and which `rsx!`
would be told with `in context,`.

Every entry stays rendered, all but the top one `hidden`, so a screen
pushed on top of another never rebuilds it. Tabs work the same way:
`Node::tab_bar` is the system tab control, and each tab's content stays
mounted. `Route`/`Router` turn paths (and `Event::DeepLink` URLs) into
named routes with typed parameters.

State that should outlive the process is a `Persisted<T>`:

```rust
let count = context.persisted("count", 0u32); // keyed by this component's key path
count.update(|count| *count += 1);            // buffered; flushed when it matters
```

The store is set once, on `Services` (`FileStateStore::for_app(id)` on
Windows: crash-safe, atomic writes under `%LOCALAPPDATA%`). Writes are
flushed after a moment of quiet, before `Lifecycle::Suspending` and
`Lifecycle::Terminating` are delivered, and when the last window closes.
`WindowsPlatform::new().with_app_id(id)` makes the application
single-instance: a second launch hands its URL to the running one.

## The `rustnative` command

`crates/rustnative` builds the `rustnative` binary, which drives the toolchains rather than
replacing them:

```sh
rustnative new my-app --syntax markup --framework-path .   # or --syntax builder
cd my-app
rustnative run windows                     # cargo run, with this project's manifest
rustnative build windows --release
rustnative test -- --nocapture             # arguments pass through to cargo test
rustnative doctor                          # what this machine can build, and what it lacks
```

A web application:

```sh
rustnative new my-site --web --framework-path .   # a page with an island
cd my-site
rustnative dev web                                # serve, rebuild and reload on save
rustnative build web --mode client                # static files; or --mode server,
                                                  # or --mode serverless --host lambda|wagi
rustnative serve static target/web/client         # or serve lambda / serve wagi: as the host runs it
```

`--syntax` has no default: the generator does not pick a side on a developer's
behalf, and the two templates are the same application written twice
(Milestone 53) — the markup one in `.rsx` files with `compile_rsx()` already in
its build script. The same milestone gives the CLI the tooling `.rsx` files
need (`rustnative fmt`, `expand`, and `lsp`, and diagnostics from `build`,
`check`, and `test` reported at the `.rsx` source). Nothing else in the CLI is
affected, because nothing below the markup lowering can tell the two syntaxes
apart.

`rustnative dev windows` is the development loop (Milestone 43,
`docs/developer-loop.md`). A token-only `app.css` change is applied to the
running application without a rebuild. Anything else is rebuilt and
restarted, with the application's state kept. `--remote` runs the
application on another machine through `rustnative dev-agent`. The loop's
wall-clock time is a budget in `budgets/windows.toml`. `rustnative preview`
browses the application's previews across themes, locales, and text sizes,
and every preview is a golden test. `rustnative generate` writes components,
screens, and services, each with its preview and test, in the project's
syntax. `rustnative lsp` gives `.rsx` files and `rsx!` the same completion,
hover, definitions, class-string help, and structural edits.

`rustnative inspect` is the inspector (Milestone 44, `docs/inspection.md`).
Start an application with `RUSTNATIVE_INSPECT=1` and ask it for its tree,
components and state (editable), why a node has its geometry, where each of its
style properties came from, the trace of events and renders with every
component's render-or-skip reason, its tasks, host-object lifetimes, and what
the host refused. It can also show the in-app overlay, or record a session
that `inspect to-test` turns into a headless regression test.

A project is a folder with a `rustnative.toml`: the application's identity (the
same id its saved state and single-instance mutex use), its display name,
version, publisher, and URL schemes. Every platform on the roadmap is
recognized; the ones whose backend does not exist yet say so, with the
milestone that brings them, and exit with a distinct code rather than
quietly building for Windows.

## Performance budgets

Performance numbers in this project come from the budget files
(`budgets/windows.toml`, `budgets/headless.toml`, with the keys defined in
`budgets/SCHEMA.md`), not from adjectives. `rustnative bench --target
<target>` measures them, and `--check` fails the build on a regression beyond
each key's declared noise. CI runs it on every push. A documentation test
(`rustnative-conformance/tests/doc_claims.rs`) rejects a performance claim in
this README or `docs/` that does not point at a budget.

The Windows budgets, per `budgets/windows.toml`:

- **Startup:** interactive in at most 450 ms from process creation, on a form
  of 125 native controls.
- **Memory:** 24 MB resident.
- **Input latency:** 16 ms median from a click on the real button window to
  the realized change.
- **Frame time:** 17.5 ms median during a transition.
- **Artifact size:** a 3.5 MB release executable.

`RUSTNATIVE_STARTUP_TRACE=1` prints any application's startup phases:
process start, runtime ready, first frame, first content, interactive.
`rustnative build windows --release --pgo` builds profile-guided from a
scripted startup.

## Packaging

An application's `build.rs` is one line:

```rust
fn main() {
    rustnative_build::embed_resources();
}
```

which gives the executable its icon, its version information, and the
Windows application manifest this framework depends on: per-monitor V2 DPI
awareness, Common Controls v6, and the `supportedOS` entries without which
layered child windows (animated opacity) and themed tab controls do not
behave as documented.

A project written in `.rsx` files adds a second line,
`rustnative_build::compile_rsx();` (see
[How `.rsx` files build](#how-rsx-files-build)); nothing about packaging
differs, because what is packaged is the same compiled program.

`rustnative` builds what people install:

```sh
rustnative package windows --format zip     # reproducible archive + SHA256SUMS
rustnative package windows --format msix    # installable package, identity from rustnative.toml
rustnative package windows --format all --sign cert.pfx --password-env CERT_PASSWORD
```

The zip is byte-identical between builds of the same files, so its
checksums mean something. The MSIX takes its identity, publisher, version,
and URL schemes from the same `rustnative.toml` the running application uses, so a
package cannot disagree with the program inside it.

## Text input

A text field is a node like any other:

**Builder**

```rust
Node::text_input("search", self.query.clone())
```

**Markup**

```rust
<TextInput key="search" value={self.query.clone()} />
```

The current Windows backend uses a native Win32 `EDIT` control. Its value is controlled by component state:

```text
Native EDIT
   ↓
TextChanged event
   ↓
Component state
   ↓
view()
   ↓
Tree diff
   ↓
Native EDIT synchronization
```

Programmatic updates are applied only when the native value differs, avoiding unnecessary feedback loops and cursor disruption.

## Verification

On Windows, run:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
cargo deny check
cargo audit
cargo run -p hello-label
```

CI (`.github/workflows/ci.yml`) runs all of the above, plus an MSRV check,
and — importantly — a `windows-latest` job. That job is what actually
compiles and runs `rustnative-windows`'s `native` module: it is
`#[cfg(windows)]`-gated, so a Linux-only pipeline silently excludes it.

`rustnative-core` is covered by unit tests, integration tests, `proptest`
property tests (identity, reconciliation, layout constraints, scroll ranges,
removal ordering), runnable doc examples on every major public API, and
`criterion` benchmarks including scaling sweeps at 10/100/1k/10k nodes.

`rustnative-windows` is covered against **real** Win32: `native::integration`
creates genuine top-level windows and drives them through the production
message loop (`native::harness` replaces only the outermost blocking
`GetMessageW` with a bounded `PeekMessageW` pump), covering window lifecycle,
reconciliation, reorder, text input, focus traversal, dynamic and modal
window lifecycle, menu dispatch, GDI resource lifetime, scheduler wakeups,
creation reentrancy, stale-event rejection, and every component-panic policy.

Those tests need an interactive window station — true on a developer machine
and on GitHub's `windows-latest` runner, not true under a service account.

A few tests need more than a window station: the real cursor (hover is
decided by where it actually is) or access to the system clipboard. Those are
`#[ignore]`d with a reason, so a restricted shell reports them as *not run*
rather than failing or quietly passing, and CI runs them explicitly:

```powershell
cargo test --workspace -- --ignored
```

## Roadmap

The complete master roadmap—including completed milestones, architectural invariants, and all planned future stages—is maintained in [`PLAN.md`](PLAN.md).

Milestones 25–32 and 34 are complete, and so is the Web track (Web milestones A–K).
What remains is the rest of the platform matrix — macOS, Android, iOS,
embedded, and terminal — plus the
core work those targets share: a `no_std`-capable core subset, an executor
seam for single-threaded hosts, and time from the host clock rather than
`std::time::Instant`. The markup syntax and the style spellings are core work of
the same kind and are scheduled with them, in Tier 0 below.

**Milestone 33 — the macOS backend** is next in numbering, but it needs a
macOS machine to build and verify on and this project has none yet, so the
order follows hardware availability rather than the numbers. macOS and iOS
stay fully planned regardless; nothing in the portable layer is designed as
though they were optional.

Interleaved with the backends, `PLAN.md` section 11 carries the
production-parity milestones (39–69) in four tiers and an optional band:

- **Tier 0 (53, 58, 39, 40), before the second backend exists** — the markup
  syntax, so that every later example, template, guide, doc test, and
  conformance case is written in both syntaxes once rather than retrofitted
  through a corpus that has grown for years; the style spellings, for the same
  reason and because the per-property capability table and unit mapping are
  something every backend has to answer; portable-surface obligations
  (right-to-left mirroring in the layout model, safe areas, permission states,
  gesture arbitration, panic and teardown policy, ownership and escape-hatch
  contracts, plus a typed environment, a command model, per-property native
  mappers, platform-group crates, and capability grants distinct from
  capability availability); and interoperability, so a RustNative tree can be
  embedded in an existing application and a foreign control embedded in ours.
  These cost once now and once per backend later. (Milestone numbers are identities, not an
  order — `PLAN.md` section 8 establishes that convention, and section 11
  sequences these three.)
- **Tier 1 (41–45, 59), continuous, and part of section 8's definition of a
  finished backend** — the conformance suites that turn this framework's
  guarantees into tested ones, including the syntax-equivalence suite that
  keeps the two authoring surfaces from drifting apart, CI-enforced budgets, a
  state-preserving developer loop with previews and development builds, a
  runtime inspection protocol with record and replay, and the headless test
  backend — queried through the accessibility tree — that lets the application
  layer be tested without one machine per target. Milestone 59 finishes the
  toolchain layer around all of it: one platform version with a supported-host
  matrix, project diagnostics, build profiles with credentials the developer
  never holds, store submission, update channels gated by a fingerprint of the
  native inputs, repository-declared pipelines, native-module authoring with
  autolinking, generated launch assets, device pairing, a shareable playground,
  and artifact size attribution — so each device backend inherits a release
  process instead of inventing one. Milestone 61 adds the interaction depth a
  backend is not finished without (scroll coordination, keyboard avoidance,
  gesture physics, the host's text editing surface, directional navigation),
  and Milestone 65 makes the framework's own promises mechanical: an API
  snapshot gate, architecture invariants as tests, fuzzing and flakiness
  control, build provenance and reproducibility, and rollouts that halt on a
  health breach.
- **Tier 2 (46–48, 54), before any public release** — internationalization and
  localization, shared state, error boundaries as supervision, the asynchronous
  data layer, forms and validation, a native component library with a
  design-token pipeline, and responsiveness under load: prioritized,
  interruptible rendering and work that pauses when nobody can see it. Three
  more join it: Milestone 60, the visual pipeline (compositing and damage,
  frame pacing with measured input-to-present latency, colour management, host
  materials, secure capture); Milestone 62, media, capture, and files (audio
  sessions, camera and screen capture, privacy-preserving pickers, background
  transfer); and Milestone 63, trust (account deletion and revocation,
  attestation, a consent contract the data layer enforces, backup
  classification and data export). Two more complete it: Milestone 67, the
  extended style range (guards, declared targets, gradients, transforms,
  effects, motion, text, control parts, and structural variants, each answered
  by every backend); and Milestone 69, Windows scale and composition
  (per-monitor scaling first, then host composition for the boxes the
  framework owns, and the control-set decision named with its criteria).
- **Tier 3 (49–52, 55–57), with and after the Web track** — the server
  application model, deployment and post-ship updates, observability and
  compliance, the stability policy, ecosystem contract, and documentation that
  decide whether the framework gets a second project; reconciliation beyond the
  screen (local-first sync, server-interactive UI, device fleets); durable and
  event-driven execution; and the surfaces beyond the main window — widgets,
  extensions, push, commerce, secure storage, feature flags; product
  operations (links that survive installation, in-product guidance, typed
  analytics, privacy-safe diagnostics); and reach (managed fleets, desktop
  shell integration, delta updates over metered networks, and the field set for
  devices — time and calibration, support bundles, provisioning).
- **Optional (68), on demand** — relational styling on another node's state,
  container queries, generated content, and single-host style passthrough.
  Each is specified as a typed relationship or an explicit escape hatch, and
  each is something the framework already covers another way, so it is built
  when applications ask for it; the framework is complete without it.

They come out of the standing analysis in
[`docs/ecosystem-analysis/`](docs/ecosystem-analysis/), which examines the
framework families this project is measured against from their substrate
choices upward — naming no product or vendor, deliberately — catalogues the
concepts those families introduced and analyses each on its own merits, and
scores this codebase layer by layer and concept by concept against them.
