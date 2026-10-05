//! List infrastructure (`PLAN.md` Milestone 48, `C26`): sort, filter, and
//! group as views over a source that never copy it, and the identity diff
//! that turns two orders of keyed rows into inserts, removals, and moves.
//!
//! A [`Projection`] is the source plus a vector of indices into it. Sorting
//! and filtering rearrange the indices; grouping splits them. The rows
//! themselves stay where they are, so a view over 200 000 rows costs
//! 200 000 `usize`s, not 200 000 clones.
//!
//! ```
//! use rustnative_data::list::Projection;
//!
//! let people = [("Ada", 36), ("Grace", 85), ("Alan", 41), ("Barbara", 30)];
//! let view = Projection::new(&people)
//!     .filter(|(_, age)| *age < 50)
//!     .sort_by_key(|(name, _)| *name);
//! let names: Vec<_> = view.iter().map(|(name, _)| *name).collect();
//! assert_eq!(names, ["Ada", "Alan", "Barbara"]);
//!
//! let sections = view.group_by(|(name, _)| name.chars().next());
//! assert_eq!(sections.len(), 2);
//! assert_eq!(sections[0].key, Some('A'));
//! assert_eq!(sections[0].rows.len(), 2);
//! ```
//!
//! [`diff_keys`] compares the keys a list showed with the keys it shows
//! now. Rows rendered with those keys are already reconciled by identity;
//! the diff is what a list needs to animate the change — a row that moved
//! can slide (a `Position` transition on the row does that natively), an
//! inserted one can fade in, a removed one can fade out.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::Hash;

/// A non-copying view over `source`: which of its rows, in which order.
#[derive(Debug, Clone)]
pub struct Projection<'a, T> {
    source: &'a [T],
    order: Vec<usize>,
}

impl<'a, T> Projection<'a, T> {
    /// Every row of `source`, in source order.
    #[must_use]
    pub fn new(source: &'a [T]) -> Self {
        Self { source, order: (0..source.len()).collect() }
    }

    /// Only the rows `keep` accepts, in the current order.
    #[must_use]
    pub fn filter(mut self, mut keep: impl FnMut(&T) -> bool) -> Self {
        let source = self.source;
        self.order.retain(|index| keep(&source[*index]));
        self
    }

    /// The rows sorted by `compare`; equal rows keep their current order.
    #[must_use]
    pub fn sort_by(mut self, mut compare: impl FnMut(&T, &T) -> Ordering) -> Self {
        let source = self.source;
        self.order.sort_by(|a, b| compare(&source[*a], &source[*b]));
        self
    }

    /// The rows sorted by the key `key` gives them (stable).
    #[must_use]
    pub fn sort_by_key<K: Ord>(self, mut key: impl FnMut(&T) -> K) -> Self {
        self.sort_by(|a, b| key(a).cmp(&key(b)))
    }

    /// The rows reversed.
    #[must_use]
    pub fn reversed(mut self) -> Self {
        self.order.reverse();
        self
    }

    /// Splits the rows into sections of consecutive rows with the same key,
    /// in the current order — sort by the key first for one section per key.
    #[must_use]
    pub fn group_by<K: PartialEq>(&self, mut key: impl FnMut(&T) -> K) -> Vec<Section<'a, T, K>> {
        let mut sections: Vec<Section<'a, T, K>> = Vec::new();
        for &index in &self.order {
            let row_key = key(&self.source[index]);
            match sections.last_mut() {
                Some(section) if section.key == row_key => section.rows.order.push(index),
                _ => sections.push(Section {
                    key: row_key,
                    rows: Projection { source: self.source, order: vec![index] },
                }),
            }
        }
        sections
    }

    /// How many rows the view shows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// Whether the view shows no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// The row at `position` in the view.
    #[must_use]
    pub fn get(&self, position: usize) -> Option<&'a T> {
        self.order.get(position).map(|index| &self.source[*index])
    }

    /// The source index of each row, in view order.
    #[must_use]
    pub fn indices(&self) -> &[usize] {
        &self.order
    }

    /// The rows, in view order.
    pub fn iter(&self) -> impl Iterator<Item = &'a T> + '_ {
        self.order.iter().map(|index| &self.source[*index])
    }
}

