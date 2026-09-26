//! Application components: the [`Component`] trait, the render-time
//! [`ComponentContext`] capability, and the framework-managed
//! [`ComponentTree`] that creates/reuses/removes them.
//!
//! This module owns exactly the "what is a component, and how does the
//! framework keep a tree of them alive across renders" responsibility. It
//! depends on [`crate::node`] (what a component *produces*),
//! [`crate::event`] (what a component *receives*), and
//! [`crate::scheduler`] (how a component does asynchronous work) — but
//! nothing here depends on layout, styling, or any platform: see the
//! standards audit's P1.21/P2.22 findings on module boundaries, which this
//! split is a direct response to.

pub mod boundary;
mod context;
mod effects;
mod error;
mod invalidation;
pub mod pure;
mod tree;

pub use boundary::Failure;
pub use context::{
    AnimationRequest, AnimationRequests, Callback, ComponentContext, InputRequest, InputRequests,
    WindowRequests,
};
pub use effects::{EffectCleanup, EffectContext};
pub use error::RenderError;
pub use invalidation::{RenderCause, RenderRecord};
pub use pure::{Pure, PureComponent};
pub use tree::{ComponentTree, Deferred};

pub(crate) use context::WindowCommand;

use crate::event::Event;
use crate::node::Node;

/// The application/component contract.
///
/// A component owns its state and describes how state becomes UI. Platform
/// backends only need to know how to feed framework events into this
/// contract.
///
/// # Example
///
/// A counter. [`Self::view`] describes the UI as a function of state, and
/// [`Self::update`] is the only place that state changes — the framework
/// re-renders and reconciles afterwards.
///
/// ```
/// use rustnative_core::{Component, ComponentTree, Event, Node, NodeId};
///
/// struct Counter {
///     count: u32,
/// }
///
/// impl Component for Counter {
///     // Nothing is passed in from a parent, and nothing is emitted to one.
///     type Props = ();
///     type Message = ();
///
///     fn new((): Self::Props) -> Self {
///         Self { count: 0 }
///     }
///     fn props(&self) -> &Self::Props {
///         &()
///     }
///     fn set_props(&mut self, (): Self::Props) {}
///
///     fn view(&self) -> Node {
///         Node::column(
///             "root",
///             [
///                 Node::label("count", format!("Count: {}", self.count)),
///                 Node::button("increment", "Increment"),
///             ],
///         )
///     }
///
///     fn update(&mut self, event: Event) {
///         if let Event::Click { target } = event {
///             if target == NodeId::from_key("increment") {
///                 self.count += 1;
///             }
///         }
///     }
/// }
///
/// // A `ComponentTree` is what a platform backend drives. Constructing one
/// // performs the first render, so a view is available immediately.
/// let mut tree = ComponentTree::new(Counter::new(()));
/// assert!(matches!(tree.view(), Node::Column(_)));
///
/// // Dispatching an event updates state and re-renders.
/// tree.dispatch(Event::Click { target: NodeId::from_key("increment") });
/// let Node::Column(root) = tree.view() else { panic!("the root is a column") };
/// let Node::Label(label) = &root.children()[0] else { panic!("first child is the label") };
/// assert_eq!(label.text(), "Count: 1");
///
/// // A fresh counter's view, in markup:
/// let markup = rustnative_core::rsx! {
///     <Column key="root">
///         <Label key="count" text="Count: 0" />
///         <Button key="increment" text="Increment" />
///     </Column>
/// };
/// assert_eq!(markup, Counter::new(()).view());
/// ```
pub trait Component: 'static {
    /// Parent-provided, externally comparable inputs to this component.
    type Props: Clone + PartialEq + 'static;
    /// A typed message this component can emit to its parent through a
    /// [`Callback`], or receive from an asynchronous task/effect.
    type Message: Send + 'static;

    /// Constructs the component from its initial props.
    fn new(props: Self::Props) -> Self;
    /// Returns the component's current props.
    fn props(&self) -> &Self::Props;
    /// Replaces the component's props (called by the framework when a
    /// parent supplies new, unequal props; see [`Self::props_changed`]).
    fn set_props(&mut self, props: Self::Props);

    /// Describes the component's current UI as a [`Node`] tree.
    fn view(&self) -> Node;
    /// Updates the component's state in response to `event`.
    fn update(&mut self, event: Event);

    /// Handles typed messages emitted by child components through a
    /// [`Callback`].
    fn message(&mut self, _message: Self::Message) {}

    /// Renders the component with access to the framework-managed child
    /// tree. Components that do not compose child components can rely on
    /// the default implementation, which simply calls `view()`.
    fn render(&mut self, _context: &mut ComponentContext<'_, Self::Message>) -> Node {
        self.view()
    }

    /// Called after the framework applies changed parent-provided props.
    fn props_changed(&mut self) {}

    /// Called when the component becomes part of the active component
    /// tree.
    fn mounted(&mut self) {}

    /// Called after the component handles an event and has a chance to
    /// update its state.
    fn updated(&mut self) {}

    /// Called when the component leaves the active component tree.
    fn unmounted(&mut self) {}

    /// This component's state as the inspector shows it (`PLAN.md`
    /// Milestone 44), or `None` — the default — to show nothing. What it
    /// returns is also what the inspector's state history records after
    /// each change.
    fn inspect(&self) -> Option<serde_json::Value> {
        None
    }

    /// The message that sets `field` to `value`, for the inspector's state
    /// editing, or `None` — the default — when that field is not editable.
    ///
    /// An edit is a message, delivered and rendered like any other, so
    /// editing state from the inspector goes through the same path as the
    /// application changing it itself.
    fn edit(&self, _field: &str, _value: &serde_json::Value) -> Option<Self::Message> {
        None
    }
}

