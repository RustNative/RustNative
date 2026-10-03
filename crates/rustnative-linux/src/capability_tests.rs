//! The capability answers: advertised only once realized (`PLAN.md` 2.5),
//! and answered per display server where the display server decides.

use rustnative_core::{Capability, Platform, SurfaceKind};

use crate::LinuxPlatform;

#[test]
fn capabilities_only_advertise_realized_backend_features() {
    let capabilities = LinuxPlatform::new().capabilities();
    for realized in [
        Capability::MultipleWindows,
        Capability::WindowManagement,
        Capability::Clipboard,
        Capability::FileDialogs,
        Capability::UrlLaunch,
        Capability::Notifications,
        Capability::StatePersistence,
        Capability::Printing,
        Capability::SerialPorts,
        Capability::Permissions,
        Capability::DragAndDrop,
        Capability::Touch,
        Capability::Pen,
        Capability::Ime,
        Capability::Cursors,
        Capability::Hover,
        Capability::CommandShortcuts,
        Capability::RightToLeft,
        Capability::HostTraits,
        Capability::SystemAppearance,
        Capability::ReducedMotionPreference,
        Capability::Animations,
        Capability::CustomDrawing,
        Capability::NativeSurfaces,
    ] {
        assert!(capabilities.supports(realized), "{realized:?}");
    }
    // Not realized on Linux, or not a desktop concept.
    for unrealized in [
        Capability::WebContent,
        Capability::Camera,
        Capability::Bluetooth,
        Capability::Location,
        Capability::SystemShare,
        Capability::Surface(SurfaceKind::Widget),
        Capability::Surface(SurfaceKind::LiveActivity),
        Capability::Surface(SurfaceKind::Tile),
        Capability::Surface(SurfaceKind::Extension),
        Capability::Surface(SurfaceKind::InstantApp),
        Capability::Surface(SurfaceKind::CompanionDevice),
        Capability::History,
        Capability::ServiceWorker,
        Capability::Installable,
    ] {
        assert!(!capabilities.supports(unrealized), "{unrealized:?}");
    }
    // Answered from the machine: GTK's media backend, if installed.
    assert_eq!(
        capabilities.supports(Capability::MediaPlayback),
        crate::gtk::foreign::media_available()
    );
}

#[test]
fn window_placement_is_answered_by_the_display_server() {
    let session = crate::Session::from_environment(
        |name| (name == "WAYLAND_DISPLAY").then(|| "wayland-0".to_owned()),
        false,
    );
    assert!(!crate::desktop::capabilities(&session).contains(&Capability::WindowPlacement));
    let session = crate::Session::from_environment(
        |name| (name == "DISPLAY").then(|| ":0".to_owned()),
        false,
    );
    assert!(crate::desktop::capabilities(&session).contains(&Capability::WindowPlacement));
}
