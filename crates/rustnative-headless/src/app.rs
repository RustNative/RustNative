//! [`HeadlessApp`]: drive an application with synthetic input.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use rustnative_core::{
    AnimationRequest, Application, Component, Event, Executor, InputRequest, KeyCode, KeyModifiers,
    Lifecycle, ManualExecutor, MemoryStateStore, NodeId, NodeKind, Point, Services, Size, Theme,
    VirtualRange, Window, WindowId,
};

use crate::measure::HeadlessMeasurer;
use crate::query::{Query, QueryError, describe};
use crate::services::MockHttp;
use crate::tree::HeadlessTree;

/// How long a quiet application waits before flushing persisted state —
/// the same debounce the Windows backend uses (`native::lifecycle`).
pub const STATE_FLUSH_DELAY: Duration = Duration::from_millis(1_500);

type Factory = Box<dyn Fn(Services, Theme, Arc<dyn Executor>) -> Application>;

/// A running application on the headless backend, driven the way a person
/// drives it.
///
/// Every interaction goes through the *real* input path: a click is
/// resolved to a node by hit-testing the realized layout, focus moves as a
/// host moves it, a keystroke in a text field becomes the same
/// `TextChanged` a native edit control reports — and only then does the
/// resulting [`Event`] reach [`Application::dispatch_to_window`]. Nothing
/// calls a component directly, so what a test exercises is what a person
/// exercises.
///
/// Time is virtual: tasks and delays run on a [`ManualExecutor`], and
/// nothing happens between interactions unless [`Self::advance`] moves the
/// clock.
///
/// ```
/// use rustnative_core::{AccessibilityRole, Component, Event, Node, NodeId, Size, Window};
/// use rustnative_headless::{HeadlessApp, Query};
///
/// struct Counter { count: u32 }
///
/// impl Component for Counter {
///     type Props = ();
///     type Message = ();
///     fn new((): ()) -> Self { Self { count: 0 } }
///     fn props(&self) -> &() { &() }
///     fn set_props(&mut self, (): ()) {}
///     fn view(&self) -> Node {
///         Node::column("root", [
///             Node::label("count", format!("Count: {}", self.count)),
///             Node::button("increment", "Increment"),
///         ])
///     }
///     fn update(&mut self, event: Event) {
///         if matches!(event, Event::Click { target } if target == NodeId::from_key("increment")) {
///             self.count += 1;
///         }
///     }
/// }
///
/// let mut app = HeadlessApp::launch(Window::new("Counter", Size::new(320, 200)), || Counter::new(()));
/// app.click(&Query::role(AccessibilityRole::Button).name("Increment"))?;
/// assert!(app.find(&Query::text("Count: 1")).is_ok());
///
/// // A fresh counter's view, in markup:
/// let markup = rustnative_core::rsx! {
///     <Column key="root">
///         <Label key="count" text="Count: 0" />
///         <Button key="increment" text="Increment" />
///     </Column>
/// };
/// assert_eq!(markup, Counter::new(()).view());
/// # Ok::<(), rustnative_headless::QueryError>(())
/// ```
pub struct HeadlessApp {
    factory: Factory,
    app: Application,
    executor: ManualExecutor,
    services: Services,
    theme: Theme,
    state: Arc<MemoryStateStore>,
    http: Option<MockHttp>,
    measurer: HeadlessMeasurer,
    trees: HashMap<WindowId, HeadlessTree>,
    input_requests: Vec<(WindowId, InputRequest)>,
    animation_requests: Vec<(WindowId, AnimationRequest)>,
    flush_due: Option<Duration>,
    reported_ranges: HashMap<(WindowId, NodeId), VirtualRange>,
    exhaustive: bool,
    window: WindowId,
}

impl std::fmt::Debug for HeadlessApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessApp")
            .field("windows", &self.app.window_ids())
            .field("now", &self.executor.now())
            .finish_non_exhaustive()
    }
}

impl HeadlessApp {
    /// Launches `root()` in a window described by `window`, with an
    /// in-memory state store and otherwise default services.
    pub fn launch<C, F>(window: Window, root: F) -> Self
    where
        C: Component,
        F: Fn() -> C + 'static,
    {
        Self::launch_with(window, Services::default(), Theme::default(), root)
    }

