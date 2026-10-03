//! Foreign widgets adopted as leaves of the tree (embedding outward,
//! `PLAN.md` Milestone 40), and the host content built on them (Milestone
//! 48): a widget the framework did not write, created by a factory, then
//! measured, laid out, clipped, and released by the framework's own rules.
//! Its accessibility is its own.
//!
//! | Host content | Widget |
//! |---|---|
//! | Media | `GtkVideo` over a `GtkMediaFile`, played by GTK's media backend (GStreamer) |
//! | Camera | not offered: GTK 4.14 has no camera widget, and the camera portal's stream needs a pipeline the framework does not ship |
//! | Web | not offered: it needs WebKitGTK, which the backend does not depend on |

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk::prelude::*;
use rustnative_core::{HostContent, NodeId, Size};

use crate::{Error, NativeContext};

/// Who owns a foreign widget once the framework has adopted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    /// The framework: removing the node releases the widget, which goes
    /// away with the framework's reference.
    Owned,
    /// The application: removing the node takes the widget out of the tree,
    /// and the application's own reference keeps it alive for reuse.
    Borrowed,
}

/// What a foreign factory hands the framework.
#[derive(Debug, Clone)]
pub struct ForeignWidget {
    /// The widget, not yet in any container.
    pub widget: gtk::Widget,
    /// Who keeps it.
    pub ownership: Ownership,
}

type Create = Rc<dyn Fn() -> Option<ForeignWidget>>;

struct Factory {
    preferred: Size,
    create: Create,
}

thread_local! {
    static FACTORIES: RefCell<HashMap<String, Factory>> = RefCell::new(HashMap::new());
}

/// Registers the factory for foreign nodes of `kind` on this (the GTK)
/// thread. `preferred` is the widget's natural size, which layout uses
/// where the node does not fix one; `create` makes the widget, or returns
/// `None` if it could not.
///
/// ```no_run
/// use rustnative_core::Size;
/// use rustnative_linux::{ForeignWidget, Ownership, register_foreign};
///
/// register_foreign("color-button", Size::new(48, 32), || {
///     Some(ForeignWidget { widget: gtk::ColorDialogButton::new(None).into(), ownership: Ownership::Owned })
/// });
/// ```
pub fn register_foreign(
    kind: impl Into<String>,
    preferred: Size,
    create: impl Fn() -> Option<ForeignWidget> + 'static,
) {
    FACTORIES.with(|factories| {
        factories.borrow_mut().insert(kind.into(), Factory { preferred, create: Rc::new(create) });
    });
}

/// The natural size of a foreign kind, if known.
pub(crate) fn preferred_size(kind: &str) -> Option<Size> {
    if let Some(registered) =
        FACTORIES.with(|factories| factories.borrow().get(kind).map(|factory| factory.preferred))
    {
        return Some(registered);
    }
    match HostContent::from_kind(kind)? {
        HostContent::Media { .. } => Some(Size::new(480, 320)),
        HostContent::Camera { .. } => Some(Size::new(320, 240)),
        HostContent::Web { .. } => Some(Size::new(640, 480)),
    }
}

/// Creates the widget for a node of `kind`.
pub(crate) fn create(kind: &str, node: NodeId) -> Result<ForeignWidget, Error> {
    let context = NativeContext::none().with_node(node);
    // Cloned out so the factory runs without the registry borrowed.
    let registered = FACTORIES
        .with(|factories| factories.borrow().get(kind).map(|factory| Rc::clone(&factory.create)));
    // An application factory registered for the exact kind wins over the
    // backend's own host content.
    let created = match registered {
        Some(create) => create(),
        None => match HostContent::from_kind(kind) {
            Some(hosted) => host_content(&hosted),
            None => {
                return Err(Error::ForeignUnavailable {
                    kind: kind.to_owned(),
                    reason: "no factory is registered",
                    context,
                });
            }
        },
    };
    created.ok_or_else(|| Error::ForeignUnavailable {
        kind: kind.to_owned(),
        reason: "the factory created nothing",
        context,
    })
}

/// The backend's own host content.
fn host_content(content: &HostContent) -> Option<ForeignWidget> {
    match content {
        HostContent::Media { source } => {
            let file = if source.contains("://") {
                gtk::gio::File::for_uri(source)
            } else {
                gtk::gio::File::for_path(source)
            };
            let video = gtk::Video::for_file(Some(&file));
            Some(ForeignWidget { widget: video.upcast(), ownership: Ownership::Owned })
        }
        HostContent::Camera { .. } | HostContent::Web { .. } => None,
    }
}

/// Whether GTK has a media backend to play with — what
/// `Capability::MediaPlayback` is answered from.
pub(crate) fn media_available() -> bool {
    // GTK loads its media backend from its module directory on first use;
    // whether one is installed is a question about that directory.
    // ponytail: the standard multiarch and lib64 layouts only; a prefix
    // install elsewhere reads as "no media" until checked through GIO.
    ["/usr/lib/x86_64-linux-gnu", "/usr/lib/aarch64-linux-gnu", "/usr/lib64", "/usr/lib"]
        .iter()
        .any(|lib| {
            std::fs::read_dir(format!("{lib}/gtk-4.0/4.0.0/media")).is_ok_and(|mut entries| {
                entries.any(|entry| {
                    entry.is_ok_and(|entry| entry.file_name().to_string_lossy().ends_with(".so"))
                })
            })
        })
}
