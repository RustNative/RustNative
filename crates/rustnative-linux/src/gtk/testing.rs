//! Driving the real GTK backend from tests.
//!
//! # One GTK thread
//!
//! GTK allows one main thread per process, and Rust's test harness runs
//! each test on a thread of its own. [`on_gtk`] therefore sends each test's
//! body to one dedicated thread that initialized GTK, runs it there, and
//! hands the result (or the panic) back. Tests are serialized on that
//! thread, which also serializes what is process-global in GTK — the
//! default display, its style providers, the focus.
//!
//! # The harness
//!
//! [`Harness`] replaces only the outermost blocking loop: it attaches the
//! same `Backend` `run` does, brings up the application's windows through
//! the same `WindowRegistry::sync`, and iterates GTK's main context in
//! bounded steps. Every signal, every queued work item, every render is the
//! production path.

use std::any::Any;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use gtk::glib;
use gtk::prelude::*;
use rustnative_core::{Application, NodeId, WindowId};

use super::backend::Backend;
use super::registry::WindowRegistry;

type Job = Box<dyn FnOnce() + Send>;

fn gtk_thread() -> &'static Mutex<mpsc::Sender<Job>> {
    static SENDER: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
    SENDER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("gtk".to_owned())
            .spawn(move || {
                // A display server that is still starting (WSLg after a cold
                // start) refuses the first connections; give it a moment.
                let started = Instant::now();
                while let Err(error) = super::app::init() {
                    assert!(
                        started.elapsed() < Duration::from_secs(10),
                        "GTK must initialize for the Linux backend's tests: run them under \
                         tools/linux-session.sh, which provides a display server ({error})"
                    );
                    std::thread::sleep(Duration::from_millis(200));
                }
                for job in receiver {
                    job();
                }
            })
            .expect("spawning the GTK thread");
        Mutex::new(sender)
    })
}

/// Runs `body` on the process's GTK thread and returns what it returned,
/// re-raising a panic on the calling thread.
pub(crate) fn on_gtk<R: Send + 'static>(body: impl FnOnce() -> R + Send + 'static) -> R {
    let (reply, result) = mpsc::channel::<Result<R, Box<dyn Any + Send>>>();
    let job: Job = Box::new(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
        let _ = reply.send(outcome);
    });
    gtk_thread()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .send(job)
        .expect("the GTK thread is running");
    match result.recv().expect("the GTK thread answers every job") {
        Ok(value) => value,
        Err(panic) => {
            // `resume_unwind` does not run the panic hook, so the message
            // would otherwise be lost with the GTK thread's output.
            eprintln!(
                "panicked on the GTK thread: {}",
                super::backend::panic_message(panic.as_ref())
            );
            std::panic::resume_unwind(panic)
        }
    }
}

/// Drives an [`Application`] through the real GTK backend.
pub(crate) struct Harness {
    backend: Rc<Backend>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.backend.detach();
        // Let GTK finish destroying what was released.
        pump_for(Duration::from_millis(20));
    }
}

/// Iterates the default main context until nothing is pending (bounded).
pub(crate) fn pump_pending() {
    let context = glib::MainContext::default();
    for _ in 0..10_000 {
        if !context.iteration(false) {
            return;
        }
    }
    panic!("the main context did not settle within 10 000 iterations");
}

