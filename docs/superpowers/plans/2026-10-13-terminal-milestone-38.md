# Milestone 38 Implementation Plan (the terminal backend)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan phase-by-phase. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Status: built (2026-10-15); macOS terminals owed to a Mac. See "Design decisions" and "Progress" at the end.** Unlike the Apple targets, nothing here needs hardware this project lacks: Windows Terminal and conhost on Windows, and the terminals of the WSL distributions (and a serial console through WSL), are the hosts it is verified on.

**Goal:** Build `rustnative-tui` — `PLAN.md` "Milestone 38 — Terminal (TUI) backend" — to `PLAN.md` §8's definition in the reduced form §8 and Milestone 38 allow a terminal: the terminal's cell grid as the host object, honest capabilities, and §11's gates where a terminal can carry them.

**Architecture:**

- **`rustnative-drawlist`** (the draw-list group: terminal, embedded displays, the inspection overlay — `docs/conformance/platform-groups.md`): rasterizing a `DrawList`, the geometry conversion between logical pixels and a host's own grid, a focus-ring and hit-test model for hosts with no native controls, and damage tracking with dirty-region coalescing. Created with this milestone (its first host member) and shared with Milestone 37 rather than written twice.
- **`rustnative-tui`**: the terminal host — raw mode and its restoration, input decoding, output encoding with the terminal's colour depth and attributes, the cell grid and its damage-tracked writer, drawn widgets for every node kind, and its capability table, style table (`TUI`, `TUI_UNITS`), conformance column, and budgets.

**Tech stack:** Rust 2024 at the workspace MSRV. On Windows, `windows-sys` for the console API (virtual-terminal processing, input records, resize events); elsewhere `rustix` for `termios`, `SIGWINCH`, and non-blocking reads. Unicode display width through a vendored East Asian Width and emoji-presentation table generated from the Unicode version the core's i18n data uses, and grapheme clusters through `unicode-segmentation` — no terminal library, because the backend's job is exactly what such a library does and the framework needs it shaped as a backend.

**Spec:** `PLAN.md` §2.3 (the terminal paragraph), §2.11 (geometry in cells), §2.14 (capability honesty), §8 (Milestone 38), §11; `docs/conformance/new-backend-checklist.md`; `docs/styling.md`.

## Scope decision (2026-10-13)

Written by the 2026-10-12 review, which found Milestone 38 well specified in `PLAN.md` but with no implementation plan, no `tui` platform in the CLI, no budgets file, and no style table.

- **In scope:** every Milestone 38 bullet; §8's definition in its reduced form; the §11 gates a terminal can carry.
- **Out of scope:** Android, iOS, iPadOS, and the browser (Milestone 38: a terminal emulator inside them is the emulator's application); mouse-pointer cursors, pen, touch, gamepad.

## Global Constraints

- The core stays platform-free; the draw-list group crate depends on the core only.
- **Terminal restoration is a correctness requirement**: raw mode, the alternate screen, the cursor, mouse reporting, and bracketed paste are restored on exit, on `SIGINT`/`SIGTERM`/console close, and on panic, through one guard installed before the first escape sequence is written and a panic hook chained to the core's teardown policy (Milestone 39).
- Geometry converts at the backend boundary only (2.11): the core lays out in logical pixels; the backend maps them to cells with its documented ratio per axis, a nonzero length never rounding to zero cells.
- Output is written only as damage: no full redraw except on resize, colour-depth change, or after the terminal reports it lost state.

## Verification gate

```bash
cargo clippy -p rustnative-drawlist -p rustnative-tui --all-targets -- -D warnings
cargo test -p rustnative-drawlist -p rustnative-tui      # the synthetic terminal
tools/tui-session.sh cargo test -p rustnative-tui -- --ignored  # a real pty (WSL) / ConPTY (Windows)
```

`tools/tui-session.sh` starts the tests under a pseudo-terminal of a known size (`script` or a ConPTY host), so real escape sequences round-trip through a real terminal driver.

## Crates

| Crate | Kind | Change |
|---|---|---|
| `rustnative-drawlist` | library (group) | new |
| `rustnative-tui` | library (backend) | new |
| `rustnative-host` | library | not selected by `target_os`: a terminal application chooses `TuiPlatform` explicitly (one binary can be both a GUI and a TUI application), exported as `rustnative_host::terminal()` |
| `rustnative-style` | library | `TUI`, `TUI_UNITS`; `tui` in `[style] targets` |
| `rustnative` (CLI) | binary | `Platform::Tui` (`rustnative run|build|package tui`), recognized today and failing with "Milestone 38" until the backend exists |

## Phase 0 — The portable pieces (no new host)

1. **`Platform::Tui` in the CLI**, failing with the milestone until the backend exists (done by the 2026-10-12 review, with `ipados`).
2. **Damage tracking in the draw-list group**, extracted so the embedded backend and the inspection overlay use the same coalescing.
3. **A cell-quantized layout check**: the core's layout under a unit mapping whose pixel is a cell (8×16 by default, configurable), asserting nothing collapses to zero cells — a test the core runs, not a backend.

## Phase 1 — The host: raw mode, input, output, restoration

- Raw mode (`termios` / `SetConsoleMode` with virtual-terminal input and output), the alternate screen, cursor visibility, bracketed paste, focus reporting, SGR mouse reporting where the terminal answers it; all restored by the guard.
- Input decoding: keys with modifiers and function keys (a key the terminal cannot report is a capability answer, recorded per terminal in `docs/tui.md`), paste as one `Event::TextInput`, mouse click, drag, and wheel (`WheelDelta::Lines`), resize from `SIGWINCH` or console buffer events, focus in and out.
- Output: the terminal's colour depth detected (`COLORTERM`, `TERM`, the console's mode), never assumed; truecolour downsampled to 256 or 16 colours with the documented rule; text attributes (bold, italic, underline, reverse, dim).

