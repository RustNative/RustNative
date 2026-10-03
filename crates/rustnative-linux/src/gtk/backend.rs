//! The backend's one piece of mutable state, and the queue every GTK
//! callback goes through to reach it.
//!
//! # Why a queue
//!
//! GTK emits signals synchronously: setting an entry's text emits
//! `changed`, destroying a focused widget moves focus, presenting a window
//! can allocate it. A callback that borrowed the backend directly would, the
//! first time a render caused one of those, find the backend already
//! borrowed by that render — a `RefCell` panic, or with raw pointers the
//! aliasing bug the Windows backend's `native::context` exists to prevent.
//!
//! So no callback touches the state. Each one [`post`]s a [`Work`] item and
//! returns. If nothing is running, the item runs at once — the common case,
//! a click reaching its component with no delay. If something is running,
//! the item waits in the queue and runs as soon as the running work
//! finishes, still before GTK paints. This is the GTK form of a Win32
//! message queue, with the same guarantee: one piece of work at a time,
//! never nested.
//!
//! Framework-made changes to widgets do not echo back as events at all:
//! the handlers are blocked around them (`rendering::controls`).
//!
//! # Panics
//!
//! Application code (components, effects) only ever runs inside
//! [`Backend::run_work`], which catches a panic there — unwinding into
//! GTK's C frames would abort — and applies the application's
//! [`rustnative_core::PanicPolicy`].

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use gtk::glib;
use rustnative_core::{Application, Event, PanicAction, PanicReport, Size, WindowId};

use super::context::HostRef;
use super::registry::WindowRegistry;
use crate::Error;

/// One unit of work for the backend: what a GTK callback asks for.
pub(crate) enum Work {
    /// Deliver an event to a window's component tree.
    Event(WindowId, Event),
    /// Run a window's ready tasks and deferred work.
    Pump(WindowId),
    /// A window's content area changed size.
    Resized(WindowId, Size),
    /// The person asked to close a window.
    CloseRequested(WindowId),
    /// A window's native object is gone.
    Destroyed(WindowId),
    /// A desktop setting the environment follows changed.
    HostTraitsChanged,
    /// A key press or release that was not a shortcut.
    Key {
        window: WindowId,
        key: rustnative_core::KeyCode,
        modifiers: rustnative_core::KeyModifiers,
        pressed: bool,
    },
    /// Text an input method committed.
    Text { window: WindowId, text: String },
    /// An input-method composition step.
    Composition { window: WindowId, composition: rustnative_core::Composition },
    /// Keyboard focus moved within a window.
    FocusChanged(WindowId),
    /// A gesture recognizer's long-press deadline passed.
    LongPress(WindowId),
    /// The frame clock ticked while something animates.
    Frame(WindowId),
    /// A virtual list in a window scrolled.
    ListScrolled(WindowId),
    /// A window's display scale changed (it moved to a monitor of another
    /// scale, or the monitor's scale changed under it).
    ScaleChanged(WindowId),
    /// GTK allocated a native surface's host widget.
    SurfaceAllocated(WindowId, rustnative_core::NodeId),
    /// Run a closure against the backend's state (inspection, tests, and
    /// the services that need a window).
    Call(Box<dyn FnOnce(&mut WindowRegistry)>),
}

impl std::fmt::Debug for Work {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Event(window, event) => write!(f, "Event({window:?}, {event:?})"),
            Self::Pump(window) => write!(f, "Pump({window:?})"),
            Self::Resized(window, size) => write!(f, "Resized({window:?}, {size:?})"),
            Self::CloseRequested(window) => write!(f, "CloseRequested({window:?})"),
            Self::Destroyed(window) => write!(f, "Destroyed({window:?})"),
            Self::HostTraitsChanged => f.write_str("HostTraitsChanged"),
            Self::Key { window, key, pressed, .. } => {
                write!(f, "Key({window:?}, {key:?}, {pressed})")
            }
            Self::Text { window, .. } => write!(f, "Text({window:?})"),
            Self::Composition { window, .. } => write!(f, "Composition({window:?})"),
            Self::FocusChanged(window) => write!(f, "FocusChanged({window:?})"),
            Self::LongPress(window) => write!(f, "LongPress({window:?})"),
            Self::Frame(window) => write!(f, "Frame({window:?})"),
            Self::ListScrolled(window) => write!(f, "ListScrolled({window:?})"),
            Self::ScaleChanged(window) => write!(f, "ScaleChanged({window:?})"),
            Self::SurfaceAllocated(window, node) => {
                write!(f, "SurfaceAllocated({window:?}, {node:?})")
            }
            Self::Call(_) => f.write_str("Call(..)"),
        }
    }
}

