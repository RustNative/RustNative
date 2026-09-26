//! The framework-managed, keyed component tree: creates/reuses child
//! components across renders, scopes their node identities (see
//! `crate::identity`), routes events and task results to the right
//! component, and commits reactive effects.

use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use super::Component;
use super::context::{
    AnimationRequest, ComponentContext, InputRequest, QueuedMessage, WindowCommand,
};
use super::effects::{DeclaredEffect, EffectContext, EffectDependencies, EffectEntry};
use super::error::RenderError;
use super::invalidation::{RenderCause, RenderRecord};
use crate::environment::{EnvKey, EnvValue, Environment, Preference, PreferenceKey, Stored};
use crate::event::Event;
use crate::identity::{ComponentId, NodeId};
use crate::node::Node;
use crate::scheduler::{CompletedTask, Scheduler, TaskScope};
use crate::services::Services;
use crate::style::Theme;

/// The descendants that published one preference, with the versions seen.
type Publishers = Vec<(ComponentId, u64)>;

/// Object-safe facade over `Component`, letting `ComponentTree` store
/// heterogeneous component types behind one `Box<dyn ManagedComponent>` per
/// entry. Application code never implements this directly — it exists only
/// to erase `Component::Props`/`Component::Message` for tree bookkeeping.
trait ManagedComponent {
    fn render(
        &mut self,
        tree: &mut ComponentTree,
        id: ComponentId,
        generation: u64,
        task_scope: TaskScope,
    ) -> Node;
    fn update(&mut self, event: Event);
    fn message_any(&mut self, message: Box<dyn Any>) -> bool;
    fn props_equal(&self, props: &dyn Any) -> bool;
    fn set_props_any(&mut self, props: Box<dyn Any>) -> bool;
    fn props_changed(&mut self);
    fn updated(&mut self);
    fn unmounted(&mut self);
    fn type_name(&self) -> &'static str;
    fn inspect(&self) -> Option<serde_json::Value>;
    fn edit(&self, field: &str, value: &serde_json::Value) -> Option<Box<dyn Any>>;
}

impl<C: Component> ManagedComponent for C {
    fn render(
        &mut self,
        tree: &mut ComponentTree,
        id: ComponentId,
        generation: u64,
        task_scope: TaskScope,
    ) -> Node {
        let mut context = ComponentContext::<C::Message> {
            tree,
            parent: id,
            generation,
            task_scope,
            _message: std::marker::PhantomData,
        };
        Component::render(self, &mut context)
    }

    fn update(&mut self, event: Event) {
        Component::update(self, event);
    }

    fn message_any(&mut self, message: Box<dyn Any>) -> bool {
        let Ok(message) = message.downcast::<C::Message>() else {
            return false;
        };
        Component::message(self, *message);
        true
    }

    fn props_equal(&self, props: &dyn Any) -> bool {
        props.downcast_ref::<C::Props>().is_some_and(|incoming| self.props() == incoming)
    }

    fn set_props_any(&mut self, props: Box<dyn Any>) -> bool {
        let Ok(props) = props.downcast::<C::Props>() else {
            return false;
        };
        Component::set_props(self, *props);
        true
    }

    fn props_changed(&mut self) {
        Component::props_changed(self);
    }

    fn updated(&mut self) {
        Component::updated(self);
    }

    fn unmounted(&mut self) {
        Component::unmounted(self);
    }

    fn type_name(&self) -> &'static str {
        std::any::type_name::<C>()
    }

    fn inspect(&self) -> Option<serde_json::Value> {
        Component::inspect(self)
    }

    fn edit(&self, field: &str, value: &serde_json::Value) -> Option<Box<dyn Any>> {
        Component::edit(self, field, value).map(|message| Box::new(message) as Box<dyn Any>)
    }
}

struct ComponentEntry {
    component: Box<dyn ManagedComponent>,
    component_type: std::any::TypeId,
    children: HashMap<String, ComponentId>,
    view: Option<Node>,
    // Global native-tree ID -> component-local ID. This keeps component
    // event handlers independent of the composition path that realizes
    // their view.
    node_ids: HashMap<NodeId, NodeId>,
    used_generation: u64,
    task_scope: TaskScope,
    effects: HashMap<String, EffectEntry>,
}

/// Framework-owned keyed component tree.
///
/// Child components are created/reused during `render()`. A child that is
/// no longer requested by its parent is structurally removed and receives
/// exactly one `unmounted()` callback. Reappearing with the same key
/// creates a new instance, which receives a fresh `mounted()` callback.
pub struct ComponentTree {
    components: HashMap<ComponentId, ComponentEntry>,
    pending_children: HashMap<ComponentId, HashMap<String, ComponentId>>,
    node_owners: HashMap<NodeId, (ComponentId, NodeId)>,
    local_node_owners: HashMap<NodeId, Vec<(ComponentId, NodeId)>>,
    generation: u64,
    next_component_id: u64,
    root_view: Option<Node>,
    /// The last render's output before style resolution, kept while any
    /// node carries declarations, so an environment change can re-resolve
    /// without re-rendering.
    unresolved_root: Option<Node>,
    message_sink: Rc<RefCell<VecDeque<QueuedMessage>>>,
    window_commands: Rc<RefCell<VecDeque<WindowCommand>>>,
    input_requests: Rc<RefCell<VecDeque<InputRequest>>>,
    animation_requests: Rc<RefCell<VecDeque<AnimationRequest>>>,
    motion: std::cell::Cell<crate::animation::MotionPreference>,
    scheduler: Scheduler,
    /// The pool `!Send` component tasks run on (see
    /// [`crate::LocalExecutor`]), polled by [`Self::pump_tasks`].
    local: Rc<dyn crate::LocalExecutor>,
    pending_effects: HashMap<ComponentId, Vec<DeclaredEffect>>,
    /// Structured, user-triggerable composition problems collected during
    /// the render pass currently in progress; drained (and the first one
    /// surfaced) at the end of [`Self::render`]. See
    /// [`RenderError`]'s own docs for why these do not panic.
    pending_render_errors: Vec<RenderError>,
    last_render_error: Option<RenderError>,
    services: Services,
    theme: Theme,
    /// Each component's key path — the chain of child keys from this tree's
    /// root — which is what persisted state is keyed by (see
    /// [`crate::persistence`]).
    paths: HashMap<ComponentId, String>,
    /// Buffered persisted state for this window.
    state: Rc<RefCell<crate::persistence::StateCache>>,
    /// Each non-root component's parent — what environment lookup walks.
    parents: HashMap<ComponentId, ComponentId>,
    /// Components that must render in the next pass, and why.
    dirty: HashMap<ComponentId, RenderCause>,
    /// Ancestors of dirty components in the pass in progress: reused, but
    /// with their dirty descendants' new output spliced in.
    on_path: HashSet<ComponentId>,
    /// Whether the pass in progress renders every component.
    force_pass: bool,
    /// The window's root environment.
    environment: Environment,
    /// The last version handed out to an environment value.
    env_version: u64,
    /// What each component provides to its descendants.
    provided: HashMap<ComponentId, Environment>,
    /// What a component provided in its previous render, while it renders
    /// again — so an unchanged value keeps its version.
    provided_previous: HashMap<ComponentId, Environment>,
    /// The environment values each component read, and their versions.
    env_seen: HashMap<ComponentId, HashMap<&'static str, u64>>,
    /// Upward preferences each component published in its last render.
    published: HashMap<ComponentId, HashMap<&'static str, Stored>>,
    /// The preference publishers each component read, and their versions.
    preferences_seen: HashMap<ComponentId, HashMap<&'static str, Publishers>>,
    /// Who rendered in the most recent pass, and why.
    render_log: Vec<RenderRecord>,
    /// The commands declared in the last render of each component.
    commands: crate::command::CommandRegistry,
    /// Laid-out sizes the backend reported for watched nodes.
    node_sizes: HashMap<NodeId, crate::layout::Size>,
    /// The container size classes each component read, per node.
    size_reads: HashMap<ComponentId, Vec<(NodeId, crate::environment::SizeClasses)>>,
    /// Each component's store selections from its last render.
    selections: HashMap<ComponentId, Vec<crate::state::Selection>>,
    /// The store epoch the last check for stale selections saw.
    seen_epoch: u64,
    /// Values each component provides to its subtree by type.
    scoped: HashMap<ComponentId, crate::state::ScopedValues>,
    /// The error boundaries, by component.
    boundaries: HashMap<ComponentId, Rc<RefCell<super::boundary::BoundaryState>>>,
    /// Failures contained since [`Self::take_failures`] was last called.
    failures: Vec<super::boundary::Failure>,
    /// Deferrable messages waiting for nothing more urgent (Milestone 54).
    deferred_messages: VecDeque<QueuedMessage>,
    /// How long one slice of deferrable work may take.
    render_budget: std::time::Duration,
    /// Components currently suspended because their output is hidden.
    hidden: HashSet<ComponentId>,
    /// Whether the whole window is in the background (minimized).
    backgrounded: bool,
    /// Deferred values, by component and key.
    deferreds: HashMap<(ComponentId, String), Rc<dyn Any>>,
    /// Props types seen to compare unequal to their own clone.
    unskippable: std::collections::BTreeSet<&'static str>,
}

