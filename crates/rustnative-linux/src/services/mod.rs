//! The portable service contracts (`rustnative_core::Services`) on Linux.
//!
//! | Service | Linux |
//! |---|---|
//! | [`LinuxClipboard`] | the display's `GdkClipboard` |
//! | [`LinuxFileDialogs`] | `GtkFileDialog` — the FileChooser portal where the desktop provides one, GTK's own dialog otherwise |
//! | [`LinuxSystem`] | the OpenURI portal or GIO's default handler; notifications through the notification server or portal |
//! | [`SoupHttp`] | libsoup 3, with certificate pins checked at the TLS handshake |
//! | [`LinuxPrinting`] | GTK's print operation: CUPS's printers, or a PDF file |
//! | [`LinuxSerial`] | termios on `/dev/tty*` |
//! | [`LinuxLocale`] | glibc's locales: `printf`, `strfmon`, `strftime`, `strcoll`, `towupper` |
//! | [`LinuxSecureStorage`] | the desktop's keyring, through the Secret Service |
//! | [`LinuxPermissions`] | ungated unsandboxed; the Camera and Location portals in a sandbox |
//! | [`LinuxPush`], [`LinuxStore`] | unavailable, saying why: no desktop push service or in-app billing |
//! | [`LinuxConditions`] | GIO's network monitor and UPower |
//! | [`PixbufDecoder`] | gdk-pixbuf's loaders |
//! | [`FileStateStore`] | crash-safe files under `$XDG_STATE_HOME/<app-id>/state` |
//!
//! A service is `Send + Sync` and may be called from any thread; the ones
//! that touch GTK run their GTK half on the GTK thread ([`on_gtk`]) and
//! await it.

mod clipboard;
mod data;
mod dialogs;
mod http;
mod industrial;
mod locale;
mod permissions;
mod product;
mod secure_storage;
mod state_store;
mod system;

use std::future::Future;

pub use clipboard::LinuxClipboard;
pub use data::{LinuxConditions, PixbufDecoder};
pub use dialogs::LinuxFileDialogs;
pub use http::SoupHttp;
pub use industrial::{LinuxPrinting, LinuxSerial};
pub use locale::LinuxLocale;
pub use permissions::LinuxPermissions;
pub use product::{LinuxPush, LinuxStore};
use rustnative_core::ServiceError;
pub use secure_storage::LinuxSecureStorage;
pub use state_store::FileStateStore;
pub use system::LinuxSystem;

/// Runs `work` on the GTK thread — the default main context's owner — and
/// awaits its result from wherever the caller is.
pub(crate) async fn on_gtk<R, F, W>(work: W) -> Result<R, ServiceError>
where
    W: FnOnce() -> F + Send + 'static,
    F: Future<Output = Result<R, ServiceError>> + 'static,
    R: Send + 'static,
{
    let (reply, answer) = tokio::sync::oneshot::channel();
    gtk::glib::MainContext::default().invoke(move || {
        gtk::glib::MainContext::default().spawn_local(async move {
            let _ = reply.send(work().await);
        });
    });
    answer.await.map_err(|_| ServiceError::new("the GTK main loop ended before answering"))?
}

/// Runs `work` on the GTK thread and waits for it — directly when this is
/// the GTK thread — for the synchronous service contracts.
pub(crate) fn on_gtk_blocking<R, W>(work: W) -> Result<R, ServiceError>
where
    W: FnOnce() -> Result<R, ServiceError> + Send + 'static,
    R: Send + 'static,
{
    let context = gtk::glib::MainContext::default();
    if context.is_owner() {
        return work();
    }
    let (reply, answer) = std::sync::mpsc::channel();
    context.invoke(move || {
        let _ = reply.send(work());
    });
    answer
        .recv_timeout(std::time::Duration::from_secs(60))
        .map_err(|_| ServiceError::new("the GTK main loop is not running, or did not answer"))?
}
