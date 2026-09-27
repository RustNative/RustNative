# Input, navigation, and browser capabilities (Web milestones D, F, G, E)

## Input (D)

The runtime turns the browser's events into the framework's, at each
island's root:

| DOM | `Event` |
|---|---|
| `click` | `Click` (a tab: `TabSelected`) |
| `input` on a text control | `TextChanged` (every change, including during a composition) |
| `input` on a slider or spinner | `ValueChanged` |
| `change` on a check box, radio, select, date | `Toggled`, `SelectionChanged`, `DateChanged` |
| `keydown`, `keyup` | `KeyDown`, `KeyUp`, with `KeyCode` and modifiers |
| `focusin`, `focusout` | `FocusGained`, `FocusLost` |
| `pointerdown/move/up/cancel` | `PointerDown/Move/Up/Cancel`, with a `PointerEvent` in the target's own coordinates |
| `pointerover`, `pointerout` (crossing the node's boundary) | `PointerEnter`, `PointerLeave` |
| `wheel` | `Wheel`: `Pixels`, or `Lines` in 1/120ths of a notch; positive `y` scrolls down |
| `compositionstart/update/end` | `Composition`: `Started`, `Updated { text, cursor }`, `Committed { text }`, `Cancelled` |
| `copy`, `cut`, `paste` | `Clipboard`: `Copy`, `Cut`, `Paste { text }` |
| `visibilitychange` | `Lifecycle(Suspending)`, `Lifecycle(Resuming)` |

A handler that drags asks for the pointer: `fx.capture_pointer(key, id)`
routes the pointer's events to `key` until `fx.release_pointer` or the
pointer is lifted, wherever it goes. Tab order is document order: the
framework adds `tabindex` only where the accessibility model makes a node
focusable that HTML does not.

An event reaches the server only when the handler it triggers calls a
server function (or the subtree is held on the server).

## Scheduling (F)

Events update state at once; the view is realized and patched once per
microtask, however many events arrived in it; effects run after the patch.

## Navigation (G)

- Every route is a real URL, rendered on the server (a deep link works, and
  so does a page with scripts off).
- A link made with `rustnative_web::form::link` is a real `<a href>`. On a
  page with the runtime, a same-origin link (and `fx.navigate`) fetches the
  next page and swaps it in: its body, its title and head metadata, its
  styles (through the CSS object model, so the policy's nonce is not
  needed), and its islands, while the runtime and its modules stay loaded.
  Anything else — another origin, a download, a response that is not a
  page — is an ordinary navigation.
- Back and forward restore each page and its islands' state (kept per
  history entry in `sessionStorage`), and its scroll position; a page left
  for good (`pagehide`) keeps its entry's state for a return through the
  browser's own history.
- A `Persisted<S>` client component keeps its state in `localStorage`, so
  it comes back as the person left it.
- Typed query parameters (`C13-2`): `request.query_as::<T>()` reads the
  query as a `T` (each value parsed as its field's type, with the type's
  defaults for what is missing), and `query_string(&value)` writes one; the
  server's `Query<T>` uses the same reading.

## Capabilities (E)

Each capability is an effect. In the browser the runtime carries it out
with the platform API; natively, where the core has a service contract,
the component's services do, and otherwise the reply says the capability
is unavailable.

| Effect | Browser API | Natively |
|---|---|---|
| `http_get`, `http_post`, `fetch` | `fetch` (same-origin requests carry the token) | `HttpService` |
| `store`, `load` | Web Storage | `StorageService` |
| `db_put`, `db_get` | IndexedDB | `StorageService` |
| `cache_put`, `cache_get` | Cache Storage | unavailable |
| `copy`, `read_clipboard` | Clipboard | `ClipboardService` |
| `notify` | Notifications | `SystemService::notify` |
| `permission`, `request_permission` | Permissions | unsupported |
| `share` | Web Share | unavailable |
| `locate` | Geolocation | unavailable |
| `open_file`, `save_file` | a file input; File System Access where present, a download where not | unavailable |
| `download` | a download | unavailable |
| `socket_open`, `socket_send`, `socket_close` | WebSocket | unavailable |
| `worker` | a module Web Worker | unavailable |
| `media` | media devices | unavailable |
| `bluetooth` | Web Bluetooth | unavailable |
| `sensor` | the Generic Sensor API | unavailable |
| `vibrate` | Vibration | nothing |
| `online` | `online`/`offline` | always online |
| `capture_pointer`, `release_pointer` | pointer capture | the tree's input requests |

`fx.capabilities(reply)` (and `rn.caps()` in the runtime) answers which of
them this browser has, in this security context, under this page's
permissions policy. Bluetooth and sensors are probed, not assumed: a
desktop browser can have the API and no radio or sensor behind it.

### The permissions policy (`C69-1`)

Every page carries a `Permissions-Policy` that closes the powerful
features — camera, microphone, geolocation, Bluetooth, the motion and
light sensors, clipboard reading, serial, USB, HID, MIDI, payment, and
screen capture. `ServerApp::capabilities(&[..])` opens the ones the
application declares, to its own origin only:

| Capability | Features |
|---|---|
| `Camera` | `camera` |
| `Microphone` | `microphone` |
| `Location` | `geolocation` |
| `Bluetooth` | `bluetooth` |
| `Sensors` | `accelerometer`, `gyroscope`, `magnetometer`, `ambient-light-sensor` |
| `Clipboard` | `clipboard-read` |
| `SystemShare` | `web-share` |
| `SerialPorts` | `serial` |

What is closed cannot be reached by the application's own pages, an
embedded third party, or injected script.

## Verified, and where

In headless Microsoft Edge: an input method's composition and its final
`TextChanged`; Tab order across islands; pointer capture delivering moves
and the release outside the node; client-side navigation keeping the
runtime, with back and forward restoring each page's island state; a deep
link with a typed query, with scripts off; lifecycle events (the test
changes `document.visibilityState` itself, because a headless browser
never hides its page); storage, the clipboard with its permission granted,
and HTTP; the capability answer (no Bluetooth or sensors) and the policy
closing an undeclared feature. The same client logic runs natively and in
Node with the same pointer, wheel, composition, and clipboard events and
the same capability replies. Firefox and Safari are not available here;
hardware-bound capabilities (Bluetooth, sensors, camera) are implemented
and capability-answered, not exercised.
