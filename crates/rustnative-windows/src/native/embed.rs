//! Running inside someone else's program (`PLAN.md` Milestone 40):
//!
//! - **guest-runtime mode** ([`ExternalLoop`]): the application's windows
//!   exist and work, but the message loop is the host's — the framework
//!   owns neither `main`, startup, nor the loop;
//! - **embedding inward** ([`EmbeddedRoot`]): the same, with the primary
//!   window realized as a `WS_CHILD` of a window the host created and owns,
//!   participating in the host's sizing ([`EmbeddedRoot::set_bounds`]) and
//!   teardown (dropping the root destroys the framework's windows and
//!   nothing of the host's).
//!
//! The host's loop offers each message first:
//!
//! ```text
//! while GetMessageW(&mut msg, null, 0, 0) > 0 {
//!     if !root.handle_message(&msg) {
//!         TranslateMessage(&msg);
//!         DispatchMessageW(&msg);
//!     }
//! }
//! ```
//!
//! `handle_message` takes the messages for the framework's windows (so Tab
//! traversal, shortcuts, and focus tracking behave as under the
//! framework's own loop) and leaves every other message to the host. A
//! host that dispatches everything itself still works — scheduler wakes
//! and controls are handled by the windows' own procedures — but loses
//! keyboard traversal between the framework's controls, exactly as a host
//! that never calls `IsDialogMessage` loses it between its own.
//!
//! Where the framework would end the process's loop (its primary window
//! closing, a native error), it records the request instead of posting
//! `WM_QUIT` into a loop that is not its own: see
//! [`ExternalLoop::finished`] and [`ExternalLoop::error`].

use std::cell::Cell;
use std::marker::PhantomData;

use rustnative_core::{Application, WindowId};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, IsWindow, MSG, PostQuitMessage, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
};

use super::app::OleApartment;
use super::message_loop::register_window_classes;
use super::runtime::WindowRegistry;
use super::util::module_instance;
use super::win32::{best_effort, ignored_by_contract};
use crate::Error;

thread_local! {
    /// How many host-driven loops are active on this thread.
    static HOST_LOOPS: Cell<u32> = const { Cell::new(0) };
    /// The exit code the framework asked for while a host owned the loop.
    static QUIT_REQUESTED: Cell<Option<i32>> = const { Cell::new(None) };
}

/// Asks the message loop to end — the framework's own loop through
/// `WM_QUIT`; a host's loop by recording the request for
/// [`ExternalLoop::finished`], because posting `WM_QUIT` would end a loop
/// the framework does not own.
pub(crate) fn request_quit(code: i32) {
    if HOST_LOOPS.with(Cell::get) > 0 {
        QUIT_REQUESTED.with(|requested| requested.set(Some(code)));
    } else {
        // SAFETY: `PostQuitMessage` takes an exit code and no pointers; it
        // only queues `WM_QUIT` so the loop unwinds through its ordinary
        // exit path.
        unsafe { PostQuitMessage(code) };
    }
}

struct HostLoop;

impl HostLoop {
    fn enter() -> Self {
        HOST_LOOPS.with(|loops| loops.set(loops.get() + 1));
        QUIT_REQUESTED.with(|requested| requested.set(None));
        Self
    }
}

impl Drop for HostLoop {
    fn drop(&mut self) {
        HOST_LOOPS.with(|loops| loops.set(loops.get().saturating_sub(1)));
    }
}

/// An application running under a host's message loop (guest-runtime
/// mode). Created by [`crate::WindowsPlatform::start_external`]; see the
/// module documentation for the loop the host writes.
pub struct ExternalLoop<'a> {
    // Dropped first: the windows go before the loop marker and OLE.
    registry: Box<WindowRegistry>,
    _host: HostLoop,
    _ole: OleApartment,
    _application: PhantomData<&'a mut Application>,
}

impl<'a> ExternalLoop<'a> {
    pub(crate) fn start(application: &'a mut Application, parent: HWND) -> Result<Self, Error> {
        register_window_classes(module_instance())?;
        let ole = OleApartment::enter();
        let host = HostLoop::enter();
        super::host_traits::apply(application, &super::host_traits::read());
        // SAFETY: `application` is borrowed for `'a`, the lifetime of the
        // returned value, which owns `registry` and every `Runtime` it
        // creates — point 1 of `native::context`'s module documentation,
        // held by the borrow checker rather than by a stack frame.
        let mut registry = Box::new(unsafe { WindowRegistry::new(application) });
        registry.embed_parent = parent;
        registry.sync()?;
        Ok(Self { registry, _host: host, _ole: ole, _application: PhantomData })
    }

    /// Offers `message`, just retrieved by the host's loop. Returns `true`
    /// when it was for one of the framework's windows and has been fully
    /// handled — the host must not translate or dispatch it again — and
    /// `false` when it is the host's.
    ///
    /// # Errors
    ///
    /// A native failure while handling it (also recorded for
    /// [`Self::error`]).
    pub fn handle_message(&mut self, message: &MSG) -> Result<bool, Error> {
        super::message_loop::handle_for_host(message)
    }

    /// Whether the application asked to end — its primary window closed,
    /// or a native error — and with which code. The host decides what that
    /// means for its own loop.
    #[must_use]
    pub fn finished(&self) -> Option<i32> {
        QUIT_REQUESTED.with(Cell::get)
    }

    /// The first native failure a window recorded, if any.
    #[must_use]
    pub fn error(&self) -> Option<&Error> {
        self.registry.runtimes.values().find_map(|runtime| runtime.error.as_ref())
    }

