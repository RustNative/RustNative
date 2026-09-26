//! Platform-independent service contracts.
//!
//! Components only ever see [`Services`], a portable registry of optional
//! `Arc<dyn Trait>` contracts — never a platform backend's concrete type —
//! which is what keeps `crate::component` free of any platform dependency.
//! See `memory` for the deterministic in-memory implementations used by
//! tests and previews.

mod memory;

pub use memory::{MemoryClipboard, MemoryStorage};

use std::error::Error;
use std::fmt;
use std::sync::Arc;

use crate::WindowId;

/// An error returned by a platform service implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceError {
    message: String,
}

impl ServiceError {
    /// Creates a service error carrying `message`.
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for ServiceError {}

/// A validated HTTP method. `Other` covers verbs this enum doesn't name yet
/// (e.g. `WebDAV` extensions) without falling back to an unvalidated `String`
/// for the common cases.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Method {
    /// `GET`.
    Get,
    /// `POST`.
    Post,
    /// `PUT`.
    Put,
    /// `DELETE`.
    Delete,
    /// `PATCH`.
    Patch,
    /// `HEAD`.
    Head,
    /// `OPTIONS`.
    Options,
    /// A verb this enum does not name explicitly, carried verbatim.
    Other(String),
}

impl Method {
    /// Returns the method's canonical uppercase HTTP verb string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
            Self::Other(value) => value,
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An outgoing HTTP request.
///
/// Encapsulated (private fields + a constructor/builder) rather than a
/// plain-field struct: unlike this crate's geometry/color value types, a
/// request has a meaningful "this came from `HttpRequest::get`, so its
/// method and body are consistent" shape worth protecting, and it is the
/// kind of type application code constructs but never needs to
/// destructure/pattern-match field-by-field (contrast with
/// `crate::reconcile::TreeNode`, which every backend does need to
/// destructure).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    method: Method,
    url: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpRequest {
    /// Creates a `GET` request to `url` with no headers or body.
    pub fn get(url: impl Into<String>) -> Self {
        Self { method: Method::Get, url: url.into(), headers: Vec::new(), body: Vec::new() }
    }

    /// Creates a request using `method` to `url` with no headers or body.
    pub fn new(method: Method, url: impl Into<String>) -> Self {
        Self { method, url: url.into(), headers: Vec::new(), body: Vec::new() }
    }

    /// Appends one header.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Sets the request body.
    #[must_use]
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    /// Returns the request's method.
    #[must_use]
    pub const fn method(&self) -> &Method {
        &self.method
    }

    /// Returns the request's target URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns the request's headers, in the order they were added.
    #[must_use]
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    /// Returns the request's raw body bytes.
    #[must_use]
    pub fn body_bytes(&self) -> &[u8] {
        &self.body
    }
}

/// A received HTTP response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpResponse {
    /// Creates a response from its status code, headers, and body.
    #[must_use]
    pub const fn new(status: u16, headers: Vec<(String, String)>, body: Vec<u8>) -> Self {
        Self { status, headers, body }
    }

    /// Returns the response's HTTP status code.
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }

    /// Returns the response's headers.
    #[must_use]
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    /// Returns the response's raw body bytes.
    #[must_use]
    pub fn body_bytes(&self) -> &[u8] {
        &self.body
    }
}

/// Executes a single HTTP request.
///
/// `#[async_trait::async_trait]` lets this stay a plain `async fn` in the
/// trait and in every impl below; the macro desugars each into a
/// `fn(...) -> Pin<Box<dyn Future<...> + Send>>` so the trait remains
/// `dyn`-compatible for `Arc<dyn HttpService>` (see [`Services`] below) — no
/// hand-written `Box::pin(async move { ... })` at every call site. This is
/// the one place this crate's service layer takes a dependency on the
/// `async-trait` macro's desugaring choice; see the module-level "why not
/// `ServiceFuture<T>`" note below for the alternative that was considered
/// and rejected.
///
/// (`trait_variant::make` was tried first here and reverted: it desugars
/// `async fn` into `-> impl Future<...> + Send` instead of a boxed future,
/// which is *not* `dyn`-compatible — exactly wrong for a trait this crate
/// needs to store as `Arc<dyn _>`.)
#[async_trait::async_trait]
pub trait HttpService: Send + Sync {
    /// Executes `request` and returns the response, or a [`ServiceError`]
    /// if the request could not be completed.
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ServiceError>;
}

