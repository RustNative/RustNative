//! The headless behaviour layer (`PLAN.md` Milestone 48, `C19`): the focus,
//! keyboard, and selection logic of composite controls, independent of how
//! they look.
//!
//! Each behaviour is a plain state machine over key presses. The
//! composites in this crate are built on them, and so can an application's
//! own composites, a custom-drawn control, or a terminal or embedded
//! backend — none reimplements arrow-key handling.
//!
//! ```
//! use rustnative_components::behaviour::{ListSelection, Outcome};
//! use rustnative_core::{KeyCode, KeyModifiers};
//!
//! let mut list = ListSelection::multiple(4);
//! assert_eq!(list.key(KeyCode::ArrowDown, KeyModifiers::default()), Outcome::Moved);
//! assert_eq!(list.key(KeyCode::Space, KeyModifiers::default()), Outcome::Selected);
//! assert_eq!(list.selected(), vec![1]);
//! ```

use std::collections::BTreeSet;

use rustnative_core::{CalendarDate, KeyCode, KeyModifiers};

/// What a key press did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing: the behaviour does not handle this key here.
    Ignored,
    /// Focus moved.
    Moved,
    /// The selection changed.
    Selected,
    /// The focused item was activated (Enter).
    Activated,
    /// The value changed.
    Changed,
    /// Something opened.
    Opened,
    /// Something closed (Escape).
    Closed,
}

fn step(focused: usize, count: usize, key: KeyCode, page: usize) -> Option<usize> {
    let last = count.checked_sub(1)?;
    Some(match key {
        KeyCode::ArrowDown | KeyCode::ArrowRight => (focused + 1).min(last),
        KeyCode::ArrowUp | KeyCode::ArrowLeft => focused.saturating_sub(1),
        KeyCode::Home => 0,
        KeyCode::End => last,
        KeyCode::PageDown => (focused + page).min(last),
        KeyCode::PageUp => focused.saturating_sub(page),
        _ => return None,
    })
}

/// Focus and selection in a list: arrows move, Space selects, Shift
/// extends, Ctrl+A selects all (multiple selection).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListSelection {
    count: usize,
    focused: usize,
    anchor: usize,
    selected: BTreeSet<usize>,
    multiple: bool,
    page: usize,
}

impl ListSelection {
    /// A list of `count` items choosing one.
    #[must_use]
    pub fn single(count: usize) -> Self {
        Self { count, focused: 0, anchor: 0, selected: BTreeSet::new(), multiple: false, page: 10 }
    }

    /// A list of `count` items choosing any number.
    #[must_use]
    pub fn multiple(count: usize) -> Self {
        Self { multiple: true, ..Self::single(count) }
    }

    /// The focused item.
    #[must_use]
    pub fn focused(&self) -> usize {
        self.focused
    }

    /// The selected items, in order.
    #[must_use]
    pub fn selected(&self) -> Vec<usize> {
        self.selected.iter().copied().collect()
    }

    /// Changes the number of items, keeping focus and selection in range.
    pub fn set_count(&mut self, count: usize) {
        self.count = count;
        self.focused = self.focused.min(count.saturating_sub(1));
        self.selected.retain(|index| *index < count);
    }

    /// Selects `index` as a click would.
    pub fn click(&mut self, index: usize, modifiers: KeyModifiers) -> Outcome {
        if index >= self.count {
            return Outcome::Ignored;
        }
        self.focused = index;
        if self.multiple && modifiers.ctrl {
            if !self.selected.remove(&index) {
                self.selected.insert(index);
            }
        } else if self.multiple && modifiers.shift {
            self.extend_to(index);
            return Outcome::Selected;
        } else {
            self.selected = BTreeSet::from([index]);
        }
        self.anchor = index;
        Outcome::Selected
    }

    fn extend_to(&mut self, index: usize) {
        let (low, high) = (self.anchor.min(index), self.anchor.max(index));
        self.selected = (low..=high).collect();
    }

    /// Handles a key press.
    pub fn key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Outcome {
        if let Some(next) = step(self.focused, self.count, key, self.page) {
            self.focused = next;
            if self.multiple && modifiers.shift {
                self.extend_to(next);
                return Outcome::Selected;
            }
            if !self.multiple && !modifiers.ctrl {
                // Single selection follows focus, as a list box does.
                self.selected = BTreeSet::from([next]);
                self.anchor = next;
                return Outcome::Selected;
            }
            return Outcome::Moved;
        }
        match key {
            KeyCode::Space => self.click(self.focused, modifiers),
            KeyCode::Enter => Outcome::Activated,
            KeyCode::Character('a' | 'A') if self.multiple && modifiers.ctrl => {
                self.selected = (0..self.count).collect();
                Outcome::Selected
            }
            _ => Outcome::Ignored,
        }
    }
}

