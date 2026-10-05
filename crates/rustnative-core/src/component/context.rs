//! Render-time capabilities handed to a component: composing children,
//! spawning tasks, sending typed messages to a parent, and requesting a
//! sibling window be opened or closed.

use std::any::Any;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use super::Component;
use super::effects::{EffectCleanup, EffectContext};
use super::tree::ComponentTree;
use crate::animation::{AnimatedProperty, Animation, MotionPreference};
use crate::identity::{ComponentId, NodeId, WindowId};
use crate::input::DropEffect;
use crate::node::Node;
use crate::scheduler::{SleepFuture, TaskHandle, TaskScope};
use crate::services::Services;
use crate::style::Theme;
use crate::window::Window;

pub(crate) struct QueuedMessage {
    pub(crate) target: ComponentId,
    pub(crate) message: Box<dyn Any>,
    pub(crate) priority: crate::scheduler::Priority,
}

/// A deferred request to open or close a sibling window, queued by a
/// component through [`ComponentContext::windows`] and applied by
/// [`crate::application::Application`] right after the requesting window's
/// dispatch/task-pump finishes. Opening is boxed as a constructor so
/// [`WindowRequests`] stays generic-free while still being able to build a
/// strongly typed `ComponentTree` once it reaches the `Application` that
/// owns the window registry.
pub(crate) enum WindowCommand {
    Open(Box<dyn FnOnce(&mut crate::application::Application) -> WindowId>),
    Close(WindowId),
}

/// A render-time capability for requesting that a sibling window be opened
/// or closed. Unlike a UI event, the request is deferred: it is queued here
/// and applied by the owning `Application` once the current dispatch or task
/// pump finishes, so opening a window never happens in the middle of
/// rendering the requesting window's own tree.
#[derive(Clone)]
pub struct WindowRequests {
    sink: Rc<RefCell<VecDeque<WindowCommand>>>,
}

impl WindowRequests {
    pub(crate) fn new(sink: Rc<RefCell<VecDeque<WindowCommand>>>) -> Self {
        Self { sink }
    }

    /// Requests that `window`, owned by a fresh `component`, be opened. When
    /// `modal_parent` is `Some`, the platform backend disables that window's
    /// native input while this one remains open.
    pub fn open<C: Component>(&self, component: C, window: Window, modal_parent: Option<WindowId>) {
        self.sink.borrow_mut().push_back(WindowCommand::Open(Box::new(
            move |application: &mut crate::application::Application| {
                application.open_window(component, window, modal_parent)
            },
        )));
    }

    /// Requests that `id` be closed. Requesting the primary window's own id
    /// is a harmless no-op, matching `Application::close_window`.
    pub fn close(&self, id: WindowId) {
        self.sink.borrow_mut().push_back(WindowCommand::Close(id));
    }
}

/// A request from a component to the platform backend about advanced
/// input, queued through [`InputRequests`] and drained by the backend
/// (`Application::take_input_requests`) after the dispatch that produced
/// it completes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InputRequest {
    /// Route every event from `pointer_id` to `node` until released, even
    /// when the pointer leaves it.
    CapturePointer {
        /// The framework-wide identity of the capturing node.
        node: NodeId,
        /// Which contact to capture.
        pointer_id: u32,
    },
    /// End a capture started with [`InputRequest::CapturePointer`].
    ReleasePointer {
        /// The framework-wide identity of the capturing node.
        node: NodeId,
        /// Which contact to release.
        pointer_id: u32,
    },
    /// The answer to the most recent `DragEnter`/`DragOver`: what would
    /// happen if the data were dropped now.
    SetDropEffect(DropEffect),
}

