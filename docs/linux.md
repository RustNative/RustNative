# The Linux backend

`PLAN.md` Milestone 34. A Rust Native application runs on Linux desktops
with the same components, events, and state as on Windows. Its nodes become
GTK 4 widgets, its text is Pango's, its accessibility tree is GTK's AT-SPI2
tree (what Orca reads), and the desktop's services are the freedesktop ones
— portals, the notification server, the Secret Service, logind.

| | |
|---|---|
| Crate | `rustnative-linux` (`LinuxPlatform`) |
| Toolkit | GTK 4.14 or newer (Ubuntu 24.04 LTS, Debian 13, Fedora 40, Kali) |
| Display servers | Wayland and X11 (Xwayland included), answered per session |
| Desktops | any; GNOME, Plasma, xfce, and WSLg tested |

## Start

```sh
sudo apt install build-essential libgtk-4-dev libsoup-3.0-dev
rustnative new hello
cd hello
rustnative run linux
```

The generated `main.rs` picks the host's backend, so the same project runs
on Windows and on Linux:

```rust
#[cfg(target_os = "linux")]
use rustnative_linux::{LinuxPlatform as HostPlatform, run_catalogue};
#[cfg(not(target_os = "linux"))]
use rustnative_windows::{WindowsPlatform as HostPlatform, run_catalogue};

HostPlatform::new().with_app_id(APP_ID).run(&mut application)?;
```

`with_app_id` matters more on Linux than anywhere else: the id is the
Wayland app id and X11 window class (which match windows to the desktop
entry, its name and icon), the single-instance bus name, and the folder
state is saved under. Use a reverse-DNS name (`com.example.Notes`).

From Windows, WSL is a Linux host: `wsl rustnative run linux` builds and
runs inside it, and the windows appear on the Windows desktop through WSLg.
`rustnative doctor` says what is missing.

## How the tree is realized

