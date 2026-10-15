# The terminal backend

`rustnative-tui` realizes the application model in a terminal (`PLAN.md`
Milestone 38): the same components, state, layout, identity, and events as
every other backend, with the terminal's own primitives as the host — the
cell grid, its colour depth and text attributes, its key and mouse
protocols, its resize notifications, its alternate screen and cursor, and,
where it has one, its clipboard sequence. A terminal is a real host with its
own conventions, so nothing here imitates a window: there are no title bars,
shadows, or bevels.

In scope: Windows, Linux, and macOS terminals, and embedded Linux consoles —
locally, over SSH, or on a serial line. Out of scope: Android, iOS, iPadOS,
and the browser (a terminal emulator there is that emulator's application).

## Start

A terminal application depends on `rustnative-core` and `rustnative-tui`
only — not on `rustnative-host`, which links the operating system's GUI
toolkit, so the binary runs on a server or a console with no desktop:

```rust
use rustnative_core::{Application, Component, Platform, Size, Window};
use rustnative_tui::{Channel, TuiPlatform};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let channel = Channel::new();
    let services = rustnative_tui::services("com.example.todo", "To do", &channel)?;
    let mut application =
        Application::with_services(Todo::new(()), Window::new("To do", Size::new(0, 0)), services);
    TuiPlatform::new().with_channel(channel).run(&mut application)?;
    Ok(())
}
```

The window's size is the terminal's. `examples/tui-demo` is a complete
application; `rustnative run tui` runs a project in the terminal it is
started from.

A binary that is both a desktop and a terminal application uses
`rustnative_host::terminal`, `terminal_services`, and `wants_terminal`
(`--tui`, or `RUSTNATIVE_TARGET=tui`, which `rustnative run tui` sets); the
project template does. On Windows such a binary is GUI-subsystem, and in
terminal mode it attaches to the console of the process that started it.

In markup the same view is written the same way — the terminal never sees
either syntax:

```rust
rsx! {
    <Column key="root">
        <Label key="title" text="To do" />
        <Button key="add" text="Add" />
    </Column>
}
```

## Geometry: cells

The core lays out in logical pixels; the backend converts at its boundary
(`PLAN.md` 2.11), by `rustnative_style::TUI_UNITS`:

- one cell is **8 logical pixels wide and 16 high** (`TuiPlatform::with_cell_size`
  changes it). Terminals do not reliably report their font's pixel size, so
  the ratio is the backend's, documented, rather than measured;
- every declared length — fixed sizes, margins, constraints, padding, gaps,
  grid tracks — is rounded to whole cells **before** layout
  (`TreeSnapshot::map_lengths` through `GridMapping::quantize`), half away
  from zero, a nonzero length at least one cell. So the default 12-pixel gap
  is one row everywhere, and the default 24-pixel padding is two rows and
  three columns;
- measured sizes are whole cells (below), so every rectangle lands on cell
  boundaries; a rectangle's two edges are snapped separately, so boxes that
  touch still touch;
- `rem` is one line: the terminal's text size is the person's, never
  reported, so the text scale is 1.

Dense terminal layouts set `padding` and `gap` to 0 on their containers
(`examples/tui-demo` does where it wants to).

### Text: Unicode display width

Text is measured in cells by display width, never by bytes or characters,
per grapheme cluster (UAX #29, through `unicode-segmentation`), from
Unicode 17.0.0's data (`width::generate::regenerate` builds the table from
`EastAsianWidth.txt`, `emoji-data.txt`, and `DerivedGeneralCategory.txt`):

| A cluster whose first visible scalar is | Takes |
|---|---|
| East Asian Wide or Fullwidth, or has emoji presentation | 2 cells |
| an emoji with text presentation followed by VS16 | 2 cells |
| East Asian Ambiguous | 1 cell; 2 under a Chinese, Japanese, or Korean locale (`LC_ALL`, `LC_MESSAGES`, `LANG`) |
| a nonspacing or enclosing mark, a format or control character, a Hangul medial vowel or final consonant | 0 cells |
| anything else | 1 cell |

A pair of regional indicators (a flag) takes 2. `RUSTNATIVE_TUI_AMBIGUOUS=wide`
or `narrow` overrides the locale, for a terminal configured against it.
Labels wrap at spaces, then between wide characters (as CJK text breaks),
then inside a word at a cluster boundary; a wide cluster is never split at
an edge.

Bidirectional reordering inside a line is the terminal's: most terminals
draw logical order. Layout mirroring is the backend's (`Capability::RightToLeft`):
rows are mirrored and start-aligned text sits at the right edge.

## Drawn widgets

Every node kind is a drawn widget with the portable semantics — the same
events a native control reports, the same focus rules:

| Node | Drawn as | Keys | Mouse |
|---|---|---|---|
| label | its text, wrapped, start-aligned | — | — |
| button | `[ text ]` | Enter, Space → `Click` | click → `Click` |
| text field | its text, underlined across the field, scrolled to the caret | editing (below) → `TextChanged` | click places the caret, drag selects |
| check box | `[x] label` | Space, Enter → `Toggled` | click |
| radio | `(*) label` | Space, Enter → `Toggled { on: true }` | click |
| switch | `[on] label` | Space, Enter → `Toggled` | click |
| slider | `───●───` and the value | arrows ±1, Page ±10 %, Home/End → `ValueChanged` | click and drag on the track |
| progress | `[████░░░]  40%`, or a moving block while unknown | — | — |
| select | the choice and `▾`; open, a bordered list below it (above if no room) | arrows change it closed; Enter/Space/Alt+Down open; in the list arrows, Enter, Escape → `SelectionChanged` | click opens, click chooses |
| list box | an item per row, `>` marking the chosen one | arrows, Page, Home/End → `SelectionChanged` | click chooses, wheel scrolls |
| date picker | `YYYY-MM-DD`, the focused part reversed | Left/Right choose the part, Up/Down step it (staying a date) → `DateChanged` | click chooses the part |
| spinner | `< 7 >` | Up/Down, Page, Home/End → `ValueChanged` | click: `<` side down, `>` side up |
| separator | `────` | — | — |
| link | underlined text | Enter → `Click` | click |
| multi-line text | soft-wrapped, scrolled to the caret | editing, Enter breaks lines, Up/Down by visual line | as a text field |
| image | half blocks `▀` (two pixels a cell); `[name]` with no colour | — | — |
| tab bar | ` One  Two ` with the chosen tab reversed | Left/Right, Home/End → `TabSelected` | click a tab |
| canvas | its draw list rasterized to cells (`rustnative_drawlist::raster`): fills as backgrounds, strokes as box drawing, text, images as half blocks; rotations by their bounding box | `TextInput`, `KeyDown` | `PointerDown/Move/Up` with the hit region |
| row / column | its background, a box-drawn border when it declares one, a scroll bar when it scrolls | Page Up/Down scroll | wheel scrolls three rows |

**Editing** moves by grapheme cluster, by word with Ctrl, to line or text
ends with Home/End (Ctrl for the text); Shift extends the selection; typing,
paste, Backspace, and Delete replace it; Ctrl+A selects all; Ctrl+C copies,
Ctrl+X cuts, Ctrl+V pastes (from the terminal's clipboard where it answers
OSC 52 reads, otherwise from what was copied in the application — the
terminal's own paste, Ctrl+Shift+V or right-click, arrives as a bracketed
paste either way). A field keeps what was typed until a render supplies a
different value, the controlled-input contract every backend implements.

**Focus** is the portable model unchanged: Tab and Shift+Tab visit every
focusable, enabled, shown node in tree order and wrap; a click focuses the
nearest focusable node; focus moving to a node out of view scrolls it in.
Scrolling is a viewport change and renders nothing (`PLAN.md` 2.10).

**Windows** stack: a second window an application opens covers the first
and titles the terminal; closing it shows the one beneath.
`MultipleWindows` is not advertised.

**Keys the host keeps:** Ctrl+C requests the shown window's close (the
primary window's close ends the application) unless a command bound to it
or a field with a selection takes it; Ctrl+Z suspends on Unix unless a
command takes it.

## Style

Styles are realized where they differ from the default theme's resolution
for the same kind and state: the default theme stands for the terminal's own
look (its default colours, SGR 39/49), so an unstyled application looks like
the terminal it runs in, light or dark. `rustnative_style::TUI`:

| Property | Answer |
|---|---|
| `color`, `background-color` | approximated: the terminal's colour depth (below); translucent colours blended over what is beneath |
| `border-color` | approximated: box drawing on the box's edge cells |
| `font-weight` | approximated: 600 and above bold, 300 and below faint |
| `opacity` | approximated: 0 draws nothing, below one half faint, otherwise opaque |
| `overflow`, `display`, and every layout property | realized |
| `border-radius`, `font-size`, `font-family`, `box-shadow` | unavailable — a cell has none of them; declaring one for a `tui` target (`[style] targets = ["tui"]`) is a build error naming `rustnative_style::TUI` |

`tests/style_table.rs` holds the table against the screen: every property
it answers as realized or approximated changes the grid when declared, and
every property it answers as unavailable changes nothing.

### Colour depth: detected, never assumed

The first rule that answers decides:

1. `RUSTNATIVE_TUI_COLORS` = `truecolor`, `256`, `16`, or `none`;
2. `NO_COLOR` set and not empty: no colour (attributes still apply);
3. `TERM=dumb`: no colour;
4. `COLORTERM` = `truecolor` or `24bit`: true colour;
5. Windows Terminal (`WT_SESSION`), iTerm2, WezTerm, VS Code, Ghostty,
   Hyper (`TERM_PROGRAM`), kitty, Alacritty, foot, a `-direct` `TERM`: true
   colour;
6. the Windows console in virtual-terminal mode with no `TERM`: true colour
   (since Windows 10 1703);
7. Apple's Terminal, or a `TERM` ending in `256color`: 256;
8. any other `TERM`, or none: 16.

Downsampling picks the nearest colour by weighted ("redmean") distance: from
the 256 palette's cube and grey ramp, or from xterm's 16. Nothing is ever
conveyed by colour alone: focus is reverse video, a check box's state is its
mark, a list's choice its `>`.

## Input

All input is one byte stream — termios on Unix, the console in
virtual-terminal input mode on Windows (conhost, or ConPTY under Windows
Terminal, translates keys, mouse, and paste into the same sequences) —
decoded by `rustnative_tui::decode`: UTF-8 text; C0 controls; Alt as an
`ESC` prefix; xterm's `CSI`/`SS3` keys with modifiers; the kitty keyboard
protocol (`CSI u`) where the terminal answers its query; xterm's
`modifyOtherKeys`; bracketed paste as one `TextInput` or `TextChanged`; SGR
mouse (press, release, drag, motion, wheel as `WheelDelta::Lines`); focus
in and out. A lone `ESC` is the Escape key after 40 ms with nothing after it.

What a terminal cannot tell apart it cannot report, and that is a capability
answer, not a bug. Without the kitty protocol: Ctrl+I is Tab, Ctrl+M is
Enter, Ctrl+[ is Escape, Ctrl+H is Ctrl+Backspace, Ctrl+Shift+letter is
Ctrl+letter, and key releases are not reported (`KeyUp` follows each
`KeyDown`).

| Terminal | Keys with modifiers | Kitty protocol | Mouse | Bracketed paste | Focus |
|---|---|---|---|---|---|
| conhost (Windows 10 19045) | arrows, Home/End, F-keys, Ctrl/Alt/Shift combinations as xterm sequences; verified in a real conhost window (`examples/tui-demo/tests/conhost.rs`) | no | not reported to virtual-terminal input on this build | conhost's own paste arrives as typed keys | console focus records |
| ConPTY (as under Windows Terminal) | as conhost, through the console's input records and back (`tests/real_terminal.rs`) | no | handled by ConPTY on Windows 10; Windows Terminal's own ConPTY forwards it | yes | yes |
| Linux terminals through the kernel's pty (`tests/real_terminal.rs`), tmux | xterm sequences | where the terminal answers | SGR | yes | yes |

Pen, touch, and gamepads are not terminal input and are not advertised.

## Capabilities: what the terminal answered

At startup the backend asks — DECRQM for modes 1003, 1004, 1006, 2004, and
2026, the kitty keyboard flags, the default background (OSC 11), and DA1
last as a sentinel — and keeps starting. Answers arrive as input; the probe
ends at DA1's answer or after 250 ms (`RUSTNATIVE_TUI_PROBE_MS`). An
unanswered question means unknown, and unknown is never advertised.

| Capability | Advertised |
|---|---|
| `CommandShortcuts`, `StatePersistence`, `Storage`, `Lifecycle`, `RightToLeft`, `Animations`, `ReducedMotionPreference`, `CustomDrawing` | always |
| `Clipboard` | where OSC 52 reaches a clipboard: DA1 lists 52, or the terminal is one known to (kitty, foot, Alacritty, Ghostty, iTerm2, WezTerm, Windows Terminal) |
| `Notifications` | where the terminal implements a sequence: OSC 99 (kitty), OSC 777 (foot, urxvt), OSC 9 (iTerm2, WezTerm, Ghostty) |
| `UrlLaunch` | in a local session with an opener (ShellExecute, `open`, `xdg-open` with a display); never over SSH, where it would open on the far machine |
| `Hover` | where DECRQM answers that modes 1003 and 1006 are recognized |
| `MultipleWindows`, `WindowManagement`, `Menus`, `FileDialogs`, `SystemAppearance`, `DragAndDrop`, `Touch`, `Pen`, `Gamepad`, `Ime`, `Cursors`, `NativeSurfaces`, `Printing`, `Http` | never |

The running session puts its answer in `keys::CAPABILITIES`. A dark default
background sets `keys::COLOR_SCHEME` to dark. `NO_MOTION` or
`RUSTNATIVE_REDUCED_MOTION` reduces motion.

**IME:** the terminal's input method composes and commits text into the
byte stream; composition never reaches the application, so `Ime` is not
advertised and committed text arrives as `TextInput`.

## Accessibility

A terminal's accessibility belongs to the terminal and its screen reader
(NVDA and Narrator with the Windows console, Orca with a Linux terminal,
BRLTTY and speakup on a console). The backend's obligation is a readable,
correctly ordered screen, and it keeps it:

- the screen reads in tree order, labels before their fields
  (`tests/portable.rs::the_screen_reads_in_order_with_labels_before_their_fields`);
- the terminal's cursor follows focus — shown as the caret in a field,
  placed (hidden) on the focused control otherwise — so a reader that tracks
  the cursor reads what has focus;
- nothing is conveyed by colour alone (`tests/portable.rs::nothing_is_conveyed_by_colour_alone`);
- live regions have no announcement mechanism to map onto: no terminal
  offers one, so none is assumed. A live region's text is on the screen
  where it is, for a reader that reviews it.

## Scheduling and redraw

One thread: input and redraw decoupled, frames paced to 30 a second
(`RUSTNATIVE_TUI_FPS`) and coalesced — fifty keystrokes inside one interval
are one frame (`tests/redraw.rs::frames_coalesce_under_load`). A frame is the
difference between the grid on screen and the next one, written as runs with
the cheapest cursor motion and the fewest SGR changes (a one-digit change is
one cell; typing writes about 24 bytes a key); a full redraw happens only on
resize, a colour-depth change, a probe's answers, or a return from
suspension. Where the terminal answers mode 2026, frames are synchronized.
Autowrap is off while the session runs, so writing the last column never
scrolls.

Animations run on the core's `Timeline`: opacity and colours animate (an
opacity transition goes faint below one half and disappears at zero);
position and size move in one step, since a cell cannot move by less than
itself. Under reduced motion, transitions jump and an unknown progress bar
stands still.

## Restoration

A crashed application must not leave a terminal unusable. Restoration is
armed before the terminal is changed and runs once, on whichever comes
first:

| Path | How |
|---|---|
| normal exit | the terminal's `Drop` |
| a panic, anywhere | a panic hook chained to the one before it, so the report prints on the restored screen |
| `SIGTERM`, `SIGINT`, `SIGQUIT`, `SIGHUP` | the loop ends cleanly, flushing state; a second signal while it is stuck restores from the signal handler and exits with 128 + the signal |
| `SIGTSTP` | the terminal is given back and the process stops; `SIGCONT` retakes it and redraws (a process alone in its session — an orphaned process group — is not stopped, as POSIX says, and retakes at once) |
| Ctrl+Break, the console closing, logoff, shutdown (Windows) | the control handler tells the loop, waits up to three seconds, then restores itself |

What is restored: the termios mode and file flags (or the console's input
and output modes and code pages), the main screen, the cursor's visibility
and shape, autowrap, mouse reporting, focus reporting, bracketed paste, the
kitty protocol, a synchronized frame left open, the SGR state, and the
title (`rustnative_tui::guard`; `guard::teardown_policy` for Milestone 39).
`tests/real_terminal.rs` proves each path on a real pty and through ConPTY.

## Services

`rustnative_tui::services(app_id, name, &channel)`: the OSC 52 clipboard
(reads where the terminal answers, a copy kept in the process always),
notifications through the terminal's sequence, URL launch (only a whole
http, https, or mailto URL, through ShellExecute on Windows — never a
command interpreter), crash-safe state files in the same folders the desktop
backends use (`$XDG_STATE_HOME/<id>/state`, `%LOCALAPPDATA%\<id>\state`), and
documents. `rustnative_host::terminal_services` adds the operating system's
windowless services (HTTP, secure storage, locale formatting) for a binary
that has them. Nothing that opens a window is offered.

