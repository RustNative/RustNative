# The web wire format (Web milestone H)

What crosses between the server and the browser, and how a change to it is
survived. Everything is JSON with serde's default shapes: an `Option` is
`null` or the value, a unit variant is its name, a variant with data is
`{"Variant": data}`, a `Result` is `{"Ok": v}` or `{"Err": e}`.

## The page

A server-rendered page is ordinary HTML. When it has interactive subtrees,
the end of `<body>` holds the page data and one module script, the runtime:

```html
<script type="application/json" id="rn-data">{ "config": {…}, "islands": [ … ] }</script>
<script type="module" src="/_rn/rn.<hash>.js" nonce="…"></script>
```

A page with no interactive subtree has neither (selective attachment,
`W-IS-1`). `<`, `>`, `&`, U+2028 and U+2029 in the data are JSON escapes, so
nothing in it can end the element.

`config`:

| Field | Meaning |
|---|---|
| `base` | The application's path prefix; server calls go to `{base}/_fn/{path}`. |
| `assets` | Where the runtime and client modules are served (`/_rn/`). |
| `version` | The build's version, sent with every server call (below). |

Each island:

| Field | Meaning |
|---|---|
| `i` | Its index. Its root element has `data-rn-i="{i}"`, and every element inside has the id `i{i}-{key}`. |
| `kind` | `client` (generated JavaScript), `live` (held on the server), or `wasm` (a WebAssembly subtree). |
| `s` | Its state: the client component's state, or a live subtree's props. |
| `flow` | How its parent lays it out: `{"t":"column","a":…}`, `{"t":"row","a":…}`, `{"t":"grid"}` or `{"t":"root"}`. |
| `m` | Its client module's URL (`/_rn/m/{name}.{hash}.js`); for `live`, the module that takes over in `Auto` mode. |
| `fns` | The server functions its module calls: type name to path. |
| `url` | For `live`: the WebSocket path of its sessions. |
| `fresh` | The server left its markup to the browser (a client-only page). |

The runtime and each module are served with `Cache-Control: public,
max-age=31536000, immutable`: their names carry the hash of their contents,
so a new build is a new URL. A page is served `Cache-Control: no-store`,
because it carries the request's nonce and token.

## Streamed boundaries

A streamed page sends its shell at once, each `pending` boundary showing its
fallback (marked `aria-busy`). Each boundary follows on the same response
when its data arrives:

```html
<style nonce="…">/* rules the new content needs */</style>
<template id="rn-f-{id}">…the boundary's element…</template>
<script nonce="…">rnFill("{id}")</script>
```

`rnFill` is defined once, in the shell. The page data and runtime come
last, so islands inside a filled boundary attach with the rest.

## Server functions

A call is `POST {base}/_fn/{path}` with the input as its JSON body, and:

| Header | Value |
|---|---|
| `content-type` | `application/json` |
| `accept` | `application/json` |
| `x-csrf-token` | The `__Host-csrf` cookie's value (request-forgery protection). |
| `x-rn-fn-version` | The page's build version, when the application sets one. |

The answer is `200` with the output as JSON, or an error status with
`{"error": "message"}`. In client logic it arrives as
`Result<Output, ServerFnError>`: `Server { status, message }` for an error
status, `Decode(..)` for an answer that did not parse, `Transport(..)` for a
request that did not complete.

### Versioning

The wire format of a server function is its input and output types. A page
loaded before a deploy can call a server of the next build: when the
application sets a version (`ServerApp::version`) and the call carries
another, the server answers `409` with `{"error": "version", "version":
"<the server's>"}` and does not run the function. The runtime reloads the
page, which then speaks the new format. A server function whose types
change in a compatible way (a new optional field) needs no new version.

## Forms without JavaScript

A `form` renders as `<form method="post" action="…">` with a hidden
`_csrf` field carrying the token. Its inputs are named by their keys, and
its buttons are submit buttons named, and valued, by theirs. The body is
`application/x-www-form-urlencoded`; the handler reads it with `Form<T>`.

## Live subtrees

A `live` island speaks `rustnative-sync`'s frames over a WebSocket at its
`url` (on the application's own port and origin; a connection from another
origin is refused).

| Frame | From | Fields |
|---|---|---|
| `hello` | browser | `session` (to resume, or `null`), `snapshot` (the state to start from), `dom` (`{scope, flow}`: send elements) |
| `event` | browser | `event` (`click`, `text_changed`, `toggled`, `value_changed`, with `target` the element id without the scope), `sequence` |
| `welcome` | server | `session` |
| `dom` | server | `element` (the subtree's element), `rules` (the `(class, rule)` pairs it needs), `acknowledged` (the last event applied), `snapshot` |
| `drain` | server | `to`, `after_ms`, `snapshot`, `acknowledged`: this instance is going away |

A dropped connection reconnects with its session id and resends the events
not yet acknowledged; within the grace period the session is still there.
In `Auto` mode, once the client module has loaded, the runtime starts it
from the last `snapshot` and closes the socket.
