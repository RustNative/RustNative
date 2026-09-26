# Host content controls

`PLAN.md` Milestone 48 (`C28`). Some content is best shown by the host's
own player:

- embedded web pages;
- audio and video, with the host's transport controls;
- a camera's live preview.

The framework shows these as capability-guarded nodes.

## Using one

```rust
use rustnative_core::{HostContent, LayoutStyle, Node, host_content};

let capabilities = platform.capabilities();
let video = HostContent::Media { source: "intro.mp4".into() };
let node = host_content("intro", &video, LayoutStyle::new(), &capabilities, || {
    Node::label("intro-fallback", "Video playback is not available here")
});
```

In markup, the same player (where the capability is present):

```rust
rsx! { <Foreign key="intro" kind={video.kind()} /> }
```

- `host_content` builds a `Node::foreign` of the content's kind
  (`HostContent::kind`) when the platform has the capability
  (`HostContent::capability`).
- When the platform lacks it, `host_content` builds the fallback instead.
  A missing player is a stated alternative, not an empty rectangle.
- Because it is a foreign node, it is laid out, clipped, and destroyed by
  the same rules as every foreign object (Milestone 40).
- Its accessibility is the host control's own.

## On Windows

| Content | Capability | Realized as |
|---|---|---|
| `Media` | `MediaPlayback`, always | an `MCIWnd` window (`msvfw32`): the system media control, with its own play bar, seeking, and volume |
| `Camera` | `Camera`, when a capture driver is installed | an `avicap32` capture window, connected to the driver and previewing |
| `Web` | not offered | the application's fallback |

- **Web content.** Web content needs WebView2. Its loader is not part of
  the Windows API surface this backend binds, so the backend does not
  claim `WebContent`. `BUILD_STATUS.md` records this.
- **Media.** The system media transport controls (SMTC) and
  picture-in-picture are WinRT APIs. They are owed.

## Adding another kind

1. Add a variant to `rustnative_core::HostContent`, with its capability and
   its kind prefix.
2. In each backend:
   - realize the kind in its foreign-object path (on Windows,
     `native::host_content`);
   - advertise the capability only where it is realized.
3. Write a test that the node is the host's control where the capability is
   present, and the fallback where it is not. On Windows, see
   `native_host_content_is_the_hosts_player_or_the_stated_fallback`.

An application's own content does not need a variant. It registers a
factory for a kind of its own with `rustnative_windows::register_foreign`.