/// Certificate pinning as declared policy (`PLAN.md` Milestone 47, `C34`):
/// for each pinned host, the SHA-256 digests of the certificates it may
/// present. An [`HttpService`] that enforces pins (on Windows,
/// `rustnative_windows::WinHttp`) refuses a connection to a pinned host whose
/// certificate matches none of its digests, before the request body is
/// sent. A host with no pins is checked by the system's trust store alone.
///
/// ```
/// use rustnative_core::CertificatePins;
///
/// let digest = [7_u8; 32];
/// let pins = CertificatePins::new().pin("api.example.com", digest);
/// assert!(pins.allows("api.example.com", &digest));
/// assert!(!pins.allows("api.example.com", &[0; 32]));
/// assert!(pins.allows("other.example.com", &[0; 32]), "unpinned hosts are not restricted");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CertificatePins {
    pins: Vec<(String, [u8; 32])>,
}

impl CertificatePins {
    /// No pins.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Allows `host` to present the certificate whose SHA-256 digest is
    /// `sha256` (among any others pinned for it).
    #[must_use]
    pub fn pin(mut self, host: impl Into<String>, sha256: [u8; 32]) -> Self {
        self.pins.push((host.into().to_ascii_lowercase(), sha256));
        self
    }

    /// Whether `host` has pins.
    #[must_use]
    pub fn is_pinned(&self, host: &str) -> bool {
        self.pins.iter().any(|(pinned, _)| pinned.eq_ignore_ascii_case(host))
    }

    /// Whether `host` may present a certificate with digest `sha256`.
    #[must_use]
    pub fn allows(&self, host: &str, sha256: &[u8; 32]) -> bool {
        !self.is_pinned(host)
            || self
                .pins
                .iter()
                .any(|(pinned, digest)| pinned.eq_ignore_ascii_case(host) && digest == sha256)
    }
}

/// Persistent key-value storage.
#[async_trait::async_trait]
pub trait StorageService: Send + Sync {
    /// Returns the value stored under `key`, or `None` if it has none.
    async fn get(&self, key: String) -> Result<Option<Vec<u8>>, ServiceError>;
    /// Stores `value` under `key`, replacing any existing value.
    async fn set(&self, key: String, value: Vec<u8>) -> Result<(), ServiceError>;
    /// Removes the value stored under `key`, if any.
    async fn remove(&self, key: String) -> Result<(), ServiceError>;
}

/// The system clipboard, restricted to plain text.
#[async_trait::async_trait]
pub trait ClipboardService: Send + Sync {
    /// Returns the clipboard's current plain-text content, if any.
    async fn read_text(&self) -> Result<Option<String>, ServiceError>;
    /// Replaces the clipboard's content with `value`.
    async fn write_text(&self, value: String) -> Result<(), ServiceError>;
}

/// Which native file-dialog affordance to present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileDialogKind {
    /// A dialog for choosing one existing file to open.
    OpenFile,
    /// A dialog for choosing a destination path to save to.
    SaveFile,
    /// A dialog for choosing a folder.
    PickFolder,
}

/// Parameters for one native file-dialog invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDialogRequest {
    /// Which dialog affordance to present.
    pub kind: FileDialogKind,
    /// An optional window title override.
    pub title: Option<String>,
    /// Named filter groups, each a display label paired with the file
    /// extensions it matches (e.g. `("Images", vec!["png", "jpg"])`).
    pub filters: Vec<(String, Vec<String>)>,
    /// The window this dialog is logically modal to, if any.
    ///
    /// Closes the standards audit's Phase 3 roadmap item 15: earlier
    /// versions of this type had no way to carry a window identity through
    /// to a platform backend at all, so every backend's dialog was shown
    /// with no owner regardless of which window the request logically
    /// belonged to (no taskbar grouping under the right window, no
    /// automatic re-enable-on-close relationship, and the dialog could
    /// surface behind its logical parent). A backend that supports window
    /// ownership (`rustnative-windows`'s `IFileDialog::Show`, for example)
    /// resolves this to its native window handle at dialog-creation time;
    /// a backend without a concept of window ownership, or asked to use a
    /// window id it does not recognize, may simply ignore it and show an
    /// unowned dialog rather than treating it as an error — an owner is an
    /// enhancement to the dialog's presentation, not a correctness
    /// requirement the dialog cannot function without.
    pub owner: Option<WindowId>,
}

/// A native open/save/pick-folder file dialog.
#[async_trait::async_trait]
pub trait FileDialogService: Send + Sync {
    /// Shows the dialog `request` describes and returns the chosen path, or
    /// `None` if the person cancelled.
    async fn show(&self, request: FileDialogRequest) -> Result<Option<String>, ServiceError>;
}

