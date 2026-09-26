//! A bounded driver for the real Win32 message loop, so the native backend
//! can be tested against actual windows.
//!
//! # Why this exists
//!
//! The standards audit's P1.17 finding is that the most failure-prone code
//! in this crate had no test at all:
//!
//! > The most failure-prone code is: HWND creation; synchronous Win32
//! > callbacks; `GWLP_USERDATA`; reentrancy; menus; modal windows;
//! > destruction ordering; GDI lifetime; COM; clipboard ownership; file
//! > dialogs; scheduler wakeups.
//!
//! and that the fix is real execution rather than a proxy for it:
//!
//! > Where GUI CI is difficult, use a dedicated Windows runner with
//! > deterministic scripted interactions rather than pretending
//! > compile-time tests cover native behavior.
//!
//! The obstacle was never the assertions — it was that `run_application`
//! blocks in `GetMessageW` until `WM_QUIT`, which a test cannot drive.
//! [`NativeHarness`] replaces only that outermost blocking loop with a
//! bounded `PeekMessageW` pump. Everything inside it — window creation,
//! every `WNDPROC`, reconciliation, layout, focus, task pumping, the panic
//! boundary — is the production code path, reached through the same
//! [`message_loop::handle_message`] the real loop calls. A harness with its
//! own dispatch would test the harness.
//!
//! # What these tests are and are not
//!
//! They create genuine top-level `WS_OVERLAPPEDWINDOW` windows and show
//! them, because that is the thing under test: message-only windows (which
//! `test_support` provides for the narrower unit tests) never receive
//! `WM_SIZE`, never participate in focus, and never own a menu. The
//! consequence is that these tests need an interactive window station —
//! true on a developer's machine and on GitHub's `windows-latest` runner,
//! not true under a service account.
//!
//! # Serialization
//!
//! Rust's test harness runs tests concurrently in one process, and three
//! things here are process-global: the window-class registry, the
//! `WindowId -> HWND` table in `native::window_handles` (which several
//! tests would key with `WindowId::PRIMARY` at once), and the GDI/USER
//! handle quota that the resource-lifecycle test measures against. Every
//! harness therefore takes a process-wide lock for its lifetime, making
//! these tests sequential with respect to each other while leaving the rest
//! of the suite parallel.

use std::sync::{Mutex, MutexGuard, OnceLock};

use rustnative_core::{Application, NodeId, WindowId};
use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IsWindow, MSG, PM_REMOVE, PeekMessageW, SendMessageW, WM_CLOSE, WM_COMMAND, WM_KEYDOWN,
};

use super::message_loop::{LoopStep, handle_message, register_window_classes};
use super::registry::NativeObject;
use super::runtime::{Runtime, WindowRegistry};
use super::util::module_instance;
use crate::Error;

/// How many messages one [`NativeHarness::pump`] will process before giving
/// up.
///
/// A bound rather than "until the queue is empty" because some of what
/// these tests exercise is *self-sustaining*: a window that recreates
/// itself, or a scheduler wake that queues another wake, would spin
/// forever. Hitting the bound fails the test with a clear message instead
/// of hanging CI. It is generous — a full window creation with a populated
/// tree is a few dozen messages — so no legitimate scenario approaches it.
const MAX_MESSAGES_PER_PUMP: usize = 4_000;

/// The process-wide lock described in the module documentation.
fn harness_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Holds the harness lock without a harness — for a test that measures a
/// process-wide quantity (the USER-object count) that any concurrently
/// running window test would disturb.
pub(crate) fn exclusive() -> MutexGuard<'static, ()> {
    harness_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Drives an [`Application`] through the real Win32 backend for a bounded
/// number of messages.
pub(crate) struct NativeHarness {
    /// Boxed so the registry's address is fixed for its whole life: every
    /// `Runtime` it creates holds a `HostRef<WindowRegistry>` back to it
    /// (see `native::app::run_application`, which boxes it for the same
    /// reason), and moving this harness must not invalidate those.
    registry: Box<WindowRegistry>,
    /// Set once `WM_QUIT` has been seen, so a test can assert that the
    /// backend asked to exit (the panic boundary's observable effect).
    quit: bool,
    /// Declared after `registry` so it is dropped after it, as in
    /// `run_application`.
    _ole: super::app::OleApartment,
    _lock: MutexGuard<'static, ()>,
}