    /// Launches with explicit `services` and `theme`. A state store is added
    /// to `services` if it has none, so restoration can be tested.
    pub fn launch_with<C, F>(window: Window, services: Services, theme: Theme, root: F) -> Self
    where
        C: Component,
        F: Fn() -> C + 'static,
    {
        let factory: Factory = Box::new(move |services, theme, executor| {
            Application::with_executor(root(), window.clone(), services, theme, executor)
        });
        let state = Arc::new(MemoryStateStore::new());
        let services = if services.state_store().is_some() {
            services
        } else {
            services.with_state_store(Arc::clone(&state) as Arc<dyn rustnative_core::StateStore>)
        };
        Self::start(factory, services, theme, state)
    }

    fn start(
        factory: Factory,
        services: Services,
        theme: Theme,
        state: Arc<MemoryStateStore>,
    ) -> Self {
        use rustnative_core::perf::{self, StartupPhase};
        perf::mark(StartupPhase::RuntimeReady);
        let executor = ManualExecutor::new();
        let services = services.with_clock(Arc::new(executor.clone()));
        let mut app = factory(services.clone(), theme.clone(), Arc::new(executor.clone()));
        // Host-following tokens take the fixed reference palette, the same on
        // every machine (`docs/tokens.md`).
        app.set_host_palette(rustnative_core::HostPalette::default());
        let mut headless = Self {
            factory,
            app,
            executor,
            services,
            theme,
            state,
            http: None,
            measurer: HeadlessMeasurer::new(),
            trees: HashMap::new(),
            input_requests: Vec::new(),
            animation_requests: Vec::new(),
            flush_due: None,
            reported_ranges: HashMap::new(),
            exhaustive: false,
            window: WindowId::PRIMARY,
        };
        headless.settle();
        // The model is realized and settled: there is no paint, so the
        // first frame, first content, and interactive coincide here
        // (`rustnative_core::perf`).
        perf::mark(StartupPhase::FirstFrame);
        perf::mark(StartupPhase::FirstContent);
        perf::mark(StartupPhase::Interactive);
        headless
    }

    /// Replaces the HTTP service with `http` and relaunches, so every
    /// request the application makes is checked against the mock's script.
    #[must_use]
    pub fn with_http(mut self, http: MockHttp) -> Self {
        self.services = self.services.clone().with_http(Arc::new(http.clone()));
        self.http = Some(http);
        self.relaunch_fresh();
        self
    }

    /// Uses `measurer` (for example a larger text scale) and re-realizes.
    #[must_use]
    pub fn with_measurer(mut self, measurer: HeadlessMeasurer) -> Self {
        self.measurer = measurer;
        self.realize_all();
        self
    }

    /// Turns on exhaustive mode (`C11`): when this value is dropped, any
    /// task still pending, any HTTP expectation unmet or request
    /// unexpected, and any input or animation request not consumed through
    /// [`Self::take_animation_requests`]/[`Self::take_input_requests`]
    /// fails the test.
    #[must_use]
    pub const fn exhaustive(mut self) -> Self {
        self.exhaustive = true;
        self
    }

    fn relaunch_fresh(&mut self) {
        self.app = (self.factory)(
            self.services.clone(),
            self.theme.clone(),
            Arc::new(self.executor.clone()),
        );
        self.trees.clear();
        self.reported_ranges.clear();
        self.settle();
    }

    // ------------------------------------------------------------------
    // Observation
    // ------------------------------------------------------------------

    /// The application being driven.
    #[must_use]
    pub const fn application(&self) -> &Application {
        &self.app
    }

    /// Mutable access, for what a host would do outside input (open a
    /// window programmatically, change the motion preference).
    pub const fn application_mut(&mut self) -> &mut Application {
        &mut self.app
    }

    /// The executor driving every task and delay.
    #[must_use]
    pub const fn executor(&self) -> &ManualExecutor {
        &self.executor
    }

    /// The durable state store: what survives [`Self::kill_and_restore`].
    #[must_use]
    pub fn state_store(&self) -> &MemoryStateStore {
        &self.state
    }

    /// Directs subsequent interactions and queries at window `id`.
    pub fn focus_window(&mut self, id: WindowId) {
        self.window = id;
    }

    /// The realized model of the current window.
    ///
    /// # Panics
    ///
    /// If the current window has been closed.
    #[must_use]
    #[allow(clippy::expect_used, reason = "documented: a closed window has no realized tree")]
    pub fn realized(&self) -> &HeadlessTree {
        self.trees.get(&self.window).expect("the current headless window is open")
    }

