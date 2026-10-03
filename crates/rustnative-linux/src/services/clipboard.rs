//! The system clipboard: the display's `GdkClipboard`, which speaks the
//! Wayland data-device protocol or X11 selections as the session needs.

use gtk::prelude::*;
use rustnative_core::{ClipboardService, ServiceError};

use super::on_gtk;

/// The system clipboard's plain text.
///
/// ```no_run
/// use std::sync::Arc;
///
/// use rustnative_core::Services;
/// use rustnative_linux::LinuxClipboard;
///
/// let services = Services::default().with_clipboard(Arc::new(LinuxClipboard));
/// ```
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxClipboard;

fn clipboard() -> Result<gtk::gdk::Clipboard, ServiceError> {
    gtk::gdk::Display::default()
        .map(|display| display.clipboard())
        .ok_or_else(|| ServiceError::new("no display is open"))
}

#[async_trait::async_trait]
impl ClipboardService for LinuxClipboard {
    async fn read_text(&self) -> Result<Option<String>, ServiceError> {
        on_gtk(|| async {
            clipboard()?
                .read_text_future()
                .await
                .map(|text| text.map(|text| text.to_string()))
                .map_err(|error| ServiceError::new(format!("reading the clipboard: {error}")))
        })
        .await
    }

    async fn write_text(&self, value: String) -> Result<(), ServiceError> {
        on_gtk(move || async move {
            clipboard()?.set_text(&value);
            Ok(())
        })
        .await
    }
}