/// Iterates the main context for `duration` — for what needs the frame
/// clock (allocation happens on a frame, and frames are paced by the
/// compositor).
pub(crate) fn pump_for(duration: Duration) {
    let context = glib::MainContext::default();
    let until = Instant::now() + duration;
    while Instant::now() < until {
        if !context.iteration(false) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// Iterates the main context until `done` answers yes, or panics naming
/// `what` after `timeout`.
pub(crate) fn pump_until(what: &str, timeout: Duration, mut done: impl FnMut() -> bool) {
    let context = glib::MainContext::default();
    let until = Instant::now() + timeout;
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        if !context.iteration(false) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Harness {
    /// Attaches to `application`, creates every window it wants open, and
    /// waits until the primary window has been allocated.
    ///
    /// # Safety
    ///
    /// `application` must outlive the harness: declare it before the
    /// harness in the same scope.
    pub(crate) unsafe fn attach(application: &mut Application) -> Self {
        // SAFETY: forwarded from this function's contract; `Drop` detaches.
        let backend = unsafe { Backend::attach(application, false) };
        backend
            .enter(|registry| {
                registry.refresh_host_traits()?;
                registry.sync()
            })
            .expect("bringing up the application's windows");
        backend.drain();
        let harness = Self { backend };
        harness.settle();
        harness
    }

    /// Waits until every window is allocated at the size its tree is laid
    /// out at, and nothing is pending.
    pub(crate) fn settle(&self) {
        // Generous: a window manager can take seconds to map a window under
        // load (WSLg's Xwayland, a CI runner).
        pump_until("the windows to be allocated", Duration::from_secs(20), || {
            self.with_registry(|registry| {
                registry.windows.values().filter(|runtime| !runtime.destroyed).all(|runtime| {
                    let (width, height) = runtime.root.allocated_size();
                    width > 0 && height > 0
                })
            })
        });
        pump_for(Duration::from_millis(30));
        pump_pending();
    }

    /// Runs `f` against the backend's state.
    pub(crate) fn with_registry<R>(&self, f: impl FnOnce(&mut WindowRegistry) -> R) -> R {
        self.backend.enter(f)
    }

    /// Lets queued work and GTK's own idle work run.
    pub(crate) fn pump(&self) {
        self.backend.drain();
        pump_pending();
        self.backend.drain();
    }

    /// The widget realizing `key` in `window` — by the node's own key, or by
    /// its local key inside a child component.
    pub(crate) fn widget(&self, window: WindowId, key: &str) -> Option<gtk::Widget> {
        self.with_registry(|registry| {
            let runtime = registry.windows.get(&window)?;
            let renderer = &runtime.renderer;
            renderer.widget(NodeId::from_key(key)).cloned().or_else(|| {
                renderer
                    .snapshot
                    .nodes()
                    .find(|node| node.id.local_key().as_deref() == Some(key))
                    .and_then(|node| renderer.widget(node.id).cloned())
            })
        })
    }

    /// [`Self::widget`], panicking when absent.
    pub(crate) fn expect(&self, key: &str) -> gtk::Widget {
        self.widget(WindowId::PRIMARY, key)
            .unwrap_or_else(|| panic!("no widget realizes node {key:?}"))
    }

    /// [`Self::expect`], as the widget type realizing it.
    pub(crate) fn expect_as<W: IsA<gtk::Widget>>(&self, key: &str) -> W {
        let widget = self.expect(key);
        let type_name = widget.type_().name();
        widget.downcast::<W>().unwrap_or_else(|_| panic!("node {key:?} is a {type_name}"))
    }

    /// Clicks the button realizing `key`, as a person would activate it.
    pub(crate) fn click(&self, key: &str) {
        let button = self.expect_as::<gtk::Button>(key);
        button.emit_clicked();
        self.pump();
    }

    /// The window's `GtkWindow`.
    pub(crate) fn gtk_window(&self, window: WindowId) -> Option<gtk::Window> {
        self.with_registry(|registry| {
            registry.windows.get(&window).and_then(|runtime| runtime.window.clone())
        })
    }

    /// Whether the backend finished (the application ended).
    pub(crate) fn finished(&self) -> bool {
        self.backend.is_finished()
    }

    /// The failure that ended the run, if any.
    pub(crate) fn take_error(&self) -> Option<crate::Error> {
        self.backend.take_error()
    }

    /// Window ids the backend has not torn down.
    pub(crate) fn live_windows(&self) -> Vec<WindowId> {
        let mut ids: Vec<WindowId> = self.with_registry(|registry| {
            registry
                .windows
                .iter()
                .filter(|(_, runtime)| !runtime.destroyed)
                .map(|(id, _)| *id)
                .collect()
        });
        ids.sort_unstable_by_key(|id| id.get());
        ids
    }
}
