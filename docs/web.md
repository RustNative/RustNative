# The Web backend

`PLAN.md` Web milestones A–K. A Rust Native application runs in the browser
with the same components, events, and state as on Windows. Its views become
semantic HTML, its styles become CSS, and its logic runs in the browser
compiled to JavaScript or WebAssembly, or on the server.

## Three modes, one application

| Mode | Where the logic runs | What the browser receives |
|---|---|---|
| **Client-side** (a static export) | the browser | HTML rendered at build time, the runtime, and each island's module |
| **Server-rendered** (a long-lived server) | the server renders; islands run in the browser | HTML per request, streamed when parts are slow; islands attach to it |
| **Serverless** (a function, or an edge module) | as server-rendered, one request at a time | the same as server-rendered |

A subtree picks its render mode:

- **static**: HTML only;
- **client**: a `#[client]` component, compiled to JavaScript;
- **WebAssembly**: `wasm_subtree!`;
- **server-interactive**: `Live`, where the state stays on the server and
  the DOM is patched over a socket;
- **automatic**: server-interactive until the client module arrives, then
  handed over with its state.

The same application in the same state has an identical DOM in every mode
(`docs/guarantees.md`, `W-MF-5`).

## Start

```sh
rustnative new hello --web
cd hello
rustnative dev web          # http://127.0.0.1:3000, rebuilds and reloads on save
```

The template is a page with one island. `rustnative dev web` keeps the
island's state across reloads, and shows a failed build's errors over the
page until it is fixed.

## An island

A client component is ordinary Rust in a `#[rustnative_web::client]`
module. It has state, `update` for events, `message` for answers, and a
`view`. It is compiled to JavaScript for the browser and to Rust
everywhere else, so its first view is rendered on the server and its tests
run natively. The subset it may use is `docs/web/client-subset.md`.

The builder syntax, with the typed style:

```rust
#[rustnative_web::client]
pub mod counter {
    use rustnative_core::{Color, Event, Node, NodeId, VisualStyle};
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Counter {
        pub clicks: u32,
    }

    impl Counter {
        pub fn update(&mut self, event: Event, fx: &mut Effects<()>) {
            let _ = fx;
            if let Event::Click { target } = event {
                if target == NodeId::from_key("click") {
                    self.clicks += 1;
                }
            }
        }

        pub fn view(&self) -> Node {
            Node::column(
                "counter",
                [
                    Node::label("count", format!("Clicked {} times", self.clicks)).with_style(
                        VisualStyle::new().background(Color::rgb(0x2b, 0x7f, 0xff)).border_radius(8),
                    ),
                    Node::button("click", "Click me"),
                ],
            )
        }
    }
}
```

The same view in markup, with the utility classes:

```rust
pub fn view(&self) -> Node {
    rsx! {
        <Column key="counter">
            <Label key="count" text={format!("Clicked {} times", self.clicks)}
                class="bg-blue-500 rounded-lg" />
            <Button key="click" text="Click me" />
        </Column>
    }
}
```

The typed style, the utility classes, and the declarations
(`styles!("background: …")`) produce the same rules. They are compiled at
build time into the page's style sheet, and there is no style work at run
time. How nodes and styles map to HTML and CSS is in
`docs/web/dom-mapping.md` and `docs/web/layout-mapping.md`.

## A page

A page is a component tree rendered for one request. It has a `Head` (title,
description, language alternates) and a strategy:

- whole;
- streamed: the slow parts follow the first bytes;
- partially prerendered: a cached shell, with holes filled per request.

```rust
struct Home;

impl Component for Home {
    // …
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let counter = context.child_with_props::<Client<counter::Counter>, _>(
            "counter", counter::Counter::default(), Client::new);
        Node::column("home", [Node::label("greeting", "Hello"), counter])
    }
}
```

