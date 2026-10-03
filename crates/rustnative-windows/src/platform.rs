//! The `Platform` entry point this backend exposes to `rustnative_core`.

#[cfg(windows)]
use rustnative_core::Capability;
use rustnative_core::{Application, Platform, PlatformCapabilities};

use crate::Error;

/// The Win32 backend an application hands to `rustnative_core::Application`
/// to run.
///
/// Carries only configuration: everything a running application needs
/// (`WindowRegistry`, per-window `Runtime`s, native object registries) is
/// owned by [`Platform::run`]'s own stack frame for exactly as long as the
/// message loop runs — see `native::context`'s module documentation for why
/// that lifetime relationship is what makes the backend's raw-pointer
/// bookkeeping sound.
#[derive(Debug, Default, Clone)]
pub struct WindowsPlatform {
    app_id: Option<String>,
}

impl WindowsPlatform {
    /// Creates the backend. Equivalent to [`Default::default`].
    #[must_use]
    pub const fn new() -> Self {
        Self { app_id: None }
    }

    /// Identifies the application, which makes it **single-instance**.
    ///
    /// With an id, a second launch does not open a second copy: it hands
    /// the URL it was launched with (the first `scheme://…` command-line
    /// argument, which is how Windows launches an application for a
    /// protocol it handles) to the running instance as
    /// [`rustnative_core::Event::DeepLink`], brings that instance to the
    /// front, and exits. The first launch's own URL, if any, is delivered
    /// the same way once its windows exist.
    ///
    /// Use the same id as `FileStateStore::for_app` (Windows); a reverse
    /// domain name (`com.example.notes`) is conventional.
    #[must_use]
    pub fn with_app_id(mut self, app_id: impl Into<String>) -> Self {
        self.app_id = Some(app_id.into());
        self
    }

    /// The application id, if one was set.
    #[must_use]
    pub fn app_id(&self) -> Option<&str> {
        self.app_id.as_deref()
    }

    /// Realizes `application`'s primary window as a child of `parent`, a
    /// window the host created and owns (embedding inward, `PLAN.md`
    /// Milestone 40). The host's message loop drives it; see
    /// [`crate::EmbeddedRoot`].
    ///
    /// # Errors
    ///
    /// `parent` is not a window, or the root cannot be created.
    #[cfg(windows)]
    pub fn embed<'a>(
        &self,
        parent: windows_sys::Win32::Foundation::HWND,
        application: &'a mut Application,
    ) -> Result<crate::EmbeddedRoot<'a>, Error> {
        crate::native::embed::EmbeddedRoot::start(application, parent)
    }

    /// Brings up `application`'s windows without running a message loop:
    /// the host's loop drives them (guest-runtime mode, `PLAN.md`
    /// Milestone 40). See [`crate::ExternalLoop`].
    ///
    /// # Errors
    ///
    /// A window cannot be created.
    #[cfg(windows)]
    pub fn start_external<'a>(
        &self,
        application: &'a mut Application,
    ) -> Result<crate::ExternalLoop<'a>, Error> {
        crate::native::embed::ExternalLoop::start(application, std::ptr::null_mut())
    }
}

#[cfg(windows)]
impl Platform for WindowsPlatform {
    type Error = Error;

    fn run(&mut self, application: &mut Application) -> Result<(), Self::Error> {
        crate::native::run_application(
            application,
            self.app_id.as_deref(),
            crate::native::single_instance::launch_url(std::env::args()),
        )
    }

    fn style_capabilities(&self) -> rustnative_core::StyleCapabilities {
        rustnative_style::WINDOWS
    }

    fn unit_mapping(&self) -> Option<rustnative_core::UnitMapping> {
        Some(rustnative_style::WINDOWS_UNITS)
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::new(
            [
                Capability::Clipboard,
                Capability::UrlLaunch,
                Capability::MultipleWindows,
                Capability::WindowManagement,
                Capability::FileDialogs,
                Capability::Notifications,
                Capability::Menus,
                Capability::DragAndDrop,
                Capability::Touch,
                Capability::Pen,
                Capability::Gamepad,
                Capability::Ime,
                Capability::Animations,
                Capability::ReducedMotionPreference,
                Capability::CustomDrawing,
                Capability::NativeSurfaces,
                Capability::StatePersistence,
                Capability::DeepLinks,
                Capability::Lifecycle,
                // Milestone 39: cursors per node, hover, command shortcuts,
                // host-mirrored right-to-left, host traits in the environment
                // (which includes following the system appearance), and the
                // consent-store permission states.
                Capability::Cursors,
                Capability::Hover,
                Capability::CommandShortcuts,
                Capability::RightToLeft,
                Capability::HostTraits,
                Capability::SystemAppearance,
                Capability::Permissions,
                // Milestone 48: host content. Media always (`MCIWnd`); the
                // camera where a capture driver is installed; web content is
                // not offered (see `native::host_content`).
                Capability::MediaPlayback,
                // Milestone 51: the spooler and COM ports.
                Capability::Printing,
                Capability::SerialPorts,
                // Milestone 57: surfaces beyond the window.
                Capability::Surface(rustnative_core::SurfaceKind::TrayExtra),
                Capability::Surface(rustnative_core::SurfaceKind::JumpList),
                Capability::Surface(rustnative_core::SurfaceKind::TaskbarProgress),
                // Milestone 34's display-server answers, which Windows gives
                // unconditionally: a top-level window goes where it is put
                // (its saved placement is restored), and the system draws
                // its frame. The menu bar is in the window, never global.
                Capability::WindowPlacement,
                Capability::ServerSideDecorations,
            ]
            .into_iter()
            .chain(crate::native::accelerator::gpu_available().then_some(Capability::Accelerator(
                rustnative_core::capability::AcceleratorKind::Gpu,
            )))
            .chain(crate::native::accelerator::npu_available().then_some(Capability::Accelerator(
                rustnative_core::capability::AcceleratorKind::Npu,
            )))
            .chain(crate::native::host_content::camera_available().then_some(Capability::Camera)),
        )
    }

