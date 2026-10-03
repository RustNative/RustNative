//! `run` for the GTK toolkit: initializes GTK, brings up every window the
//! application wants open, and runs the main loop until the application
//! ends.

use gtk::prelude::*;
use rustnative_core::Application;

use super::backend::Backend;
use crate::Error;
use crate::desktop::Session;
use crate::desktop::single_instance::{Claim, claim, forward, launch_token};
use crate::toolkit::RunOptions;

/// Initializes GTK on this thread, answering a missing display server as
/// [`Error::NoDisplay`].
pub(crate) fn init() -> Result<(), Error> {
    gtk::init().map_err(|error| Error::NoDisplay { detail: error.to_string() })
}

pub(crate) fn run_application(
    application: &mut Application,
    options: &RunOptions,
    _session: &Session,
) -> Result<(), Error> {
    init()?;
    // The id names the application to the desktop: its Wayland app id and
    // X11 class (which match it to its desktop entry), and its one instance.
    let mut _instance = None;
    if let Some(app_id) = options.app_id.as_deref() {
        gtk::glib::set_prgname(Some(app_id));
        match claim(app_id, |activation| {
            if let Some(window) = super::registry::gtk_window(rustnative_core::WindowId::PRIMARY) {
                if let Some(token) = &activation.token {
                    window.set_startup_id(token);
                }
                window.present();
            }
            if let Some(url) = activation.url {
                super::backend::post(super::backend::Work::Event(
                    rustnative_core::WindowId::PRIMARY,
                    rustnative_core::Event::DeepLink { url },
                ));
            }
        }) {
            // Another instance runs: it gets this launch's link (or just
            // comes forward) and this one ends.
            Claim::AlreadyRunning => {
                let _ = forward(app_id, options.launch_url.as_deref(), launch_token().as_deref());
                return Ok(());
            }
            Claim::First(instance) => _instance = Some(instance),
            Claim::Unavailable => {}
        }
    }
    rustnative_core::dev::capture_panics();
    rustnative_core::perf::mark(rustnative_core::perf::StartupPhase::RuntimeReady);

    // `RUSTNATIVE_INSPECT=1` attaches the inspector (`PLAN.md` Milestone
    // 44); requests are answered when the primary window's loop is woken.
    let _ = application.enable_inspection_from_env();

    // SAFETY: `application` is borrowed for this whole function, and the
    // backend is detached (below) before the borrow is used again or ends.
    let backend = unsafe { Backend::attach(application, false) };
    // Host traits are in the environment before the first window renders,
    // so the first frame is already in the person's scheme, scale, and
    // direction.
    let started = backend.enter(|registry| {
        registry.app_id.clone_from(&options.app_id);
        registry.refresh_host_traits()?;
        registry.sync()
    });
    let mut failure = started.err();
    if failure.is_none() {
        rustnative_core::perf::mark(rustnative_core::perf::StartupPhase::FirstFrame);
        if let Some(url) = options.launch_url.clone() {
            super::backend::post(super::backend::Work::Call(Box::new(move |registry| {
                registry.with_application(|application| {
                    application.dispatch(rustnative_core::Event::DeepLink { url });
                });
            })));
        }
        backend.drain();
        backend.run_loop();
    }
    super::teardown::restore();
    if let Some(error) = backend.take_error() {
        failure.get_or_insert(error);
    }
    backend.detach();
    drop(backend);
    // Every window is gone: the application is terminating. State is
    // flushed whether the loop ended cleanly or not.
    let _ = application.lifecycle(rustnative_core::Lifecycle::Terminating);
    failure.map_or(Ok(()), Err)
}

/// Marks the startup model's "first content" when `window` (the primary
/// window) first paints, and "interactive" at the first idle after it —
/// nothing left to do, so the next input is answered at once. With
/// `RUSTNATIVE_EXIT_AT=interactive` the run then ends: the scripted startup
/// a profile-guided build and the startup budget run (`PLAN.md` Milestone
/// 42).
pub(crate) fn watch_startup(window: &gtk::Window) {
    use rustnative_core::perf::{self, StartupPhase};
    if perf::reached(StartupPhase::FirstContent) {
        return;
    }
    window.connect_map(|window| {
        let Some(clock) = window.frame_clock() else { return };
        clock.connect_after_paint(|_| {
            if !perf::mark(StartupPhase::FirstContent) {
                return;
            }
            // The lowest priority: runs once nothing else is waiting.
            gtk::glib::idle_add_local_full(gtk::glib::Priority::LOW, || {
                let exit =
                    std::env::var("RUSTNATIVE_EXIT_AT").is_ok_and(|value| value == "interactive");
                if perf::mark(StartupPhase::Interactive) && exit {
                    if let Some(backend) = super::backend::current() {
                        backend.quit();
                    }
                }
                gtk::glib::ControlFlow::Break
            });
        });
    });
}
