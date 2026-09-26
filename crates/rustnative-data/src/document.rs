//! The document model (`PLAN.md` Milestone 48, `C27`): open, save, save
//! as, revert, autosave, dirty state, recent documents, per-document undo
//! bound to the standard commands, and external-change detection.
//!
//! A [`DocumentController`] owns one document: its content (as a
//! [`History`], so every edit is a step to undo), the content as last saved
//! (dirty is "differs from that", so undoing back to the saved state is
//! clean again), and the file it came from.
//!
//! Host conventions:
//!
//! - **One window per document.** Each open document gets its own window
//!   (`Application::open_window`); its title is [`DocumentController::title`],
//!   which follows the host's convention for an unsaved change — on Windows
//!   a leading `*`.
//! - **Save on an untitled document asks for a path.** [`DocumentController::save`]
//!   answers [`DocumentError::NeedsPath`]; show the host's save dialog
//!   ([`DocumentController::save_dialog`] with the `FileDialogService`) and
//!   call [`DocumentController::save_as`] with the answer.
//! - **Saving never leaves half a file.** The content is written beside the
//!   target and renamed over it, so a crash mid-save keeps the old file.
//!
//! ```
//! use rustnative_data::document::{DocumentController, DocumentError, TextDocument};
//!
//! let path = std::env::temp_dir().join(format!("doc-example-{}.txt", std::process::id()));
//! let mut document = DocumentController::untitled(TextDocument::default());
//! document.edit(TextDocument("Hello".into()));
//! assert!(document.is_dirty());
//! assert_eq!(document.title("Notes"), "*Untitled - Notes");
//! assert!(matches!(document.save(), Err(DocumentError::NeedsPath)));
//!
//! document.save_as(&path).unwrap();
//! assert!(!document.is_dirty());
//! document.edit(TextDocument("Hello, world".into()));
//! document.undo();
//! assert!(!document.is_dirty(), "undoing to the saved content is clean");
//! # std::fs::remove_file(&path).unwrap();
//! ```

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use rustnative_core::command::standard;
use rustnative_core::{CommandId, FileDialogKind, FileDialogRequest, WindowId};
use serde::{Deserialize, Serialize};

use crate::history::History;

/// What a document is: content that reads from and writes to bytes.
pub trait Document: Clone + PartialEq + 'static {
    /// The file types it opens and saves, for the host's dialogs: a label
    /// and its extensions.
    const FILE_TYPES: &'static [(&'static str, &'static [&'static str])];

    /// Parses a file's bytes.
    ///
    /// # Errors
    ///
    /// Why the bytes are not a document of this type.
    fn read(bytes: &[u8]) -> Result<Self, String>;

    /// The bytes to save.
    fn write(&self) -> Vec<u8>;
}

/// A plain-text document, UTF-8.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextDocument(pub String);

impl Document for TextDocument {
    const FILE_TYPES: &'static [(&'static str, &'static [&'static str])] = &[("Text", &["txt"])];

    fn read(bytes: &[u8]) -> Result<Self, String> {
        String::from_utf8(bytes.to_vec()).map(Self).map_err(|error| error.to_string())
    }

    fn write(&self) -> Vec<u8> {
        self.0.as_bytes().to_vec()
    }
}

/// Why a document operation failed.
#[derive(Debug)]
pub enum DocumentError {
    /// The document has no file yet: ask where to save it.
    NeedsPath,
    /// Reading or writing the file failed.
    Io(io::Error),
    /// The file is not a document of this type.
    Format(String),
}

impl fmt::Display for DocumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NeedsPath => formatter.write_str("the document has not been saved yet"),
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Format(reason) => write!(formatter, "not a readable document: {reason}"),
        }
    }
}

impl std::error::Error for DocumentError {}

impl From<io::Error> for DocumentError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// What changed on disk since the document was opened or saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalChange {
    /// Nothing.
    None,
    /// Another program wrote the file: offer to reload (or [`DocumentController::revert`]).
    Modified,
    /// The file is gone.
    Deleted,
}

/// One open document; see the [module documentation](self).
#[derive(Debug)]
pub struct DocumentController<D: Document> {
    content: History<D>,
    saved: Option<D>,
    path: Option<PathBuf>,
    modified_at: Option<SystemTime>,
    autosave_after: Option<Duration>,
    last_edit: Option<Instant>,
}

impl<D: Document> DocumentController<D> {
    /// A new document that has never been saved.
    pub fn untitled(content: D) -> Self {
        Self {
            content: History::new(content),
            saved: None,
            path: None,
            modified_at: None,
            autosave_after: None,
            last_edit: None,
        }
    }