    fn native_extension(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(not(windows))]
impl Platform for WindowsPlatform {
    type Error = Error;

    fn run(&mut self, _application: &mut Application) -> Result<(), Self::Error> {
        Err(Error::UnsupportedHost)
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::default()
    }

    fn native_extension(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn capabilities_only_advertise_realized_backend_features() {
        let capabilities = WindowsPlatform::new().capabilities();
        assert!(capabilities.supports(Capability::MultipleWindows));
        assert!(capabilities.supports(Capability::WindowManagement));
        assert!(capabilities.supports(Capability::Clipboard));
        assert!(capabilities.supports(Capability::UrlLaunch));
        assert!(capabilities.supports(Capability::FileDialogs));
        assert!(capabilities.supports(Capability::Notifications));
        assert!(capabilities.supports(Capability::Menus));
        // Milestone 25: OLE drop target, `WM_POINTER` touch and pen,
        // `XInput` controllers, and IMM32 composition.
        assert!(capabilities.supports(Capability::DragAndDrop));
        assert!(capabilities.supports(Capability::Touch));
        assert!(capabilities.supports(Capability::Pen));
        assert!(capabilities.supports(Capability::Gamepad));
        assert!(capabilities.supports(Capability::Ime));
        // Milestone 27: frame-driven native property animation, and the
        // system's own reduced-motion setting.
        assert!(capabilities.supports(Capability::Animations));
        assert!(capabilities.supports(Capability::ReducedMotionPreference));
        // Milestone 29: Direct2D canvases and raw-window-handle surfaces.
        assert!(capabilities.supports(Capability::CustomDrawing));
        assert!(capabilities.supports(Capability::NativeSurfaces));
        // Milestone 30: the file state store, single-instance deep links,
        // and session/power lifecycle notifications.
        assert!(capabilities.supports(Capability::StatePersistence));
        // Milestone 48: media playback; web content is not offered.
        assert!(capabilities.supports(Capability::MediaPlayback));
        assert!(!capabilities.supports(Capability::WebContent));
        assert!(capabilities.supports(Capability::DeepLinks));
        assert!(capabilities.supports(Capability::Lifecycle));
        // Milestone 39.
        for realized in [
            Capability::Cursors,
            Capability::Hover,
            Capability::CommandShortcuts,
            Capability::RightToLeft,
            Capability::HostTraits,
            Capability::SystemAppearance,
            Capability::Permissions,
        ] {
            assert!(capabilities.supports(realized), "{realized:?}");
        }
        // System sharing is still only a portable contract.
        assert!(!capabilities.supports(Capability::SystemShare));
        assert!(capabilities.supports(Capability::Printing));
        assert!(capabilities.supports(Capability::SerialPorts));
        // Answered from the machine, never assumed.
        assert_eq!(
            capabilities.supports(Capability::Accelerator(
                rustnative_core::capability::AcceleratorKind::Gpu
            )),
            crate::native::accelerator::gpu_available()
        );
        assert_eq!(
            capabilities.supports(Capability::Accelerator(
                rustnative_core::capability::AcceleratorKind::Npu
            )),
            crate::native::accelerator::npu_available()
        );
        // Milestone 57 realizes the tray, the jump list, and taskbar
        // progress; the rest need package identity or are not Windows
        // concepts (`docs/surfaces.md`).
        for surface in [
            rustnative_core::SurfaceKind::TrayExtra,
            rustnative_core::SurfaceKind::JumpList,
            rustnative_core::SurfaceKind::TaskbarProgress,
        ] {
            assert!(capabilities.supports(Capability::Surface(surface)), "{surface:?}");
        }
        for surface in [
            rustnative_core::SurfaceKind::Widget,
            rustnative_core::SurfaceKind::LiveActivity,
            rustnative_core::SurfaceKind::Tile,
            rustnative_core::SurfaceKind::Extension,
            rustnative_core::SurfaceKind::InstantApp,
            rustnative_core::SurfaceKind::CompanionDevice,
        ] {
            assert!(!capabilities.supports(Capability::Surface(surface)), "{surface:?}");
        }
        // Milestone 34's display-server answers.
        assert!(capabilities.supports(Capability::WindowPlacement));
        assert!(capabilities.supports(Capability::ServerSideDecorations));
        assert!(!capabilities.supports(Capability::GlobalMenuBar));
    }
}