/// One group of a [`Projection::group_by`].
#[derive(Debug, Clone)]
pub struct Section<'a, T, K> {
    /// What the rows have in common.
    pub key: K,
    /// The rows, still a view over the source.
    pub rows: Projection<'a, T>,
}

/// One step from an old order of keys to a new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListChange {
    /// The row at `index` of the old order is gone.
    Remove {
        /// Its position in the old order.
        index: usize,
    },
    /// A new row is at `index` of the new order.
    Insert {
        /// Its position in the new order.
        index: usize,
    },
    /// A row that is in both orders changed its place relative to the
    /// others.
    Move {
        /// Its position in the old order.
        from: usize,
        /// Its position in the new order.
        to: usize,
    },
}

/// What changed between two orders of unique keys: the rows removed, the
/// rows inserted, and the fewest rows that must be said to have moved (the
/// rest keep their relative order — the longest increasing subsequence).
#[must_use]
pub fn diff_keys<K: Eq + Hash>(previous: &[K], next: &[K]) -> Vec<ListChange> {
    let old_position: HashMap<&K, usize> =
        previous.iter().enumerate().map(|(index, key)| (key, index)).collect();
    let new_keys: HashMap<&K, usize> =
        next.iter().enumerate().map(|(index, key)| (key, index)).collect();
    let mut changes: Vec<ListChange> = previous
        .iter()
        .enumerate()
        .filter(|(_, key)| !new_keys.contains_key(key))
        .map(|(index, _)| ListChange::Remove { index })
        .collect();

    // Rows present in both, in new order, with their old positions.
    let kept: Vec<(usize, usize)> = next
        .iter()
        .enumerate()
        .filter_map(|(to, key)| old_position.get(key).map(|from| (*from, to)))
        .collect();
    let stays = longest_increasing(&kept.iter().map(|(from, _)| *from).collect::<Vec<_>>());
    for (position, (from, to)) in kept.iter().enumerate() {
        if !stays[position] {
            changes.push(ListChange::Move { from: *from, to: *to });
        }
    }
    changes.extend(
        next.iter()
            .enumerate()
            .filter(|(_, key)| !old_position.contains_key(key))
            .map(|(index, _)| ListChange::Insert { index }),
    );
    changes
}

/// Which elements of `values` belong to one longest strictly increasing
/// subsequence (patience sorting, `O(n log n)`).
fn longest_increasing(values: &[usize]) -> Vec<bool> {
    let mut tails: Vec<usize> = Vec::new(); // positions in `values`
    let mut previous = vec![usize::MAX; values.len()];
    for (position, value) in values.iter().enumerate() {
        let slot = tails.partition_point(|tail| values[*tail] < *value);
        if slot > 0 {
            previous[position] = tails[slot - 1];
        }
        if slot == tails.len() {
            tails.push(position);
        } else {
            tails[slot] = position;
        }
    }
    let mut member = vec![false; values.len()];
    let mut at = tails.last().copied();
    while let Some(position) = at {
        member[position] = true;
        at = (previous[position] != usize::MAX).then(|| previous[position]);
    }
    member
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_never_copy_rows() {
        let rows = vec![String::from("b"), String::from("a"), String::from("c")];
        let view = Projection::new(&rows).sort_by_key(Clone::clone);
        assert!(std::ptr::eq(view.get(0).unwrap(), &raw const rows[1]));
        assert_eq!(view.indices(), &[1, 0, 2]);
    }

    #[test]
    fn a_diff_says_what_was_removed_inserted_and_moved() {
        let changes = diff_keys(&["a", "b", "c", "d"], &["d", "a", "c", "e"]);
        assert_eq!(
            changes,
            vec![
                ListChange::Remove { index: 1 },
                ListChange::Move { from: 3, to: 0 },
                ListChange::Insert { index: 3 },
            ]
        );
        assert_eq!(diff_keys(&[1, 2, 3], &[1, 2, 3]), []);
    }
}
