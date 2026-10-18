//! Text editing for drawn fields (`PLAN.md` Milestone 38: "text inputs (a
//! caret, selection, clipboard …)"; Milestone 61, the host's text editing
//! surface).
//!
//! A terminal has no edit control, so the backend is the host's: the caret
//! and the selection move by grapheme cluster (never inside one), by word
//! with Ctrl, and to the line's or the text's ends with Home and End (Ctrl
//! for the text). Shift extends the selection; typing, pasting, Backspace,
//! and Delete replace it. Ctrl+A selects everything; Ctrl+C copies,
//! Ctrl+X cuts, and Ctrl+V pastes. A multi-line field soft-wraps at its
//! width and moves by visual line with Up and Down, keeping the column it
//! started from.
//!
//! The value is the component's: an edit produces the new value, which the
//! session reports as `Event::TextChanged`, exactly as a native edit
//! control reports its change.

use unicode_segmentation::UnicodeSegmentation;

use rustnative_core::KeyCode;

use crate::decode::Key;
use crate::width::Width;

/// A field's caret, selection anchor, and scroll position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Edit {
    /// The caret, as a byte offset at a cluster boundary.
    pub caret: usize,
    /// The other end of the selection (equal to `caret` when nothing is
    /// selected).
    pub anchor: usize,
    /// The first column (one line) or visual line (several) shown.
    pub scroll: usize,
    /// The column Up and Down aim for.
    pub goal: Option<usize>,
}

/// What a key did to a field.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outcome {
    /// Whether the field used the key (otherwise it goes on to the host's
    /// other handling).
    pub handled: bool,
    /// The new value, when it changed.
    pub value: Option<String>,
    /// Text to put on the clipboard.
    pub copied: Option<String>,
    /// Whether the field asked to paste from the clipboard.
    pub paste: bool,
}

impl Edit {
    /// At the end of `value`, nothing selected.
    #[must_use]
    pub fn at_end(value: &str) -> Self {
        Self { caret: value.len(), anchor: value.len(), scroll: 0, goal: None }
    }

    /// The selected byte range (empty when nothing is selected).
    #[must_use]
    pub fn selection(&self) -> std::ops::Range<usize> {
        self.caret.min(self.anchor)..self.caret.max(self.anchor)
    }

    /// Keeps the caret and anchor inside `value` and on cluster boundaries
    /// (after the component rendered a different value).
    pub fn clamp(&mut self, value: &str) {
        let snap = |offset: usize| {
            let offset = offset.min(value.len());
            value
                .grapheme_indices(true)
                .map(|(index, _)| index)
                .chain(std::iter::once(value.len()))
                .rfind(|index| *index <= offset)
                .unwrap_or(0)
        };
        self.caret = snap(self.caret);
        self.anchor = snap(self.anchor);
    }
}

fn previous_boundary(value: &str, offset: usize) -> usize {
    value
        .grapheme_indices(true)
        .map(|(index, _)| index)
        .take_while(|index| *index < offset)
        .last()
        .unwrap_or(0)
}

fn next_boundary(value: &str, offset: usize) -> usize {
    value
        .grapheme_indices(true)
        .map(|(index, grapheme)| index + grapheme.len())
        .find(|end| *end > offset)
        .unwrap_or(value.len())
}

fn is_word(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

fn previous_word(value: &str, offset: usize) -> usize {
    value
        .split_word_bound_indices()
        .filter(|(index, word)| *index < offset && is_word(word))
        .map(|(index, _)| index)
        .next_back()
        .unwrap_or(0)
}

fn next_word(value: &str, offset: usize) -> usize {
    value
        .split_word_bound_indices()
        .map(|(index, word)| (index + word.len(), word))
        .find(|(end, word)| *end > offset && is_word(word))
        .map_or(value.len(), |(end, _)| end)
}

/// One visual line of a field: a byte range of the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    /// Its first byte.
    pub start: usize,
    /// One past its last byte (before the line break, if any).
    pub end: usize,
}

/// The value broken into visual lines of at most `columns` cells: at line
/// breaks, and (when `wrap`) wherever a line is full, between clusters.
#[must_use]
pub fn visual_lines(value: &str, columns: usize, width: Width, wrap: bool) -> Vec<Line> {
    let columns = columns.max(1);
    let mut lines = Vec::new();
    let mut start = 0;
    let mut used = 0;
    for (index, grapheme) in value.grapheme_indices(true) {
        if grapheme == "\n" || grapheme == "\r\n" {
            lines.push(Line { start, end: index });
            start = index + grapheme.len();
            used = 0;
            continue;
        }
        let cells = width.of_grapheme(grapheme);
        if wrap && used + cells > columns && used > 0 {
            lines.push(Line { start, end: index });
            start = index;
            used = 0;
        }
        used += cells;
    }
    lines.push(Line { start, end: value.len() });
    lines
}

