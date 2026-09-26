//! Stable identity primitives.
//!
//! This module is deliberately small and self-contained: every other module
//! that needs a framework identity (components, nodes, windows, tasks)
//! depends on it, but it depends on nothing else in the crate. That makes it
//! the natural place to enforce the single invariant the rest of the
//! reconciliation pipeline is built on — **two different declarative keys
//! must never resolve to the same [`NodeId`]** — in one location instead of
//! scattered across reconciliation code.
//!
//! # Why interning, not hashing
//!
//! An earlier design derived [`NodeId::from_key`] from an FNV-1a hash of the
//! key string. A hash digest is compact and stateless, but it is
//! fundamentally a many-to-one mapping: the pigeonhole principle guarantees
//! that *some* pair of distinct strings collides onto the same 64-bit value,
//! even though a real collision is astronomically unlikely for any one
//! application. That residual risk is exactly the kind of thing a framework
//! should design away rather than accept, especially since a collision would
//! silently merge two logically distinct UI nodes into one identity —
//! corrupting native object reuse and event routing without ever raising an
//! error a developer could act on.
//!
//! [`NodeId::from_key`] therefore uses a true interning table keyed by exact
//! string equality instead. Distinct strings are *always* assigned distinct
//! integers — there is no hash, so there is no collision to worry about,
//! full stop. The cost is that the table must be retained for the life of
//! the process (an id, once assigned, can never be reused for a different
//! key, which is what gives the "never collides" guarantee its teeth) and
//! that interning is a call into shared, thread-local state rather than a
//! pure function of the bytes. Both costs are well worth paying: keys are
//! expected to be a small, stable, developer-authored vocabulary (the same
//! assumption React, `SwiftUI`, and similar frameworks make about `key`/`id`),
//! not a per-render-unique value, so the table stays small in practice.
//!
//! # Why component-scoped, not globally unique
//!
//! A single interned key is only unique among *other keys*, not among every
//! node any component in the application might ever create — two unrelated,
//! independently reusable components are still allowed to each use the
//! ordinary key `"submit"`. `NodeId::scoped` combines an interned local key
//! with the owning [`ComponentId`] to produce the identity a platform
//! backend actually sees, so reuse across components can never alias either:
//! the upper bits carry the component identity (itself an allocator that can
//! never repeat a live value — see `ComponentId::next`) and the lower bits
//! carry the interned, collision-free local key.

use std::cell::RefCell;
use std::collections::HashMap;

/// Stable, opaque identity for a UI node as seen by a platform backend.
///
/// Application and component code never constructs a [`NodeId`] used for
/// native realization directly; it authors [`crate::Node`]s with an ordinary
/// string key (see [`NodeId::from_key`]), and the component runtime scopes
/// that local key against the owning component (see `NodeId::scoped`)
/// before the tree is handed to a platform backend. The two-level model is
/// intentional: a component's local key is the only part an application
/// author has to reason about, while the runtime-assigned global identity is
/// what backends and reconciliation actually key native/tree state by.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct NodeId(u128);

thread_local! {
    static KEY_INTERNER: RefCell<KeyInterner> = RefCell::new(KeyInterner::new());
}

/// Backing table for [`NodeId::from_key`]. Thread-local rather than a
/// process-wide `Mutex`/`RwLock` because every type that touches a `NodeId`
/// through the declarative tree (`Node`, `ComponentTree`, ...) is already
/// `Rc`-based and therefore confined to a single thread; a lock would only
/// add contention with no corresponding safety benefit.
struct KeyInterner {
    ids: HashMap<Box<str>, u64>,
    /// The reverse of `ids`, indexed by interned id — what lets a
    /// diagnostic (a golden file, the inspector, a failed test query) name
    /// a node by the key its author wrote rather than by a number.
    keys: Vec<Box<str>>,
    next: u64,
}

impl KeyInterner {
    fn new() -> Self {
        Self { ids: HashMap::new(), keys: Vec::new(), next: 0 }
    }

