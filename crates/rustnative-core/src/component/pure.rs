//! Pure components (`PLAN.md` Milestone 54, `C03`): render purity enforced
//! by type.
//!
//! A [`PureComponent`] renders from shared access to its props and nothing
//! else — it has no state to mutate, and its `render` takes `&Props` — so a
//! render that mutates does not compile. It is skipped whenever its props
//! equal the previous render's, and it is safe to render at any time, in
//! any order, as often as the scheduler likes: the property interruptible
//! and deferred rendering rely on.
//!
//! A component with state is a [`Component`]; its `render` takes `&mut
//! self` for the child and effect bookkeeping it does, and the framework
//! treats it as non-interruptible.

use std::marker::PhantomData;

use super::{Component, ComponentContext};
use crate::event::Event;
use crate::node::Node;

/// A component that is a function of its props; see the [module
/// documentation](crate::component::pure).
///
/// ```
/// use rustnative_core::{Component, ComponentContext, ComponentTree, Event, Node, PureComponent};
///
/// struct Row;
/// impl PureComponent for Row {
///     type Props = (u32, String);
///     fn render((index, name): &(u32, String)) -> Node {
///         Node::label("name", format!("{index}. {name}"))
///     }
/// }
///
/// struct List;
/// impl Component for List {
///     type Props = ();
///     type Message = ();
///     fn new((): ()) -> Self { Self }
///     fn props(&self) -> &() { &() }
///     fn set_props(&mut self, (): ()) {}
///     fn view(&self) -> Node { Node::column("list", []) }
///     fn update(&mut self, _: Event) {}
///     fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
///         let row = context.pure::<Row>("first", (1, "Ada".to_owned()));
///         Node::column("list", [row])
///     }
/// }
///
/// let tree = ComponentTree::new(List);
/// # let _ = tree;
///
/// // A row in markup:
/// assert_eq!(rustnative_core::rsx! { <Label key="name" text="1. Ada" /> }, Row::render(&(1, "Ada".to_owned())));
/// ```
pub trait PureComponent: 'static {
    /// What it renders from.
    type Props: Clone + PartialEq + 'static;

    /// Its output for `props`.
    fn render(props: &Self::Props) -> Node;
}

/// The [`Component`] a [`PureComponent`] is composed as.
pub struct Pure<P: PureComponent> {
    props: P::Props,
    _component: PhantomData<fn() -> P>,
}

impl<P: PureComponent> Component for Pure<P> {
    type Props = P::Props;
    type Message = ();

    fn new(props: Self::Props) -> Self {
        Self { props, _component: PhantomData }
    }
    fn props(&self) -> &Self::Props {
        &self.props
    }
    fn set_props(&mut self, props: Self::Props) {
        self.props = props;
    }
    fn view(&self) -> Node {
        P::render(&self.props)
    }
    fn update(&mut self, _: Event) {}
}

impl<M: Send + 'static> ComponentContext<'_, M> {
    /// Composes the pure component `P` with `props`: rendered again only
    /// when `props` changes.
    pub fn pure<P: PureComponent>(&mut self, key: impl Into<String>, props: P::Props) -> Node {
        self.child_with_props::<Pure<P>, _>(key, props, Pure::new)
    }
}
