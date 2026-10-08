//! Every window of the running application, and the work each one does:
//! the Android counterpart of the Linux backend's `WindowRegistry`.
//!
//! A window is an activity. The registry owns the application; an
//! activity is attached to its window when Android creates it and detached
//! when Android destroys it — the application, its components, and their
//! state outlive every activity.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rustnative_core::{Application, Event, Lifecycle, NodeId, Size, WindowId};

use crate::backend::{Flow, Work, post};
use crate::jni_host::{Arg, Class, JavaRef, call, call_static};
use crate::rendering::realization::Renderer;
use crate::{Error, NativeContext, protocol};

/// One window's native state.
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent facts about one window, each changing on its own"
)]
pub(crate) struct WindowRuntime {
    pub(crate) id: WindowId,
    /// The activity showing it, while one does.
    pub(crate) activity: Option<JavaRef>,
    /// The activity's root `RnLayout`.
    pub(crate) root: Option<JavaRef>,
    /// The views (`None` until an activity is attached).
    pub(crate) renderer: Option<Renderer>,
    /// The content size the tree is laid out at (dp).
    pub(crate) size: Size,
    /// The root's size in pixels.
    pub(crate) pixels: (i32, i32),
    /// System bars, cutout, IME, and gesture insets, in pixels.
    pub(crate) insets: [i32; 16],
    pub(crate) density: f32,
    /// The application closed it, or its activity finished.
    pub(crate) destroyed: bool,
    /// An activity was asked for and has not arrived.
    pub(crate) opening: bool,
    /// Whether a wake is already on its way to the looper.
    wake_pending: Arc<AtomicBool>,
    /// Pointer, keyboard, and input-method state.
    pub(crate) input: crate::input::InputState,
    /// Whether the activity currently claims the system back gesture.
    pub(crate) back_claimed: bool,
    /// Whether the activity is started (visible).
    pub(crate) started: bool,
    /// The options menu last sent to the activity.
    pub(crate) menu: Option<Vec<crate::menus::Entry>>,
    /// Running animations and transitions (`animation`).
    pub(crate) animation: crate::animation::AnimationState,
}

impl std::fmt::Debug for WindowRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowRuntime")
            .field("id", &self.id)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

/// Every window of the application.
pub(crate) struct WindowRegistry {
    application: RefCell<Application>,
    pub(crate) windows: HashMap<WindowId, WindowRuntime>,
    /// The host's traits, last read.
    pub(crate) traits: crate::host_traits::HostTraits,
    /// Whether an idle callback has been asked for.
    idle_scheduled: bool,
    /// Whether visible-range changes are being reported.
    reporting_ranges: bool,
}

impl WindowRegistry {
    pub(crate) fn new(application: Application) -> Self {
        Self {
            application: RefCell::new(application),
            windows: HashMap::new(),
            traits: crate::host_traits::HostTraits::default(),
            idle_scheduled: false,
            reporting_ranges: false,
        }
    }

    /// Runs `f` against the application.
    ///
    /// # Panics
    ///
    /// Panics if `f` reaches the application again (the backend never
    /// nests work, so it cannot).
    pub(crate) fn with_application<R>(&self, f: impl FnOnce(&mut Application) -> R) -> R {
        f(&mut self.application.borrow_mut())
    }

    /// The window whose id's number is `raw`.
    pub(crate) fn window_for_raw(&self, raw: u64) -> Option<WindowId> {
        self.windows.keys().copied().find(|id| id.get() == raw).or_else(|| {
            self.with_application(|application| {
                application.window_ids().into_iter().find(|id| id.get() == raw)
            })
        })
    }

    /// Whether anything is left to run.
    pub(crate) fn flow(&self) -> Flow {
        let primary_gone =
            self.windows.get(&WindowId::PRIMARY).is_some_and(|runtime| runtime.destroyed);
        if primary_gone { Flow::Finished } else { Flow::Continue }
    }

    // ---- Activities. ----

