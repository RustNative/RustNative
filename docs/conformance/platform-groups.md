# Platform-group crates (`C65`, Milestone 39)

Shared code between backends lives in a crate for the *group* of hosts that
genuinely agree, defining trait contracts its members implement — never
copied from one backend into another. The decision is made before a group's
second member is written; the crate is created when that member is.

| Group | Members | What they share | Crate, when its second member lands |
|---|---|---|---|
| Apple | macOS (33), iOS (36), iPadOS (70) | Objective-C runtime bindings and the retain/release convention, Core Text measurement, `NSAccessibility`/`UIAccessibility` role mapping, Foundation locale facilities, the Keychain, the Apple toolchain driver | `rustnative-apple` |
| UIKit (inside Apple) | iOS (36), iPadOS (70) | UIKit realization of node kinds, the scene bridge and the idiom launcher of a universal bundle, `UITextInput`, the touch, gesture, and drag bridge, `UIAccessibility` elements, `CADisplayLink` pacing, the UIKit services both realize identically | `rustnative-uikit` |
| Draw-list | terminal (38), embedded displays (37), the in-application overlay (44) | rasterizing `DrawList`, cell/pixel geometry conversion, a focus-ring and hit-test model for hosts with no native controls | `rustnative-drawlist` |
| Android | Android (35) only | nothing to share: no other host runs the Android framework | none |
| Desktop shell | Windows, macOS, Linux | tray/menu-bar extras, jump lists and dock menus, document windows (Milestones 48, 57) | `rustnative-desktop-shell` |

Rules:

- a group is defined by what its hosts *agree on*, not by market;
- the group crate depends on `rustnative-core` only — and, for a group nested
  inside another, on the outer group's crate; members depend on it;
- a backend never depends on another backend.

No group crate exists yet.

**Apple and UIKit, decided ahead of their members (2026-10-04).** iOS and
iPadOS are separate targets — separate backends, capability tables, style
tables, conformance columns, and budgets (`PLAN.md` Milestones 36 and 70) —
that share a toolkit, a compiler target (`target_os = "ios"`), and, for a
universal application, one bundle. What they agree on is the whole of UIKit's
realization; what differs is the host around it (windows the person arranges,
the keyboard, pointer, and pen as ordinary input, popover presentation). Both
are planned to be built together, so the UIKit group is decided now and its
crate is created with the first of the two, rather than when the second lands —
the one exception to the rule above, made because waiting would only mean
moving code between crates once both exist. The Apple group's crate is created
with the first Apple backend for the same reason. Neither backend depends on
the other: a universal bundle links both, and `rustnative-uikit`'s launcher
selects one by the device's interface idiom before any tree is realized.
The plan is `docs/superpowers/plans/2026-10-04-ipados-milestone-70.md`.

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
remains per backend is entirely the host's API. A `rustnative-desktop-shell`
crate would hold no code, so it is **not created**. The decision is
revisited when macOS (Milestone 33) arrives: its dock menu and status item
are the most likely place for a third member to share a host-level
algorithm (menu diffing against a live native menu, say) with one of the
other two.

**Android, decided at its first member (Milestone 35, 2026-10-09).**
Android is the only member of the Android family: no other host runs its
framework, its Java host library, or JNI. What it has in common with iOS
and iPadOS is what any mobile host has — the five permission states, the
suspend/resume/terminate lifecycle with restoration after process death,
safe areas and cutouts, back as a command, ongoing activities and widgets
as surfaces — and every one of those is already a portable model in
`rustnative-core` (`permission`, `Lifecycle` and `persistence`,
`environment::keys::SAFE_AREA`, `command::standard::BACK`,
`surfaces::SurfaceCommand`). What remains per backend is the host's API
(Activity versus `UIScene`, `requestPermissions` versus the authorization
APIs, `AccessibilityNodeInfo` versus `UIAccessibility`), so no mobile group
crate is created. The question is revisited when iOS (Milestone 36) lands,
by comparing the two backends' code as the desktop shell's decision did.