/// A stable slot for composing one component directly outside a managed
/// [`ComponentTree`]. Kept for low-level ownership scenarios; application
/// code should prefer [`ComponentContext::child`].
pub struct ComponentHost<C: Component> {
    component: C,
}

impl<C: Component> ComponentHost<C> {
    /// Wraps `component`, calling its [`Component::mounted`] hook.
    pub fn new(mut component: C) -> Self {
        component.mounted();
        Self { component }
    }

    /// Returns whether the hosted component is mounted. A `ComponentHost`
    /// mounts its component immediately in [`Self::new`] and unmounts it
    /// only when replaced or dropped, so this is always `true` for as long
    /// as the host itself exists.
    pub fn is_mounted(&self) -> bool {
        true
    }

    /// Returns the hosted component's current view.
    pub fn view(&self) -> Node {
        self.component.view()
    }

    /// Delivers `event` to the hosted component if [`Self::owns_event`]
    /// says it should receive it. Returns whether it was delivered.
    pub fn update(&mut self, event: Event) -> bool {
        if !self.owns_event(&event) {
            return false;
        }
        self.component.update(event);
        self.component.updated();
        true
    }

    /// Returns whether `event` targets a node within the hosted
    /// component's current view (or has no specific target at all).
    pub fn owns_event(&self, event: &Event) -> bool {
        match event.target() {
            Some(target) => self.component.view().contains_id(target),
            None => true,
        }
    }

    /// Returns a reference to the hosted component.
    pub fn component(&self) -> &C {
        &self.component
    }

    /// Returns a mutable reference to the hosted component.
    pub fn component_mut(&mut self) -> &mut C {
        &mut self.component
    }

    /// Unmounts the current component and mounts `component` in its place.
    pub fn replace(&mut self, mut component: C) {
        self.component.unmounted();
        component.mounted();
        self.component = component;
    }
}

impl<C: Component> Drop for ComponentHost<C> {
    fn drop(&mut self) {
        self.component.unmounted();
    }
}

#[cfg(test)]
mod host_tests {
    use super::*;

    #[derive(Clone, PartialEq, Default)]
    struct Props;

    struct Simple {
        updates: u32,
    }

    impl Component for Simple {
        type Props = Props;
        type Message = ();

        fn new(_props: Self::Props) -> Self {
            Self { updates: 0 }
        }
        fn props(&self) -> &Self::Props {
            &Props
        }
        fn set_props(&mut self, _props: Self::Props) {}
        fn view(&self) -> Node {
            Node::button("go", "Go")
        }
        fn update(&mut self, _event: Event) {
            self.updates += 1;
        }
    }

    #[test]
    fn component_host_only_delivers_events_it_owns() {
        let mut host = ComponentHost::new(Simple::new(Props));
        assert!(host.update(Event::Click { target: crate::identity::NodeId::from_key("go") }));
        assert_eq!(host.component().updates, 1);
        assert!(!host.update(Event::Click { target: crate::identity::NodeId::from_key("other") }));
        assert_eq!(host.component().updates, 1);
    }
}