impl ComponentTree {
    /// Creates a tree rooted at `root`, with default services and theme.
    pub fn new<C: Component>(root: C) -> Self {
        Self::with_services(root, Services::default())
    }

    /// Creates a tree rooted at `root`, with `services` and the default
    /// theme.
    pub fn with_services<C: Component>(root: C, services: Services) -> Self {
        Self::with_services_and_theme(root, services, Theme::default())
    }

    /// Creates a tree rooted at `root`, with `services` and `theme`.
    pub fn with_services_and_theme<C: Component>(
        root: C,
        services: Services,
        theme: Theme,
    ) -> Self {
        Self::with_scheduler(root, services, theme, Scheduler::new())
    }

    /// Creates a tree rooted at `root` whose tasks run on `scheduler` — for
    /// a host with its own executor, and for tests that drive a
    /// [`crate::ManualExecutor`] so every task and delay is deterministic.
    pub fn with_scheduler<C: Component>(
        mut root: C,
        services: Services,
        theme: Theme,
        scheduler: Scheduler,
    ) -> Self {
        root.mounted();
        let local: Rc<dyn crate::LocalExecutor> =
            Rc::new(crate::LocalPool::new(scheduler.host_waker()));
        let state = crate::persistence::StateCache::new(services.state_store().cloned());
        let mut tree = Self {
            components: HashMap::new(),
            pending_children: HashMap::new(),
            node_owners: HashMap::new(),
            local_node_owners: HashMap::new(),
            generation: 0,
            next_component_id: 1,
            root_view: None,
            unresolved_root: None,
            message_sink: Rc::new(RefCell::new(VecDeque::new())),
            window_commands: Rc::new(RefCell::new(VecDeque::new())),
            input_requests: Rc::new(RefCell::new(VecDeque::new())),
            animation_requests: Rc::new(RefCell::new(VecDeque::new())),
            motion: std::cell::Cell::new(crate::animation::MotionPreference::default()),
            scheduler,
            local,
            pending_effects: HashMap::new(),
            pending_render_errors: Vec::new(),
            last_render_error: None,
            services,
            theme,
            // The root's path is its type's name: stable across runs of the
            // same program, and distinct for windows of different kinds.
            paths: HashMap::from([(ComponentId::ROOT, std::any::type_name::<C>().to_owned())]),
            state: Rc::new(RefCell::new(state)),
            parents: HashMap::new(),
            dirty: HashMap::new(),
            on_path: HashSet::new(),
            force_pass: false,
            environment: Environment::new(),
            env_version: 0,
            provided: HashMap::new(),
            provided_previous: HashMap::new(),
            env_seen: HashMap::new(),
            published: HashMap::new(),
            preferences_seen: HashMap::new(),
            render_log: Vec::new(),
            commands: crate::command::CommandRegistry::default(),
            node_sizes: HashMap::new(),
            size_reads: HashMap::new(),
            selections: HashMap::new(),
            seen_epoch: crate::state::epoch(),
            scoped: HashMap::new(),
            boundaries: HashMap::new(),
            failures: Vec::new(),
            deferred_messages: VecDeque::new(),
            render_budget: std::time::Duration::from_millis(4),
            hidden: HashSet::new(),
            backgrounded: false,
            deferreds: HashMap::new(),
            unskippable: std::collections::BTreeSet::new(),
        };
        let root_scope =
            TaskScope::new(tree.scheduler.clone(), Rc::clone(&tree.local), ComponentId::ROOT);
        tree.components.insert(
            ComponentId::ROOT,
            ComponentEntry {
                component_type: std::any::TypeId::of::<C>(),
                component: Box::new(root),
                children: HashMap::new(),
                view: None,
                node_ids: HashMap::new(),
                used_generation: 0,
                task_scope: root_scope,
                effects: HashMap::new(),
            },
        );
        // The initial render's result is captured into `last_render_error`
        // (inspectable via `Self::last_render_error`) rather than
        // propagated as a `Result` from this constructor: every
        // `ComponentTree`/`Application` constructor already returns `Self`
        // directly, matching this crate's existing ergonomics, and a
        // render-time composition mistake should degrade gracefully rather
        // than make construction itself fallible.
        let _ = tree.render();
        tree
    }