    /// The realized model of window `id`, if open.
    #[must_use]
    pub fn realized_window(&self, id: WindowId) -> Option<&HeadlessTree> {
        self.trees.get(&id)
    }

    /// Finds the one node matching `query` in the current window.
    ///
    /// # Errors
    ///
    /// See [`Query::one`].
    pub fn find(&self, query: &Query) -> Result<&crate::tree::RealizedNode, QueryError> {
        query.one(self.realized())
    }

    /// Every node matching `query` in the current window.
    #[must_use]
    pub fn find_all(&self, query: &Query) -> Vec<&crate::tree::RealizedNode> {
        query.all(self.realized())
    }

    /// A stable description of every open window's realized tree, for a
    /// golden file.
    #[must_use]
    pub fn golden(&self) -> String {
        let mut out = String::new();
        let mut ids = self.app.window_ids();
        ids.sort_by_key(|id| id.get());
        for id in ids {
            if let (Some(window), Some(tree)) = (self.app.window_for(id), self.trees.get(&id)) {
                let size = tree.size();
                let _ = writeln!(out, "window {:?} {}x{}", window.title(), size.width, size.height);
                out.push_str(&tree.describe());
            }
        }
        out
    }

    /// Input requests (pointer capture, drop feedback) components made,
    /// consumed so exhaustive mode does not report them.
    pub fn take_input_requests(&mut self) -> Vec<(WindowId, InputRequest)> {
        std::mem::take(&mut self.input_requests)
    }

    /// Animation requests components made, consumed so exhaustive mode does
    /// not report them.
    pub fn take_animation_requests(&mut self) -> Vec<(WindowId, AnimationRequest)> {
        std::mem::take(&mut self.animation_requests)
    }

    // ------------------------------------------------------------------
    // Input — through hit testing and host focus rules
    // ------------------------------------------------------------------

    /// Clicks the node matching `query`, as a pointer would: the click lands
    /// at the centre of the node's visible area, is hit-tested, moves focus
    /// if the control takes it, and activates it.
    ///
    /// # Errors
    ///
    /// The query failed, or the node could not be clicked (disabled,
    /// hidden, scrolled out of view, covered, or not a clickable control).
    pub fn click(&mut self, query: &Query) -> Result<(), QueryError> {
        let (id, center) = {
            let node = self.find(query)?;
            if !node.is_interactable() {
                return Err(not_interactable(
                    node,
                    "it is disabled, hidden, or has no visible area",
                ));
            }
            (node.id, node.center())
        };
        self.click_at(center, Some(id))
    }

    /// Clicks at `point` in window coordinates.
    ///
    /// # Errors
    ///
    /// Nothing clickable is at `point`.
    pub fn click_point(&mut self, point: Point) -> Result<(), QueryError> {
        self.click_at(point, None)
    }

    fn click_at(&mut self, point: Point, expected: Option<NodeId>) -> Result<(), QueryError> {
        let hit = self.realized().hit_test(point).ok_or_else(|| QueryError::NotInteractable {
            node: format!("point {},{}", point.x, point.y),
            reason: "nothing is there".to_owned(),
        })?;
        if let Some(expected) = expected {
            if hit != expected && !self.is_ancestor(expected, hit) {
                let covering = self.realized().get(hit).map_or_else(String::new, describe);
                let target = self.realized().get(expected).map_or_else(String::new, describe);
                return Err(QueryError::NotInteractable {
                    node: target,
                    reason: format!("it is covered by {covering}"),
                });
            }
        }
        let target = expected.unwrap_or(hit);
        let node =
            self.realized().get(target).cloned().ok_or_else(|| QueryError::NotInteractable {
                node: format!("{target:?}"),
                reason: "it vanished".to_owned(),
            })?;
        if !node.is_interactable() {
            return Err(not_interactable(&node, "it is disabled or hidden"));
        }
        if node.accessibility.is_focusable() {
            self.move_focus(Some(target));
        }
        match node.kind {
            NodeKind::Button => self.dispatch(Event::Click { target }),
            NodeKind::TextInput => {}
            _ if node.accessibility.is_focusable() => {}
            _ => {
                return Err(not_interactable(&node, "it is not a clickable control"));
            }
        }
        Ok(())
    }

