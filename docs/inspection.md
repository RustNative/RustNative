# Inspection and diagnostics

`PLAN.md` Milestone 44. A compiled framework does not inherit introspection
from a runtime, so the runtime exposes its own: one protocol, answered by every
backend, spoken by the `rustnative inspect` client and by an overlay drawn
inside the application.

## The protocol

`rustnative_core::inspect`. A `Request` goes in and a `Reply` (`{"ok": …}` or
`{"error": "…"}`) comes out. `Application::inspect(request, backend)` answers
it. The core answers everything about the tree. The backend, through
`InspectBackend`, answers what only it knows: its objects, their rectangles, its
lifetimes, its capability and style tables, and its mappers.

| Request | Answer |
|---|---|
| `hello` | protocol version, backend, open windows |
| `tree` | the declarative tree: each node's key, kind, text, owning component's key path, and the classes or declarations applied to it as written |
| `components` | every component: key path, Rust type, parent, state (`Component::inspect`), unfinished tasks |
| `realized` | the host objects and the node each realizes. On Windows these are the window class, `HWND`, and the rectangle Win32 has |
| `state` / `set_state` | one component's state. Editing goes through the message the component names (`Component::edit`), so an edit is delivered and rendered like any other message |
| `explain_layout` | the node's rectangle and why: its parent and container style, its width and height modes against what it was given or measured, constraints, margin, alignment. Uses the backend's own rectangles when it has them |
| `explain_style` | each property's source by precedence level (`C18-2`): the **class** that set it (named as written), a **declaration** (`styles!`), a **typed override** set in code, or the **component default** (the theme's default for the kind). Also shows the token it references with its resolved value, the condition it is written under (`hover:`, `dark:`, `md:`), and whether it applies now |
| `trace` | events and task deliveries — and failures an error boundary contained — with their cost and the render pass each caused. Each component either **rendered**, with its cause (`event`, `message`, `props`, `environment(<key>)`, …), or was **skipped** because nothing it depends on changed (`C04-2`) |
| `tasks` | unfinished tasks per component, which are cancelled when it unmounts |
| `lifetimes` | host objects created, destroyed, and live, with the most recent of each |
| `stores` | every live `Store::inspectable`, with its version and value (`docs/data.md`) |
| `capabilities` | what the host advertises. Also what it does not, and which services the application did not provide, each with a reason. Includes the backend's style table (realized / approximated / unavailable, with its reason) and unit mapping |
| `mappers` | active per-property mapper customizations (`C24-2`) |
| `history` | inspectable state after each change, oldest first: stepping back through state |
| `overlay` | shows or hides the in-application overlay |
| `start_recording` / `stop_recording` | see below |

A node is named by its key (first match in document order), by
`<component path>::<key>`, or by the numeric id the tree reports.

Tracing and history cost nothing until something turns inspection on: a
server, an overlay, a recording, or any request.

## The transport

`InspectServer` carries line-delimited JSON over TCP. Each client line is
`{"token": …, "version": 1, "request": {"request": "tree", …}}`; each server
line is a `Reply`.

- **Off by default.** Starting the application with `RUSTNATIVE_INSPECT=1`
  listens on loopback, on a port the system picks.
  `RUSTNATIVE_INSPECT=<addr:port>` binds that address instead, which is how you
  inspect a machine on the network (the token is still required). An
  application can also call `Application::enable_inspection`.
- **Authenticated.** The token is 128 bits, generated per process. It reaches
  the client out of band, in the endpoint file
  `<temp>/rustnative-inspect/<pid>.json`. The file is removed when the server
  stops. A connection that presents the wrong token gets one refusal and is
  closed.
- **Versioned.** A request for another protocol version is refused, not
  answered wrongly.
- **Answered on the UI thread.** Connections are served on their own threads.
  Requests wait for the UI thread, which the server wakes through the primary
  window's scheduler waker. On Windows that is the same `WM_FRAMEWORK_SCHEDULE`
  that delivers task results, so it is also answered inside the host's modal
  loops. The application is never touched off its thread.

## The client

```sh
rustnative inspect tree                      # the most recently started inspectable app
rustnative inspect --pid 4242 components
rustnative inspect --addr 192.168.1.20:50111 --token … hello
rustnative inspect explain save              # why `save` is where it is
rustnative inspect style heading             # where each style property came from
rustnative inspect set my_app::Root/counter count 5
rustnative inspect trace --since 40
rustnative inspect caps
rustnative inspect overlay layout            # layout | events | frame-cost | off
rustnative inspect record --redact password
rustnative inspect stop --out session.json
rustnative inspect to-test session.json --launch "my_app::Root::new(())" --out tests/session.rs
```