    /// Re-renders the declarative tree from the root.
    ///
    /// # Panics
    ///
    /// Panics if this tree has produced more than `u64::MAX` renders over
    /// its lifetime (see `crate::identity`'s allocators for why this crate
    /// treats identity/generation-space exhaustion as an unrecoverable
    /// condition rather than something to silently wrap around).
    ///
    /// # Errors
    ///
    /// Returns the first [`RenderError`] detected during this render (for
    /// example, a component that used the same node key twice in one
    /// render). The tree still completes a structurally consistent render
    /// pass regardless — the offending duplicate is simply not registered a
    /// second time — so a caller can choose to log this and continue rather
    /// than lose the application to a panic; see [`Self::last_render_error`]
    /// to inspect the most recent outcome without re-triggering a render.
    #[allow(
        clippy::expect_used,
        reason = "the standards audit's P1.20 finding requires an identity allocator to fail loudly on \
    /// exhaustion rather than silently wrap and reuse a live id; there is no caller that \
    /// could act on a `Result` here"
    )]
    pub fn render(&mut self) -> Result<(), RenderError> {
        self.render_pass(true)
    }

    /// Renders what the invalidation contract says must render — dirty
    /// components, and readers of changed environment values — reusing
    /// every other component's previous output. With `force`, renders
    /// everything.
    #[allow(
        clippy::expect_used,
        reason = "the standards audit's P1.20 finding requires an identity allocator to fail loudly on \
    /// exhaustion rather than silently wrap and reuse a live id"
    )]
    fn render_pass(&mut self, force: bool) -> Result<(), RenderError> {
        self.pending_render_errors.clear();
        self.render_log.clear();
        let mut force = force || self.root_view.is_none();
        // A provided value can change while its provider renders, after a
        // reader has already been reused in the same pass; a second pass
        // settles it. Three is a backstop, not an expectation.
        for _ in 0..3 {
            self.mark_stale_readers();
            if !force && self.dirty.is_empty() {
                break;
            }
            self.generation = self
                .generation
                .checked_add(1)
                .expect("framework render generation space exhausted");
            let generation = self.generation;
            self.force_pass = force;
            self.on_path = self.ancestors_of_dirty();
            let root = self.render_or_reuse(ComponentId::ROOT, generation);
            self.force_pass = false;
            self.on_path.clear();
            self.dirty.clear();
            let mut root = root;
            let commands = &self.commands;
            root.apply_command_states(&|id| {
                commands.state(id, &[]).is_none_or(crate::command::Command::is_enabled)
            });
            self.root_view = Some(root);
            self.rebuild_node_owners();
            self.update_visibility();
            self.resolve_styles();
            self.commit_effects();
            force = false;
        }

        self.last_render_error = self.pending_render_errors.first().cloned();
        match &self.last_render_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    /// Who rendered in the most recent render pass, and why — every other
    /// component was skipped and its previous output reused.
    #[must_use]
    pub fn last_render_log(&self) -> &[RenderRecord] {
        &self.render_log
    }

    fn mark_dirty(&mut self, id: ComponentId, cause: RenderCause) {
        self.dirty.entry(id).or_insert(cause);
    }

    fn ancestors_of_dirty(&self) -> HashSet<ComponentId> {
        let mut path = HashSet::new();
        for id in self.dirty.keys() {
            let mut current = *id;
            while let Some(parent) = self.parents.get(&current) {
                if !path.insert(*parent) {
                    break;
                }
                current = *parent;
            }
        }
        path
    }

    /// Marks every component that read an environment value or preference
    /// that has since changed.
    fn mark_stale_readers(&mut self) {
        self.mark_stale_selections();
        let mut stale = Vec::new();
        for (id, seen) in &self.env_seen {
            if let Some((name, _)) =
                seen.iter().find(|(name, version)| self.env_version_at(*id, name) != **version)
            {
                stale.push((*id, RenderCause::Environment(name)));
            }
        }
        for (id, seen) in &self.preferences_seen {
            if let Some((name, _)) =
                seen.iter().find(|(name, publishers)| &self.publishers(*id, name) != *publishers)
            {
                stale.push((*id, RenderCause::Preference(name)));
            }
        }
        for (id, cause) in stale {
            if self.components.contains_key(&id) {
                self.mark_dirty(id, cause);
            }
        }
    }

    #[allow(
        clippy::expect_used,
        reason = "an invariant this runtime itself just established, not a condition an application can \
    /// trigger"
    )]
    fn render_or_reuse(&mut self, id: ComponentId, generation: u64) -> Node {
        let has_view = self.components.get(&id).is_some_and(|entry| entry.view.is_some());
        let cause = if self.force_pass {
            Some(RenderCause::Forced)
        } else if !has_view {
            Some(RenderCause::Initial)
        } else {
            self.dirty.remove(&id)
        };
        if let Some(cause) = cause {
            let path = self.paths.get(&id).cloned().unwrap_or_default();
            self.render_log.push(RenderRecord { component: id, path, cause });
            return self.render_component(id, generation);
        }
        if self.on_path.contains(&id) {
            // Clean itself, but a descendant is not: keep this component's
            // output and splice in each changed child's new output.
            let (mut view, children) = {
                let entry = self
                    .components
                    .get_mut(&id)
                    .expect("component tree entry must exist while rendering");
                entry.used_generation = generation;
                (
                    entry.view.clone().expect("checked above"),
                    entry.children.values().copied().collect::<Vec<_>>(),
                )
            };
            for child in children {
                if self.dirty.contains_key(&child) || self.on_path.contains(&child) {
                    // The child's own previous output: what it set on its
                    // root itself, as opposed to what this parent added.
                    let previous_root = self
                        .components
                        .get(&child)
                        .and_then(|entry| entry.view.as_ref())
                        .map(|own| (own.id(), own.is_hidden(), own.item_index()));
                    let fresh = self.render_or_reuse(child, generation);
                    if let Some((previous_root, hidden, index)) = previous_root {
                        replace_subtree(&mut view, previous_root, fresh, (hidden, index));
                    }
                } else {
                    self.mark_used(child, generation);
                }
            }
            if let Some(entry) = self.components.get_mut(&id) {
                entry.view = Some(view.clone());
            }
            return view;
        }
        self.mark_used(id, generation);
        self.components.get(&id).and_then(|entry| entry.view.clone()).expect("checked above")
    }

    fn mark_used(&mut self, id: ComponentId, generation: u64) {
        let children = match self.components.get_mut(&id) {
            Some(entry) => {
                entry.used_generation = generation;
                entry.children.values().copied().collect::<Vec<_>>()
            }
            None => return,
        };
        for child in children {
            self.mark_used(child, generation);
        }
    }

    // -----------------------------------------------------------------
    // Environment
    // -----------------------------------------------------------------

    /// Sets `key` in this window's root environment. If the value changed,
    /// the components that read it re-render — and only they.
    pub fn set_environment<T: EnvValue>(&mut self, key: &EnvKey<T>, value: T) {
        self.env_version += 1;
        if self.environment.set(key, value, self.env_version) {
            let _ = self.render_pass(false);
            self.reresolve_styles();
        }
    }

    /// Folds every node's declarations into its typed properties
    /// (`crate::style` resolve), against the theme and each node's
    /// environment — the environment of the component that rendered it.
    fn resolve_styles(&mut self) {
        let Some(mut root) = self.root_view.take() else { return };
        let mut any = false;
        root.visit(&mut |node, _, _| any |= !node.declarations().is_empty());
        if !any {
            self.unresolved_root = None;
            self.root_view = Some(root);
            return;
        }
        let unresolved = root.clone();
        let mut cache: HashMap<Option<ComponentId>, crate::style::ResolveEnv> = HashMap::new();
        crate::style::resolve_tree(&mut root, &self.theme, &mut |id| {
            let owner = self.node_owners.get(&id).map(|(component, _)| *component);
            *cache.entry(owner).or_insert_with(|| self.style_env(owner))
        });
        // Kept whenever anything is styled: besides conditions, a length
        // in `rem` follows the text scale.
        self.unresolved_root = Some(unresolved);
        self.root_view = Some(root);
    }

    /// Re-resolves the last render's output after an environment change
    /// that re-rendered nothing (a scheme switch with no component reading
    /// the scheme still changes every `dark:` class).
    fn reresolve_styles(&mut self) {
        if let Some(unresolved) = self.unresolved_root.clone() {
            self.root_view = Some(unresolved);
            self.resolve_styles();
        }
    }

    fn env_at<T: EnvValue>(&self, owner: Option<ComponentId>, key: &EnvKey<T>) -> T {
        let stored = match owner {
            Some(component) => self.stored_at(component, key.name()),
            None => self.environment.values.get(key.name()),
        };
        stored.and_then(Stored::get::<T>).unwrap_or_else(|| key.default_value())
    }

    fn style_env(&self, owner: Option<ComponentId>) -> crate::style::ResolveEnv {
        use crate::environment::keys;
        use rustnative_style::{Direction, Pointer, Scheme};
        let condition = rustnative_style::ConditionEnv {
            scheme: match self.env_at(owner, &keys::COLOR_SCHEME) {
                crate::environment::ColorScheme::Light => Scheme::Light,
                crate::environment::ColorScheme::Dark => Scheme::Dark,
            },
            width: self.env_at(owner, &keys::WINDOW_WIDTH),
            direction: match self.env_at(owner, &keys::LAYOUT_DIRECTION) {
                crate::layout::LayoutDirection::Ltr => Direction::Ltr,
                crate::layout::LayoutDirection::Rtl => Direction::Rtl,
            },
            reduced_motion: self.env_at(owner, &keys::REDUCED_MOTION)
                == crate::animation::MotionPreference::Reduced,
            pointer: match self.env_at(owner, &keys::POINTER) {
                crate::environment::PointerPrecision::Fine => Pointer::Fine,
                crate::environment::PointerPrecision::Coarse => Pointer::Coarse,
            },
        };
        let scale = f64::from(self.env_at(owner, &keys::TEXT_SCALE).get());
        crate::style::ResolveEnv { condition, rem_px: 16.0 * scale }
    }

    /// Copies every value of `environment` into this window's root
    /// environment — how a window opened later starts from what the
    /// application's other windows already have.
    pub(crate) fn inherit_environment(&mut self, environment: &Environment) {
        let mut changed = false;
        for (name, stored) in &environment.values {
            self.env_version += 1;
            let mut stored = stored.clone();
            stored.version = self.env_version;
            self.environment.values.insert(name, stored);
            changed = true;
        }
        if changed {
            let _ = self.render_pass(false);
            self.reresolve_styles();
        }
    }

    /// The nodes whose laid-out size some component reads — what a backend
    /// reports through [`Self::report_sizes`] after each layout.
    #[must_use]
    pub fn watched_nodes(&self) -> Vec<NodeId> {
        let mut nodes: Vec<NodeId> =
            self.size_reads.values().flatten().map(|(node, _)| *node).collect();
        nodes.sort_unstable();
        nodes.dedup();
        nodes
    }

    /// Records laid-out sizes for watched nodes (`C22`: container-relative
    /// decisions from the constraints layout already has). A component that
    /// read a node's size classes re-renders only if a class changed — a
    /// one-pixel resize re-renders nobody. Returns whether anything
    /// re-rendered, in which case the backend lays out again.
    pub fn report_sizes(
        &mut self,
        sizes: impl IntoIterator<Item = (NodeId, crate::layout::Size)>,
    ) -> bool {
        for (node, size) in sizes {
            self.node_sizes.insert(node, size);
        }
        let mut stale = Vec::new();
        for (component, reads) in &self.size_reads {
            let changed = reads.iter().any(|(node, seen)| {
                self.node_sizes.get(node).is_some_and(|size| {
                    crate::environment::SizeClasses::of(size.width, size.height) != *seen
                })
            });
            if changed {
                stale.push(*component);
            }
        }
        if stale.is_empty() {
            return false;
        }
        for component in stale {
            self.mark_dirty(component, RenderCause::Environment("rustnative.container-size"));
        }
        let _ = self.render_pass(false);
        true
    }

    pub(crate) fn read_container_classes(
        &mut self,
        owner: ComponentId,
        local: NodeId,
    ) -> Option<crate::environment::SizeClasses> {
        let node = if owner == ComponentId::ROOT { local } else { NodeId::scoped(owner, local) };
        let size = self.node_sizes.get(&node).copied();
        let classes = size.map_or_else(crate::environment::SizeClasses::default, |size| {
            crate::environment::SizeClasses::of(size.width, size.height)
        });
        self.size_reads.entry(owner).or_default().push((node, classes));
        size.map(|_| classes)
    }

    /// The commands declared in this window's last render.
    #[must_use]
    pub fn commands(&self) -> &crate::command::CommandRegistry {
        &self.commands
    }

    pub(crate) fn declare_command(&mut self, owner: ComponentId, command: crate::command::Command) {
        self.commands.declare(owner, command);
    }

    /// The focus chain for `focused`: the component owning that node, then
    /// its ancestors — the order commands are routed in.
    #[must_use]
    pub fn focus_chain(&self, focused: Option<NodeId>) -> Vec<ComponentId> {
        let mut chain = Vec::new();
        let mut current =
            focused.and_then(|node| self.node_owners.get(&node)).map(|(owner, _)| *owner);
        while let Some(owner) = current {
            chain.push(owner);
            current = self.parents.get(&owner).copied();
        }
        chain
    }

    /// Invokes command `id` as the focus chain from `focused` routes it,
    /// delivering [`Event::Command`] to the declaring component. Returns
    /// whether a declaration was found and enabled.
    pub fn invoke_command(
        &mut self,
        id: crate::command::CommandId,
        focused: Option<NodeId>,
    ) -> bool {
        let chain = self.focus_chain(focused);
        let Some((owner, command)) = self.commands.resolve(id, &chain) else {
            return false;
        };
        if !command.is_enabled() {
            return false;
        }
        self.update_component(owner, &Event::Command { id });
        if !self.message_sink.borrow().is_empty() {
            self.drain_messages();
        }
        let _ = self.render_pass(false);
        true
    }

    /// Invokes the command whose shortcut is `key` with `modifiers`, if an
    /// enabled one is declared. Returns whether one was.
    pub fn handle_shortcut(
        &mut self,
        key: crate::event::KeyCode,
        modifiers: crate::event::KeyModifiers,
        focused: Option<NodeId>,
    ) -> bool {
        let chain = self.focus_chain(focused);
        let Some((_, command)) = self.commands.for_shortcut(key, modifiers, &chain) else {
            return false;
        };
        let id = command.id();
        self.invoke_command(id, focused)
    }

    /// The value of `key` in this window's root environment.
    #[must_use]
    pub fn environment<T: EnvValue>(&self, key: &EnvKey<T>) -> T {
        self.environment.get(key)
    }

    /// The whole root environment.
    #[must_use]
    pub fn root_environment(&self) -> &Environment {
        &self.environment
    }

    fn stored_at(&self, id: ComponentId, name: &str) -> Option<&Stored> {
        let mut current = self.parents.get(&id).copied();
        while let Some(ancestor) = current {
            if let Some(stored) = self.provided.get(&ancestor).and_then(|env| env.values.get(name))
            {
                return Some(stored);
            }
            current = self.parents.get(&ancestor).copied();
        }
        self.environment.values.get(name)
    }

    fn env_version_at(&self, id: ComponentId, name: &str) -> u64 {
        self.stored_at(id, name).map_or(0, |stored| stored.version)
    }

    pub(crate) fn read_env<T: EnvValue>(&mut self, id: ComponentId, key: &EnvKey<T>) -> T {
        let (value, version) = match self.stored_at(id, key.name()) {
            Some(stored) => (stored.get::<T>(), stored.version),
            None => (None, 0),
        };
        self.env_seen.entry(id).or_default().insert(key.name(), version);
        value.unwrap_or_else(|| key.default_value())
    }

    pub(crate) fn provide_env<T: EnvValue>(&mut self, id: ComponentId, key: &EnvKey<T>, value: T) {
        let previous = self
            .provided_previous
            .get(&id)
            .and_then(|env| env.values.get(key.name()))
            .filter(|stored| stored.same_as(&value))
            .map(|stored| stored.version);
        let version = previous.unwrap_or_else(|| {
            self.env_version += 1;
            self.env_version
        });
        self.provided.entry(id).or_default().values.insert(key.name(), Stored::new(version, value));
    }

    pub(crate) fn publish<T: Preference>(
        &mut self,
        id: ComponentId,
        key: &PreferenceKey<T>,
        value: T,
    ) {
        let unchanged = self
            .published
            .get(&id)
            .and_then(|values| values.get(key.name()))
            .is_some_and(|stored| stored.same_as(&value));
        if unchanged {
            return;
        }
        self.env_version += 1;
        let version = self.env_version;
        self.published.entry(id).or_default().insert(key.name(), Stored::new(version, value));
    }

    /// Every descendant of `id` that published `name`, with the version,
    /// in a stable order.
    fn publishers(&self, id: ComponentId, name: &str) -> Vec<(ComponentId, u64)> {
        let mut out = Vec::new();
        // While `id` itself renders, its entry is out of the map; its
        // children as of the start of that render are in `pending_children`.
        let mut stack: Vec<ComponentId> = self
            .components
            .get(&id)
            .map(|entry| entry.children.values().copied().collect())
            .or_else(|| {
                self.pending_children.get(&id).map(|children| children.values().copied().collect())
            })
            .unwrap_or_default();
        stack.sort_unstable_by(|a, b| b.cmp(a));
        while let Some(current) = stack.pop() {
            if let Some(stored) = self.published.get(&current).and_then(|values| values.get(name)) {
                out.push((current, stored.version));
            }
            if let Some(entry) = self.components.get(&current) {
                let mut children: Vec<_> = entry.children.values().copied().collect();
                children.sort_unstable_by(|a, b| b.cmp(a));
                stack.extend(children);
            }
        }
        out
    }

    pub(crate) fn read_preference<T: Preference>(
        &mut self,
        id: ComponentId,
        key: &PreferenceKey<T>,
    ) -> Option<T> {
        let publishers = self.publishers(id, key.name());
        let value = publishers
            .iter()
            .filter_map(|(publisher, _)| {
                self.published
                    .get(publisher)
                    .and_then(|values| values.get(key.name()))
                    .and_then(Stored::get::<T>)
            })
            .reduce(T::reduce);
        self.preferences_seen.entry(id).or_default().insert(key.name(), publishers);
        value
    }

    /// The first [`RenderError`] detected during the most recent
    /// [`Self::render`] call (via any path that triggers one — `dispatch`,
    /// `pump_tasks`, or an explicit `render` call), or `None` if that render
    /// completed without one.
    #[must_use]
    pub fn last_render_error(&self) -> Option<&RenderError> {
        self.last_render_error.as_ref()
    }

    /// Returns the current rendered tree.
    ///
    /// # Panics
    ///
    /// Panics if called before the first render. In practice this cannot
    /// happen through the public API: every constructor (`new`,
    /// `with_services`, `with_services_and_theme`) performs an initial
    /// render before returning.
    #[must_use]
    #[allow(
        clippy::expect_used,
        reason = "an invariant this runtime itself just established, not a condition an application can \
    /// trigger — see `crate::component::RenderError` for the line this crate draws between \
    /// the two"
    )]
    pub fn view(&self) -> Node {
        self.root_view.clone().expect("component tree must be rendered before its view is read")
    }

    /// Routes `event` to its owning component, re-rendering if it (or any
    /// message it caused to be sent) changed component state. Returns
    /// whether the event was delivered to a component.
    pub fn dispatch(&mut self, event: Event) -> bool {
        // A node bound to a command invokes it when activated.
        if let Event::Click { target } = &event {
            let bound = self.root_view.as_ref().and_then(|root| {
                let mut found = None;
                root.visit(&mut |node, _, _| {
                    if node.id() == *target {
                        found = node.command();
                    }
                });
                found
            });
            if let Some(command) = bound {
                return self.invoke_command(command, Some(*target));
            }
        }
        let (owner, event) = match event.target() {
            Some(target) => match self.node_owners.get(&target).copied().or_else(|| {
                self.local_node_owners
                    .get(&target)
                    .filter(|owners| owners.len() == 1)
                    .and_then(|owners| owners.first().copied())
            }) {
                Some((owner, local)) => (owner, event.with_local_target(Some(local))),
                // Targeted native events for removed objects are stale. They
                // must never be repurposed as root-component events.
                None => return false,
            },
            None => (ComponentId::ROOT, event),
        };

        let handled = self.update_component(owner, &event);

        // Child callbacks are transient runtime messages. They must be
        // delivered before the next declarative render so the render
        // observes the parent's updated state in the same event
        // transaction.
        let had_messages = !self.message_sink.borrow().is_empty();
        if had_messages {
            self.drain_messages();
        }

        if handled || had_messages {
            let _ = self.render_pass(false);
        }

        handled || had_messages
    }

    /// Delivers every completed background-task result to its owning
    /// component, re-rendering if any changed state. Returns whether
    /// anything changed.
    pub fn pump_tasks(&mut self) -> bool {
        self.local.run_until_stalled();
        // Messages a local task sent through a callback.
        let had_messages = !self.message_sink.borrow().is_empty();
        if had_messages {
            self.drain_messages();
        }
        let completed = self.scheduler.drain();
        if completed.is_empty() {
            // Local work (a data layer's fetch, say) may have updated a
            // store without delivering a message to anyone.
            if self.stores_changed() || had_messages {
                let before = self.generation;
                let _ = self.render_pass(false);
                return self.generation != before;
            }
            return false;
        }
        let mut changed = false;
        for completed_task in completed {
            let CompletedTask { target, message, .. } = completed_task;
            let Some(mut entry) = self.components.remove(&target) else {
                continue;
            };
            let delivered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let delivered = entry.component.message_any(message);
                if delivered {
                    entry.component.updated();
                }
                delivered
            }));
            let delivered = match delivered {
                Ok(delivered) => delivered,
                Err(payload) => {
                    self.components.insert(target, entry);
                    self.fail(target, payload);
                    changed = true;
                    continue;
                }
            };
            if delivered {
                self.dirty.entry(target).or_insert(RenderCause::Message);
                changed = true;
            } else {
                // See `drain_messages` below for why this is not a normal,
                // ignorable outcome.
                debug_assert!(
                    false,
                    "task result for {target:?} did not match its own component's Message type; \
                     this indicates a framework bug in component identity/keying, not a \
                     legitimate runtime condition"
                );
                eprintln!(
                    "rustnative-core: dropped a task result for {target:?} because it did not \
                     match the target component's Message type (framework bug, not application \
                     code)"
                );
            }
            self.components.insert(target, entry);
        }
        if changed || self.stores_changed() || had_messages {
            let _ = self.render_pass(false);
            changed = true;
        }
        changed
    }

    /// Drains any window-open/close requests queued through
    /// `ComponentContext::windows()` during the most recent dispatch, task
    /// pump, or render. Called by `Application`, which owns the window
    /// registry these requests act on.
    pub(crate) fn take_window_commands(&mut self) -> Vec<WindowCommand> {
        self.window_commands.borrow_mut().drain(..).collect()
    }

    /// Drains the pointer-capture and drag-feedback requests components
    /// queued through [`crate::InputRequests`] since the last call, in the
    /// order they were made. A platform backend calls this after every
    /// dispatch.
    pub fn take_input_requests(&mut self) -> Vec<InputRequest> {
        self.input_requests.borrow_mut().drain(..).collect()
    }

    pub(crate) fn input_requests(&self) -> &Rc<RefCell<VecDeque<InputRequest>>> {
        &self.input_requests
    }

    /// Drains the animation requests components queued since the last
    /// call, in order. A platform backend calls this after every dispatch.
    pub fn take_animation_requests(&mut self) -> Vec<AnimationRequest> {
        self.animation_requests.borrow_mut().drain(..).collect()
    }

    pub(crate) fn animation_requests(&self) -> &Rc<RefCell<VecDeque<AnimationRequest>>> {
        &self.animation_requests
    }

    /// Whether the person has asked for reduced motion.
    #[must_use]
    pub fn motion_preference(&self) -> crate::animation::MotionPreference {
        self.motion.get()
    }

    /// Records the platform's reduced-motion setting.
    pub fn set_motion_preference(&self, motion: crate::animation::MotionPreference) {
        self.motion.set(motion);
    }

    pub(crate) fn message_sink(&self) -> &Rc<RefCell<VecDeque<QueuedMessage>>> {
        &self.message_sink
    }
    pub(crate) fn window_commands(&self) -> &Rc<RefCell<VecDeque<WindowCommand>>> {
        &self.window_commands
    }

    /// Returns the tree's scheduler.
    #[must_use]
    pub fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    /// Returns the tree's platform-independent service contracts.
    #[must_use]
    pub fn services(&self) -> &Services {
        &self.services
    }
    /// Replaces the theme and re-renders, so every node's style is resolved
    /// again against it. A backend then applies the resulting diff to the
    /// native objects it already has: a theme change is a re-resolution,
    /// never a rebuild (`PLAN.md` 2.14).
    ///
    /// # Errors
    ///
    /// The re-render's first [`RenderError`], as [`Self::render`] reports it.
    pub fn set_theme(&mut self, theme: Theme) -> Result<(), RenderError> {
        self.theme = theme;
        for id in self.components.keys().copied().collect::<Vec<_>>() {
            self.mark_dirty(id, RenderCause::Theme);
        }
        self.render_pass(false)
    }

    /// Returns the tree's active theme.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub(crate) fn register_effect(
        &mut self,
        component: ComponentId,
        key: String,
        dependencies: EffectDependencies,
        run: Box<dyn FnOnce(EffectContext) -> super::effects::EffectCleanup>,
    ) {
        let effects = self.pending_effects.entry(component).or_default();
        if effects.iter().any(|effect| effect.key == key) {
            // A component composition mistake, not a framework-internal
            // impossible state (see `RenderError`'s docs): surface it
            // through the render result instead of panicking. The
            // second/duplicate registration for this key is dropped; the
            // first one already declared for this render stands.
            self.pending_render_errors.push(RenderError::DuplicateEffectKey { component, key });
            return;
        }
        effects.push(DeclaredEffect { key, dependencies, run });
    }

    fn drain_messages(&mut self) {
        loop {
            let next = self.message_sink.borrow_mut().pop_front();
            let Some(QueuedMessage { target, message, priority }) = next else {
                break;
            };

            if priority == crate::scheduler::Priority::Deferrable {
                self.deferred_messages.push_back(QueuedMessage { target, message, priority });
                continue;
            }
            let Some(mut entry) = self.components.remove(&target) else {
                continue;
            };
            let delivered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let delivered = entry.component.message_any(message);
                if delivered {
                    entry.component.updated();
                }
                delivered
            }));
            let delivered = match delivered {
                Ok(delivered) => delivered,
                Err(payload) => {
                    self.components.insert(target, entry);
                    self.fail(target, payload);
                    continue;
                }
            };
            if delivered {
                self.dirty.entry(target).or_insert(RenderCause::Message);
            } else {
                // `message_any` returns `false` only when the boxed
                // `Message` failed to downcast to the target component's
                // own `C::Message` — every message queued through
                // `ComponentContext<C::Message>` is constructed with that
                // exact type, so this is only reachable if a component's
                // identity/keying is broken (e.g. two different component
                // types ended up sharing a `ComponentId`), not a normal
                // "message for someone else" situation. Silently dropping
                // it here is exactly the "my button's onClick just...
                // didn't arrive" trap the standards audit calls out (P1.4):
                // surface it loudly instead of swallowing it.
                debug_assert!(
                    false,
                    "message for {target:?} did not match its own component's Message type; \
                     this indicates a framework bug in component identity/keying, not a \
                     legitimate runtime condition"
                );
                eprintln!(
                    "rustnative-core: dropped a message for {target:?} because it did not match \
                     the target component's Message type (framework bug, not application code)"
                );
            }
            self.components.insert(target, entry);
        }
    }

    /// A persisted value for component `owner` under `key` (see
    /// [`crate::ComponentContext::persisted`]).
    pub(crate) fn persisted<T>(
        &self,
        owner: ComponentId,
        key: &str,
        default: T,
    ) -> crate::persistence::Persisted<T>
    where
        T: serde::Serialize + serde::de::DeserializeOwned + Clone,
    {
        let path = self.paths.get(&owner).map_or("", String::as_str);
        crate::persistence::Persisted::new(
            format!("{path}#{}", escape_key(key)),
            default,
            Rc::clone(&self.state),
        )
    }

    /// Writes this window's buffered persisted state to its store.
    ///
    /// # Errors
    ///
    /// The first store write that failed; every other buffered write was
    /// still attempted, and the failed ones stay buffered for the next
    /// flush.
    pub fn flush_state(&self) -> Result<(), crate::services::ServiceError> {
        self.state.borrow_mut().flush()
    }

    /// Whether this window has persisted-state writes not yet flushed.
    #[must_use]
    pub fn has_unsaved_state(&self) -> bool {
        self.state.borrow().is_dirty()
    }

    /// Composes/reuses a keyed child component and returns its rendered
    /// view. `key` is consumed as an owned `String` (rather than `&str`)
    /// because it is stored as-is in `pending_children`'s key set below on
    /// every call path (create *and* reuse) — accepting a borrow would just
    /// move the same allocation to a `.to_owned()` at the call site with no
    /// net reduction, since `ComponentContext::child_with_props` already
    /// produces an owned `String` via `key.into()` before this is reached.
    #[allow(clippy::needless_pass_by_value)]
    #[allow(
        clippy::expect_used,
        reason = "an invariant this runtime itself just \n    /// established, not a condition an application can trigger — see \n    /// `crate::component::RenderError` for the line this crate draws between the two"
    )]
    pub(crate) fn render_child_with_props<C, F>(
        &mut self,
        parent: ComponentId,
        key: String,
        props: C::Props,
        generation: u64,
        constructor: F,
    ) -> Node
    where
        C: Component,
        F: FnOnce(C::Props) -> C,
    {
        let existing_id =
            self.pending_children.get(&parent).and_then(|children| children.get(&key).copied());

        let needs_create = match existing_id {
            Some(existing_id) => self
                .components
                .get(&existing_id)
                .is_none_or(|entry| entry.component_type != std::any::TypeId::of::<C>()),
            None => true,
        };

        let id = existing_id.unwrap_or_else(|| self.allocate_component_id());

        if needs_create {
            if let Some(existing_id) = existing_id {
                self.remove_component(existing_id);
            }

            let path = format!(
                "{}/{}",
                self.paths.get(&parent).map_or("", String::as_str),
                escape_key(&key)
            );
            self.paths.insert(id, path);
            self.parents.insert(id, parent);
            let mut component = constructor(props);
            component.mounted();
            self.components.insert(
                id,
                ComponentEntry {
                    component: Box::new(component),
                    component_type: std::any::TypeId::of::<C>(),
                    children: HashMap::new(),
                    view: None,
                    node_ids: HashMap::new(),
                    used_generation: generation,
                    task_scope: TaskScope::new(self.scheduler.clone(), Rc::clone(&self.local), id),
                    effects: HashMap::new(),
                },
            );
            self.pending_children.entry(parent).or_default().insert(key.clone(), id);
        } else {
            let changed = self
                .components
                .get(&id)
                .is_some_and(|entry| !entry.component.props_equal(&props as &dyn Any));
            // A props type unequal to its own clone can never be skipped
            // (`C04-1`): report it rather than silently re-render it.
            if changed && cfg!(debug_assertions) && props != props.clone() {
                self.unskippable.insert(std::any::type_name::<C>());
            }

            if changed {
                let mut entry = self
                    .components
                    .remove(&id)
                    .expect("managed child must exist when updating its props");
                let applied = entry.component.set_props_any(Box::new(props));
                if applied {
                    entry.component.props_changed();
                }
                self.components.insert(id, entry);
                self.mark_dirty(id, RenderCause::Props);
            }

            self.pending_children.entry(parent).or_default().insert(key.clone(), id);
        }

        self.render_or_reuse(id, generation)
    }

    #[allow(
        clippy::expect_used,
        reason = "an invariant this runtime itself just established, not a condition an application can \
    /// trigger — see `crate::component::RenderError` for the line this crate draws between \
    /// the two"
    )]
    fn render_component(&mut self, id: ComponentId, generation: u64) -> Node {
        let mut entry =
            self.components.remove(&id).expect("component tree entry must exist while rendering");

        entry.used_generation = generation;
        let task_scope = entry.task_scope.clone();
        self.pending_children.insert(id, entry.children.clone());
        self.pending_effects.insert(id, Vec::new());
        self.env_seen.remove(&id);
        self.preferences_seen.remove(&id);
        self.commands.clear_owner(id);
        self.size_reads.remove(&id);
        self.selections.remove(&id);
        if let Some(previous) = self.provided.remove(&id) {
            self.provided_previous.insert(id, previous);
        }
        // Messages become text against this component's locale; showing one
        // is reading the locale (`crate::i18n`).
        let locale = self.env_at(Some(id), &crate::environment::keys::LOCALE);
        crate::i18n::enter(locale, self.services.catalogues().cloned());
        let mut node = entry.component.render(self, id, generation, task_scope);
        if crate::i18n::leave() {
            let _ = self.read_env(id, &crate::environment::keys::LOCALE);
        }
        self.provided_previous.remove(&id);
        let child_node_ids = self
            .pending_children
            .get(&id)
            .into_iter()
            .flat_map(|children| children.values())
            .filter_map(|child| self.components.get(child))
            .filter_map(|child| child.view.as_ref())
            .flat_map(|view| {
                let mut ids = Vec::new();
                view.visit(&mut |node, _, _| ids.push(node.id()));
                ids
            })
            .collect::<HashSet<_>>();
        let mut node_ids = HashMap::new();
        scope_component_node_ids(
            id,
            &mut node,
            &child_node_ids,
            &mut node_ids,
            &mut self.pending_render_errors,
        );
        entry.view = Some(node.clone());
        entry.node_ids = node_ids;
        entry.children = self.pending_children.remove(&id).unwrap_or_default();
        self.components.insert(id, entry);

        self.prune_unused_children(id, generation);
        node
    }

    fn commit_effects(&mut self) {
        // Only components that rendered declared effects this pass; a
        // skipped component keeps the effects it already has.
        let mut components = self.pending_effects.keys().copied().collect::<Vec<_>>();
        components.sort();
        for id in components {
            self.commit_component_effects(id);
        }
    }

    fn commit_component_effects(&mut self, id: ComponentId) {
        let declarations = self.pending_effects.remove(&id).unwrap_or_default();
        let Some(mut entry) = self.components.remove(&id) else {
            return;
        };
        let mut declared = declarations
            .into_iter()
            .map(|effect| (effect.key.clone(), effect))
            .collect::<HashMap<_, _>>();

        let obsolete = entry
            .effects
            .keys()
            .filter(|key| !declared.contains_key(*key))
            .cloned()
            .collect::<Vec<_>>();
        for key in obsolete {
            if let Some(mut effect) = entry.effects.remove(&key) {
                Self::dispose_effect(&mut effect);
            }
        }

        for (key, declaration) in declared.drain() {
            let should_restart = entry
                .effects
                .get(&key)
                .is_none_or(|current| !current.dependencies.equals(&declaration.dependencies));
            if !should_restart {
                continue;
            }
            if let Some(mut previous) = entry.effects.remove(&key) {
                Self::dispose_effect(&mut previous);
            }
            let scope = TaskScope::with_suspension(
                self.scheduler.clone(),
                Rc::clone(&self.local),
                id,
                Arc::clone(entry.task_scope.suspension()),
            );
            let cleanup = (declaration.run)(EffectContext { task_scope: scope.clone() });
            entry.effects.insert(
                key,
                EffectEntry {
                    dependencies: declaration.dependencies,
                    task_scope: scope,
                    cleanup: Some(cleanup),
                },
            );
        }
        self.components.insert(id, entry);
    }

    fn dispose_effect(effect: &mut EffectEntry) {
        if let Some(cleanup) = effect.cleanup.take() {
            cleanup();
        }
        effect.task_scope.cancel_all();
    }

    fn dispose_effects(entry: &mut ComponentEntry) {
        for effect in entry.effects.values_mut() {
            Self::dispose_effect(effect);
        }
        entry.effects.clear();
    }

    fn rebuild_node_owners(&mut self) {
        self.node_owners.clear();
        self.local_node_owners.clear();
        for (component, entry) in &self.components {
            for (global, local) in &entry.node_ids {
                let previous = self.node_owners.insert(*global, (*component, *local));
                // Not a `RenderError`: given the collision-free identity
                // model in `crate::identity` and the duplicate-key check
                // already performed per-component in
                // `scope_component_node_ids`, two different components can
                // structurally never produce the same `global` id here.
                // Reaching this would be a framework bug, not a
                // user-triggerable composition mistake — see
                // `RenderError`'s docs for that distinction.
                debug_assert!(
                    previous.is_none(),
                    "duplicate node identity after component composition: {global:?}"
                );
                self.local_node_owners.entry(*local).or_default().push((*component, *local));
            }
        }
    }

    fn prune_unused_children(&mut self, parent: ComponentId, generation: u64) {
        let stale = self
            .components
            .get(&parent)
            .map(|entry| {
                entry
                    .children
                    .values()
                    .copied()
                    .filter(|id| {
                        self.components
                            .get(id)
                            .is_some_and(|child| child.used_generation != generation)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if stale.is_empty() {
            return;
        }

        if let Some(entry) = self.components.get_mut(&parent) {
            entry.children.retain(|_, id| !stale.contains(id));
        }

        for id in stale {
            self.remove_component(id);
        }
    }

    fn remove_component(&mut self, id: ComponentId) {
        if id == ComponentId::ROOT {
            return;
        }

        let Some(mut entry) = self.components.remove(&id) else {
            return;
        };
        self.paths.remove(&id);
        self.forget(id);

        // Structured ownership ends with the component lifetime: its tasks
        // are cancelled here, and its animations are queued for the backend
        // to stop for the same reason — neither may outlive its owner.
        entry.task_scope.cancel_all();
        self.animation_requests.borrow_mut().push_back(AnimationRequest::CancelOwner(id));
        Self::dispose_effects(&mut entry);

        let children = entry.children.values().copied().collect::<Vec<_>>();
        for child in children {
            self.remove_component(child);
        }

        entry.component.unmounted();
    }

    /// Drops the invalidation and environment bookkeeping of a removed
    /// component.
    fn forget(&mut self, id: ComponentId) {
        self.parents.remove(&id);
        self.dirty.remove(&id);
        self.provided.remove(&id);
        self.provided_previous.remove(&id);
        self.env_seen.remove(&id);
        self.published.remove(&id);
        self.preferences_seen.remove(&id);
        self.commands.clear_owner(id);
        self.size_reads.remove(&id);
        self.selections.remove(&id);
        self.scoped.remove(&id);
        self.boundaries.remove(&id);
        self.hidden.remove(&id);
        self.deferreds.retain(|(owner, _), _| *owner != id);
    }

    fn update_component(&mut self, id: ComponentId, event: &Event) -> bool {
        let Some(mut entry) = self.components.remove(&id) else {
            return false;
        };

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            entry.component.update(event.clone());
            entry.component.updated();
        }));
        self.components.insert(id, entry);
        self.mark_dirty(id, RenderCause::Event);
        if let Err(payload) = outcome {
            self.fail(id, payload);
        }
        true
    }

    fn allocate_component_id(&mut self) -> ComponentId {
        ComponentId::next(&mut self.next_component_id)
    }
}