/// A component's handle for requesting pointer capture and answering drag
/// feedback.
///
/// Obtained during `render` through [`ComponentContext::input`] and, like a
/// [`Callback`], kept in component state so `update` can use it — `update`
/// itself receives no context. Requests are deferred: the backend applies
/// them once the current dispatch finishes, so a component never reaches
/// into native state mid-update.
///
/// # Example
///
/// ```
/// use rustnative_core::{
///     Component, ComponentContext, Event, InputInterest, InputRequests, Node,
/// };
///
/// struct Slider {
///     input: Option<InputRequests>,
///     dragging: bool,
/// }
///
/// impl Component for Slider {
///     type Props = ();
///     type Message = ();
///     fn new((): ()) -> Self { Self { input: None, dragging: false } }
///     fn props(&self) -> &() { &() }
///     fn set_props(&mut self, (): ()) {}
///     fn view(&self) -> Node {
///         Node::column("track", []).with_input(InputInterest::new().pointer())
///     }
///     fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
///         self.input = Some(context.input());
///         self.view()
///     }
///     fn update(&mut self, event: Event) {
///         match event {
///             Event::PointerDown { pointer, .. } => {
///                 self.dragging = true;
///                 // Keep receiving moves even when the pointer leaves the track.
///                 if let Some(input) = &self.input {
///                     input.capture_pointer("track", pointer.pointer_id());
///                 }
///             }
///             Event::PointerUp { .. } | Event::PointerCancel { .. } => self.dragging = false,
///             _ => {}
///         }
///     }
/// }
///
/// // The view in markup:
/// let markup = rustnative_core::rsx! { <Column key="track" input={InputInterest::new().pointer()}></Column> };
/// assert_eq!(markup, Slider::new(()).view());
/// ```
#[derive(Clone)]
pub struct InputRequests {
    owner: ComponentId,
    sink: Rc<RefCell<VecDeque<InputRequest>>>,
}

impl fmt::Debug for InputRequests {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InputRequests").field("owner", &self.owner).finish_non_exhaustive()
    }
}

impl InputRequests {
    pub(crate) fn new(owner: ComponentId, sink: Rc<RefCell<VecDeque<InputRequest>>>) -> Self {
        Self { owner, sink }
    }

    /// Resolves a component-local key to the framework-wide identity the
    /// backend knows the node by — the same scoping `ComponentTree`
    /// applies to every node a component renders.
    fn global(&self, key: &str) -> NodeId {
        let local = NodeId::from_key(key);
        if self.owner == ComponentId::ROOT { local } else { NodeId::scoped(self.owner, local) }
    }

    /// Captures `pointer_id` to this component's node `key`.
    pub fn capture_pointer(&self, key: impl AsRef<str>, pointer_id: u32) {
        let node = self.global(key.as_ref());
        self.sink.borrow_mut().push_back(InputRequest::CapturePointer { node, pointer_id });
    }

    /// Releases a capture of `pointer_id` held by this component's node
    /// `key`.
    pub fn release_pointer(&self, key: impl AsRef<str>, pointer_id: u32) {
        let node = self.global(key.as_ref());
        self.sink.borrow_mut().push_back(InputRequest::ReleasePointer { node, pointer_id });
    }

    /// Answers the current drag: what dropping now would do.
    pub fn set_drop_effect(&self, effect: DropEffect) {
        self.sink.borrow_mut().push_back(InputRequest::SetDropEffect(effect));
    }
}

/// A request from a component to the platform backend about animation,
/// queued through [`AnimationRequests`] and drained by the backend after
/// the dispatch that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnimationRequest {
    /// Start an animation on `node`, owned by `owner`.
    Start {
        /// The framework-wide identity of the node to animate.
        node: NodeId,
        /// What to animate, and how.
        animation: Animation,
        /// The component that asked, so the animation can be cancelled
        /// when it unmounts.
        owner: ComponentId,
    },
    /// Stop the animation of one property, returning it to rest.
    Cancel {
        /// The framework-wide identity of the animated node.
        node: NodeId,
        /// Which property to stop animating.
        property: AnimatedProperty,
    },
    /// Stop everything a component started — queued by the runtime itself
    /// when that component leaves the tree.
    CancelOwner(ComponentId),
}

