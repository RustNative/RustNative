# The Linux backend

`PLAN.md` Milestone 34. A Rust Native application runs on Linux desktops
with the same components, events, and state as on Windows. Its nodes become
GTK 4 widgets, its text is Pango's, its accessibility tree is GTK's AT-SPI2
tree (what Orca reads), and the desktop's services are the freedesktop ones
— portals, the notification server, the Secret Service, logind.

| | |
|---|---|
| Crate | `rustnative-linux` (`LinuxPlatform`) |
| Toolkit | GTK 4.14 or newer (Ubuntu 24.04 LTS, Debian 13, Kali, Fedora 40+, Arch Linux) |
| Distributions | the Debian, Fedora, and Arch families tested (Ubuntu, Kali, Fedora, Arch); openSUSE expected |
| Display servers | Wayland and X11 (Xwayland included), answered per session |
| Desktops | any; GNOME, Plasma, xfce, and WSLg tested |

## Start

Install a C compiler, `pkg-config`, and GTK 4's and libsoup 3's
development files with the distribution's package manager:

```sh
sudo apt install build-essential pkg-config libgtk-4-dev libsoup-3.0-dev   # Debian, Ubuntu, Kali, Mint
sudo dnf install gcc pkgconf-pkg-config gtk4-devel libsoup3-devel          # Fedora, RHEL, CentOS Stream
sudo pacman -S --needed base-devel gtk4 libsoup3                           # Arch, Manjaro, EndeavourOS
sudo zypper install gcc pkgconf gtk4-devel libsoup-devel                   # openSUSE (untested)
```

`rustnative doctor` reads `/etc/os-release` and names whatever is missing
in the distribution's own terms. Then:

```sh
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
rustnative package linux                  # every format below; the AppImage if appimagetool is installed
rustnative package linux --format deb     # Debian, Ubuntu, Kali, Mint:     sudo apt install ./notes_1.0.0_amd64.deb
rustnative package linux --format rpm     # Fedora, RHEL, openSUSE:         sudo dnf install ./notes-1.0.0-1.x86_64.rpm
rustnative package linux --format pacman  # Arch, Manjaro, EndeavourOS:     sudo pacman -U notes-1.0.0-1-x86_64.pkg.tar
rustnative package linux --format tar     # anywhere: unpack and run
```

Each distribution's package names its dependencies in that distribution's
terms (`libgtk-4-1 (>= 4.14)`, `gtk4 >= 4.14`, `gtk4>=4.14`), so its own
package manager installs them. All are written by `rustnative` itself —
no `dpkg-deb`, `rpmbuild`, or `makepkg` needed — so any one Linux host
builds every distribution's package. The packages are unsigned: sign an `.rpm` with `rpmsign` and a
pacman package with `gpg --detach-sign`, as a repository requires.

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
(CI runs them on Xvfb on Ubuntu, Fedora, and Arch Linux):

```sh
tools/linux-session.sh wayland cargo test -p rustnative-linux
tools/linux-session.sh x11     cargo test -p rustnative-linux
tools/linux-session.sh xvfb    cargo test -p rustnative-linux   # XTest input injection
```

The shared guarantee suites (`rustnative-conformance`) run over the GTK
harness as they do over the Windows one.

Besides GTK and libsoup, the tests need the session's pieces: an X server
for Xvfb runs, `xdotool`, the D-Bus daemon and `dbus-run-session`, the
AT-SPI2 bus, `gnome-keyring`, fonts, and the `en_NZ`, `de_DE`, `fr_FR`,
and `tr_TR` UTF-8 locales the locale tests read back:

| Family | Packages | Locales |
|---|---|---|
| Debian | `xvfb xdotool dbus dbus-x11 at-spi2-core gnome-keyring locales` | `sudo locale-gen en_NZ.UTF-8 de_DE.UTF-8 fr_FR.UTF-8 tr_TR.UTF-8` |
| Fedora | `xorg-x11-server-Xvfb xdotool dbus-daemon dbus-tools at-spi2-core gnome-keyring mesa-dri-drivers dejavu-sans-fonts` | `glibc-langpack-{en,de,fr,tr}` |
| Arch | `xorg-server-xvfb xdotool dbus at-spi2-core gnome-keyring mesa ttf-dejavu` | uncomment them in `/etc/locale.gen`, then `sudo locale-gen` |

Under WSL, GTK 4.16 and newer (Fedora, Arch, Kali) first try a Vulkan
renderer and print `Vulkan: … Failed to enumerate drm devices` once per
process before falling back to OpenGL. It is harmless; `GSK_RENDERER=ngl`
skips the attempt.