    fn is_ancestor(&self, ancestor: NodeId, mut node: NodeId) -> bool {
        while let Some(parent) = self.realized().get(node).and_then(|node| node.parent) {
            if parent == ancestor {
                return true;
            }
            node = parent;
        }
        false
    }

    /// Chooses tab `index` of the tab bar matching `query`.
    ///
    /// # Errors
    ///
    /// The query failed, the node is not a tab bar, or `index` is past its
    /// last tab.
    pub fn select_tab(&mut self, query: &Query, index: usize) -> Result<(), QueryError> {
        let node = self.find(query)?.clone();
        let Some(tabs) = &node.tabs else {
            return Err(not_interactable(&node, "it is not a tab bar"));
        };
        if index >= tabs.labels().len() || !node.is_interactable() {
            return Err(not_interactable(&node, "no such tab, or the tab bar is disabled"));
        }
        self.move_focus(Some(node.id));
        self.dispatch(Event::TabSelected { target: node.id, index });
        Ok(())
    }

    fn control_node(
        &mut self,
        query: &Query,
    ) -> Result<(rustnative_core::Control, NodeId), QueryError> {
        let node = self.find(query)?.clone();
        let Some(control) = node.control.clone() else {
            return Err(not_interactable(&node, "it is not a native control"));
        };
        if !node.is_interactable() {
            return Err(not_interactable(&node, "it is disabled"));
        }
        self.move_focus(Some(node.id));
        Ok((control, node.id))
    }

    /// Clicks the check box, switch, or radio button matching `query`, as
    /// the person would (Milestone 48).
    ///
    /// # Errors
    ///
    /// The query failed, or the node is not an enabled toggle.
    pub fn toggle(&mut self, query: &Query) -> Result<(), QueryError> {
        use rustnative_core::Control;
        let (control, id) = self.control_node(query)?;
        let on = match control {
            Control::Checkbox { checked, .. } => !checked,
            Control::Toggle { on, .. } => !on,
            Control::Radio { .. } => true,
            _ => return Err(not_interactable(self.find(query)?, "it is not a toggle")),
        };
        self.dispatch(Event::Toggled { target: id, on });
        Ok(())
    }

    /// Moves the slider or spinner matching `query` to `value`, clamped to
    /// its range.
    ///
    /// # Errors
    ///
    /// The query failed, or the node is not an enabled slider or spinner.
    pub fn set_value(&mut self, query: &Query, value: i64) -> Result<(), QueryError> {
        use rustnative_core::Control;
        let (control, id) = self.control_node(query)?;
        let value = match control {
            Control::Slider { min, max, .. } | Control::Spinner { min, max, .. } => {
                value.clamp(min, max)
            }
            _ => return Err(not_interactable(self.find(query)?, "it has no range")),
        };
        self.dispatch(Event::ValueChanged { target: id, value });
        Ok(())
    }

    /// Chooses item `index` of the select or list box matching `query`.
    ///
    /// # Errors
    ///
    /// The query failed, the node is not an enabled select or list box, or
    /// it has no such item.
    pub fn choose(&mut self, query: &Query, index: usize) -> Result<(), QueryError> {
        use rustnative_core::Control;
        let (control, id) = self.control_node(query)?;
        let count = match &control {
            Control::Select { options, .. } => options.len(),
            Control::ListBox { items, .. } => items.len(),
            _ => 0,
        };
        if index >= count {
            return Err(not_interactable(self.find(query)?, "no such item"));
        }
        self.dispatch(Event::SelectionChanged { target: id, index: Some(index) });
        Ok(())
    }

    /// Picks `date` in the date picker matching `query`.
    ///
    /// # Errors
    ///
    /// The query failed, or the node is not an enabled date picker.
    pub fn pick_date(
        &mut self,
        query: &Query,
        date: rustnative_core::CalendarDate,
    ) -> Result<(), QueryError> {
        let (control, id) = self.control_node(query)?;
        if !matches!(control, rustnative_core::Control::DatePicker { .. }) {
            return Err(not_interactable(self.find(query)?, "it is not a date picker"));
        }
        self.dispatch(Event::DateChanged { target: id, date });
        Ok(())
    }

