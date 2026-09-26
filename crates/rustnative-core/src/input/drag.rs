//! Drag-and-drop payloads.

use std::path::PathBuf;

/// What is being dragged over, or dropped onto, a node.
///
/// Carries the portable subset every desktop platform and the Web agree
/// on: a list of files and plain text. Richer formats stay behind the
/// backend's native escape hatch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DragData {
    files: Vec<PathBuf>,
    text: Option<String>,
}

impl DragData {
    /// Drag data carrying nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `self` carrying `files`.
    #[must_use]
    pub fn with_files(mut self, files: impl IntoIterator<Item = PathBuf>) -> Self {
        self.files = files.into_iter().collect();
        self
    }

    /// `self` carrying `text`.
    #[must_use]
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// The dragged files, if any.
    #[must_use]
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// The dragged plain text, if any.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// Whether the drag carries nothing this framework understands.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.text.is_none()
    }
}

/// What a drop target says would happen if the dragged data were dropped
/// on it now. Reported back to the drag source so the platform can show
/// the right cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DropEffect {
    /// The drop would be refused.
    #[default]
    None,
    /// The data would be copied.
    Copy,
    /// The data would be moved.
    Move,
    /// A link to the data would be created.
    Link,
}
