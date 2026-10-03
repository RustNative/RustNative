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
pub(crate) mod foreign;
#[cfg(test)]
mod guarantees;
mod host_traits;
pub(crate) mod input;
#[cfg(test)]
mod input_integration;
#[cfg(test)]
mod integration;
pub(crate) mod layout_widget;
mod measure;
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

    fn capabilities(&self, _session: &Session) -> Vec<Capability> {
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
        realized
    }
}