/// Tabs with roving focus: arrows move focus, and (automatic activation)
/// select the focused tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabsBehaviour {
    count: usize,
    selected: usize,
}

impl TabsBehaviour {
    /// `count` tabs with `selected` chosen.
    #[must_use]
    pub fn new(count: usize, selected: usize) -> Self {
        Self { count, selected: selected.min(count.saturating_sub(1)) }
    }

    /// The selected tab.
    #[must_use]
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Handles a key press; right-to-left layouts swap the arrows.
    pub fn key(&mut self, key: KeyCode, rtl: bool) -> Outcome {
        let key = match (key, rtl) {
            (KeyCode::ArrowLeft, true) => KeyCode::ArrowRight,
            (KeyCode::ArrowRight, true) => KeyCode::ArrowLeft,
            (KeyCode::ArrowUp | KeyCode::ArrowDown, _) => return Outcome::Ignored,
            (other, _) => other,
        };
        match step(self.selected, self.count, key, 1) {
            Some(next) if next != self.selected => {
                self.selected = next;
                Outcome::Selected
            }
            Some(_) => Outcome::Moved,
            None => Outcome::Ignored,
        }
    }
}

/// A menu: arrows move among enabled items (wrapping), a letter jumps to the
/// next item starting with it, Enter activates, Escape closes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuNav {
    labels: Vec<String>,
    enabled: Vec<bool>,
    focused: Option<usize>,
}

impl MenuNav {
    /// A menu of `labels`, each enabled or not.
    #[must_use]
    pub fn new(items: impl IntoIterator<Item = (String, bool)>) -> Self {
        let (labels, enabled) = items.into_iter().unzip();
        Self { labels, enabled, focused: None }
    }

    /// The focused item.
    #[must_use]
    pub fn focused(&self) -> Option<usize> {
        self.focused
    }

    fn next_enabled(&self, from: Option<usize>, forward: bool) -> Option<usize> {
        let count = self.labels.len();
        (1..=count)
            .map(|offset| match (from, forward) {
                (None, true) => offset - 1,
                (None, false) => count - offset,
                (Some(at), true) => (at + offset) % count,
                (Some(at), false) => (at + count - offset % count) % count,
            })
            .find(|index| self.enabled.get(*index).copied().unwrap_or(false))
    }

    /// Handles a key press.
    pub fn key(&mut self, key: KeyCode) -> Outcome {
        let next = match key {
            KeyCode::ArrowDown => self.next_enabled(self.focused, true),
            KeyCode::ArrowUp => self.next_enabled(self.focused, false),
            KeyCode::Home => self.next_enabled(None, true),
            KeyCode::End => self.next_enabled(None, false),
            KeyCode::Enter | KeyCode::Space => {
                return if self.focused.is_some() { Outcome::Activated } else { Outcome::Ignored };
            }
            KeyCode::Escape => return Outcome::Closed,
            KeyCode::Character(letter) => {
                let letter = letter.to_lowercase().next().unwrap_or(letter);
                let count = self.labels.len();
                let start = self.focused.map_or(0, |at| at + 1);
                (0..count).map(|offset| (start + offset) % count).find(|index| {
                    self.enabled[*index] && self.labels[*index].to_lowercase().starts_with(letter)
                })
            }
            _ => return Outcome::Ignored,
        };
        match next {
            Some(index) => {
                self.focused = Some(index);
                Outcome::Moved
            }
            None => Outcome::Ignored,
        }
    }
}

/// One item of a tree for [`TreeNav`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeItem {
    /// Its identity.
    pub id: String,
    /// Its parent's identity, `None` at the top.
    pub parent: Option<String>,
    /// Its label.
    pub label: String,
}

/// A tree: Up/Down move through the visible items, Right expands or moves
/// to the first child, Left collapses or moves to the parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNav {
    items: Vec<TreeItem>,
    expanded: BTreeSet<String>,
    focused: Option<String>,
}

impl TreeNav {
    /// A tree of `items` (parents before their children), all collapsed.
    #[must_use]
    pub fn new(items: Vec<TreeItem>) -> Self {
        let focused = items.first().map(|item| item.id.clone());
        Self { items, expanded: BTreeSet::new(), focused }
    }

    /// The expanded items.
    #[must_use]
    pub fn expanded(&self) -> &BTreeSet<String> {
        &self.expanded
    }

