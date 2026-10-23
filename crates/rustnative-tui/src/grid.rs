//! The cell grid: this backend's host object (`PLAN.md` 2.2 and Milestone
//! 38, "the native host object is the terminal's own cell grid").
//!
//! Every frame is painted into a [`Grid`], and what reaches the terminal is
//! the difference between it and the grid already on screen
//! ([`crate::encode::write_diff`]). A cell holds one grapheme cluster and
//! its [`CellStyle`]; a wide cluster takes two cells, the second a
//! continuation (width 0) that is never written on its own.

use rustnative_core::Color;
use rustnative_drawlist::GridRect;

use crate::width::{Width, graphemes};

/// Text attributes, as SGR sets them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Attrs(u8);

impl Attrs {
    /// None.
    pub const NONE: Self = Self(0);
    /// Bold (SGR 1).
    pub const BOLD: Self = Self(1);
    /// Faint (SGR 2).
    pub const DIM: Self = Self(2);
    /// Italic (SGR 3).
    pub const ITALIC: Self = Self(4);
    /// Underlined (SGR 4).
    pub const UNDERLINE: Self = Self(8);
    /// Foreground and background swapped (SGR 7).
    pub const REVERSE: Self = Self(16);
    /// Struck through (SGR 9).
    pub const STRIKE: Self = Self(32);

    /// Both sets.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Without `other`'s.
    #[must_use]
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Whether every attribute of `other` is set.
    #[must_use]
    pub const fn has(self, other: Self) -> bool {
        other.0 != 0 && self.0 & other.0 == other.0
    }

    /// Whether every attribute set here is set in `other` too.
    #[must_use]
    pub const fn is_subset_of(self, other: Self) -> bool {
        self.0 & !other.0 == 0
    }

    /// The raw bits.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }
}

/// How a cell is drawn. `None` colours are the terminal's own defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CellStyle {
    /// The foreground, or the terminal's default.
    pub fg: Option<Color>,
    /// The background, or the terminal's default.
    pub bg: Option<Color>,
    /// The attributes.
    pub attrs: Attrs,
}

impl CellStyle {
    /// The terminal's defaults with no attributes.
    pub const DEFAULT: Self = Self { fg: None, bg: None, attrs: Attrs::NONE };

    /// With foreground `fg`.
    #[must_use]
    pub const fn fg(mut self, fg: Option<Color>) -> Self {
        self.fg = fg;
        self
    }

    /// With background `bg`.
    #[must_use]
    pub const fn bg(mut self, bg: Option<Color>) -> Self {
        self.bg = bg;
        self
    }

    /// With `attrs` added.
    #[must_use]
    pub const fn attrs(mut self, attrs: Attrs) -> Self {
        self.attrs = self.attrs.with(attrs);
        self
    }
}

/// One cell.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Cell {
    /// The grapheme cluster shown (`" "` for blank; empty for the second
    /// half of a wide cluster).
    pub text: String,
    /// 1, 2 for a wide cluster's first half, 0 for its continuation.
    pub width: u8,
    /// Its style.
    pub style: CellStyle,
}

impl Cell {
    /// A blank cell in `style`.
    #[must_use]
    pub fn blank(style: CellStyle) -> Self {
        Self { text: " ".to_owned(), width: 1, style }
    }

    /// Whether it is the second half of a wide cluster.
    #[must_use]
    pub const fn is_continuation(&self) -> bool {
        self.width == 0
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self::blank(CellStyle::DEFAULT)
    }
}

/// A grid of cells, `columns` × `rows`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Grid {
    columns: u16,
    rows: u16,
    cells: Vec<Cell>,
}

impl Grid {
    /// A blank grid.
    #[must_use]
    pub fn new(columns: u16, rows: u16) -> Self {
        Self {
            columns,
            rows,
            cells: vec![Cell::default(); usize::from(columns) * usize::from(rows)],
        }
    }

    /// Columns.
    #[must_use]
    pub const fn columns(&self) -> u16 {
        self.columns
    }

    /// Rows.
    #[must_use]
    pub const fn rows(&self) -> u16 {
        self.rows
    }

