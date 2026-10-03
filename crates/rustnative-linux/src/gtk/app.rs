//! `run` for the GTK toolkit: initializes GTK, brings up every window the
//! application wants open, and runs the main loop until the application
//! ends.

use rustnative_core::Application;

use super::backend::Backend;
use crate::Error;
use crate::desktop::Session;
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
    rustnative_core::dev::capture_panics();
    rustnative_core::perf::mark(rustnative_core::perf::StartupPhase::RuntimeReady);

    // SAFETY: `application` is borrowed for this whole function, and the
    // backend is detached (below) before the borrow is used again or ends.
    let backend = unsafe { Backend::attach(application, false) };
    // Host traits are in the environment before the first window renders,
    // so the first frame is already in the person's scheme, scale, and
    // direction.
    let started = backend.enter(|registry| {
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
