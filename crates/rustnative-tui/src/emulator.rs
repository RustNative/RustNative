//! A VT emulator: what a terminal makes of the bytes the backend writes.
//!
//! The synthetic terminal (`crate::harness`) does not trust the grid the
//! writer *meant* to produce; it runs the bytes actually written through
//! this emulator and asserts the screen *they* produce, so an encoding bug
//! (a wrong cursor move, a style that leaks, a wide character split) fails
//! a test the way it would fail on a person's terminal.
//!
//! It interprets what a modern xterm-compatible terminal does with
//! everything the backend writes, and answers the questions the backend
//! asks as a configured [`Answers`] says a terminal would: printing
//! (grapheme clusters, wide cells, autowrap), cursor motion (CUP, CUU/CUD/
//! CUF/CUB, CHA, VPA, save/restore), erasing (ED, EL, ECH), SGR (attributes,
//! 16/256/true colour), DEC private modes (tracked, with the alternate
//! screen and cursor visibility realized), the title stack, OSC 52 and
//! notification sequences (recorded), and DA1, DECRQM, DSR, the kitty
//! keyboard query, and OSC 11 (answered).

use std::collections::{BTreeMap, BTreeSet};

use rustnative_core::Color;

use crate::color::{ANSI_PALETTE, palette_256};
use crate::decode::base64_encode;
use crate::grid::{Attrs, Cell, CellStyle, Grid};
use crate::width::{Width, graphemes};

/// How the emulated terminal answers questions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answers {
    /// Its DA1 attributes, or `None` to answer nothing (a terminal that
    /// ignores the question).
    pub attributes: Option<Vec<u16>>,
    /// DECRQM: the modes it recognizes (answered set or reset as they are);
    /// `None` answers nothing at all.
    pub modes: Option<BTreeSet<u16>>,
    /// Whether it speaks the kitty keyboard protocol.
    pub kitty_keyboard: bool,
    /// Its default background, answered to OSC 11.
    pub background: Option<Color>,
    /// What it answers to an OSC 52 read, if it answers.
    pub clipboard_read: bool,
}

impl Answers {
    /// A modern terminal: DA1 with clipboard (52), every mode the backend
    /// uses recognized, no kitty protocol, a dark background.
    #[must_use]
    pub fn modern() -> Self {
        Self {
            attributes: Some(vec![62, 22, 52]),
            modes: Some(
                [1000, 1002, 1003, 1004, 1006, 1049, 2004, 2026, 25, 7].into_iter().collect(),
            ),
            kitty_keyboard: false,
            background: Some(Color::rgb(0, 0, 0)),
            clipboard_read: true,
        }
    }

    /// A terminal that answers nothing (conhost, a serial console).
    #[must_use]
    pub fn silent() -> Self {
        Self {
            attributes: None,
            modes: None,
            kitty_keyboard: false,
            background: None,
            clipboard_read: false,
        }
    }
}

/// The emulated terminal.
#[derive(Debug, Clone)]
pub struct Emulator {
    main: Grid,
    alternate: Grid,
    on_alternate: bool,
    cursor: (i32, i32),
    saved: (i32, i32),
    pending_wrap: bool,
    style: CellStyle,
    modes: BTreeSet<u16>,
    title: String,
    titles: Vec<String>,
    clipboard: Option<String>,
    notifications: Vec<String>,
    kitty: Vec<u16>,
    replies: Vec<u8>,
    pending: Vec<u8>,
    answers: Answers,
    width: Width,
    cursor_shape: u16,
}

impl Emulator {
    /// A terminal of `columns` × `rows` answering as `answers`.
    #[must_use]
    pub fn new(columns: u16, rows: u16, answers: Answers) -> Self {
        Self {
            main: Grid::new(columns, rows),
            alternate: Grid::new(columns, rows),
            on_alternate: false,
            cursor: (0, 0),
            saved: (0, 0),
            pending_wrap: false,
            style: CellStyle::DEFAULT,
            modes: [7, 25].into_iter().collect(),
            title: String::new(),
            titles: Vec::new(),
            clipboard: None,
            notifications: Vec::new(),
            kitty: Vec::new(),
            replies: Vec::new(),
            pending: Vec::new(),
            answers,
            width: Width::default(),
            cursor_shape: 0,
        }
    }

