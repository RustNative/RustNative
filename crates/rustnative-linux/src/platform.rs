//! The `Platform` entry point this backend exposes to `rustnative_core`.

use rustnative_core::{Application, Platform, PlatformCapabilities};

use crate::Error;
use crate::desktop::Session;
use crate::toolkit::ToolkitKind;

/// The Linux backend an application hands to `rustnative_core::Application`
/// to run.
///
/// Carries only configuration; everything a running application needs is
/// owned by [`Platform::run`]'s own frame for as long as the toolkit's main
/// loop runs, as on Windows.
#[derive(Debug, Default, Clone)]
pub struct LinuxPlatform {
    app_id: Option<String>,
    toolkit: ToolkitKind,
}

#[cfg(target_os = "linux")]
impl LinuxPlatform {
    /// Runs `application` under the host's own main loop (guest-runtime
    /// mode, Milestone 40): its windows open now and work as the host's
    /// loop iterates GLib's default context. Drop the result to close them.
    ///
    /// # Errors
    ///
    /// There is no display, or the first windows could not be realized.
    pub fn start_external<'a>(
        &self,
        application: &'a mut Application,
    ) -> Result<crate::ExternalLoop<'a>, Error> {
        crate::gtk::embed::ExternalLoop::start(application, false)
    }

    /// Realizes `application`'s primary window as a widget for the host to
    /// place in its own widget tree (embedding inward, Milestone 40).
    ///
    /// # Errors
    ///
    /// As [`Self::start_external`].
    pub fn embed<'a>(
        &self,
        application: &'a mut Application,
    ) -> Result<crate::EmbeddedRoot<'a>, Error> {
        crate::gtk::embed::EmbeddedRoot::start(application)
    }
}

impl LinuxPlatform {
    /// Creates the backend, realizing the tree with GTK 4. Equivalent to
    /// [`Default::default`].
    #[must_use]
    pub const fn new() -> Self {
        Self { app_id: None, toolkit: ToolkitKind::Gtk4 }
    }

    /// Identifies the application, which makes it **single-instance**.
    ///
    /// The id is the application's D-Bus name (a reverse domain name,
    /// `com.example.Notes`), which is also its desktop entry's name. A
    /// second launch does not open a second copy: it hands the URL it was
    /// launched with to the running instance as
    /// [`rustnative_core::Event::DeepLink`] and exits.
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

    /// Realizes the tree with `toolkit` instead of the default.
    #[must_use]
    pub const fn with_toolkit(mut self, toolkit: ToolkitKind) -> Self {
        self.toolkit = toolkit;
        self
    }

    /// The toolkit this backend realizes the tree with.
    #[must_use]
    pub const fn toolkit(&self) -> ToolkitKind {
        self.toolkit
    }

    /// The session this backend would run in: display server and desktop
    /// environment, as the environment describes them.
    #[must_use]
    pub fn session(&self) -> Session {
        Session::detect()
    }
}

#[cfg(target_os = "linux")]
impl Platform for LinuxPlatform {
    type Error = Error;

    fn run(&mut self, application: &mut Application) -> Result<(), Self::Error> {
        let options = crate::toolkit::RunOptions {
            launch_url: crate::toolkit::launch_url(std::env::args()),
            app_id: self.app_id.clone(),
        };
        crate::toolkit::run(self.toolkit, application, &options, &Session::detect())
    }

    fn style_capabilities(&self) -> rustnative_core::StyleCapabilities {
        rustnative_style::LINUX
    }

    fn unit_mapping(&self) -> Option<rustnative_core::UnitMapping> {
        Some(rustnative_style::LINUX_UNITS)
    }

    fn capabilities(&self) -> PlatformCapabilities {
        crate::toolkit::capabilities(self.toolkit, &Session::detect())
    }

    fn native_extension(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(not(target_os = "linux"))]
impl Platform for LinuxPlatform {
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
