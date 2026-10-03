//! Open, save, and folder dialogs: `GtkFileDialog`, which shows the
//! desktop's own chooser through the FileChooser portal where the session
//! has one (a sandbox, or a desktop that provides it) and GTK's own
//! chooser otherwise — the same dialog every GTK application shows.

use gtk::prelude::*;
use rustnative_core::{FileDialogKind, FileDialogRequest, FileDialogService, ServiceError};

use super::on_gtk;

/// Native file dialogs.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxFileDialogs;

/// The `GtkFileFilter`s a request's filter groups become.
fn filters(groups: &[(String, Vec<String>)]) -> Option<gtk::gio::ListStore> {
    if groups.is_empty() {
        return None;
    }
    let store = gtk::gio::ListStore::new::<gtk::FileFilter>();
    for (label, extensions) in groups {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(label));
        for extension in extensions {
            filter.add_suffix(extension.trim_start_matches("*.").trim_start_matches('.'));
        }
        store.append(&filter);
    }
    Some(store)
}

fn chosen(file: Result<gtk::gio::File, gtk::glib::Error>) -> Result<Option<String>, ServiceError> {
    match file {
        Ok(file) => Ok(file
            .path()
            .map(|path| path.to_string_lossy().into_owned())
            .or_else(|| Some(file.uri().to_string()))),
        // Dismissed or cancelled: the person chose nothing.
        Err(error)
            if error.matches(gtk::DialogError::Dismissed)
                || error.matches(gtk::DialogError::Cancelled) =>
        {
            Ok(None)
        }
        Err(error) => Err(ServiceError::new(format!("the file dialog failed: {error}"))),
    }
}

#[async_trait::async_trait]
impl FileDialogService for LinuxFileDialogs {
    async fn show(&self, request: FileDialogRequest) -> Result<Option<String>, ServiceError> {
        on_gtk(move || async move {
            let dialog = gtk::FileDialog::new();
            dialog.set_modal(true);
            if let Some(title) = &request.title {
                dialog.set_title(title);
            }
            if let Some(filters) = filters(&request.filters) {
                dialog.set_filters(Some(&filters));
            }
            // An owner the backend does not know is shown unowned, as the
            // contract allows.
            let owner = request.owner.and_then(crate::gtk::registry::gtk_window);
            let owner = owner.as_ref();
            match request.kind {
                FileDialogKind::OpenFile => chosen(dialog.open_future(owner).await),
                FileDialogKind::SaveFile => chosen(dialog.save_future(owner).await),
                FileDialogKind::PickFolder => chosen(dialog.select_folder_future(owner).await),
            }
        })
        .await
    }
}
