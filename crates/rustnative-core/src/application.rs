//! Multi-window orchestration: opening/closing windows, routing events and
//! task completions to the right window's [`ComponentTree`], and applying
//! deferred window-open/close requests queued by components (see
//! [`crate::component::WindowRequests`]).
//!
//! This is a distinct responsibility from [`crate::window`] (which owns
//! what a window *is* and its live state) and from [`crate::component`]
//! (which owns one window's tree in isolation): coordinating *several*
//! independent trees, each with their own scheduler and services handle, is
//! its own concern — see the standards audit's P1.21 finding on module
//! boundaries for why this split exists.

use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::component::{Component, ComponentTree, RenderError, WindowCommand};
use crate::event::Event;
use crate::identity::WindowId;
use crate::node::Node;
use crate::panic::{PanicAction, PanicPolicy, PanicReport};
use crate::scheduler::Scheduler;
use crate::services::Services;
use crate::style::Theme;
use crate::window::{Window, WindowState, apply_window_event, event_window_id};

struct WindowEntry {
    window: Window,
    state: WindowState,
    components: ComponentTree,
}

/// Owns every open window's component tree and routes events, task
/// completions, and deferred window-open/close requests between them.
///
/// # Example
///
/// ```
/// use rustnative_core::{
///     Application, Component, Event, Node, PanicPolicy, Size, Window, WindowId,
/// };
/// # struct Counter { count: u32 }
/// # impl Component for Counter {
/// #     type Props = ();
/// #     type Message = ();
/// #     fn new((): Self::Props) -> Self { Self { count: 0 } }
/// #     fn props(&self) -> &Self::Props { &() }
/// #     fn set_props(&mut self, (): Self::Props) {}
/// #     fn view(&self) -> Node { Node::button("increment", "Increment") }
/// #     fn update(&mut self, event: Event) {
/// #         if let Event::Click { .. } = event { self.count += 1; }
/// #     }
/// # }
///
/// let mut application = Application::new(
///     Counter::new(()),
///     Window::new("Counter", Size::new(400, 300)),
/// );
///
/// // A component panic would otherwise end the application; this one would
/// // rather lose a single window.
/// application.set_panic_policy(PanicPolicy::CloseWindow);
///
/// // Secondary windows get their own component tree and their own scheduler.
/// let second = application.open_window(
///     Counter::new(()),
///     Window::new("Another Counter", Size::new(300, 200)),
///     None,
/// );
/// assert_eq!(application.window_ids(), vec![WindowId::PRIMARY, second]);
///
/// // Events are routed per window; each tree keeps its own state.
/// application.dispatch_to_window(second, Event::Click {
///     target: rustnative_core::NodeId::from_key("increment"),
/// });
///
/// application.close_window(second);
/// assert_eq!(application.window_ids(), vec![WindowId::PRIMARY]);
/// ```
///
/// A platform backend takes it from here: `WindowsPlatform::run(&mut application)`
/// creates the native windows, runs the message loop, and feeds real input
/// back through `dispatch_to_window`.
pub struct Application {
    windows: HashMap<WindowId, WindowEntry>,
    primary_window: WindowId,
    next_window_id: u64,
    services: Services,
    theme: Theme,
    panic_policy: PanicPolicy,
    motion: crate::MotionPreference,
    /// The executor every window's scheduler runs on, when the host supplied
    /// one (see [`Self::with_executor`]); otherwise each window's scheduler
    /// uses the shared default.
    executor: Option<std::sync::Arc<dyn crate::Executor>>,
    /// The environment every window's root starts from.
    environment: crate::environment::Environment,
    /// Tracing, history, recording, the overlay, and the inspection
    /// server (`crate::inspect`).
    pub(crate) inspection: crate::inspect::Inspection,
    /// The host's colors, once a backend has read them.
    host_palette: Option<crate::style::HostPalette>,
}

impl fmt::Debug for Application {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Application")
            .field("windows", &self.windows.len())
            .field("primary_window", &self.primary_window)
            .finish_non_exhaustive()
    }
}

impl Application {
    /// Creates an application with a single primary window, default
    /// services, and the default theme.
    pub fn new<C: Component>(component: C, window: Window) -> Self {
        Self::with_services_and_theme(component, window, Services::default(), Theme::default())
    }