/// Shell-level integration: launching URLs and posting system
/// notifications.
#[async_trait::async_trait]
pub trait SystemService: Send + Sync {
    /// Opens `url` with the system's default handler.
    async fn open_url(&self, url: String) -> Result<(), ServiceError>;
    /// Posts a system notification with `title` and `body`.
    async fn notify(&self, title: String, body: String) -> Result<(), ServiceError>;
}

/// Application-owned platform services. Components only receive this portable
/// contract and therefore never need to import a platform backend's API.
///
/// # Why not a `ServiceFuture<T>` type alias
///
/// An earlier version of this module additionally exported
/// `pub type ServiceFuture<T> = Pin<Box<dyn Future<Output = Result<T,
/// ServiceError>> + Send>>`, intended as a hand-written alternative to
/// `#[async_trait::async_trait]`'s desugaring. It was never actually used as
/// a trait signature anywhere — every trait above uses `async fn` through
/// the macro instead — so it was two public async abstractions for the same
/// concept with no call site choosing between them (standards audit P2.26).
/// Removed rather than adopted, since the macro form is what every impl
/// (including `rustnative-windows`'s) already commits to.
///
/// # Example
///
/// An application registers concrete implementations once; components only
/// ever see the portable trait, so nothing in a component tree needs to
/// know which backend is running.
///
/// ```
/// use std::sync::Arc;
///
/// use rustnative_core::{MemoryClipboard, MemoryStorage, Services};
///
/// // `MemoryStorage`/`MemoryClipboard` are the deterministic in-memory
/// // implementations this crate ships for tests and headless use; a real
/// // application registers its platform's instead (for Windows, e.g.
/// // `rustnative_windows::WindowsClipboard`).
/// let services = Services::default()
///     .with_storage(Arc::new(MemoryStorage::default()))
///     .with_clipboard(Arc::new(MemoryClipboard::default()));
///
/// assert!(services.storage().is_some());
/// // A service the host did not register is absent rather than a stub that
/// // silently does nothing, so a component can detect and adapt.
/// assert!(services.http().is_none());
/// ```
#[derive(Clone, Default)]
pub struct Services {
    http: Option<Arc<dyn HttpService>>,
    storage: Option<Arc<dyn StorageService>>,
    clipboard: Option<Arc<dyn ClipboardService>>,
    file_dialogs: Option<Arc<dyn FileDialogService>>,
    system: Option<Arc<dyn SystemService>>,
    state: Option<Arc<dyn crate::persistence::StateStore>>,
    clock: Option<Arc<dyn crate::clock::Clock>>,
    permissions: Option<Arc<dyn crate::permission::PermissionService>>,
    catalogues: Option<Arc<crate::i18n::Catalogues>>,
    locale: Option<Arc<dyn crate::i18n::LocaleService>>,
    printing: Option<Arc<dyn crate::industrial::PrintService>>,
    serial: Option<Arc<dyn crate::industrial::SerialService>>,
    surfaces: crate::surfaces::Surfaces,
    flags: crate::product::Flags,
    secure_storage: Option<Arc<dyn crate::product::SecureStorage>>,
    push: Option<Arc<dyn crate::product::PushService>>,
    commerce: Option<Arc<dyn crate::product::CommerceService>>,
    packages: Arc<std::collections::BTreeMap<String, Arc<dyn std::any::Any + Send + Sync>>>,
}

