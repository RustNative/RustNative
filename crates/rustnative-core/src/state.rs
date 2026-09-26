//! Shared state (`PLAN.md` Milestone 47): a [`Store`] two distant
//! components can both read, provided to a subtree rather than made global,
//! and read by *slice* so a component re-renders only when the part it
//! selected changes (`C08`).
//!
//! # The contract
//!
//! - **Scoped.** A component provides a store to its descendants with
//!   [`ComponentContext::provide_scoped`](crate::ComponentContext::provide_scoped);
//!   they find it with [`ComponentContext::scoped`](crate::ComponentContext::scoped).
//!   Nothing outside that subtree sees it, and it is dropped with its
//!   provider.
//! - **Observed by slice.** [`ComponentContext::select`](crate::ComponentContext::select)
//!   reads a value computed from the store and records the read. After an
//!   update, the selector runs again, and the component re-renders only when
//!   its result is unequal to what it saw.
//! - **Ordered.** Updates apply in the order they are made. An update made
//!   while another is running (from inside its closure) is queued and
//!   applied right after it, never interleaved.
//! - **Same lifetime discipline as component state.** A store is plain
//!   data behind a shared handle, owned by whoever holds it, on the UI
//!   thread (it is deliberately `!Send`).
//!
//! # Example
//!
//! ```
//! use rustnative_core::{Component, ComponentContext, ComponentTree, Event, Node, Store};
//!
//! #[derive(Clone, Default)]
//! struct Cart {
//!     items: Vec<String>,
//!     coupon: Option<String>,
//! }
//!
//! struct Shop;
//! impl Component for Shop {
//!     type Props = ();
//!     type Message = ();
//!     fn new((): ()) -> Self { Self }
//!     fn props(&self) -> &() { &() }
//!     fn set_props(&mut self, (): ()) {}
//!     fn view(&self) -> Node { Node::column("shop", []) }
//!     fn update(&mut self, _: Event) {}
//!     fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
//!         context.provide_scoped_with(|| Store::new("cart", Cart::default()));
//!         Node::column("shop", [context.child::<Badge>("badge")])
//!     }
//! }
//!
//! struct Badge;
//! impl Component for Badge {
//!     type Props = ();
//!     type Message = ();
//!     fn new((): ()) -> Self { Self }
//!     fn props(&self) -> &() { &() }
//!     fn set_props(&mut self, (): ()) {}
//!     fn view(&self) -> Node { Node::label("count", "") }
//!     fn update(&mut self, _: Event) {}
//!     fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
//!         let cart = context.scoped::<Store<Cart>>().expect("provided by Shop");
//!         // Re-renders when the number of items changes — not the coupon.
//!         let count = context.select(&cart, |cart| cart.items.len());
//!         Node::label("count", count.to_string())
//!     }
//! }
//!
//! let tree = ComponentTree::new(Shop);
//! # let _ = tree;
//!
//! // The badge's resting view in markup:
//! assert_eq!(rustnative_core::rsx! { <Label key="count" text="" /> }, Badge.view());
//! ```

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};

thread_local! {
    /// Bumped by every store update on this thread, so a component tree can
    /// tell in one comparison whether any selection might be stale.
    static EPOCH: Cell<u64> = const { Cell::new(0) };
    /// Every store made with [`Store::inspectable`], for the inspector.
    static REGISTRY: RefCell<Vec<Weak<dyn Inspectable>>> = const { RefCell::new(Vec::new()) };
}

/// The number of store updates made on this thread so far.
pub(crate) fn epoch() -> u64 {
    EPOCH.with(Cell::get)
}

type PendingUpdate<T> = Box<dyn FnOnce(&mut T)>;

struct Inner<T> {
    name: String,
    value: RefCell<T>,
    version: Cell<u64>,
    pending: RefCell<Vec<PendingUpdate<T>>>,
    applying: Cell<bool>,
    snapshot: Option<fn(&T) -> serde_json::Value>,
}

trait Inspectable {
    fn describe(&self) -> StoreSnapshot;
}