    /// Attaches `activity` (with its root layout) to `window`: creates the
    /// window's views and shows its tree.
    pub(crate) fn attach_activity(
        &mut self,
        window: WindowId,
        activity: JavaRef,
        root: JavaRef,
        intent: Option<&JavaRef>,
    ) -> Result<(), Error> {
        call(&activity, "attach", "(J)V", &[Arg::Long(i64::try_from(window.get()).unwrap_or(0))])?;
        let density = call_static(
            Class::Bridge,
            "density",
            "(Landroid/content/Context;)F",
            &[Arg::Obj(&activity)],
        )?
        .float()
        .max(0.1);
        let wake_pending = Arc::new(AtomicBool::new(false));
        let runtime = self.windows.entry(window).or_insert_with(|| WindowRuntime {
            id: window,
            activity: None,
            root: None,
            renderer: None,
            size: Size::new(0, 0),
            pixels: (0, 0),
            insets: [0; 16],
            density,
            destroyed: false,
            opening: false,
            wake_pending: Arc::clone(&wake_pending),
            input: crate::input::InputState::default(),
            back_claimed: false,
            started: false,
            menu: None,
            animation: crate::animation::AnimationState::default(),
        });
        if let Some(mut old) = runtime.renderer.take() {
            // A recreated activity: the old views belonged to the old one.
            old.release();
        }
        runtime.activity = Some(activity.clone());
        runtime.root = Some(root);
        runtime.renderer = Some(Renderer::new(window.get(), activity, density));
        runtime.density = density;
        runtime.opening = false;
        runtime.destroyed = false;
        runtime.back_claimed = false;
        runtime.menu = None;
        crate::posture::watch(runtime);
        let wake_pending = Arc::clone(&runtime.wake_pending);
        // A task finishing on another thread wakes this window through the
        // main looper.
        let waker = Arc::new(move || {
            if !wake_pending.swap(true, Ordering::AcqRel) {
                crate::looper::post(move || post(Work::Pump(window)));
            }
        });
        self.with_application(|application| {
            if let Some(scheduler) = application.scheduler_for(window) {
                scheduler.set_waker(waker);
            }
        });
        self.refresh_host_traits()?;
        // The root may have its size already (the activity was laid out
        // before the application attached): that is the window's size.
        let root_frame = self
            .windows
            .get(&window)
            .and_then(|runtime| runtime.root.as_ref())
            .map(crate::rendering::controls::frame);
        if let Some([_, _, width, height]) = root_frame {
            if width > 0 && height > 0 {
                self.resized(window, width, height)?;
            }
        }
        self.render(window)?;
        if let Some(intent) = intent {
            if window == WindowId::PRIMARY {
                self.intent(window, intent)?;
            }
        }
        self.after_change(window)
    }

    /// Ends every activity (the application is over).
    pub(crate) fn finish_activities(&self) {
        for runtime in self.windows.values() {
            if let Some(activity) = &runtime.activity {
                let _ = call(activity, "finish", "()V", &[]);
            }
        }
    }

    /// Restores what the backend changed outside its own views (the soft
    /// keyboard, a back claim) — on a panic, and when the backend stops.
    pub(crate) fn restore_host_state(&mut self) {
        for runtime in self.windows.values_mut() {
            runtime.input.release();
            if runtime.back_claimed {
                if let Some(activity) = &runtime.activity {
                    let _ = call(activity, "setBackHandled", "(Z)V", &[Arg::Bool(false)]);
                }
                runtime.back_claimed = false;
            }
        }
    }

    /// Releases every window's views.
    pub(crate) fn release_all(&mut self) {
        self.restore_host_state();
        for (id, runtime) in &mut self.windows {
            if let Some(renderer) = runtime.renderer.as_mut() {
                renderer.release();
            }
            // The views' surfaces go with them; their destruction callbacks
            // reach a stopped backend, so they are released here.
            crate::surface::release_all(*id);
        }
    }

