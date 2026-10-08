//! Where Android starts the application.
//!
//! Android owns the main thread's loop, and an activity's `onCreate` must
//! return before the activity can start, so `Platform::run` cannot block
//! there as it does on a desktop. The application's portable `main` stays
//! as it is everywhere else; `export_main!` exports the `JNI_OnLoad` that
//! records it, and the launcher activity's creation runs it on the main
//! thread. Its `AndroidPlatform::run` adopts the application
//! (`Application::take`) and returns; the main looper drives it from then
//! on.

use std::cell::RefCell;
use std::sync::OnceLock;

use rustnative_core::WindowId;

use crate::backend::{Work, post};
use crate::jni_host::{Arg, Class, JavaRef, call, call_static};

static MAIN: OnceLock<Option<fn()>> = OnceLock::new();

/// The activity `main`'s `run` attaches the application to.
pub(crate) struct Pending {
    pub(crate) activity: JavaRef,
    pub(crate) root: JavaRef,
    pub(crate) intent: Option<JavaRef>,
}

thread_local! {
    static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
    static CONTEXT: RefCell<Option<JavaRef>> = const { RefCell::new(None) };
}

/// Records the application's `main` (from `JNI_OnLoad`).
pub(crate) fn set_main(main: Option<fn()>) {
    let _ = MAIN.set(main);
}

/// The process loaded the library (`RnBridge.nativeInit`, main thread).
pub(crate) fn init(context: JavaRef) {
    crate::looper::install();
    CONTEXT.with(|slot| *slot.borrow_mut() = Some(context.clone()));
    crate::services::set_context(context);
}

/// The application context, once the library is loaded.
#[allow(dead_code, reason = "read by the services and the library-only mode")]
pub(crate) fn application_context() -> Option<JavaRef> {
    CONTEXT.with(|slot| slot.borrow().clone())
}

/// The activity `main`'s `run` attaches to, taken by `run`.
pub(crate) fn take_pending() -> Option<Pending> {
    PENDING.with(|pending| pending.borrow_mut().take())
}

/// An activity was created (`RnBridge.nativeCreate`).
pub(crate) fn activity_created(
    activity: JavaRef,
    window_raw: u64,
    root: JavaRef,
    _restored: bool,
    intent: Option<JavaRef>,
) {
    crate::looper::install();
    let test = call_static(
        Class::Bridge,
        "isTestActivity",
        "(Landroid/app/Activity;)Z",
        &[Arg::Obj(&activity)],
    )
    .is_ok_and(crate::jni_host::Ret::bool);
    if test {
        #[cfg(feature = "device-tests")]
        crate::device_tests::activity_ready(activity, root);
        return;
    }
    if let Some(backend) = crate::backend::current() {
        if backend.is_finished() {
            // The application ended and the person opened it again in the
            // same process: a fresh start.
            backend.stop();
        } else {
            // A window's activity, or one the system recreated: attach it to
            // the running application.
            let window = if window_raw == 0 {
                Some(WindowId::PRIMARY)
            } else {
                crate::backend::window_id(window_raw)
            };
            let Some(window) = window else {
                let _ = call(&activity, "finish", "()V", &[]);
                return;
            };
            post(Work::Call(Box::new(move |registry| {
                if let Err(error) =
                    registry.attach_activity(window, activity, root, intent.as_ref())
                {
                    crate::log::error(&format!(
                        "attaching an activity to window {} failed: {error}",
                        window.get()
                    ));
                }
            })));
            return;
        }
    }
    PENDING.with(|pending| {
        *pending.borrow_mut() = Some(Pending { activity: activity.clone(), root, intent });
    });
    match MAIN.get().copied().flatten() {
        Some(main) => main(),
        None => crate::log::error(
            "this library exports no main (`export_main!` without one): nothing to show",
        ),
    }
    if take_pending().is_some() {
        // `main` returned without `run`: there is nothing to show.
        crate::log::error("the application's main returned without calling AndroidPlatform::run");
        let _ = call(&activity, "finish", "()V", &[]);
    }
}