    /// The focused item.
    #[must_use]
    pub fn focused(&self) -> Option<&str> {
        self.focused.as_deref()
    }

    /// Expands or collapses `id`.
    pub fn toggle(&mut self, id: &str) {
        if !self.expanded.remove(id) {
            self.expanded.insert(id.to_owned());
        }
    }

    fn has_children(&self, id: &str) -> bool {
        self.items.iter().any(|item| item.parent.as_deref() == Some(id))
    }

    /// The items shown, depth-first, with their depth.
    #[must_use]
    pub fn visible(&self) -> Vec<(&TreeItem, usize)> {
        let mut out = Vec::new();
        self.walk(None, 0, &mut out);
        out
    }

    fn walk<'a>(
        &'a self,
        parent: Option<&str>,
        depth: usize,
        out: &mut Vec<(&'a TreeItem, usize)>,
    ) {
        for item in self.items.iter().filter(|item| item.parent.as_deref() == parent) {
            out.push((item, depth));
            if self.expanded.contains(&item.id) {
                self.walk(Some(&item.id), depth + 1, out);
            }
        }
    }

    /// Handles a key press.
    pub fn key(&mut self, key: KeyCode) -> Outcome {
        let visible =
            self.visible().into_iter().map(|(item, _)| item.id.clone()).collect::<Vec<_>>();
        let Some(focused) = self.focused.clone() else { return Outcome::Ignored };
        let at = visible.iter().position(|id| *id == focused).unwrap_or(0);
        match key {
            KeyCode::ArrowDown | KeyCode::ArrowUp | KeyCode::Home | KeyCode::End => {
                match step(at, visible.len(), key, 1) {
                    Some(next) => {
                        self.focused = Some(visible[next].clone());
                        Outcome::Moved
                    }
                    None => Outcome::Ignored,
                }
            }
            KeyCode::ArrowRight if self.has_children(&focused) => {
                if self.expanded.insert(focused.clone()) {
                    Outcome::Opened
                } else {
                    self.focused = self
                        .items
                        .iter()
                        .find(|item| item.parent.as_deref() == Some(focused.as_str()))
                        .map(|item| item.id.clone());
                    Outcome::Moved
                }
            }
            KeyCode::ArrowLeft => {
                if self.expanded.remove(&focused) {
                    Outcome::Closed
                } else if let Some(parent) = self
                    .items
                    .iter()
                    .find(|item| item.id == focused)
                    .and_then(|item| item.parent.clone())
                {
                    self.focused = Some(parent);
                    Outcome::Moved
                } else {
                    Outcome::Ignored
                }
            }
            KeyCode::Enter => Outcome::Activated,
            _ => Outcome::Ignored,
        }
    }
}

/// A grid: arrows move by cell, Home/End along the row, Ctrl+Home/End to
/// the corners, Page Up/Down by `page` rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridNav {
    rows: usize,
    columns: usize,
    row: usize,
    column: usize,
    page: usize,
}

impl GridNav {
    /// A `rows`×`columns` grid.
    #[must_use]
    pub fn new(rows: usize, columns: usize) -> Self {
        Self { rows, columns, row: 0, column: 0, page: 10 }
    }

    /// The focused cell, `(row, column)`.
    #[must_use]
    pub fn focused(&self) -> (usize, usize) {
        (self.row, self.column)
    }

    /// Handles a key press.
    pub fn key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Outcome {
        let (last_row, last_column) = (self.rows.saturating_sub(1), self.columns.saturating_sub(1));
        let before = (self.row, self.column);
        match key {
            KeyCode::ArrowDown => self.row = (self.row + 1).min(last_row),
            KeyCode::ArrowUp => self.row = self.row.saturating_sub(1),
            KeyCode::ArrowRight => self.column = (self.column + 1).min(last_column),
            KeyCode::ArrowLeft => self.column = self.column.saturating_sub(1),
            KeyCode::Home if modifiers.ctrl => (self.row, self.column) = (0, 0),
            KeyCode::End if modifiers.ctrl => (self.row, self.column) = (last_row, last_column),
            KeyCode::Home => self.column = 0,
            KeyCode::End => self.column = last_column,
            KeyCode::PageDown => self.row = (self.row + self.page).min(last_row),
            KeyCode::PageUp => self.row = self.row.saturating_sub(self.page),
            KeyCode::Enter => return Outcome::Activated,
            _ => return Outcome::Ignored,
        }
        if (self.row, self.column) == before { Outcome::Ignored } else { Outcome::Moved }
    }
}

