//! The device suite's harness: an application realized in the test
//! activity, driven the way the GTK harness drives one on Linux.
//!
//! A test's body runs on the main thread ([`on_main`]); input injected
//! through the instrumentation (`Instrumentation.sendPointerSync`) runs on
//! the instrumentation thread, which is where the suite itself runs.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use rustnative_core::{Application, NodeId, WindowId};

use crate::backend::Backend;
use crate::jni_host::{Arg, Class, JavaRef, Ret, call, call_static};
use crate::registry::WindowRegistry;

thread_local! {
    /// The test activity and its root, once it exists (main thread).
    static ACTIVITY: RefCell<Option<(JavaRef, JavaRef)>> = const { RefCell::new(None) };
    /// A harness a test keeps across several trips to the main thread
    /// (main thread).
    static KEPT: RefCell<Option<Harness>> = const { RefCell::new(None) };
    /// The harness running now, if any (main thread).
    static RUNNING: RefCell<Option<std::rc::Rc<Backend>>> = const { RefCell::new(None) };
}

/// The test activity was created.
pub(crate) fn activity_ready(activity: JavaRef, root: JavaRef) {
    ACTIVITY.with(|slot| *slot.borrow_mut() = Some((activity, root)));
}

/// Keeps `harness` for later trips to the main thread ([`with_kept`]), so a
/// test can let the main looper run (a configuration change, an activity
/// result) between steps.
pub(crate) fn keep(harness: Harness) {
    KEPT.with(|kept| *kept.borrow_mut() = Some(harness));
}

/// Runs `f` with the kept harness (main thread).
///
/// # Panics
///
/// When no harness is kept.
pub(crate) fn with_kept<R>(f: impl FnOnce(&Harness) -> R) -> R {
    KEPT.with(|kept| f(kept.borrow().as_ref().expect("a harness is kept")))
}

/// Stops whatever harness a test left running (main thread).
pub(crate) fn stop_running() {
    drop(KEPT.with(|kept| kept.borrow_mut().take()));
    if let Some(previous) = RUNNING.with(|running| running.borrow_mut().take()) {
        previous.stop();
    }
}

/// Runs `task` on the main thread and returns what it returned.
pub(crate) fn on_main<R: Send + 'static>(task: impl FnOnce() -> R + Send + 'static) -> R {
    crate::looper::on_main(task)
}

/// An application realized in the test activity, for one test.
pub(crate) struct Harness {
    backend: std::rc::Rc<Backend>,
}

impl Harness {
    /// Realizes `application` in the test activity (main thread).
    ///
    /// # Panics
    ///
    /// When there is no test activity, or attaching fails.
    pub(crate) fn launch(application: Application) -> Self {
        if let Some(previous) = RUNNING.with(|running| running.borrow_mut().take()) {
            previous.stop();
        }
        let (activity, root) =
            ACTIVITY.with(|slot| slot.borrow().clone()).expect("the test activity exists");
        let backend = Backend::start(application);
        let attached = backend.guarded(WindowId::PRIMARY, |registry| {
            registry.attach_activity(WindowId::PRIMARY, activity, root, None)?;
            Ok((registry.flow(), ()))
        });
        if let Some(error) = backend.take_error() {
            panic!("attaching the application failed: {error}");
        }
        assert!(attached.is_some(), "attaching the application failed");
        RUNNING.with(|running| *running.borrow_mut() = Some(std::rc::Rc::clone(&backend)));
        let harness = Self { backend };
        harness.settle();
        harness
    }

    /// Runs whatever is queued: the backend's work, and the looper's
    /// cross-thread tasks (a woken scheduler).
    pub(crate) fn pump(&self) {
        crate::looper::run_pending();
        self.backend.drain();
    }

