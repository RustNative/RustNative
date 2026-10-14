//! Text measurement in cells (`PLAN.md` 2.11 and Milestone 38): what the
//! portable [`IntrinsicMeasurer`] asks, answered by Unicode display width
//! and reported in logical pixels at the backend's cell size — so the core
//! keeps one geometry model and every measured size is a whole number of
//! cells.
//!
//! | Node | Cells |
//! |---|---|
//! | label | its text wrapped to the width offered; one line per row |
//! | button | `[ text ]`: the text and four cells of chrome, one row |
//! | text field | 20 cells of text and two of chrome (a field does not grow as it is typed into), one row |
//! | tab bar | each label with a cell either side, a cell between tabs |
//! | check box, radio, switch | the label and four (`[x] `, `(*) `) or six (`[on] `) cells |
//! | slider, progress | 20 cells (progress: and 5 for its percentage), one row |
//! | select | the widest option and three cells (` ▾`), at least 10 |
//! | list box | the widest item and two cells, 3 to 8 rows |
//! | date picker | 10 cells (`YYYY-MM-DD`) |
//! | spinner | the widest value and four cells (`< 9 >`) |
//! | separator | one row |
//! | multi-line text | 30 cells, 5 rows |
//! | image | its pixels as logical pixels |

use rustnative_core::{Control, IntrinsicMeasurer, NodeKind, Size};
use rustnative_drawlist::GridMapping;

use crate::width::Width;

/// Measures in cells, reporting logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TuiMeasurer {
    mapping: GridMapping,
    width: Width,
}

/// Columns a text field shows before it scrolls.
pub const FIELD_COLUMNS: usize = 20;

impl TuiMeasurer {
    /// A measurer at `mapping`'s cell size and `width`'s rules.
    #[must_use]
    pub const fn new(mapping: GridMapping, width: Width) -> Self {
        Self { mapping, width }
    }

    fn size(&self, columns: usize, rows: usize) -> Size {
        let columns = u32::try_from(columns.min(10_000)).unwrap_or(10_000);
        let rows = u32::try_from(rows.min(10_000)).unwrap_or(10_000);
        Size::new(columns * self.mapping.unit_width(), rows * self.mapping.unit_height())
    }

    fn available(&self, max_width: Option<i32>) -> Option<usize> {
        max_width.map(|width| {
            usize::try_from(width.max(0)).unwrap_or(0) / self.mapping.unit_width() as usize
        })
    }

    fn text(&self, text: &str) -> usize {
        self.width.of(text)
    }
}

impl IntrinsicMeasurer for TuiMeasurer {
    fn measure(&self, kind: NodeKind, text: Option<&str>, max_width: Option<i32>) -> Size {
        let text = text.unwrap_or("");
        let available = self.available(max_width);
        match kind {
            NodeKind::Label => {
                let lines = match available {
                    Some(columns) => self.width.wrap(text, columns.max(1)),
                    None => text.split('\n').map(str::to_owned).collect(),
                };
                let widest = lines.iter().map(|line| self.text(line)).max().unwrap_or(0);
                self.size(widest, lines.len().max(1))
            }
            NodeKind::Button => {
                let columns = self.text(text) + 4;
                self.size(available.map_or(columns, |limit| columns.min(limit.max(1))), 1)
            }
            NodeKind::TextInput => {
                let columns = FIELD_COLUMNS + 2;
                self.size(available.map_or(columns, |limit| columns.min(limit.max(1))), 1)
            }
            NodeKind::TabBar => {
                // The snapshot joins the labels with five spaces; the bar
                // draws each as ` label ` with one cell between.
                let labels: Vec<&str> = text.split("     ").collect();
                let columns: usize = labels.iter().map(|label| self.text(label) + 2).sum::<usize>()
                    + labels.len().saturating_sub(1);
                self.size(columns, 1)
            }
            NodeKind::Canvas
            | NodeKind::Surface
            | NodeKind::Control
            | NodeKind::Column
            | NodeKind::Row => self.size(0, 0),
        }
    }

    fn measure_control(&self, control: &Control) -> Size {
        match control {
            Control::Checkbox { label, .. } | Control::Radio { label, .. } => {
                self.size(self.text(label) + 4, 1)
            }
            Control::Toggle { label, .. } => self.size(self.text(label) + 6, 1),
            Control::Slider { min, max, .. } => {
                let digits = min.to_string().len().max(max.to_string().len());
                self.size(20 + digits + 1, 1)
            }
            Control::Progress { .. } => self.size(25, 1),
            Control::Select { options, .. } => {
                let widest = options.iter().map(|option| self.text(option)).max().unwrap_or(0);
                self.size((widest + 3).max(10), 1)
            }
            Control::ListBox { items, .. } => {
                let widest = items.iter().map(|item| self.text(item)).max().unwrap_or(0);
                self.size((widest + 2).max(12), items.len().clamp(3, 8))
            }
            Control::DatePicker { .. } => self.size(10, 1),
            Control::Spinner { min, max, .. } => {
                let digits = min.to_string().len().max(max.to_string().len());
                self.size(digits + 4, 1)
            }
            Control::Separator => self.size(0, 1),
            Control::Link { text } => self.size(self.text(text), 1),
            Control::MultilineText { .. } => self.size(30, 5),
            Control::Image { image } => Size::new(image.width(), image.height()),
            _ => self.size(1, 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_whole_cells() {
        let measure = TuiMeasurer::new(GridMapping::new(8, 16), Width::default());
        assert_eq!(measure.measure(NodeKind::Label, Some("日本語 ok"), None), Size::new(9 * 8, 16));
        assert_eq!(
            measure.measure(NodeKind::Label, Some("one two three"), Some(7 * 8)),
            Size::new(7 * 8, 32)
        );
        assert_eq!(measure.measure(NodeKind::Button, Some("OK"), None), Size::new(6 * 8, 16));
        assert_eq!(
            measure.measure(NodeKind::TextInput, Some("a long typed value that is long"), None),
            measure.measure(NodeKind::TextInput, Some(""), None),
            "a field does not grow as it is typed into"
        );
        assert_eq!(
            measure.measure(NodeKind::TabBar, Some("One     Two"), None),
            Size::new(11 * 8, 16)
        );
        assert_eq!(
            measure
                .measure_control(&Control::Checkbox { label: "Remember".into(), checked: false }),
            Size::new(12 * 8, 16)
        );
        assert_eq!(
            measure.measure_control(&Control::ListBox { items: vec!["a".into()], selected: None }),
            Size::new(12 * 8, 3 * 16)
        );
    }
}