/// A combobox: typing filters the options, arrows move the highlight,
/// Enter chooses, Escape closes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combobox {
    options: Vec<String>,
    query: String,
    highlighted: Option<usize>,
    open: bool,
    chosen: Option<usize>,
}

impl Combobox {
    /// A combobox over `options`.
    #[must_use]
    pub fn new(options: Vec<String>) -> Self {
        Self { options, query: String::new(), highlighted: None, open: false, chosen: None }
    }

    /// The options matching the query, as indices into the options.
    #[must_use]
    pub fn filtered(&self) -> Vec<usize> {
        let query = self.query.to_lowercase();
        (0..self.options.len())
            .filter(|index| self.options[*index].to_lowercase().contains(&query))
            .collect()
    }

    /// Whether the list is open.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The highlighted option.
    #[must_use]
    pub fn highlighted(&self) -> Option<usize> {
        self.highlighted
    }

    /// The chosen option.
    #[must_use]
    pub fn chosen(&self) -> Option<usize> {
        self.chosen
    }

    /// Replaces the query (what the field holds), opening the list.
    pub fn set_query(&mut self, query: &str) -> Outcome {
        query.clone_into(&mut self.query);
        self.open = true;
        self.highlighted = self.filtered().first().copied();
        Outcome::Opened
    }

    /// Handles a key press.
    pub fn key(&mut self, key: KeyCode) -> Outcome {
        let filtered = self.filtered();
        let at = self.highlighted.and_then(|index| filtered.iter().position(|i| *i == index));
        match key {
            KeyCode::ArrowDown | KeyCode::ArrowUp if !self.open => {
                self.open = true;
                self.highlighted = filtered.first().copied();
                Outcome::Opened
            }
            KeyCode::ArrowDown | KeyCode::ArrowUp | KeyCode::Home | KeyCode::End => {
                match step(at.unwrap_or(0), filtered.len(), key, 5) {
                    Some(next) => {
                        self.highlighted = Some(filtered[next]);
                        Outcome::Moved
                    }
                    None => Outcome::Ignored,
                }
            }
            KeyCode::Enter => match self.highlighted {
                Some(index) if self.open => {
                    self.chosen = Some(index);
                    self.query.clone_from(&self.options[index]);
                    self.open = false;
                    Outcome::Selected
                }
                _ => Outcome::Ignored,
            },
            KeyCode::Escape if self.open => {
                self.open = false;
                Outcome::Closed
            }
            _ => Outcome::Ignored,
        }
    }
}

/// Which part of a date [`DateEntry`] edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateSegment {
    /// The year.
    Year,
    /// The month.
    Month,
    /// The day.
    Day,
}

/// Keyboard date entry by segment: Left/Right choose the segment, Up/Down
/// change it, and the day is kept valid for the month.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateEntry {
    date: CalendarDate,
    segment: DateSegment,
}

impl DateEntry {
    /// Entry starting at `date`, on the year.
    #[must_use]
    pub fn new(date: CalendarDate) -> Self {
        Self { date, segment: DateSegment::Year }
    }

    /// The date.
    #[must_use]
    pub fn date(&self) -> CalendarDate {
        self.date
    }

    /// The segment being edited.
    #[must_use]
    pub fn segment(&self) -> DateSegment {
        self.segment
    }

    fn clamp(year: i32, month: u8, day: u8) -> CalendarDate {
        (1..=day).rev().find_map(|day| CalendarDate::new(year, month, day)).unwrap_or_default()
    }

