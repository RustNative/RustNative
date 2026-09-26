//! Error boundaries (`PLAN.md` Milestone 47, `C17`): a subtree that fails is
//! contained, shows a fallback, and is retried, without taking the
//! application down.
//!
//! A boundary is composed with
//! [`ComponentContext::boundary`](crate::ComponentContext::boundary). It
//! contains a panic in any component of its subtree — while rendering,
//! handling an event, or receiving a message. Containing means:
//!
//! 1. every component of the subtree is removed, its tasks cancelled and its
//!    effects cleaned up, exactly as if it had been unmounted, so nothing
//!    whose invariants the panic disproved keeps running;
//! 2. the boundary renders its fallback in the subtree's place;
//! 3. the failure is reported: to [`ComponentTree::take_failures`], and
//!    through the inspection trace (Milestone 44);
//! 4. the boundary's [`SupervisionPolicy`] decides what comes next. Under
//!    [`SupervisionPolicy::RestartWithBackoff`] the subtree is rebuilt, from
//!    new state, after the policy's delay. Under
//!    [`SupervisionPolicy::Isolate`] it stays on the fallback until retried
//!    by hand: a fallback node keyed `retry`, when clicked, rebuilds it.
//!    [`SupervisionPolicy::Escalate`] contains nothing, and the enclosing
//!    boundary handles the failure.
//!
//! A panic with no boundary above it reaches the application's
//! [`crate::PanicPolicy`], as before.
//!
//! [`ComponentTree::take_failures`]: crate::ComponentTree::take_failures

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;
use std::time::Duration;

use super::{Component, ComponentContext};
use crate::event::Event;
use crate::identity::NodeId;
use crate::node::Node;
use crate::scheduler::{SupervisionPolicy, panic_message};

/// A contained failure of a subtree.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Failure {
    /// The boundary's key path.
    pub component: String,
    /// The panic's message.
    pub message: String,
    /// How many times this boundary has contained a failure.
    pub attempt: u32,
}

#[derive(Debug, Default)]
pub(crate) struct BoundaryState {
    pub(crate) policy: SupervisionPolicy,
    pub(crate) failure: Option<Failure>,
    pub(crate) failures: u32,
    pub(crate) retry: Option<Duration>,
}

/// What a boundary is composed with.
#[derive(Clone)]
pub struct BoundaryProps<P> {
    props: P,
    policy: SupervisionPolicy,
    fallback: fn(&Failure) -> Node,
}

impl<P: PartialEq> PartialEq for BoundaryProps<P> {
    fn eq(&self, other: &Self) -> bool {
        // The fallback is code, not data; comparing function pointers is
        // not meaningful, so a changed fallback takes effect on the next
        // failure rather than forcing a render.
        self.props == other.props && self.policy == other.policy
    }
}

/// The message a boundary sends itself when its restart delay ends.
#[derive(Debug)]
pub struct Retry(u32);

/// The component [`ComponentContext::boundary`] composes around `C`.
pub struct Boundary<C: Component> {
    props: BoundaryProps<C::Props>,
    state: Rc<RefCell<BoundaryState>>,
}

impl<C: Component> Boundary<C> {
    fn retry(&self) {
        self.state.borrow_mut().failure = None;
    }
}

impl<C: Component> Component for Boundary<C> {
    type Props = BoundaryProps<C::Props>;
    type Message = Retry;

    fn new(props: Self::Props) -> Self {
        let state = BoundaryState { policy: props.policy, ..BoundaryState::default() };
        Self { props, state: Rc::new(RefCell::new(state)) }
    }

    fn props(&self) -> &Self::Props {
        &self.props
    }

    fn set_props(&mut self, props: Self::Props) {
        self.state.borrow_mut().policy = props.policy;
        self.props = props;
    }