Everything a service writes to the terminal goes through the loop, between
frames.

## Embedding (Milestone 40)

A terminal has no host objects to embed in either direction: there is no
foreign widget a terminal application could host, and no other program's
widget tree it could live inside — a terminal multiplexer (tmux, screen) is
another terminal, and the application runs in it unchanged (verified in
tmux). Both directions are answered as the host permitting neither.
Incremental adoption works at the process level instead: a terminal program
can hand the terminal to a Rust Native screen and take it back
(`rustnative_tui::guard::restore`, then a new `TuiPlatform::run`).

## Inspection and the development loop

The inspection protocol is already a side channel — line-delimited JSON over
loopback TCP (`RUSTNATIVE_INSPECT=1`) — so a terminal application is
inspected from another terminal with `rustnative inspect`; the backend
answers with each node's cells (`TuiInspect`). The overlay, when an
inspector turns it on, is drawn into the grid.

`rustnative dev tui` builds, starts the application in this terminal, and
restarts it with its state when a source changes. While the application owns
the terminal, the loop writes nothing to it: its messages and the build's
output go to `target/rustnative-dev/dev.log`, and the loop ends when the
application does.

## Testing

`TuiHarness` drives an application on a synthetic terminal of a given size:
input goes in as the bytes a terminal sends, and the screen asserted is what
a VT emulator (`rustnative_tui::emulator`) makes of the bytes the session
actually wrote — so the encoder is tested with everything else. Time is
virtual. It is one of the headless test backends (Milestone 45), and it runs
the shared guarantee suites and layout conformance (`tests/conformance.rs`).