    /// Handles a key press.
    pub fn key(&mut self, key: KeyCode) -> Outcome {
        let CalendarDate { year, month, day } = self.date;
        let delta: i32 = match key {
            KeyCode::ArrowLeft => {
                self.segment = match self.segment {
                    DateSegment::Day => DateSegment::Month,
                    _ => DateSegment::Year,
                };
                return Outcome::Moved;
            }
            KeyCode::ArrowRight => {
                self.segment = match self.segment {
                    DateSegment::Year => DateSegment::Month,
                    _ => DateSegment::Day,
                };
                return Outcome::Moved;
            }
            KeyCode::ArrowUp => 1,
            KeyCode::ArrowDown => -1,
            _ => return Outcome::Ignored,
        };
        self.date = match self.segment {
            DateSegment::Year => Self::clamp(year + delta, month, day),
            DateSegment::Month => {
                let month = (i32::from(month) - 1 + delta).rem_euclid(12) + 1;
                Self::clamp(year, u8::try_from(month).unwrap_or(1), day)
            }
            DateSegment::Day => {
                let days = (28..=31)
                    .rev()
                    .find(|days| CalendarDate::new(year, month, *days).is_some())
                    .unwrap_or(28);
                let day = (i32::from(day) - 1 + delta).rem_euclid(i32::from(days)) + 1;
                Self::clamp(year, month, u8::try_from(day).unwrap_or(1))
            }
        };
        Outcome::Changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: KeyModifiers = KeyModifiers { shift: false, ctrl: false, alt: false, meta: false };
    const SHIFT: KeyModifiers = KeyModifiers { shift: true, ctrl: false, alt: false, meta: false };

    #[test]
    fn a_multiple_selection_extends_with_shift() {
        let mut list = ListSelection::multiple(5);
        list.click(1, NONE);
        list.key(KeyCode::ArrowDown, SHIFT);
        list.key(KeyCode::ArrowDown, SHIFT);
        assert_eq!(list.selected(), vec![1, 2, 3]);
        assert_eq!(list.key(KeyCode::End, NONE), Outcome::Moved);
        assert_eq!(list.focused(), 4);
    }

    #[test]
    fn tabs_mirror_in_right_to_left_layouts() {
        let mut tabs = TabsBehaviour::new(3, 1);
        tabs.key(KeyCode::ArrowLeft, true);
        assert_eq!(tabs.selected(), 2);
    }

    #[test]
    fn a_menu_skips_disabled_items_wraps_and_jumps_by_letter() {
        let mut menu =
            MenuNav::new([("Open".into(), true), ("Save".into(), false), ("Share".into(), true)]);
        menu.key(KeyCode::ArrowDown);
        menu.key(KeyCode::ArrowDown);
        assert_eq!(menu.focused(), Some(2), "Save is disabled");
        menu.key(KeyCode::ArrowDown);
        assert_eq!(menu.focused(), Some(0), "wrapped");
        menu.key(KeyCode::Character('s'));
        assert_eq!(menu.focused(), Some(2));
        assert_eq!(menu.key(KeyCode::Escape), Outcome::Closed);
    }

    #[test]
    fn a_tree_expands_descends_and_climbs() {
        let item = |id: &str, parent: Option<&str>| TreeItem {
            id: id.into(),
            parent: parent.map(Into::into),
            label: id.into(),
        };
        let mut tree = TreeNav::new(vec![
            item("src", None),
            item("main.rs", Some("src")),
            item("README", None),
        ]);
        assert_eq!(tree.visible().len(), 2);
        assert_eq!(tree.key(KeyCode::ArrowRight), Outcome::Opened);
        assert_eq!(tree.visible().len(), 3);
        tree.key(KeyCode::ArrowRight);
        assert_eq!(tree.focused(), Some("main.rs"));
        tree.key(KeyCode::ArrowLeft);
        assert_eq!(tree.focused(), Some("src"));
        assert_eq!(tree.key(KeyCode::ArrowLeft), Outcome::Closed);
    }

    #[test]
    fn a_grid_moves_by_cell_and_to_its_corners() {
        let mut grid = GridNav::new(3, 4);
        grid.key(KeyCode::ArrowRight, NONE);
        grid.key(KeyCode::ArrowDown, NONE);
        assert_eq!(grid.focused(), (1, 1));
        let ctrl = KeyModifiers { ctrl: true, ..NONE };
        grid.key(KeyCode::End, ctrl);
        assert_eq!(grid.focused(), (2, 3));
    }

    #[test]
    fn a_combobox_filters_and_chooses() {
        let mut combo = Combobox::new(vec!["Apple".into(), "Apricot".into(), "Banana".into()]);
        combo.set_query("ap");
        assert_eq!(combo.filtered(), vec![0, 1]);
        combo.key(KeyCode::ArrowDown);
        assert_eq!(combo.key(KeyCode::Enter), Outcome::Selected);
        assert_eq!(combo.chosen(), Some(1));
        assert!(!combo.is_open());
    }

    #[test]
    fn date_entry_keeps_the_day_valid() {
        let mut entry = DateEntry::new(CalendarDate::new(2024, 1, 31).unwrap_or_default());
        entry.key(KeyCode::ArrowRight);
        entry.key(KeyCode::ArrowUp);
        assert_eq!(entry.date(), CalendarDate::new(2024, 2, 29).unwrap_or_default());
        entry.key(KeyCode::ArrowRight);
        entry.key(KeyCode::ArrowUp);
        assert_eq!(entry.date(), CalendarDate::new(2024, 2, 1).unwrap_or_default(), "wraps");
    }
}