    /// Creates an application with a single primary window and `services`,
    /// using the default theme.
    pub fn with_services<C: Component>(component: C, window: Window, services: Services) -> Self {
        Self::with_services_and_theme(component, window, services, Theme::default())
    }

    /// Creates an application with a single primary window, `services`, and
    /// `theme`.
    pub fn with_services_and_theme<C: Component>(
        component: C,
        window: Window,
        services: Services,
        theme: Theme,
    ) -> Self {
        Self::build(component, window, services, theme, None)
    }

    /// Creates an application whose every window schedules its tasks on
    /// `executor` — a host's own executor, or a [`crate::ManualExecutor`] so
    /// that a test controls every task and every delay.
    pub fn with_executor<C: Component>(
        component: C,
        window: Window,
        services: Services,
        theme: Theme,
        executor: std::sync::Arc<dyn crate::Executor>,
    ) -> Self {
        Self::build(component, window, services, theme, Some(executor))
    }

    fn scheduler_for_new_window(
        executor: Option<&std::sync::Arc<dyn crate::Executor>>,
    ) -> Scheduler {
        executor.map_or_else(Scheduler::new, |executor| {
            Scheduler::with_executor(std::sync::Arc::clone(executor))
        })
    }

    fn build<C: Component>(
        component: C,
        window: Window,
        services: Services,
        theme: Theme,
        executor: Option<std::sync::Arc<dyn crate::Executor>>,
    ) -> Self {
        let primary_window = WindowId::PRIMARY;
        let entry = WindowEntry {
            state: WindowState::new(window.size()),
            window,
            components: ComponentTree::with_scheduler(
                component,
                services.clone(),
                theme.clone(),
                Self::scheduler_for_new_window(executor.as_ref()),
            ),
        };
        let mut windows = HashMap::new();
        windows.insert(primary_window, entry);
        let mut application = Self {
            windows,
            primary_window,
            next_window_id: 1,
            services,
            theme,
            panic_policy: PanicPolicy::default(),
            motion: crate::MotionPreference::default(),
            executor,
            environment: crate::environment::Environment::new(),
            inspection: crate::inspect::Inspection::default(),
            host_palette: None,
        };
        application.update_size_class(WindowId::PRIMARY);
        // A component may request another window from its first render. The
        // root tree is rendered while this Application is being
        // constructed, so drain those requests only after the primary
        // entry is installed.
        application.apply_queued_window_commands();
        application
    }

    /// Moves the application out, leaving an empty one with no windows in
    /// its place.
    ///
    /// For a host whose loop the application cannot block in: Android's
    /// main thread belongs to the host's own loop, so its backend's
    /// `Platform::run` adopts the application it was handed and returns,
    /// and the host's loop drives it from then on. What is left behind has
    /// no windows, no components, and default services; dropping it does
    /// nothing.
    ///
    /// ```
    /// use rustnative_core::{Application, Component, Event, Node, Size, Window};
    /// # struct Root;
    /// # impl Component for Root {
    /// #     type Props = ();
    /// #     type Message = ();
    /// #     fn new((): Self::Props) -> Self { Self }
    /// #     fn props(&self) -> &Self::Props { &() }
    /// #     fn set_props(&mut self, (): Self::Props) {}
    /// #     fn view(&self) -> Node { Node::label("greeting", "Hello") }
    /// #     fn update(&mut self, _event: Event) {}
    /// # }
    /// let mut application = Application::new(Root, Window::new("Root", Size::new(320, 200)));
    /// let adopted = application.take();
    /// assert_eq!(adopted.window_ids().len(), 1);
    /// assert!(application.window_ids().is_empty());
    /// ```
    #[must_use]
    pub fn take(&mut self) -> Self {
        let empty = Self {
            windows: HashMap::new(),
            primary_window: WindowId::PRIMARY,
            next_window_id: 1,
            services: Services::default(),
            theme: Theme::default(),
            panic_policy: PanicPolicy::default(),
            motion: crate::MotionPreference::default(),
            executor: None,
            environment: crate::environment::Environment::new(),
            inspection: crate::inspect::Inspection::default(),
            host_palette: None,
        };
        std::mem::replace(self, empty)
    }