    /// The native window realizing `id`, while it exists.
    #[must_use]
    pub fn window(&self, id: WindowId) -> Option<HWND> {
        self.registry
            .runtimes
            .get(&id)
            .filter(|runtime| !runtime.destroyed)
            .map(|runtime| runtime.window)
    }

    fn primary(&self) -> Option<HWND> {
        self.registry
            .runtimes
            .get(&WindowId::PRIMARY)
            .filter(|runtime| !runtime.destroyed)
            .map(|runtime| runtime.window)
    }
}

impl Drop for ExternalLoop<'_> {
    fn drop(&mut self) {
        let windows: Vec<HWND> = self
            .registry
            .runtimes
            .values()
            .filter(|runtime| !runtime.destroyed)
            .map(|runtime| runtime.window)
            .collect();
        for hwnd in windows {
            // SAFETY: `hwnd` is a window this registry created and still
            // owns; destroying it runs its own `WM_DESTROY` teardown (which,
            // for an embedded root, ends nothing but itself).
            best_effort(
                unsafe { DestroyWindow(hwnd) } != 0,
                "DestroyWindow",
                "the window outlives its runtime",
            );
        }
    }
}

/// A Rust Native tree realized inside a window the host created and owns
/// (embedding inward). Created by [`crate::WindowsPlatform::embed`].
pub struct EmbeddedRoot<'a> {
    host: ExternalLoop<'a>,
}

impl<'a> EmbeddedRoot<'a> {
    pub(crate) fn start(application: &'a mut Application, parent: HWND) -> Result<Self, Error> {
        // SAFETY: `IsWindow` accepts any handle value and takes no
        // pointers.
        if parent.is_null() || unsafe { IsWindow(parent) } == 0 {
            return Err(Error::windows_api("IsWindow(embed parent)"));
        }
        Ok(Self { host: ExternalLoop::start(application, parent)? })
    }

    /// The framework's root window: a child of the host's window.
    #[must_use]
    pub fn hwnd(&self) -> HWND {
        self.host.primary().unwrap_or(std::ptr::null_mut())
    }

    /// Places the root within the host's window, in the host's client
    /// coordinates; the tree lays itself out to the new size.
    pub fn set_bounds(&self, x: i32, y: i32, width: i32, height: i32) {
        let Some(hwnd) = self.host.primary() else { return };
        // SAFETY: `hwnd` is the live root this value owns; a null
        // insert-after with `SWP_NOZORDER` is documented as "keep the
        // z-order".
        let moved = unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                x,
                y,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        best_effort(
            moved != 0,
            "SetWindowPos(embedded root)",
            "the root keeps its previous bounds",
        );
    }

    /// Offers a message from the host's loop; see
    /// [`ExternalLoop::handle_message`].
    ///
    /// # Errors
    ///
    /// A native failure while handling it.
    pub fn handle_message(&mut self, message: &MSG) -> Result<bool, Error> {
        self.host.handle_message(message)
    }

    /// The first native failure the embedded tree recorded, if any.
    #[must_use]
    pub fn error(&self) -> Option<&Error> {
        self.host.error()
    }
}

impl Drop for EmbeddedRoot<'_> {
    fn drop(&mut self) {
        // The host's window stays; only the root is hidden before the
        // loop's teardown destroys it, so the host never paints a
        // half-destroyed child.
        if let Some(hwnd) = self.host.primary() {
            // SAFETY: `hwnd` is the live root this value owns.
            ignored_by_contract(unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(
                    hwnd,
                    windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE,
                )
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use rustnative_core::{Application, Component, Event, Node, Size, Window};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        PM_REMOVE, PeekMessageW, PostMessageW, WM_CLOSE, WM_QUIT,
    };

    use super::*;

    struct Empty;
    impl Component for Empty {
        type Props = ();
        type Message = ();
        fn new((): ()) -> Self {
            Self
        }
        fn props(&self) -> &() {
            &()
        }
        fn set_props(&mut self, (): ()) {}
        fn view(&self) -> Node {
            Node::label("x", "guest")
        }
        fn update(&mut self, _: Event) {}
    }

    /// Guest-runtime mode: closing the primary window under a host's loop
    /// records the finish for the host and posts no `WM_QUIT` into a loop
    /// the framework does not own.
    #[test]
    fn under_a_host_loop_the_primary_window_closing_finishes_without_quitting() {
        let _lock = super::super::harness::exclusive();
        let mut application = Application::new(Empty, Window::new("guest", Size::new(200, 100)));
        let mut guest = crate::WindowsPlatform::new().start_external(&mut application).unwrap();
        let primary = guest.primary().unwrap();
        assert_eq!(guest.finished(), None);
        // SAFETY: posting to a live window of this thread.
        unsafe { PostMessageW(primary, WM_CLOSE, 0, 0) };
        let mut message = MSG::default();
        let mut saw_quit = false;
        for _ in 0..1_000 {
            // SAFETY: `message` is writable.
            if unsafe { PeekMessageW(&raw mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) } == 0
            {
                break;
            }
            saw_quit |= message.message == WM_QUIT;
            if !guest.handle_message(&message).unwrap() {
                // SAFETY: an ordinary retrieved message.
                unsafe {
                    windows_sys::Win32::UI::WindowsAndMessaging::TranslateMessage(
                        &raw const message,
                    );
                    windows_sys::Win32::UI::WindowsAndMessaging::DispatchMessageW(
                        &raw const message,
                    );
                }
            }
        }
        assert_eq!(guest.finished(), Some(0));
        assert!(!saw_quit, "no WM_QUIT reached the host's loop");
        assert!(guest.error().is_none());
    }
}