    // ---- Work. ----

    /// Runs one work item.
    pub(crate) fn handle(&mut self, work: Work) -> Result<Flow, Error> {
        match work {
            Work::Event(window, event) => self.dispatch(window, event)?,
            Work::Pump(window) => self.pump(window)?,
            Work::Resized(window, width, height) => self.resized(window, width, height)?,
            Work::Insets(window, insets) => self.insets_changed(window, insets)?,
            Work::Lifecycle(window, what, argument) => {
                return crate::lifecycle::step(self, window, what, argument);
            }
            Work::ViewEvent { window, tag, event, a, b, text } => {
                self.view_event(window, tag, event, a, b, text)?;
            }
            Work::Back { window, phase, progress, edge } => {
                crate::input::back(self, window, phase, progress, edge)?;
            }
            Work::Intent(window, intent) => self.intent(window, &intent)?,
            Work::Frame(window, nanos) => crate::animation::frame(self, window, nanos)?,
            Work::Idle => self.idle()?,
            Work::Timer(token) => crate::timers::fired(self, token)?,
            Work::Call(call) => call(self),
        }
        Ok(self.flow())
    }

    /// Delivers `event` to `window`'s components and realizes the result.
    pub(crate) fn dispatch(&mut self, window: WindowId, event: Event) -> Result<(), Error> {
        let handled =
            self.with_application(|application| application.dispatch_to_window(window, event));
        if handled {
            self.render(window)?;
        }
        self.after_change(window)
    }

    /// What follows every change the application made.
    pub(crate) fn after_change(&mut self, window: WindowId) -> Result<(), Error> {
        crate::input::apply_requests(self, window)?;
        crate::animation::apply_requests(self, window)?;
        crate::input::sync_back(self, window)?;
        if self.with_application(|application| application.has_deferred_work(window)) {
            self.schedule_idle()?;
        }
        self.sync()?;
        crate::surfaces::apply(self)
    }

    fn schedule_idle(&mut self) -> Result<(), Error> {
        if !self.idle_scheduled {
            self.idle_scheduled = true;
            call_static(Class::Bridge, "scheduleIdle", "()V", &[])?;
        }
        Ok(())
    }

    /// The main thread is idle: run deferred work, one window at a time.
    fn idle(&mut self) -> Result<(), Error> {
        self.idle_scheduled = false;
        let windows: Vec<WindowId> = self.windows.keys().copied().collect();
        for window in windows {
            if self.with_application(|application| application.pump_deferred_for(window)) {
                self.render(window)?;
            }
            self.after_change(window)?;
        }
        Ok(())
    }

    fn pump(&mut self, window: WindowId) -> Result<(), Error> {
        if let Some(runtime) = self.windows.get(&window) {
            runtime.wake_pending.store(false, Ordering::Release);
        }
        let inspected = crate::inspect::poll(self, window);
        if self.with_application(|application| application.pump_tasks_for(window)) || inspected {
            self.render(window)?;
        }
        self.after_change(window)
    }

