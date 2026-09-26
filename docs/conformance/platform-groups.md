# Platform-group crates (`C65`, Milestone 39)

Shared code between backends lives in a crate for the *group* of hosts that
genuinely agree, defining trait contracts its members implement — never
copied from one backend into another. The decision is made before a group's
second member is written; the crate is created when that member is.

| Group | Members | What they share | Crate, when its second member lands |
|---|---|---|---|
| Apple | macOS (33), iOS (36) | Objective-C runtime bindings, Core Text measurement, `NSAccessibility`/`UIAccessibility` role mapping, the Apple toolchain driver | `framework-apple` |
| Draw-list | terminal (38), embedded displays (37), the in-application overlay (44) | rasterizing `DrawList`, cell/pixel geometry conversion, a focus-ring and hit-test model for hosts with no native controls | `framework-drawlist` |
| Desktop shell | Windows, macOS, Linux | tray/menu-bar extras, jump lists and dock menus, document windows (Milestones 48, 57) | `framework-desktop-shell` |

Rules:

- a group is defined by what its hosts *agree on*, not by market;
- the group crate depends on `rustnative-core` only; members depend on it;
- a backend never depends on another backend.

No group crate exists yet: Windows is the only native backend, and a
one-member group is exactly the premature abstraction this rule avoids.