impl fmt::Debug for Services {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Services")
            .field("http", &self.http.is_some())
            .field("storage", &self.storage.is_some())
            .field("clipboard", &self.clipboard.is_some())
            .field("file_dialogs", &self.file_dialogs.is_some())
            .field("system", &self.system.is_some())
            .field("state", &self.state.is_some())
            .field("clock", &self.clock.is_some())
            .field("permissions", &self.permissions.is_some())
            .field("catalogues", &self.catalogues.is_some())
            .field("locale", &self.locale.is_some())
            .field("printing", &self.printing.is_some())
            .field("serial", &self.serial.is_some())
            .field("secure_storage", &self.secure_storage.is_some())
            .field("push", &self.push.is_some())
            .field("commerce", &self.commerce.is_some())
            .field("surfaces", &self.surfaces)
            .field("flags", &self.flags)
            .field("packages", &self.packages.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Services {
    /// Installs a capability package (Milestone 52): checks it supports
    /// `backend` and this framework version, builds its service from a
    /// scope holding only the grants it declares, and keeps the service
    /// under the package's name in these services — nowhere global.
    ///
    /// # Errors
    ///
    /// The package does not support `backend` or this framework version,
    /// or declined to build here.
    pub fn install<P: crate::package::CapabilityPackage>(
        self,
        package: &P,
        backend: &str,
    ) -> Result<Self, crate::package::PackageError> {
        let manifest = package.manifest();
        manifest.check(backend)?;
        let scope = self.scoped(manifest.grants.clone());
        let service = package.build(scope, backend).ok_or_else(|| {
            crate::package::PackageError::Declined { package: manifest.name.clone() }
        })?;
        let mut packages = (*self.packages).clone();
        packages.insert(manifest.name, Arc::new(service));
        Ok(Self { packages: Arc::new(packages), ..self })
    }

    /// The service a package installed under `name`, if it did.
    #[must_use]
    pub fn package<T: Send + Sync + 'static>(&self, name: &str) -> Option<Arc<T>> {
        self.packages.get(name).cloned().and_then(|service| service.downcast::<T>().ok())
    }

    /// The handle through which the application asks for surfaces beyond
    /// its windows (Milestone 57); always present, realized where the
    /// backend has them.
    #[must_use]
    pub const fn surfaces(&self) -> &crate::surfaces::Surfaces {
        &self.surfaces
    }

    /// Returns `self` with `flags` as the feature flags (to share one
    /// [`crate::product::Flags`] with the code that refreshes it).
    #[must_use]
    pub fn with_flags(mut self, flags: crate::product::Flags) -> Self {
        self.flags = flags;
        self
    }

    /// The feature flags (Milestone 57): compiled defaults until remote
    /// configuration or a local override says otherwise.
    #[must_use]
    pub const fn flags(&self) -> &crate::product::Flags {
        &self.flags
    }

    /// Returns `self` with the secure store set (Milestone 57).
    #[must_use]
    pub fn with_secure_storage(mut self, service: Arc<dyn crate::product::SecureStorage>) -> Self {
        self.secure_storage = Some(service);
        self
    }

    /// Returns the secure store, if any.
    #[must_use]
    pub fn secure_storage(&self) -> Option<&Arc<dyn crate::product::SecureStorage>> {
        self.secure_storage.as_ref()
    }

    /// Returns `self` with remote push set (Milestone 57).
    #[must_use]
    pub fn with_push(mut self, service: Arc<dyn crate::product::PushService>) -> Self {
        self.push = Some(service);
        self
    }

    /// Returns remote push, if any.
    #[must_use]
    pub fn push(&self) -> Option<&Arc<dyn crate::product::PushService>> {
        self.push.as_ref()
    }

    /// Returns `self` with the store's billing set (Milestone 57).
    #[must_use]
    pub fn with_commerce(mut self, service: Arc<dyn crate::product::CommerceService>) -> Self {
        self.commerce = Some(service);
        self
    }

    /// Returns the store's billing, if any.
    #[must_use]
    pub fn commerce(&self) -> Option<&Arc<dyn crate::product::CommerceService>> {
        self.commerce.as_ref()
    }

    /// Returns `self` with the printing service set (Milestone 51).
    #[must_use]
    pub fn with_printing(mut self, service: Arc<dyn crate::industrial::PrintService>) -> Self {
        self.printing = Some(service);
        self
    }

    /// Returns the configured printing service, if any.
    #[must_use]
    pub fn printing(&self) -> Option<&Arc<dyn crate::industrial::PrintService>> {
        self.printing.as_ref()
    }

    /// Returns `self` with the serial-port service set (Milestone 51).
    #[must_use]
    pub fn with_serial(mut self, service: Arc<dyn crate::industrial::SerialService>) -> Self {
        self.serial = Some(service);
        self
    }

    /// Returns the configured serial-port service, if any.
    #[must_use]
    pub fn serial(&self) -> Option<&Arc<dyn crate::industrial::SerialService>> {
        self.serial.as_ref()
    }

    /// Returns `self` with the permission service set.
    #[must_use]
    pub fn with_permissions(
        mut self,
        service: Arc<dyn crate::permission::PermissionService>,
    ) -> Self {
        self.permissions = Some(service);
        self
    }

    /// The permission service, if the host has one.
    #[must_use]
    pub fn permissions(&self) -> Option<&Arc<dyn crate::permission::PermissionService>> {
        self.permissions.as_ref()
    }

    /// These services as seen through `grants`: only what the grants cover
    /// is obtainable (see [`crate::grant`]). How a third-party package is
    /// handed services.
    #[must_use]
    pub fn scoped(&self, grants: crate::grant::GrantSet) -> crate::grant::ScopedServices {
        crate::grant::ScopedServices::new(self.clone(), grants)
    }

    /// Resolves messages from `catalogues` (Milestone 46).
    #[must_use]
    pub fn with_catalogues(mut self, catalogues: Arc<crate::i18n::Catalogues>) -> Self {
        self.catalogues = Some(catalogues);
        self
    }

    /// The message catalogues messages are resolved from (Milestone 46).
    #[must_use]
    pub fn catalogues(&self) -> Option<&Arc<crate::i18n::Catalogues>> {
        self.catalogues.as_ref()
    }

    /// Uses `service` for locale-aware formatting (Milestone 46).
    #[must_use]
    pub fn with_locale_service(mut self, service: Arc<dyn crate::i18n::LocaleService>) -> Self {
        self.locale = Some(service);
        self
    }

    /// Locale-aware formatting: the host's, or — when none was provided —
    /// the deterministic [`crate::i18n::InvariantLocale`].
    #[must_use]
    pub fn locale_service(&self) -> Arc<dyn crate::i18n::LocaleService> {
        self.locale.clone().unwrap_or_else(|| Arc::new(crate::i18n::InvariantLocale))
    }

    /// Returns `self` with the clock set (see [`crate::clock`]).
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn crate::clock::Clock>) -> Self {
        self.clock = Some(clock);
        self
    }