impl Drop for NativeHarness {
    /// Destroys the windows this harness created and discards what their
    /// teardown queued (the primary window's `WM_QUIT` among it), so a
    /// second harness on the same thread starts clean — without this, a
    /// window would outlive the `Runtime` its `GWLP_USERDATA` points at.
    fn drop(&mut self) {
        let windows: Vec<HWND> = self
            .registry
            .runtimes
            .values()
            .filter(|runtime| !runtime.destroyed)
            .map(|runtime| runtime.window)
            .collect();
        for hwnd in windows {
            // SAFETY: `hwnd` is a window this harness's registry created and
            // still owns; `IsWindow` accepts any handle value.
            unsafe {
                if IsWindow(hwnd) != 0 {
                    windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
                }
            }
        }
        let mut message = MSG::default();
        // SAFETY: `message` is writable; removing without dispatching.
        while unsafe { PeekMessageW(&raw mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0
        {
        }
    }
}

impl NativeHarness {
    /// Attaches to `application`, creating every window it currently wants
    /// open, and pumps until the UI has settled.
    ///
    /// # Safety
    ///
    /// `application` must outlive the returned harness. In practice this
    /// means declaring it as a local *before* the harness in the same test
    /// function, so drop order guarantees it — the same obligation
    /// `WindowRegistry::new` documents and `run_application` discharges by
    /// owning both in one stack frame.
    ///
    /// # Panics
    ///
    /// Panics if window-class registration or initial window creation
    /// fails, both of which are test-environment failures rather than
    /// conditions under test.
    pub(crate) unsafe fn attach(application: &mut Application) -> Self {
        let lock = harness_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        register_window_classes(module_instance())
            .expect("registering this backend's window classes must succeed");

        let ole = super::app::OleApartment::enter();
        // SAFETY: forwarded from this function's own contract above.
        let registry = Box::new(unsafe { WindowRegistry::new(application) });
        let mut harness = Self { registry, quit: false, _ole: ole, _lock: lock };
        harness.registry.sync().expect("bringing up the application's initial windows");
        harness.pump();
        harness
    }

    /// Processes every message currently queued for this thread, then
    /// returns.
    ///
    /// # Panics
    ///
    /// Panics if the queue does not drain within [`MAX_MESSAGES_PER_PUMP`]
    /// messages, or if the backend reports a native error.
    pub(crate) fn pump(&mut self) {
        self.try_pump().expect("pumping the native message loop must not fail");
    }

    /// Pumps like [`Self::pump`], returning how many messages it handled —
    /// the probe for "no wakes while idle" (`PLAN.md` Milestone 54).
    pub(crate) fn pump_counting(&mut self) -> Vec<u32> {
        let mut seen = Vec::new();
        let mut message = MSG::default();
        for _ in 0..MAX_MESSAGES_PER_PUMP {
            // SAFETY: as in `try_pump`.
            let available =
                unsafe { PeekMessageW(&raw mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) }
                    != 0;
            if !available {
                break;
            }
            seen.push(message.message);
            if handle_message(&message).expect("pumping must not fail") == LoopStep::Quit {
                self.quit = true;
                break;
            }
        }
        seen
    }

    /// [`Self::pump`], surfacing a backend error instead of panicking on
    /// it — used by the tests that deliberately drive a failure path.
    pub(crate) fn try_pump(&mut self) -> Result<(), Error> {
        let mut message = MSG::default();
        for _ in 0..MAX_MESSAGES_PER_PUMP {
            // SAFETY: `message` is a valid, exclusively borrowed `MSG` for
            // `PeekMessageW` to write into; a null `hWnd` filter is the
            // documented way to retrieve messages for every window owned by
            // this thread, and `PM_REMOVE` takes them off the queue exactly
            // as `GetMessageW` would.
            let available =
                unsafe { PeekMessageW(&raw mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) }
                    != 0;
            if !available {
                return Ok(());
            }
            if handle_message(&message)? == LoopStep::Quit {
                self.quit = true;
                return Ok(());
            }
        }
        panic!(
            "the native message queue did not drain within {MAX_MESSAGES_PER_PUMP} messages, \
             which means something in the backend is generating messages faster than they are \
             consumed"
        );
    }

    /// Whether `WM_QUIT` has been seen — the backend's way of asking the
    /// application to exit.
    pub(crate) fn quit_requested(&self) -> bool {
        self.quit
    }

    /// Every window id the backend currently has a `Runtime` for, including
    /// ones already torn down (see [`Self::is_destroyed`]).
    pub(crate) fn window_ids(&self) -> Vec<WindowId> {
        let mut ids: Vec<_> = self.registry.runtimes.keys().copied().collect();
        ids.sort_unstable_by_key(|id| id.get());
        ids
    }

    /// The window ids the backend still considers live.
    pub(crate) fn live_window_ids(&self) -> Vec<WindowId> {
        self.window_ids().into_iter().filter(|id| !self.is_destroyed(*id)).collect()
    }

    /// Runs `f` against one window's `Runtime`.
    ///
    /// # Panics
    ///
    /// Panics if `id` names no window the backend created.
    pub(crate) fn with_runtime<R>(&self, id: WindowId, f: impl FnOnce(&Runtime) -> R) -> R {
        f(self.registry.runtimes.get(&id).expect("the harness was asked about an unknown window"))
    }

    /// Runs `f` against one window's `Runtime` mutably — for tests that
    /// stand in a fake device (a controller source) or drive a backend
    /// entry point directly with a native structure of their own.
    ///
    /// # Panics
    ///
    /// Panics if `id` names no window the backend created.
    pub(crate) fn with_runtime_mut<R>(
        &mut self,
        id: WindowId,
        f: impl FnOnce(&mut Runtime) -> R,
    ) -> R {
        let runtime = self
            .registry
            .runtimes
            .get_mut(&id)
            .expect("the harness was asked about an unknown window");
        let result = f(runtime);
        self.pump();
        result
    }

    /// Posts `message` to `hwnd` — through the queue, so it reaches the
    /// loop's pre-dispatch pass exactly as real input does — and pumps.
    pub(crate) fn post(&mut self, hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) {
        // SAFETY: `hwnd` is a live window owned by this thread;
        // `PostMessageW` takes no pointer arguments beyond it.
        let posted = unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(hwnd, message, wparam, lparam)
        } != 0;
        assert!(posted, "posting a test message must succeed");
        self.pump();
    }

    /// Sends `message` to `hwnd` synchronously, then pumps whatever it
    /// caused to be posted.
    pub(crate) fn send(
        &mut self,
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> isize {
        // SAFETY: as in `post`.
        let result = unsafe { SendMessageW(hwnd, message, wparam, lparam) };
        self.pump();
        result
    }

    /// One window's native top-level `HWND`.
    pub(crate) fn hwnd(&self, id: WindowId) -> HWND {
        self.with_runtime(id, |runtime| runtime.window)
    }

    /// Whether the backend has torn this window's native window down.
    pub(crate) fn is_destroyed(&self, id: WindowId) -> bool {
        self.registry.runtimes.get(&id).is_none_or(|runtime| runtime.destroyed)
    }

    /// The native error a window recorded, if any. This is what
    /// `run_application` collects and returns from `Platform::run`, so it
    /// is the same value a real application would see.
    pub(crate) fn error_for(&self, id: WindowId) -> Option<String> {
        self.with_runtime(id, |runtime| runtime.error.as_ref().map(ToString::to_string))
    }

    /// The `HWND` realizing `key`'s node in `window`, if the backend
    /// created one.
    pub(crate) fn control(&self, window: WindowId, key: &str) -> Option<HWND> {
        self.with_runtime(window, |runtime| {
            let registry = &runtime.renderer.registry;
            // A root-component node's identity is its bare key; a child
            // component's is scoped, so it is found by its local key.
            registry.get(NodeId::from_key(key)).map(NativeObject::hwnd).or_else(|| {
                runtime
                    .renderer
                    .snapshot
                    .nodes()
                    .find(|node| node.id.local_key().as_deref() == Some(key))
                    .and_then(|node| registry.get(node.id))
                    .map(NativeObject::hwnd)
            })
        })
    }

    /// [`Self::control`], panicking with the node key when it is absent.
    ///
    /// # Panics
    ///
    /// Panics if no native object exists for `key`.
    pub(crate) fn expect_control(&self, window: WindowId, key: &str) -> HWND {
        self.control(window, key)
            .unwrap_or_else(|| panic!("no native control was realized for node {key:?}"))
    }

    /// Simulates activating a button: Win32 delivers a click as a
    /// `BN_CLICKED` notification to the control's parent, which is exactly
    /// what a real press produces.
    ///
    /// Sent rather than posted so the effect is observable the moment this
    /// returns, and sent to the control's *parent* because that is where
    /// Win32 addresses `WM_COMMAND`.
    ///
    /// # Panics
    ///
    /// Panics if `key` names no realized control.
    pub(crate) fn click(&mut self, window: WindowId, key: &str) {
        const BN_CLICKED: u16 = 0;
        let control = self.expect_control(window, key);
        // SAFETY: `control` is a live HWND from this window's registry, and
        // `GetParent` returns null only for a top-level window, which a
        // realized control never is; `wparam`/`lparam` are packed exactly as
        // `WM_COMMAND` documents for a control notification.
        let parent = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetParent(control) };
        let wparam = pack_notification(BN_CLICKED, 0);
        // SAFETY: `parent` is a live HWND owned by this backend; the
        // message and its parameters match `WM_COMMAND`'s documented shape
        // for a control notification, which `container_proc`/`window_proc`
        // already handle.
        unsafe {
            SendMessageW(parent, WM_COMMAND, wparam, control as LPARAM);
        }
        self.pump();
    }

    /// Simulates selecting a native menu item by its command id. Win32
    /// delivers these as a `WM_COMMAND` with a null `lParam` and a zero
    /// notification code, which is how `window_proc` distinguishes them
    /// from control notifications.
    pub(crate) fn select_menu_command(&mut self, window: WindowId, command_id: u16) {
        let hwnd = self.hwnd(window);
        // SAFETY: `hwnd` is this window's live top-level handle; the
        // parameters match `WM_COMMAND`'s documented menu shape.
        unsafe {
            SendMessageW(hwnd, WM_COMMAND, pack_notification(0, command_id), 0);
        }
        self.pump();
    }

    /// Sends a key press to the window, as Win32 would after a real
    /// keystroke.
    ///
    /// Posted rather than sent: `WM_KEYDOWN` is handled in the loop's
    /// *pre-dispatch* pass (see `message_loop::pre_dispatch`), not in
    /// `window_proc`, so it has to travel through the queue to be seen at
    /// all — which is itself part of what this exercises.
    pub(crate) fn press_key(&mut self, window: WindowId, virtual_key: u32) {
        let hwnd = self.hwnd(window);
        // SAFETY: `hwnd` is this window's live top-level handle;
        // `PostMessageW` takes no pointer arguments beyond it.
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(
                hwnd,
                WM_KEYDOWN,
                virtual_key as WPARAM,
                0,
            );
        }
        self.pump();
    }

    /// Asks a window to close, exactly as its title-bar close button does.
    pub(crate) fn request_close(&mut self, window: WindowId) {
        let hwnd = self.hwnd(window);
        // SAFETY: `hwnd` is this window's live top-level handle.
        unsafe {
            SendMessageW(hwnd, WM_CLOSE, 0, 0);
        }
        self.pump();
    }

    /// Whether Win32 still recognizes `hwnd` as a window — the ground truth
    /// for "was this actually destroyed", independent of any bookkeeping
    /// this crate keeps.
    pub(crate) fn window_exists(hwnd: HWND) -> bool {
        // SAFETY: `IsWindow` is documented to accept any handle value,
        // including a stale or null one, and is the supported way to ask
        // this question.
        unsafe { IsWindow(hwnd) != 0 }
    }
}

/// Packs a `WM_COMMAND` `wParam` the way Win32 does: notification code in
/// the high word, control or menu id in the low word.
fn pack_notification(notification: u16, id: u16) -> WPARAM {
    (usize::from(notification) << 16) | usize::from(id)
}
