//! Surfaces beyond the main window (`PLAN.md` Milestone 57): a tray or
//! menu-bar extra, a jump list, progress on the taskbar icon, and
//! notifications from them — asked for through [`Surfaces`], realized by
//! each backend that has them (`Capability::Surface`), and answered by
//! [`crate::Event::SurfaceAction`].
//!
//! ```
//! use rustnative_core::surfaces::{SurfaceCommand, Surfaces, TrayMenuItem};
//!
//! let surfaces = Surfaces::default();
//! surfaces.show_tray("Notes", vec![TrayMenuItem::new("new", "New note"), TrayMenuItem::new("quit", "Quit")]);
//! surfaces.set_progress(Some(0.4));
//! // The backend takes what was asked for after each change.
//! assert!(matches!(surfaces.take()[..], [SurfaceCommand::ShowTray { .. }, SurfaceCommand::Progress(Some(_))]));
//! ```
//!
//! A choice in the tray menu arrives as `Event::SurfaceAction` with the
//! item's id; a click on the icon with the action `"activate"`; a click on
//! a notification with `"notification"`. Each goes to the primary window's
//! root component, as a menu choice does.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

/// The action a click on the tray icon itself arrives with.
pub const ACTIVATE: &str = "activate";
/// The action a click on a notification arrives with.
pub const NOTIFICATION: &str = "notification";

/// One entry in the tray icon's menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayMenuItem {
    /// What choosing it arrives as.
    pub id: String,
    /// What the menu shows.
    pub label: String,
}

impl TrayMenuItem {
    /// An item.
    #[must_use]
    pub fn new(id: &str, label: &str) -> Self {
        Self { id: id.to_owned(), label: label.to_owned() }
    }
}

/// A task in the jump list: the application started again with
/// `arguments` (a second launch reaches the running instance as
/// `Event::OpenUrl` when the arguments are a URL, Milestone 30).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpTask {
    /// What the list shows.
    pub label: String,
    /// The command-line arguments.
    pub arguments: String,
}

/// What the application asked of its surfaces.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SurfaceCommand {
    /// Show (or update) the tray icon.
    ShowTray {
        /// Its tooltip.
        tooltip: String,
        /// Its menu.
        menu: Vec<TrayMenuItem>,
    },
    /// Remove the tray icon.
    HideTray,
    /// Replace the jump list's tasks.
    JumpList(Vec<JumpTask>),
    /// Progress on the taskbar icon, 0–1; `None` removes it.
    Progress(Option<f32>),
    /// A notification from the tray icon.
    Notify {
        /// Its title.
        title: String,
        /// Its text.
        body: String,
    },
}

/// The handle through which an application asks for surfaces
/// ([`crate::Services::surfaces`]); cloning shares it.
#[derive(Debug, Clone, Default)]
pub struct Surfaces {
    queue: Arc<Mutex<VecDeque<SurfaceCommand>>>,
}

impl Surfaces {
    fn push(&self, command: SurfaceCommand) {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner).push_back(command);
    }

    /// Shows the tray icon with `tooltip` and `menu`.
    pub fn show_tray(&self, tooltip: &str, menu: Vec<TrayMenuItem>) {
        self.push(SurfaceCommand::ShowTray { tooltip: tooltip.to_owned(), menu });
    }

    /// Removes the tray icon.
    pub fn hide_tray(&self) {
        self.push(SurfaceCommand::HideTray);
    }

    /// Replaces the jump list's tasks.
    pub fn set_jump_list(&self, tasks: Vec<JumpTask>) {
        self.push(SurfaceCommand::JumpList(tasks));
    }

    /// Shows progress (0–1) on the taskbar icon, or removes it.
    pub fn set_progress(&self, progress: Option<f32>) {
        self.push(SurfaceCommand::Progress(progress.map(|value| value.clamp(0.0, 1.0))));
    }

    /// Shows a notification from the tray icon.
    pub fn notify(&self, title: &str, body: &str) {
        self.push(SurfaceCommand::Notify { title: title.to_owned(), body: body.to_owned() });
    }

    /// Everything asked for since the last call — what a backend applies.
    #[must_use]
    pub fn take(&self) -> Vec<SurfaceCommand> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner).drain(..).collect()
    }
}