    /// The clock framework code reads "now" from: the one set with
    /// [`Self::with_clock`], or the process's monotonic
    /// [`crate::SystemClock`]. Unlike the other services this is never
    /// absent — every host has a clock.
    #[must_use]
    pub fn clock(&self) -> Arc<dyn crate::clock::Clock> {
        self.clock.clone().unwrap_or_else(|| Arc::new(crate::clock::SystemClock))
    }

    /// Returns `self` with the HTTP service set.
    #[must_use]
    pub fn with_http(mut self, service: Arc<dyn HttpService>) -> Self {
        self.http = Some(service);
        self
    }

    /// Returns `self` with the storage service set.
    #[must_use]
    pub fn with_storage(mut self, service: Arc<dyn StorageService>) -> Self {
        self.storage = Some(service);
        self
    }

    /// Returns `self` with the clipboard service set.
    #[must_use]
    pub fn with_clipboard(mut self, service: Arc<dyn ClipboardService>) -> Self {
        self.clipboard = Some(service);
        self
    }

    /// Returns `self` with the file-dialog service set.
    #[must_use]
    pub fn with_file_dialogs(mut self, service: Arc<dyn FileDialogService>) -> Self {
        self.file_dialogs = Some(service);
        self
    }

    /// Returns `self` with the system service set.
    #[must_use]
    pub fn with_system(mut self, service: Arc<dyn SystemService>) -> Self {
        self.system = Some(service);
        self
    }

    /// Returns the configured HTTP service, if any.
    #[must_use]
    pub fn http(&self) -> Option<&Arc<dyn HttpService>> {
        self.http.as_ref()
    }

    /// Returns the configured storage service, if any.
    #[must_use]
    pub fn storage(&self) -> Option<&Arc<dyn StorageService>> {
        self.storage.as_ref()
    }

    /// Returns `self` with the store persisted component state is kept in
    /// (see [`crate::persistence`]).
    #[must_use]
    pub fn with_state_store(mut self, store: Arc<dyn crate::persistence::StateStore>) -> Self {
        self.state = Some(store);
        self
    }

    /// Returns the configured state store, if any.
    #[must_use]
    pub fn state_store(&self) -> Option<&Arc<dyn crate::persistence::StateStore>> {
        self.state.as_ref()
    }

    /// Returns the configured clipboard service, if any.
    #[must_use]
    pub fn clipboard(&self) -> Option<&Arc<dyn ClipboardService>> {
        self.clipboard.as_ref()
    }

    /// Returns the configured file-dialog service, if any.
    #[must_use]
    pub fn file_dialogs(&self) -> Option<&Arc<dyn FileDialogService>> {
        self.file_dialogs.as_ref()
    }

    /// Returns the configured system service, if any.
    #[must_use]
    pub fn system(&self) -> Option<&Arc<dyn SystemService>> {
        self.system.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_request_builder_round_trips() {
        let request = HttpRequest::get("https://example.test")
            .header("accept", "text/plain")
            .body(b"hi".to_vec());
        assert_eq!(request.method(), &Method::Get);
        assert_eq!(request.url(), "https://example.test");
        assert_eq!(request.headers(), &[("accept".to_owned(), "text/plain".to_owned())]);
        assert_eq!(request.body_bytes(), b"hi");
    }
}
