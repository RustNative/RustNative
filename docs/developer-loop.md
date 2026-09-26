# The developer loop

`PLAN.md` Milestone 43. The time budgets live in `budgets/windows.toml`:
`dev_loop_restart_ms` and `first_run_s`.

## First run

Three commands take you from nothing to a running application:

```sh
rustnative new my-app --syntax markup   # or --syntax builder
cd my-app
rustnative run windows
```

The budget harness times this sequence: `rustnative bench --build-times`,
key `first_run_s`. It measures from `new` until the application is
interactive. The framework's dependencies are already compiled, so their
first download and build are not counted.

A new project is a library holding the application and its previews
(`src/lib.rs`, with its component in `src/app.rsx` for markup), plus a thin
executable (`src/main.rs`). A change to the application recompiles one
crate.

## `rustnative dev windows`

This command builds the application, runs it, and watches the project. For
each save it does one of two things, and prints which:

- **Only `@theme` token values changed in `app.css`:** the new values go to
  the running application, which re-resolves its style. Nothing is rebuilt.
  Tokens are references, so a colour or spacing change reaches the controls
  that exist, in place.
- **Anything else changed** (Rust, `.rsx`, a utility added or removed,
  `Cargo.toml`, `build.rs`, `rustnative.toml`): the application is rebuilt.
  - If the build fails, the running application stays and the errors are
    shown at their `.rs` or `.rsx` position.
  - If the build succeeds, the running application's inspectable state is
    read, and the application is closed the way a person closes it: its
    persisted state is flushed and its window's placement saved.
  - The new build starts, and the state is written back through each
    component's `Component::edit`. You stay where you were.

  The loop prints how long the restart took and how many state fields were
  restored.

`examples/hello-label`'s counter shows the round trip: its count and name
survive a restart. The loop reads and restores state over the inspection
protocol (`docs/inspection.md`), so any component that implements
`Component::inspect` and `Component::edit` keeps its state.

### On another machine

On the machine that runs the application:

```sh
rustnative dev-agent --listen 0.0.0.0:7878   # prints its token
```

On the development machine:

```sh
rustnative dev windows --remote 192.168.1.20:7878 --token <token>
```

Each build is sent to the agent, which starts it and returns its inspection
endpoint. State is read and restored over the network in the same way.

The agent runs whatever a holder of its token sends it, so:

- it listens on loopback unless you give it another address;
- it answers a deployment's header before accepting any bytes, so a refused
  deployment sends nothing;
- it writes only into its own folder.

### Development resources

A resource declared in `rustnative.toml` is created under
`target/rustnative-dev/resources` when it does not exist:

```toml
[resources.uploads]
kind = "directory"

[resources.catalogue]
kind = "file"
seed = "fixtures/catalogue.json"
```

The application receives it as `RUSTNATIVE_RESOURCE_<NAME>`, which
`rustnative_core::dev::resource("uploads")` reads.

### Errors

A development run is started with `RUSTNATIVE_DEV=1`. In that run, a
component panic shows the panic message and where it happened. When the code
was lowered from markup, the position is in the `.rsx` file, found through
the source map the build writes (`rustnative_core::dev`).

### Tests on save

`rustnative test --watch` runs the tests again on every save under `src/` or
`tests/`.

## Previews and the catalogue

An application declares its previews in `previews()`:

```rust
pub fn previews() -> Vec<Preview> {
    vec![Preview::component::<App>("app", ()).with_matrix(PreviewMatrix::full())]
}
```

`PreviewMatrix::full()` covers 48 configurations: both schemes, the default
locale and the pseudo-locale, text scale 1, 1.5, and 2, both directions, and
both contrasts. Widths can be added.

- `rustnative preview` opens the catalogue: the application's own
  executable, run with `RUSTNATIVE_PREVIEW`, showing
  `rustnative_core::preview::Catalogue` on the native backend. A toolbar
  cycles the configuration.
- `rustnative preview --headless` runs `tests/previews.rs`. That test calls
  `rustnative_headless::preview_goldens`, which makes every preview in every
  configuration a golden test.
  - A new preview's golden is written the first time.
  - A changed preview fails until it is re-blessed with
    `RUSTNATIVE_BLESS=1`.

## `rustnative generate`

```sh
rustnative generate component Profile
rustnative generate screen Settings --route /settings
rustnative generate service Storage
```

Each command writes the item in the project's syntax: markup if the project
has `.rsx` files, builder otherwise. It also writes a headless test, and
registers the item in `src/lib.rs`:

- the module declaration;
- the preview;
- for a screen, the route in `router()`, which the first screen creates.

Nothing that exists is overwritten.

`rustnative generate server-resource` belongs to the server application
model (Milestone 49).

## Editor assistance (`rustnative lsp`)

The language server gives markup the same help whether it is written in a
`.rsx` file or inside `rsx!` in a `.rs` file:

- **Elements and attributes:** completion, hover that names the builder
  method an attribute calls, and go-to-definition to that method in the
  framework's source.
- **Class strings** (`class="…"`, `style="…"`, `classes!`, `styles!`):
  completion of the vocabulary's classes, with variants such as `hover:bg-…`
  kept. Hover lists every property a class sets and the token each value
  references, with that token's value in the project's `app.css`.
- **Diagnostics:** a diagnostic that names a class or an attribute is
  narrowed to the word it names, rather than covering the whole string or
  macro call.
- **Structural editing** (`C56`), for a visual designer built on the markup
  tooling: `rustnative/setAttribute`, `rustnative/insertElement`,
  `rustnative/removeElement`, and `rustnative/moveElement`. Each answers with
  a workspace edit that changes only the element it names. Formatting and
  comments elsewhere are kept.

## Toolchains

`rustnative doctor --install` adds the toolchain pieces `rustup` can add that
a command needs, such as `llvm-tools` for `build --pgo`. `--dry-run` prints
the commands without running them. You install the Windows SDK and the MSVC
build tools yourself; `rustnative doctor` says where to get them.

## Owed

- **Device targets:** the loop on a phone or board, board quickstarts, and
  development builds that load the application crate onto a device without
  reinstalling. These are owed with Milestones 35 to 37. The Windows remote
  loop above is the same protocol those targets will use.
- **Dynamic-library reload** (`--hot`) is not built.
  - Reloading the application crate as a separately loaded library would
    duplicate the framework's process-wide state across the library
    boundary: its node-key interner, its thread-locals, and its type
    identities.
  - It is sound only if the framework itself is built as a shared library
    that the host and the application both link. That is a build
    arrangement the project does not have yet.
  - Restart with preserved state is what `rustnative dev` does instead.
- **Live locale catalogue pushes** arrive with Milestone 46's catalogues.
  Until then, a change to a locale file is a rebuild.