    /// Dispatches `event` to the primary window. Returns whether it was
    /// handled.
    pub fn dispatch(&mut self, event: Event) -> bool {
        self.dispatch_to_window(self.primary_window, event)
    }

    /// Dispatches `event` to window `id`. Returns whether it was handled;
    /// returns `false` without effect if `event` names a different window,
    /// or if `id` does not name an open window.
    pub fn dispatch_to_window(&mut self, id: WindowId, event: Event) -> bool {
        if !self.inspection.active() {
            return self.dispatch_untraced(id, event);
        }
        let started = self.scheduler_for(id).map_or_else(Default::default, Scheduler::now);
        self.record_input(id, &event);
        let description = format!("{event:?}");
        let handled = self.dispatch_untraced(id, event);
        self.trace_change(id, started, |pass| crate::inspect::TraceKind::Event {
            event: description,
            handled,
            pass: handled.then_some(pass),
        });
        self.trace_failures(id);
        handled
    }

    /// Traces the failures window `id`'s error boundaries contained since
    /// the last call.
    pub(crate) fn trace_failures(&mut self, id: WindowId) {
        let Some(entry) = self.windows.get_mut(&id) else { return };
        for failure in entry.components.take_failures() {
            self.inspection.push(
                id.get(),
                0,
                crate::inspect::TraceKind::Failure {
                    component: failure.component,
                    message: failure.message,
                    attempt: failure.attempt,
                },
            );
        }
    }

    fn dispatch_untraced(&mut self, id: WindowId, event: Event) -> bool {
        if event_window_id(&event).is_some_and(|event_window| event_window != id) {
            return false;
        }
        // A minimized window's components keep their state and stop their
        // deferrable work (`PLAN.md` Milestone 54).
        if let Event::WindowStateChanged { state, .. } = &event {
            if let Some(entry) = self.windows.get_mut(&id) {
                entry
                    .components
                    .set_backgrounded(*state == crate::window::WindowPresentation::Minimized);
            }
        }

        // A menu item bound to a command invokes it.
        if let Event::MenuAction { item, .. } = &event {
            let command = self
                .windows
                .get(&id)
                .and_then(|entry| entry.window.menu())
                .and_then(|menu| menu.find(*item))
                .and_then(crate::menu::MenuItem::bound_command);
            if let Some(command) = command {
                return self.invoke_command(id, command, None);
            }
        }
        let Some(entry) = self.windows.get_mut(&id) else {
            return false;
        };
        let resized = matches!(event, Event::WindowResized { .. });
        apply_window_event(&mut entry.state, &event);
        if resized {
            // Size classes follow the window before the component hears of
            // the resize, so it renders against the new class.
            let size = entry.state.size();
            entry.components.set_environment(
                &crate::environment::keys::SIZE_CLASS,
                crate::environment::SizeClasses::of(size.width, size.height),
            );
            entry.components.set_environment(&crate::environment::keys::WINDOW_WIDTH, size.width);
        }
        let handled = entry.components.dispatch(event);
        let commands = entry.components.take_window_commands();
        self.apply_window_commands(commands);
        handled
    }

    fn update_size_class(&mut self, id: WindowId) {
        let Some(entry) = self.windows.get_mut(&id) else { return };
        let size = entry.state.size();
        entry.components.set_environment(
            &crate::environment::keys::SIZE_CLASS,
            crate::environment::SizeClasses::of(size.width, size.height),
        );
        entry.components.set_environment(&crate::environment::keys::WINDOW_WIDTH, size.width);
    }

    /// Applies deferred window-open/close requests queued by a component
    /// through `ComponentContext::windows()`. Applied after the requesting
    /// window's own dispatch/task-pump/render finishes so a component never
    /// mutates the window registry while it is itself mid-render.
    fn apply_window_commands(&mut self, commands: Vec<WindowCommand>) {
        for command in commands {
            match command {
                WindowCommand::Open(constructor) => {
                    constructor(self);
                }
                WindowCommand::Close(id) => {
                    self.close_window(id);
                }
            }
        }
    }