| Node | GTK |
|---|---|
| Column, Row | `RnLayout` (a container the portable layout engine places children in); inside a `GtkScrolledWindow` when it scrolls |
| Label | `GtkLabel` (wrapping, not selectable) |
| Button | `GtkButton` |
| Text input | `GtkEntry` |
| Multi-line text | `GtkTextView` in a `GtkScrolledWindow` |
| Tab bar | a tab-list `RnLayout` of grouped `GtkToggleButton`s, each a tab |
| Check box, radio | `GtkCheckButton` (a radio joins its siblings' group) |
| Toggle | `GtkSwitch` beside its `GtkLabel`, labelled by it |
| Slider | `GtkScale` |
| Progress | `GtkProgressBar` (pulsing while unknown) |
| Select | `GtkDropDown` over a `GtkStringList` |
| List box | `GtkListBox` of `GtkLabel`s |
| Date picker | `GtkMenuButton` showing the date, with a `GtkCalendar` popover |
| Spinner | `GtkSpinButton` |
| Separator | `GtkSeparator` |
| Link | a `GtkButton` in the link style, with the link role |
| Image | `GtkPicture` over a `GdkMemoryTexture` |
| Canvas | `RnCanvas`, drawn with cairo and Pango |
| Media (host content) | `GtkVideo`, where GTK has a media backend |
| A native surface | a Wayland subsurface, or an X11 child window |
| Foreign | the application's own widget (`register_foreign`) |

Layout is the portable engine's on every backend: GTK measures text and
controls (`gtk::measure`), the engine places them, and `RnLayout` puts each
widget at its rectangle. Right-to-left mirroring is the engine's; GTK
mirrors what is drawn inside each widget.

Styles compile to one GTK style sheet per display, applied by class, and
only where a node differs from the desktop's theme: an unstyled application
looks like every other GTK application on that desktop (Adwaita, Breeze,
Yaru). The style capability table is `rustnative_style::LINUX`: every
property realized; a font family is approximated through fontconfig.

GTK's own style language is what makes this host rich: it already paints
borders of any width and style, gradients, and shadows on the widgets it
draws, so most of the extended style range (`PLAN.md` Milestone 67,
planned) is expected to be answered as realized here, through the same
display-wide style sheet. Each property is answered when its family lands,
from what GTK actually paints and read back by `gtk::style_integration`
rather than assumed — see `docs/styling.md`.

## Input

Keyboard, pointer, touch, and pen come from GTK's event controllers;
commands' shortcuts are offered first, in the capture phase, so a shortcut
works from anywhere in the window. Text is GTK's input method
(`GtkIMMulticontext`: IBus, Fcitx, the compositor's text-input), with
composition reported as on Windows. Drag-and-drop is `GtkDropTarget`.

## Accessibility

GTK publishes every widget on the AT-SPI2 bus; the backend gives each the
role, name, description, states, relations, value, and actions the portable
model declares, and the canvas' virtual elements are accessible objects of
their own (`RnVirtual`). Live regions announce through
`gtk_accessible_announce`. The integration tests read the tree back through
AT-SPI2 itself, as Orca does (`gtk::atspi_reader`).

## Services

| Service | Linux |
|---|---|
| `LinuxClipboard` | the display's `GdkClipboard` |
| `LinuxFileDialogs` | `GtkFileDialog`: the FileChooser portal where the desktop has one, GTK's own chooser otherwise |
| `LinuxSystem` | the OpenURI portal or GIO's default handler; notifications through the notification server, or the Notification portal in a sandbox |
| `SoupHttp` | libsoup 3, the system's trust store, the desktop's proxy; certificate pins checked at the TLS handshake, before a byte is sent |
| `LinuxLocale` | glibc's locales (`strfmon`, `strftime`, `strcoll`, `towupper`) |
| `LinuxSecureStorage` | the Secret Service (GNOME Keyring, KWallet, KeePassXC) |
| `LinuxPermissions` | ungated outside a sandbox; the Camera and Location portals inside Flatpak or Snap |
| `LinuxPrinting` | GTK's print operation over CUPS, or a PDF file |
| `LinuxSerial` | termios on `/dev/tty*` |
| `LinuxConditions`, `PixbufDecoder` | GIO's network monitor and UPower; gdk-pixbuf's image loaders |
| `LinuxPush`, `LinuxStore` | unavailable, and say why: Linux desktops have no system push service or in-app billing |
| `FileStateStore` | crash-safe files under `$XDG_STATE_HOME/<app-id>/state` |

## The desktop

- **Menu bars** are `GMenu` models shown by a `GtkPopoverMenuBar` in the
  window. GTK 4 exports only an application-wide menu to a global menu bar,
  so `Capability::GlobalMenuBar` is not advertised.
- **Lifecycle**: logind's sleep and shutdown signals, GIO's memory monitor,
  and an idle flush of persisted state; the primary window reopens at its
  saved size, maximized state, and (X11) position.
- **One instance**: the app id is owned on the session bus with the
  `org.freedesktop.Application` interface; a second launch hands its URL to
  the first, with its activation token so the window may come forward.
- **Tray**: a StatusNotifierItem where a tray host runs (Plasma, GNOME with
  the AppIndicator extension, xfce, waybar); the capability is answered by
  whether one does. **Taskbar progress** is the `LauncherEntry` API (Plasma,
  Ubuntu's dock, Dash to Dock). A **jump list** is the desktop entry's
  actions, fixed at packaging, so it is not a runtime capability.
- **Placement and frames**: X11 honours a requested window position; a
  Wayland compositor places windows itself, so `Capability::WindowPlacement`
  is answered per session, and so is `Capability::ServerSideDecorations`.
  Every per-desktop answer is in [`linux/desktop-matrix.md`](linux/desktop-matrix.md).
- **Scaling**: GTK draws each window at its monitor's scale, fractional
  included; native surfaces are told their new device size when the scale
  changes.

## Customizing a widget

Per-property mappers (`register_mapper`) extend or replace how the backend
applies a node's text, style, accessibility, or visibility to its GTK widget
— for every node of a kind, or one node by key — with the widget in hand.
The inspector lists every active mapper.

## Inspection and the development loop

`rustnative inspect` reads the realized widgets (their GTK types, handles,
and allocations), the lifetime census, the capability answers, and the
mappers, and draws the layout overlay over the window. `rustnative dev
linux` rebuilds and restarts the application on save with its state
restored, and `rustnative preview` opens the preview catalogue
(`run_catalogue`).

## Performance

`budgets/linux.toml` holds the Linux budgets, measured with `rustnative bench
--target linux` on a desktop session (startup, frame times, input latency,
memory, the shared core). Layout measures text and controls through GTK and
remembers each answer until the fonts or theme change, so a change that
moves nothing re-measures nothing.

## Packaging

```sh
rustnative package linux                 # .deb, tarball, and an AppImage if appimagetool is installed
rustnative package linux --format deb
```

Every package carries the desktop entry (`Exec=<name> %u`, the URL schemes
it handles, `StartupWMClass=<app id>`), the AppStream metadata software
centres show, and the icon in the hicolor theme. The archives are
reproducible (`SOURCE_DATE_EPOCH`).

## Embedding

A GTK application adopts Rust Native the way a Win32 one does
(`docs/interop`):

- `LinuxPlatform::start_external` runs the application under the host's
  main loop;
- `LinuxPlatform::embed` returns the primary window as a widget the host
  places in its own tree;
- `register_foreign` adopts the host's widgets as leaves of the tree.

The `adoption-gtk` example does all three.

## Testing

The integration tests run against real GTK on a private D-Bus session
(CI runs them on Xvfb in the `linux-backend` job):

```sh
tools/linux-session.sh wayland cargo test -p rustnative-linux
tools/linux-session.sh x11     cargo test -p rustnative-linux
tools/linux-session.sh xvfb    cargo test -p rustnative-linux   # XTest input injection
```

The shared guarantee suites (`rustnative-conformance`) run over the GTK
harness as they do over the Windows one.