    fn view(&self) -> Node {
        Node::column("boundary", [])
    }

    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { target } if target == NodeId::from_key("retry")) {
            self.retry();
        }
    }

    fn message(&mut self, Retry(attempt): Retry) {
        // A retry scheduled before a later failure is stale.
        if self.state.borrow().failures == attempt {
            self.retry();
        }
    }

    fn render(&mut self, context: &mut ComponentContext<'_, Retry>) -> Node {
        context.tree.register_boundary(context.parent, Rc::clone(&self.state));
        if self.state.borrow().failure.is_none() {
            let depth = crate::i18n::depth();
            let props = self.props.props.clone();
            let rendered = catch_unwind(AssertUnwindSafe(|| {
                context.child_with_props("content", props, C::new)
            }));
            match rendered {
                Ok(node) => return node,
                Err(payload) => {
                    crate::i18n::truncate(depth);
                    let policy = context.tree.contain(context.parent, panic_message(&*payload));
                    if policy == SupervisionPolicy::Escalate {
                        resume_unwind(payload);
                    }
                }
            }
        }
        let (retry, attempt) = {
            let mut state = self.state.borrow_mut();
            (state.retry.take(), state.failures)
        };
        if let Some(delay) = retry {
            let sleep = context.sleep(delay);
            context.spawn(async move {
                sleep.await;
                Retry(attempt)
            });
        }
        let state = self.state.borrow();
        state.failure.as_ref().map_or_else(|| self.view(), self.props.fallback)
    }
}

impl<M: Send + 'static> ComponentContext<'_, M> {
    /// Composes child component `C` inside an error boundary; see
    /// [`crate::component::boundary`].
    ///
    /// `fallback` is shown while the subtree is failed. A node in it keyed
    /// `retry` rebuilds the subtree when clicked.
    ///
    /// ```
    /// use rustnative_core::{
    ///     Component, ComponentContext, ComponentTree, Event, Node, SupervisionPolicy,
    /// };
    ///
    /// struct Flaky;
    /// impl Component for Flaky {
    ///     type Props = ();
    ///     type Message = ();
    ///     fn new((): ()) -> Self { Self }
    ///     fn props(&self) -> &() { &() }
    ///     fn set_props(&mut self, (): ()) {}
    ///     fn view(&self) -> Node { panic!("the feed could not be parsed") }
    ///     fn update(&mut self, _: Event) {}
    /// }
    ///
    /// struct Screen;
    /// impl Component for Screen {
    ///     type Props = ();
    ///     type Message = ();
    ///     fn new((): ()) -> Self { Self }
    ///     fn props(&self) -> &() { &() }
    ///     fn set_props(&mut self, (): ()) {}
    ///     fn view(&self) -> Node { Node::column("screen", []) }
    ///     fn update(&mut self, _: Event) {}
    ///     fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
    ///         let feed = context.boundary::<Flaky>("feed", (), SupervisionPolicy::Isolate, |failure| {
    ///             Node::column("failed", [
    ///                 Node::label("why", failure.message.clone()),
    ///                 Node::button("retry", "Try again"),
    ///             ])
    ///         });
    ///         Node::column("screen", [Node::label("title", "News"), feed])
    ///     }
    /// }
    ///
    /// # std::panic::set_hook(Box::new(|_| {}));
    /// let mut tree = ComponentTree::new(Screen);
    /// let failures = tree.take_failures();
    /// assert_eq!(failures[0].message, "the feed could not be parsed");
    ///
    /// // The screen's resting view in markup:
    /// assert_eq!(rustnative_core::rsx! { <Column key="screen"></Column> }, Screen.view());
    /// ```
    pub fn boundary<C>(
        &mut self,
        key: impl Into<String>,
        props: C::Props,
        policy: SupervisionPolicy,
        fallback: fn(&Failure) -> Node,
    ) -> Node
    where
        C: Component,
    {
        self.child_with_props::<Boundary<C>, _>(
            key,
            BoundaryProps { props, policy, fallback },
            Boundary::new,
        )
    }
}