    /// The whole grid as a rectangle.
    #[must_use]
    pub fn bounds(&self) -> GridRect {
        GridRect::new(0, 0, i32::from(self.columns), i32::from(self.rows))
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let (x, y) = (u16::try_from(x).ok()?, u16::try_from(y).ok()?);
        (x < self.columns && y < self.rows)
            .then(|| usize::from(y) * usize::from(self.columns) + usize::from(x))
    }

    /// The cell at (`x`, `y`).
    #[must_use]
    pub fn get(&self, x: i32, y: i32) -> Option<&Cell> {
        self.index(x, y).map(|index| &self.cells[index])
    }

    /// Every cell of row `y`.
    #[must_use]
    pub fn row(&self, y: u16) -> &[Cell] {
        let start = usize::from(y) * usize::from(self.columns);
        self.cells.get(start..start + usize::from(self.columns)).unwrap_or(&[])
    }

    /// Blanks every cell in `style`.
    pub fn clear(&mut self, style: CellStyle) {
        for cell in &mut self.cells {
            *cell = Cell::blank(style);
        }
    }

    /// Breaks a wide cluster that a write at (`x`, `y`) would split.
    fn unsplit(&mut self, x: i32, y: i32) {
        let Some(index) = self.index(x, y) else { return };
        let cell = &self.cells[index];
        if cell.is_continuation() {
            if let Some(left) = self.index(x - 1, y) {
                let style = self.cells[left].style;
                self.cells[left] = Cell::blank(style);
            }
            let style = self.cells[index].style;
            self.cells[index] = Cell::blank(style);
        } else if cell.width == 2
            && let Some(right) = self.index(x + 1, y)
        {
            let style = self.cells[right].style;
            self.cells[right] = Cell::blank(style);
        }
    }

    /// Puts one cluster of `width` cells (1 or 2) at (`x`, `y`), clipped to
    /// `clip`. A wide cluster that does not fit leaves a blank instead.
    pub fn put(
        &mut self,
        x: i32,
        y: i32,
        grapheme: &str,
        width: u8,
        style: CellStyle,
        clip: GridRect,
    ) {
        let clip = clip.intersect(&self.bounds());
        if !clip.contains(x, y) || width == 0 {
            return;
        }
        self.unsplit(x, y);
        if width >= 2 {
            if !clip.contains(x + 1, y) {
                if let Some(index) = self.index(x, y) {
                    self.cells[index] = Cell::blank(style);
                }
                return;
            }
            self.unsplit(x + 1, y);
            if let (Some(first), Some(second)) = (self.index(x, y), self.index(x + 1, y)) {
                self.cells[first] = Cell { text: grapheme.to_owned(), width: 2, style };
                self.cells[second] = Cell { text: String::new(), width: 0, style };
            }
        } else if let Some(index) = self.index(x, y) {
            self.cells[index] = Cell { text: grapheme.to_owned(), width: 1, style };
        }
    }

    /// Prints `text` on one row from (`x`, `y`), clipped to `clip`, and
    /// returns the columns it took (including those clipped away).
    /// Control characters and zero-width clusters with nothing to attach to
    /// are not printed.
    pub fn print(
        &mut self,
        x: i32,
        y: i32,
        text: &str,
        style: CellStyle,
        clip: GridRect,
        width: Width,
    ) -> i32 {
        let mut column = x;
        for grapheme in graphemes(text) {
            let cells = width.of_grapheme(grapheme);
            if cells == 0 {
                continue;
            }
            let cells = u8::try_from(cells.min(2)).unwrap_or(1);
            self.put(column, y, grapheme, cells, style, clip);
            column += i32::from(cells);
        }
        column - x
    }