    /// Writes every window's buffered persisted state to the state store
    /// (see [`crate::persistence`]).
    ///
    /// A platform backend calls this before suspending or exiting; an
    /// application may call it whenever it wants a known-saved point.
    ///
    /// # Errors
    ///
    /// The first failed write. Every window is flushed regardless, and
    /// failed writes stay buffered for the next attempt.
    pub fn flush_state(&mut self) -> Result<(), crate::services::ServiceError> {
        let mut first_error = None;
        let mut ids = self.windows.keys().copied().collect::<Vec<_>>();
        ids.sort();
        for id in ids {
            if let Some(entry) = self.windows.get(&id) {
                if let Err(error) = entry.components.flush_state() {
                    first_error.get_or_insert(error);
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Whether any window has persisted-state writes not yet flushed —
    /// what a backend's idle flush waits on.
    #[must_use]
    pub fn has_unsaved_state(&self) -> bool {
        self.windows.values().any(|entry| entry.components.has_unsaved_state())
    }

    /// Tells the application it is being suspended, resumed, or terminated.
    ///
    /// Persisted state is flushed *first* for [`Lifecycle::Suspending`] and
    /// [`Lifecycle::Terminating`] — a platform may not wait for anything
    /// after this returns — and then [`Event::Lifecycle`] is delivered to
    /// the primary window's root, so a component can do what is specific
    /// to it.
    ///
    /// # Errors
    ///
    /// The flush's error; the event is delivered either way.
    ///
    /// [`Lifecycle::Suspending`]: crate::Lifecycle::Suspending
    /// [`Lifecycle::Terminating`]: crate::Lifecycle::Terminating
    pub fn lifecycle(
        &mut self,
        lifecycle: crate::lifecycle::Lifecycle,
    ) -> Result<(), crate::services::ServiceError> {
        use crate::lifecycle::Lifecycle;
        let flushed = match lifecycle {
            Lifecycle::Suspending | Lifecycle::Terminating | Lifecycle::LowMemory => {
                self.flush_state()
            }
            _ => Ok(()),
        };
        self.dispatch(Event::Lifecycle(lifecycle));
        flushed
    }

    /// Asks the application to open `url`, delivering [`Event::DeepLink`]
    /// to the primary window's root component. Returns whether it was
    /// handled.
    pub fn open_url(&mut self, url: impl Into<String>) -> bool {
        self.dispatch(Event::DeepLink { url: url.into() })
    }

    /// Returns the primary window's current rendered tree.
    ///
    /// # Panics
    ///
    /// Panics if the primary window has been closed. `Application` does not
    /// currently expose a way to close the primary window itself (only
    /// secondary windows via `ComponentContext::windows`), so this cannot
    /// happen through the public API today.
    #[must_use]
    #[allow(
        clippy::expect_used,
        reason = "an invariant this runtime itself just established, not a condition an application can \
    /// trigger — see `crate::component::RenderError` for the line this crate draws between \
    /// the two"
    )]
    pub fn view(&self) -> Node {
        self.view_for(self.primary_window).expect("primary window must exist")
    }

    /// Returns window `id`'s current rendered tree, or `None` if it is not
    /// open.
    #[must_use]
    pub fn view_for(&self, id: WindowId) -> Option<Node> {
        self.windows.get(&id).map(|entry| entry.components.view())
    }

    /// Re-renders the primary window's tree explicitly (outside of an event
    /// or task-completion transaction).
    ///
    /// # Errors
    ///
    /// See [`ComponentTree::render`].
    pub fn render(&mut self) -> Result<(), RenderError> {
        self.render_window(self.primary_window)
    }

    /// Re-renders window `id`'s tree explicitly.
    ///
    /// # Errors
    ///
    /// See [`ComponentTree::render`]. Returns `Ok(())` if `id` does not
    /// name an open window (there is nothing to render, which is not
    /// itself an error).
    pub fn render_window(&mut self, id: WindowId) -> Result<(), RenderError> {
        let result = match self.windows.get_mut(&id) {
            Some(entry) => entry.components.render(),
            None => Ok(()),
        };
        // Explicit renders have the same deferred-command guarantee as an
        // event or task transaction. Without this, a request made during a
        // first/manual render would wait for an unrelated later event.
        self.apply_queued_window_commands();
        result
    }

    /// Returns the primary window's component tree.
    #[must_use]
    pub fn components(&self) -> &ComponentTree {
        &self.windows[&self.primary_window].components
    }

    /// Returns window `id`'s component tree, or `None` if it is not open.
    #[must_use]
    pub fn components_for(&self, id: WindowId) -> Option<&ComponentTree> {
        self.windows.get(&id).map(|entry| &entry.components)
    }

    /// Rebuilds the application's services with `change` — what later
    /// windows are opened with.
    pub(crate) fn replace_services(&mut self, change: impl FnOnce(Services) -> Services) {
        self.services = change(self.services.clone());
    }

    pub(crate) fn components_mut(&mut self, id: WindowId) -> Option<&mut ComponentTree> {
        self.windows.get_mut(&id).map(|entry| &mut entry.components)
    }

    /// Pumps completed background-task results for every open window.
    /// Returns whether any window's tree changed as a result.
    pub fn pump_tasks(&mut self) -> bool {
        let mut changed = false;
        for id in self.window_ids() {
            changed |= self.pump_tasks_for(id);
        }
        changed
    }

    /// Pumps completions for one native window without causing unrelated
    /// windows to rerender.
    pub fn pump_tasks_for(&mut self, id: WindowId) -> bool {
        let started = self.scheduler_for(id).map_or_else(Default::default, Scheduler::now);
        let Some(entry) = self.windows.get_mut(&id) else {
            return false;
        };
        let changed = entry.components.pump_tasks();
        let commands = entry.components.take_window_commands();
        self.apply_window_commands(commands);
        if changed && self.inspection.active() {
            self.trace_change(id, started, |pass| crate::inspect::TraceKind::Tasks { pass });
            self.trace_failures(id);
        }
        changed
    }

    /// Drains window `id`'s pending [`crate::InputRequest`]s (pointer
    /// capture and drag feedback), or returns nothing if it is not open.
    pub fn take_input_requests(&mut self, id: WindowId) -> Vec<crate::InputRequest> {
        self.windows
            .get_mut(&id)
            .map(|entry| entry.components.take_input_requests())
            .unwrap_or_default()
    }

    /// Drains window `id`'s pending [`crate::AnimationRequest`]s, or
    /// returns nothing if it is not open.
    pub fn take_animation_requests(&mut self, id: WindowId) -> Vec<crate::AnimationRequest> {
        self.windows
            .get_mut(&id)
            .map(|entry| entry.components.take_animation_requests())
            .unwrap_or_default()
    }

    /// Whether the person has asked their system for reduced motion.
    #[must_use]
    pub const fn motion_preference(&self) -> crate::MotionPreference {
        self.motion
    }

    /// Records the platform's reduced-motion setting, for this application
    /// and every window it owns. A backend calls this at startup and
    /// whenever the system setting changes.
    pub fn set_motion_preference(&mut self, motion: crate::MotionPreference) {
        self.motion = motion;
        for entry in self.windows.values() {
            entry.components.set_motion_preference(motion);
        }
    }

    /// Returns the primary window's scheduler.
    #[must_use]
    pub fn scheduler(&self) -> &Scheduler {
        self.components().scheduler()
    }

    /// Returns window `id`'s scheduler, or `None` if it is not open.
    #[must_use]
    pub fn scheduler_for(&self, id: WindowId) -> Option<&Scheduler> {
        self.windows.get(&id).map(|entry| entry.components.scheduler())
    }

    /// Opens a new secondary window running `component`, returning its
    /// assigned id. `modal_parent`, if set, names the window this one is
    /// logically modal to (see [`WindowState::modal_parent`]).
    pub fn open_window<C: Component>(
        &mut self,
        component: C,
        window: Window,
        modal_parent: Option<WindowId>,
    ) -> WindowId {
        let id = self.allocate_window_id();
        self.windows.insert(
            id,
            WindowEntry {
                state: WindowState::new(window.size()).with_modal_parent(modal_parent),
                window,
                components: ComponentTree::with_scheduler(
                    component,
                    self.services.clone(),
                    self.theme.clone(),
                    Self::scheduler_for_new_window(self.executor.as_ref()),
                ),
            },
        );
        if let Some(entry) = self.windows.get_mut(&id) {
            entry.components.set_motion_preference(self.motion);
            entry.components.inherit_environment(&self.environment);
        }
        self.update_size_class(id);
        // The new root has already rendered and may itself have queued
        // follow-up requests. Apply them now so initial rendering is a
        // complete lifecycle transaction, including nested requests.
        self.apply_queued_window_commands();
        id
    }

    /// Closes window `id` and any window modal to it (transitively).
    /// Returns whether anything was closed; the primary window can never be
    /// closed this way and always returns `false`.
    pub fn close_window(&mut self, id: WindowId) -> bool {
        if id == self.primary_window {
            return false;
        }
        if !self.windows.contains_key(&id) {
            return false;
        }

        // A modal child cannot remain alive after its parent disappears:
        // its native backend would otherwise retain a dangling modal
        // relationship and the application would report an impossible
        // window topology.
        let mut pending = vec![id];
        let mut closing = HashSet::new();
        while let Some(current) = pending.pop() {
            if !closing.insert(current) {
                continue;
            }
            pending.extend(self.windows.iter().filter_map(|(child, entry)| {
                (entry.state.modal_parent() == Some(current)).then_some(*child)
            }));
        }
        for window in closing {
            self.windows.remove(&window);
        }
        true
    }

    /// Returns every open window's id, in a stable (sorted) order.
    #[must_use]
    pub fn window_ids(&self) -> Vec<WindowId> {
        let mut ids = self.windows.keys().copied().collect::<Vec<_>>();
        ids.sort();
        ids
    }

    /// Returns window `id`'s live state, or `None` if it is not open.
    #[must_use]
    pub fn window_state(&self, id: WindowId) -> Option<&WindowState> {
        self.windows.get(&id).map(|entry| &entry.state)
    }

    /// Returns mutable access to window `id`'s live state, or `None` if it
    /// is not open.
    pub fn window_state_mut(&mut self, id: WindowId) -> Option<&mut WindowState> {
        self.windows.get_mut(&id).map(|entry| &mut entry.state)
    }

    /// Sets what happens when a component panics inside a platform
    /// callback.
    ///
    /// Defaults to [`PanicPolicy::Terminate`]; see [`crate::panic`] for what
    /// each policy trades away and why this is the host's decision rather
    /// than the framework's.
    pub const fn set_panic_policy(&mut self, policy: PanicPolicy) {
        self.panic_policy = policy;
    }

    /// The configured component-panic policy.
    #[must_use]
    pub const fn panic_policy(&self) -> PanicPolicy {
        self.panic_policy
    }

    /// Applies the configured [`PanicPolicy`] to a caught component panic
    /// and reports what the platform backend should do.
    ///
    /// The backend calls this rather than reading the policy directly,
    /// because resolving `CloseWindow` needs to know whether any other
    /// window is open — application state the backend does not own.
    #[must_use]
    pub fn handle_component_panic(&self, report: &PanicReport) -> PanicAction {
        let other_windows_remain = self.windows.keys().any(|id| *id != report.window);
        self.panic_policy.resolve(report, other_windows_remain)
    }

    /// Delivers a slice of window `id`'s deferrable messages (see
    /// [`ComponentTree::pump_deferred`]). Returns whether anything changed.
    pub fn pump_deferred_for(&mut self, id: WindowId) -> bool {
        self.windows.get_mut(&id).is_some_and(|entry| entry.components.pump_deferred())
    }

    /// [`Self::pump_deferred_for`] every window.
    pub fn pump_deferred(&mut self) -> bool {
        let ids = self.window_ids();
        ids.into_iter().fold(false, |changed, id| self.pump_deferred_for(id) | changed)
    }

    /// Whether window `id` has deferrable messages waiting.
    #[must_use]
    pub fn has_deferred_work(&self, id: WindowId) -> bool {
        self.windows.get(&id).is_some_and(|entry| entry.components.has_deferred_work())
    }

    /// The failures window `id`'s error boundaries contained since the last
    /// call (see [`crate::component::boundary`]).
    pub fn take_failures(&mut self, id: WindowId) -> Vec<crate::Failure> {
        self.windows.get_mut(&id).map(|entry| entry.components.take_failures()).unwrap_or_default()
    }

    /// Returns the application-wide services.
    #[must_use]
    pub fn services(&self) -> &Services {
        &self.services
    }

    /// The nodes of window `id` whose laid-out size a component reads.
    #[must_use]
    pub fn watched_nodes(&self, id: WindowId) -> Vec<crate::identity::NodeId> {
        self.windows.get(&id).map(|entry| entry.components.watched_nodes()).unwrap_or_default()
    }

    /// Reports laid-out sizes of watched nodes in window `id` (see
    /// [`ComponentTree::report_sizes`]). Returns whether anything
    /// re-rendered.
    pub fn report_sizes(
        &mut self,
        id: WindowId,
        sizes: impl IntoIterator<Item = (crate::identity::NodeId, crate::layout::Size)>,
    ) -> bool {
        let Some(entry) = self.windows.get_mut(&id) else { return false };
        let changed = entry.components.report_sizes(sizes);
        let commands = entry.components.take_window_commands();
        self.apply_window_commands(commands);
        changed
    }

    /// Invokes command `command` in window `id`, routed from `focused`.
    pub fn invoke_command(
        &mut self,
        id: WindowId,
        command: crate::command::CommandId,
        focused: Option<crate::identity::NodeId>,
    ) -> bool {
        let Some(entry) = self.windows.get_mut(&id) else { return false };
        let invoked = entry.components.invoke_command(command, focused);
        let commands = entry.components.take_window_commands();
        self.apply_window_commands(commands);
        invoked
    }

    /// Reports the host's back gesture progressing toward the back command
    /// ([`crate::command::standard::BACK`]) in window `id`, delivering
    /// [`Event::BackProgress`] to the component that declared it enabled.
    /// Returns whether one did — when none does, the host handles back
    /// itself and no progress is reported. A completed gesture is
    /// [`Self::invoke_command`] with `BACK`.
    pub fn back_progress(
        &mut self,
        id: WindowId,
        phase: crate::event::BackPhase,
        focused: Option<crate::identity::NodeId>,
    ) -> bool {
        let Some(entry) = self.windows.get_mut(&id) else { return false };
        let delivered = entry.components.deliver_to_command(
            crate::command::standard::BACK,
            focused,
            &Event::BackProgress { window: id, phase },
        );
        let commands = entry.components.take_window_commands();
        self.apply_window_commands(commands);
        delivered
    }

    /// Whether window `id` has a component that handles the back command
    /// right now — what a host asks before claiming its back gesture, so
    /// that with nothing to go back to, the host's own back (leaving the
    /// application, with its own animation) happens instead.
    #[must_use]
    pub fn handles_back(&self, id: WindowId, focused: Option<crate::identity::NodeId>) -> bool {
        self.command_state(id, crate::command::standard::BACK, focused)
            .is_some_and(|command| command.is_enabled())
    }

    /// Offers a key press to window `id`'s command shortcuts before it is
    /// delivered as an ordinary key event. Returns whether a command took
    /// it — in which case the backend must not deliver it further.
    pub fn handle_shortcut(
        &mut self,
        id: WindowId,
        key: crate::event::KeyCode,
        modifiers: crate::event::KeyModifiers,
        focused: Option<crate::identity::NodeId>,
    ) -> bool {
        let Some(entry) = self.windows.get_mut(&id) else { return false };
        let handled = entry.components.handle_shortcut(key, modifiers, focused);
        let commands = entry.components.take_window_commands();
        self.apply_window_commands(commands);
        handled
    }

    /// Command `command` as window `id`'s focus chain from `focused` sees
    /// it — what a menu item bound to it shows.
    #[must_use]
    pub fn command_state(
        &self,
        id: WindowId,
        command: crate::command::CommandId,
        focused: Option<crate::identity::NodeId>,
    ) -> Option<crate::command::Command> {
        let entry = self.windows.get(&id)?;
        let chain = entry.components.focus_chain(focused);
        entry.components.commands().state(command, &chain).cloned()
    }

    /// Sets `key` in every window's root environment (and in the
    /// environment windows opened later start with). Only components that
    /// read the key re-render. This is how a backend reports a host
    /// setting: the colour scheme, the text scale, reduced motion.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "an environment value is owned by the environment; callers hand it over"
    )]
    pub fn set_environment<T: crate::environment::EnvValue>(
        &mut self,
        key: &crate::environment::EnvKey<T>,
        value: T,
    ) {
        let _ = self.environment.set(key, value.clone(), 0);
        for entry in self.windows.values_mut() {
            entry.components.set_environment(key, value.clone());
        }
        self.apply_queued_window_commands();
    }