/// Where `offset` is: (column, visual line).
#[must_use]
pub fn position(value: &str, offset: usize, lines: &[Line], width: Width) -> (usize, usize) {
    let row = lines.iter().rposition(|line| line.start <= offset).unwrap_or(0);
    let Some(line) = lines.get(row) else { return (0, 0) };
    // `offset` should be a cluster boundary; one inside a character counts
    // from where that character starts.
    let end = offset.min(line.end).max(line.start);
    let end = (line.start..=end).rev().find(|&index| value.is_char_boundary(index)).unwrap_or(end);
    (value.get(line.start..end).map_or(0, |text| width.of(text)), row)
}

/// The offset nearest `column` on visual line `row`.
#[must_use]
pub fn offset_at(value: &str, lines: &[Line], column: usize, row: usize, width: Width) -> usize {
    let Some(line) = lines.get(row.min(lines.len().saturating_sub(1))) else { return 0 };
    let mut used = 0;
    let text = value.get(line.start..line.end).unwrap_or_default();
    for (index, grapheme) in text.grapheme_indices(true) {
        let cells = width.of_grapheme(grapheme);
        if used + cells.div_ceil(2) > column {
            return line.start + index;
        }
        used += cells;
    }
    line.end
}

#[allow(clippy::string_slice, reason = "the selection is clamped to cluster boundaries first")]
fn replace_selection(value: &str, edit: &mut Edit, text: &str) -> String {
    // `Edit`'s fields are public: an offset inside a character must not
    // reach the slicing below.
    edit.clamp(value);
    let range = edit.selection();
    let mut next = String::with_capacity(value.len() + text.len());
    next.push_str(&value[..range.start]);
    next.push_str(text);
    next.push_str(&value[range.end..]);
    edit.caret = range.start + text.len();
    edit.anchor = edit.caret;
    edit.goal = None;
    next
}

/// Inserts `text` at the caret, replacing the selection; a one-line field
/// takes line breaks as spaces.
#[must_use]
pub fn insert(value: &str, edit: &mut Edit, text: &str, multiline: bool) -> String {
    let text = if multiline {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.replace("\r\n", " ").replace(['\r', '\n'], " ")
    };
    let text: String = text
        .chars()
        .filter(|c| !c.is_control() || (multiline && *c == '\n') || *c == '\t')
        .collect();
    replace_selection(value, edit, &text)
}