Output is text by default. `--json` prints the protocol's JSON.

## The overlay

For a host an external client cannot attach to, `Application::set_overlay`
(or `inspect overlay`) draws the inspector's views over the application:

- `layout`: every node's rectangle and key;
- `events`: the latest event targets, newest brightest;
- `frame-cost`: one bar per recent change against a 16.7 ms line.

The core builds it as an ordinary `DrawList` (`Application::overlay_draw_list`).
The backend draws it the way it draws a canvas, so no backend needs a second
renderer:

- On Windows it is a canvas-class window, made a layered, click-through,
  topmost, non-activating popup over the client area, with its background
  keyed out. It follows the window and is destroyed when hidden.
- On headless it is `HeadlessApp::overlay()`.

## Record, replay, and time travel (`C61`)

State changes only through messages, time only through the clock service, and
work only through the executor. So the same input and the same service
responses, replayed against the same program, reach the same state.

- **Recording.** A recording holds:
  - the input: clicks, focus, text, keys, and resizes. Pointer streams and
    gestures are noted as unreplayable;
  - when each input arrived, by the clock service;
  - the HTTP exchanges seen by a `RecordingHttp`, which wraps the real service
    and hands its tape to `Application::record_http`;
  - the inspectable state it ended in.

  Nodes are named by key and by their component's path from the root, so a
  recording made by one binary replays in another.
- **Redaction.** Text entered into a node whose key contains a redacted pattern
  is recorded as `[redacted]`. Authorization and cookie headers are never
  recorded. A redacted input refuses to replay, rather than typing
  `[redacted]` into the field.
- **Replay.**
  - `HeadlessApp::replay` is deterministic: virtual time advances between
    inputs, and the recorded HTTP responses stand in for the server.
  - `Recording::replay` drives any `Application`, including one on the
    originating backend.
- **Time travel.** The inspector's `history` holds each inspectable state
  after every change. Replaying a prefix of a recording on the headless backend
  reproduces any earlier point exactly.
- **To a test.** `Recording::to_test` (`inspect to-test`) writes a `#[test]`.
  The test replays the recording on the headless backend and asserts the final
  state. `crates/rustnative-headless/tests/replayed_session.rs` is one,
  generated and checked in; `RUSTNATIVE_BLESS=1` regenerates it.

## The reduced form (`inspect::compact`)

For a target with no second screen and no room for formatting code, each
diagnostic is a frame:

- a message id (LEB128) from a format table both sides share;
- the argument count;
- each argument, tagged as integer or UTF-8 text.

The host formats the frames. `encode_trace` and `format_trace` are the trace's
two sides, tested round trip against the JSON form (which is larger).

## Backends

| | Windows | Headless |
|---|---|---|
| Answers the protocol | yes (`native::inspect`, polled on the primary window's wake) | yes (`HeadlessInspect`, polled as it settles; `HeadlessApp::inspect`) |
| Realized objects | window class, `HWND`, Win32 rectangle | the model's node, kind, rectangle |
| Lifetimes | the renderer's registry: counts and recent creations/destructions | counts |
| Overlay | layered click-through canvas popup | `HeadlessApp::overlay()` |
| Replay | `Recording::replay` against the real `Application` | deterministic, with virtual time and recorded HTTP |
| Tests | `native::inspect_integration` (over the real transport) | `tests/inspection.rs`, `tests/replayed_session.rs` |

## Owed

- **Terminal and embedded backends** answer the protocol in reduced form: over
  a side channel for the terminal, and over a probe or serial line through
  `compact` for embedded. They are owed with their backends (Milestones 36 and
  37). So are emitting the trace to existing embedded trace formats and
  debugger kernel-awareness (`C92`).
- **Device recordings** with sensor inputs, replayed in the host-side simulator
  (`C88`), are owed with the device backends.
- **Recorded services.** Only HTTP is recorded. Storage and clipboard
  responses are not yet.
- **Multi-window.** Only the primary window's runtime answers `realized` and
  `lifetimes` on Windows.
- **Scroll offsets** are not applied to the Windows overlay's
  rectangles.