    /// Sets `key` in one window's root environment only — for a trait that
    /// belongs to a window rather than the application (its size classes,
    /// whether it is snapped). Returns whether the window exists.
    pub fn set_window_environment<T: crate::environment::EnvValue>(
        &mut self,
        id: WindowId,
        key: &crate::environment::EnvKey<T>,
        value: T,
    ) -> bool {
        let Some(entry) = self.windows.get_mut(&id) else { return false };
        entry.components.set_environment(key, value);
        self.apply_queued_window_commands();
        true
    }

    /// Sets the locale, and the layout direction its script is written in.
    pub fn set_locale(&mut self, locale: crate::environment::Locale) {
        let direction = locale.direction();
        self.set_environment(&crate::environment::keys::LOCALE, locale);
        self.set_environment(&crate::environment::keys::LAYOUT_DIRECTION, direction);
    }

    /// The value of `key` in window `id`'s root environment.
    #[must_use]
    pub fn environment_for<T: crate::environment::EnvValue>(
        &self,
        id: WindowId,
        key: &crate::environment::EnvKey<T>,
    ) -> T {
        self.windows
            .get(&id)
            .map_or_else(|| key.default_value(), |entry| entry.components.environment(key))
    }

    /// Window `id`'s layout direction — what a backend lays its root out
    /// in.
    #[must_use]
    pub fn layout_direction(&self, id: WindowId) -> crate::layout::LayoutDirection {
        self.environment_for(id, &crate::environment::keys::LAYOUT_DIRECTION)
    }