**Acceptance tests:** `a_panic_restores_the_terminal` (a deliberate panic under the pty, then the terminal's mode read back); `every_key_the_terminal_reports_is_decoded`; `a_resize_redraws_once`.

## Phase 2 — The cell grid and drawn widgets

- A `DrawList` rasterized to cells: text at its display width (grapheme clusters, wide East Asian characters, combining marks, emoji presentation), box-drawing borders where the font has them (a border approximated, `BorderColor` answered approximated), fills as background colours.
- Every node kind as a drawn widget with the portable semantics: labels, buttons (activated by Enter and Space and a click), text inputs (a caret, selection, clipboard through OSC 52 where available), check boxes, radios, toggles, sliders, progress, select and list box as lists, tab bars, scroll containers with scroll bars; focus and traversal through the portable focus model (Tab, Shift+Tab, `disabled`).
- **Style**: `rustnative_style::TUI` — colours and text attributes realized; font size, family, corner radius, shadow, and opacity unavailable (a cell has none), stated with the reason; `TUI_UNITS`: one cell is 8 logical pixels wide and 16 high by default, `rem` is one line, rounding to whole cells half away from zero with a nonzero length at least one cell.

**Acceptance tests:** the synthetic-terminal harness's grid goldens for every node kind; `the_tui_capability_table_is_what_the_grid_shows`; the shared guarantee suites over the synthetic terminal; layout under pseudo-localization with wide characters.

## Phase 3 — Accessibility, honestly

A terminal's accessibility belongs to the terminal and its screen reader: the backend's obligation is a readable, correctly ordered screen — focus moved with the cursor, labels before their fields, nothing conveyed by colour alone — and live regions mapped onto the terminal's announcement mechanism where one exists (none is assumed). `docs/tui.md` states this, and the checklist's accessibility rows answer it as such.

## Phase 4 — Scheduling, redraw, and animation

A single-threaded loop on the core's `LocalExecutor`, input and redraw decoupled, frames coalesced and paced to a terminal-sane rate (30 Hz by default), animations through the same `Timeline` (or skipped under reduced motion, read from `NO_MOTION`/the environment), damage-only writes.

**Acceptance tests:** `a_change_writes_only_its_cells` (bytes written measured); `frames_coalesce_under_load`.

## Phase 5 — The §11 gates, reduced

- **Inspection** over a side channel (a socket), since the terminal is the output (Milestone 44).
- **Headless test backend**: the synthetic terminal counts as one (Milestone 45).
- **Conformance**: the terminal column of `new-backend-checklist.md`, with each row answered or stated inapplicable and why.
- **Budgets** (Milestone 42): `budgets/tui.toml` — startup to first frame, resident memory, bytes written per keystroke, frame time under a full-screen update — enforced by `rustnative bench --target tui` in CI on Linux and Windows.
- **Developer loop** (Milestone 43): `rustnative dev tui` restarting into the same terminal with state kept.

## Phase 6 — Toolchain, packaging, documentation

`rustnative run|build tui` (Cargo alone); packaging reuses the host operating system's: the zip and checksums on Windows, the Linux distribution packages and tarball, a Homebrew formula on macOS (when Milestone 33's toolchain exists); `docs/tui.md`; the update rule (the host operating system's rows in `docs/deploy/update-rules.md`); `README.md`, `PLAN.md`, `BUILD_STATUS.md`.

**Done when** Milestone 38's bullets hold on Windows Terminal, conhost, and at least two Unix terminals (one over SSH), terminal restoration is proven under panic and signal, the terminal checklist column is complete, and the budgets are met in CI.

## Design decisions (2026-10-14)

Taken when implementation started, after a survey of the core contracts the
backend drives (`Application`, `TreeSnapshot`, `LayoutEngine`, `Timeline`,
`InspectBackend`, `ConformanceHost`) and of the two backends it resembles
most: the headless backend (a realized model with no host objects) and the
Linux backend (styles applied only where they differ from the default theme).

### Module map

`rustnative-drawlist` (depends on `rustnative-core` only):

| Module | Owns |
|---|---|
| `grid` | `GridMapping`: logical pixels ↔ a host grid (cells, or a display's pixels), per axis, with the one rounding rule — half away from zero, edges snapped separately so adjacent boxes stay adjacent, a nonzero length never zero units |
| `damage` | `Region`: dirty rectangles, coalesced (overlapping or touching rectangles merged when the merge wastes little; a bounded count, beyond which they collapse to bounding boxes) |
| `focus` | the drawn host's focus and hit-test model: traversal order from a snapshot (focusable, enabled, shown, preorder), next/previous, the deepest interactable node under a point |
| `raster` | a `DrawList` rasterized onto any grid through `RasterTarget` (fills, strokes as edge-connected units so a terminal can choose box-drawing glyphs, text, images sampled per unit, clips, opacity, translation and scale exactly and other transforms by their bounding box), hit regions collected |

`rustnative-tui`:

| Module | Owns |
|---|---|
| `width` (+ generated `width_table`) | display width per grapheme cluster from Unicode 17.0 (`unicode-segmentation`'s version): East Asian Wide/Fullwidth 2, emoji presentation 2 (VS16 widens, VS15 narrows), marks and format characters 0, Ambiguous 1 except under a CJK locale; wrapping and truncation by width. The table is generated by an ignored test (`width::table_tests::regenerate`) from the UCD files |
| `color` | `ColorDepth` detected (`NO_COLOR`, `COLORTERM`, `TERM`, `WT_SESSION`, the console mode), downsampling truecolour → 256 (cube and grey ramp, nearest by weighted distance) → 16 (nearest of the xterm palette) |
| `decode` | the VT input decoder: CSI/SS3 keys with xterm modifiers, the kitty keyboard protocol's `CSI u`, Alt as an `ESC` prefix, a lone `ESC` resolved by a timeout, bracketed paste, SGR mouse, focus in/out, and the replies the probe asks for (DA1, DECRQM, kitty flags, OSC 52) |
| `encode` | SGR with tracked state, cursor motion, modes; synchronized output (mode 2026) where answered |
| `grid` | the cell grid (wide-cell continuation, clipping), and the damage-only writer: a frame is the diff of two grids, written as the fewest runs |
| `emulator` | a VT emulator interpreting exactly what the encoder writes, so the synthetic terminal asserts the *bytes'* result rather than the grid the writer meant |
| `term` | the `Terminal` seam; `unix` (libc `termios`, `TIOCGWINSZ`, `poll`, a self-pipe for signals and wakes — libc rather than `rustix`, because signal disposition is not in `rustix`'s stable API) and `windows` (`windows-sys` console API in virtual-terminal mode, `WaitForMultipleObjects` over the input handle and a wake event, `SetConsoleCtrlHandler`) |
| `guard` | restoration: one guard entered before the first escape sequence; restored on drop, by a panic hook chained to the previous one, on `SIGINT`/`SIGTERM`/`SIGHUP`/`SIGQUIT` (a second signal restores from the handler and exits), on `SIGTSTP` (restored, stopped, re-entered on `SIGCONT` with a full redraw), and on console close/logoff/shutdown on Windows |
| `probe` | what the terminal is: environment, then DA1, DECRQM for modes 1003/1004/1006/2004/2026, the kitty keyboard query, answered within a deadline; unanswered means unknown, never assumed |
| `model` | the realized tree: per-node identity and widget state (caret, selection, scroll, list cursor, an open select), logical and cell rectangles, focus, creation/destruction counts |
| `paint` | every node kind as a drawn widget onto the grid |
| `edit` | grapheme-aware single- and multi-line editing: caret, selection, word motion, clipboard |
| `session` | the loop independent of I/O: input → focus and widget semantics → dispatch; tasks, inspection, animation, frames paced (30 Hz) and coalesced; used by the platform and by the harness |
| `platform` | `TuiPlatform` on the process's own terminal |
| `harness` | `TuiHarness`: a synthetic terminal of a given size, virtual time, input as bytes, the screen as text and attributes, bytes written counted |
| `services` | `Osc52Clipboard`, `TerminalSystem` (OSC 9/99/777 notifications where the terminal implements one; URL launch only in a local session), the state store's directory |
| `inspect` | the `InspectBackend` answers, over the TCP side channel |

### Decisions

- **Style baseline.** As on Linux, a style is realized where it differs from
  the default theme's resolution for the same kind and state: the default
  theme stands for the terminal's own default colours (SGR 39/49), so an
  unstyled application looks like the terminal it runs in, light or dark.
- **Services without a GUI toolkit.** A terminal application depends on
  `rustnative-tui` (and `rustnative-core`), never on `rustnative-host`, so a
  server or console without GTK can run it; `rustnative_host::terminal()`
  exists for one binary that is both. HTTP is not a terminal service; the
  backend does not advertise it.
- **A window stack, not windows.** A second window an application opens
  covers the first (the terminal shows the topmost open window); closing it
  shows the one beneath. `MultipleWindows` is not advertised.
- **Ctrl+C** is a key first: a command bound to it takes it; otherwise it
  requests the window's close. **Ctrl+Z** suspends on Unix unless a command
  takes it. On Windows Ctrl+Z is a key.
- **Mouse** is requested (1000/1002/1003 with 1006) everywhere; `Hover` is
  advertised only where DECRQM answers that 1003 and 1006 are recognized.
- **Select** opens as a list drawn over the content below it — the
  terminal's equivalent of a drop-down, not an imitation window.
- **Opacity is approximated**, not unavailable as Phase 2 first said: zero
  draws nothing and below one half is faint (SGR 2) — something a cell does
  show, so answering it unavailable would refuse what the screen realizes
  (`tests/style_table.rs` holds every answer against the screen).
- **Lengths are quantized before layout** (`TreeSnapshot::map_lengths`), not
  only rectangles after it: rounding positions alone made a 12-pixel gap
  alternate between one row and none.

### Progress

- [x] Phase 0 — `Platform::Tui`, `rustnative-drawlist` with damage tracking, the cell-quantized layout check (`rustnative-drawlist/tests/cell_quantized_layout.rs`), and `TreeSnapshot::map_lengths`
- [x] Phase 1 — host I/O, decoding, encoding, restoration (`tests/real_terminal.rs` on a pty, ConPTY, and over SSH)
- [x] Phase 2 — the cell grid, drawn widgets, style table, harness goldens (`tests/widgets.rs`, `tests/style_table.rs`, `tests/conformance.rs`)
- [x] Phase 3 — accessibility as the terminal's (`tests/portable.rs`)
- [x] Phase 4 — the loop, redraw, animation (`tests/redraw.rs`)
- [x] Phase 5 — inspection, conformance column, budgets, developer loop
- [x] Phase 6 — toolchain, packaging, documentation
- [ ] macOS: run `tests/real_terminal.rs` and `tools/tui-session.sh` on a Mac (Terminal.app, iTerm2) when one is available; add the Homebrew formula with Milestone 33