    /// Opens the document at `path`.
    ///
    /// # Errors
    ///
    /// The file cannot be read, or is not a `D`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DocumentError> {
        let path = path.as_ref();
        let content = D::read(&std::fs::read(path)?).map_err(DocumentError::Format)?;
        let mut document = Self::untitled(content.clone());
        document.saved = Some(content);
        document.path = Some(path.to_path_buf());
        document.modified_at = modified(path);
        Ok(document)
    }

    /// Saves after `delay` without an edit, from [`Self::autosave`].
    #[must_use]
    pub const fn autosave_after(mut self, delay: Duration) -> Self {
        self.autosave_after = Some(delay);
        self
    }

    /// The content now.
    pub fn content(&self) -> &D {
        self.content.present()
    }

    /// The file, once there is one.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Replaces the content, as one step to undo.
    pub fn edit(&mut self, content: D) {
        self.content.record(content);
        self.last_edit = Some(Instant::now());
    }

    /// Whether the content differs from what was last saved.
    pub fn is_dirty(&self) -> bool {
        self.saved.as_ref() != Some(self.content.present())
    }

    /// Undoes one edit; `false` when there is none.
    pub fn undo(&mut self) -> bool {
        self.content.undo()
    }

    /// Redoes one undone edit; `false` when there is none.
    pub fn redo(&mut self) -> bool {
        self.content.redo()
    }

    /// Whether [`Self::undo`] would do anything (the undo command's
    /// enabled state).
    pub fn can_undo(&self) -> bool {
        self.content.can_undo()
    }

    /// Whether [`Self::redo`] would do anything.
    pub fn can_redo(&self) -> bool {
        self.content.can_redo()
    }

    /// Saves to the document's file.
    ///
    /// # Errors
    ///
    /// [`DocumentError::NeedsPath`] for a document never saved; an I/O
    /// failure otherwise, with the old file intact.
    pub fn save(&mut self) -> Result<(), DocumentError> {
        let path = self.path.clone().ok_or(DocumentError::NeedsPath)?;
        self.save_as(path)
    }

    /// Saves to `path`, which becomes the document's file.
    ///
    /// # Errors
    ///
    /// Writing failed; the file at `path`, if any, is unchanged.
    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), DocumentError> {
        let path = path.as_ref();
        let mut staging = path.as_os_str().to_owned();
        staging.push(".saving");
        let staging = PathBuf::from(staging);
        std::fs::write(&staging, self.content.present().write())?;
        if let Err(error) = std::fs::rename(&staging, path) {
            let _ = std::fs::remove_file(&staging);
            return Err(error.into());
        }
        self.saved = Some(self.content.present().clone());
        self.path = Some(path.to_path_buf());
        self.modified_at = modified(path);
        Ok(())
    }

    /// Discards unsaved changes: the content is the file's again (read
    /// anew, so a change another program made is picked up). The revert is
    /// itself a step to undo.
    ///
    /// # Errors
    ///
    /// [`DocumentError::NeedsPath`] for a document never saved, or the
    /// file cannot be read.
    pub fn revert(&mut self) -> Result<(), DocumentError> {
        let path = self.path.clone().ok_or(DocumentError::NeedsPath)?;
        let content = D::read(&std::fs::read(&path)?).map_err(DocumentError::Format)?;
        self.content.record(content.clone());
        self.saved = Some(content);
        self.modified_at = modified(&path);
        Ok(())
    }

    /// Saves if autosave is on, the document is dirty and has a file, and
    /// nothing was edited for the autosave delay. Call it from a timer;
    /// `true` when it saved.
    ///
    /// # Errors
    ///
    /// The save failed.
    pub fn autosave(&mut self, now: Instant) -> Result<bool, DocumentError> {
        let (Some(delay), Some(edited)) = (self.autosave_after, self.last_edit) else {
            return Ok(false);
        };
        if self.path.is_none() || !self.is_dirty() || now.duration_since(edited) < delay {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    /// Whether another program changed or removed the file since it was
    /// opened or saved (poll it when the window is activated).
    pub fn external_change(&self) -> ExternalChange {
        let Some(path) = &self.path else { return ExternalChange::None };
        match (path.exists(), modified(path)) {
            (false, _) => ExternalChange::Deleted,
            (true, now) if now != self.modified_at => ExternalChange::Modified,
            _ => ExternalChange::None,
        }
    }

    /// The window title, by the host's convention: on Windows,
    /// `*name - Application` while there are unsaved changes.
    pub fn title(&self, application: &str) -> String {
        let name = self
            .path
            .as_deref()
            .and_then(Path::file_name)
            .map_or_else(|| "Untitled".to_owned(), |name| name.to_string_lossy().into_owned());
        let dirty = if self.is_dirty() { "*" } else { "" };
        format!("{dirty}{name} - {application}")
    }

    /// The request for the host's save dialog, owned by `window`.
    pub fn save_dialog(&self, window: WindowId) -> FileDialogRequest {
        FileDialogRequest {
            kind: FileDialogKind::SaveFile,
            title: None,
            filters: D::FILE_TYPES
                .iter()
                .map(|(label, extensions)| {
                    (
                        (*label).to_owned(),
                        extensions.iter().map(|&extension| extension.to_owned()).collect(),
                    )
                })
                .collect(),
            owner: Some(window),
        }
    }

    /// Runs a standard command this document answers — undo, redo, save —
    /// and says whether it did. Route the window's `Event::Command` here so
    /// the menu, the shortcut, and a toolbar button all reach the document.
    ///
    /// # Errors
    ///
    /// Saving failed (including [`DocumentError::NeedsPath`]).
    pub fn handle(&mut self, command: CommandId) -> Result<bool, DocumentError> {
        Ok(match command {
            standard::UNDO => self.undo(),
            standard::REDO => self.redo(),
            standard::SAVE => {
                self.save()?;
                true
            }
            _ => false,
        })
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|metadata| metadata.modified()).ok()
}

/// The recently opened documents, newest first, for the host's recent list
/// (on Windows, the File menu's list and the taskbar jump list). It
/// serializes, so it is persisted with the rest of the application's state.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RecentDocuments {
    paths: Vec<PathBuf>,
}

