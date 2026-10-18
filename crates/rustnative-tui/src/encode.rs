//! Terminal output: escape sequences, and the damage-only writer.
//!
//! Output is written only as damage (`PLAN.md` Milestone 38, "damage
//! tracking so only changed cells are written"): [`write_diff`] compares the
//! grid on screen with the next one and writes the changed cells as runs,
//! moving the cursor only where a run does not continue the last one, and
//! changing SGR state only where the style changes. A full redraw happens
//! only when the screen is known to be lost — on resize, a colour-depth
//! change, or a return from suspension ([`write_full`]).

use std::fmt::Write as _;

use crate::color::{ColorDepth, TermColor};
use crate::grid::{Attrs, CellStyle, Grid};

/// The sequences the backend writes, by name.
pub mod seq {
    /// The alternate screen, with the cursor saved (DEC 1049).
    pub const ENTER_ALT_SCREEN: &str = "\x1b[?1049h";
    /// Back to the main screen and the saved cursor.
    pub const LEAVE_ALT_SCREEN: &str = "\x1b[?1049l";
    /// Hides the cursor (DECTCEM).
    pub const HIDE_CURSOR: &str = "\x1b[?25l";
    /// Shows the cursor.
    pub const SHOW_CURSOR: &str = "\x1b[?25h";
    /// Autowrap off (DECAWM), so writing the last column never scrolls.
    pub const AUTOWRAP_OFF: &str = "\x1b[?7l";
    /// Autowrap on.
    pub const AUTOWRAP_ON: &str = "\x1b[?7h";
    /// Bracketed paste on (2004).
    pub const PASTE_ON: &str = "\x1b[?2004h";
    /// Bracketed paste off.
    pub const PASTE_OFF: &str = "\x1b[?2004l";
    /// Focus reporting on (1004).
    pub const FOCUS_ON: &str = "\x1b[?1004h";
    /// Focus reporting off.
    pub const FOCUS_OFF: &str = "\x1b[?1004l";
    /// Mouse reporting: presses (1000), drags (1002), all motion (1003),
    /// in SGR encoding (1006).
    pub const MOUSE_ON: &str = "\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h";
    /// Mouse reporting off.
    pub const MOUSE_OFF: &str = "\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l";
    /// The kitty keyboard protocol, "disambiguate" flag, pushed.
    pub const KITTY_KEYS_ON: &str = "\x1b[>1u";
    /// The kitty keyboard protocol popped.
    pub const KITTY_KEYS_OFF: &str = "\x1b[<u";
    /// The window title saved (xterm's title stack).
    pub const PUSH_TITLE: &str = "\x1b[22;0t";
    /// The window title restored.
    pub const POP_TITLE: &str = "\x1b[23;0t";
    /// Every SGR attribute and colour reset.
    pub const RESET_STYLE: &str = "\x1b[0m";
    /// The screen erased.
    pub const CLEAR: &str = "\x1b[2J";
    /// The cursor shape reset to the terminal's own (DECSCUSR 0).
    pub const RESET_CURSOR_SHAPE: &str = "\x1b[0 q";
    /// A frame begins (synchronized output, 2026).
    pub const BEGIN_SYNC: &str = "\x1b[?2026h";
    /// A frame ends.
    pub const END_SYNC: &str = "\x1b[?2026l";
    /// Primary device attributes asked (DA1).
    pub const ASK_ATTRIBUTES: &str = "\x1b[c";
    /// The kitty keyboard protocol's flags asked.
    pub const ASK_KITTY_KEYS: &str = "\x1b[?u";
    /// The default background colour asked (OSC 11).
    pub const ASK_BACKGROUND: &str = "\x1b]11;?\x1b\\";

    /// Everything a session turned on, turned off again, in the order that
    /// leaves the terminal as it was: what the restoration guard writes.
    pub const RESTORE: &str = concat!(
        "\x1b[?2026l", // a frame cut short
        "\x1b[0m",
        "\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l",
        "\x1b[?1004l",
        "\x1b[?2004l",
        "\x1b[<u",
        "\x1b[?7h",
        "\x1b[0 q",
        "\x1b[?25h",
        "\x1b[?1049l",
        "\x1b[23;0t",
    );

    /// A DECRQM question about DEC private mode `mode`.
    #[must_use]
    pub fn ask_mode(mode: u16) -> String {
        format!("\x1b[?{mode}$p")
    }

    /// The window title set (OSC 2), with controls removed.
    #[must_use]
    pub fn title(title: &str) -> String {
        let clean: String = title.chars().filter(|c| !c.is_control()).collect();
        format!("\x1b]2;{clean}\x1b\\")
    }
}

