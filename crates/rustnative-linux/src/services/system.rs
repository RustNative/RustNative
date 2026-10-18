//! URL launching and notifications (`rustnative_core::SystemService`).

use rustnative_core::{ServiceError, SystemService, UrlPolicy};

use super::on_gtk;
use crate::desktop::{notifications, open_uri};

/// The desktop's URL handlers and notification server. A URL reaches the
/// OpenURI portal or GIO's default handler only once its [`UrlPolicy`]
/// allows it (by default `http`, `https`, and `mailto`).
#[derive(Debug, Default, Clone)]
pub struct LinuxSystem {
    application: String,
    url_policy: UrlPolicy,
}

impl LinuxSystem {
    /// Notifications sent as `application` (its display name).
    #[must_use]
    pub fn named(application: impl Into<String>) -> Self {
        Self { application: application.into(), url_policy: UrlPolicy::default() }
    }

    /// This service, opening the URLs `policy` allows.
    #[must_use]
    pub fn with_url_policy(mut self, policy: UrlPolicy) -> Self {
        self.url_policy = policy;
        self
    }
}

#[async_trait::async_trait]
impl SystemService for LinuxSystem {
    async fn open_url(&self, url: String) -> Result<(), ServiceError> {
        // Refused here as well as in `open_uri::open`, so a refused URL
        // never waits on the GTK thread.
        self.url_policy.check(&url)?;
        let policy = self.url_policy.clone();
        on_gtk(move || async move {
            open_uri::open(&url, &policy).await.map(|_| ()).map_err(ServiceError::new)
        })
        .await
    }

    async fn notify(&self, title: String, body: String) -> Result<(), ServiceError> {
        let application = self.application.clone();
        on_gtk(move || async move {
            notifications::notify(&application, &title, &body, None)
                .await
                .map(|_| ())
                .map_err(ServiceError::new)
        })
        .await
    }
}
