//! The host an application runs on, chosen once (`PLAN.md` 2.5).
//!
//! An application should ask for capabilities, never for a platform. This
//! crate is where the one unavoidable platform decision is made — which
//! native backend realizes the tree for the target being compiled — so no
//! application has to write a `cfg(target_os)` ladder or wire a dozen
//! services by hand:
//!
//! - [`HostPlatform`] is the backend for this target: `WindowsPlatform`,
//!   `LinuxPlatform`, or `AndroidPlatform`. On a target whose backend is
//!   planned but not built (macOS, iOS, iPadOS, terminals, embedded) it is
//!   [`NoBackend`], which compiles and fails at [`Platform::run`] naming the
//!   milestone that brings it — never a different platform's backend.
//! - [`services`] is that backend's full set of services: HTTP, clipboard,
//!   file dialogs and documents, URLs and notifications, the share sheet
//!   and background work where the host has them, crash-safe state,
//!   permissions, locale formatting, printing, serial ports, secure storage,
//!   push, and store billing (answered "unavailable, because…" where the
//!   host or build has none).
//! - [`run`] does both and runs the application.
//!
//! ```no_run
//! use rustnative_core::{Application, Component, Event, Node, Size, Window};
//! # struct Notes;
//! # impl Component for Notes {
//! #     type Props = ();
//! #     type Message = ();
//! #     fn new((): ()) -> Self { Self }
//! #     fn props(&self) -> &() { &() }
//! #     fn set_props(&mut self, (): ()) {}
//! #     fn view(&self) -> Node { Node::label("notes", "Notes") }
//! #     fn update(&mut self, _event: Event) {}
//! # }
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let app = rustnative_host::AppIdentity::new("com.example.notes", "Notes");
//!     let services = rustnative_host::services(&app)?;
//!     let mut application =
//!         Application::with_services(Notes, Window::new("Notes", Size::new(480, 320)), services);
//!     rustnative_host::run(&mut application, &app)?;
//!     Ok(())
//! }
//! ```
//!
//! The browser is not a host here: a web application is pages and islands
//! served by `rustnative-server`, and its capabilities are the browser's
//! (`rustnative_web::capability`). The headless backend is for tests
//! (`rustnative-headless`).

use std::fmt;
use std::sync::Arc;

pub use rustnative_core::Platform;
use rustnative_core::{Application, ServiceError, Services};

/// Who the application is: what single-instance hand-off, the state store's
/// folder, secure storage, and notifications are keyed by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIdentity {
    /// The application's id, a reverse domain name (`com.example.notes`):
    /// `rustnative.toml`'s `app.id`.
    pub id: String,
    /// The name people see (notifications are sent as it).
    pub display_name: String,
}

impl AppIdentity {
    /// The application `id`, shown as `display_name`.
    #[must_use]
    pub fn new(id: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self { id: id.into(), display_name: display_name.into() }
    }
}

#[cfg(target_os = "windows")]
pub use rustnative_windows::WindowsPlatform as HostPlatform;

#[cfg(target_os = "linux")]
pub use rustnative_linux::LinuxPlatform as HostPlatform;

#[cfg(target_os = "android")]
pub use rustnative_android::AndroidPlatform as HostPlatform;

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "android")))]
pub use NoBackend as HostPlatform;

/// The platform for a target with no backend yet: it compiles, so a
/// workspace builds everywhere, and fails at [`Platform::run`] naming the
/// milestone that brings the backend.
#[derive(Debug, Default, Clone)]
pub struct NoBackend {
    app_id: Option<String>,
}

impl NoBackend {
    /// The placeholder platform.
    #[must_use]
    pub const fn new() -> Self {
        Self { app_id: None }
    }

    /// The same signature as every backend's.
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

    /// What this target is, and the milestone that brings its backend.
    #[must_use]
    pub const fn missing() -> &'static str {
        if cfg!(target_os = "macos") {
            "macOS: its AppKit backend is Milestone 33"
        } else if cfg!(target_os = "ios") {
            "iOS and iPadOS: their UIKit backends are Milestones 36 and 70"
        } else if cfg!(target_family = "wasm") {
            "the browser: a web application is served by rustnative-server, not run as a native host"
        } else {
            "this target: terminals are Milestone 38 and embedded targets Milestone 37"
        }
    }
}

/// Why [`NoBackend`] cannot run an application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoBackendError;

impl fmt::Display for NoBackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "no Rust Native backend for {}", NoBackend::missing())
    }
}

impl std::error::Error for NoBackendError {}

impl Platform for NoBackend {
    type Error = NoBackendError;

