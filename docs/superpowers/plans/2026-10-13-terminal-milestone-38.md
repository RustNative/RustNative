# Milestone 38 Implementation Plan (the terminal backend)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan phase-by-phase. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Status: planned, not started.** Unlike the Apple targets, nothing here needs hardware this project lacks: Windows Terminal and conhost on Windows, and the terminals of the WSL distributions (and a serial console through WSL), are the hosts it is verified on.

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