impl Drop for ComponentTree {
    fn drop(&mut self) {
        if self.components.contains_key(&ComponentId::ROOT) {
            self.remove_component_children(ComponentId::ROOT);
            if let Some(mut root) = self.components.remove(&ComponentId::ROOT) {
                root.task_scope.cancel_all();
                Self::dispose_effects(&mut root);
                root.component.unmounted();
            }
        }
    }
}

impl ComponentTree {
    fn remove_component_children(&mut self, parent: ComponentId) {
        let children = self
            .components
            .get(&parent)
            .map(|entry| entry.children.values().copied().collect::<Vec<_>>())
            .unwrap_or_default();

        for child in children {
            self.remove_component_children(child);
            self.paths.remove(&child);
            if let Some(mut entry) = self.components.remove(&child) {
                entry.task_scope.cancel_all();
                Self::dispose_effects(&mut entry);
                entry.component.unmounted();
            }
        }
    }
}

/// Rewrites every node an authored view directly contributed (i.e., every
/// node id *not* already present in `child_node_ids`, meaning it came from a
/// nested managed child's already-scoped view — see
/// `crate::component::context::ComponentContext::child`) from its
/// declarative local key into the framework-wide identity a platform
/// backend and `crate::reconcile` actually use, by combining it with
/// `owner` (see `crate::identity::NodeId::scoped`).
///
/// Detects — without panicking — the one composition mistake this step can
/// discover: the same component authoring the same local key more than once
/// in a single render. That is reported as
/// [`RenderError::DuplicateNodeKey`] rather than the `debug_assert!` this
/// function used before this rewrite (standards audit P1.10); the second
/// occurrence's scoped id is still assigned, so the tree remains
/// structurally well-formed even though the application has a bug to fix.
fn scope_component_node_ids(
    owner: ComponentId,
    node: &mut Node,
    child_node_ids: &HashSet<NodeId>,
    node_ids: &mut HashMap<NodeId, NodeId>,
    errors: &mut Vec<RenderError>,
) {
    let local = node.id();
    // A node another component already scoped — a child's view, or one
    // handed to this component in its props (a card's body holding the
    // parent's badges) — keeps its identity: scoping it again would keep
    // only its local key and make two such nodes collide.
    if child_node_ids.contains(&local) || local.owner().is_some() {
        return;
    }

    // Local keys are exposed back to `Component::update`, while the
    // platform sees an opaque component-scoped identity. This keeps
    // reusable components composable: two children can each have a
    // `"submit"` node — see `crate::identity` for the full identity model.
    let scope =
        |id: NodeId| if owner == ComponentId::ROOT { id } else { NodeId::scoped(owner, id) };
    let global = scope(local);
    node.set_id(global);
    // Relationship targets are local keys of the same component, so they
    // take the same scoping as the node's own key (see
    // `crate::accessibility`'s "Relationship keys").
    node.accessibility_mut().scope_relationships(&scope);
    if node_ids.insert(global, local).is_some() {
        errors.push(RenderError::DuplicateNodeKey { component: owner });
    }

    match node {
        Node::Column(column) => {
            for child in column.children_mut() {
                scope_component_node_ids(owner, child, child_node_ids, node_ids, errors);
            }
        }
        Node::Row(row) => {
            for child in row.children_mut() {
                scope_component_node_ids(owner, child, child_node_ids, node_ids, errors);
            }
        }
        Node::Label(_)
        | Node::Button(_)
        | Node::TextInput(_)
        | Node::Control(_)
        | Node::Canvas(_)
        | Node::Surface(_)
        | Node::TabBar(_) => {}
    }
}

