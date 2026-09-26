//! Every replicated type's merge is commutative, associative, and
//! idempotent (`PLAN.md` Milestone 55): replicas that saw the same
//! operations agree, whatever order the merges came in.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use proptest::prelude::*;
use rustnative_sync::{Crdt, GCounter, Hlc, LwwMap, LwwRegister, OrSet, PnCounter, Rga};

fn stamp(wall: u64, replica: u64) -> Hlc {
    Hlc { wall, counter: 0, replica }
}

/// The three laws, for any `Crdt` with equality.
fn laws<T: Crdt + PartialEq + std::fmt::Debug>(a: &T, b: &T, c: &T) {
    let merged = |x: &T, y: &T| {
        let mut out = x.clone();
        out.merge(y);
        out
    };
    assert_eq!(merged(a, b), merged(b, a), "commutative");
    assert_eq!(merged(&merged(a, b), c), merged(a, &merged(b, c)), "associative");
    assert_eq!(merged(a, a), *a, "idempotent");
}

/// One replica's operations: (kind, value), applied in order.
fn operations() -> impl Strategy<Value = Vec<(u8, u8)>> {
    prop::collection::vec((0u8..3, 0u8..6), 0..12)
}

fn counters(replica: u64, ops: &[(u8, u8)]) -> (GCounter, PnCounter) {
    let (mut grow, mut both) = (GCounter::default(), PnCounter::default());
    for (kind, value) in ops {
        grow.increment(replica, u64::from(*value));
        both.add(replica, if *kind == 0 { -i64::from(*value) } else { i64::from(*value) });
    }
    (grow, both)
}

fn set(replica: u64, ops: &[(u8, u8)]) -> OrSet<u8> {
    let mut set = OrSet::default();
    for (kind, value) in ops {
        if *kind == 0 { set.remove(value) } else { set.add(replica, *value) }
    }
    set
}

fn map(replica: u64, ops: &[(u8, u8)]) -> LwwMap<u8, u8> {
    let mut map = LwwMap::default();
    for (index, (kind, value)) in ops.iter().enumerate() {
        let at = stamp(u64::try_from(index).unwrap() * 3 + u64::from(*kind), replica);
        if *kind == 0 { map.remove(*value, at) } else { map.insert(*value, *kind, at) }
    }
    map
}

fn sequence(replica: u64, ops: &[(u8, u8)]) -> Rga<char> {
    let mut text = Rga::default();
    for (index, (kind, value)) in ops.iter().enumerate() {
        let length = text.to_vec().len();
        if *kind == 0 && length > 0 {
            text.delete(usize::from(*value) % length);
        } else {
            let at = stamp(u64::try_from(index).unwrap() + 1, replica);
            text.insert(usize::from(*value) % (length + 1), char::from(b'a' + value), at);
        }
    }
    text
}

proptest! {
    #[test]
    fn counters_merge_lawfully(a in operations(), b in operations(), c in operations()) {
        let (a, b, c) = (counters(1, &a), counters(2, &b), counters(3, &c));
        laws(&a.0, &b.0, &c.0);
        laws(&a.1, &b.1, &c.1);
    }

    #[test]
    fn sets_merge_lawfully(a in operations(), b in operations(), c in operations()) {
        laws(&set(1, &a), &set(2, &b), &set(3, &c));
    }

    #[test]
    fn maps_merge_lawfully(a in operations(), b in operations(), c in operations()) {
        laws(&map(1, &a), &map(2, &b), &map(3, &c));
    }

    #[test]
    fn sequences_merge_lawfully(a in operations(), b in operations(), c in operations()) {
        let (a, b, c) = (sequence(1, &a), sequence(2, &b), sequence(3, &c));
        laws(&a, &b, &c);
        // And the visible text agrees too, whatever the order.
        let mut ab = a.clone();
        ab.merge(&b);
        let mut ba = b.clone();
        ba.merge(&a);
        prop_assert_eq!(ab.text(), ba.text());
    }

    #[test]
    fn registers_merge_lawfully(x in 0u64..20, y in 0u64..20, z in 0u64..20) {
        laws(&LwwRegister::new(1, stamp(x, 1)), &LwwRegister::new(2, stamp(y, 2)), &LwwRegister::new(3, stamp(z, 3)));
    }
}

#[test]
fn concurrent_edits_to_text_interleave_the_same_everywhere() {
    let mut ada = Rga::default();
    for (index, character) in "cat".chars().enumerate() {
        ada.insert(index, character, stamp(u64::try_from(index).unwrap() + 1, 1));
    }
    let mut grace = ada.clone();
    ada.insert(3, 's', stamp(10, 1)); // cats
    grace.insert(0, 'a', stamp(11, 2)); // acat
    grace.delete(1); // aat
    let mut left = ada.clone();
    left.merge(&grace);
    let mut right = grace.clone();
    right.merge(&ada);
    assert_eq!(left.text(), right.text());
    assert_eq!(left.text(), "aats");
}

#[test]
fn an_add_survives_a_concurrent_remove() {
    let mut ada = OrSet::default();
    ada.add(1, "milk");
    let mut grace = ada.clone();
    grace.remove(&"milk");
    ada.add(1, "milk"); // Ada added it again, concurrently.
    grace.merge(&ada);
    assert!(grace.contains(&"milk"));
}

#[test]
fn every_type_travels_as_json() {
    let mut text = Rga::default();
    text.insert(0, 'a', stamp(1, 1));
    let back: Rga<char> = serde_json::from_value(serde_json::to_value(&text).unwrap()).unwrap();
    assert_eq!(back, text);
    let mut set = OrSet::default();
    set.add(1, 'x');
    let back: OrSet<char> = serde_json::from_value(serde_json::to_value(&set).unwrap()).unwrap();
    assert_eq!(back, set);
}
