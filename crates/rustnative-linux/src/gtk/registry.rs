//! Every window of one run, and the work each one does: the GTK
//! counterpart of the Windows backend's `WindowRegistry` and `Runtime`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gtk::glib;
use gtk::prelude::*;
use rustnative_core::{Application, Event, Size, WindowId, WindowPresentation};

use super::backend::{Flow, Work, post, post_later};
use super::context::HostRef;
use super::layout_widget::RnLayout;
use super::rendering::realization::Renderer;
use super::rendering::styling::StyleSheet;
use crate::desktop::settings::SettingsPortal;
use crate::{Error, NativeContext};

thread_local! {
    /// Each open window's `GtkWindow`, for code that names a window by id
    /// (a dialog's owner) without the registry at hand.
    static GTK_WINDOWS: RefCell<HashMap<WindowId, gtk::Window>> = RefCell::new(HashMap::new());
}

/// The `GtkWindow` of window `id`, if it is open (on the GTK thread).
pub(crate) fn gtk_window(id: WindowId) -> Option<gtk::Window> {
    GTK_WINDOWS.with(|windows| windows.borrow().get(&id).cloned())
}

/// One top-level window's native state.
pub(crate) struct WindowRuntime {
    pub(crate) id: WindowId,
    /// The GTK window (`None` for an embedded root, which lives in a
    /// host's widget tree).
    pub(crate) window: Option<gtk::Window>,
    /// The container every root node is placed in.
    pub(crate) root: RnLayout,
    pub(crate) renderer: Renderer,
    /// The content size the tree is laid out at.
    pub(crate) size: Size,
    pub(crate) destroyed: bool,
    /// The window this one is modal to, if any.
    pub(crate) modal_parent: Option<WindowId>,
    /// Whether a wake is already on its way to the loop.
    wake_pending: Arc<AtomicBool>,
    /// Keyboard, pointer, and input-method state (`gtk::input`).
    pub(crate) input: super::input::InputState,
    /// Running animations (`gtk::animation`).
    pub(crate) animation: super::animation::AnimationState,
    /// Native surfaces (`gtk::surface`).
    pub(crate) surfaces: super::surface::NativeSurfaces,
    /// The window's menu bar (`gtk::menu`).
    pub(crate) menu: Option<super::menu::WindowMenu>,
    /// The virtual lists whose scrolling is watched.
    watched_lists: std::collections::HashSet<rustnative_core::NodeId>,
    /// The inspector's overlay, while shown (`gtk::inspect`).
    pub(crate) overlay: Option<super::canvas::RnCanvas>,
}

impl WindowRuntime {
    /// Watches each virtual list's adjustments, so scrolling one recomputes
    /// its range (inside a range, nothing more happens).
    fn watch_list_scrolling(&mut self) {
        let snapshot = &self.renderer.snapshot;
        self.watched_lists.retain(|id| snapshot.contains(*id));
        let window = self.id;
        let lists: Vec<rustnative_core::NodeId> = snapshot
            .nodes()
            .filter(|node| node.virtualization.is_some())
            .map(|node| node.id)
            .filter(|id| !self.watched_lists.contains(id))
            .collect();
        for id in lists {
            let Some(scrolled) = self
                .renderer
                .widget(id)
                .and_then(|widget| widget.clone().downcast::<gtk::ScrolledWindow>().ok())
            else {
                continue;
            };
            for adjustment in [scrolled.hadjustment(), scrolled.vadjustment()] {
                adjustment.connect_value_changed(move |_| post(Work::ListScrolled(window)));
            }
            self.watched_lists.insert(id);
        }
    }
}

impl std::fmt::Debug for WindowRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowRuntime")
            .field("id", &self.id)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

/// Every window of one run.
pub(crate) struct WindowRegistry {
    application: HostRef<Application>,
    pub(crate) windows: HashMap<WindowId, WindowRuntime>,
    pub(crate) styles: Rc<RefCell<StyleSheet>>,
    /// The widget an embedded primary window's root is handed to, instead
    /// of a `GtkWindow` (`LinuxPlatform::embed`).
    embedded: bool,
    /// Where the desktop's appearance settings are read from.
    portal: SettingsPortal,
    /// Handlers on the process-wide `GtkSettings`, removed when this run
    /// ends.
    settings_handlers: Vec<glib::SignalHandlerId>,
    /// The desktop's reduced-motion setting, which every timeline follows.
    motion: rustnative_core::MotionPreference,
    /// The desktop's lifecycle sources and the idle flush (`gtk::lifecycle`).
    lifecycle: super::lifecycle::Watchers,
    /// The application's id, for its tray icon and launcher entry.
    pub(crate) app_id: Option<String>,
    /// The tray icon, while shown.
    tray: Option<crate::desktop::tray::Tray>,
    /// Whether visible-range changes are being reported (`report_ranges`).
    reporting_ranges: bool,
}

