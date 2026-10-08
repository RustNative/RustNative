//! The clipboard, documents through the Storage Access Framework, URLs,
//! notifications, and the share sheet.

use std::sync::atomic::{AtomicI32, Ordering};

use rustnative_core::{
    ClipboardService, FileDialogKind, FileDialogRequest, FileDialogService, ServiceError,
    SystemService,
};

use super::{Answer, abandon, checked, java, off_thread, pending};
use crate::jni_host::{Arg, Class, Ret};

/// The clipboard. Android hands its contents only to the application with
/// input focus: read it from an event handler, not from a background task.
#[derive(Debug, Default, Clone, Copy)]
pub struct AndroidClipboard;

#[async_trait::async_trait]
impl ClipboardService for AndroidClipboard {
    async fn read_text(&self) -> Result<Option<String>, ServiceError> {
        Ok(java(Class::Services, "readClipboard", "()Ljava/lang/String;", &[])?.string())
    }

    async fn write_text(&self, value: String) -> Result<(), ServiceError> {
        java(Class::Services, "writeClipboard", "(Ljava/lang/String;)V", &[Arg::Str(&value)])
            .map(drop)
    }
}

/// Documents through the Storage Access Framework. The answer is a
/// `content://` URI, not a path: Android's scoped storage gives the
/// application the document, not the file system. Read and write it with
/// [`read_document`] and [`write_document`]; the grant is persisted, so the
/// URI stays usable across restarts.
#[derive(Debug, Default, Clone, Copy)]
pub struct AndroidFileDialogs;

#[async_trait::async_trait]
impl FileDialogService for AndroidFileDialogs {
    async fn show(&self, request: FileDialogRequest) -> Result<Option<String>, ServiceError> {
        let kind = match request.kind {
            FileDialogKind::OpenFile => 0,
            FileDialogKind::SaveFile => 1,
            FileDialogKind::PickFolder => 2,
        };
        let extensions: Vec<String> = request
            .filters
            .iter()
            .flat_map(|(_, extensions)| extensions.iter())
            .map(|extension| extension.trim_start_matches("*.").trim_start_matches('.').to_owned())
            .filter(|extension| !extension.is_empty() && extension != "*")
            .collect();
        let (token, answer) = pending();
        let title = request.title.unwrap_or_default();
        let started = java(
            Class::Services,
            "pickDocument",
            "(II[Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[Arg::Int(token), Arg::Int(kind), Arg::Strs(&extensions), Arg::Str(&title)],
        )
        .and_then(checked);
        if let Err(error) = started {
            abandon(token);
            return Err(error);
        }
        match answer.await {
            Ok(Answer::Chosen(uri)) => Ok(uri),
            Ok(Answer::Answered) | Err(_) => Ok(None),
        }
    }
}

/// The bytes of a document a file dialog chose (a `content://` URI).
///
/// # Errors
///
/// The document's provider refused to open it.
pub fn read_document(uri: &str) -> Result<Vec<u8>, ServiceError> {
    match java(Class::Services, "readUri", "(Ljava/lang/String;)[B", &[Arg::Str(uri)])? {
        Ret::Bytes(Some(bytes)) => Ok(bytes),
        _ => Err(ServiceError::new(format!("could not read {uri}"))),
    }
}

/// Replaces the contents of a document a file dialog chose.
///
/// # Errors
///
/// The document's provider refused to open or write it.
pub fn write_document(uri: &str, bytes: &[u8]) -> Result<(), ServiceError> {
    checked(java(
        Class::Services,
        "writeUri",
        "(Ljava/lang/String;[B)Ljava/lang/String;",
        &[Arg::Str(uri), Arg::Bytes(bytes)],
    )?)
}

/// URLs, notifications, and the share sheet.
#[derive(Debug, Default)]
pub struct AndroidSystem {
    channel: Option<String>,
}

/// Notification ids, so each notification is its own.
static NOTIFICATION: AtomicI32 = AtomicI32::new(1);

impl AndroidSystem {
    /// Notifications on `channel` (created on first use; its name is what
    /// the person sees in the application's notification settings) rather
    /// than the default one.
    #[must_use]
    pub fn on_channel(channel: impl Into<String>) -> Self {
        Self { channel: Some(channel.into()) }
    }

    /// Posts a notification whose `actions` (id, label) return as
    /// `Event::SurfaceAction` from the tray when tapped; returns its id.
    ///
    /// # Errors
    ///
    /// `POST_NOTIFICATIONS` is not granted (ask for
    /// `Permission::Notifications` first), or notifications are off.
    pub fn notify_with_actions(
        &self,
        title: &str,
        body: &str,
        actions: &[(String, String)],
    ) -> Result<i32, ServiceError> {
        let id = NOTIFICATION.fetch_add(1, Ordering::Relaxed);
        let flat: Vec<String> =
            actions.iter().flat_map(|(id, label)| [id.clone(), label.clone()]).collect();
        let channel = self.channel.clone().unwrap_or_else(|| "rustnative.default".to_owned());
        checked(java(
            Class::Services,
            "notify",
            "(ILjava/lang/String;Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;)Ljava/lang/String;",
            &[Arg::Int(id), Arg::Str(&channel), Arg::Str(title), Arg::Str(body), Arg::Strs(&flat)],
        )?)?;
        Ok(id)
    }

    /// Offers `text` to the share sheet.
    ///
    /// # Errors
    ///
    /// The device has no share sheet.
    pub fn share(&self, text: &str, subject: Option<&str>) -> Result<(), ServiceError> {
        checked(java(
            Class::Services,
            "share",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[Arg::Str(text), Arg::OptStr(subject)],
        )?)
    }
}

#[async_trait::async_trait]
impl SystemService for AndroidSystem {
    async fn open_url(&self, url: String) -> Result<(), ServiceError> {
        off_thread(move || {
            checked(java(
                Class::Services,
                "openUrl",
                "(Ljava/lang/String;)Ljava/lang/String;",
                &[Arg::Str(&url)],
            )?)
        })
        .await?
    }

    async fn notify(&self, title: String, body: String) -> Result<(), ServiceError> {
        self.notify_with_actions(&title, &body, &[]).map(drop)
    }
}
