//! The backend's one piece of mutable state, and the queue every Java
//! callback goes through to reach it — the Android form of the Linux
//! backend's GTK queue and the Windows backend's message queue.
//!
//! # Why a queue
//!
//! Android calls listeners synchronously: setting an `EditText`'s text runs
//! its `TextWatcher`, removing a focused view moves focus, a layout pass
//! can resize the root. A callback that reached the backend directly would,
//! the first time a render caused one, find the backend already borrowed by
//! that render. So no callback touches the state: each one [`post`]s a
//! [`Work`] item. If nothing is running, the item runs at once — a tap
//! reaching its component with no delay. If something is running, it waits
//! and runs as soon as the running work finishes, before the frame is drawn.
//! One piece of work at a time, never nested.
//!
//! # Panics
//!
//! Application code runs only inside [`Backend::guarded`], which catches a
//! panic and applies the application's [`rustnative_core::PanicPolicy`].

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use rustnative_core::{Application, Event, PanicAction, PanicReport, WindowId};

use crate::Error;
use crate::input::{KeyFrame, PointerFrame};
use crate::jni_host::JavaRef;
use crate::registry::WindowRegistry;

/// One unit of work for the backend: what a Java callback asks for.
pub(crate) enum Work {
    /// Deliver an event to a window's component tree.
    Event(WindowId, Event),
    /// Run a window's ready tasks and deferred work.
    Pump(WindowId),
    /// A window's root is now this many pixels.
    Resized(WindowId, i32, i32),
    /// A window's insets changed.
    Insets(WindowId, [i32; 16]),
    /// An activity lifecycle step (`protocol::LC_*`).
    Lifecycle(WindowId, i32, i32),
    /// A view reported something.
    ViewEvent { window: WindowId, tag: i32, event: i32, a: i64, b: i64, text: Option<String> },
    /// A back gesture's phase.
    Back { window: WindowId, phase: i32, progress: f32, edge: i32 },
    /// An intent reached a running activity.
    Intent(WindowId, JavaRef),
    /// A display frame while something animates.
    Frame(WindowId, i64),
    /// The main thread is idle and idle work was asked for.
    Idle,
    /// A timer fired.
    Timer(i64),
    /// Run a closure against the backend's state.
    Call(Box<dyn FnOnce(&mut WindowRegistry)>),
}

impl std::fmt::Debug for Work {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Event(window, event) => write!(f, "Event({window:?}, {event:?})"),
            Self::Pump(window) => write!(f, "Pump({window:?})"),
            Self::Resized(window, width, height) => {
                write!(f, "Resized({window:?}, {width}x{height})")
            }
            Self::Insets(window, _) => write!(f, "Insets({window:?})"),
            Self::Lifecycle(window, what, argument) => {
                write!(f, "Lifecycle({window:?}, {what}, {argument})")
            }
            Self::ViewEvent { window, tag, event, .. } => {
                write!(f, "ViewEvent({window:?}, {tag}, {event})")
            }
            Self::Back { window, phase, .. } => write!(f, "Back({window:?}, {phase})"),
            Self::Intent(window, _) => write!(f, "Intent({window:?})"),
            Self::Frame(window, _) => write!(f, "Frame({window:?})"),
            Self::Idle => f.write_str("Idle"),
            Self::Timer(token) => write!(f, "Timer({token})"),
            Self::Call(_) => f.write_str("Call(..)"),
        }
    }
}

/// What a piece of work leaves the backend to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flow {
    /// Keep running.
    Continue,
    /// The application has ended (its primary activity finished).
    Finished,
}

/// The running backend.
pub(crate) struct Backend {
    registry: RefCell<WindowRegistry>,
    queue: RefCell<VecDeque<Work>>,
    busy: Cell<bool>,
    finished: Cell<bool>,
    error: RefCell<Option<Error>>,
}

thread_local! {
    /// The backend running on this (the main) thread, if any.
    static CURRENT: RefCell<Option<Rc<Backend>>> = const { RefCell::new(None) };
}

/// The running backend, if any.
pub(crate) fn current() -> Option<Rc<Backend>> {
    CURRENT.with(|current| current.borrow().clone())
}

/// Queues `work` for the running backend, running it now when nothing else
/// is running. With no backend running, it does nothing.
pub(crate) fn post(work: Work) {
    if let Some(backend) = current() {
        backend.queue.borrow_mut().push_back(work);
        backend.drain();
    }
}

/// Runs `f` now, for a callback Java needs an answer from before it
/// returns (whether a key or a touch was the framework's). Queued work runs
/// first. `None` when no backend runs, work is running, or `f` failed.
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

impl Backend {
    /// Creates the backend owning `application` and makes it this thread's
    /// current one.
    pub(crate) fn start(application: Application) -> Rc<Self> {
        let backend = Rc::new(Self {
            registry: RefCell::new(WindowRegistry::new(application)),
            queue: RefCell::new(VecDeque::new()),
            busy: Cell::new(false),
            finished: Cell::new(false),
            error: RefCell::new(None),
        });
        CURRENT.with(|current| *current.borrow_mut() = Some(Rc::clone(&backend)));
        backend
    }

    /// Stops being the current backend, releasing every view, and hands
    /// the application back.
    pub(crate) fn stop(&self) {
        CURRENT.with(|current| current.borrow_mut().take());
        self.queue.borrow_mut().clear();
        if let Ok(mut registry) = self.registry.try_borrow_mut() {
            registry.release_all();
        }
    }

    /// Whether the application has ended.
    pub(crate) fn is_finished(&self) -> bool {
        self.finished.get()
    }