/// The running backend.
pub(crate) struct Backend {
    /// Every window and its renderer. Borrowed only by [`Self::enter`].
    registry: RefCell<WindowRegistry>,
    queue: RefCell<VecDeque<Work>>,
    /// Whether work is running now — a `post` during it queues.
    busy: Cell<bool>,
    /// Whether an idle drain has been scheduled.
    drain_scheduled: Cell<bool>,
    /// Set when the application has no windows left, or failed.
    finished: Cell<bool>,
    /// The first failure, which `run` returns.
    error: RefCell<Option<Error>>,
    /// The main loop `run` is blocked in, if any (a harness has none).
    main_loop: RefCell<Option<glib::MainLoop>>,
}

thread_local! {
    /// The backend running on this (the GTK) thread, if any. Callbacks find
    /// it here rather than capturing it, so a widget never keeps the
    /// backend alive and a callback reached after `run` ends finds nothing.
    static CURRENT: RefCell<Option<Rc<Backend>>> = const { RefCell::new(None) };
}

/// Queues `work` for the running backend, running it now when nothing
/// else is running. A callback reached with no backend running (after
/// `run` returned) does nothing.
pub(crate) fn post(work: Work) {
    if let Some(backend) = current() {
        backend.queue.borrow_mut().push_back(work);
        backend.drain();
    }
}

/// Queues `work` to run from the main loop, never synchronously — for
/// callbacks GTK makes during its own layout or paint phase, when the
/// widget tree must not change under it.
pub(crate) fn post_later(work: Work) {
    if let Some(backend) = current() {
        backend.queue.borrow_mut().push_back(work);
        backend.schedule_drain();
    }
}

/// Runs `f` now, for a callback GTK needs an answer from before it returns
/// (whether a key was a shortcut, whether a drop is accepted). Queued work
/// runs first, so the answer reflects everything the person already did.
/// `None` when no backend is running, work is already running (the answer
/// cannot wait, so the caller takes its default), or `f` failed.
pub(crate) fn answer<R>(
    window: WindowId,
    f: impl FnOnce(&mut WindowRegistry) -> Result<R, Error>,
) -> Option<R> {
    let backend = current()?;
    if backend.busy.get() || backend.finished.get() {
        return None;
    }
    backend.drain();
    let value = backend.guarded(window, |registry| {
        let value = f(registry)?;
        Ok((registry.flow(), value))
    });
    backend.drain();
    value
}

/// The running backend, if any.
pub(crate) fn current() -> Option<Rc<Backend>> {
    CURRENT.with(|current| current.borrow().clone())
}

impl Backend {
    /// Creates the backend for `application` and makes it this thread's
    /// current one.
    ///
    /// # Safety
    ///
    /// `application` must outlive the backend's use of it: the caller calls
    /// [`Self::detach`] before the borrow ends (`run` and the test harness
    /// both do, on every path).
    pub(crate) unsafe fn attach(application: &mut Application, embedded: bool) -> Rc<Self> {
        // SAFETY: forwarded from this function's contract.
        let application = unsafe { HostRef::new(application) };
        let backend = Rc::new(Self {
            registry: RefCell::new(WindowRegistry::new(application, embedded)),
            queue: RefCell::new(VecDeque::new()),
            busy: Cell::new(false),
            drain_scheduled: Cell::new(false),
            finished: Cell::new(false),
            error: RefCell::new(None),
            main_loop: RefCell::new(None),
        });
        CURRENT.with(|current| *current.borrow_mut() = Some(Rc::clone(&backend)));
        backend
    }

    /// Stops being the current backend and releases every window. After
    /// this, no callback can reach the application.
    pub(crate) fn detach(&self) {
        CURRENT.with(|current| current.borrow_mut().take());
        self.queue.borrow_mut().clear();
        if let Ok(mut registry) = self.registry.try_borrow_mut() {
            registry.release_all();
        }
    }

    /// Runs the main loop until the application has no windows left.
    pub(crate) fn run_loop(&self) {
        if self.finished.get() {
            return;
        }
        let main_loop = glib::MainLoop::new(None, false);
        *self.main_loop.borrow_mut() = Some(main_loop.clone());
        main_loop.run();
        self.main_loop.borrow_mut().take();
    }

    /// Whether the application has finished (its last window closed, or a
    /// failure ended it).
    pub(crate) fn is_finished(&self) -> bool {
        self.finished.get()
    }

    /// The failure that ended the run, if one did.
    pub(crate) fn take_error(&self) -> Option<Error> {
        self.error.borrow_mut().take()
    }

