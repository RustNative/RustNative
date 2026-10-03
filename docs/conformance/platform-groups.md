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

No group crate exists yet.

**Desktop shell, decided at its second member (Linux, Milestone 34).** With
Windows and Linux both written, what the two backends actually share for the
shell surfaces was compared:

| Surface | Windows | Linux | Shared beyond `rustnative-core` |
|---|---|---|---|
| Tray icon and its menu | `Shell_NotifyIconW`, a popup `HMENU` | a StatusNotifierItem and a `com.canonical.dbusmenu` menu over D-Bus | nothing: the portable model (`surfaces::SurfaceCommand`, `TrayMenuItem`, `Event::SurfaceAction`) is already in the core |
| Taskbar progress | `ITaskbarList3` | the `LauncherEntry` D-Bus signal | nothing |
| Jump list | `ICustomDestinationList`, at run time | the desktop entry's actions, fixed at packaging | nothing (Linux does not advertise it) |
| Single instance, deep links | a named mutex and `WM_COPYDATA` | a bus name and `org.freedesktop.Application` | nothing |
| Menu bar | `HMENU` | `GMenu` | the core's `MenuBar` model |

Every shared piece is a portable model, and every model is in the core; what
remains per backend is entirely the host's API. A `framework-desktop-shell`
crate would hold no code, so it is **not created**. The decision is
revisited when macOS (Milestone 33) arrives: its dock menu and status item
are the most likely place for a third member to share a host-level
algorithm (menu diffing against a live native menu, say) with one of the
other two.