    #[allow(
        clippy::expect_used,
        reason = "the standards audit's P1.20 finding requires an identity allocator to fail loudly on \
    /// exhaustion rather than silently wrap and reuse a live id; there is no caller that \
    /// could act on a `Result` here"
    )]
    fn intern(&mut self, key: &str) -> u64 {
        if let Some(&id) = self.ids.get(key) {
            return id;
        }
        let id = self.next;
        self.next = self.next.checked_add(1).expect(
            "local node-key interner exhausted: more than u64::MAX distinct node keys have \
             been used by this process. This almost always means keys are being generated \
             from an unbounded per-render value (a counter, a timestamp, a random id) \
             instead of a stable, small, developer-authored key vocabulary — see \
             `NodeId::from_key` for the identity model this interner backs.",
        );
        self.ids.insert(key.into(), id);
        self.keys.push(key.into());
        id
    }

    fn key(&self, id: u64) -> Option<&str> {
        usize::try_from(id).ok().and_then(|index| self.keys.get(index)).map(AsRef::as_ref)
    }
}

impl NodeId {
    /// Creates a component-local node identity from a stable application
    /// key.
    ///
    /// Distinct key strings are *always* assigned distinct identities:
    /// see the module-level documentation for why this is a true interning
    /// table rather than a hash, and therefore has no collision risk at all
    /// for distinct inputs, no matter how many keys an application uses over
    /// its lifetime.
    #[must_use]
    pub fn from_key(key: &str) -> Self {
        let interned = KEY_INTERNER.with(|interner| interner.borrow_mut().intern(key));
        Self(u128::from(interned))
    }

    /// Combines a component-local identity with its owning component,
    /// producing the identity a platform backend and the reconciliation
    /// pipeline actually use.
    ///
    /// The owner's bits and the local key's bits occupy disjoint halves of
    /// the value (rather than being combined through arithmetic or hashing),
    /// so the result is unique whenever *either* input differs — no new
    /// collision can be introduced by this step, matching the "no hash is
    /// treated as uniqueness without collision handling" principle applied
    /// throughout this module.
    pub(crate) const fn scoped(owner: ComponentId, local: Self) -> Self {
        Self(((owner.0 as u128) << 64) | (local.0 & (u64::MAX as u128)))
    }

    /// The key this node was created with — the component-local part of its
    /// identity, as its author wrote it — if it was interned on this thread.
    ///
    /// For diagnostics only: a golden file, an inspector, or a failed test
    /// query names a node this way. Identity itself is the id, never the
    /// string.
    ///
    /// ```
    /// use rustnative_core::NodeId;
    ///
    /// assert_eq!(NodeId::from_key("submit").local_key().as_deref(), Some("submit"));
    /// ```
    #[must_use]
    pub fn local_key(self) -> Option<String> {
        #[allow(clippy::cast_possible_truncation, reason = "masked to the low 64 bits first")]
        let local = (self.0 & u128::from(u64::MAX)) as u64;
        KEY_INTERNER.with(|interner| interner.borrow().key(local).map(str::to_owned))
    }

    /// The component that owns this node, or `None` for a node the root
    /// component rendered (whose identity is its bare key).
    #[must_use]
    pub fn owner(self) -> Option<ComponentId> {
        #[allow(clippy::cast_possible_truncation, reason = "the high half of a u128 fits a u64")]
        let owner = (self.0 >> 64) as u64;
        (owner != 0).then_some(ComponentId(owner))
    }

    /// Returns the raw numeric value. Exposed for backends that need a
    /// stable, hashable, FFI-friendly representation (for example, as a key
    /// into a native object registry or a diagnostic log), not for
    /// application code to construct or compare identities out-of-band.
    #[must_use]
    pub const fn get(self) -> u128 {
        self.0
    }
}

/// Stable identity for an entry in the framework-managed component tree.
///
/// Allocated sequentially by `ComponentId::next` and never reused within
/// one [`crate::ComponentTree`]'s lifetime, which is what lets
/// `NodeId::scoped` guarantee no two live components' nodes can alias.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct ComponentId(u64);

impl ComponentId {
    /// The identity of a tree's root component. Chosen away from zero/small
    /// integers so it does not collide with the low end of the sequential
    /// allocator used for every other component, even though the allocator
    /// starting at `1` already makes that impossible on its own — this is
    /// belt-and-braces, not load-bearing.
    pub const ROOT: Self = Self(0x6a09_e667_f3bc_c909);

