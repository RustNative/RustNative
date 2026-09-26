//! Conflict-free replicated data types: counters, a register, a set, a
//! map, and a sequence for lists and collaborative text. Each merges in any
//! order, any grouping, any number of times, to the same value — the
//! property tests check commutativity, associativity, and idempotence.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::clock::{Hlc, ReplicaId};

/// A replicated value.
pub trait Crdt: Clone {
    /// Folds `other`'s state into this one.
    fn merge(&mut self, other: &Self);
}

/// A counter that only grows.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GCounter(BTreeMap<ReplicaId, u64>);

impl GCounter {
    /// Adds `by` for `replica`.
    pub fn increment(&mut self, replica: ReplicaId, by: u64) {
        let entry = self.0.entry(replica).or_default();
        *entry = entry.saturating_add(by);
    }

    /// The total.
    #[must_use]
    pub fn value(&self) -> u64 {
        self.0.values().copied().fold(0, u64::saturating_add)
    }
}

impl Crdt for GCounter {
    fn merge(&mut self, other: &Self) {
        for (replica, count) in &other.0 {
            let entry = self.0.entry(*replica).or_default();
            *entry = (*entry).max(*count);
        }
    }
}

/// A counter that goes up and down.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PnCounter {
    up: GCounter,
    down: GCounter,
}

impl PnCounter {
    /// Adds `by` (negative to subtract) for `replica`.
    pub fn add(&mut self, replica: ReplicaId, by: i64) {
        if by >= 0 {
            self.up.increment(replica, by.unsigned_abs());
        } else {
            self.down.increment(replica, by.unsigned_abs());
        }
    }

    /// The value.
    #[must_use]
    pub fn value(&self) -> i64 {
        let up = i64::try_from(self.up.value()).unwrap_or(i64::MAX);
        let down = i64::try_from(self.down.value()).unwrap_or(i64::MAX);
        up.saturating_sub(down)
    }
}

impl Crdt for PnCounter {
    fn merge(&mut self, other: &Self) {
        self.up.merge(&other.up);
        self.down.merge(&other.down);
    }
}

/// A single value; the latest write wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LwwRegister<T> {
    value: T,
    stamp: Hlc,
}

impl<T> LwwRegister<T> {
    /// A register holding `value`, written at `stamp`.
    pub const fn new(value: T, stamp: Hlc) -> Self {
        Self { value, stamp }
    }

    /// Writes `value` at `stamp`, if that is later than the current write.
    pub fn set(&mut self, value: T, stamp: Hlc) {
        if stamp > self.stamp {
            self.value = value;
            self.stamp = stamp;
        }
    }

    /// The value.
    pub const fn get(&self) -> &T {
        &self.value
    }

    /// When it was written.
    pub const fn stamp(&self) -> Hlc {
        self.stamp
    }
}

impl<T: Clone> Crdt for LwwRegister<T> {
    fn merge(&mut self, other: &Self) {
        if other.stamp > self.stamp {
            self.value = other.value.clone();
            self.stamp = other.stamp;
        }
    }
}

/// A unique tag for one add.
pub type Dot = (ReplicaId, u64);

/// A set where an add wins over a concurrent remove (observed-remove).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrSet<T: Ord> {
    adds: BTreeMap<T, BTreeSet<Dot>>,
    removed: BTreeSet<Dot>,
    counters: BTreeMap<ReplicaId, u64>,
}

impl<T: Ord> Default for OrSet<T> {
    fn default() -> Self {
        Self { adds: BTreeMap::new(), removed: BTreeSet::new(), counters: BTreeMap::new() }
    }
}

impl<T: Ord + Clone> OrSet<T> {
    /// Adds `value` on `replica`.
    pub fn add(&mut self, replica: ReplicaId, value: T) {
        let counter = self.counters.entry(replica).or_default();
        *counter += 1;
        self.adds.entry(value).or_default().insert((replica, *counter));
    }

    /// Removes `value` as this replica sees it: adds made concurrently
    /// elsewhere survive.
    pub fn remove(&mut self, value: &T) {
        if let Some(dots) = self.adds.get(value) {
            self.removed.extend(dots.iter().copied());
        }
    }

    /// Whether `value` is present.
    #[must_use]
    pub fn contains(&self, value: &T) -> bool {
        self.adds.get(value).is_some_and(|dots| dots.iter().any(|dot| !self.removed.contains(dot)))
    }

    /// The present values, in order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.adds
            .iter()
            .filter(|(_, dots)| dots.iter().any(|dot| !self.removed.contains(dot)))
            .map(|(value, _)| value)
    }
}

impl<T: Ord + Clone> Crdt for OrSet<T> {
    fn merge(&mut self, other: &Self) {
        for (value, dots) in &other.adds {
            self.adds.entry(value.clone()).or_default().extend(dots.iter().copied());
        }
        self.removed.extend(other.removed.iter().copied());
        for (replica, counter) in &other.counters {
            let entry = self.counters.entry(*replica).or_default();
            *entry = (*entry).max(*counter);
        }
    }
}

/// A map whose entries are last-writer-wins registers; a removal is a
/// write of nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LwwMap<K: Ord, V> {
    entries: BTreeMap<K, LwwRegister<Option<V>>>,
}

impl<K: Ord, V> Default for LwwMap<K, V> {
    fn default() -> Self {
        Self { entries: BTreeMap::new() }
    }
}

