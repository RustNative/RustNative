# Milestone 34 Implementation Plan (the Linux backend)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan phase-by-phase. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Linux backend — `PLAN.md` "Milestone 34 — Linux backend" — to the definition of a complete backend in `PLAN.md` §8, including the §11 gates that exist today, and every Linux half another milestone recorded as owed.

**Architecture:** One new backend crate, `rustnative-linux`. Its toolkit-independent half owns what every Linux desktop shares whatever toolkit draws the widgets: the display-server and desktop-environment model, the XDG base directories, the freedesktop D-Bus services (portals, notifications, StatusNotifierItem, logind, Secret Service), glibc locale facilities, and termios. Its toolkit half is GTK 4, behind an internal `Toolkit` seam (`toolkit::Toolkit`) so a second native toolkit is a second module, not a change to the core or to the shared half. GTK 4 realizes the tree as real widgets (`GtkLabel`, `GtkButton`, `GtkEntry`, `GtkCheckButton`, `GtkScale`, …) inside a framework container widget (`RnLayout`) that places children exactly where the portable layout engine says; Pango measures; GDK delivers input; GTK's AT-SPI2 backend carries the portable accessibility model; `GdkFrameClock` paces animation; portals serve dialogs, URIs, notifications, and settings. The D-Bus plumbing uses GIO's GDBus (already present through GTK) rather than a second D-Bus stack.

**Tech Stack:** Rust 2024 (MSRV 1.85); `gtk4` / `gdk4` / `gio` / `glib` / `pango` / `cairo-rs` from gtk-rs (MIT), resolved to versions whose MSRV is ≤ 1.85 and whose C API floor is GTK 4.14 (Ubuntu 24.04 LTS); `gdk4-wayland` / `gdk4-x11` for display-server specifics; `libc` for glibc locale, termios, and evdev; `raw-window-handle` 0.6 for native surfaces. Verified in WSL 2 on Ubuntu 24.04 (GTK 4.14) and Kali rolling (a newer GTK), under WSLg's Wayland compositor and its Xwayland server, and under a headless Weston and Xvfb for reproducible runs.

**Spec:** `PLAN.md` §2 (2.2–2.5, 2.9, 2.13, 2.14), §8 (the backend definition and Milestone 34), §11 (Milestones 39–45 and 58 gates); `docs/conformance/new-backend-checklist.md`; `docs/conformance/platform-groups.md`; the owed lines of `BUILD_STATUS.md` naming Milestone 34, "the deferred backends", or "the other backends".

## Scope decision (2026-09-29)

The user's instruction: *create and complete the Linux backend through WSL (Ubuntu and Kali are installed); make sure it is fully complete; plan it thoroughly; don't stop until done.* Therefore:

