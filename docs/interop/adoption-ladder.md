# The adoption ladder

`PLAN.md` Milestone 40: a team can adopt Rust Native one rung at a time — a
library first, then an embedded subtree, then a whole application — and each
rung has a worked example under test.

| Rung | What changes in the existing application | Example | Test |
|---|---|---|---|
| 1. **Library** | Nothing but a DLL and a generated binding: the application model — state, rules, services — runs with no UI, and the host calls it from C, C#, or any language with a C FFI. | `examples/adoption-library` (a component driven through `counter.ril`) | `tests/hosts.rs`: a C program built with MSVC against the generated header, and a C# program built with the .NET Framework compiler against the generated bindings, both driving the DLL |
| 2. **Embedded subtree** | The host keeps its own window, class, and message loop, and gives part of its window to a Rust Native tree: `WindowsPlatform::embed`, one call in its loop (`handle_message`), and `set_bounds` when it lays itself out. | `examples/adoption-subtree` (a `windows-sys`-only Win32 program) | `tests/subtree.rs`: embeds, clicks the embedded button through plain Win32, resizes the host, drops the subtree, and checks each from the host's side |
| 3. **Full application** | The application is Rust Native, and hosts the controls it cannot rewrite yet as foreign objects (`Node::foreign`, `register_foreign`). | `examples/adoption-foreign` (the system month calendar, owned, and date picker, borrowed) | `tests/foreign.rs`: both adopted at their factories' sizes, then removed — the owned one destroyed, the borrowed one handed back |

## Rung 1: the interface description

One `.ril` file describes the library's surface — services, their
constructors, methods, and events, with ownership and threading annotated —
and every binding is generated from it, never written by hand per language:

```sh
rustnative bindgen counter.ril --lang c        # the C header
rustnative bindgen counter.ril --lang csharp   # P/Invoke bindings, C# 5
rustnative bindgen counter.ril --lang rust     # the implementation shims
```

The implementing crate's `build.rs` calls `rustnative_interop::generate_rust`,
implements each service's trait, and calls `export_<library>!`. The shims
contain every failure at the boundary — a panic, a call from the wrong thread,
a stale handle, invalid UTF-8 — as a status code with a message, never a
crash; an event handler that calls back into the instance raising it gets
`BUSY` rather than a deadlock. A service whose every method is owner-affine
may hold a `!Send` model (a component tree); one with an `[thread = any]`
method must be `Send`.

## Rung 2: the host's loop

```text
while GetMessageW(&mut msg, null, 0, 0) > 0 {
    if !root.handle_message(&msg)? {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}
```

Dropping the `EmbeddedRoot` destroys the framework's windows and nothing of
the host's. Where the framework would end the loop (its primary window
closing, a native error) under a host's loop it records the request instead
(`ExternalLoop::finished`, `ExternalLoop::error`) — posting `WM_QUIT` into a
loop it does not own would end the host. The same machinery without a parent
window is **guest-runtime mode** (`WindowsPlatform::start_external`): the
framework's windows, the host's `main` and loop.

## Rung 3: foreign objects

`register_foreign(kind, preferred_size, factory)` on the UI thread;
`Node::foreign(key, kind, layout)` (`<Foreign key="…" kind="…" />`) in the tree.
The framework creates the object when the node is inserted, sizes it from the
factory's preferred size, lays it out and clips it, and on removal destroys it
(`Ownership::Owned`) or hides it and hands it back under a message-only parent
(`Ownership::Borrowed`) — including when the whole window closes, before
Windows would destroy its children with it. Its accessibility is its own: the
framework neither subclasses it nor changes its tab stop.

## Owed

- **The web rung** — a component exported as a web custom element (`C43`) —
  is owed by Web milestone B, with the web backend.
- The other backends' embedding (a view, a widget, a document node) is owed by
  Milestones 33–38.
