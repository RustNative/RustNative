//! Grids (`PLAN.md` Milestone 48, `C18-1`): a container that places its
//! children in rows and columns, with each child's placement as typed
//! layout data the container owns.
//!
//! A grid is a [`crate::Node::grid`]: its tracks are a [`GridStyle`], and a
//! child says where it goes with [`crate::LayoutStyle::grid`] — the same
//! `LayoutStyle` every node has, so both syntaxes carry it
//! (`grid={GridPlacement::at(0, 1)}` in markup). A child without a
//! placement takes the next free cell, row by row.
//!
//! Tracks are [`Track::Fixed`] (pixels), [`Track::Auto`] (the largest
//! natural size of what sits in that track alone), or [`Track::Fraction`]
//! (a share of the space left over, like CSS's `fr`). Rows beyond the
//! declared ones are `Auto`.

/// The size of one row or column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Track {
    /// Exactly this many pixels.
    Fixed(i32),
    /// As large as its content wants.
    Auto,
    /// This many shares of the space the other tracks leave.
    Fraction(u16),
}

/// A grid's tracks, gap, and padding.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct GridStyle {
    /// The columns, from the start edge.
    pub columns: Vec<Track>,
    /// The rows, from the top; rows past these are [`Track::Auto`].
    pub rows: Vec<Track>,
    /// The space between tracks.
    pub gap: i32,
    /// The space inside the grid's edges.
    pub padding: super::EdgeInsets,
}

impl GridStyle {
    /// A grid with `columns`.
    #[must_use]
    pub fn new(columns: impl IntoIterator<Item = Track>) -> Self {
        Self { columns: columns.into_iter().collect(), ..Self::default() }
    }

    /// With `rows`.
    #[must_use]
    pub fn rows(mut self, rows: impl IntoIterator<Item = Track>) -> Self {
        self.rows = rows.into_iter().collect();
        self
    }

    /// With `gap` between tracks.
    #[must_use]
    pub const fn gap(mut self, gap: i32) -> Self {
        self.gap = gap;
        self
    }

    /// With `padding`.
    #[must_use]
    pub const fn padding(mut self, padding: super::EdgeInsets) -> Self {
        self.padding = padding;
        self
    }
}

/// Where a child of a grid goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct GridPlacement {
    /// The first row it occupies.
    pub row: usize,
    /// The first column it occupies.
    pub column: usize,
    /// How many rows it spans (at least 1).
    pub row_span: usize,
    /// How many columns it spans (at least 1).
    pub column_span: usize,
}

impl GridPlacement {
    /// The cell at `row`, `column`.
    #[must_use]
    pub const fn at(row: usize, column: usize) -> Self {
        Self { row, column, row_span: 1, column_span: 1 }
    }

    /// Spanning `rows` rows and `columns` columns.
    #[must_use]
    pub const fn span(mut self, rows: usize, columns: usize) -> Self {
        self.row_span = if rows == 0 { 1 } else { rows };
        self.column_span = if columns == 0 { 1 } else { columns };
        self
    }
}

/// Where each child goes: explicit placements as given (clamped to the
/// columns), the rest in the next free cell, row by row.
#[must_use]
pub(crate) fn place(columns: usize, placements: &[Option<GridPlacement>]) -> Vec<GridPlacement> {
    let columns = columns.max(1);
    let mut taken: Vec<Vec<bool>> = Vec::new();
    let occupy = |placement: &GridPlacement, taken: &mut Vec<Vec<bool>>| {
        for row in placement.row..placement.row + placement.row_span {
            while taken.len() <= row {
                taken.push(vec![false; columns]);
            }
            let end = (placement.column + placement.column_span).min(columns);
            for cell in taken[row].iter_mut().take(end).skip(placement.column) {
                *cell = true;
            }
        }
    };
    let mut placed = vec![None; placements.len()];
    for (index, placement) in placements.iter().enumerate() {
        if let Some(placement) = placement {
            let column = placement.column.min(columns - 1);
            let clamped = GridPlacement {
                column,
                column_span: placement.column_span.max(1).min(columns - column),
                row_span: placement.row_span.max(1),
                ..*placement
            };
            occupy(&clamped, &mut taken);
            placed[index] = Some(clamped);
        }
    }
    let mut cursor = 0_usize;
    for slot in &mut placed {
        if slot.is_some() {
            continue;
        }
        loop {
            let (row, column) = (cursor / columns, cursor % columns);
            cursor += 1;
            if taken.get(row).is_none_or(|cells| !cells[column]) {
                let placement = GridPlacement::at(row, column);
                occupy(&placement, &mut taken);
                *slot = Some(placement);
                break;
            }
        }
    }
    placed.into_iter().map(|slot| slot.unwrap_or(GridPlacement::at(0, 0))).collect()
}

/// Sizes `tracks` (with `extra` implicit `Auto` tracks) to fill `available`
/// minus the gaps: fixed first, then auto at `natural`, then fractions share
/// what is left. With no `available` (measuring), fractions take their
/// natural size.
#[must_use]
pub(crate) fn size_tracks(
    tracks: &[Track],
    count: usize,
    natural: &[i32],
    gap: i32,
    available: Option<i32>,
) -> Vec<i32> {
    let track = |index: usize| tracks.get(index).copied().unwrap_or(Track::Auto);
    let mut sizes = (0..count)
        .map(|index| match track(index) {
            Track::Fixed(size) => size.max(0),
            Track::Auto => natural.get(index).copied().unwrap_or(0),
            Track::Fraction(_) => 0,
        })
        .collect::<Vec<_>>();
    let shares: i64 = (0..count)
        .map(|index| match track(index) {
            Track::Fraction(share) => i64::from(share),
            _ => 0,
        })
        .sum();
    if shares > 0 {
        let gaps =
            gap.max(0).saturating_mul(i32::try_from(count.saturating_sub(1)).unwrap_or(i32::MAX));
        let used: i32 = sizes.iter().copied().fold(0, i32::saturating_add);
        for (index, size) in sizes.iter_mut().enumerate() {
            if let Track::Fraction(share) = track(index) {
                *size = match available {
                    Some(available) => {
                        let left =
                            i64::from(available.saturating_sub(used).saturating_sub(gaps).max(0));
                        i32::try_from(left * i64::from(share) / shares).unwrap_or(0)
                    }
                    None => natural.get(index).copied().unwrap_or(0),
                };
            }
        }
    }
    sizes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_fills_free_cells_row_by_row_around_explicit_ones() {
        let placed = place(
            2,
            &[
                Some(GridPlacement::at(0, 1)),
                None,
                None,
                Some(GridPlacement::at(1, 0).span(1, 2)),
                None,
            ],
        );
        assert_eq!(placed[1], GridPlacement::at(0, 0));
        assert_eq!(placed[2], GridPlacement::at(2, 0), "row 1 is taken by the span");
        assert_eq!(placed[4], GridPlacement::at(2, 1));
    }

    #[test]
    fn fractions_share_what_fixed_and_auto_tracks_leave() {
        let sizes = size_tracks(
            &[Track::Fixed(100), Track::Auto, Track::Fraction(1), Track::Fraction(3)],
            4,
            &[0, 50, 0, 0],
            10,
            Some(400),
        );
        // 400 - 100 - 50 - 3 gaps of 10 = 220, shared 1:3.
        assert_eq!(sizes, vec![100, 50, 55, 165]);
    }
}