impl<T> Inspectable for Inner<T> {
    fn describe(&self) -> StoreSnapshot {
        StoreSnapshot {
            name: self.name.clone(),
            version: self.version.get(),
            value: self.snapshot.map_or(serde_json::Value::Null, |snapshot| {
                self.value.try_borrow().map_or(serde_json::Value::Null, |value| snapshot(&value))
            }),
        }
    }
}

/// One live store as the inspector sees it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoreSnapshot {
    /// The name it was made with.
    pub name: String,
    /// How many updates it has had.
    pub version: u64,
    /// Its value, serialized.
    pub value: serde_json::Value,
}

/// Every live store made with [`Store::inspectable`] on this thread.
#[must_use]
pub fn inspect_stores() -> Vec<StoreSnapshot> {
    REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        registry.retain(|store| store.strong_count() > 0);
        registry.iter().filter_map(Weak::upgrade).map(|store| store.describe()).collect()
    })
}

/// A shared, observable value; see the [module documentation](self).
///
/// Clones are handles to the same store, and compare equal only to each
/// other.
pub struct Store<T: 'static> {
    inner: Rc<Inner<T>>,
}

impl<T> Clone for Store<T> {
    fn clone(&self) -> Self {
        Self { inner: Rc::clone(&self.inner) }
    }
}

impl<T> PartialEq for Store<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl<T> fmt::Debug for Store<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Store")
            .field("name", &self.inner.name)
            .field("version", &self.inner.version.get())
            .finish_non_exhaustive()
    }
}

impl<T: 'static> Store<T> {
    /// A store named `name` (the name is for diagnostics) holding `value`.
    pub fn new(name: impl Into<String>, value: T) -> Self {
        Self {
            inner: Rc::new(Inner {
                name: name.into(),
                value: RefCell::new(value),
                version: Cell::new(0),
                pending: RefCell::new(Vec::new()),
                applying: Cell::new(false),
                snapshot: None,
            }),
        }
    }

    /// A store the inspector lists, with its value serialized
    /// (`rustnative inspect stores`).
    pub fn inspectable(name: impl Into<String>, value: T) -> Self
    where
        T: serde::Serialize,
    {
        let inner: Rc<Inner<T>> = Rc::new(Inner {
            name: name.into(),
            value: RefCell::new(value),
            version: Cell::new(0),
            pending: RefCell::new(Vec::new()),
            applying: Cell::new(false),
            snapshot: Some(|value| serde_json::to_value(value).unwrap_or_default()),
        });
        let weak: Weak<dyn Inspectable> = Rc::downgrade(&inner) as Weak<dyn Inspectable>;
        REGISTRY.with(|registry| registry.borrow_mut().push(weak));
        Self { inner }
    }

    /// The name it was made with.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// How many updates it has had.
    #[must_use]
    pub fn version(&self) -> u64 {
        self.inner.version.get()
    }

    /// Reads the value.
    ///
    /// # Panics
    ///
    /// If called from inside an [`Self::update`] closure of the same store
    /// (read the `&mut T` you were given instead).
    pub fn read<R>(&self, read: impl FnOnce(&T) -> R) -> R {
        read(&self.inner.value.borrow())
    }

    /// A copy of the value.
    #[must_use]
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self.read(T::clone)
    }

    /// Replaces the value.
    pub fn set(&self, value: T) {
        self.update(move |current| *current = value);
    }

    /// Changes the value. Components that selected a slice of it re-render
    /// on the tree's next pass if their slice changed.
    ///
    /// Called from inside another update of the same store, the change is
    /// queued and applied as soon as the running one finishes.
    pub fn update(&self, change: impl FnOnce(&mut T) + 'static) {
        if self.inner.applying.get() {
            self.inner.pending.borrow_mut().push(Box::new(change));
            return;
        }
        self.inner.applying.set(true);
        change(&mut self.inner.value.borrow_mut());
        loop {
            let next = {
                let mut pending = self.inner.pending.borrow_mut();
                if pending.is_empty() { None } else { Some(pending.remove(0)) }
            };
            let Some(next) = next else { break };
            next(&mut self.inner.value.borrow_mut());
        }
        self.inner.applying.set(false);
        self.inner.version.set(self.inner.version.get().wrapping_add(1));
        EPOCH.with(|epoch| epoch.set(epoch.get().wrapping_add(1)));
    }
}

