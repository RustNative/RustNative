//! Service contracts for things a host offers beside its windows: the share
//! sheet, background work the host schedules, and documents the person
//! chose (`PLAN.md` 2.5: a component asks for the contract, a backend
//! provides it, and nothing names a platform).

use std::time::Duration;

use super::ServiceError;
use crate::identity::{NodeId, WindowId};

/// What to offer to the host's share sheet.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShareRequest {
    /// The text to share.
    pub text: String,
    /// A subject or title, where the receiving application has one (an
    /// email's subject).
    pub subject: Option<String>,
    /// A link to share alongside the text.
    pub url: Option<String>,
    /// The node the sheet is presented from. Hosts that present sharing as
    /// a popover anchored to its source (iPadOS) require it; others ignore
    /// it.
    pub anchor: Option<NodeId>,
    /// The window the sheet belongs to.
    pub owner: Option<WindowId>,
}

impl ShareRequest {
    /// Sharing `text`.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self { text: text.into(), ..Self::default() }
    }

    /// With `subject`.
    #[must_use]
    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// With a link.
    #[must_use]
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Presented from `node` in window `owner`.
    #[must_use]
    pub const fn from_node(mut self, owner: WindowId, node: NodeId) -> Self {
        self.owner = Some(owner);
        self.anchor = Some(node);
        self
    }
}

/// The host's share sheet ([`crate::Capability::SystemShare`]).
#[async_trait::async_trait]
pub trait ShareService: Send + Sync {
    /// Offers `request` to the share sheet. Returns once the sheet is shown;
    /// what the person does with it is theirs.
    async fn share(&self, request: ShareRequest) -> Result<(), ServiceError>;
}

/// What must hold before background work runs — the contract both mobile
/// hosts impose on it, stated portably.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorkConstraints {
    /// The network is reachable.
    pub network: bool,
    /// The device is on external power.
    pub charging: bool,
    /// Run by this time (on the scheduler's clock) even if the other
    /// constraints never hold.
    pub deadline: Option<Duration>,
}

/// Background work the *host* schedules, so it runs even when the
/// application does not (Android's `JobScheduler`, iOS's background tasks).
///
/// A job is named, and its handler is registered with the backend where
/// every process start reaches it (on Android, the function
/// `export_main!(…, work = …)` names), because a host may start the process
/// for the job alone. In-process work that runs only while the application
/// does is `rustnative_data::BackgroundWork`.
pub trait WorkScheduler: Send + Sync {
    /// Schedules job `name` to run once `constraints` hold (or at their
    /// deadline); scheduling it again replaces the pending one.
    ///
    /// # Errors
    ///
    /// The host refused it.
    fn schedule(&self, name: &str, constraints: WorkConstraints) -> Result<(), ServiceError>;

    /// Cancels job `name` if it is pending.
    ///
    /// # Errors
    ///
    /// The host's scheduler could not be reached.
    fn cancel(&self, name: &str) -> Result<(), ServiceError>;

    /// Whether job `name` is waiting to run.
    fn is_pending(&self, name: &str) -> bool;
}

/// Reading and writing the documents a file dialog chose
/// ([`super::FileDialogService`] answers with their locations: a path on a
/// desktop, a `content://` URI on Android).
#[async_trait::async_trait]
pub trait DocumentService: Send + Sync {
    /// The document's bytes.
    async fn read(&self, location: String) -> Result<Vec<u8>, ServiceError>;
    /// Replaces the document's contents.
    async fn write(&self, location: String, bytes: Vec<u8>) -> Result<(), ServiceError>;
}

/// Documents as files: for every host whose dialogs answer with paths
/// (Windows, Linux, macOS).
///
/// ```
/// # use rustnative_core::services::{DocumentService, FileDocuments};
/// # let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
/// let path = std::env::temp_dir().join("rustnative-file-documents-doc.txt");
/// let location = path.display().to_string();
/// runtime.block_on(async {
///     FileDocuments.write(location.clone(), b"notes".to_vec()).await.unwrap();
///     assert_eq!(FileDocuments.read(location).await.unwrap(), b"notes");
/// });
/// # let _ = std::fs::remove_file(path);
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct FileDocuments;

#[async_trait::async_trait]
impl DocumentService for FileDocuments {
    async fn read(&self, location: String) -> Result<Vec<u8>, ServiceError> {
        std::fs::read(&location).map_err(|error| ServiceError::new(format!("{location}: {error}")))
    }

    async fn write(&self, location: String, bytes: Vec<u8>) -> Result<(), ServiceError> {
        std::fs::write(&location, bytes)
            .map_err(|error| ServiceError::new(format!("{location}: {error}")))
    }
}