    fn run(&mut self, _application: &mut Application) -> Result<(), Self::Error> {
        Err(NoBackendError)
    }

    fn native_extension(&self) -> &dyn std::any::Any {
        self
    }
}

/// The backend for this target, identified as `app` (which makes a desktop
/// application single-instance).
#[must_use]
pub fn platform(app: &AppIdentity) -> HostPlatform {
    HostPlatform::new().with_app_id(app.id.clone())
}

/// Runs `application` on this target's backend.
///
/// # Errors
///
/// The backend failed to start or a component panicked under the
/// terminating policy — or, on a target with no backend, [`NoBackendError`].
pub fn run(
    application: &mut Application,
    app: &AppIdentity,
) -> Result<(), <HostPlatform as Platform>::Error> {
    platform(app).run(application)
}

/// Runs the preview catalogue (`rustnative preview`) on this target's
/// backend, starting at preview `first`.
///
/// # Errors
///
/// As [`run`].
pub fn run_catalogue(
    previews: Vec<rustnative_core::preview::Preview>,
    first: &str,
) -> Result<(), <HostPlatform as Platform>::Error> {
    host::run_catalogue(previews, first)
}

/// Every service this target's backend provides, for application `app`.
///
/// # Errors
///
/// The state store's folder could not be created.
pub fn services(app: &AppIdentity) -> Result<Services, ServiceError> {
    host::services(app)
}

/// The host's answers about the device — network reachability and external
/// power — for constrained background work (`rustnative_data::BackgroundWork`).
#[must_use]
pub fn conditions() -> Arc<dyn rustnative_data::Conditions> {
    host::conditions()
}

/// The host's image decoder (`rustnative_data::ImageLoader`).
#[must_use]
pub fn image_decoder() -> Arc<dyn rustnative_data::ImageDecoder> {
    host::image_decoder()
}

#[cfg(target_os = "windows")]
mod host {
    use std::sync::Arc;

    use rustnative_core::{ServiceError, Services};
    use rustnative_windows as backend;

    pub(super) fn services(app: &super::AppIdentity) -> Result<Services, ServiceError> {
        Ok(Services::default()
            .with_http(Arc::new(backend::WinHttp::new()))
            .with_clipboard(Arc::new(backend::WindowsClipboard))
            .with_file_dialogs(Arc::new(backend::WindowsFileDialogs))
            .with_documents(Arc::new(rustnative_core::services::FileDocuments))
            .with_system(Arc::new(backend::WindowsSystem))
            .with_state_store(Arc::new(backend::FileStateStore::for_app(&app.id)?))
            .with_permissions(Arc::new(backend::WindowsPermissions::new()))
            .with_locale_service(Arc::new(backend::WindowsLocale))
            .with_printing(Arc::new(backend::WindowsPrinting))
            .with_serial(Arc::new(backend::WindowsSerial))
            .with_secure_storage(Arc::new(backend::WindowsSecureStorage::new(&app.id)))
            .with_push(Arc::new(backend::WindowsPush))
            .with_commerce(Arc::new(backend::WindowsStore)))
    }

    pub(super) fn conditions() -> Arc<dyn rustnative_data::Conditions> {
        Arc::new(backend::WindowsConditions)
    }

    pub(super) fn image_decoder() -> Arc<dyn rustnative_data::ImageDecoder> {
        Arc::new(backend::WicDecoder)
    }

    pub(super) fn run_catalogue(
        previews: Vec<rustnative_core::preview::Preview>,
        first: &str,
    ) -> Result<(), backend::Error> {
        backend::run_catalogue(previews, first)
    }
}

#[cfg(target_os = "linux")]
mod host {
    use std::sync::Arc;

    use rustnative_core::{ServiceError, Services};
    use rustnative_linux as backend;

    pub(super) fn services(app: &super::AppIdentity) -> Result<Services, ServiceError> {
        Ok(Services::default()
            .with_http(Arc::new(backend::SoupHttp::new()))
            .with_clipboard(Arc::new(backend::LinuxClipboard))
            .with_file_dialogs(Arc::new(backend::LinuxFileDialogs))
            .with_documents(Arc::new(rustnative_core::services::FileDocuments))
            .with_system(Arc::new(backend::LinuxSystem::named(app.display_name.clone())))
            .with_state_store(Arc::new(backend::FileStateStore::for_app(&app.id)?))
            .with_permissions(Arc::new(backend::LinuxPermissions::new()))
            .with_locale_service(Arc::new(backend::LinuxLocale))
            .with_printing(Arc::new(backend::LinuxPrinting))
            .with_serial(Arc::new(backend::LinuxSerial))
            .with_secure_storage(Arc::new(backend::LinuxSecureStorage::new(&app.id)))
            .with_push(Arc::new(backend::LinuxPush))
            .with_commerce(Arc::new(backend::LinuxStore)))
    }