/// A recorded [`ComponentContext::select`](crate::ComponentContext::select):
/// answers whether the component that made it must render again.
pub(crate) struct Selection {
    stale: Box<dyn Fn() -> bool>,
}

impl Selection {
    pub(crate) fn new<T, R>(store: &Store<T>, seen: R, select: impl Fn(&T) -> R + 'static) -> Self
    where
        R: PartialEq + 'static,
    {
        let store = store.clone();
        let version = store.version();
        Self { stale: Box::new(move || store.version() != version && store.read(&select) != seen) }
    }

    pub(crate) fn is_stale(&self) -> bool {
        (self.stale)()
    }
}

/// A value computed from an input, cached, and recomputed only when the
/// input changes by value (`C08`).
///
/// Use it in a selector so a store update that leaves the input equal
/// costs one comparison, not a recomputation:
///
/// ```
/// use rustnative_core::Derived;
///
/// let total = Derived::new(|prices: &Vec<u32>| prices.iter().sum::<u32>());
/// assert_eq!(total.get(&vec![3, 4]), 7);
/// assert_eq!(total.get(&vec![3, 4]), 7);
/// assert_eq!(total.computations(), 1, "an equal input reuses the result");
/// assert_eq!(total.get(&vec![5]), 5);
/// assert_eq!(total.computations(), 2);
/// ```
pub struct Derived<I, O> {
    compute: Box<dyn Fn(&I) -> O>,
    cache: RefCell<Option<(I, O)>>,
    computations: Cell<u64>,
}

impl<I, O> fmt::Debug for Derived<I, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Derived")
            .field("computations", &self.computations.get())
            .finish_non_exhaustive()
    }
}

impl<I: PartialEq + Clone, O: Clone> Derived<I, O> {
    /// A derived value computed by `compute`.
    pub fn new(compute: impl Fn(&I) -> O + 'static) -> Self {
        Self { compute: Box::new(compute), cache: RefCell::new(None), computations: Cell::new(0) }
    }

    /// The value for `input`: the cached one when `input` equals the last.
    pub fn get(&self, input: &I) -> O {
        if let Some((seen, output)) = &*self.cache.borrow() {
            if seen == input {
                return output.clone();
            }
        }
        let output = (self.compute)(input);
        self.computations.set(self.computations.get() + 1);
        *self.cache.borrow_mut() = Some((input.clone(), output.clone()));
        output
    }

    /// How many times it has computed — what the inspector shows beside it.
    #[must_use]
    pub fn computations(&self) -> u64 {
        self.computations.get()
    }
}

/// Values provided to a subtree by type (see
/// [`ComponentContext::provide_scoped`](crate::ComponentContext::provide_scoped)).
pub(crate) type ScopedValues = std::collections::HashMap<std::any::TypeId, Rc<dyn Any>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_updates_are_applied_in_order_after_the_running_one() {
        let store = Store::new("log", Vec::<u32>::new());
        let inner = store.clone();
        store.update(move |log| {
            log.push(1);
            inner.update(|log| log.push(3));
            log.push(2);
        });
        assert_eq!(store.get(), vec![1, 2, 3]);
        assert_eq!(store.version(), 1, "one update, applied as a unit");
    }

    #[test]
    fn a_selection_is_stale_only_when_its_slice_changes() {
        let store = Store::new("pair", (1, 1));
        let selection = Selection::new(&store, 1, |pair: &(i32, i32)| pair.0);
        store.update(|pair| pair.1 = 5);
        assert!(!selection.is_stale(), "the other half changed");
        store.update(|pair| pair.0 = 2);
        assert!(selection.is_stale());
    }

    #[test]
    fn inspectable_stores_are_listed_while_alive() {
        let store = Store::inspectable("counter", 3_u32);
        assert!(inspect_stores().iter().any(|s| s.name == "counter" && s.value == 3));
        drop(store);
        assert!(!inspect_stores().iter().any(|s| s.name == "counter"));
    }
}