    /// Returns the identity's raw numeric value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The identity with raw value `raw`, as [`Self::get`] gave it (for a
    /// node key that crossed the wire; see `crate::wire`).
    pub(crate) const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Allocates the next sequential id from `counter`, advancing it.
    ///
    /// # Panics
    ///
    /// Panics if the counter has been exhausted (`u64::MAX` components
    /// created by one tree over its lifetime). This is intentional: per the
    /// standards audit (P1.20), an identity allocator must never silently
    /// wrap and risk reusing a live id — it must fail loudly, and at this
    /// magnitude "loudly" cannot realistically be observed in practice.
    #[allow(
        clippy::expect_used,
        reason = "the standards audit's P1.20 finding requires an identity allocator to fail loudly on \
    /// exhaustion rather than silently wrap and reuse a live id; there is no caller that \
    /// could act on a `Result` here"
    )]
    pub(crate) fn next(counter: &mut u64) -> Self {
        let id = Self(*counter);
        *counter = counter.checked_add(1).expect(
            "framework component identity space exhausted (more than u64::MAX components \
                      were created by one ComponentTree over its lifetime)",
        );
        id
    }
}

/// Stable identity for one native top-level window.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct WindowId(u64);

impl WindowId {
    /// The application's initial window, created by every `Application`
    /// constructor.
    pub const PRIMARY: Self = Self(0);

    /// Returns the identity's raw numeric value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Allocates the next sequential id from `counter`, advancing it.
    ///
    /// # Panics
    ///
    /// Panics on exhaustion, for the same reason as `ComponentId::next`.
    #[allow(
        clippy::expect_used,
        reason = "the standards audit's P1.20 finding requires an identity allocator to fail loudly on \
    /// exhaustion rather than silently wrap and reuse a live id; there is no caller that \
    /// could act on a `Result` here"
    )]
    pub(crate) fn next(counter: &mut u64) -> Self {
        let id = Self(*counter);
        *counter = counter.checked_add(1).expect(
            "framework window identity space exhausted (more than u64::MAX windows were \
                      created by one Application over its lifetime)",
        );
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_key_is_stable_and_injective_for_distinct_keys() {
        let a1 = NodeId::from_key("submit");
        let a2 = NodeId::from_key("submit");
        let b = NodeId::from_key("cancel");
        assert_eq!(a1, a2, "the same key must always intern to the same id");
        assert_ne!(a1, b, "distinct keys must never intern to the same id");
    }

    #[test]
    fn from_key_has_no_hash_collision_risk_for_a_large_key_set() {
        // A regression guard for the exact defect the standards audit
        // flagged (P0.1): with the previous FNV-1a-hash-based
        // implementation this test would still almost certainly pass (a
        // real collision in a few thousand keys is still very unlikely),
        // so its value is less "proves no collision happened this run" and
        // more "documents, in a runnable form, the property the interning
        // design makes *structurally impossible* rather than merely
        // unlikely."
        let mut seen = std::collections::HashSet::new();
        for i in 0..10_000u32 {
            let key = format!("node-{i}");
            let id = NodeId::from_key(&key);
            assert!(seen.insert(id), "key {key:?} collided with a previously interned key");
        }
    }

    #[test]
    fn scoped_ids_never_alias_across_different_owners_for_the_same_local_key() {
        let local = NodeId::from_key("submit");
        let owner_a = ComponentId::next(&mut 1);
        let owner_b = ComponentId::next(&mut 2);
        assert_ne!(
            NodeId::scoped(owner_a, local),
            NodeId::scoped(owner_b, local),
            "two different components must be able to reuse the same local key \
             without their scoped identities colliding"
        );
    }

    #[test]
    fn component_id_allocator_never_reuses_a_value() {
        let mut counter = 0u64;
        let a = ComponentId::next(&mut counter);
        let b = ComponentId::next(&mut counter);
        assert_ne!(a, b);
        assert_ne!(a, ComponentId::ROOT);
        assert_ne!(b, ComponentId::ROOT);
    }

    #[test]
    #[should_panic(expected = "identity space exhausted")]
    fn component_id_allocator_panics_instead_of_wrapping_on_exhaustion() {
        let mut counter = u64::MAX;
        let _ = ComponentId::next(&mut counter);
    }

    #[test]
    #[should_panic(expected = "identity space exhausted")]
    fn window_id_allocator_panics_instead_of_wrapping_on_exhaustion() {
        let mut counter = u64::MAX;
        let _ = WindowId::next(&mut counter);
    }
}