    /// The failure that ended the application, if one did.
    pub(crate) fn take_error(&self) -> Option<Error> {
        self.error.borrow_mut().take()
    }

    fn finish(&self, error: Option<Error>) {
        if let Some(error) = &error {
            crate::log::error(&format!("the application ended: {error}"));
        }
        if let Some(error) = error {
            self.error.borrow_mut().get_or_insert(error);
        }
        if !self.finished.replace(true) {
            // The application is over: its activities go with it.
            if let Ok(registry) = self.registry.try_borrow() {
                registry.finish_activities();
            }
        }
    }

    /// Runs `f` against the state, as the one piece of work running now.
    ///
    /// # Panics
    ///
    /// Panics if work is already running: every entry goes through
    /// [`post`] or [`answer`], which never nest.
    pub(crate) fn enter<R>(&self, f: impl FnOnce(&mut WindowRegistry) -> R) -> R {
        assert!(!self.busy.get(), "backend work never nests (see `backend`)");
        self.busy.set(true);
        let result = {
            let mut registry = self.registry.borrow_mut();
            f(&mut registry)
        };
        self.busy.set(false);
        result
    }

    /// Runs every queued item, unless work is already running.
    pub(crate) fn drain(&self) {
        if self.busy.get() {
            return;
        }
        loop {
            let Some(work) = self.queue.borrow_mut().pop_front() else { break };
            if self.finished.get() {
                continue;
            }
            let window = match &work {
                Work::Event(window, _)
                | Work::Pump(window)
                | Work::Resized(window, ..)
                | Work::Insets(window, _)
                | Work::Lifecycle(window, ..)
                | Work::ViewEvent { window, .. }
                | Work::Back { window, .. }
                | Work::Intent(window, _)
                | Work::Frame(window, _) => *window,
                Work::Idle | Work::Timer(_) | Work::Call(_) => WindowId::PRIMARY,
            };
            let _ = self.guarded(window, |registry| registry.handle(work).map(|flow| (flow, ())));
        }
    }

    /// Runs `f` as one piece of work, inside the panic boundary, and applies
    /// the flow it returns. `None` when it failed or panicked.
    pub(crate) fn guarded<R>(
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
                self.enter(WindowRegistry::restore_host_state);
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
            _ => {}
        }
    }
}

/// A caught panic's message, when it was a string.
pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a component panicked with a non-string payload".to_owned())
}

/// A panic caught at the JNI boundary outside any work item (in the entry,
/// say): applied as the application's panic policy would apply it.
pub(crate) fn panicked(message: String) {
    if let Some(backend) = current() {
        if !backend.busy.get() {
            backend.panicked(WindowId::PRIMARY, message);
            return;
        }
        backend.finish(Some(Error::ComponentPanicked { message }));
    }
}

// ---- What the natives hand over (`jni_host::natives`). ----

/// The window Java names by `raw` (its `WindowId`'s number).
pub(crate) fn window_id(raw: u64) -> Option<WindowId> {
    let backend = current()?;
    let registry = backend.registry.try_borrow().ok()?;
    registry.window_for_raw(raw)
}

fn with_window(raw: u64, work: impl FnOnce(WindowId) -> Work) {
    if let Some(window) = window_id(raw) {
        post(work(window));
    }
}

pub(crate) fn lifecycle(window: u64, what: i32, argument: i32) {
    with_window(window, |window| Work::Lifecycle(window, what, argument));
}

pub(crate) fn resized(window: u64, width: i32, height: i32) {
    with_window(window, |window| Work::Resized(window, width, height));
}

pub(crate) fn insets(window: u64, insets: [i32; 16]) {
    with_window(window, |window| Work::Insets(window, insets));
}

pub(crate) fn view_event(window: u64, tag: i32, event: i32, a: i64, b: i64, text: Option<String>) {
    with_window(window, |window| Work::ViewEvent { window, tag, event, a, b, text });
}

pub(crate) fn key(window: u64, frame: KeyFrame) -> bool {
    let Some(window) = window_id(window) else { return false };
    answer(window, |registry| crate::input::key(registry, window, &frame)).unwrap_or(false)
}

pub(crate) fn pointer(window: u64, frame: &PointerFrame) -> bool {
    let Some(window) = window_id(window) else { return false };
    answer(window, |registry| crate::input::pointer(registry, window, frame)).unwrap_or(false)
}

pub(crate) fn gamepad(
    window: u64,
    device: i32,
    key_code: i32,
    action: i32,
    axes: Option<&[f32]>,
) -> bool {
    let Some(window) = window_id(window) else { return false };
    answer(window, |registry| {
        crate::input::gamepad_input(registry, window, device, key_code, action, axes)
    })
    .unwrap_or(false)
}

pub(crate) fn text(window: u64, tag: i32, step: i32, text: String, cursor: i32) {
    with_window(window, move |window| {
        Work::Call(Box::new(move |registry| {
            if let Err(error) = crate::input::text_input(registry, window, tag, step, text, cursor)
            {
                crate::log::error(&format!("an input-method step failed: {error}"));
            }
        }))
    });
}

pub(crate) fn back(window: u64, phase: i32, progress: f32, edge: i32) {
    with_window(window, |window| Work::Back { window, phase, progress, edge });
}

pub(crate) fn intent(window: u64, intent: &JavaRef) {
    let intent = intent.clone();
    with_window(window, move |window| Work::Intent(window, intent));
}

pub(crate) fn idle() {
    post(Work::Idle);
}

pub(crate) fn frame(window: u64, nanos: i64) {
    with_window(window, |window| Work::Frame(window, nanos));
}

pub(crate) fn timer(token: i64) {
    post(Work::Timer(token));
}