/// Applies `key` to a field holding `value`, `columns` cells wide.
#[allow(clippy::too_many_lines, reason = "one match over the editing keys")]
#[must_use]
pub fn key(
    value: &str,
    edit: &mut Edit,
    key: &Key,
    multiline: bool,
    columns: usize,
    width: Width,
) -> Outcome {
    edit.clamp(value);
    let modifiers = key.modifiers;
    let extend = modifiers.shift;
    let mut outcome = Outcome { handled: true, ..Outcome::default() };
    let move_to = |edit: &mut Edit, target: usize, keep_goal: bool| {
        edit.caret = target;
        if !extend {
            edit.anchor = target;
        }
        if !keep_goal {
            edit.goal = None;
        }
    };
    let lines = visual_lines(value, columns, width, multiline);
    match key.code {
        KeyCode::Character('a') if modifiers.ctrl && !modifiers.alt => {
            edit.anchor = 0;
            edit.caret = value.len();
        }
        KeyCode::Character('c') if modifiers.ctrl && !modifiers.alt => {
            let range = edit.selection();
            if range.is_empty() {
                outcome.handled = false;
            } else {
                outcome.copied = value.get(range).map(str::to_owned);
            }
        }
        KeyCode::Character('x') if modifiers.ctrl && !modifiers.alt => {
            let range = edit.selection();
            if !range.is_empty() {
                outcome.copied = value.get(range).map(str::to_owned);
                outcome.value = Some(replace_selection(value, edit, ""));
            }
        }
        KeyCode::Character('v') if modifiers.ctrl && !modifiers.alt => outcome.paste = true,
        KeyCode::ArrowLeft => {
            let range = edit.selection();
            let target = if !extend && !range.is_empty() {
                range.start
            } else if modifiers.ctrl {
                previous_word(value, edit.caret)
            } else {
                previous_boundary(value, edit.caret)
            };
            move_to(edit, target, false);
        }
        KeyCode::ArrowRight => {
            let range = edit.selection();
            let target = if !extend && !range.is_empty() {
                range.end
            } else if modifiers.ctrl {
                next_word(value, edit.caret)
            } else {
                next_boundary(value, edit.caret)
            };
            move_to(edit, target, false);
        }
        KeyCode::Home => {
            let (_, row) = position(value, edit.caret, &lines, width);
            let target = if modifiers.ctrl || !multiline { 0 } else { lines[row].start };
            move_to(edit, target, false);
        }
        KeyCode::End => {
            let (_, row) = position(value, edit.caret, &lines, width);
            let target = if modifiers.ctrl || !multiline { value.len() } else { lines[row].end };
            move_to(edit, target, false);
        }
        KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::PageUp | KeyCode::PageDown
            if multiline =>
        {
            let (column, row) = position(value, edit.caret, &lines, width);
            let goal = edit.goal.unwrap_or(column);
            let step = if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) { 5 } else { 1 };
            let target_row = if matches!(key.code, KeyCode::ArrowUp | KeyCode::PageUp) {
                row.checked_sub(step).unwrap_or(usize::MAX)
            } else {
                row + step
            };
            let target = if target_row == usize::MAX {
                0
            } else if target_row >= lines.len() {
                value.len()
            } else {
                offset_at(value, &lines, goal, target_row, width)
            };
            move_to(edit, target, true);
            edit.goal = Some(goal);
        }
        KeyCode::Backspace => {
            if edit.selection().is_empty() {
                edit.anchor = if modifiers.ctrl || modifiers.alt {
                    previous_word(value, edit.caret)
                } else {
                    previous_boundary(value, edit.caret)
                };
            }
            if edit.selection().is_empty() {
                return outcome;
            }
            outcome.value = Some(replace_selection(value, edit, ""));
        }
        KeyCode::Delete => {
            if edit.selection().is_empty() {
                edit.anchor = if modifiers.ctrl {
                    next_word(value, edit.caret)
                } else {
                    next_boundary(value, edit.caret)
                };
            }
            if edit.selection().is_empty() {
                return outcome;
            }
            outcome.value = Some(replace_selection(value, edit, ""));
        }
        KeyCode::Enter if multiline && !modifiers.ctrl && !modifiers.alt => {
            outcome.value = Some(replace_selection(value, edit, "\n"));
        }
        _ => match &key.text {
            Some(text) if !modifiers.ctrl && !modifiers.alt && !modifiers.meta && text != "\t" => {
                outcome.value = Some(insert(value, edit, text, multiline));
            }
            _ => outcome.handled = false,
        },
    }
    outcome
}

/// Scrolls so the caret is visible in a field `columns` wide (one line)
/// or `rows` high (several), and returns the caret's cell in the field.
pub fn reveal(
    value: &str,
    edit: &mut Edit,
    columns: usize,
    rows: usize,
    multiline: bool,
    width: Width,
) -> (usize, usize) {
    let lines = visual_lines(value, columns, width, multiline);
    let (column, row) = position(value, edit.caret, &lines, width);
    if multiline {
        let rows = rows.max(1);
        if row < edit.scroll {
            edit.scroll = row;
        } else if row >= edit.scroll + rows {
            edit.scroll = row + 1 - rows;
        }
        (column.min(columns.saturating_sub(1)), row - edit.scroll)
    } else {
        let columns = columns.max(1);
        if column < edit.scroll {
            edit.scroll = column;
        } else if column >= edit.scroll + columns {
            edit.scroll = column + 1 - columns;
        }
        (column - edit.scroll, 0)
    }
}

#[cfg(test)]
mod tests {
    use rustnative_core::KeyModifiers;

    use super::*;

    fn press(code: KeyCode, ctrl: bool, shift: bool) -> Key {
        Key::new(code).with(KeyModifiers { ctrl, shift, ..KeyModifiers::default() })
    }

    fn typed(text: &str) -> Key {
        Key {
            text: Some(text.to_owned()),
            ..Key::new(KeyCode::Character(text.chars().next().unwrap_or(' ')))
        }
    }