    fn resized(&mut self, window: WindowId, width: i32, height: i32) -> Result<(), Error> {
        let Some(runtime) = self.windows.get_mut(&window) else { return Ok(()) };
        runtime.pixels = (width, height);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "a window's size in pixels over its density"
        )]
        let dp = |pixels: i32| (pixels.max(0) as f32 / runtime.density).floor() as u32;
        let size = Size::new(dp(width), dp(height));
        if runtime.size == size || runtime.destroyed {
            return Ok(());
        }
        runtime.size = size;
        crate::environment::window_changed(self, window);
        self.with_application(|application| {
            application.dispatch_to_window(window, Event::WindowResized { window, size });
        });
        self.render(window)?;
        self.relayout(window)?;
        self.after_change(window)
    }

    fn insets_changed(&mut self, window: WindowId, insets: [i32; 16]) -> Result<(), Error> {
        let Some(runtime) = self.windows.get_mut(&window) else { return Ok(()) };
        if runtime.insets == insets {
            return Ok(());
        }
        runtime.insets = insets;
        crate::environment::window_changed(self, window);
        self.render(window)?;
        self.after_change(window)
    }

    /// A view reported something: translated into the portable event for
    /// its node.
    fn view_event(
        &mut self,
        window: WindowId,
        tag: i32,
        event: i32,
        a: i64,
        b: i64,
        text: Option<String>,
    ) -> Result<(), Error> {
        if tag == protocol::MENU_TAG {
            let id = text.unwrap_or_default();
            return crate::menus::chosen(self, window, &id, a == 1);
        }
        let Some(node) = self
            .windows
            .get(&window)
            .and_then(|runtime| runtime.renderer.as_ref())
            .and_then(|renderer| renderer.registry.node_for_tag(tag))
        else {
            return Ok(());
        };
        if event == protocol::EV_SCROLL {
            return self.list_scrolled(window);
        }
        if event == protocol::EV_SURFACE {
            return crate::surface::changed(self, window, node, tag, a, b);
        }
        if event == protocol::EV_ACCESSIBILITY {
            let Some(action) = crate::accessibility::action_of(a, text.as_deref()) else {
                return Ok(());
            };
            let element = usize::try_from(b).ok().and_then(|index| {
                let renderer = self.windows.get(&window)?.renderer.as_ref()?;
                renderer.accessibility.elements.get(&node)?.get(index).copied()
            });
            return self
                .dispatch(window, Event::AccessibilityAction { target: node, element, action });
        }
        if event == protocol::EV_FOCUS || event == protocol::EV_BLUR {
            if let Some(runtime) = self.windows.get_mut(&window) {
                if event == protocol::EV_FOCUS {
                    runtime.input.focused = Some(node);
                } else if runtime.input.focused == Some(node) {
                    runtime.input.focused = None;
                }
            }
            let focused = self.windows.get(&window).and_then(|runtime| runtime.input.focused);
            crate::input::focus_moved(self, window, focused)?;
        }
        if event == protocol::EV_DRAG {
            return self.drag(window, node, a, b, text.as_deref());
        }
        let Some(portable) = translate_view_event(node, event, a, b, text) else { return Ok(()) };
        let controlled = matches!(
            portable,
            Event::Toggled { .. }
                | Event::ValueChanged { .. }
                | Event::SelectionChanged { .. }
                | Event::DateChanged { .. }
                | Event::TabSelected { .. }
        );
        self.dispatch(window, portable)?;
        if controlled {
            // A controlled control shows what the component decided, not
            // what the person did, when the two differ.
            self.resync_node(window, node)?;
        }
        Ok(())
    }

    /// Re-applies `node`'s declared content to its view.
    fn resync_node(&self, window: WindowId, node: NodeId) -> Result<(), Error> {
        let Some(renderer) =
            self.windows.get(&window).and_then(|runtime| runtime.renderer.as_ref())
        else {
            return Ok(());
        };
        let (Some(object), Some(tree_node)) =
            (renderer.registry.get(node), renderer.snapshot.get(node))
        else {
            return Ok(());
        };
        let _muted = crate::rendering::realization::Muted::new();
        crate::rendering::controls::update(object, tree_node)
    }

    /// A step of a drag over a drop target (`RnViews.setDropTarget`).
    fn drag(
        &mut self,
        window: WindowId,
        node: NodeId,
        step: i64,
        packed: i64,
        data: Option<&str>,
    ) -> Result<(), Error> {
        let density = self.windows.get(&window).map_or(1.0, |runtime| runtime.density);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_precision_loss,
            reason = "a position in pixels over the density"
        )]
        let dp = |pixels: i64| (pixels as f32 / density.max(0.1)).round() as i32;
        #[allow(clippy::cast_possible_truncation, reason = "the low half is the y position")]
        let y = i64::from(packed as i32);
        let position = rustnative_core::Point::new(dp(packed >> 32), dp(y));
        let data = drag_data(data.unwrap_or_default());
        let event = match step {
            1 => Event::DragEnter { target: node, data, position },
            2 => Event::DragOver { target: node, data, position },
            3 => Event::DragLeave { target: node },
            _ => Event::Drop { target: node, data, position },
        };
        self.dispatch(window, event)
    }

    fn list_scrolled(&mut self, window: WindowId) -> Result<(), Error> {
        if let Some(renderer) =
            self.windows.get_mut(&window).and_then(|runtime| runtime.renderer.as_mut())
        {
            renderer.update_visible_ranges();
        }
        self.report_ranges(window)
    }

    /// An intent: a deep link (`ACTION_VIEW`) or a share (`ACTION_SEND`).
    pub(crate) fn intent(&mut self, window: WindowId, intent: &JavaRef) -> Result<(), Error> {
        match crate::intents::read(intent)? {
            crate::intents::Incoming::DeepLink(url) => {
                self.dispatch(WindowId::PRIMARY, Event::DeepLink { url })?;
            }
            crate::intents::Incoming::Share(share) => {
                self.dispatch(WindowId::PRIMARY, Event::ShareReceived { share })?;
            }
            crate::intents::Incoming::SurfaceAction(surface, action) => {
                self.dispatch(
                    WindowId::PRIMARY,
                    Event::SurfaceAction { window: WindowId::PRIMARY, surface, action },
                )?;
            }
            crate::intents::Incoming::Nothing => {}
        }
        let _ = window;
        Ok(())
    }

    /// Realizes `window`'s current view.
    pub(crate) fn render(&mut self, window: WindowId) -> Result<(), Error> {
        let Some((tree, theme, direction, text_scale)) = self.with_application(|application| {
            let text_scale =
                application.environment_for(window, &rustnative_core::keys::TEXT_SCALE).get();
            application.view_for(window).map(|tree| {
                (
                    tree,
                    application.theme().clone(),
                    application.layout_direction(window),
                    text_scale,
                )
            })
        }) else {
            return Ok(());
        };
        let Some(runtime) = self.windows.get_mut(&window) else { return Ok(()) };
        if runtime.destroyed {
            return Ok(());
        }
        let (Some(renderer), Some(root)) = (runtime.renderer.as_mut(), runtime.root.clone()) else {
            return Ok(());
        };
        renderer.set_scales(runtime.density, text_scale);
        renderer
            .render(&tree, &theme, direction, &root, runtime.size)
            .map_err(|error| error.or_context(NativeContext::none().with_window(window)))?;
        crate::accessibility::after_render(self, window)?;
        self.report_container_sizes(window)?;
        crate::animation::after_render(self, window)?;
        self.report_ranges(window)?;
        rustnative_core::perf::realized();
        Ok(())
    }

    /// Reports every virtual list whose visible range changed.
    fn report_ranges(&mut self, window: WindowId) -> Result<(), Error> {
        const MAX_PASSES: usize = 8;
        if self.reporting_ranges {
            return Ok(());
        }
        self.reporting_ranges = true;
        let mut outcome = Ok(());
        for _ in 0..MAX_PASSES {
            let changes = self
                .windows
                .get_mut(&window)
                .and_then(|runtime| runtime.renderer.as_mut())
                .map(Renderer::take_range_changes)
                .unwrap_or_default();
            if changes.is_empty() {
                break;
            }
            for (target, range) in changes {
                if let Err(error) =
                    self.dispatch(window, Event::VisibleRangeChanged { target, range })
                {
                    outcome = Err(error);
                    break;
                }
            }
            if outcome.is_err() {
                break;
            }
        }
        self.reporting_ranges = false;
        outcome
    }

    /// Lays `window` out again at its current size.
    pub(crate) fn relayout(&mut self, window: WindowId) -> Result<(), Error> {
        if let Some(runtime) = self.windows.get_mut(&window) {
            if let (Some(renderer), Some(root)) = (runtime.renderer.as_mut(), runtime.root.clone())
            {
                renderer.relayout(&root, runtime.size)?;
            }
        }
        self.report_container_sizes(window)?;
        crate::animation::after_render(self, window)
    }

    /// Reports the laid-out sizes of nodes a component decides by.
    fn report_container_sizes(&mut self, window: WindowId) -> Result<(), Error> {
        let watched = self.with_application(|application| application.watched_nodes(window));
        if watched.is_empty() {
            return Ok(());
        }
        let Some(renderer) =
            self.windows.get(&window).and_then(|runtime| runtime.renderer.as_ref())
        else {
            return Ok(());
        };
        let sizes: Vec<_> = watched
            .into_iter()
            .filter_map(|node| renderer.layout_size(node).map(|size| (node, size)))
            .collect();
        if self.with_application(|application| application.report_sizes(window, sizes)) {
            let refreshed = self.with_application(|application| {
                application.view_for(window).map(|tree| {
                    (tree, application.theme().clone(), application.layout_direction(window))
                })
            });
            if let (Some((tree, theme, direction)), Some(runtime)) =
                (refreshed, self.windows.get_mut(&window))
            {
                if let (Some(renderer), Some(root)) =
                    (runtime.renderer.as_mut(), runtime.root.clone())
                {
                    renderer.render(&tree, &theme, direction, &root, runtime.size)?;
                }
            }
        }
        Ok(())
    }

    /// Brings the activities in line with the application's windows: a new
    /// window asks for an activity; a closed one finishes its activity.
    pub(crate) fn sync(&mut self) -> Result<(), Error> {
        let desired = self.with_application(|application| application.window_ids());
        let closing: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(id, runtime)| !runtime.destroyed && !desired.contains(id))
            .map(|(id, _)| *id)
            .collect();
        for id in closing {
            self.close(id);
        }
        let primary =
            self.windows.get(&WindowId::PRIMARY).and_then(|runtime| runtime.activity.clone());
        for id in desired {
            if id == WindowId::PRIMARY || self.windows.contains_key(&id) {
                continue;
            }
            let Some(primary) = &primary else { continue };
            let title = self
                .with_application(|application| {
                    application.window_for(id).map(|definition| definition.title().to_owned())
                })
                .unwrap_or_default();
            self.windows.insert(
                id,
                WindowRuntime {
                    id,
                    activity: None,
                    root: None,
                    renderer: None,
                    size: Size::new(0, 0),
                    pixels: (0, 0),
                    insets: [0; 16],
                    density: 1.0,
                    destroyed: false,
                    opening: true,
                    wake_pending: Arc::new(AtomicBool::new(false)),
                    input: crate::input::InputState::default(),
                    back_claimed: false,
                    started: false,
                    menu: None,
                    animation: crate::animation::AnimationState::default(),
                },
            );
            call_static(
                Class::Bridge,
                "openWindow",
                "(Landroid/app/Activity;JLjava/lang/String;)V",
                &[
                    Arg::Obj(primary),
                    Arg::Long(i64::try_from(id.get()).unwrap_or(0)),
                    Arg::Str(&title),
                ],
            )?;
        }
        crate::menus::sync(self)
    }

    /// Closes window `id`: its views go, and its activity finishes.
    pub(crate) fn close(&mut self, id: WindowId) {
        let Some(runtime) = self.windows.get_mut(&id) else { return };
        if runtime.destroyed {
            return;
        }
        runtime.destroyed = true;
        runtime.input.release();
        crate::surface::release_all(id);
        if let Some(renderer) = runtime.renderer.as_mut() {
            renderer.release();
        }
        if let Some(activity) = runtime.activity.take() {
            let _ = call(&activity, "finish", "()V", &[]);
        }
        runtime.root = None;
    }

    /// An activity was destroyed without finishing (the system will make a
    /// new one): its views go; the window stays.
    pub(crate) fn activity_gone(&mut self, id: WindowId) {
        if let Some(runtime) = self.windows.get_mut(&id) {
            runtime.input.release();
            crate::surface::release_all(id);
            if let Some(mut renderer) = runtime.renderer.take() {
                renderer.release();
            }
            runtime.activity = None;
            runtime.root = None;
            runtime.started = false;
            runtime.back_claimed = false;
        }
    }

    /// Reads the host's traits, feeds them into the environment, and
    /// realizes what that changed in every window.
    pub(crate) fn refresh_host_traits(&mut self) -> Result<(), Error> {
        let Some(activity) = self.windows.values().find_map(|runtime| runtime.activity.clone())
        else {
            return Ok(());
        };
        let traits = crate::host_traits::read(&activity)?;
        let changed = traits != self.traits;
        self.traits = traits.clone();
        self.with_application(|application| crate::host_traits::apply(application, &traits));
        let windows: Vec<WindowId> = self.windows.keys().copied().collect();
        for window in windows {
            if let Some(runtime) = self.windows.get_mut(&window) {
                runtime.density = traits.density;
                if let Some(renderer) = runtime.renderer.as_mut() {
                    renderer.styles.set_baseline(
                        rustnative_core::Theme::default().with_host_palette(&traits.palette),
                    );
                    if changed {
                        renderer.restyle_all();
                    }
                }
            }
            crate::environment::window_changed(self, window);
            if changed {
                self.render(window)?;
            }
        }
        Ok(())
    }

    /// Delivers a lifecycle event (state is flushed first by the core).
    pub(crate) fn lifecycle(&mut self, lifecycle: Lifecycle) -> Result<(), Error> {
        let flushed = self.with_application(|application| application.lifecycle(lifecycle));
        if let Err(error) = flushed {
            crate::log::warn(&format!("state was not flushed: {error}"));
        }
        let windows: Vec<WindowId> = self.windows.keys().copied().collect();
        for window in windows {
            self.render(window)?;
        }
        Ok(())
    }
}

