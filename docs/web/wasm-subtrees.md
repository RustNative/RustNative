# WebAssembly subtrees (Web milestones A and F)

Client logic that the client subset cannot express — a parser, a CRDT, a
simulation, a crate from the ecosystem — runs in the browser as full Rust,
compiled to WebAssembly, for the subtrees that opt in. The rest of the page
stays HTML with generated JavaScript, and a page with no WebAssembly subtree
never fetches a module.

## Building one

A crate names its components for the browser once:

```rust
rustnative_web::wasm_subtree! {
    "counter" => rustnative_web::Client<counter::Counter>,
    "notes" => notes::Notes,
}
```

and is built with `crate-type = ["cdylib", "rlib"]` for
`wasm32-unknown-unknown`:

```bash
cargo build -p web-subtree --target wasm32-unknown-unknown --release
```

On every other target the macro is nothing, so the same crate is an
ordinary library to the server and to native builds. Any component works,
including a `#[client]` one (`Client<S>`), whose generated JavaScript and
WebAssembly build are then two realizations of one definition.

## Serving and placing one

```rust
let app = ServerApp::new()
    .wasm_module("web-subtree", include_bytes!("web_subtree.wasm").to_vec())
    .route("/notes", get(notes).public());

// In a page's component:
let props = WasmProps::new("web-subtree", "notes", NotesProps { text: "hi".into() });
context.child_with_props::<WasmSubtree<Notes>, _>("notes", props, WasmSubtree::new)
```

`WasmSubtree<C>` renders `C` natively for the page's first bytes, so the
subtree is there before the module loads, and registers it as an island. The
browser fetches `/_rn/w/{module}.wasm` only on that page, starts the
component from the same props, and checks its first view against the
server's markup (a difference is reported as `rn:mismatch` and repaired). The
page's content security policy gains exactly `'wasm-unsafe-eval'`, and only
on pages that have a subtree.

`.in_worker()` runs the module in a Web Worker instead: the page keeps the
DOM and its events, the worker the logic, and views cross as data.

## Inside

The glue is the framework's own; there is no `wasm-bindgen`. The module
exports `rn_alloc`, `rn_init`, `rn_dispatch`, `rn_pump`, `rn_view`, `rn_out`,
`rn_deadline`, and `rn_complete`, and imports three functions from `rn`:

| Import | What it is |
|---|---|
| `rn_now()` | The page's clock (`performance.now()`), the module's only clock. |
| `rn_wake(delay)` | Asks the runtime to pump the module soon. |
| `rn_request(id, pointer, length)` | A request of the component's services, answered later through `rn_complete(id, …)`. |

Inside, a `ComponentTree` runs on a `HostExecutor` over that clock: timers
and tasks are pumped by the runtime when the module asks, and nothing else
drives it. The component's services are carried out by the runtime:

| Service | In the browser |
|---|---|
| `HttpService` | `fetch` (same-origin requests carry the request-forgery token) |
| `StorageService` | `localStorage` (not in a worker) |
| `ClipboardService` | the Clipboard API (not in a worker) |

Events arrive in the runtime's JSON (the same the generated JavaScript
receives) and become `Event`s; views leave as the same elements every other
part of the page is made of.

A component whose behaviour depends on time (a timer) should start it
where it runs, not while a server renders its first markup: it can tell
from `context.services().extension::<ServerRender>()`, as `Client<S>` does
for its `init`.

## Verified

In headless Edge (`crates/rustnative-server/tests/wasm_browser.rs`): the
example builds for `wasm32-unknown-unknown` and imports only `rn`; a page
without a subtree fetches no module; the subtree attaches without a
difference, handles clicks, typing, and toggles, runs a timer on the host
clock, merges replicated text, and runs in a worker; and the same client
component as generated JavaScript, as WebAssembly, and as WebAssembly in a
worker leaves identical DOM after each step of the same events.