    fn run(value: &str, edit: &mut Edit, keys: &[Key], multiline: bool) -> String {
        let mut value = value.to_owned();
        for key in keys {
            if let Some(next) = super::key(&value, edit, key, multiline, 20, Width::default()).value
            {
                value = next;
            }
        }
        value
    }

    #[test]
    fn typing_and_deleting_move_by_cluster() {
        let mut edit = Edit::at_end("");
        let value =
            run("", &mut edit, &[typed("e"), typed("\u{301}"), typed("🙂"), typed("x")], false);
        assert_eq!(value, "e\u{301}🙂x");
        let value = run(&value, &mut edit, &[press(KeyCode::Backspace, false, false)], false);
        assert_eq!(value, "e\u{301}🙂");
        let value = run(
            &value,
            &mut edit,
            &[press(KeyCode::ArrowLeft, false, false), press(KeyCode::Backspace, false, false)],
            false,
        );
        assert_eq!(value, "🙂", "the base and its mark go together");
    }

    #[test]
    fn selection_extends_and_is_replaced() {
        let mut edit = Edit::at_end("hello world");
        let value = run(
            "hello world",
            &mut edit,
            &[press(KeyCode::ArrowLeft, true, true), typed("there")],
            false,
        );
        assert_eq!(value, "hello there");
        let mut edit = Edit::at_end(&value);
        let outcome = key(
            &value,
            &mut edit,
            &press(KeyCode::Character('a'), true, false),
            false,
            20,
            Width::default(),
        );
        assert!(outcome.handled);
        let outcome = key(
            &value,
            &mut edit,
            &press(KeyCode::Character('x'), true, false),
            false,
            20,
            Width::default(),
        );
        assert_eq!(outcome.copied.as_deref(), Some("hello there"));
        assert_eq!(outcome.value.as_deref(), Some(""));
    }

    #[test]
    fn word_motion_and_deletion() {
        let mut edit = Edit::at_end("one two  three");
        let value =
            run("one two  three", &mut edit, &[press(KeyCode::Backspace, true, false)], false);
        assert_eq!(value, "one two  ");
        let mut edit = Edit { caret: 0, anchor: 0, ..Edit::default() };
        let _ = key(
            &value,
            &mut edit,
            &press(KeyCode::ArrowRight, true, false),
            false,
            20,
            Width::default(),
        );
        assert_eq!(edit.caret, 3);
    }

    #[test]
    fn an_offset_inside_a_character_is_snapped_not_sliced() {
        // `Edit`'s fields are public; byte 2 is inside the `€`.
        let value = "a€b";
        let mut edit = Edit { caret: 2, anchor: 2, scroll: 0, goal: None };
        assert_eq!(insert(value, &mut edit, "x", false), "ax€b");
        let lines = visual_lines(value, 20, Width::default(), false);
        assert_eq!(position(value, 2, &lines, Width::default()), (1, 0));
    }

    #[test]
    fn a_one_line_field_takes_breaks_as_spaces_and_scrolls_to_its_caret() {
        let mut edit = Edit::at_end("");
        let value = insert("", &mut edit, "a\nb", false);
        assert_eq!(value, "a b");
        let long = "abcdefghijklmnopqrstuvwxyz";
        let mut edit = Edit::at_end(long);
        assert_eq!(reveal(long, &mut edit, 10, 1, false, Width::default()), (9, 0));
        assert_eq!(edit.scroll, 17);
        edit.caret = 0;
        assert_eq!(reveal(long, &mut edit, 10, 1, false, Width::default()), (0, 0));
        assert_eq!(edit.scroll, 0);
    }

    #[test]
    fn several_lines_wrap_and_move_by_visual_line() {
        let value = "first line\nsecond";
        let lines = visual_lines(value, 6, Width::default(), true);
        assert_eq!(lines.len(), 3, "{lines:?}");
        let mut edit = Edit { caret: 2, anchor: 2, ..Edit::default() };
        let _ = key(
            value,
            &mut edit,
            &press(KeyCode::ArrowDown, false, false),
            true,
            6,
            Width::default(),
        );
        assert_eq!(position(value, edit.caret, &lines, Width::default()), (2, 1));
        let _ = key(
            value,
            &mut edit,
            &press(KeyCode::ArrowDown, false, false),
            true,
            6,
            Width::default(),
        );
        assert_eq!(position(value, edit.caret, &lines, Width::default()), (2, 2));
        let next = run(value, &mut edit, &[press(KeyCode::Enter, false, false)], true);
        assert_eq!(next, "first line\nse\ncond");
    }
}