    pub(super) fn conditions() -> Arc<dyn rustnative_data::Conditions> {
        Arc::new(backend::LinuxConditions)
    }

    pub(super) fn image_decoder() -> Arc<dyn rustnative_data::ImageDecoder> {
        Arc::new(backend::PixbufDecoder)
    }

    pub(super) fn run_catalogue(
        previews: Vec<rustnative_core::preview::Preview>,
        first: &str,
    ) -> Result<(), backend::Error> {
        backend::run_catalogue(previews, first)
    }
}

#[cfg(target_os = "android")]
mod host {
    use std::sync::Arc;

    use rustnative_android as backend;
    use rustnative_core::{ServiceError, Services};

    pub(super) fn services(_app: &super::AppIdentity) -> Result<Services, ServiceError> {
        // The package is the identity on Android: its state folder, its
        // Keystore entries, and its notification channels are its own.
        Ok(Services::default()
            .with_http(Arc::new(backend::AndroidHttp::new()))
            .with_clipboard(Arc::new(backend::AndroidClipboard))
            .with_file_dialogs(Arc::new(backend::AndroidFileDialogs))
            .with_documents(Arc::new(backend::AndroidDocuments))
            .with_system(Arc::new(backend::AndroidSystem::default()))
            .with_share(Arc::new(backend::AndroidSystem::default()))
            .with_work(Arc::new(backend::AndroidWork))
            .with_state_store(Arc::new(backend::FileStateStore::for_application()?))
            .with_permissions(Arc::new(backend::AndroidPermissions))
            .with_locale_service(Arc::new(backend::AndroidLocale))
            .with_printing(Arc::new(backend::AndroidPrinting))
            .with_serial(Arc::new(backend::AndroidSerial))
            .with_secure_storage(Arc::new(backend::AndroidSecureStorage))
            .with_push(Arc::new(backend::AndroidPush))
            .with_commerce(Arc::new(backend::AndroidStore)))
    }

    pub(super) fn conditions() -> Arc<dyn rustnative_data::Conditions> {
        Arc::new(backend::AndroidConditions)
    }

    pub(super) fn image_decoder() -> Arc<dyn rustnative_data::ImageDecoder> {
        Arc::new(backend::BitmapDecoder)
    }

    pub(super) fn run_catalogue(
        previews: Vec<rustnative_core::preview::Preview>,
        first: &str,
    ) -> Result<(), backend::Error> {
        backend::run_catalogue(previews, first)
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "android")))]
mod host {
    use std::sync::Arc;

    use rustnative_core::{ServiceError, Services};

    pub(super) fn services(_app: &super::AppIdentity) -> Result<Services, ServiceError> {
        // No backend: the portable services that need no host.
        Ok(Services::default().with_documents(Arc::new(rustnative_core::services::FileDocuments)))
    }

    pub(super) fn conditions() -> Arc<dyn rustnative_data::Conditions> {
        Arc::new(rustnative_data::FixedConditions::default())
    }

    pub(super) fn image_decoder() -> Arc<dyn rustnative_data::ImageDecoder> {
        Arc::new(rustnative_data::PortableDecoder)
    }

    pub(super) fn run_catalogue(
        _previews: Vec<rustnative_core::preview::Preview>,
        _first: &str,
    ) -> Result<(), super::NoBackendError> {
        Err(super::NoBackendError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_target_has_its_own_backend_or_says_which_milestone_brings_it() {
        let platform = platform(&AppIdentity::new("com.example.host", "Host"));
        if cfg!(any(target_os = "windows", target_os = "linux", target_os = "android")) {
            assert!(!std::any::type_name_of_val(&platform).contains("NoBackend"));
        } else {
            assert!(NoBackendError.to_string().contains("Milestone"));
        }
        assert_eq!(platform.app_id(), Some("com.example.host"));
    }

    #[test]
    fn the_default_services_cover_every_contract_the_host_offers() {
        let services = services(&AppIdentity::new("com.example.host-tests", "Host")).unwrap();
        assert!(services.documents().is_some());
        if cfg!(any(target_os = "windows", target_os = "linux", target_os = "android")) {
            assert!(services.http().is_some());
            assert!(services.clipboard().is_some());
            assert!(services.file_dialogs().is_some());
            assert!(services.system().is_some());
            assert!(services.state_store().is_some());
            assert!(services.permissions().is_some());
            assert!(services.secure_storage().is_some());
        }
    }
}
