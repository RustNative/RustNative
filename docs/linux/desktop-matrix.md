# Linux desktops: the conformance matrix

`PLAN.md` Milestone 34. Linux desktops differ in conventions an application
cannot assume: who draws a window's frame, whether a tray exists, which
portal answers a file dialog, where settings come from. The Linux backend
answers each of these **per session, at run time** — from the display
server and the desktop the session reports (`Session::detect`:
`XDG_SESSION_TYPE`, `WAYLAND_DISPLAY`/`DISPLAY`, `XDG_CURRENT_DESKTOP`,
`GDK_BACKEND`), and from what is on the session bus — never from a table
compiled in. This page records what those answers are on each desktop, and
which were verified on a running session.

## What is answered, and how

| Convention | How the backend finds out | Capability or behaviour |
|---|---|---|
| Window placement | the display server: X11 honours a requested position, Wayland places windows itself | `Capability::WindowPlacement` on X11 only; a restored window keeps its size and maximized state everywhere, its position on X11 |
| Who draws the frame | GTK's rule: the window manager on X11; on Wayland GTK draws its own unless the compositor offers KWin's server-decoration protocol | `Capability::ServerSideDecorations`: X11, or Plasma on Wayland |
| A global menu bar | GTK 4 exports only an application-wide menu; Rust Native menus are per window | `Capability::GlobalMenuBar` never; menus are in the window |
| Tray | a `StatusNotifierWatcher` on the session bus | `Capability::Surface(TrayExtra)` where one runs |
| Taskbar progress | the `LauncherEntry` signal, which a dock that supports it shows | `Capability::Surface(TaskbarProgress)`; shown where the dock reads it |
| Settings (scheme, contrast, motion, text scale, fonts) | the Settings portal where it answers, `GtkSettings` otherwise | followed live in both cases |
| Dialogs, URLs, notifications | the portals where they run (and always inside a sandbox); GTK's own chooser, GIO's handlers, and the notification server otherwise | the same services, two routes |
| Secrets | the Secret Service owner on the bus | `LinuxSecureStorage` (GNOME Keyring, KWallet, KeePassXC) |
| Permissions | a sandbox (Flatpak, Snap) is detected; outside one nothing is gated | `LinuxPermissions` |

## Per desktop

The cells are what each desktop offers as shipped by its usual distribution.
**Verified** names the sessions the backend's tests ran in.

| Desktop | Usual display server | Frame | Tray host | Progress shown | Portal backend | Verified |
|---|---|---|---|---|---|---|
| GNOME | Wayland | GTK draws (client-side) | only with the AppIndicator extension (Ubuntu ships it on) | Ubuntu's dock, Dash to Dock | `xdg-desktop-portal-gnome` | — |
| KDE Plasma | Wayland | the compositor (server-side) | yes | the task manager | `xdg-desktop-portal-kde` | — |
| Xfce (Kali's default) | X11 | the window manager | yes (the panel's StatusNotifier plugin) | no | `xdg-desktop-portal-gtk` | — |
| Cinnamon | X11 | the window manager | yes | no | `xdg-desktop-portal-xapp` | — |
| MATE | X11 | the window manager | with the indicator applet | no | `xdg-desktop-portal-gtk` | — |
| LXQt | X11 or Wayland | the window manager; GTK draws on Wayland | yes | no | `xdg-desktop-portal-lxqt` | — |
| Budgie | X11 | the window manager | yes | no | `xdg-desktop-portal-gtk` | — |
| Sway, Hyprland | Wayland | GTK draws | with a bar that hosts one (waybar) | no | `xdg-desktop-portal-wlr` / `-hyprland` | — |
| WSLg | Wayland, with Xwayland | GTK draws (Wayland); Weston's X window manager (X11) | none | no | none: GTK's own dialogs, GIO's handlers | Wayland and X11 sessions: every test, on Ubuntu 24.04 (GTK 4.14) and Kali Rolling (GTK 4.22) |
| No desktop (Xvfb, CI) | X11 | none | none | no | none | every test (with XTest input injection), on Ubuntu 24.04 and Kali Rolling |

A desktop not in the table is still answered: every row's question is asked
of the session itself. `DesktopEnvironment::Other` carries the name the
session gave, for diagnostics and the inspector.

## Scaling

GTK lays out in logical pixels and draws each window at its monitor's scale
— fractional scales included (GTK 4.12+) — so a window moved to a monitor of
another scale is redrawn sharp with no work from the application. The one
thing GTK cannot redraw for itself is a native surface an application renders
into (Milestone 29): the backend reports its new device-pixel size and scale
as `Event::SurfaceResized` when the window's scale changes, whether or not its
size did (`gtk::surface_integration`, which also runs at `GDK_SCALE=2`).
