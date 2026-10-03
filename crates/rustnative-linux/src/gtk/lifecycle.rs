//! The application's life as the Linux desktop reports it, and the
//! persisted state that has to be on disk before each step of it.
//!
//! | The desktop says | The application hears | State |
//! |---|---|---|
//! | logind's `PrepareForSleep(true)` (suspend, hibernate) | `Lifecycle::Suspending` | flushed first |
//! | logind's `PrepareForSleep(false)` | `Lifecycle::Resuming` | — |
//! | logind's `PrepareForShutdown(true)` | `Lifecycle::Terminating` | flushed first |
//! | GIO's memory monitor at `low` or worse (the kernel's pressure stall information) | `Lifecycle::LowMemory` | flushed first |
//! | the last window closed (the loop ends) | `Lifecycle::Terminating` | flushed first (`gtk::app`) |
//! | a burst of writes went quiet for [`IDLE_FLUSH`] | nothing | flushed |
//!
//! The primary window's placement is saved in the same store when it
//! closes and restored when it next opens: its size and whether it was
//! maximized everywhere, and its position on X11 — a Wayland compositor
//! places windows itself and tells the client nothing of where.

use std::cell::RefCell;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gio, glib};
use rustnative_core::{Event, Lifecycle, StateStore, WindowId};

use super::backend::{Work, post};

/// How long writes must stop before they are flushed.
pub(crate) const IDLE_FLUSH: Duration = Duration::from_millis(1500);

/// The key the primary window's placement is saved under (the Windows
/// backend's key, so the meaning is the same).
const PLACEMENT_KEY: &str = "rust-native/window-placement/primary";

/// The desktop's lifecycle sources, watched while the application runs.
#[derive(Default)]
pub(crate) struct Watchers {
    subscriptions: Vec<gio::SignalSubscription>,
    memory: Option<(gio::MemoryMonitor, glib::SignalHandlerId)>,
    flush: RefCell<Option<glib::SourceId>>,
}

impl std::fmt::Debug for Watchers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watchers")
            .field("logind", &self.subscriptions.len())
            .finish_non_exhaustive()
    }
}

/// Reports `lifecycle` to the application: state is flushed, then the
/// primary window's tree hears it.
pub(crate) fn report(lifecycle: Lifecycle) {
    post(Work::Call(Box::new(|registry| {
        let _ = registry.with_application(rustnative_core::Application::flush_state);
    })));
    post(Work::Event(WindowId::PRIMARY, Event::Lifecycle(lifecycle)));
}

impl Watchers {
    /// Starts watching logind (on the system bus, if there is one) and the
    /// memory monitor. Runs on the GTK thread, whose context delivers them.
    pub(crate) fn start() -> Self {
        let mut watchers = Self::default();
        if let Ok(system) = gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE) {
            for (signal, starting, ending) in [
                ("PrepareForSleep", Lifecycle::Suspending, Some(Lifecycle::Resuming)),
                ("PrepareForShutdown", Lifecycle::Terminating, None),
            ] {
                watchers.subscriptions.push(system.subscribe_to_signal(
                    Some("org.freedesktop.login1"),
                    Some("org.freedesktop.login1.Manager"),
                    Some(signal),
                    Some("/org/freedesktop/login1"),
                    None,
                    gio::DBusSignalFlags::NONE,
                    move |signal| match signal.parameters.get::<(bool,)>() {
                        Some((true,)) => report(starting),
                        Some((false,)) => {
                            if let Some(ending) = ending {
                                report(ending);
                            }
                        }
                        None => {}
                    },
                ));
            }
        }
        let monitor = gio::MemoryMonitor::dup_default();
        let handler = monitor.connect_low_memory_warning(|_, level| {
            use glib::translate::IntoGlib as _;
            if level.into_glib() >= gio::MemoryMonitorWarningLevel::Low.into_glib() {
                report(Lifecycle::LowMemory);
            }
        });
        watchers.memory = Some((monitor, handler));
        watchers
    }

    /// Arms (or re-arms) the idle flush when anything waits to be written:
    /// called after every change, so it fires once things go quiet.
    pub(crate) fn after_change(&self, unsaved: bool) {
        if !unsaved {
            return;
        }
        if let Some(previous) = self.flush.borrow_mut().take() {
            previous.remove();
        }
        let source = glib::timeout_add_local_once(IDLE_FLUSH, || {
            post(Work::Call(Box::new(|registry| {
                registry.lifecycle_watchers().flush.borrow_mut().take();
                let _ = registry.with_application(rustnative_core::Application::flush_state);
            })));
        });
        *self.flush.borrow_mut() = Some(source);
    }

    /// Stops watching.
    pub(crate) fn release(&mut self) {
        self.subscriptions.clear();
        if let Some((monitor, handler)) = self.memory.take() {
            monitor.disconnect(handler);
        }
        if let Some(pending) = self.flush.borrow_mut().take() {
            pending.remove();
        }
    }
}

