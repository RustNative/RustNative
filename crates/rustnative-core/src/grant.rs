//! The shape of capability grants (`C68`).
//!
//! 2.5's capabilities answer *does this host have it?* Nothing answered
//! *may this part of the application use it?* A [`GrantSet`] does: services
//! are obtainable through [`crate::Services::scoped`] only to the extent a
//! grant covers them — file access to declared paths, network access to
//! declared origins — so a third-party package receives exactly what it
//! declares. Milestone 39 fixes the shape, because retrofitting it would
//! break every application that relied on ambient access; Milestone 51
//! enforces it at every service call.

use std::path::{Path, PathBuf};

/// One scoped permission to use a service.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Grant {
    /// Files under these directories (or these exact files).
    Paths(Vec<PathBuf>),
    /// Network requests to these origins (`https://api.example.com`).
    Origins(Vec<String>),
    /// Opening and closing windows.
    Windows,
    /// Installing, querying, or launching other packages.
    Packages,
    /// The clipboard.
    Clipboard,
    /// Persisted state.
    StateStore,
    /// A named service an application defines.
    Service(String),
}

/// The grants one part of an application holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GrantSet {
    grants: Vec<Grant>,
    unrestricted: bool,
}

impl GrantSet {
    /// No grants at all.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Every grant — the application's own code, which owns its process.
    #[must_use]
    pub fn unrestricted() -> Self {
        Self { grants: Vec::new(), unrestricted: true }
    }

    /// Adds `grant`.
    #[must_use]
    pub fn with(mut self, grant: Grant) -> Self {
        self.grants.push(grant);
        self
    }

    /// Whether this set is unrestricted.
    #[must_use]
    pub const fn is_unrestricted(&self) -> bool {
        self.unrestricted
    }

    /// Every grant, for diagnostics and generated permission manifests.
    #[must_use]
    pub fn grants(&self) -> &[Grant] {
        &self.grants
    }

    /// Whether `path` may be read or written.
    #[must_use]
    pub fn allows_path(&self, path: &Path) -> bool {
        self.unrestricted
            || self.grants.iter().any(|grant| match grant {
                Grant::Paths(roots) => roots.iter().any(|root| path.starts_with(root)),
                _ => false,
            })
    }

    /// Whether a request to `url` may be made. Compares scheme, host, and
    /// port exactly; a path in the grant is ignored.
    #[must_use]
    pub fn allows_url(&self, url: &str) -> bool {
        self.unrestricted
            || self.grants.iter().any(|grant| match grant {
                Grant::Origins(origins) => {
                    let origin = origin_of(url);
                    origins.iter().any(|allowed| origin_of(allowed) == origin)
                }
                _ => false,
            })
    }

    /// Whether the kind of grant `probe` names is held (for grants without
    /// a scope: windows, packages, clipboard, state store, named services).
    #[must_use]
    pub fn allows(&self, probe: &Grant) -> bool {
        self.unrestricted || self.grants.contains(probe)
    }
}

/// `scheme://host[:port]` of `url`, lower-cased.
fn origin_of(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    format!("{}://{}", scheme.to_ascii_lowercase(), authority.to_ascii_lowercase())
}

/// A service handle obtained through a grant — the typestate `C21` asks
/// for: holding a `Granted<S>` is proof the grant was checked.
#[derive(Debug, Clone)]
pub struct Granted<S: ?Sized> {
    service: std::sync::Arc<S>,
}

impl<S: ?Sized> Granted<S> {
    pub(crate) fn new(service: std::sync::Arc<S>) -> Self {
        Self { service }
    }

    /// The service.
    #[must_use]
    pub fn get(&self) -> &S {
        &self.service
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grants_are_scoped() {
        let grants = GrantSet::none()
            .with(Grant::Paths(vec![PathBuf::from("/data/app")]))
            .with(Grant::Origins(vec!["https://api.example.com".into()]));
        assert!(grants.allows_path(Path::new("/data/app/notes.json")));
        assert!(!grants.allows_path(Path::new("/etc/passwd")));
        assert!(grants.allows_url("https://API.example.com/v1/items?x=1"));
        assert!(!grants.allows_url("https://api.example.com.evil.test/"));
        assert!(
            !grants.allows_url("http://api.example.com/"),
            "a different scheme is a different origin"
        );
        assert!(!grants.allows(&Grant::Clipboard));
        assert!(GrantSet::unrestricted().allows(&Grant::Clipboard));
    }
}

/// An [`crate::services::HttpService`] that refuses requests to origins
/// the scope was not granted — checked at each call, so a redirect target
/// or a URL built at run time gets no further than a literal one.
struct OriginGuard {
    inner: std::sync::Arc<dyn crate::services::HttpService>,
    grants: GrantSet,
}

#[async_trait::async_trait]
impl crate::services::HttpService for OriginGuard {
    async fn execute(
        &self,
        request: crate::services::HttpRequest,
    ) -> Result<crate::services::HttpResponse, crate::services::ServiceError> {
        if !self.grants.allows_url(request.url()) {
            return Err(crate::services::ServiceError::new(format!(
                "not granted: {} is outside this scope's origins",
                origin_of(request.url())
            )));
        }
        self.inner.execute(request).await
    }
}

/// The services one part of an application may reach: what
/// [`crate::Services::scoped`] returns.
#[derive(Debug, Clone)]
pub struct ScopedServices {
    services: crate::services::Services,
    grants: GrantSet,
}

impl ScopedServices {
    pub(crate) fn new(services: crate::services::Services, grants: GrantSet) -> Self {
        Self { services, grants }
    }

    /// The grants this scope holds.
    #[must_use]
    pub const fn grants(&self) -> &GrantSet {
        &self.grants
    }

    /// The clipboard, if granted and present.
    #[must_use]
    pub fn clipboard(&self) -> Option<Granted<dyn crate::services::ClipboardService>> {
        self.grants
            .allows(&Grant::Clipboard)
            .then(|| self.services.clipboard().cloned().map(Granted::new))
            .flatten()
    }

    /// HTTP, if any origin is granted and the service is present. Each
    /// request's origin is checked against the grant when it is made
    /// (Milestone 51).
    #[must_use]
    pub fn http(&self) -> Option<Granted<dyn crate::services::HttpService>> {
        let any_origin = self.grants.is_unrestricted()
            || self.grants.grants().iter().any(|grant| matches!(grant, Grant::Origins(_)));
        let http = any_origin.then(|| self.services.http().cloned()).flatten()?;
        if self.grants.is_unrestricted() {
            return Some(Granted::new(http));
        }
        Some(Granted::new(std::sync::Arc::new(OriginGuard {
            inner: http,
            grants: self.grants.clone(),
        })))
    }

    /// The persisted-state store, if granted and present.
    #[must_use]
    pub fn state_store(&self) -> Option<Granted<dyn crate::persistence::StateStore>> {
        self.grants
            .allows(&Grant::StateStore)
            .then(|| self.services.state_store().cloned().map(Granted::new))
            .flatten()
    }

    /// The clock, which every scope may read: time is not a capability.
    #[must_use]
    pub fn clock(&self) -> std::sync::Arc<dyn crate::clock::Clock> {
        self.services.clock()
    }
}