impl WindowRegistry {
    pub(crate) fn new(application: HostRef<Application>, embedded: bool) -> Self {
        let portal = SettingsPortal::connect();
        portal.connect_changed(|| post(Work::HostTraitsChanged));
        let mut settings_handlers = Vec::new();
        if let Some(settings) = gtk::Settings::default() {
            for property in [
                "gtk-application-prefer-dark-theme",
                "gtk-theme-name",
                "gtk-xft-dpi",
                "gtk-enable-animations",
            ] {
                settings_handlers.push(settings.connect_notify_local(Some(property), |_, _| {
                    post_later(Work::HostTraitsChanged);
                }));
            }
        }
        Self {
            application,
            windows: HashMap::new(),
            styles: Rc::new(RefCell::new(StyleSheet::install())),
            embedded,
            portal,
            settings_handlers,
            motion: rustnative_core::MotionPreference::Full,
            lifecycle: super::lifecycle::Watchers::start(),
            app_id: None,
            tray: None,
            reporting_ranges: false,
        }
    }

    /// The lifecycle watchers (the idle flush clears its pending source).
    pub(crate) fn lifecycle_watchers(&self) -> &super::lifecycle::Watchers {
        &self.lifecycle
    }

    /// `window`'s menu bar, for tests.
    #[cfg(test)]
    pub(crate) fn window_menu(&self, window: WindowId) -> Option<&super::menu::WindowMenu> {
        self.windows.get(&window)?.menu.as_ref()
    }

    /// Brings every window's command-bound menu items up to date.
    fn refresh_menus(&mut self) {
        let ids: Vec<WindowId> = self.windows.keys().copied().collect();
        for id in ids {
            let Some(runtime) = self.windows.get_mut(&id) else { continue };
            let Some(mut menu) = runtime.menu.take() else { continue };
            let focused = runtime.input.focused;
            menu.refresh(|command| {
                self.with_application(|application| {
                    application.command_state(id, command, focused).map(|declared| {
                        (declared.is_enabled(), declared.is_checked(), declared.shortcut_key())
                    })
                })
            });
            if let Some(runtime) = self.windows.get_mut(&id) {
                runtime.menu = Some(menu);
            }
        }
    }

    /// Applies what the application asked of its surfaces (the tray, the
    /// launcher's progress, notifications from the tray).
    fn apply_surfaces(&mut self) {
        use rustnative_core::surfaces::{ACTIVATE, NOTIFICATION, SurfaceCommand};
        let commands =
            self.with_application(|application| application.services().surfaces().take());
        let action = |action: String| {
            post(Work::Event(
                WindowId::PRIMARY,
                Event::SurfaceAction {
                    window: WindowId::PRIMARY,
                    surface: rustnative_core::capability::SurfaceKind::TrayExtra,
                    action,
                },
            ));
        };
        let app_id = self
            .app_id
            .clone()
            .unwrap_or_else(|| glib::prgname().map(|name| name.to_string()).unwrap_or_default());
        for command in commands {
            match command {
                SurfaceCommand::ShowTray { tooltip, menu } => {
                    let menu: Vec<(String, String)> =
                        menu.into_iter().map(|item| (item.id, item.label)).collect();
                    if let Some(tray) = &self.tray {
                        tray.update(&tooltip, menu);
                    } else {
                        let icon = if app_id.is_empty() {
                            "application-x-executable".to_owned()
                        } else {
                            app_id.clone()
                        };
                        // Without a tray host the icon is not shown; the
                        // capability says so up front.
                        self.tray = crate::desktop::tray::Tray::show(
                            &app_id, &icon, &tooltip, menu, action,
                        )
                        .ok();
                    }
                }
                SurfaceCommand::HideTray => self.tray = None,
                SurfaceCommand::Progress(progress) => {
                    crate::desktop::tray::set_progress(&app_id, progress);
                }
                SurfaceCommand::Notify { title, body } => {
                    let application =
                        glib::application_name().map(|name| name.to_string()).unwrap_or_default();
                    glib::MainContext::default().spawn_local(async move {
                        let clicked: Box<dyn Fn()> =
                            Box::new(move || action(NOTIFICATION.to_owned()));
                        let _ = crate::desktop::notifications::notify(
                            &application,
                            &title,
                            &body,
                            Some(clicked),
                        )
                        .await;
                    });
                }
                // A jump list is the desktop entry's actions, fixed when the
                // application is packaged (`Capability::Surface(JumpList)`
                // is not advertised).
                _ => {}
            }
        }
        let _ = ACTIVATE;
    }