/// A component's handle for starting animations.
///
/// Obtained during `render` through [`ComponentContext::animations`] and,
/// like a [`Callback`], kept in component state so `update` can use it.
/// Requests are deferred: the backend applies them once the current
/// dispatch finishes, so starting an animation never reaches into native
/// state mid-update.
///
/// # Example
///
/// ```
/// use std::time::Duration;
///
/// use rustnative_core::{
///     AnimatedProperty, AnimatedValue, Animation, AnimationRequests, Component,
///     ComponentContext, Event, Node, Point, Transition,
/// };
///
/// struct Toast {
///     animations: Option<AnimationRequests>,
/// }
///
/// impl Component for Toast {
///     type Props = ();
///     type Message = ();
///     fn new((): ()) -> Self { Self { animations: None } }
///     fn props(&self) -> &() { &() }
///     fn set_props(&mut self, (): ()) {}
///     fn view(&self) -> Node { Node::label("toast", "Saved") }
///     fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
///         self.animations = Some(context.animations());
///         self.view()
///     }
///     fn update(&mut self, event: Event) {
///         if let (Event::Click { .. }, Some(animations)) = (&event, &self.animations) {
///             // Slide in from the left, then rest where layout puts it.
///             animations.animate(
///                 "toast",
///                 Animation::new(
///                     AnimatedProperty::Translation,
///                     AnimatedValue::Offset(Point::new(0, 0)),
///                     Transition::new(Duration::from_millis(180)),
///                 )
///                 .from(AnimatedValue::Offset(Point::new(-40, 0))),
///             );
///         }
///     }
/// }
///
/// // The view in markup:
/// assert_eq!(rustnative_core::rsx! { <Label key="toast" text="Saved" /> }, Toast::new(()).view());
/// ```
#[derive(Clone)]
pub struct AnimationRequests {
    owner: ComponentId,
    sink: Rc<RefCell<VecDeque<AnimationRequest>>>,
}

impl fmt::Debug for AnimationRequests {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnimationRequests").field("owner", &self.owner).finish_non_exhaustive()
    }
}

impl AnimationRequests {
    pub(crate) fn new(owner: ComponentId, sink: Rc<RefCell<VecDeque<AnimationRequest>>>) -> Self {
        Self { owner, sink }
    }

    /// Animates this component's node `key`.
    ///
    /// One property animates at a time: asking again for a property that is
    /// already animating retargets it from where it is, rather than
    /// restarting or fighting with it. The animation ends when it finishes,
    /// when [`Self::cancel`] stops it, or when this component unmounts.
    pub fn animate(&self, key: impl AsRef<str>, animation: Animation) {
        let node = self.global(key.as_ref());
        self.sink.borrow_mut().push_back(AnimationRequest::Start {
            node,
            animation,
            owner: self.owner,
        });
    }

    /// Stops animating `property` on this component's node `key`, returning
    /// it to the value the rendered tree gives it.
    pub fn cancel(&self, key: impl AsRef<str>, property: AnimatedProperty) {
        let node = self.global(key.as_ref());
        self.sink.borrow_mut().push_back(AnimationRequest::Cancel { node, property });
    }

    fn global(&self, key: &str) -> NodeId {
        let local = NodeId::from_key(key);
        if self.owner == ComponentId::ROOT { local } else { NodeId::scoped(self.owner, local) }
    }
}

/// A typed child-to-parent message sender. Clones are cheap and represent
/// the same framework-managed channel endpoint.
pub struct Callback<M: 'static> {
    target: ComponentId,
    sink: Rc<RefCell<VecDeque<QueuedMessage>>>,
    _marker: std::marker::PhantomData<fn(M)>,
}

// Written out rather than derived: a derive would require `M: Clone`, but
// cloning a callback copies an endpoint, never a message.
impl<M: 'static> Clone for Callback<M> {
    fn clone(&self) -> Self {
        Self { target: self.target, sink: Rc::clone(&self.sink), _marker: std::marker::PhantomData }
    }
}

impl<M: 'static> PartialEq for Callback<M> {
    fn eq(&self, other: &Self) -> bool {
        self.target == other.target && Rc::ptr_eq(&self.sink, &other.sink)
    }
}

impl<M: 'static> Eq for Callback<M> {}

impl<M: 'static> fmt::Debug for Callback<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Callback").field("target", &self.target).finish_non_exhaustive()
    }
}

