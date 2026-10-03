//! Running inside someone else's GTK program (`PLAN.md` Milestone 40):
//!
//! - **guest-runtime mode** ([`ExternalLoop`]): the application's windows
//!   exist and work, but the main loop is the host's — a GTK application
//!   already iterating GLib's default main context, which is all the
//!   framework's work needs;
//! - **embedding inward** ([`EmbeddedRoot`]): the same, with the primary
//!   window realized as a widget the host places in its own widget tree
//!   ([`EmbeddedRoot::widget`]), sized by the host's layout like any other
//!   child; dropping the root removes the framework's widgets and nothing
//!   of the host's.
//!
//! Where the framework would end its own loop (its primary window closing,
//! a failure), it records that instead of quitting a loop that is not its
//! own: see [`ExternalLoop::finished`] and [`ExternalLoop::take_error`].

use std::marker::PhantomData;
use std::rc::Rc;

use gtk::prelude::*;
use rustnative_core::{Application, WindowId};

use super::backend::Backend;
use crate::Error;

/// The application running under the host's main loop.
pub struct ExternalLoop<'a> {
    backend: Rc<Backend>,
    _application: PhantomData<&'a mut Application>,
}

impl std::fmt::Debug for ExternalLoop<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExternalLoop").field("finished", &self.finished()).finish_non_exhaustive()
    }
}

impl<'a> ExternalLoop<'a> {
    pub(crate) fn start(application: &'a mut Application, embedded: bool) -> Result<Self, Error> {
        // A host that already initialized GTK is fine: `init` is idempotent.
        super::app::init()?;
        // SAFETY: `application` is borrowed for `'a`, which this value
        // carries, and `Drop` detaches before that borrow can end.
        let backend = unsafe { Backend::attach(application, embedded) };
        let started = backend.enter(|registry| {
            registry.refresh_host_traits()?;
            registry.sync()
        });
        // Dropping the guest on failure detaches it.
        let guest = Self { backend, _application: PhantomData };
        started?;
        guest.backend.drain();
        Ok(guest)
    }

    /// Whether the application has ended (its primary window closed, or a
    /// failure): the host should drop this and carry on, or exit.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.backend.is_finished()
    }

    /// The failure that ended the application, if one did.
    pub fn take_error(&mut self) -> Option<Error> {
        self.backend.take_error()
    }
}

impl Drop for ExternalLoop<'_> {
    fn drop(&mut self) {
        self.backend.detach();
        // State is on disk before the borrow of the application ends.
        let _ = self.backend.enter(|registry| {
            registry.with_application(|application| {
                application.lifecycle(rustnative_core::Lifecycle::Terminating)
            })
        });
    }
}

/// The application's primary window as a widget in the host's tree.
#[derive(Debug)]
pub struct EmbeddedRoot<'a> {
    guest: ExternalLoop<'a>,
    widget: gtk::Widget,
}

impl<'a> EmbeddedRoot<'a> {
    pub(crate) fn start(application: &'a mut Application) -> Result<Self, Error> {
        let guest = ExternalLoop::start(application, true)?;
        let widget = guest
            .backend
            .enter(|registry| {
                registry
                    .windows
                    .get(&WindowId::PRIMARY)
                    .map(|runtime| runtime.root.clone().upcast::<gtk::Widget>())
            })
            .ok_or(Error::NoDisplay {
                detail: "the application has no primary window to embed".to_owned(),
            })?;
        Ok(Self { guest, widget })
    }

    /// The widget the host places (in a box, a grid, a pane): it takes
    /// whatever size the host's layout gives it, and the application's
    /// tree is laid out at that size.
    #[must_use]
    pub fn widget(&self) -> &gtk::Widget {
        &self.widget
    }

    /// Whether the application has ended.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.guest.finished()
    }

    /// The failure that ended the application, if one did.
    pub fn take_error(&mut self) -> Option<Error> {
        self.guest.take_error()
    }
}

impl Drop for EmbeddedRoot<'_> {
    fn drop(&mut self) {
        // A box gives the widget up here; in any other container the host
        // removes it (GTK has no container-independent removal). The
        // guest's drop then releases everything the framework created.
        if let Some(container) = self.widget.parent().and_downcast::<gtk::Box>() {
            container.remove(&self.widget);
        }
    }
}