impl<K: Ord + Clone, V: Clone> LwwMap<K, V> {
    /// Sets `key` to `value` at `stamp`.
    pub fn insert(&mut self, key: K, value: V, stamp: Hlc) {
        self.write(key, Some(value), stamp);
    }

    /// Removes `key` at `stamp`.
    pub fn remove(&mut self, key: K, stamp: Hlc) {
        self.write(key, None, stamp);
    }

    fn write(&mut self, key: K, value: Option<V>, stamp: Hlc) {
        match self.entries.get_mut(&key) {
            Some(register) => register.set(value, stamp),
            None => {
                self.entries.insert(key, LwwRegister::new(value, stamp));
            }
        }
    }

    /// The value at `key`.
    #[must_use]
    pub fn get(&self, key: &K) -> Option<&V> {
        self.entries.get(key)?.get().as_ref()
    }

    /// The present entries, in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries.iter().filter_map(|(key, register)| Some((key, register.get().as_ref()?)))
    }
}

impl<K: Ord + Clone, V: Clone> Crdt for LwwMap<K, V> {
    fn merge(&mut self, other: &Self) {
        for (key, register) in &other.entries {
            match self.entries.get_mut(key) {
                Some(mine) => mine.merge(register),
                None => {
                    self.entries.insert(key.clone(), register.clone());
                }
            }
        }
    }
}

/// An element's identity in a sequence: when and where it was inserted.
pub type ElementId = Hlc;

/// One element of an [`Rga`], as it serializes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Element<T> {
    id: ElementId,
    after: Option<ElementId>,
    value: T,
    deleted: bool,
}

/// A replicated sequence (RGA): lists and collaborative text. Concurrent
/// inserts at the same place are ordered by their ids, the same way on
/// every replica. It serializes as a list of elements (JSON object keys
/// must be strings; element ids are not).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Vec<Element<T>>", into = "Vec<Element<T>>")]
#[serde(bound(serialize = "T: Serialize + Clone", deserialize = "T: Deserialize<'de>"))]
pub struct Rga<T: Clone> {
    elements: BTreeMap<ElementId, Element<T>>,
}

impl<T: Clone> From<Vec<Element<T>>> for Rga<T> {
    fn from(elements: Vec<Element<T>>) -> Self {
        Self { elements: elements.into_iter().map(|element| (element.id, element)).collect() }
    }
}

impl<T: Clone> From<Rga<T>> for Vec<Element<T>> {
    fn from(rga: Rga<T>) -> Self {
        rga.elements.into_values().collect()
    }
}

impl<T: Clone> Default for Rga<T> {
    fn default() -> Self {
        Self { elements: BTreeMap::new() }
    }
}

impl<T: Clone> Rga<T> {
    /// Inserts `value` after the element `after` (`None`: at the start),
    /// with the new element's `id`.
    pub fn insert_after(&mut self, after: Option<ElementId>, value: T, id: ElementId) {
        self.elements.entry(id).or_insert(Element { id, after, value, deleted: false });
    }

    /// Inserts `value` at visible position `index`.
    pub fn insert(&mut self, index: usize, value: T, id: ElementId) {
        let after =
            index.checked_sub(1).and_then(|previous| self.visible_ids().get(previous).copied());
        self.insert_after(after, value, id);
    }

    /// Deletes the visible element at `index`.
    pub fn delete(&mut self, index: usize) {
        if let Some(id) = self.visible_ids().get(index).copied() {
            if let Some(element) = self.elements.get_mut(&id) {
                element.deleted = true;
            }
        }
    }

    fn ordered(&self) -> Vec<ElementId> {
        let mut children: BTreeMap<Option<ElementId>, Vec<ElementId>> = BTreeMap::new();
        for element in self.elements.values() {
            children.entry(element.after).or_default().push(element.id);
        }
        for siblings in children.values_mut() {
            // The newest insert at a place comes first.
            siblings.sort_by(|a, b| b.cmp(a));
        }
        let mut order = Vec::with_capacity(self.elements.len());
        let mut stack: Vec<ElementId> =
            children.get(&None).cloned().unwrap_or_default().into_iter().rev().collect();
        while let Some(id) = stack.pop() {
            order.push(id);
            if let Some(next) = children.get(&Some(id)) {
                stack.extend(next.iter().rev().copied());
            }
        }
        order
    }

    fn visible_ids(&self) -> Vec<ElementId> {
        self.ordered()
            .into_iter()
            .filter(|id| self.elements.get(id).is_some_and(|element| !element.deleted))
            .collect()
    }

    /// The visible values, in order.
    #[must_use]
    pub fn to_vec(&self) -> Vec<T> {
        self.visible_ids()
            .iter()
            .filter_map(|id| self.elements.get(id).map(|element| element.value.clone()))
            .collect()
    }
}

impl Rga<char> {
    /// The text.
    #[must_use]
    pub fn text(&self) -> String {
        self.to_vec().into_iter().collect()
    }
}

impl<T: Clone> Crdt for Rga<T> {
    fn merge(&mut self, other: &Self) {
        for (id, element) in &other.elements {
            match self.elements.get_mut(id) {
                Some(mine) => mine.deleted |= element.deleted,
                None => {
                    self.elements.insert(*id, element.clone());
                }
            }
        }
    }
}