/// Builds output, tracking the terminal's cursor and style so it writes
/// only what changes.
#[derive(Debug, Default)]
pub struct Encoder {
    out: Vec<u8>,
    cursor: Option<(i32, i32)>,
    style: Option<Sent>,
}

/// The SGR state last sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sent {
    attrs: Attrs,
    fg: TermColor,
    bg: TermColor,
}

fn color_params(out: &mut String, color: TermColor, background: bool) {
    let _ = match (color, background) {
        (TermColor::Default, false) => write!(out, ";39"),
        (TermColor::Default, true) => write!(out, ";49"),
        (TermColor::Ansi(n), false) if n < 8 => write!(out, ";{}", 30 + n),
        (TermColor::Ansi(n), true) if n < 8 => write!(out, ";{}", 40 + n),
        (TermColor::Ansi(n), false) => write!(out, ";{}", 90 + (n - 8).min(7)),
        (TermColor::Ansi(n), true) => write!(out, ";{}", 100 + (n - 8).min(7)),
        (TermColor::Indexed(n), false) => write!(out, ";38;5;{n}"),
        (TermColor::Indexed(n), true) => write!(out, ";48;5;{n}"),
        (TermColor::Rgb(r, g, b), false) => write!(out, ";38;2;{r};{g};{b}"),
        (TermColor::Rgb(r, g, b), true) => write!(out, ";48;2;{r};{g};{b}"),
    };
}

impl Encoder {
    /// An encoder that knows nothing of the terminal's state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets what it knew (after anything else wrote to the terminal).
    pub fn invalidate(&mut self) {
        self.cursor = None;
        self.style = None;
    }

    /// Appends raw bytes.
    pub fn raw(&mut self, text: &str) {
        self.out.extend_from_slice(text.as_bytes());
    }

    /// Takes what has been built.
    pub fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out)
    }

    /// The bytes built so far.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.out
    }

    /// Moves the cursor to (`x`, `y`), 0-based.
    pub fn move_to(&mut self, x: i32, y: i32) {
        if self.cursor == Some((x, y)) {
            return;
        }
        match self.cursor {
            Some((cx, cy)) if cy == y && x > cx && x - cx <= 4 => {
                // A short hop right is cheaper as CUF.
                let _ = write!(self, "\x1b[{}C", x - cx);
            }
            Some((_, cy)) if cy == y => {
                let _ = write!(self, "\x1b[{}G", x + 1);
            }
            _ => {
                let _ = write!(self, "\x1b[{};{}H", y + 1, x + 1);
            }
        }
        self.cursor = Some((x, y));
    }

    /// Sets the style cells are written in, at `depth`.
    pub fn set_style(&mut self, style: CellStyle, depth: ColorDepth) {
        let encode = |color: Option<rustnative_core::Color>| {
            color.and_then(|color| depth.encode(color)).unwrap_or(TermColor::Default)
        };
        let wanted = Sent { attrs: style.attrs, fg: encode(style.fg), bg: encode(style.bg) };
        if self.style == Some(wanted) {
            return;
        }
        let mut params = String::new();
        let previous = match self.style {
            Some(previous) if previous.attrs.is_subset_of(wanted.attrs) => previous,
            _ => {
                params.push_str(";0");
                Sent { attrs: Attrs::NONE, fg: TermColor::Default, bg: TermColor::Default }
            }
        };
        for (attr, code) in [
            (Attrs::BOLD, 1),
            (Attrs::DIM, 2),
            (Attrs::ITALIC, 3),
            (Attrs::UNDERLINE, 4),
            (Attrs::REVERSE, 7),
            (Attrs::STRIKE, 9),
        ] {
            if wanted.attrs.has(attr) && !previous.attrs.has(attr) {
                let _ = write!(params, ";{code}");
            }
        }
        if previous.fg != wanted.fg {
            color_params(&mut params, wanted.fg, false);
        }
        if previous.bg != wanted.bg {
            color_params(&mut params, wanted.bg, true);
        }
        if let Some(params) = params.strip_prefix(';') {
            let _ = write!(self, "\x1b[{params}m");
        }
        self.style = Some(wanted);
    }

    /// Writes `text` (one cluster of `width` cells) at the cursor.
    pub fn text(&mut self, text: &str, width: i32) {
        self.out.extend_from_slice(text.as_bytes());
        if let Some((x, y)) = self.cursor {
            self.cursor = Some((x + width, y));
        }
    }
}

impl std::fmt::Write for Encoder {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.out.extend_from_slice(text.as_bytes());
        Ok(())
    }
}