/// Dropped data from `RnViews.describe`: the text, then each URI, split
/// by U+0001. A URI is carried as the drop's file (Android hands content
/// URIs, not paths).
fn drag_data(described: &str) -> rustnative_core::DragData {
    let mut parts = described.split('\u{1}');
    let text = parts.next().filter(|text| !text.is_empty()).map(str::to_owned);
    let files: Vec<std::path::PathBuf> =
        parts.filter(|uri| !uri.is_empty()).map(std::path::PathBuf::from).collect();
    let mut data = rustnative_core::DragData::new().with_files(files);
    if let Some(text) = text {
        data = data.with_text(text);
    }
    data
}

/// The portable event a view's report stands for.
fn translate_view_event(
    node: NodeId,
    event: i32,
    a: i64,
    _b: i64,
    text: Option<String>,
) -> Option<Event> {
    let index = |value: i64| usize::try_from(value).ok();
    Some(match event {
        protocol::EV_CLICK => Event::Click { target: node },
        protocol::EV_TEXT => Event::TextChanged { target: node, value: text.unwrap_or_default() },
        protocol::EV_TOGGLED => Event::Toggled { target: node, on: a != 0 },
        protocol::EV_VALUE => Event::ValueChanged { target: node, value: a },
        protocol::EV_SELECTION => Event::SelectionChanged { target: node, index: index(a) },
        protocol::EV_DATE => {
            let year = i32::try_from(a / 10000).ok()?;
            let month = u8::try_from((a / 100) % 100).ok()?;
            let day = u8::try_from(a % 100).ok()?;
            Event::DateChanged {
                target: node,
                date: rustnative_core::CalendarDate::new(year, month, day)?,
            }
        }
        protocol::EV_FOCUS => Event::FocusGained { target: node },
        protocol::EV_BLUR => Event::FocusLost { target: node },
        protocol::EV_TAB => Event::TabSelected { target: node, index: index(a)? },
        _ => return None,
    })
}