    /// Lays the views out now, so their geometry can be read.
    pub(crate) fn settle(&self) {
        self.pump();
        if let Some(root) = self.root() {
            let _ = call_static(
                Class::Views,
                "layoutNow",
                "(Landroid/view/View;)V",
                &[Arg::Obj(&root)],
            );
        }
    }

    /// Pumps until `done` holds or `timeout` passes.
    ///
    /// # Panics
    ///
    /// When `timeout` passes first.
    pub(crate) fn pump_until(
        &self,
        what: &str,
        timeout: Duration,
        mut done: impl FnMut(&Self) -> bool,
    ) {
        let until = Instant::now() + timeout;
        loop {
            self.pump();
            if done(self) {
                return;
            }
            assert!(Instant::now() < until, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Whether the application has ended, and the failure that ended it.
    pub(crate) fn ended(&self) -> (bool, Option<String>) {
        (self.backend.is_finished(), self.backend.take_error().map(|error| error.to_string()))
    }

    /// Runs `f` against the backend's state.
    pub(crate) fn with_registry<R>(&self, f: impl FnOnce(&mut WindowRegistry) -> R) -> R {
        self.backend.enter(f)
    }

    /// Runs `f` against the application.
    pub(crate) fn with_application<R>(&self, f: impl FnOnce(&mut Application) -> R) -> R {
        self.with_registry(|registry| registry.with_application(f))
    }

    /// The window's root layout.
    pub(crate) fn root(&self) -> Option<JavaRef> {
        self.with_registry(|registry| {
            registry.windows.get(&WindowId::PRIMARY).and_then(|runtime| runtime.root.clone())
        })
    }

    /// The view realizing the node keyed `key`.
    pub(crate) fn view(&self, key: &str) -> Option<JavaRef> {
        let id = self.node(key)?;
        self.with_registry(|registry| {
            registry
                .windows
                .get(&WindowId::PRIMARY)
                .and_then(|runtime| runtime.renderer.as_ref())
                .and_then(|renderer| renderer.view(id).cloned())
        })
    }

    /// The view realizing `key`.
    ///
    /// # Panics
    ///
    /// When nothing realizes it.
    pub(crate) fn expect(&self, key: &str) -> JavaRef {
        self.view(key).unwrap_or_else(|| panic!("no view realizes `{key}`"))
    }

    /// The node keyed `key` in the realized tree (a key is local to its
    /// component; the first match is taken).
    pub(crate) fn node(&self, key: &str) -> Option<NodeId> {
        self.with_registry(|registry| {
            let renderer = registry.windows.get(&WindowId::PRIMARY)?.renderer.as_ref()?;
            renderer
                .snapshot
                .nodes()
                .find(|node| node.id.local_key().as_deref() == Some(key))
                .map(|node| node.id)
        })
    }

    /// Clicks `key`'s view the way a tap does (`View.performClick`).
    pub(crate) fn click(&self, key: &str) {
        let view = self.expect(key);
        let _ = call(&view, "performClick", "()Z", &[]);
        self.pump();
    }

    /// The Java class of `key`'s view.
    pub(crate) fn class_of(&self, key: &str) -> String {
        crate::rendering::controls::class_name(&self.expect(key))
    }

    /// `key`'s view's frame in its parent, in pixels.
    pub(crate) fn frame(&self, key: &str) -> [i32; 4] {
        self.settle();
        crate::rendering::controls::frame(&self.expect(key))
    }

    /// `key`'s view's text.
    pub(crate) fn text(&self, key: &str) -> Option<String> {
        crate::rendering::controls::text(&self.expect(key))
    }

    /// The display density.
    pub(crate) fn density(&self) -> f32 {
        self.with_registry(|registry| {
            registry.windows.get(&WindowId::PRIMARY).map_or(1.0, |runtime| runtime.density)
        })
    }

    /// Where the layout engine placed `key` (physical dp), if it did.
    pub(crate) fn placed(&self, key: &str) -> Option<rustnative_core::Rect> {
        let id = self.node(key)?;
        self.with_registry(|registry| {
            registry.windows.get(&WindowId::PRIMARY)?.renderer.as_ref()?.placed.get(&id).copied()
        })
    }

    /// How many views the window realizes.
    pub(crate) fn realized(&self) -> usize {
        self.with_registry(|registry| {
            registry
                .windows
                .get(&WindowId::PRIMARY)
                .and_then(|runtime| runtime.renderer.as_ref())
                .map_or(0, |renderer| renderer.registry.len())
        })
    }

    /// Created and destroyed host objects so far.
    pub(crate) fn census(&self) -> (u64, u64) {
        self.with_registry(|registry| {
            registry
                .windows
                .get(&WindowId::PRIMARY)
                .and_then(|runtime| runtime.renderer.as_ref())
                .map_or((0, 0), |renderer| (renderer.registry.created, renderer.registry.destroyed))
        })
    }
}

/// Calls a static method of a host-library class (a test's probe).
///
/// # Panics
///
/// When the call fails.
pub(crate) fn java(class: Class, name: &str, signature: &str, args: &[Arg<'_>]) -> Ret {
    call_static(class, name, signature, args).unwrap_or_else(|error| panic!("{name}: {error}"))
}

impl Drop for Harness {
    fn drop(&mut self) {
        RUNNING.with(|running| running.borrow_mut().take());
        self.backend.stop();
    }
}

/// The instrumentation, for injecting input (instrumentation thread).
pub(crate) struct Instrumentation(pub(crate) JavaRef);

impl Instrumentation {
    /// Runs `command` as the shell user (`UiAutomation.executeShellCommand`)
    /// and returns its output.
    pub(crate) fn shell(&self, command: &str) -> String {
        call(&self.0, "shell", "(Ljava/lang/String;)Ljava/lang/String;", &[Arg::Str(command)])
            .ok()
            .and_then(Ret::string)
            .unwrap_or_default()
    }

    /// Waits until `done` holds on the main thread, letting the main looper
    /// run in between.
    ///
    /// # Panics
    ///
    /// When `timeout` passes first.
    pub(crate) fn wait_for(
        &self,
        what: &str,
        timeout: Duration,
        done: impl Fn() -> bool + Send + Sync + Clone + 'static,
    ) {
        let until = Instant::now() + timeout;
        loop {
            self.wait_idle();
            let check = done.clone();
            if on_main(move || {
                crate::looper::run_pending();
                check()
            }) {
                return;
            }
            assert!(Instant::now() < until, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The active window's accessibility tree (`RnInstrumentation.dumpAccessibility`).
    pub(crate) fn dump_accessibility(&self) -> String {
        call(&self.0, "dumpAccessibility", "()Ljava/lang/String;", &[])
            .ok()
            .and_then(Ret::string)
            .unwrap_or_default()
    }

    /// Performs accessibility `action` on the node found by `key` (view id
    /// or content description), as a screen reader does.
    pub(crate) fn perform_accessibility_action(&self, key: &str, action: i32, value: f32) -> bool {
        call(
            &self.0,
            "performAccessibilityAction",
            "(Ljava/lang/String;IF)Z",
            &[Arg::Str(key), Arg::Int(action), Arg::Float(value)],
        )
        .is_ok_and(Ret::bool)
    }

    /// Brings the test activity back in front (a screen reader's tutorial
    /// can cover it when the screen reader starts).
    pub(crate) fn bring_to_front(&self) {
        let _ = call(&self.0, "bringToFront", "()V", &[]);
    }

    /// Whether a screen reader is exploring by touch.
    pub(crate) fn touch_exploration(&self) -> bool {
        call(&self.0, "touchExploration", "()Z", &[]).is_ok_and(Ret::bool)
    }

    /// Waits until the main thread is idle.
    pub(crate) fn wait_idle(&self) {
        let _ = call(&self.0, "waitForIdleSync", "()V", &[]);
    }
}