impl<M: 'static> Callback<M> {
    pub(crate) fn new(target: ComponentId, sink: Rc<RefCell<VecDeque<QueuedMessage>>>) -> Self {
        Self { target, sink, _marker: std::marker::PhantomData }
    }

    /// Queues `message` for delivery to the target component's
    /// [`Component::message`](crate::Component::message).
    pub fn send(&self, message: M) {
        self.send_with(crate::scheduler::Priority::Normal, message);
    }

    /// Queues `message` at `priority`: a [`crate::Priority::Deferrable`]
    /// message waits until nothing more urgent is pending, and is then
    /// delivered in slices that leave the host free for input (see
    /// [`crate::scheduler::suspend`]).
    pub fn send_with(&self, priority: crate::scheduler::Priority, message: M) {
        self.sink.borrow_mut().push_back(QueuedMessage {
            target: self.target,
            message: Box::new(message),
            priority,
        });
    }
}

/// Render-time capabilities available to one component: composing children,
/// spawning tasks, requesting a re-render dependency (`effect`), sending
/// messages to a parent, and reaching platform services.
///
/// # Example
///
/// A component that composes a child and does asynchronous work. Both go
/// through the context: children so the framework can own their state
/// across renders, and tasks so they are cancelled when this component is
/// unmounted.
///
/// ```
/// use std::time::Duration;
///
/// use rustnative_core::{Component, ComponentContext, ComponentTree, Event, Node};
///
/// # #[derive(Clone, PartialEq, Default)]
/// # struct Unit;
/// # struct Child { props: Unit }
/// # impl Component for Child {
/// #     type Props = Unit;
/// #     type Message = ();
/// #     fn new(props: Self::Props) -> Self { Self { props } }
/// #     fn props(&self) -> &Self::Props { &self.props }
/// #     fn set_props(&mut self, props: Self::Props) { self.props = props; }
/// #     fn view(&self) -> Node { Node::label("child-label", "child") }
/// #     fn update(&mut self, _event: Event) {}
/// # }
/// struct Parent {
///     loaded: bool,
/// }
///
/// impl Component for Parent {
///     type Props = ();
///     type Message = &'static str;
///
///     fn new((): Self::Props) -> Self {
///         Self { loaded: false }
///     }
///     fn props(&self) -> &Self::Props {
///         &()
///     }
///     fn set_props(&mut self, (): Self::Props) {}
///
///     fn view(&self) -> Node {
///         Node::label("status", if self.loaded { "loaded" } else { "loading" })
///     }
///     fn update(&mut self, _event: Event) {}
///
///     // A task's output *is* the message, delivered here once the
///     // scheduler wakes the platform's event loop.
///     fn message(&mut self, _message: Self::Message) {
///         self.loaded = true;
///     }
///
///     fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
///         // An effect runs after this render commits, and re-runs only
///         // when its dependencies change — `()` here means "once".
///         context.effect("load-once", (), |effects| {
///             effects.spawn(async { "loaded" });
///             Box::new(|| { /* cleanup runs before the effect re-runs */ })
///         });
///
///         Node::column(
///             "root",
///             [
///                 self.view(),
///                 // The child's state is owned by the framework and
///                 // survives this component re-rendering.
///                 context.child::<Child>("child"),
///             ],
///         )
///     }
/// }
///
/// let mut tree = ComponentTree::new(Parent::new(()));
/// let Node::Column(root) = tree.view() else { panic!("the root is a column") };
/// assert_eq!(root.children().len(), 2);
///
/// // The view in markup:
/// assert_eq!(rustnative_core::rsx! { <Label key="status" text="loading" /> }, Parent::new(()).view());
/// ```
pub struct ComponentContext<'a, M: Send + 'static> {
    pub(super) tree: &'a mut ComponentTree,
    pub(super) parent: ComponentId,
    pub(super) generation: u64,
    pub(super) task_scope: TaskScope,
    pub(super) _message: std::marker::PhantomData<fn(M)>,
}

impl<M: Send + 'static> ComponentContext<'_, M> {
    /// This component's identity in its tree: the owner of the nodes its
    /// view returns ([`crate::NodeId::owner`] — `None` there for the root
    /// component, whose identity is [`ComponentId::ROOT`]). A host that
    /// must find a component's subtree in the rendered tree — a server
    /// marking where an interactive subtree begins — matches on it.
    #[must_use]
    pub const fn id(&self) -> ComponentId {
        self.parent
    }