    /// Changes the style of every cell in `rect` with `change`.
    pub fn restyle(&mut self, rect: GridRect, mut change: impl FnMut(CellStyle) -> CellStyle) {
        let rect = rect.intersect(&self.bounds());
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                if let Some(index) = self.index(x, y) {
                    self.cells[index].style = change(self.cells[index].style);
                }
            }
        }
    }

    /// Fills `rect` with blank cells of background `bg`, keeping nothing.
    pub fn fill(&mut self, rect: GridRect, style: CellStyle) {
        let rect = rect.intersect(&self.bounds());
        for y in rect.y..rect.bottom() {
            self.unsplit(rect.x, y);
            self.unsplit(rect.right() - 1, y);
            for x in rect.x..rect.right() {
                if let Some(index) = self.index(x, y) {
                    self.cells[index] = Cell::blank(style);
                }
            }
        }
    }

    /// The text of row `y`, trailing blanks trimmed.
    #[must_use]
    pub fn row_text(&self, y: u16) -> String {
        let text: String = self.row(y).iter().map(|cell| cell.text.as_str()).collect();
        text.trim_end().to_owned()
    }

    /// Every row's text, joined by newlines, trailing blank rows trimmed —
    /// what a golden file holds.
    #[must_use]
    pub fn text(&self) -> String {
        let mut rows: Vec<String> = (0..self.rows).map(|y| self.row_text(y)).collect();
        while rows.last().is_some_and(String::is_empty) {
            rows.pop();
        }
        rows.join("\n")
    }

    /// Resizes to `columns` × `rows`, blank.
    pub fn resize(&mut self, columns: u16, rows: u16) {
        *self = Self::new(columns, rows);
    }

    /// Moves every row up one, blanking the last in `style` (what a
    /// terminal does at a line feed on its last row).
    pub fn scroll_up(&mut self, style: CellStyle) {
        let width = usize::from(self.columns);
        if width == 0 || self.cells.is_empty() {
            return;
        }
        self.cells.drain(..width);
        self.cells.extend(std::iter::repeat_n(Cell::blank(style), width));
    }

    /// Appends `text` to the cluster at (`x`, `y`) — a zero-width mark
    /// arriving after its base.
    pub fn append(&mut self, x: i32, y: i32, text: &str) {
        let mut x = x;
        if self.get(x, y).is_some_and(Cell::is_continuation) {
            x -= 1;
        }
        if let Some(index) = self.index(x, y) {
            self.cells[index].text.push_str(text);
        }
    }

    /// Sets the cell at (`x`, `y`) directly (no wide-cluster bookkeeping).
    pub fn set(&mut self, x: i32, y: i32, cell: Cell) {
        if let Some(index) = self.index(x, y) {
            self.cells[index] = cell;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_clusters_take_two_cells_and_are_never_split() {
        let mut grid = Grid::new(6, 1);
        let all = grid.bounds();
        assert_eq!(grid.print(0, 0, "a日b", CellStyle::DEFAULT, all, Width::default()), 4);
        assert_eq!(grid.row_text(0), "a日b");
        assert!(grid.get(2, 0).is_some_and(Cell::is_continuation));
        // Overwriting the continuation blanks the whole wide cluster.
        grid.put(2, 0, "x", 1, CellStyle::DEFAULT, all);
        assert_eq!(grid.row_text(0), "a xb");
        // A wide cluster at the clip's edge leaves a blank.
        let mut grid = Grid::new(3, 1);
        grid.print(2, 0, "日", CellStyle::DEFAULT, grid.bounds(), Width::default());
        assert_eq!(grid.get(2, 0).map(|cell| cell.text.as_str()), Some(" "));
    }

    #[test]
    fn printing_is_clipped_and_marks_attach() {
        let mut grid = Grid::new(10, 2);
        let clip = GridRect::new(2, 0, 3, 1);
        grid.print(0, 0, "abcdefg", CellStyle::DEFAULT, clip, Width::default());
        assert_eq!(grid.row_text(0), "  cde");
        grid.print(0, 1, "e\u{301}\tz", CellStyle::DEFAULT, grid.bounds(), Width::default());
        assert_eq!(grid.row_text(1), "e\u{301}z");
        assert_eq!(grid.text(), "  cde\ne\u{301}z");
    }

    #[test]
    fn fills_and_restyles() {
        let mut grid = Grid::new(4, 2);
        let red = CellStyle::DEFAULT.bg(Some(Color::rgb(255, 0, 0)));
        grid.fill(GridRect::new(1, 0, 2, 5), red);
        assert_eq!(grid.get(1, 1).map(|cell| cell.style), Some(red));
        assert_eq!(grid.get(0, 0).map(|cell| cell.style), Some(CellStyle::DEFAULT));
        grid.restyle(grid.bounds(), |style| style.attrs(Attrs::BOLD));
        assert!(grid.get(3, 1).is_some_and(|cell| cell.style.attrs.has(Attrs::BOLD)));
    }
}
