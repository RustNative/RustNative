//! The toolkit-independent half of the Linux backend: what every Linux
//! desktop shares whichever toolkit draws its widgets — the display-server
//! and desktop-environment model, and (on Linux) the freedesktop services
//! reached over D-Bus.
//!
//! Nothing here names GTK. A second toolkit behind `crate::toolkit` reuses
//! all of it.

#[cfg(all(test, target_os = "linux"))]
pub(crate) mod fake_bus;
pub mod locale;
#[cfg(target_os = "linux")]
pub mod notifications;
#[cfg(target_os = "linux")]
pub mod open_uri;
#[cfg(target_os = "linux")]
pub mod portal;
mod session;
#[cfg(target_os = "linux")]
pub mod settings;
#[cfg(target_os = "linux")]
pub mod single_instance;
#[cfg(target_os = "linux")]
pub mod tray;

pub use session::{DesktopEnvironment, DisplayServer, Session};

/// What the toolkit-independent half realizes in `session`: answers that
/// depend on the display server or the desktop rather than on the widgets.
#[cfg(target_os = "linux")]
pub(crate) fn capabilities(session: &Session) -> Vec<rustnative_core::Capability> {
    use rustnative_core::Capability;
    // Every session: URLs through the OpenURI portal or GIO's handlers,
    // notifications through the notification server or the portal, and
    // state files under the XDG state directory (`crate::services`).
    // Serial ports through termios, and permissions (ungated, or the
    // portals in a sandbox).
    let mut answered = vec![
        Capability::UrlLaunch,
        Capability::Notifications,
        Capability::StatePersistence,
        Capability::SerialPorts,
        Capability::Permissions,
        // logind, the memory monitor, and the idle flush
        // (`gtk::lifecycle`); launch URLs and a second launch's link
        // (`desktop::single_instance`).
        Capability::Lifecycle,
        Capability::DeepLinks,
        // The launcher entry's progress (`desktop::tray::set_progress`).
        Capability::Surface(rustnative_core::SurfaceKind::TaskbarProgress),
    ];
    // A tray icon only where a tray host runs.
    if tray::available() {
        answered.push(Capability::Surface(rustnative_core::SurfaceKind::TrayExtra));
    }
    // X11 honours a client's requested position; a Wayland compositor
    // places every window itself and tells the client nothing.
    if session.display_server() == Some(DisplayServer::X11) {
        answered.push(Capability::WindowPlacement);
    }
    answered
}