    /// Types `text` into the field matching `query`, one character at a
    /// time, as a keyboard would — focusing it first.
    ///
    /// # Errors
    ///
    /// The query failed, or the node is not an enabled text field.
    pub fn type_text(&mut self, query: &Query, text: &str) -> Result<(), QueryError> {
        let node = self.find(query)?.clone();
        if node.kind != NodeKind::TextInput || !node.is_interactable() {
            return Err(not_interactable(&node, "it is not an enabled text field"));
        }
        self.move_focus(Some(node.id));
        for character in text.chars() {
            let mut value =
                self.realized().get(node.id).and_then(|node| node.text.clone()).unwrap_or_default();
            value.push(character);
            self.set_native_value(node.id, value.clone());
            self.dispatch(Event::TextChanged { target: node.id, value });
        }
        Ok(())
    }

    /// Replaces the whole content of the field matching `query`, as
    /// select-all-and-type would.
    ///
    /// # Errors
    ///
    /// See [`Self::type_text`].
    pub fn set_text(&mut self, query: &Query, text: &str) -> Result<(), QueryError> {
        let node = self.find(query)?.clone();
        if node.kind != NodeKind::TextInput || !node.is_interactable() {
            return Err(not_interactable(&node, "it is not an enabled text field"));
        }
        self.move_focus(Some(node.id));
        self.set_native_value(node.id, text.to_owned());
        self.dispatch(Event::TextChanged { target: node.id, value: text.to_owned() });
        Ok(())
    }

    fn set_native_value(&mut self, id: NodeId, value: String) {
        if let Some(tree) = self.trees.get_mut(&self.window) {
            tree.set_native_value(id, value);
        }
    }

    /// Presses and releases `key` with `modifiers` on the focused node.
    ///
    /// A declared command whose shortcut matches takes the key first.
    /// Tab and Shift+Tab move focus through [`HeadlessTree::tab_order`];
    /// Enter and Space activate a focused button — the host behaviours a
    /// native control provides, and which a component therefore never
    /// implements itself.
    pub fn press(&mut self, key: KeyCode, modifiers: KeyModifiers) {
        let focused = self.realized().focused();
        if key == KeyCode::Tab && !modifiers.ctrl && !modifiers.alt {
            let order = self.realized().tab_order();
            if order.is_empty() {
                return;
            }
            let position =
                focused.and_then(|id| order.iter().position(|candidate| *candidate == id));
            let next = match (position, modifiers.shift) {
                (None, false) => order[0],
                (None, true) => order[order.len() - 1],
                (Some(index), false) => order[(index + 1) % order.len()],
                (Some(index), true) => order[(index + order.len() - 1) % order.len()],
            };
            self.move_focus(Some(next));
            return;
        }
        // A command's shortcut takes the key before the focused control
        // does, as it does on every desktop host.
        if self.app.handle_shortcut(self.window, key, modifiers, focused) {
            self.settle();
            return;
        }
        self.dispatch(Event::KeyDown { target: focused, key, modifiers });
        if matches!(key, KeyCode::Enter | KeyCode::Space) {
            if let Some(target) = focused {
                if self
                    .realized()
                    .get(target)
                    .is_some_and(|node| node.kind == NodeKind::Button && node.is_interactable())
                {
                    self.dispatch(Event::Click { target });
                }
            }
        }
        let focused = self.realized().focused();
        self.dispatch(Event::KeyUp { target: focused, key, modifiers });
    }

    /// Presses Tab.
    pub fn tab(&mut self) {
        self.press(KeyCode::Tab, KeyModifiers::default());
    }

    /// Scrolls the container matching `query` by `dy` logical pixels.
    ///
    /// Scrolling is a viewport change and causes no render (`PLAN.md`
    /// 2.10); a virtual list whose visible range moves raises
    /// [`Event::VisibleRangeChanged`] — only then.
    ///
    /// # Errors
    ///
    /// The query failed, or the node does not scroll.
    pub fn scroll(&mut self, query: &Query, dy: i32) -> Result<(), QueryError> {
        let node = self.find(query)?.clone();
        if node.scroll_range == Size::new(0, 0) && self.virtual_style(node.id).is_none() {
            return Err(not_interactable(&node, "it does not scroll"));
        }
        let offset = Point::new(node.scroll.x, node.scroll.y.saturating_add(dy));
        if let Some(tree) = self.trees.get_mut(&self.window) {
            tree.scroll_to(node.id, offset);
        }
        self.settle();
        Ok(())
    }

    fn virtual_style(&self, id: NodeId) -> Option<rustnative_core::VirtualListStyle> {
        self.realized().snapshot().get(id).and_then(|node| node.virtualization)
    }