/// A saved placement: the size, whether maximized, and (X11) the position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Placement {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) maximized: bool,
    pub(crate) position: Option<(i32, i32)>,
}

impl Placement {
    fn encode(self) -> Vec<u8> {
        let (x, y) = self
            .position
            .map_or((String::new(), String::new()), |(x, y)| (x.to_string(), y.to_string()));
        format!("{} {} {} {x} {y}", self.width, self.height, u8::from(self.maximized))
            .trim_end()
            .as_bytes()
            .to_vec()
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?;
        let mut fields = text.split(' ');
        let width = fields.next()?.parse().ok().filter(|width| *width > 0)?;
        let height = fields.next()?.parse().ok().filter(|height| *height > 0)?;
        let maximized = fields.next()? == "1";
        let position = match (fields.next(), fields.next()) {
            (Some(x), Some(y)) => Some((x.parse().ok()?, y.parse().ok()?)),
            _ => None,
        };
        Some(Self { width, height, maximized, position })
    }
}

/// `window`'s X11 handle, if it is on X11 and realized.
fn x11(window: &gtk::Window) -> Option<(*mut x11::xlib::Display, x11::xlib::Window)> {
    let surface = window.surface()?;
    let x11 = surface.downcast_ref::<gdk4_x11::X11Surface>()?;
    let display = x11.display().downcast::<gdk4_x11::X11Display>().ok()?;
    // SAFETY: the display is GTK's own, open while the window is.
    Some((unsafe { display.xdisplay() }, x11.xid()))
}

/// Where `window`'s content is on the X screen.
fn x11_position(window: &gtk::Window) -> Option<(i32, i32)> {
    let (display, xid) = x11(window)?;
    let (mut x, mut y, mut child) = (0, 0, 0);
    // SAFETY: a live display and window; the outputs are plain values.
    let translated = unsafe {
        let root = x11::xlib::XDefaultRootWindow(display);
        x11::xlib::XTranslateCoordinates(
            display,
            xid,
            root,
            0,
            0,
            &raw mut x,
            &raw mut y,
            &raw mut child,
        )
    };
    (translated != 0).then_some((x, y))
}

/// Saves `window`'s placement in `store`; called as the primary window
/// closes.
pub(crate) fn save_placement(window: &gtk::Window, store: &dyn StateStore) {
    let (width, height) = window.default_size();
    let placement = Placement {
        width: if width > 0 { width } else { window.width() },
        height: if height > 0 { height } else { window.height() },
        maximized: window.is_maximized(),
        position: x11_position(window),
    };
    let _ = store.save(PLACEMENT_KEY, &placement.encode());
}

/// The saved placement, if any.
pub(crate) fn saved_placement(store: &dyn StateStore) -> Option<Placement> {
    Placement::decode(&store.load(PLACEMENT_KEY).ok()??)
}

/// Applies `placement` to `window` before it is shown: the size and
/// maximized state now, the position (X11) once it is mapped — if that
/// position is still on a connected monitor.
pub(crate) fn restore_placement(window: &gtk::Window, placement: Placement) {
    window.set_default_size(placement.width, placement.height);
    if placement.maximized {
        window.maximize();
    }
    let Some((x, y)) = placement.position else { return };
    let on_screen =
        WidgetExt::display(window).monitors().iter::<gtk::gdk::Monitor>().flatten().any(
            |monitor| {
                let geometry = monitor.geometry();
                let scale = monitor.scale_factor().max(1);
                let (left, top) = (geometry.x() * scale, geometry.y() * scale);
                let (right, bottom) =
                    (left + geometry.width() * scale, top + geometry.height() * scale);
                (left..right).contains(&x) && (top..bottom).contains(&y)
            },
        );
    if !on_screen {
        return;
    }
    window.connect_map(move |window| {
        if let Some((display, xid)) = x11(window) {
            // The window manager may offset this by its frame, as it does
            // for any client-requested position.
            // SAFETY: a live display and the mapped window.
            unsafe {
                x11::xlib::XMoveWindow(display, xid, x, y);
                x11::xlib::XFlush(display);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_placement_round_trips_and_garbage_reads_as_none() {
        let wayland = Placement { width: 800, height: 600, maximized: true, position: None };
        assert_eq!(Placement::decode(&wayland.encode()), Some(wayland));
        let x11 =
            Placement { width: 640, height: 480, maximized: false, position: Some((-20, 35)) };
        assert_eq!(Placement::decode(&x11.encode()), Some(x11));
        assert_eq!(Placement::decode(b"0 600 0"), None, "an empty size");
        assert_eq!(Placement::decode(b"not a placement"), None);
        assert_eq!(
            Placement::decode(b"800 600 0 12"),
            Some(Placement { width: 800, height: 600, maximized: false, position: None })
        );
    }
}
