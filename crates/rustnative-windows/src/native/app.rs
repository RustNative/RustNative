//! The `Platform::run` entry point for the native Win32 backend: registers
//! window classes, brings up every window the `Application` currently wants
//! open, and runs the message loop until the process quits.

use rustnative_core::Application;

use super::message_loop::{register_window_classes, run_message_loop};
use super::runtime::WindowRegistry;
use super::util::module_instance;
use super::win32::best_effort;
use crate::Error;

/// OLE initialized on the UI thread for as long as this value lives.
///
/// Drag-and-drop registration (`RegisterDragDrop`) requires the calling
/// thread to have entered a single-threaded apartment through
/// `OleInitialize` — plain `CoInitializeEx` is not enough. It also makes the
/// COM objects the accessibility bridge creates on this thread available,
/// which previously degraded to a no-op outside of the dialog threads.
///
/// Initialization failing (most likely because the application already
/// entered a multithreaded apartment on this thread) is not fatal: windows
/// still work, they just do not accept drops.
pub(crate) struct OleApartment {
    initialized: bool,
}

impl OleApartment {
    pub(crate) fn enter() -> Self {
        // SAFETY: a null reserved pointer is the documented argument; the
        // call is made on the UI thread before any window exists.
        let initialized = unsafe { windows::Win32::System::Ole::OleInitialize(None) }.is_ok();
        best_effort(initialized, "OleInitialize", "windows on this thread do not accept drops");
        Self { initialized }
    }
}

impl Drop for OleApartment {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: pairs the successful `OleInitialize` in `enter`, on the
            // same thread (this type is `!Send` through its use sites: it is
            // created and dropped within one function or one harness).
            unsafe { windows::Win32::System::Ole::OleUninitialize() };
        }
    }
}

/// Tells the startup model when this process was created (`GetProcessTimes`),
/// so its phases are measured from the process's start rather than from
/// `run`.
fn process_start() {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let zero = || FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let (mut created, mut exited, mut kernel, mut user, mut now) =
        (zero(), zero(), zero(), zero(), zero());
    // SAFETY: the current process's pseudo-handle; five writable
    // `FILETIME`s.
    let read = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &raw mut created,
            &raw mut exited,
            &raw mut kernel,
            &raw mut user,
        )
    } != 0;
    // SAFETY: a writable `FILETIME`.
    unsafe { GetSystemTimePreciseAsFileTime(&raw mut now) };
    let ticks =
        |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    if read {
        // `FILETIME`s count 100-nanosecond intervals.
        let before = ticks(now).saturating_sub(ticks(created)).saturating_mul(100);
        rustnative_core::perf::set_process_start(std::time::Duration::from_nanos(before));
    }
}

pub(crate) fn run_application(
    application: &mut Application,
    app_id: Option<&str>,
    launch_url: Option<String>,
) -> Result<(), Error> {
    // A single-instance application hands its launch URL to the running
    // instance, if there is one, instead of starting a second copy.
    let _instance = match app_id.map(super::single_instance::claim) {
        Some(super::single_instance::Claim::AlreadyRunning) => {
            let handed_over = super::single_instance::forward(
                app_id.unwrap_or_default(),
                launch_url.as_deref().unwrap_or_default(),
            );
            super::win32::best_effort(handed_over, "forward(deep link)", "the link is dropped");
            return Ok(());
        }
        Some(super::single_instance::Claim::First(instance)) => Some(instance),
        None => None,
    };
    process_start();
    rustnative_core::dev::capture_panics();
    let instance = module_instance();
    register_window_classes(instance)?;
    rustnative_core::perf::mark(rustnative_core::perf::StartupPhase::RuntimeReady);
    // Declared before the registry so it is dropped after it: OLE must stay
    // initialized until every window has revoked its drop target.
    let _ole = OleApartment::enter();

    // Host traits are in the environment before the first window renders,
    // so the first frame is already in the person's scheme, scale, and
    // direction.
    super::host_traits::apply(application, &super::host_traits::read());
    // `RUSTNATIVE_INSPECT=1` attaches the inspector (`PLAN.md` Milestone
    // 44); requests are answered when the primary window's loop is woken.
    let _ = application.enable_inspection_from_env();

    // SAFETY: `application` is borrowed for the whole of this function,
    // and `registry` — along with every `Runtime` it creates, each of which
    // captures a copy of the same borrow — is dropped at the end of it,
    // after `run_message_loop` has returned. That is exactly point 1 of
    // `native::context`'s module documentation, which `WindowRegistry::new`
    // requires its caller to establish.
    //
    // The `Box` is load-bearing, not a convenience. Each `Runtime` the
    // registry creates holds a `HostRef<WindowRegistry>` pointing at the
    // registry's address (so it can reconcile the window set after a
    // dispatch), and those references are captured during `sync`. Behind a
    // `Box`, the registry's address is fixed at allocation and stays valid
    // however the owning handle is later moved — the invariant becomes
    // structural rather than a fact about this one function's body.
    let mut registry = Box::new(unsafe { WindowRegistry::new(application) });
    registry.sync()?;
    // The first window exists, its tree realized.
    rustnative_core::perf::mark(rustnative_core::perf::StartupPhase::FirstFrame);
    if let Some(url) = launch_url {
        super::single_instance::deliver_later(url);
    }
    // Stopped (and its thread joined) when this function returns, after the
    // loop has ended and before the registry is dropped.
    let _memory = super::memory_watch::MemoryWatcher::start();

    let looped = run_message_loop();
    super::teardown::restore(&super::teardown::policy());

    let mut failure = looped.err();
    for runtime in registry.runtimes.drain().map(|(_, runtime)| runtime) {
        if let Some(error) = runtime.error {
            failure.get_or_insert(error);
        }
    }
    drop(registry);
    // Every window is gone: the application is terminating. State is
    // flushed (and components told) whether the loop ended cleanly or not,
    // so a failure elsewhere does not also lose what the person was doing.
    let flushed = application.lifecycle(rustnative_core::Lifecycle::Terminating);
    super::win32::best_effort(flushed.is_ok(), "flush_state(exit)", "unsaved state is lost");
    failure.map_or(Ok(()), Err)
}