mod inspection;
mod resilience;
mod responsiveness;

pub use responsiveness::Deferred;

#[cfg(test)]
mod tests;

/// Escapes a component key for use as one segment of a key path, so a key
/// containing `/` or `#` cannot impersonate a deeper path or a value key.
fn escape_key(key: &str) -> String {
    let mut escaped = String::with_capacity(key.len());
    for character in key.chars() {
        match character {
            '%' => escaped.push_str("%25"),
            '/' => escaped.push_str("%2F"),
            '#' => escaped.push_str("%23"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// A child's fresh output, keeping what its parent set on the node it
/// composed — whether it is hidden (a screen under another on a navigation
/// stack) and its place in a virtual list — but not what the child itself
/// had set, which its fresh output decides again. `own` is what the child's
/// previous output set itself. Without this, a child that re-renders alone
/// would reappear, or lose its index, in a parent that was not rendered
/// again.
fn keep_placement(previous: &Node, fresh: Node, own: (bool, Option<usize>)) -> Node {
    let (own_hidden, own_index) = own;
    let fresh = if previous.is_hidden() && !own_hidden { fresh.hidden(true) } else { fresh };
    match (previous.item_index(), own_index) {
        (Some(index), None) => fresh.with_item_index(index),
        _ => fresh,
    }
}

/// Replaces the subtree rooted at `target` inside `root` with `fresh`,
/// returning whether it was found. How a reused component's output takes a
/// re-rendered child's new output without the parent rendering again.
fn replace_subtree(
    root: &mut Node,
    target: NodeId,
    fresh: Node,
    own: (bool, Option<usize>),
) -> bool {
    if root.id() == target {
        *root = keep_placement(root, fresh, own);
        return true;
    }
    let children = match root {
        Node::Column(column) => column.children_mut(),
        Node::Row(row) => row.children_mut(),
        _ => return false,
    };
    let mut fresh = Some(fresh);
    for child in children.iter_mut() {
        if child.id() == target {
            if let Some(fresh) = fresh.take() {
                *child = keep_placement(child, fresh, own);
            }
            return true;
        }
    }
    for child in children.iter_mut() {
        if let Some(value) = fresh.take() {
            if replace_subtree(child, target, value.clone(), own) {
                return true;
            }
            fresh = Some(value);
        }
    }
    false
}
