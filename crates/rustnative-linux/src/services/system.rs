//! URL launching and notifications (`rustnative_core::SystemService`).

use rustnative_core::{ServiceError, SystemService};

use super::on_gtk;
use crate::desktop::{notifications, open_uri};

/// The desktop's URL handlers and notification server.
#[derive(Debug, Default, Clone)]
pub struct LinuxSystem {
    application: String,
}

impl LinuxSystem {
    /// Notifications sent as `application` (its display name).
    #[must_use]
    pub fn named(application: impl Into<String>) -> Self {
        Self { application: application.into() }
    }
}

#[async_trait::async_trait]
impl SystemService for LinuxSystem {
    async fn open_url(&self, url: String) -> Result<(), ServiceError> {
        on_gtk(move || async move { open_uri::open(&url).await.map(|_| ()).map_err(ServiceError::new) }).await
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