    /// The screen shown now (the alternate one while it is on).
    #[must_use]
    pub fn screen(&self) -> &Grid {
        if self.on_alternate { &self.alternate } else { &self.main }
    }

    /// The main screen, whatever is shown.
    #[must_use]
    pub fn main_screen(&self) -> &Grid {
        &self.main
    }

    /// Whether the alternate screen is shown.
    #[must_use]
    pub const fn on_alternate_screen(&self) -> bool {
        self.on_alternate
    }

    /// The cursor, 0-based.
    #[must_use]
    pub const fn cursor(&self) -> (i32, i32) {
        self.cursor
    }

    /// Whether DEC private mode `mode` is set.
    #[must_use]
    pub fn mode(&self, mode: u16) -> bool {
        self.modes.contains(&mode)
    }

    /// Every DEC private mode set.
    #[must_use]
    pub const fn modes(&self) -> &BTreeSet<u16> {
        &self.modes
    }

    /// The window title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The text last written to the clipboard (OSC 52).
    #[must_use]
    pub fn clipboard(&self) -> Option<&str> {
        self.clipboard.as_deref()
    }

    /// Sets the clipboard, as the person copying elsewhere would.
    pub fn set_clipboard(&mut self, text: &str) {
        self.clipboard = Some(text.to_owned());
    }

    /// The notifications posted (OSC 9, 99, 777), oldest first.
    #[must_use]
    pub fn notifications(&self) -> &[String] {
        &self.notifications
    }

    /// The kitty keyboard flags in force (0 when none were pushed).
    #[must_use]
    pub fn kitty_flags(&self) -> u16 {
        self.kitty.last().copied().unwrap_or(0)
    }

    /// The cursor shape last set (DECSCUSR; 0 is the terminal's own).
    #[must_use]
    pub const fn cursor_shape(&self) -> u16 {
        self.cursor_shape
    }

    /// Takes the bytes the terminal answered with, for the application's
    /// input.
    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    /// Resizes the terminal: both screens are cut or extended, keeping what
    /// fits (as most terminals do; the application redraws anyway).
    pub fn resize(&mut self, columns: u16, rows: u16) {
        for grid in [&mut self.main, &mut self.alternate] {
            let mut next = Grid::new(columns, rows);
            for y in 0..rows.min(grid.rows()) {
                for x in 0..columns.min(grid.columns()) {
                    if let Some(cell) = grid.get(i32::from(x), i32::from(y)) {
                        next.set(i32::from(x), i32::from(y), cell.clone());
                    }
                }
            }
            *grid = next;
        }
        self.cursor.0 = self.cursor.0.min(i32::from(columns).saturating_sub(1));
        self.cursor.1 = self.cursor.1.min(i32::from(rows).saturating_sub(1));
        self.pending_wrap = false;
    }

    fn grid(&mut self) -> &mut Grid {
        if self.on_alternate { &mut self.alternate } else { &mut self.main }
    }

    fn size(&self) -> (i32, i32) {
        let screen = self.screen();
        (i32::from(screen.columns()), i32::from(screen.rows()))
    }