    /// Creates a typed child-to-parent callback for this component.
    #[must_use]
    pub fn callback<Msg: 'static>(&self) -> Callback<Msg> {
        Callback::new(self.parent, Rc::clone(self.tree.message_sink()))
    }

    /// Spawns `future` on this component's task scope, delivering its
    /// output as a message via [`Component::message`](crate::Component::message)
    /// when it completes.
    pub fn spawn<F>(&self, future: F) -> TaskHandle
    where
        F: std::future::Future<Output = M> + Send + 'static,
    {
        self.task_scope().spawn(future)
    }

    /// Spawns a `!Send` future — one holding an `Rc` or a thread-bound host
    /// object — on this component's task scope. It runs on this component
    /// tree's own thread (see [`crate::LocalExecutor`]) and is owned,
    /// cancelled, and delivered exactly as [`Self::spawn`]'s tasks are.
    pub fn spawn_local<F>(&self, future: F) -> TaskHandle
    where
        F: std::future::Future<Output = M> + 'static,
    {
        self.task_scope().spawn_local(future)
    }

    /// The current time by this component tree's scheduler — the clock its
    /// delays are measured on, so a timestamp and a [`Self::sleep`] agree
    /// (virtual under [`crate::ManualExecutor`]).
    #[must_use]
    pub fn now(&self) -> Duration {
        self.task_scope.scheduler().now()
    }

    /// Reads `key` from the environment: the nearest ancestor's provided
    /// value, else the window's, else the key's default. The read is
    /// recorded, so this component re-renders when — and only when — the
    /// value it saw changes (see [`crate::environment`]).
    pub fn env<T: crate::environment::EnvValue>(
        &mut self,
        key: &crate::environment::EnvKey<T>,
    ) -> T {
        self.tree.read_env(self.parent, key)
    }

    /// Provides `value` for `key` to every descendant of this component,
    /// overriding what the window or an ancestor provides. Provide before
    /// composing the children that read it; a reader composed earlier in
    /// the same render is brought up to date by a second pass.
    pub fn provide_env<T: crate::environment::EnvValue>(
        &mut self,
        key: &crate::environment::EnvKey<T>,
        value: T,
    ) {
        self.tree.provide_env(self.parent, key, value);
    }

    /// Publishes `value` for an upward preference, read by ancestors with
    /// [`Self::preference`] on their next render.
    pub fn prefer<T: crate::environment::Preference>(
        &mut self,
        key: &crate::environment::PreferenceKey<T>,
        value: T,
    ) {
        self.tree.publish(self.parent, key, value);
    }

    /// The reduction of every value this component's descendants published
    /// for `key` in their last render, or `None` if none did. This
    /// component re-renders when that set of values changes.
    pub fn preference<T: crate::environment::Preference>(
        &mut self,
        key: &crate::environment::PreferenceKey<T>,
    ) -> Option<T> {
        self.tree.read_preference(self.parent, key)
    }

    /// Declares `command` for this render: this component can perform it,
    /// and receives [`crate::Event::Command`] when it is invoked (see
    /// [`crate::command`]). A command not declared again in a later render
    /// stops existing.
    pub fn command(&mut self, command: crate::command::Command) {
        self.tree.declare_command(self.parent, command);
    }

    /// The size classes of the node this component rendered with key `key`,
    /// from the last layout — `None` before the first one. This component
    /// re-renders when either class changes, which is how a container
    /// chooses its own arrangement from the space it was actually given
    /// rather than from the window's (`C22`).
    pub fn container_classes(&mut self, key: &str) -> Option<crate::environment::SizeClasses> {
        self.tree.read_container_classes(self.parent, crate::identity::NodeId::from_key(key))
    }

    /// Returns the structured task scope owned by this component.
    ///
    /// The scope remains owned by the component even when this render-time
    /// handle is dropped. Tasks therefore survive renders but are
    /// automatically cancelled when the component leaves the managed tree.
    #[must_use]
    pub fn task_scope(&self) -> TaskScope {
        self.task_scope.clone()
    }

    /// Returns a future that completes after `duration`, driven by the
    /// scheduler rather than a real-time OS sleep (see
    /// [`crate::scheduler::Scheduler::sleep`]).
    pub fn sleep(&self, duration: Duration) -> SleepFuture {
        self.task_scope.scheduler().sleep(duration)
    }

    /// Returns the application-owned, platform-independent service
    /// contracts.
    #[must_use]
    pub fn services(&self) -> &Services {
        self.tree.services()
    }

    /// Returns a handle for requesting that a sibling window be opened or
    /// closed. Requests are deferred and applied once this window's current
    /// dispatch or task pump finishes.
    #[must_use]
    pub fn windows(&self) -> WindowRequests {
        WindowRequests::new(Rc::clone(self.tree.window_commands()))
    }

    /// Returns this component's handle for pointer capture and drag
    /// feedback; see [`InputRequests`].
    #[must_use]
    pub fn input(&self) -> InputRequests {
        InputRequests::new(self.parent, Rc::clone(self.tree.input_requests()))
    }

    /// Returns this component's handle for starting animations; see
    /// [`AnimationRequests`].
    #[must_use]
    pub fn animations(&self) -> AnimationRequests {
        AnimationRequests::new(self.parent, Rc::clone(self.tree.animation_requests()))
    }

    /// Whether the person has asked their system for reduced motion.
    ///
    /// Transitions and animations already honor this without being asked
    /// (see [`crate::animation`]); read it when a component wants to choose
    /// *different content* — a static illustration instead of a moving one.
    #[must_use]
    pub fn motion_preference(&self) -> MotionPreference {
        self.tree.motion_preference()
    }

    /// A value this component keeps across runs, under `key`, starting as
    /// `default` (see [`crate::persistence`] and [`crate::Persisted`]).
    ///
    /// The value is found again on the next launch because it is keyed by
    /// this component's *key path* — the child keys from its window's root
    /// down to it — which the same program rebuilds the same way every run.
    /// Two components with the same `key` in different places never share
    /// a value; a component that moves to a different parent starts from
    /// its default there.
    pub fn persisted<T>(&self, key: &str, default: T) -> crate::persistence::Persisted<T>
    where
        T: serde::Serialize + serde::de::DeserializeOwned + Clone,
    {
        self.tree.persisted(self.parent, key, default)
    }

    /// Returns the active theme.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        self.tree.theme()
    }

    /// Registers work that follows this component's reactive dependencies.
    ///
    /// The effect runs only after the current declarative render is
    /// committed. On a later render it is retained while `dependencies` is
    /// unchanged; when they change, its cleanup runs and its effect-owned
    /// tasks are cancelled before the replacement effect starts. Removing
    /// the component follows the same cleanup and cancellation path.
    ///
    /// Calling this with a `key` already used earlier in the *same* render
    /// is a component composition mistake (see
    /// [`crate::component::RenderError::DuplicateEffectKey`]): the second
    /// registration is ignored and the mistake is surfaced through
    /// [`ComponentTree::last_render_error`] rather than through a panic.
    pub fn effect<D, F>(&mut self, key: impl Into<String>, dependencies: D, effect: F)
    where
        D: PartialEq + 'static,
        F: FnOnce(EffectContext) -> EffectCleanup + 'static,
    {
        self.tree.register_effect(
            self.parent,
            key.into(),
            super::effects::EffectDependencies::new(dependencies),
            Box::new(effect),
        );
    }

    /// Compose a child component with no meaningful inputs. Its props type
    /// must implement `Default`.
    pub fn child<C>(&mut self, key: impl Into<String>) -> Node
    where
        C: Component,
        C::Props: Default,
    {
        self.child_with_props(key, C::Props::default(), C::new)
    }

    /// Compose a child component with typed parent-provided inputs. The
    /// child instance is reused by key; changed props update the existing
    /// instance instead of remounting it.
    pub fn child_with_props<C, F>(
        &mut self,
        key: impl Into<String>,
        props: C::Props,
        constructor: F,
    ) -> Node
    where
        C: Component,
        F: FnOnce(C::Props) -> C,
    {
        self.tree.render_child_with_props::<C, F>(
            self.parent,
            key.into(),
            props,
            self.generation,
            constructor,
        )
    }
}