- **In scope:** every Milestone 34 bullet; every item of §8's backend definition; the §11 gates that exist on the shipped backends — the shared conformance suites and the host-specific guarantees (Milestone 41), budgets (42), the developer loop (43), inspection (44), embedding both ways (40), the portable-surface obligations and the new-backend checklist column (39), the style capability table and unit mapping (58); the Linux halves recorded as owed by Milestones 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 51, 57, and 58; the Desktop-shell platform-group decision (`platform-groups.md`), which Linux, as the group's second member, forces.
- **Out of scope, and why:** Milestones 59 (toolchain and service layer) and 61 (interaction conformance) are named by §8 as gates but are not built for any backend; a backend cannot pass a suite that does not exist. They remain owed by every backend. Embedded Linux (Milestone 37) is a different profile and stays with 37. Desktop auto-update (Milestone 50's updater) is Windows-only today and its Linux half is not recorded as owed; it is noted, not built.
- **Verification bar (2.13):** everything runs in WSL 2 on this machine. WSLg provides a real Wayland compositor (Weston-based) and Xwayland, which is both display servers; a headless Weston and Xvfb make the same tests reproducible without WSLg. Kali rolling is the second distribution. What WSL cannot provide — a physical touch screen, a pen, a gamepad, a second physical monitor, GNOME Shell or KDE Plasma sessions, a printer, a serial device — is implemented, capability-answered from the running system, tested at the translation layer with recorded input, and recorded as unverified on hardware. A recorded Orca screen-reader pass is owed to a person, as Narrator's is.

## Global Constraints

- Everything in the 39–58 plan's Global Constraints still holds: the core stays platform-free (no `cfg(target_os)` item in `rustnative-core`); MSRV 1.85; `#![deny(missing_docs)]`; clippy pedantic clean with every `#[allow]` reasoned and every `unsafe` block carrying `SAFETY:`; `cargo deny check` clean; capabilities advertised only once realized; transient interaction never renders; both syntaxes and both style spellings for anything new.
- The workspace keeps building and testing on Windows exactly as before. `rustnative-linux`'s GTK dependencies are `[target.'cfg(target_os = "linux")'.dependencies]`; on any other OS the crate compiles to its portable half and `LinuxPlatform::run` returns `Error::UnsupportedHost`, mirroring `WindowsPlatform` on Linux.
- The whole workspace also builds, lints, documents, and tests on Linux (`gate-linux.sh`): it is the only way the Linux code is ever compiled.
- GTK is touched from exactly one thread. The backend asserts it (`affinity::UiThread`), and tests run GTK work on one dedicated thread through a test executor (`gtk::testing::on_gtk`), because GTK allows one main thread per process.
- No GTK signal handler borrows backend state that a render can be holding: handlers enqueue, and the queue drains when no render is in progress (the GTK form of the Windows backend's "post, never re-enter" rule).
- Capability answers that differ by display server or desktop environment are computed from the running session, never from the build.

## Verification gate

The workspace gate (`rustnative-tools/gate.sh`, on Windows) plus `rustnative-tools/gate-linux.sh`, run in WSL:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --exclude rustnative-linux           # the portable workspace, on Linux
tools/linux-session.sh wayland cargo test -p rustnative-linux   # WSLg, or a headless Weston
tools/linux-session.sh x11 cargo test -p rustnative-linux       # Xwayland, or Xvfb
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo +1.85 check --workspace --all-targets
cargo deny check
```

`linux-session.sh` starts a private D-Bus session with the AT-SPI2 bus, picks the display server (`GDK_BACKEND`), and falls back to a headless Weston (`--backend=headless`) or Xvfb when WSLg is absent. The builds use a Linux-native target directory (`~/.cache/rustnative-target`), never the Windows one.

## Crates

| Crate | Kind | Change |
|---|---|---|
| `rustnative-linux` | library (backend) | new |
| `rustnative-desktop-shell` | library (group) | new: the Desktop-shell group crate, forced by Linux being the group's second member |
| `rustnative-core` | library | three capability variants (`WindowPlacement`, `ServerSideDecorations`, `GlobalMenuBar`); no platform code |
| `rustnative-style` | library | `LINUX`, `LINUX_UNITS` |
| `rustnative-macros` | proc-macro | the unavailable-property check selects its table by target OS |
| `rustnative-build` | library | Linux resources: desktop entry, icon theme layout, AppStream metainfo |
| `rustnative` (CLI) | binary | `build`/`run`/`package linux`, Linux `doctor` rows, `new` template per target |
| `rustnative-windows` | library | answers the three new capabilities; consumes the Desktop-shell crate |
| `rustnative-conformance` | library | a Linux compile-fail case; nothing Linux-specific otherwise |

---

## Phase 0 — Linux environment, workspace portability, and the portable widenings

**Design.**
1. **Toolchain in WSL.** `build-essential`, `pkg-config`, `libgtk-4-dev` and its stack, `at-spi2-core`, `dbus`, `weston`, `xvfb`, `xdotool`, `python3-gi` with `gir1.2-atspi-2.0` (a second, independent AT-SPI reader for cross-checks), `gnome-keyring`, `libsecret-tools`, `desktop-file-utils`, `appstream`, `dpkg-dev`; rustup with stable, 1.85, clippy, rustfmt; `cargo-deny`. The same in Kali.
2. **The workspace on Linux.** `cargo build/test/clippy --workspace` on Linux, fixing whatever assumes Windows outside `cfg(windows)`.
3. **Capabilities.** `Capability::WindowPlacement` (a window can be put at coordinates the application chooses), `ServerSideDecorations` (the host draws the title bar and frame), `GlobalMenuBar` (a menu bar outside the window, owned by the desktop). Windows: yes, yes, no. Headless: none. Web: none. Added to `Capability::ALL`, `rustnative describe`, `docs/api/framework.json`, and every backend's capability test.
4. **Style.** `rustnative_style::LINUX` answers every property — GTK's CSS engine draws colours, borders, radii, shadows, opacity, and fonts on real widgets — with `FontFamily` approximated as on Windows (first family; generic families map through fontconfig). `LINUX_UNITS`: one logical pixel is one GTK application pixel, scaled to device pixels by the surface's (possibly fractional) scale; `rem` is 16 px × the desktop's text-scaling factor; rounding half away from zero. The macro's unavailable-property check emits one `compile_error!` per target table under `cfg(target_os = …)`.

**Files:** `crates/rustnative-core/src/capability.rs`, `crates/rustnative/src/describe.rs`, `docs/api/framework.json`, `crates/rustnative-windows/src/platform.rs`, `crates/rustnative-headless/src/platform.rs`, `crates/rustnative-web/…capabilities`, `crates/rustnative-style/src/capability.rs`, `crates/rustnative-macros/src/lib.rs`, `rustnative-tools/gate-linux.sh`, `tools/linux-session.sh`.

**Acceptance tests:** the workspace gate passes on Windows and on Linux; each backend's capability test names the three new variants; `every_shipped_table_answers_every_property` covers `LINUX`.

---

## Phase 1 — The crate, the toolkit seam, and the realized tree

**Design.**
- **`LinuxPlatform`** (`platform.rs`): `new()`, `with_app_id(id)` (single instance through GApplication's D-Bus name), `with_toolkit(ToolkitKind)` (only `Gtk4` today), `run`, `capabilities` (computed from the session: display server, desktop, portals, tray watcher, input devices), `style_capabilities` → `LINUX`, `unit_mapping` → `LINUX_UNITS`, `native_extension`.
- **The seam** (`toolkit/mod.rs`): `trait Toolkit { type Error; fn run(&mut self, application: &mut Application, session: &Session) -> Result<(), Self::Error>; fn capabilities(&self, session: &Session) -> Vec<Capability>; fn measurer(&self) -> Box<dyn IntrinsicMeasurer>; }`. The desktop half (`desktop/`) never names GTK; the GTK half (`gtk/`) never re-implements what the desktop half answers.
- **`Session`** (`desktop/session.rs`): the display server (`WAYLAND_DISPLAY`, `DISPLAY`, `GDK_BACKEND`, then what GDK actually opened), the desktop environment (`XDG_CURRENT_DESKTOP`, `XDG_SESSION_DESKTOP`, `DESKTOP_SESSION`; WSLg recognized), and the portal/tray/notification services present on the bus.
- **Runtime** (`gtk/runtime.rs`): one `Runtime` per window, a `WindowRegistry` syncing `Application::window_ids()` to `GtkWindow`s; modal windows (`set_modal`, `set_transient_for`) disable their parent; closing through `close-request`. The `Application` borrow is a `HostRef` valid for `run`'s frame, as on Windows. The scheduler's waker is a thread-safe `glib::MainContext::invoke` that pumps that window's tasks on the GTK thread. Deferred work runs from an idle source at low priority, so input comes first (Milestone 54).
- **Event queue** (`gtk/queue.rs`): signal handlers push `Event`s; the queue drains immediately when no render is in progress and otherwise at the end of the render — no handler ever re-enters a borrowed runtime.
- **Renderer** (`gtk/rendering/realization.rs`): snapshot → diff → apply → relayout, the same phases as Windows. Host objects: `RnLayout` (a `GtkWidget` subclass that allocates each child at the rectangle the layout engine gave it, and clips) for Column/Row/Grid; a `GtkScrolledWindow` around an `RnLayout` for `Overflow::Scroll`; `GtkLabel` (wrapping, selectable off), `GtkButton`, `GtkEntry` / `GtkPasswordEntry`, `GtkNotebook`-free tab bars (`GtkStackSwitcher`-like row of `GtkToggleButton`s with the tab role). A node's accessible role is a construct-only property in GTK 4, so a role change replaces the widget (`needs_replacement`). Virtual-list recycling parks widgets per parent and kind, as `rendering::pool` does on Windows.
- **Measurement** (`gtk/measure.rs`): `GtkIntrinsicMeasurer` measures through prototype widgets of each kind (`gtk_widget_measure`), so the theme's own padding and Pango's own shaping produce the number; labels wrap through the prototype's width-for-height. Styled typography measures with the node's resolved font description.
- **Harness** (`gtk/testing.rs`, test-only): `on_gtk(|| …)` runs a closure on the process's one GTK thread and returns its value (panics included); `NativeHarness` realizes an `Application` without running a blocking loop, pumps the main context, and exposes widgets by key — the Linux counterpart of `native::harness`.

**Files:** create `crates/rustnative-linux/{Cargo.toml, src/lib.rs, src/error.rs, src/platform.rs, src/toolkit/mod.rs, src/desktop/{mod.rs, session.rs}, src/gtk/{mod.rs, app.rs, context.rs, runtime.rs, queue.rs, registry.rs, measure.rs, layout_widget.rs, testing.rs, rendering/{mod.rs, realization.rs, controls.rs, pool.rs, scrolling.rs}}}`.

**Acceptance tests (on Wayland and X11):** a window of every basic kind realizes the widget type the mapping names, at the rectangle the layout engine computed (read back from GTK's allocation); a click on a `GtkButton` reaches `Component::update`; typing into a `GtkEntry` produces one `TextChanged` per change and no echo on the framework's own `set_text`; a scroll container scrolls without rendering; `Application::open_window` and close work, a modal disables its parent; the scheduler wakes the loop from another thread; the shared guarantee suites (`rustnative_conformance::suites::all`) pass on `LinuxHost`; label measurement grows with text and wraps within a width.

---

## Phase 2 — Controls, styling, direction, and host traits

**Design.**
- **Controls** (`gtk/rendering/native_controls.rs`): Checkbox → `GtkCheckButton`; Radio → `GtkCheckButton` grouped by parent; Toggle → `GtkSwitch` with its label (labelled-by); Slider → `GtkScale`; Progress → `GtkProgressBar` (pulsing while unknown); Select → `GtkDropDown` over a `GtkStringList`; ListBox → `GtkListBox` of labels; DatePicker → `GtkMenuButton` showing the date with a `GtkCalendar` popover; Spinner → `GtkSpinButton`; Separator → `GtkSeparator`; Link → `GtkLinkButton`-styled `GtkButton` with the link role (activation is the application's, not the URI launcher's); MultilineText → `GtkTextView` in a `GtkScrolledWindow`; Image → `GtkPicture` over a `GdkMemoryTexture`. Each raises the portable event its Windows counterpart raises (`Toggled`, `ValueChanged`, `SelectionChanged`, `DateChanged`, `TextChanged`), and each echo of a framework-made change is suppressed.
- **Styling** (`gtk/rendering/styling.rs`): one display-wide `GtkCssProvider` holds a rule per distinct resolved style, named `rn-s<fnv64>`; a node carries its class. Colours, border, radius, shadow, font (family, size, weight), and state variants (`:hover`, `:active`, `:focus-visible`, `:disabled`) are CSS; opacity is `gtk_widget_set_opacity`; hidden is `set_visible(false)`; disabled is `set_sensitive(false)`. The theme's per-kind defaults are rules on `.rn-label` etc. The component library (Milestone 48) renders through the same path.
- **Host traits** (`gtk/host_traits.rs`, `desktop/settings.rs`): colour scheme, contrast, reduced motion, and text scale from the Settings portal (`org.freedesktop.appearance` `color-scheme`, `contrast`, `reduced-motion`; `org.gnome.desktop.interface` `text-scaling-factor`), falling back to `GtkSettings` (`gtk-application-prefer-dark-theme`, `gtk-enable-animations`, `gtk-xft-dpi`) where no portal answers; locale and direction from the process locale; changes arrive through the portal's `SettingChanged` and `GtkSettings` notifications and restyle existing widgets without re-rendering.
- **Direction:** the layout engine mirrors (as for the headless backend) and `gtk_widget_set_default_direction`/per-widget direction mirror what GTK draws inside a widget (text alignment, check-box placement); a runtime switch reuses the same widgets.

**Acceptance tests:** every control kind realizes its widget, reports its event, and suppresses its echo; the `LINUX` table is what the backend applies (colours, font, border, radius, shadow, opacity read back through GTK's computed style or rendered pixels); a scheme switch restyles the same widgets with no application render; a right-to-left locale mirrors a row and flips the widgets' direction with the same objects; `docs/components.md`'s library renders on Linux.

---

## Phase 3 — Input

**Design.** (`gtk/input/*`)
- **Keys:** `GtkEventControllerKey` on each window; XKB keysyms (via `gdk::Key`) mapped to `KeyCode`, modifiers from `ModifierType`; key-down, key-up, character input; command shortcuts through a `GtkShortcutController` in the managed scope, routed by focus as the command model requires.
- **Pointer:** `GtkGestureClick` (press/release, count, button), `GtkEventControllerMotion` (move, enter/leave → hover), `GtkEventControllerScroll` (discrete and smooth deltas, kinetic), per-node cursors (`gdk::Cursor::from_name` with the CSS cursor names). Pointer capture: an implicit grab is GTK's; explicit capture holds the target for the drag.
- **Touch and pen:** touch sequences through `GtkGesture` controllers as `PointerKind::Touch` with per-sequence ids; `GtkGestureStylus` for pen pressure, tilt, and eraser. Gesture arbitration: scroll-vs-pan in scroll containers under `DeferToHost` yields to `GtkScrolledWindow`'s own kinetic scrolling.
- **IME:** native text widgets use GTK's own input method; custom text targets (a canvas that requested text input) get a `GtkIMMulticontext` delivering preedit (`Composition`) and commit events with a cursor rectangle.
- **Clipboard:** `GdkClipboard` (text and `ClipboardAction`s); the primary selection is exposed as a Linux extension.
- **Drag and drop:** `GtkDropTarget` for files (`GdkFileList`), text, and URIs, with enter/over/leave/drop and effects; `GtkDragSource` when a component starts a drag.
- **Gamepad:** Linux evdev (`/dev/input/event*` with `EV_ABS`/`EV_KEY` capabilities of a gamepad, read with `libc`) behind the portable `GamepadSource`; the capability is advertised only when a readable gamepad device exists.

**Acceptance tests:** a key map table test; under X11, real keystrokes and clicks injected with XTest (`xdotool`) arrive as the portable events; under Wayland, the same through GTK's own event delivery (synthesized with `gtk_widget_activate`/controller emission where the compositor offers no injection), recorded as such; hover changes style without rendering; a declared cursor is the window's cursor; wheel scrolling moves a scroll container; the clipboard round-trips; a dropped file list reaches the component (a drop synthesized through `GtkDropTarget`'s `drop` signal); gesture arbitration table holds; evdev parsing of recorded gamepad reports.

---

## Phase 4 — Accessibility through AT-SPI2

**Design.** (`gtk/accessibility/*`)
- Roles: the portable `AccessibilityRole` → `gtk::AccessibleRole` table, set at construction (`accessible-role`). Names, descriptions, values (`ValueNow/Min/Max/Text`), states (checked, expanded, selected, disabled, busy, required, read-only, hidden), relations (labelled-by, described-by, controls), position in set, heading levels, and automation ids (as the `accessible-id` platform attribute where GTK exposes one, otherwise the widget name) through `update_property`/`update_state`/`update_relation`, driven by the portable `AccessibilityTree` exactly as the Windows bridge is.
- Virtual elements: GTK ≥ 4.10 lets an object implement `GtkAccessible`; `RnVirtualAccessible` objects become the accessible children of their host widget, with bounds, role, name, and state.
- Actions: AT-SPI actions GTK exposes on native widgets (activate, toggle, value set) raise the portable events; `Event::AccessibilityAction` is raised for the actions GTK routes to the framework; what GTK 4.14 cannot express (custom actions on virtual elements) is documented in `docs/linux.md` and answered, not faked.
- Live regions: `gtk_accessible_announce` (4.14) for polite and assertive regions.
- Verification: a small AT-SPI2 client over GDBus (`gtk/testing/atspi.rs`) walks the application's tree on the accessibility bus — role, name, states, children — and compares it with `AccessibilityTree`; `python3-gi`'s Atspi is the independent cross-check.

**Acceptance tests:** every role maps; a form's AT-SPI tree carries the roles, names, relations, and states the portable model computes; a virtual element appears as a child with its bounds; toggling a check box through AT-SPI's action raises `Toggled`; setting a slider's value through AT-SPI's Value interface raises `ValueChanged`; a live region's text change is announced; removing a node removes its accessible.

---

## Phase 5 — Animation, virtual lists, canvas, native surfaces, host content

**Design.**
- **Animation:** a tick callback on the window (`gtk_widget_add_tick_callback`) driven by `GdkFrameClock` evaluates the portable `Timeline` and applies frames (position, size, translation through the layout widget's child transform, opacity, colours through the style rule); reduced motion from the settings.
- **Virtual lists:** the Windows logic, shared through the renderer: extents, anchors, ranges, recycling.
- **Canvas:** an `RnCanvas` widget draws a `DrawList` through cairo (paths, fills, strokes, gradients, images, clips, transforms) and Pango (text), in the widget's `snapshot`; pixel tests render offscreen to a `cairo::ImageSurface` and read it back.
- **Native surfaces:** under X11, a child X window of the toplevel positioned at the node's rectangle (`RawWindowHandle::Xlib`); under Wayland, a `wl_subsurface` of the toplevel's `wl_surface` positioned at the node's rectangle (`RawWindowHandle::Wayland`), synchronized with the parent. Scale factor reported with `SurfaceResized`. Tests draw into the surface with the protocol's own primitives (an X fill and `XGetImage`; a shared-memory buffer and a committed frame) to prove the handle is live and placed.
- **Host content (Milestone 48):** media playback through `GtkVideo`/`GtkMediaFile` (GStreamer where installed), answered from `gtk::MediaFile`'s availability; web content is not offered (no WebKitGTK dependency); camera answered from the camera portal's availability and not offered without it.

**Acceptance tests:** a transition runs across frames and releases the property; reduced motion jumps to the end; a 100 000-item virtual list realizes only its window and recycles widgets; canvas pixel goldens for fills, strokes, text, and clipping; a native surface's handle is valid, placed, resized, and destroyed with its node on both display servers; media playback capability matches the GStreamer install.

---

## Phase 6 — Desktop services, menus, windows, and the desktop matrix

**Design.**
- **Portals and D-Bus services** (`desktop/*`, GDBus): file dialogs through `GtkFileDialog` (which uses the FileChooser portal when present and GTK's own dialog otherwise); URL launching through `GtkUriLauncher` (OpenURI portal); notifications through `GNotification` (portal, `org.gtk.Notifications`, or `org.freedesktop.Notifications`); the Settings portal (Phase 2); the tray through StatusNotifierItem + `com.canonical.dbusmenu`, advertised only when a `org.kde.StatusNotifierWatcher` is on the bus; logind `PrepareForSleep` for suspend/resume, GApplication's `query-end` for session end, `GMemoryMonitor` for low memory; `GNetworkMonitor` and `GPowerProfileMonitor` for data-layer conditions; Secret Service (`org.freedesktop.secrets`, plain session) for secure storage, answered from the bus.
- **Menus:** `GMenuModel` from the portable `MenuBar`, shown as a `GtkPopoverMenuBar` inside the window (no global menu bar: `GlobalMenuBar` is not claimed unless `com.canonical.AppMenu.Registrar` is present and the menu is exported); items are `GAction`s; accelerators through `set_accels_for_action`; context menus through `GtkPopoverMenu`.
- **Windows:** placement answered by display server — Wayland refuses client positioning, so `WindowPlacement` is not claimed there and a requested position is recorded but not applied; X11 honours it. Decorations: `ServerSideDecorations` claimed where the compositor negotiates them (X11 window managers; Wayland with `xdg-decoration`), GTK's client-side decorations otherwise. Minimize/maximize/fullscreen through `GtkWindow`. Window placement persisted through the state store (size and state everywhere; position on X11).
- **State and identity:** `FileStateStore` under `$XDG_STATE_HOME/<app-id>`; single instance and deep links through GApplication (`HANDLES_OPEN`, `open` signal); the launch URL is delivered as `Event::DeepLink`.
- **Locale (Milestone 46):** glibc: `newlocale`/`strcoll_l` collation, `localeconv`/`nl_langinfo_l` number and date formats, direction from the language.
- **HTTP:** the portable `HttpService` over libsoup 3 (the GNOME stack's own HTTP, GnuTLS with the system trust store), with certificate pins checked against the peer certificate's SPKI digest.
- **Printing and serial (Milestone 51):** `GtkPrintOperation` for printing (answered from the print backends present); serial ports through termios on `/dev/ttyS*`, `/dev/ttyUSB*`, `/dev/ttyACM*`.
- **Mixed DPI:** the window follows `notify::scale-factor` (and GDK 4.12's fractional scale); surfaces report the new factor; a monitor configuration change re-reads the monitors (`GdkDisplay::monitors` `items-changed`).
- **Desktop-environment conformance matrix** (`desktop/conventions.rs`, `docs/linux/desktop-matrix.md`): GNOME, KDE Plasma, Xfce, Cinnamon, MATE, LXQt, Budgie, wlroots compositors (Sway, Hyprland), and WSLg — per environment: decorations, global menu, tray protocol, portal backend, notification server, button layout, and placement — answered from the session (live D-Bus names, GtkSettings) with the table as the documented expectation.
- **Desktop-shell group crate:** `rustnative-desktop-shell` holds what Windows and Linux genuinely share for tray extras, jump lists (Linux: desktop-entry actions), and taskbar progress (Linux: `com.canonical.Unity.LauncherEntry`), as trait contracts and the menu/diff model; Windows is moved onto it; the decision is recorded in `platform-groups.md`.

**Acceptance tests:** against fake services registered on the test's private bus — a FileChooser portal returning a chosen path; OpenURI receiving the launched URI; a notification server receiving title and body; a StatusNotifierWatcher receiving the tray item and its menu; a Secret Service storing and reading a secret (or the real gnome-keyring in the session); a Settings portal changing the colour scheme live; logind's `PrepareForSleep` producing suspend and resume. A menu bar's item runs its command and its accelerator works. Window placement is refused on Wayland and applied on X11. A second launch hands its URL to the first instance. glibc collation orders `ä` per locale. HTTP GET against a local server. Termios configures a pseudo-terminal pair. Scale-factor changes are reported. The matrix answers for WSLg match what the session shows.

---

## Phase 7 — The §11 gates: inspection, embedding, developer loop, conformance, budgets

**Design.**
- **Inspection (Milestone 44):** `GtkInspect` implements `InspectBackend` for every window (realized widgets with their GType names and addresses, rects, lifetimes, capabilities, style table, unit mapping); the overlay draws with an `RnOverlay` layer; `RUSTNATIVE_INSPECT=1` attaches as on Windows.
- **Embedding (Milestone 40):** `LinuxPlatform::embed(parent: &gtk::Widget …)` realizes the primary window's tree as an `RnLayout` the host places in its own widget tree, driven by the host's main loop (`EmbeddedRoot`); `start_external` for guest-runtime mode; `register_foreign(kind, factory)` adopts a host `GtkWidget` as a leaf, laid out and clipped by our layout (`ForeignWidget`, `Ownership::{Owned, Borrowed}`).
- **Developer loop (Milestone 43):** `run_catalogue` (previews), the dev agent's reload-with-state and live style pushes on Linux, `rustnative dev` driving a Linux build.
- **Conformance (Milestone 41):** the shared suites (Phase 1) plus Linux-specific guarantees: host-object lifetime (live `GtkWidget` count through a weak-reference census returns to baseline after churn); fidelity (an unstyled button, entry, and check box are GTK's own — same CSS nodes and style classes as a plain GTK widget, same measured size); text through Pango (complex scripts: Arabic shaping, Devanagari clusters, bidirectional ordering, fallback fonts, line breaking, caret geometry via `pango_layout_index_to_pos`); layout under text scaling and pseudo-localization; modal operation (a modal window blocks its parent's input; a popover menu's own loop); panic and teardown (a deliberate panic in a handler restores the cursor and grabs and reports through the panic policy); the new-backend checklist's Linux column, every row with a test or an honest answer.
- **Budgets (Milestone 42):** `budgets/linux.toml` measured by `rustnative bench --target linux` from `examples/bench-app` (launch, input latency, filter latency, render/tree-diff/layout microbenchmarks, idle memory) in release, with tolerances calibrated on this machine.

**Acceptance tests:** inspection answers `realized`/`rects`/`lifetimes` for two windows; an embedded root inside a host `GtkBox` renders and dispatches; a foreign `GtkColorDialogButton` is laid out and clipped; the preview catalogue runs; lifetime census returns to baseline; the text suite's goldens; `rustnative bench --target linux --check` passes.

---

## Phase 8 — Toolchain and packaging

**Design.**
- **`rustnative-build` on Linux:** from `rustnative.toml`, generates `<app-id>.desktop` (Name, Exec, Icon, Categories, `MimeType=x-scheme-handler/<scheme>;` per URL scheme, `Actions=` from jump-list entries), the icon at every hicolor size it can derive from the source PNG (no image library: the PNG is copied at its native size into `hicolor/<n>x<n>/apps`, and `scalable` when an SVG is given), and AppStream `<app-id>.metainfo.xml`; validated with `desktop-file-validate` and `appstreamcli validate` when present.
- **CLI:** `rustnative build linux` / `run linux` (Cargo with the system compiler; `pkg-config` checks the GTK floor first and names the missing `-dev` package); `rustnative package linux --format deb|tar|appimage|all`: the `.deb` is written natively (an `ar` archive of `debian-binary`, `control.tar.gz`, `data.tar.gz`, reproducible: sorted entries, fixed mtimes, root ownership), with `Depends` computed from the GTK floor; the tarball is the relocatable tree plus `SHA256SUMS`; the AppImage is assembled when `appimagetool` is on the path and refused with its install hint otherwise; `--sign <key>` detached-signs with `gpg` when present.
- **`doctor`:** Linux rows — compiler, `pkg-config`, GTK 4 version against the floor, display server, portals, AT-SPI bus, notification server, tray watcher, `appimagetool`, `dpkg-deb` (only as a cross-check).
- **`rustnative new`:** the template selects `rustnative-windows` or `rustnative-linux` per target.

**Acceptance tests:** the desktop entry and metainfo validate; the `.deb` installs with `dpkg -i` into a throwaway root (`dpkg --root`), and `dpkg-deb -I` reads its control file; a byte-identical rebuild; `rustnative doctor --json` has the Linux rows; `rustnative new` + `rustnative run linux` launches on WSLg.

---

## Phase 9 — Examples, documentation, distributions, and the full verification

**Design.** Every portable example selects its backend per target (`LinuxPlatform` on Linux); Linux gains `adoption-gtk` (embedding both directions); `docs/linux.md` (the guide: toolkit, mapping, capabilities, display servers, portals, packaging, what is owed), `docs/linux/{widget-mapping.md, desktop-matrix.md, accessibility.md}`; the checklist column; `README.md`, `PLAN.md` (Milestone 34 moved to Completed with an honest Implemented list), `BUILD_STATUS.md` (what was verified, where, and what is owed). The gate passes on Windows and on Linux (Ubuntu), the Linux crate's tests pass on Kali, `hello-label`, `gallery`, and `reference-app` run on WSLg under Wayland and under X11.

**Commit:** directly to `master` at the end of each phase, after its gate.