```rust
fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
    let counter = context.child_with_props::<Client<counter::Counter>, _>(
        "counter", counter::Counter::default(), Client::new);
    rsx! {
        <Column key="home">
            <Label key="greeting" text="Hello" />
            {counter}
        </Column>
    }
}
```

The application serves it:

```rust
ServerApp::new()
    .client::<counter::Counter>()
    .route("/", get(|| async { Page::new::<Home>(Head::new("Hello", "A page."), ()) }).public())
```

- A page with no island ships no JavaScript.
- A route loads only the modules its islands use.
- A component can wait for its data. The render waits with it, up to the
  page's budget; past the budget, a `pending` boundary shows its fallback.

The wire format between the server and the runtime is
`docs/web/wire-format.md`.

## The server, from the browser

- **Server functions.** `fx.call::<AddNote>(title, Msg::Added)` from an
  island, served by `server_fn` / `server_fn_with` and checked for access
  and CSRF like any route. A call from an older build is told to reload.
- **Forms** work without JavaScript. `form(context, "sign-in", "/sign-in",
  …)` posts with the CSRF token, and the island enhances it.
- **Links** are real `href`s. The runtime navigates without a reload,
  prefetches, and restores scroll and state on back and forward.

## Capabilities

An island reaches the browser through `Effects`: HTTP, storage and
IndexedDB, the clipboard, files, sharing, location, permissions, sockets,
workers, media, sensors, Bluetooth, and vibration. Each needs a declared
capability, and the page's `Permissions-Policy` opens only those. See
`docs/web/capabilities.md`.

## WebAssembly subtrees

`wasm_subtree!` compiles a component tree to one WebAssembly module. It can
run on the page's thread or in a worker, and its HTTP, storage, and clipboard
go through the page. The same component as JavaScript and as WebAssembly has
an identical DOM after the same events. See `docs/web/wasm-subtrees.md`.

## Offline and installable

`ServerApp::pwa(…)` adds a manifest, icons, and a service worker:

- assets are precached;
- pages are served network-first;
- server calls are queued while offline and replayed when the network
  returns;
- updates are applied when the person accepts them.

See `docs/web/offline.md`.

## Loading

- Images are served responsive, as WebP.
- Fonts are subset to the characters the application uses, and preloaded
  with a metric-matched fallback, so there is no layout shift.
- Scripts are split per route.
- `budgets/web.toml` holds the route's script size, startup, LCP, CLS, and
  INP. `rustnative bench --target web` measures them in Edge on a throttled
  profile.
- `?_rn_explain` on a page says which parts are static and which islands it
  ships.

## Build, export, deploy

```sh
rustnative build web --mode client                     # static files, with _headers and a sitemap
rustnative serve static target/web/client              # as a static host serves them
rustnative build web --mode server --release
rustnative package web                                 # the server, one archive
rustnative build web --mode serverless --host lambda   # a function
rustnative build web --mode serverless --host wagi     # an edge module
rustnative serve lambda target/web/lambda/bootstrap    # locally, as the host runs it
rustnative serve wagi target/web/wagi/hello.wasm
rustnative deploy export sam                           # or spin; container, compose, kubernetes, systemd
```

Every shape, its limits, and deployment with preview, promotion, and
rollback are in `docs/web/deploy.md`. The security model is
`docs/security/threat-model-web.md`.

## Other ways in

- **Custom elements.** `rustnative_web::element::CustomElement::new::<S>("my-counter")`
  makes a client component a custom element, usable on a page the framework
  did not render.
- **Source maps.** They lead generated JavaScript back to the Rust it came
  from.

## Testing

- An island's logic is Rust. It can be tested natively, and the headless
  backend renders its views.
- A page renders with `rustnative_web::page::render`, or through
  `AppService::handle`, with no socket.
- `rustnative-web-testing` drives headless Edge or Chrome over the DevTools
  protocol: clicks, typing, the accessibility tree, offline, throttling.
  `rustnative test --browser` runs the suites that need it.

Only Chromium engines are tested; Firefox and Safari are not.