    /// Reads the host's traits, makes GTK follow them, feeds them into the
    /// environment, and realizes whatever that changed in every window.
    pub(crate) fn refresh_host_traits(&mut self) -> Result<(), Error> {
        let traits = super::host_traits::read(&self.portal);
        super::host_traits::follow_in_gtk(&traits);
        self.styles
            .borrow_mut()
            .set_baseline(rustnative_core::Theme::default().with_host_palette(&traits.palette));
        self.with_application(|application| super::host_traits::apply(application, &traits));
        self.motion = traits.motion;
        self.set_motion_preference(traits.motion);
        let live: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, runtime)| !runtime.destroyed)
            .map(|(id, _)| *id)
            .collect();
        for window in live {
            // The desktop's theme decides the widgets' padding and fonts, so
            // every node is restyled and measured again.
            if let Some(runtime) = self.windows.get_mut(&window) {
                runtime.renderer.restyle_all();
            }
            self.render(window)?;
        }
        Ok(())
    }

    /// Runs `f` against the application. Narrow by construction: `f` reads
    /// or mutates what it needs and returns.
    pub(crate) fn with_application<R>(&self, f: impl FnOnce(&mut Application) -> R) -> R {
        // SAFETY: `application` is the borrow `run` (or the harness) holds
        // for as long as this registry exists (`backend::Backend::attach`'s
        // contract); the backend runs one work item at a time and `f` never
        // re-enters it (`gtk::context`, points 1–3).
        unsafe { self.application.with(f) }
    }

    /// Runs one work item.
    pub(crate) fn handle(&mut self, work: Work) -> Result<Flow, Error> {
        match work {
            Work::Event(window, event) => self.dispatch(window, event)?,
            Work::Pump(window) => self.pump(window)?,
            Work::Resized(window, size) => self.resized(window, size)?,
            Work::CloseRequested(window) => self.close_requested(window)?,
            Work::Destroyed(window) => {
                if let Some(runtime) = self.windows.get_mut(&window) {
                    runtime.destroyed = true;
                }
                if window == WindowId::PRIMARY && !self.embedded {
                    return Ok(Flow::Finished);
                }
            }
            Work::HostTraitsChanged => self.refresh_host_traits()?,
            Work::Key { window, key, modifiers, pressed } => {
                self.key(window, key, modifiers, pressed)?;
            }
            Work::Text { window, text } => self.text(window, text)?,
            Work::Composition { window, composition } => self.composition(window, composition)?,
            Work::FocusChanged(window) => self.focus_changed(window)?,
            Work::LongPress(window) => self.long_press(window)?,
            Work::Frame(window) => self.animation_frame(window)?,
            Work::SurfaceAllocated(window, node) => self.surface_allocated(window, node)?,
            // Every native surface is told its new scale, as the surface
            // hand-off contract requires (`docs/interop/surface-handoff.md`);
            // GTK's own widgets and the canvas redraw at it by themselves.
            Work::ScaleChanged(window) => {
                let nodes = self
                    .windows
                    .get(&window)
                    .map(|runtime| runtime.surfaces.nodes())
                    .unwrap_or_default();
                for node in nodes {
                    self.surface_allocated(window, node)?;
                }
            }
            Work::ListScrolled(window) => {
                if let Some(runtime) = self.windows.get_mut(&window) {
                    runtime.renderer.update_visible_ranges();
                }
                self.report_ranges(window)?;
            }
            Work::Call(call) => call(self),
        }
        Ok(self.flow())
    }

    /// Whether anything is left to run.
    pub(crate) fn flow(&self) -> Flow {
        let live = self.windows.values().any(|runtime| !runtime.destroyed);
        let primary_gone =
            self.windows.get(&WindowId::PRIMARY).is_some_and(|runtime| runtime.destroyed);
        if live && !primary_gone { Flow::Continue } else { Flow::Finished }
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
    fn after_change(&mut self, window: WindowId) -> Result<(), Error> {
        self.apply_input_requests(window);
        self.apply_animation_requests(window);
        self.schedule_deferred(window);
        let unsaved = self.with_application(|application| application.has_unsaved_state());
        self.lifecycle.after_change(unsaved);
        self.sync()
    }

    fn pump(&mut self, window: WindowId) -> Result<(), Error> {
        if let Some(runtime) = self.windows.get(&window) {
            runtime.wake_pending.store(false, Ordering::Release);
        }
        // An inspector's requests wake the loop as a task does (`PLAN.md`
        // Milestone 44); an answered edit or overlay change is realized like
        // a task's result.
        let inspected = self.windows.get(&window).is_some_and(|runtime| {
            self.with_application(|application| super::inspect::poll(runtime, application))
        });
        if self.with_application(|application| application.pump_tasks_for(window)) || inspected {
            self.render(window)?;
        }
        if self.with_application(|application| application.pump_deferred_for(window)) {
            self.render(window)?;
        }
        self.after_change(window)
    }

    /// Asks for another pump while deferrable work remains. Posted at idle
    /// priority by the queue's own drain, so input queued meanwhile runs
    /// first (Milestone 54).
    fn schedule_deferred(&self, window: WindowId) {
        if self.with_application(|application| application.has_deferred_work(window)) {
            glib::idle_add_local_full(glib::Priority::DEFAULT_IDLE, move || {
                post(Work::Pump(window));
                glib::ControlFlow::Break
            });
        }
    }

    fn resized(&mut self, window: WindowId, size: Size) -> Result<(), Error> {
        let Some(runtime) = self.windows.get_mut(&window) else { return Ok(()) };
        if runtime.size == size || runtime.destroyed {
            return Ok(());
        }
        runtime.size = size;
        let presentation =
            runtime.window.as_ref().map_or(WindowPresentation::Normal, presentation_of);
        self.with_application(|application| {
            application.dispatch_to_window(window, Event::WindowResized { window, size });
            application.dispatch_to_window(
                window,
                Event::WindowStateChanged { window, state: presentation },
            );
        });
        // Either dispatch may have re-rendered; the render lays out at the
        // new size, and an unchanged tree still needs placing again.
        self.render(window)?;
        self.relayout(window)?;
        self.after_change(window)
    }

    fn close_requested(&mut self, window: WindowId) -> Result<(), Error> {
        self.dispatch(window, Event::WindowCloseRequested { window })?;
        // The primary window's close is the application's exit; any other
        // window leaves the application's set so `sync` closes it.
        if window == WindowId::PRIMARY {
            self.destroy(window);
        } else {
            self.with_application(|application| application.close_window(window));
            self.sync()?;
        }
        Ok(())
    }

    /// Realizes `window`'s current view.
    pub(crate) fn render(&mut self, window: WindowId) -> Result<(), Error> {
        let Some((tree, theme, direction)) = self.with_application(|application| {
            application.view_for(window).map(|tree| {
                (tree, application.theme().clone(), application.layout_direction(window))
            })
        }) else {
            return Ok(());
        };
        let text_scale = self.with_application(|application| {
            application.environment_for(window, &rustnative_core::keys::TEXT_SCALE).get()
        });
        if self.styles.borrow_mut().set_text_scale(text_scale) {
            for runtime in self.windows.values_mut() {
                runtime.renderer.restyle_all();
            }
        }
        let Some(runtime) = self.windows.get_mut(&window) else { return Ok(()) };
        if runtime.destroyed {
            return Ok(());
        }
        runtime
            .renderer
            .render(&tree, &theme, direction, &runtime.root, runtime.size)
            .map_err(|error| error.or_context(NativeContext::none().with_window(window)))?;
        super::input::pointer::prune(runtime);
        let snapshot = &runtime.renderer.snapshot;
        runtime.surfaces.retain(|node| snapshot.contains(node));
        runtime.watch_list_scrolling();
        self.report_container_sizes(window)?;
        self.after_animation_change(window);
        self.report_ranges(window)?;
        if let Some(mut runtime) = self.windows.remove(&window) {
            self.with_application(|application| {
                super::inspect::sync_overlay(&mut runtime, application);
            });
            self.windows.insert(window, runtime);
        }
        rustnative_core::perf::realized();
        Ok(())
    }

    /// Reports every virtual list whose visible range changed, rendering
    /// the items each component returns. A render inside this loop queues
    /// its own change rather than nesting a dispatch; the loop picks it up.
    /// Two passes is the normal maximum (a range, then the measurement it
    /// settles at); the cap is a backstop against a component that renders
    /// a different item count than it was asked for.
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
                .map(|runtime| runtime.renderer.take_range_changes())
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
            runtime.renderer.relayout(&runtime.root, runtime.size);
        }
        self.report_container_sizes(window)?;
        self.after_animation_change(window);
        Ok(())
    }

    /// Reports the laid-out sizes of nodes a component decides by, and
    /// realizes the arrangement that follows, once.
    fn report_container_sizes(&mut self, window: WindowId) -> Result<(), Error> {
        let watched = self.with_application(|application| application.watched_nodes(window));
        if watched.is_empty() {
            return Ok(());
        }
        let Some(runtime) = self.windows.get(&window) else { return Ok(()) };
        let sizes: Vec<_> = watched
            .into_iter()
            .filter_map(|node| runtime.renderer.layout_size(node).map(|size| (node, size)))
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
                runtime.renderer.render(&tree, &theme, direction, &runtime.root, runtime.size)?;
            }
        }
        Ok(())
    }

    /// Brings the native windows in line with the application's set.
    pub(crate) fn sync(&mut self) -> Result<(), Error> {
        let desired = self.with_application(|application| application.window_ids());
        let closing: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(id, runtime)| !runtime.destroyed && !desired.contains(id))
            .map(|(id, _)| *id)
            .collect();
        for id in closing {
            self.destroy(id);
        }
        for id in desired {
            if !self.windows.contains_key(&id) {
                self.create_window(id)?;
            }
        }
        self.refresh_menus();
        self.apply_surfaces();
        Ok(())
    }

    /// Tears `id`'s window down.
    fn destroy(&mut self, id: WindowId) {
        let Some(runtime) = self.windows.get_mut(&id) else { return };
        if runtime.destroyed {
            return;
        }
        runtime.destroyed = true;
        runtime.renderer.release();
        runtime.input.release();
        runtime.animation.release();
        runtime.surfaces.release();
        let modal_parent = runtime.modal_parent;
        GTK_WINDOWS.with(|windows| windows.borrow_mut().remove(&id));
        let window = runtime.window.take();
        runtime.menu = None;
        if let Some(window) = window {
            if id == WindowId::PRIMARY {
                if let Some(store) = self
                    .with_application(|application| application.services().state_store().cloned())
                {
                    super::lifecycle::save_placement(&window, store.as_ref());
                }
            }
            window.destroy();
        }
        if let Some(parent) = modal_parent.and_then(|parent| self.windows.get(&parent)) {
            if let Some(window) = &parent.window {
                window.set_sensitive(true);
            }
        }
    }

    fn create_window(&mut self, id: WindowId) -> Result<(), Error> {
        let Some((title, size, modal_parent, menu_bar, placement)) =
            self.with_application(|application| {
                application.window_for(id).map(|definition| {
                    (
                        definition.title().to_owned(),
                        definition.size(),
                        application
                            .window_state(id)
                            .and_then(rustnative_core::WindowState::modal_parent),
                        definition.menu().cloned(),
                        // The primary window opens where it was last closed.
                        (id == WindowId::PRIMARY)
                            .then(|| application.services().state_store().cloned())
                            .flatten()
                            .and_then(|store| super::lifecycle::saved_placement(store.as_ref())),
                    )
                })
            })
        else {
            return Ok(());
        };
        let root = RnLayout::default();
        root.connect_resized(move |width, height| {
            // GTK is allocating: the tree must not change under it, so the
            // relayout runs right after, before the frame is painted.
            let size = Size::new(
                u32::try_from(width.max(0)).unwrap_or(0),
                u32::try_from(height.max(0)).unwrap_or(0),
            );
            post_later(Work::Resized(id, size));
        });
        let embedded = id == WindowId::PRIMARY && self.embedded;
        let mut menus = None;
        let window = if embedded {
            None
        } else {
            let window = gtk::Window::new();
            window.set_title(Some(&title));
            window.set_default_size(clamp_dimension(size.width), clamp_dimension(size.height));
            if let Some(placement) = placement {
                super::lifecycle::restore_placement(&window, placement);
            }
            if let Some(bar) = &menu_bar {
                // The bar above, the content filling the rest.
                let menu = super::menu::WindowMenu::build(bar, id, &window);
                let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
                content.append(menu.widget());
                root.set_vexpand(true);
                root.set_hexpand(true);
                content.append(&root);
                window.set_child(Some(&content));
                menus.replace(menu);
            } else {
                window.set_child(Some(&root));
            }
            window.connect_scale_factor_notify(move |_| post_later(Work::ScaleChanged(id)));
            window.connect_close_request(move |_| {
                post(Work::CloseRequested(id));
                // The application decides; `sync` closes the window.
                glib::Propagation::Stop
            });
            window.connect_destroy(move |_| post(Work::Destroyed(id)));
            if let Some(parent) = modal_parent
                .and_then(|parent| self.windows.get(&parent))
                .and_then(|runtime| runtime.window.clone())
            {
                window.set_transient_for(Some(&parent));
                window.set_modal(true);
                parent.set_sensitive(false);
            }
            Some(window)
        };
        let wake_pending = Arc::new(AtomicBool::new(false));
        // Input is attached before the first render, so a window is never
        // shown without it.
        let input = match &window {
            Some(window) => super::input::attach(window.upcast_ref(), &root, id),
            // An embedded root hears the input that reaches it in the host.
            None => super::input::attach(root.upcast_ref(), &root, id),
        };
        let runtime = WindowRuntime {
            id,
            window,
            root,
            renderer: Renderer::new(id, Rc::clone(&self.styles)),
            size,
            destroyed: false,
            modal_parent,
            wake_pending: Arc::clone(&wake_pending),
            input,
            animation: super::animation::AnimationState::default(),
            surfaces: super::surface::NativeSurfaces::default(),
            menu: menus,
            watched_lists: std::collections::HashSet::new(),
            overlay: None,
        };
        if let Some(window) = &runtime.window {
            GTK_WINDOWS.with(|windows| windows.borrow_mut().insert(id, window.clone()));
        }
        self.windows.insert(id, runtime);
        self.set_motion_preference(self.motion);
        // A task finishing on another thread wakes this window's loop.
        let context = glib::MainContext::default();
        let waker = Arc::new(move || {
            if !wake_pending.swap(true, Ordering::AcqRel) {
                context.invoke(move || post(Work::Pump(id)));
            }
        });
        self.with_application(|application| {
            if let Some(scheduler) = application.scheduler_for(id) {
                scheduler.set_waker(waker);
            }
        });
        self.render(id)?;
        if let Some(window) = self.windows.get(&id).and_then(|runtime| runtime.window.clone()) {
            if id == WindowId::PRIMARY {
                super::app::watch_startup(&window);
            }
            window.present();
        }
        Ok(())
    }

    /// Places a native surface where GTK allocated its host, and tells the
    /// component when its size or scale changed.
    fn surface_allocated(
        &mut self,
        window: WindowId,
        node: rustnative_core::NodeId,
    ) -> Result<(), Error> {
        let Some(runtime) = self.windows.get_mut(&window) else { return Ok(()) };
        let (Some(gtk_window), Some(host)) =
            (runtime.window.clone(), runtime.renderer.widget(node).cloned())
        else {
            return Ok(());
        };
        let Some(change) = runtime.surfaces.place(node, &host, &gtk_window) else { return Ok(()) };
        #[allow(clippy::cast_possible_truncation, reason = "a display scale, 1 to 4")]
        let scale_factor = rustnative_core::Scalar::new(change.scale as f32);
        self.dispatch(
            window,
            Event::SurfaceResized {
                target: node,
                surface: change.surface,
                size: change.size,
                scale_factor,
            },
        )
    }

    /// Releases every window (the run is over).
    pub(crate) fn release_all(&mut self) {
        self.lifecycle.release();
        self.tray = None;
        let ids: Vec<WindowId> = self.windows.keys().copied().collect();
        for id in ids {
            self.destroy(id);
        }
        self.styles.borrow().uninstall();
        if let Some(settings) = gtk::Settings::default() {
            for handler in self.settings_handlers.drain(..) {
                settings.disconnect(handler);
            }
        }
    }
}

/// A window's presentation, read from GTK.
fn presentation_of(window: &gtk::Window) -> WindowPresentation {
    if window.is_fullscreen() {
        WindowPresentation::Fullscreen
    } else if window.is_maximized() {
        WindowPresentation::Maximized
    } else if window
        .surface()
        .and_downcast::<gtk::gdk::Toplevel>()
        .is_some_and(|toplevel| toplevel.state().contains(gtk::gdk::ToplevelState::MINIMIZED))
    {
        WindowPresentation::Minimized
    } else {
        WindowPresentation::Normal
    }
}

fn clamp_dimension(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}