```rust
let mut terminal = TuiHarness::launch(Window::new("To do", Size::new(0, 0)), 80, 24, || Todo::new(()));
terminal.click_key("draft");
terminal.type_text("Buy milk");
terminal.press_enter();
assert!(terminal.text().contains("Buy milk"));
```

`terminal.golden()` is the screen's text with a line of attribute marks per
row; `RUSTNATIVE_BLESS=1` rewrites goldens. `tests/real_terminal.rs` runs the
backend as a child on a real pty (Linux, macOS) or ConPTY (Windows);
`examples/tui-demo/tests/conhost.rs` (ignored by default: it opens a window)
drives a real conhost window.

## Performance

`budgets/tui.toml`, measured by `rustnative bench --target tui`
(`examples/tui-bench`): launch to first frame, Enter to its frame, bytes per
keystroke, full-screen frame time, resident memory, and binary size. On the
reference machine: 12 / 6 ms to the first frame, 24 bytes a keystroke, 6 /
4 ms for a full-screen frame, and 8 / 7 MB resident (Windows / Linux).

## Packaging

`rustnative package tui` uses the host operating system's formats: on
Windows a zip with `SHA256SUMS`; on Linux a `.deb`, an `.rpm`, a pacman
package, a tarball, and an AppImage, each with a desktop entry that opens the
application in the desktop's terminal (`Terminal=true`), an AppStream
`console-application` entry, and only the C library as a dependency. Updates
follow the host operating system's rules (`docs/deploy/update-rules.md`).
On macOS the format is a Homebrew formula, which arrives with Milestone 33's
toolchain.

## Verified on

| Host | How |
|---|---|
| Windows 10 Pro 19045, conhost | a real console window driven through its input records and read from its screen buffer (`examples/tui-demo/tests/conhost.rs`) |
| Windows 10, ConPTY (the console host under Windows Terminal) | `tests/real_terminal.rs`: restoration on Ctrl+C, on a component panic, and on a panic outside every boundary; resizes; every key; closing the console |
| Ubuntu 24.04 (WSL), the kernel's pty | `tests/real_terminal.rs`: the above, and SIGTERM, a second SIGTERM on a stuck component, SIGTSTP/SIGCONT, SIGWINCH |
| Ubuntu 24.04 (WSL), tmux; over SSH | `tools/tui-session.sh` (see `BUILD_STATUS.md` for the run) |
| the synthetic terminal | every other test, the same on every machine |

**macOS: not yet verified.** The Unix half (`term/unix.rs`: termios,
`TIOCGWINSZ`, the self-pipe, job control) is written for macOS too and builds
against `libc`'s macOS definitions, but it has not run on a Mac: Terminal.app
(256 colours), iTerm2, and the macOS `script`-based pty tests are owed when a
Mac is available (`PLAN.md` 2.13). Until then macOS is not called supported.