    /// Recomputes every virtual list's visible range from where it is
    /// scrolled and how large it is, and returns the ones that differ from
    /// what was last reported — the rule the Windows backend follows
    /// (`rendering::virtual_list`): the first frame reports, scrolling
    /// within a range reports nothing, moving past it reports once.
    fn range_changes(&mut self) -> Vec<(WindowId, NodeId, VirtualRange)> {
        let mut changes = Vec::new();
        for (window, tree) in &self.trees {
            for node in tree.nodes() {
                let Some(style) =
                    tree.snapshot().get(node.id).and_then(|snapshot| snapshot.virtualization)
                else {
                    continue;
                };
                let extents = rustnative_core::ExtentCache::new(style.item_count, style.extent);
                let (offset, viewport) = match style.axis {
                    rustnative_core::Axis::Vertical => (node.scroll.y, node.rect.height),
                    rustnative_core::Axis::Horizontal => (node.scroll.x, node.rect.width),
                };
                let range = VirtualRange::compute(
                    u32::try_from(offset.max(0)).unwrap_or(0),
                    u32::try_from(viewport.max(0)).unwrap_or(0),
                    &extents,
                    style.overscan,
                );
                if self.reported_ranges.get(&(*window, node.id)) != Some(&range) {
                    changes.push((*window, node.id, range));
                }
            }
        }
        for (window, id, range) in &changes {
            self.reported_ranges.insert((*window, *id), *range);
        }
        changes
    }

    fn move_focus(&mut self, next: Option<NodeId>) {
        let previous = self.realized().focused();
        if previous == next {
            return;
        }
        if let Some(tree) = self.trees.get_mut(&self.window) {
            tree.set_focused(next);
        }
        if let Some(previous) = previous {
            self.dispatch(Event::FocusLost { target: previous });
        }
        if let Some(next) = next {
            self.dispatch(Event::FocusGained { target: next });
        }
    }

    /// Resizes the current window's client area to `size`.
    pub fn resize(&mut self, size: Size) {
        self.dispatch(Event::WindowResized { window: self.window, size });
    }

    /// Delivers `url` as a deep link.
    pub fn open_url(&mut self, url: &str) {
        self.app.open_url(url);
        self.settle();
    }

    /// Reports a lifecycle transition, as a host does.
    pub fn lifecycle(&mut self, lifecycle: Lifecycle) {
        let _ = self.app.lifecycle(lifecycle);
        self.settle();
    }

    /// Dispatches `event` to the current window and settles. Prefer the
    /// interaction methods above: this bypasses hit-testing and focus.
    pub fn dispatch(&mut self, event: Event) {
        self.app.dispatch_to_window(self.window, event);
        self.settle();
    }

    // ------------------------------------------------------------------
    // Inspection (`PLAN.md` Milestone 44)
    // ------------------------------------------------------------------

    /// Answers an inspection request with this backend's model, then
    /// settles (an edit renders).
    pub fn inspect(
        &mut self,
        request: &rustnative_core::inspect::Request,
    ) -> rustnative_core::inspect::Reply {
        let reply = self.app.inspect(request, &crate::HeadlessInspect::new(&self.trees));
        self.settle();
        reply
    }

    /// Each inspectable component's state in the current window, by key
    /// path relative to the root — what a recording's `final_state` is
    /// compared with.
    #[must_use]
    pub fn inspected_state(&self) -> std::collections::BTreeMap<String, serde_json::Value> {
        self.app
            .components_for(self.window)
            .map(rustnative_core::ComponentTree::relative_states)
            .unwrap_or_default()
    }

    /// The overlay over the current window, drawn over the realized
    /// model's window rectangles, or `None` when it is hidden.
    #[must_use]
    pub fn overlay(&self) -> Option<rustnative_core::DrawList> {
        let rects = crate::HeadlessInspect::new(&self.trees).window_rects(self.window);
        self.app.overlay_draw_list(self.window, &rects)
    }