impl RecentDocuments {
    /// How many are kept.
    pub const CAPACITY: usize = 10;

    /// Records `path` as the newest.
    pub fn opened(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        self.paths.retain(|existing| *existing != path);
        self.paths.insert(0, path);
        self.paths.truncate(Self::CAPACITY);
    }

    /// Forgets documents that no longer exist.
    pub fn prune(&mut self) {
        self.paths.retain(|path| path.exists());
    }

    /// The paths, newest first.
    #[must_use]
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("rustnative-document-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        directory.join(name)
    }

    #[test]
    fn open_edit_revert_and_detect_an_external_change() {
        let path = scratch("notes.txt");
        std::fs::write(&path, "first").unwrap();
        let mut document = DocumentController::<TextDocument>::open(&path).unwrap();
        assert_eq!(document.title("Notes"), "notes.txt - Notes");
        document.edit(TextDocument("second".into()));
        assert!(document.is_dirty());
        document.revert().unwrap();
        assert_eq!(document.content().0, "first");
        assert!(!document.is_dirty());
        assert_eq!(document.external_change(), ExternalChange::None);

        // Another program writes the file (with a later timestamp).
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&path, "theirs").unwrap();
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(SystemTime::now() + Duration::from_secs(5)).unwrap();
        assert_eq!(document.external_change(), ExternalChange::Modified);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(document.external_change(), ExternalChange::Deleted);
    }

    #[test]
    fn autosave_waits_for_a_pause_and_commands_reach_the_document() {
        let path = scratch("auto.txt");
        let mut document = DocumentController::untitled(TextDocument::default())
            .autosave_after(Duration::from_secs(2));
        document.edit(TextDocument("a".into()));
        assert!(!document.autosave(Instant::now()).unwrap(), "no file yet");
        document.save_as(&path).unwrap();
        document.edit(TextDocument("ab".into()));
        assert!(!document.autosave(Instant::now()).unwrap(), "still typing");
        assert!(document.autosave(Instant::now() + Duration::from_secs(3)).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "ab");

        assert!(document.handle(standard::UNDO).unwrap());
        assert_eq!(document.content().0, "a");
        assert!(document.handle(standard::SAVE).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a");
        assert!(!document.handle(standard::FIND).unwrap());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn recent_documents_are_newest_first_without_duplicates() {
        let mut recent = RecentDocuments::default();
        for name in ["a", "b", "a"] {
            recent.opened(name);
        }
        assert_eq!(recent.paths(), [PathBuf::from("a"), PathBuf::from("b")]);
        for index in 0..20 {
            recent.opened(format!("f{index}"));
        }
        assert_eq!(recent.paths().len(), RecentDocuments::CAPACITY);
    }
}