    /// Ends the run as the last window closing would (the scripted startup
    /// of `RUSTNATIVE_EXIT_AT=interactive`).
    pub(crate) fn quit(&self) {
        self.finish(None);
    }

    fn finish(&self, error: Option<Error>) {
        if let Some(error) = error {
            self.error.borrow_mut().get_or_insert(error);
        }
        self.finished.set(true);
        if let Some(main_loop) = self.main_loop.borrow().as_ref() {
            main_loop.quit();
        }
    }

    /// Runs `f` against the state, as the one piece of work running now.
    ///
    /// # Panics
    ///
    /// Panics if work is already running: every entry point goes through
    /// [`post`], which never nests.
    pub(crate) fn enter<R>(&self, f: impl FnOnce(&mut WindowRegistry) -> R) -> R {
        assert!(!self.busy.get(), "backend work never nests (see `gtk::backend`)");
        self.busy.set(true);
        let result = {
            let mut registry = self.registry.borrow_mut();
            f(&mut registry)
        };
        self.busy.set(false);
        result
    }

    fn schedule_drain(&self) {
        if self.drain_scheduled.replace(true) {
            return;
        }
        // Above GTK's redraw priority, so queued work is realized in the
        // frame that follows the input that caused it.
        glib::idle_add_local_full(glib::Priority::HIGH_IDLE, || {
            if let Some(backend) = current() {
                backend.drain_scheduled.set(false);
                backend.drain();
            }
            glib::ControlFlow::Break
        });
    }

    /// Runs every queued item, unless work is already running (which will
    /// drain the queue itself when it finishes).
    pub(crate) fn drain(&self) {
        if self.busy.get() {
            return;
        }
        loop {
            let Some(work) = self.queue.borrow_mut().pop_front() else { break };
            if self.finished.get() {
                // Nothing runs after the application ended; the queue is
                // discarded with the backend.
                continue;
            }
            self.run_work(work);
        }
    }

    /// Runs one item inside the panic boundary.
    fn run_work(&self, work: Work) {
        let window = match &work {
            Work::Event(window, _)
            | Work::Pump(window)
            | Work::Resized(window, _)
            | Work::CloseRequested(window)
            | Work::Destroyed(window)
            | Work::Key { window, .. }
            | Work::Text { window, .. }
            | Work::Composition { window, .. }
            | Work::FocusChanged(window)
            | Work::LongPress(window)
            | Work::Frame(window)
            | Work::ListScrolled(window)
            | Work::ScaleChanged(window)
            | Work::SurfaceAllocated(window, _) => Some(*window),
            Work::HostTraitsChanged | Work::Call(_) => None,
        };
        let _ = self.guarded(window.unwrap_or(WindowId::PRIMARY), |registry| {
            registry.handle(work).map(|flow| (flow, ()))
        });
    }

    /// Runs `f` as one piece of work, inside the panic boundary, and applies
    /// the flow it returns. `None` when it failed or panicked.
    fn guarded<R>(
        &self,
        window: WindowId,
        f: impl FnOnce(&mut WindowRegistry) -> Result<(Flow, R), Error>,
    ) -> Option<R> {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.enter(f)));
        match outcome {
            Ok(Ok((flow, value))) => {
                if flow == Flow::Finished {
                    self.finish(None);
                }
                Some(value)
            }
            Ok(Err(error)) => {
                self.finish(Some(error));
                None
            }
            Err(payload) => {
                // The borrow was released by unwinding; the flag was not.
                self.busy.set(false);
                self.panicked(window, panic_message(payload.as_ref()));
                None
            }
        }
    }

    /// Applies the application's panic policy to a caught panic.
    fn panicked(&self, window: WindowId, message: String) {
        let report = PanicReport { message: message.clone(), window };
        let action = self.enter(|registry| {
            registry.with_application(|application| application.handle_component_panic(&report))
        });
        match action {
            PanicAction::Terminate => {
                super::teardown::restore();
                self.finish(Some(Error::ComponentPanicked { message }));
            }
            PanicAction::CloseWindow(target) => {
                let synced = self.enter(|registry| {
                    registry.with_application(|application| application.close_window(target));
                    registry.sync()
                });
                if let Err(error) = synced {
                    self.finish(Some(error));
                }
            }
            // `Continue`, and any action a later core adds: the panic was
            // caught, which is what had to happen.
            _ => {}
        }
    }
}

/// What a piece of work leaves the backend to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flow {
    /// Keep running.
    Continue,
    /// The application has no windows left: end the loop.
    Finished,
}

/// A caught panic's message, when it was a string.
pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a component panicked with a non-string payload".to_owned())
}
