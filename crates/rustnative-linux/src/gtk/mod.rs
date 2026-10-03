//! The GTK 4 realization of the tree — the one toolkit behind
//! `crate::toolkit` today.

mod accessibility;
#[cfg(test)]
mod accessibility_integration;
mod animation;
#[cfg(test)]
mod animation_integration;
mod app;
#[cfg(test)]
mod atspi_reader;
pub(crate) mod backend;
pub(crate) mod canvas;
mod context;
pub(crate) mod embed;
#[cfg(test)]
mod embed_integration;
pub(crate) mod foreign;
#[cfg(test)]
mod guarantees;
mod host_traits;
pub(crate) mod input;
#[cfg(test)]
mod input_integration;
pub(crate) mod inspect;
#[cfg(test)]
mod inspect_integration;
#[cfg(test)]
mod integration;
pub(crate) mod layout_widget;
pub(crate) mod lifecycle;
mod measure;
pub(crate) mod menu;
#[cfg(test)]
mod menu_integration;
pub(crate) mod registry;
pub(crate) mod rendering;
#[cfg(test)]
mod services_integration;
#[cfg(test)]
mod style_integration;
pub(crate) mod surface;
#[cfg(test)]
mod surface_integration;
mod teardown;
#[cfg(test)]
pub(crate) mod testing;
pub(crate) mod virtual_accessible;
#[cfg(test)]
mod virtual_list_integration;

use rustnative_core::{Application, Capability};

use crate::Error;
use crate::desktop::Session;
use crate::toolkit::{RunOptions, Toolkit};

/// GTK 4.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Gtk4;

impl Toolkit for Gtk4 {
    fn run(
        &self,
        application: &mut Application,
        options: &RunOptions,
        session: &Session,
    ) -> Result<(), Error> {
        app::run_application(application, options, session)
    }

    fn capabilities(&self, session: &Session) -> Vec<Capability> {
        let media = foreign::media_available().then_some(Capability::MediaPlayback);
        let mut realized = vec![
            Capability::MultipleWindows,
            Capability::WindowManagement,
            // Services (`crate::services`): GDK's clipboard and GTK's file
            // dialog.
            Capability::Clipboard,
            Capability::FileDialogs,
            // GTK's print operation over CUPS.
            Capability::Printing,
            // Menu bars (`gtk::menu`).
            Capability::Menus,
            // Input (`gtk::input`): GTK's drop targets, touch sequences and
            // pen samples through the legacy controller, an input method for
            // custom text targets, per-node cursors, hover, and shortcuts
            // offered before the focused widget sees the key.
            Capability::DragAndDrop,
            Capability::Touch,
            Capability::Pen,
            Capability::Ime,
            Capability::Cursors,
            Capability::Hover,
            Capability::CommandShortcuts,
            // The layout engine mirrors placement and GTK mirrors each
            // widget's content (`rendering::realization`).
            Capability::RightToLeft,
            // Host traits (`gtk::host_traits`): the Settings portal and
            // GTK's own settings, followed live.
            Capability::HostTraits,
            Capability::SystemAppearance,
            Capability::ReducedMotionPreference,
            // `GdkFrameClock`-paced transitions and animations
            // (`gtk::animation`).
            Capability::Animations,
            // Canvas nodes drawn with cairo and Pango (`gtk::canvas`).
            Capability::CustomDrawing,
            // `wl_subsurface` or child X window surfaces, through
            // `raw-window-handle` (`gtk::surface`).
            Capability::NativeSurfaces,
        ];
        // Media through GTK's media backend, where one is installed.
        realized.extend(media);
        if decorated_by_server(session) {
            realized.push(Capability::ServerSideDecorations);
        }
        realized
    }
}

/// Whether GTK leaves a window's frame to the host in `session`. On X11
/// GTK asks the window manager to decorate (no custom title bar is set); on
/// Wayland it draws its own unless the compositor offers `KWin`'s
/// server-side decoration protocol, which only Plasma's does — GTK 4 does
/// not use `xdg-decoration`.
pub(crate) fn decorated_by_server(session: &Session) -> bool {
    use crate::desktop::{DesktopEnvironment, DisplayServer};
    match session.display_server() {
        Some(DisplayServer::X11) => true,
        Some(DisplayServer::Wayland) => matches!(session.desktop(), DesktopEnvironment::Kde),
        _ => false,
    }
}