    /// Replaces the theme for every window and re-renders them all. See
    /// [`ComponentTree::set_theme`]: a backend applies the result to its
    /// existing native objects, creating none.
    pub fn set_theme(&mut self, theme: Theme) {
        // A theme with host-following tokens takes the host's current
        // colors (`docs/tokens.md`).
        let theme = match &self.host_palette {
            Some(palette) if !theme.host_roles().is_empty() => theme.with_host_palette(palette),
            _ => theme,
        };
        self.theme = theme;
        for entry in self.windows.values_mut() {
            let _ = entry.components.set_theme(self.theme.clone());
        }
        self.apply_queued_window_commands();
    }

    /// Records the host's colors, and re-themes every window whose theme
    /// has tokens following them — what a backend calls at start and when
    /// the person changes their colors.
    pub fn set_host_palette(&mut self, palette: crate::style::HostPalette) {
        if self.host_palette == Some(palette) {
            return;
        }
        self.host_palette = Some(palette);
        if !self.theme.host_roles().is_empty() {
            let theme = self.theme.clone();
            self.set_theme(theme);
        }
    }

    /// Returns the application-wide theme.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Returns the primary window's definition.
    #[must_use]
    pub fn window(&self) -> &Window {
        &self.windows[&self.primary_window].window
    }

    /// Returns window `id`'s definition, or `None` if it is not open.
    #[must_use]
    pub fn window_for(&self, id: WindowId) -> Option<&Window> {
        self.windows.get(&id).map(|entry| &entry.window)
    }

    fn allocate_window_id(&mut self) -> WindowId {
        WindowId::next(&mut self.next_window_id)
    }

    fn apply_queued_window_commands(&mut self) {
        loop {
            let commands = self
                .window_ids()
                .into_iter()
                .flat_map(|id| {
                    self.windows
                        .get_mut(&id)
                        .map(|entry| entry.components.take_window_commands())
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>();
            if commands.is_empty() {
                break;
            }
            self.apply_window_commands(commands);
        }
    }
}