    /// Replays `recording` deterministically: its HTTP responses become
    /// this app's (relaunching it with them, when there are any), and each
    /// input arrives after the virtual time it arrived after when recorded.
    /// Returns how many inputs were replayed.
    ///
    /// # Errors
    ///
    /// An input whose target is not in the tree when it is reached, or
    /// whose text was redacted.
    pub fn replay(
        &mut self,
        recording: &rustnative_core::inspect::Recording,
    ) -> Result<usize, rustnative_core::inspect::ReplayError> {
        if !recording.http.is_empty() {
            let http = MockHttp::new();
            for exchange in &recording.http {
                match exchange.response() {
                    Ok(response) => http.expect(exchange.method(), exchange.url.clone(), response),
                    Err(error) => {
                        http.expect_failure(exchange.method(), exchange.url.clone(), error);
                    }
                }
            }
            self.services = self.services.clone().with_http(Arc::new(http.clone()));
            self.http = Some(http);
            self.relaunch_fresh();
        }
        let mut replayed = 0;
        let mut previous = 0;
        for input in &recording.inputs {
            self.advance(Duration::from_millis(input.at_ms.saturating_sub(previous)));
            previous = input.at_ms;
            let Some(tree) = self.app.components_for(self.window) else {
                return Err(rustnative_core::inspect::ReplayError::MissingWindow(self.window));
            };
            if let Some(event) = recording.event(tree, self.window, &input.event)? {
                self.dispatch(event);
                replayed += 1;
            }
        }
        Ok(replayed)
    }

    // ------------------------------------------------------------------
    // Time and settling
    // ------------------------------------------------------------------

    /// Moves virtual time forward by `duration`, firing every delay that
    /// comes due (and the persistence flush, if its debounce elapses), then
    /// settles.
    pub fn advance(&mut self, duration: Duration) {
        self.executor.advance(duration);
        self.settle();
    }

    /// Runs every ready task and delivers every result until nothing more
    /// happens, then realizes the result — what a host's loop does between
    /// two inputs.
    pub fn settle(&mut self) {
        // Requests from an attached inspector are answered between inputs,
        // as a native host answers them when its loop is woken.
        self.app.poll_inspection(&crate::HeadlessInspect::new(&self.trees));
        for _ in 0..1_000 {
            self.executor.run_until_stalled();
            let changed = self.app.pump_tasks();
            self.executor.run_until_stalled();
            // Deferrable work only once nothing more urgent is left, as a
            // native host runs it when no input is waiting.
            if !changed && !self.app.pump_tasks() && !self.app.pump_deferred() {
                break;
            }
        }
        self.collect_requests();
        self.realize_all();
        // A range change renders new items, which can move the range again
        // (newly measured extents); two passes settle it in practice, and
        // the bound is a backstop against a list that never settles.
        for _ in 0..8 {
            let changes = self.range_changes();
            if changes.is_empty() {
                break;
            }
            for (window, target, range) in changes {
                self.app.dispatch_to_window(window, Event::VisibleRangeChanged { target, range });
            }
            self.executor.run_until_stalled();
            self.app.pump_tasks();
            self.collect_requests();
            self.realize_all();
        }
        // Container sizes a component decides by (`C22`): a class change
        // re-renders the reader, which can change layout again.
        for _ in 0..4 {
            let mut changed = false;
            for id in self.app.window_ids() {
                let Some(tree) = self.trees.get(&id) else { continue };
                let sizes: Vec<_> = self
                    .app
                    .watched_nodes(id)
                    .into_iter()
                    .filter_map(|node| {
                        let rect = tree.get(node)?.rect;
                        Some((
                            node,
                            Size::new(
                                u32::try_from(rect.width.max(0)).unwrap_or(0),
                                u32::try_from(rect.height.max(0)).unwrap_or(0),
                            ),
                        ))
                    })
                    .collect();
                changed |= self.app.report_sizes(id, sizes);
            }
            if !changed {
                break;
            }
            self.realize_all();
        }
        self.schedule_flush();
    }

    fn collect_requests(&mut self) {
        for id in self.app.window_ids() {
            for request in self.app.take_input_requests(id) {
                self.input_requests.push((id, request));
            }
            for request in self.app.take_animation_requests(id) {
                self.animation_requests.push((id, request));
            }
        }
    }

    fn schedule_flush(&mut self) {
        let now = self.executor.now();
        if let Some(due) = self.flush_due {
            if now >= due {
                let _ = self.app.flush_state();
                self.flush_due = None;
            }
        }
        if self.flush_due.is_none() && self.app.has_unsaved_state() {
            self.flush_due = Some(now + STATE_FLUSH_DELAY);
        }
    }