    /// Interprets `bytes`.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        let data = std::mem::take(&mut self.pending);
        let mut at = 0;
        while at < data.len() {
            let byte = data[at];
            match byte {
                0x1B => {
                    if let Some(length) = self.escape(&data[at..]) {
                        at += length;
                    } else {
                        self.pending = data[at..].to_vec();
                        return;
                    }
                }
                b'\r' => {
                    self.cursor.0 = 0;
                    self.pending_wrap = false;
                    at += 1;
                }
                b'\n' | 0x0B | 0x0C => {
                    self.line_feed();
                    at += 1;
                }
                0x08 => {
                    self.cursor.0 = (self.cursor.0 - 1).max(0);
                    self.pending_wrap = false;
                    at += 1;
                }
                b'\t' => {
                    let (columns, _) = self.size();
                    self.cursor.0 = ((self.cursor.0 / 8 + 1) * 8).min(columns - 1);
                    at += 1;
                }
                0x00..=0x1F | 0x7F => at += 1,
                _ => {
                    // A run of printable text, printed by grapheme cluster.
                    let end = data[at..]
                        .iter()
                        .position(|b| *b < 0x20 || *b == 0x7F)
                        .map_or(data.len(), |offset| at + offset);
                    // Keep a UTF-8 sequence cut at the end for the next feed.
                    let run = &data[at..end];
                    let valid = match std::str::from_utf8(run) {
                        Ok(_) => run.len(),
                        Err(error) if error.error_len().is_none() && end == data.len() => {
                            error.valid_up_to()
                        }
                        Err(error) => error.valid_up_to().max(1),
                    };
                    let text = String::from_utf8_lossy(&run[..valid]).into_owned();
                    self.print(&text);
                    at += valid;
                    if valid < run.len()
                        && end == data.len()
                        && std::str::from_utf8(&run[valid..]).is_err()
                    {
                        self.pending = data[at..].to_vec();
                        return;
                    }
                }
            }
        }
    }

    fn line_feed(&mut self) {
        let (_, rows) = self.size();
        self.pending_wrap = false;
        if self.cursor.1 + 1 >= rows {
            let style = CellStyle { attrs: Attrs::NONE, ..self.style };
            self.grid().scroll_up(style);
        } else {
            self.cursor.1 += 1;
        }
    }

    fn print(&mut self, text: &str) {
        for grapheme in graphemes(text) {
            let cells = self.width.of_grapheme(grapheme);
            let (columns, _) = self.size();
            if cells == 0 {
                let (x, y) = self.cursor;
                let x = if self.pending_wrap { x } else { x - 1 };
                self.grid().append(x.max(0), y, grapheme);
                continue;
            }
            let cells = i32::try_from(cells.min(2)).unwrap_or(1);
            if self.pending_wrap || self.cursor.0 + cells > columns {
                if self.modes.contains(&7) {
                    self.cursor.0 = 0;
                    self.line_feed();
                } else {
                    self.cursor.0 = (columns - cells).max(0);
                }
                self.pending_wrap = false;
            }
            let (x, y) = self.cursor;
            let style = self.style;
            let clip = self.screen().bounds();
            self.grid().put(x, y, grapheme, u8::try_from(cells).unwrap_or(1), style, clip);
            if x + cells >= columns {
                self.cursor.0 = columns - 1;
                self.pending_wrap = self.modes.contains(&7);
            } else {
                self.cursor.0 = x + cells;
            }
        }
    }

    /// One escape sequence at the start of `data`; `None` when it is not
    /// complete yet.
    fn escape(&mut self, data: &[u8]) -> Option<usize> {
        let next = *data.get(1)?;
        match next {
            b'[' => {
                let mut index = 2;
                while index < data.len() && (0x20..=0x3F).contains(&data[index]) {
                    index += 1;
                }
                let final_byte = *data.get(index)?;
                self.csi(&data[2..index], final_byte);
                Some(index + 1)
            }
            b']' | b'P' | b'_' | b'^' => {
                let mut index = 2;
                loop {
                    let byte = *data.get(index)?;
                    if byte == 0x07 {
                        if next == b']' {
                            self.osc(&data[2..index]);
                        }
                        return Some(index + 1);
                    }
                    if byte == 0x1B {
                        let after = *data.get(index + 1)?;
                        if after == b'\\' {
                            if next == b']' {
                                self.osc(&data[2..index]);
                            }
                            return Some(index + 2);
                        }
                        return Some(index);
                    }
                    index += 1;
                }
            }
            b'7' => {
                self.saved = self.cursor;
                Some(2)
            }
            b'8' => {
                self.cursor = self.saved;
                Some(2)
            }
            b'c' => {
                let (columns, rows) = (self.screen().columns(), self.screen().rows());
                *self = Self::new(columns, rows, self.answers.clone());
                Some(2)
            }
            b'(' | b')' | b'#' => data.get(2).map(|_| 3),
            _ => Some(2),
        }
    }

    fn osc(&mut self, body: &[u8]) {
        let text = String::from_utf8_lossy(body).into_owned();
        let (code, rest) = text.split_once(';').unwrap_or((text.as_str(), ""));
        match code {
            "0" | "2" => rest.clone_into(&mut self.title),
            "52" => {
                let data = rest.split_once(';').map_or("", |(_, data)| data);
                if data == "?" {
                    if self.answers.clipboard_read {
                        let encoded =
                            base64_encode(self.clipboard.clone().unwrap_or_default().as_bytes());
                        self.replies
                            .extend_from_slice(format!("\x1b]52;c;{encoded}\x07").as_bytes());
                    }
                } else if let Some(bytes) = crate::decode::base64_decode(data) {
                    self.clipboard = Some(String::from_utf8_lossy(&bytes).into_owned());
                }
            }
            "9" | "777" | "99" => self.notifications.push(rest.to_owned()),
            "11" if rest == "?" => {
                if let Some(color) = self.answers.background {
                    let reply = format!(
                        "\x1b]11;rgb:{0:02x}{0:02x}/{1:02x}{1:02x}/{2:02x}{2:02x}\x1b\\",
                        color.red, color.green, color.blue
                    );
                    self.replies.extend_from_slice(reply.as_bytes());
                }
            }
            _ => {}
        }
    }

    fn set_mode(&mut self, mode: u16, on: bool) {
        match (mode, on) {
            (1049 | 1047 | 47, true) if !self.on_alternate => {
                if mode == 1049 {
                    self.saved = self.cursor;
                }
                self.on_alternate = true;
                self.alternate.clear(CellStyle::DEFAULT);
            }
            (1049 | 1047 | 47, false) if self.on_alternate => {
                self.on_alternate = false;
                if mode == 1049 {
                    self.cursor = self.saved;
                }
            }
            _ => {}
        }
        if on {
            self.modes.insert(mode);
        } else {
            self.modes.remove(&mode);
        }
    }

    #[allow(clippy::too_many_lines, reason = "one match over every CSI final the backend uses")]
    fn csi(&mut self, body: &[u8], final_byte: u8) {
        let (private, body) = match body.first() {
            Some(marker @ (b'?' | b'<' | b'>' | b'=')) => (Some(*marker), &body[1..]),
            _ => (None, body),
        };
        let intermediates: Vec<u8> =
            body.iter().copied().filter(|b| (0x20..=0x2F).contains(b)).collect();
        let raw: String =
            body.iter().copied().filter(|b| (0x30..=0x3F).contains(b)).map(char::from).collect();
        let params: Vec<u16> = if raw.is_empty() {
            Vec::new()
        } else {
            raw.split(';').map(|p| p.parse().unwrap_or(0)).collect()
        };
        let param = |index: usize, default: u16| {
            params.get(index).copied().filter(|v| *v != 0).unwrap_or(default)
        };
        let (columns, rows) = self.size();
        match (private, intermediates.as_slice(), final_byte) {
            (None, [], b'H' | b'f') => {
                self.cursor = (
                    (i32::from(param(1, 1)) - 1).clamp(0, columns - 1),
                    (i32::from(param(0, 1)) - 1).clamp(0, rows - 1),
                );
                self.pending_wrap = false;
            }
            (None, [], b'A') => self.cursor.1 = (self.cursor.1 - i32::from(param(0, 1))).max(0),
            (None, [], b'B') => {
                self.cursor.1 = (self.cursor.1 + i32::from(param(0, 1))).min(rows - 1);
            }
            (None, [], b'C') => {
                self.cursor.0 = (self.cursor.0 + i32::from(param(0, 1))).min(columns - 1);
                self.pending_wrap = false;
            }
            (None, [], b'D') => {
                self.cursor.0 = (self.cursor.0 - i32::from(param(0, 1))).max(0);
                self.pending_wrap = false;
            }
            (None, [], b'G') => {
                self.cursor.0 = (i32::from(param(0, 1)) - 1).clamp(0, columns - 1);
                self.pending_wrap = false;
            }
            (None, [], b'd') => self.cursor.1 = (i32::from(param(0, 1)) - 1).clamp(0, rows - 1),
            (None, [], b'J') => {
                let style = CellStyle { attrs: Attrs::NONE, ..self.style };
                let (x, y) = self.cursor;
                let grid = self.grid();
                let bounds = grid.bounds();
                match params.first().copied().unwrap_or(0) {
                    0 => {
                        grid.fill(rustnative_drawlist::GridRect::new(x, y, columns - x, 1), style);
                        grid.fill(
                            rustnative_drawlist::GridRect::new(0, y + 1, columns, rows - y - 1),
                            style,
                        );
                    }
                    1 => {
                        grid.fill(rustnative_drawlist::GridRect::new(0, 0, columns, y), style);
                        grid.fill(rustnative_drawlist::GridRect::new(0, y, x + 1, 1), style);
                    }
                    _ => grid.fill(bounds, style),
                }
            }
            (None, [], b'K') => {
                let style = CellStyle { attrs: Attrs::NONE, ..self.style };
                let (x, y) = self.cursor;
                let rect = match params.first().copied().unwrap_or(0) {
                    0 => rustnative_drawlist::GridRect::new(x, y, columns - x, 1),
                    1 => rustnative_drawlist::GridRect::new(0, y, x + 1, 1),
                    _ => rustnative_drawlist::GridRect::new(0, y, columns, 1),
                };
                self.grid().fill(rect, style);
            }
            (None, [], b'X') => {
                let style = CellStyle { attrs: Attrs::NONE, ..self.style };
                let (x, y) = self.cursor;
                let rect = rustnative_drawlist::GridRect::new(x, y, i32::from(param(0, 1)), 1);
                self.grid().fill(rect, style);
            }
            (None, [], b'm') => self.sgr(&raw),
            (Some(b'?'), [], b'h') => {
                for mode in &params {
                    self.set_mode(*mode, true);
                }
            }
            (Some(b'?'), [], b'l') => {
                for mode in &params {
                    self.set_mode(*mode, false);
                }
            }
            (None, [], b's') => self.saved = self.cursor,
            (None, [], b'u') => self.cursor = self.saved,
            (None, [], b'c') => {
                if let Some(attributes) = &self.answers.attributes {
                    let list: Vec<String> = attributes.iter().map(u16::to_string).collect();
                    self.replies.extend_from_slice(format!("\x1b[?{}c", list.join(";")).as_bytes());
                }
            }
            (None, [], b'n') if params.first() == Some(&6) => {
                let reply = format!("\x1b[{};{}R", self.cursor.1 + 1, self.cursor.0 + 1);
                self.replies.extend_from_slice(reply.as_bytes());
            }
            (None, [], b't') => match params.first() {
                Some(22) => self.titles.push(self.title.clone()),
                Some(23) => {
                    if let Some(title) = self.titles.pop() {
                        self.title = title;
                    }
                }
                _ => {}
            },
            (None, [b' '], b'q') => self.cursor_shape = params.first().copied().unwrap_or(0),
            (Some(b'?'), [b'$'], b'p') => {
                if let Some(modes) = &self.answers.modes {
                    let mode = params.first().copied().unwrap_or(0);
                    let state = if !modes.contains(&mode) {
                        0
                    } else if self.modes.contains(&mode) {
                        1
                    } else {
                        2
                    };
                    self.replies.extend_from_slice(format!("\x1b[?{mode};{state}$y").as_bytes());
                }
            }
            (Some(b'?'), [], b'u') => {
                if self.answers.kitty_keyboard {
                    let reply = format!("\x1b[?{}u", self.kitty_flags());
                    self.replies.extend_from_slice(reply.as_bytes());
                }
            }
            (Some(b'>'), [], b'u') => {
                if self.answers.kitty_keyboard {
                    self.kitty.push(params.first().copied().unwrap_or(0));
                }
            }
            (Some(b'<'), [], b'u') => {
                let count = usize::from(param(0, 1));
                for _ in 0..count {
                    self.kitty.pop();
                }
            }
            _ => {}
        }
    }

    fn sgr(&mut self, raw: &str) {
        let values: Vec<u16> = if raw.is_empty() {
            vec![0]
        } else {
            raw.split([';', ':']).map(|p| p.parse().unwrap_or(0)).collect()
        };
        let mut index = 0;
        let color_at = |values: &[u16], index: &mut usize| -> Option<Color> {
            match values.get(*index + 1) {
                Some(5) => {
                    let n = u8::try_from(*values.get(*index + 2)?).ok()?;
                    *index += 2;
                    let (r, g, b) = palette_256(n);
                    Some(Color::rgb(r, g, b))
                }
                Some(2) => {
                    let channel = |offset: usize| u8::try_from(*values.get(*index + offset)?).ok();
                    let color = Color::rgb(channel(2)?, channel(3)?, channel(4)?);
                    *index += 4;
                    Some(color)
                }
                _ => None,
            }
        };
        let ansi = |n: u16| {
            let (r, g, b) = ANSI_PALETTE[usize::from(n.min(15))];
            Color::rgb(r, g, b)
        };
        while index < values.len() {
            let value = values[index];
            let style = &mut self.style;
            match value {
                0 => *style = CellStyle::DEFAULT,
                1 => style.attrs = style.attrs.with(Attrs::BOLD),
                2 => style.attrs = style.attrs.with(Attrs::DIM),
                3 => style.attrs = style.attrs.with(Attrs::ITALIC),
                4 => style.attrs = style.attrs.with(Attrs::UNDERLINE),
                7 => style.attrs = style.attrs.with(Attrs::REVERSE),
                9 => style.attrs = style.attrs.with(Attrs::STRIKE),
                22 => style.attrs = style.attrs.without(Attrs::BOLD).without(Attrs::DIM),
                23 => style.attrs = style.attrs.without(Attrs::ITALIC),
                24 => style.attrs = style.attrs.without(Attrs::UNDERLINE),
                27 => style.attrs = style.attrs.without(Attrs::REVERSE),
                29 => style.attrs = style.attrs.without(Attrs::STRIKE),
                30..=37 => style.fg = Some(ansi(value - 30)),
                90..=97 => style.fg = Some(ansi(value - 90 + 8)),
                40..=47 => style.bg = Some(ansi(value - 40)),
                100..=107 => style.bg = Some(ansi(value - 100 + 8)),
                39 => style.fg = None,
                49 => style.bg = None,
                38 => {
                    let color = color_at(&values, &mut index);
                    self.style.fg = color.or(self.style.fg);
                }
                48 => {
                    let color = color_at(&values, &mut index);
                    self.style.bg = color.or(self.style.bg);
                }
                _ => {}
            }
            index += 1;
        }
    }

    /// Each cell's style, by position, where it differs from the default:
    /// the attribute half of a golden.
    #[must_use]
    pub fn styles(&self) -> BTreeMap<(u16, u16), CellStyle> {
        let screen = self.screen();
        let mut out = BTreeMap::new();
        for y in 0..screen.rows() {
            for (x, cell) in screen.row(y).iter().enumerate() {
                if cell.style != CellStyle::DEFAULT {
                    out.insert((u16::try_from(x).unwrap_or(0), y), cell.style);
                }
            }
        }
        out
    }

    /// The cell at (`x`, `y`) of the shown screen.
    #[must_use]
    pub fn cell(&self, x: i32, y: i32) -> Option<&Cell> {
        self.screen().get(x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorDepth;
    use crate::encode::{Encoder, seq, write_diff, write_full};

    #[test]
    fn printing_wraps_and_scrolls() {
        let mut terminal = Emulator::new(4, 2, Answers::silent());
        terminal.feed(b"abcdefghij");
        assert_eq!(terminal.screen().text(), "efgh\nij");
        terminal.feed(b"\x1b[?7l\x1b[1;1Hxyzw!");
        assert_eq!(
            terminal.screen().row_text(0),
            "xyz!",
            "without autowrap the last column is overwritten"
        );
    }

    #[test]
    fn the_alternate_screen_keeps_the_main_one() {
        let mut terminal = Emulator::new(10, 2, Answers::silent());
        terminal.feed(b"shell$ ");
        terminal.feed(seq::ENTER_ALT_SCREEN.as_bytes());
        terminal.feed(b"\x1b[2;1Happ");
        assert_eq!(terminal.screen().text(), "\napp");
        terminal.feed(seq::LEAVE_ALT_SCREEN.as_bytes());
        assert_eq!(terminal.screen().text(), "shell$");
        assert_eq!(terminal.cursor(), (7, 0));
    }

    #[test]
    fn questions_are_answered_as_configured() {
        let mut terminal = Emulator::new(10, 2, Answers::modern());
        terminal.feed(b"\x1b[c\x1b[?2004$p\x1b[?9999$p");
        assert_eq!(terminal.take_replies(), b"\x1b[?62;22;52c\x1b[?2004;2$y\x1b[?9999;0$y");
        let mut silent = Emulator::new(10, 2, Answers::silent());
        silent.feed(b"\x1b[c\x1b[?2004$p\x1b[?u");
        assert_eq!(silent.take_replies(), b"");
    }

    /// The writer's grid and the emulator's screen agree, cell for cell,
    /// through every frame of a scripted sequence of changes.
    #[test]
    fn what_the_writer_means_is_what_the_terminal_shows() {
        type Frame<'a> = Box<dyn Fn(&mut Grid) + 'a>;
        let width = Width::default();
        let mut terminal = Emulator::new(30, 6, Answers::silent());
        terminal.feed(seq::AUTOWRAP_OFF.as_bytes());
        let mut encoder = Encoder::new();
        let mut shown = Grid::new(30, 6);
        let red = CellStyle::DEFAULT.fg(Some(Color::rgb(200, 10, 10))).attrs(Attrs::BOLD);
        let frames: Vec<Frame<'_>> = vec![
            Box::new(|grid| {
                grid.print(0, 0, "hello 日本 world", CellStyle::DEFAULT, grid.bounds(), width);
            }),
            Box::new(move |grid| {
                grid.print(6, 0, "ab", red, grid.bounds(), width);
            }),
            Box::new(|grid| {
                grid.print(
                    25,
                    5,
                    "edge!",
                    CellStyle::DEFAULT.attrs(Attrs::REVERSE),
                    grid.bounds(),
                    width,
                );
            }),
            Box::new(|grid| {
                grid.print(28, 4, "日", CellStyle::DEFAULT, grid.bounds(), width);
                grid.print(0, 3, "e\u{301}👩‍👩‍👧x", CellStyle::DEFAULT, grid.bounds(), width);
            }),
            Box::new(|grid| grid.fill(grid.bounds(), CellStyle::DEFAULT)),
        ];
        let mut first = true;
        for frame in frames {
            let mut next = shown.clone();
            frame(&mut next);
            if first {
                write_full(&mut encoder, &next, ColorDepth::TrueColor);
                first = false;
            } else {
                write_diff(&mut encoder, &shown, &next, ColorDepth::TrueColor);
            }
            terminal.feed(&encoder.take());
            assert_eq!(terminal.screen(), &next, "after a frame:\n{}", next.text());
            shown = next;
        }
    }
}
