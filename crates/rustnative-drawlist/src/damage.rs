//! Damage tracking: which units of the grid changed since the last frame.
//!
//! A drawn host writes only what changed (`PLAN.md` Milestones 37 and 38):
//! a terminal's bytes per keystroke and an embedded display's transfer per
//! frame are both proportional to the damage. A [`Region`] collects the
//! rectangles that changed and coalesces them, so a writer visits few
//! rectangles rather than many small ones:
//!
//! - a rectangle inside one already recorded adds nothing;
//! - two rectangles that overlap or touch are merged when their union wastes
//!   at most a quarter of its area on units neither covered;
//! - past [`Region::MAX_RECTS`] rectangles, the two whose union wastes least
//!   are merged, so the count stays bounded however the damage arrives.
//!
//! ```
//! use rustnative_drawlist::{GridRect, Region};
//!
//! let mut damage = Region::new();
//! damage.add(GridRect::new(0, 0, 4, 1));
//! damage.add(GridRect::new(4, 0, 4, 1)); // touching: one run
//! damage.add(GridRect::new(1, 0, 2, 1)); // inside it: nothing new
//! assert_eq!(damage.rects(), &[GridRect::new(0, 0, 8, 1)]);
//! ```

use crate::grid::GridRect;

/// A set of damaged rectangles, coalesced as they are added.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Region {
    rects: Vec<GridRect>,
}

fn touches(a: &GridRect, b: &GridRect) -> bool {
    a.x <= b.right() && b.x <= a.right() && a.y <= b.bottom() && b.y <= a.bottom()
}

fn contains(outer: &GridRect, inner: &GridRect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}

/// Units of `union(a, b)` neither `a` nor `b` covers.
fn waste(a: &GridRect, b: &GridRect) -> i64 {
    let union = a.union(b);
    union.area() - a.area() - b.area() + a.intersect(b).area()
}

impl Region {
    /// The most rectangles a region holds before merging the closest two.
    pub const MAX_RECTS: usize = 16;

    /// No damage.
    #[must_use]
    pub const fn new() -> Self {
        Self { rects: Vec::new() }
    }

    /// Whether nothing is damaged.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    /// The damaged rectangles, disjoint or overlapping only where a merge
    /// was refused as too wasteful.
    #[must_use]
    pub fn rects(&self) -> &[GridRect] {
        &self.rects
    }

    /// How many units the rectangles cover (overlaps counted once per
    /// rectangle — an upper bound).
    #[must_use]
    pub fn area(&self) -> i64 {
        self.rects.iter().map(GridRect::area).sum()
    }

    /// The smallest rectangle covering all damage.
    #[must_use]
    pub fn bounds(&self) -> GridRect {
        self.rects.iter().fold(GridRect::default(), |bounds, rect| bounds.union(rect))
    }

    /// Forgets all damage.
    pub fn clear(&mut self) {
        self.rects.clear();
    }

    /// Takes the damage, leaving none.
    #[must_use = "the damage taken is what must be written"]
    pub fn take(&mut self) -> Self {
        std::mem::take(self)
    }

    /// Records `rect` as damaged.
    pub fn add(&mut self, rect: GridRect) {
        if rect.is_empty() || self.rects.iter().any(|existing| contains(existing, &rect)) {
            return;
        }
        let mut rect = rect;
        // Absorb what it now covers or merges cheaply with, until stable.
        loop {
            let mut merged = false;
            let mut index = 0;
            while index < self.rects.len() {
                let existing = self.rects[index];
                let cheap = touches(&existing, &rect)
                    && waste(&existing, &rect) * 4 <= existing.union(&rect).area();
                if contains(&rect, &existing) || cheap {
                    rect = rect.union(&existing);
                    self.rects.swap_remove(index);
                    merged = true;
                } else {
                    index += 1;
                }
            }
            if !merged {
                break;
            }
        }
        self.rects.push(rect);
        while self.rects.len() > Self::MAX_RECTS {
            self.merge_closest();
        }
    }

    /// Adds every rectangle of `other`.
    pub fn extend(&mut self, other: &Self) {
        for rect in &other.rects {
            self.add(*rect);
        }
    }

    fn merge_closest(&mut self) {
        let mut best = (0, 1, i64::MAX);
        for i in 0..self.rects.len() {
            for j in i + 1..self.rects.len() {
                let cost = waste(&self.rects[i], &self.rects[j]);
                if cost < best.2 {
                    best = (i, j, cost);
                }
            }
        }
        let (i, j, _) = best;
        let union = self.rects[i].union(&self.rects[j]);
        self.rects.swap_remove(j);
        self.rects[i] = union;
    }

    /// The damaged units of each row, as `(row, first column, end column)`
    /// runs in row order — what a line-oriented writer (a terminal) visits.
    #[must_use]
    pub fn row_spans(&self) -> Vec<(i32, i32, i32)> {
        let mut spans: Vec<(i32, i32, i32)> = Vec::new();
        for rect in &self.rects {
            for row in rect.y..rect.bottom() {
                spans.push((row, rect.x, rect.right()));
            }
        }
        spans.sort_unstable();
        let mut merged: Vec<(i32, i32, i32)> = Vec::with_capacity(spans.len());
        for span in spans {
            match merged.last_mut() {
                Some(last) if last.0 == span.0 && span.1 <= last.2 => last.2 = last.2.max(span.2),
                _ => merged.push(span),
            }
        }
        merged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn far_apart_damage_stays_apart() {
        let mut damage = Region::new();
        damage.add(GridRect::new(0, 0, 2, 1));
        damage.add(GridRect::new(70, 20, 2, 1));
        assert_eq!(damage.rects().len(), 2);
        assert_eq!(damage.area(), 4);
    }

    #[test]
    fn a_covering_rectangle_absorbs_what_it_covers() {
        let mut damage = Region::new();
        damage.add(GridRect::new(1, 1, 1, 1));
        damage.add(GridRect::new(5, 5, 1, 1));
        damage.add(GridRect::new(0, 0, 10, 10));
        assert_eq!(damage.rects(), &[GridRect::new(0, 0, 10, 10)]);
    }

    #[test]
    fn the_count_is_bounded_and_nothing_is_lost() {
        let mut damage = Region::new();
        let mut added = Vec::new();
        for i in 0..100 {
            let rect = GridRect::new((i * 7) % 80, (i * 3) % 24, 1, 1);
            added.push(rect);
            damage.add(rect);
        }
        assert!(damage.rects().len() <= Region::MAX_RECTS);
        for rect in added {
            assert!(
                damage.rects().iter().any(|r| r.contains(rect.x, rect.y)),
                "{rect:?} was dropped"
            );
        }
    }

    #[test]
    fn row_spans_merge_per_row() {
        let mut damage = Region::new();
        damage.add(GridRect::new(0, 0, 3, 2));
        damage.add(GridRect::new(10, 1, 2, 1));
        assert_eq!(damage.row_spans(), vec![(0, 0, 3), (1, 0, 3), (1, 10, 12)]);
        assert_eq!(damage.bounds(), GridRect::new(0, 0, 12, 2));
        assert!(!damage.take().is_empty());
        assert!(damage.is_empty());
    }

    #[test]
    fn empty_damage_is_ignored() {
        let mut damage = Region::new();
        damage.add(GridRect::new(3, 3, 0, 9));
        assert!(damage.is_empty());
    }
}