/// Writes the cells of `next` that differ from `previous` (which must be
/// the same size; otherwise everything is written). Returns how many cells
/// were written.
pub fn write_diff(encoder: &mut Encoder, previous: &Grid, next: &Grid, depth: ColorDepth) -> usize {
    let same_size = previous.columns() == next.columns() && previous.rows() == next.rows();
    let mut written = 0;
    for y in 0..next.rows() {
        let row = next.row(y);
        let old = if same_size { previous.row(y) } else { &[] };
        let mut x = 0;
        while x < row.len() {
            let cell = &row[x];
            // A wide cell whose continuation changed is rewritten whole, and
            // a continuation is written with its first half.
            let changed =
                old.get(x) != Some(cell) || (cell.width == 2 && old.get(x + 1) != row.get(x + 1));
            if cell.is_continuation() || !changed {
                x += 1;
                continue;
            }
            let column = i32::try_from(x).unwrap_or(0);
            encoder.move_to(column, i32::from(y));
            encoder.set_style(cell.style, depth);
            let text = if cell.text.is_empty() { " " } else { cell.text.as_str() };
            encoder.text(text, i32::from(cell.width.max(1)));
            written += 1;
            x += usize::from(cell.width.max(1));
        }
    }
    written
}

/// Clears the screen and writes every cell of `grid`.
pub fn write_full(encoder: &mut Encoder, grid: &Grid, depth: ColorDepth) -> usize {
    encoder.set_style(CellStyle::DEFAULT, depth);
    encoder.raw(seq::CLEAR);
    encoder.invalidate();
    encoder.set_style(CellStyle::DEFAULT, depth);
    let blank = Grid::new(grid.columns(), grid.rows());
    write_diff(encoder, &blank, grid, depth)
}

#[cfg(test)]
mod tests {
    use rustnative_core::Color;

    use super::*;
    use crate::width::Width;

    #[test]
    fn styles_change_only_what_changed() {
        let mut encoder = Encoder::new();
        let bold = CellStyle::DEFAULT.attrs(Attrs::BOLD);
        encoder.set_style(bold, ColorDepth::TrueColor);
        assert_eq!(encoder.take(), b"\x1b[0;1m");
        encoder.set_style(bold.fg(Some(Color::rgb(1, 2, 3))), ColorDepth::TrueColor);
        assert_eq!(encoder.take(), b"\x1b[38;2;1;2;3m", "an addition is not a reset");
        encoder.set_style(CellStyle::DEFAULT, ColorDepth::TrueColor);
        assert_eq!(encoder.take(), b"\x1b[0m", "a removal resets");
        encoder.set_style(CellStyle::DEFAULT.bg(Some(Color::rgb(255, 0, 0))), ColorDepth::Ansi16);
        assert_eq!(encoder.take(), b"\x1b[101m");
        encoder.set_style(CellStyle::DEFAULT.bg(Some(Color::rgb(255, 0, 0))), ColorDepth::Ansi256);
        assert_eq!(encoder.take(), b"\x1b[48;5;196m");
        encoder.set_style(
            CellStyle::DEFAULT.fg(Some(Color::rgb(9, 9, 9))).attrs(Attrs::REVERSE),
            ColorDepth::None,
        );
        assert_eq!(
            encoder.take(),
            b"\x1b[7;49m",
            "no colour: the default background, attributes kept"
        );
    }

    #[test]
    fn cursor_moves_take_the_cheapest_form() {
        let mut encoder = Encoder::new();
        encoder.move_to(4, 2);
        assert_eq!(encoder.take(), b"\x1b[3;5H");
        encoder.move_to(6, 2);
        assert_eq!(encoder.take(), b"\x1b[2C");
        encoder.move_to(0, 2);
        assert_eq!(encoder.take(), b"\x1b[1G");
        encoder.move_to(0, 2);
        assert_eq!(encoder.take(), b"");
    }

    #[test]
    fn a_diff_writes_only_changed_cells() {
        let mut before = Grid::new(20, 3);
        before.print(0, 0, "unchanged line", CellStyle::DEFAULT, before.bounds(), Width::default());
        let mut after = before.clone();
        after.print(0, 1, "new", CellStyle::DEFAULT, after.bounds(), Width::default());
        let mut encoder = Encoder::new();
        assert_eq!(write_diff(&mut encoder, &before, &after, ColorDepth::TrueColor), 3);
        let bytes = String::from_utf8(encoder.take()).unwrap();
        assert!(bytes.contains("new") && !bytes.contains("unchanged"), "{bytes:?}");
        assert_eq!(write_diff(&mut encoder, &after, &after, ColorDepth::TrueColor), 0);
        assert_eq!(encoder.take(), b"");
    }
}