    fn realize_all(&mut self) {
        rustnative_core::perf::realized();
        let ids = self.app.window_ids();
        self.trees.retain(|id, _| ids.contains(id));
        for id in ids {
            let (Some(view), Some(state)) = (self.app.view_for(id), self.app.window_state(id))
            else {
                continue;
            };
            let size = state.size();
            let theme = self.app.theme().clone();
            let direction = self.app.layout_direction(id);
            let safe_area = self.app.environment_for(id, &rustnative_core::keys::SAFE_AREA);
            self.trees.entry(id).or_default().realize(
                &view,
                &theme,
                size,
                self.measurer,
                direction,
                safe_area,
            );
        }
        if !self.trees.contains_key(&self.window) {
            self.window = WindowId::PRIMARY;
        }
    }

    // ------------------------------------------------------------------
    // Process lifecycle
    // ------------------------------------------------------------------

    /// Kills the process without warning — no flush, no lifecycle event —
    /// and launches it again against the same durable state store. Only
    /// what was flushed before the kill survives, exactly as on a host that
    /// reclaims a background process.
    #[must_use]
    pub fn kill_and_restore(mut self) -> Self {
        self.exhaustive = false;
        let factory =
            std::mem::replace(&mut self.factory, Box::new(|_, _, _| unreachable_factory()));
        let services = self.services.clone();
        let theme = self.theme.clone();
        let state = Arc::clone(&self.state);
        let http = self.http.take();
        drop(self);
        let mut restored = Self::start(factory, services, theme, state);
        restored.http = http;
        restored
    }

    /// Terminates politely — `Lifecycle::Terminating`, which flushes — then
    /// launches again against the same state store.
    #[must_use]
    pub fn terminate_and_relaunch(mut self) -> Self {
        self.lifecycle(Lifecycle::Terminating);
        self.kill_and_restore()
    }

    /// Launches a new process against the same state store and delivers
    /// `url` before it has settled — a deep link arriving *during*
    /// restoration, the collision the lifecycle suite exists to test.
    #[must_use]
    pub fn relaunch_with_deep_link(mut self, url: &str) -> Self {
        self.exhaustive = false;
        let factory =
            std::mem::replace(&mut self.factory, Box::new(|_, _, _| unreachable_factory()));
        let services = self.services.clone();
        let theme = self.theme.clone();
        let state = Arc::clone(&self.state);
        drop(self);
        let executor = ManualExecutor::new();
        let services = services.with_clock(Arc::new(executor.clone()));
        let mut app = factory(services.clone(), theme.clone(), Arc::new(executor.clone()));
        app.open_url(url);
        let mut restored = Self {
            factory,
            app,
            executor,
            services,
            theme,
            state,
            http: None,
            measurer: HeadlessMeasurer::new(),
            trees: HashMap::new(),
            input_requests: Vec::new(),
            animation_requests: Vec::new(),
            flush_due: None,
            reported_ranges: HashMap::new(),
            exhaustive: false,
            window: WindowId::PRIMARY,
        };
        restored.settle();
        restored
    }

    /// Changes the theme — a configuration change — re-rendering and
    /// re-realizing without restarting.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme.clone();
        self.app.set_theme(theme);
        self.settle();
    }

    fn violations(&self) -> Vec<String> {
        let mut violations = Vec::new();
        let pending = self.executor.pending_task_count();
        if pending > 0 {
            violations.push(format!("{pending} task(s) still pending"));
        }
        if let Some(http) = &self.http {
            violations.extend(http.violations());
        }
        for (window, request) in &self.input_requests {
            violations.push(format!("unconsumed input request in {window:?}: {request:?}"));
        }
        for (window, request) in &self.animation_requests {
            violations.push(format!("unconsumed animation request in {window:?}: {request:?}"));
        }
        violations
    }
}

#[allow(clippy::panic, reason = "unreachable by construction: the placeholder is never called")]
fn unreachable_factory() -> Application {
    panic!("a headless application's factory was used after it was moved")
}

impl Drop for HeadlessApp {
    #[allow(
        clippy::panic,
        reason = "exhaustive mode is an assertion; failing it must fail the test"
    )]
    fn drop(&mut self) {
        if self.exhaustive && !std::thread::panicking() {
            let violations = self.violations();
            assert!(
                violations.is_empty(),
                "exhaustive mode: the test left unasserted work:\n  {}",
                violations.join("\n  ")
            );
        }
    }
}

fn not_interactable(node: &crate::tree::RealizedNode, reason: &str) -> QueryError {
    QueryError::NotInteractable { node: describe(node), reason: reason.to_owned() }
}
