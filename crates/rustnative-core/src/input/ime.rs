//! IME composition and clipboard-action payloads.

/// A step in an input-method composition session — how CJK text, emoji
/// pickers, and dead-key sequences are entered before they become
/// committed text.
///
/// Native text controls run their own composition; a backend delivers
/// these only for focus targets that do not (for example a custom canvas
/// editor), so the component can draw the in-progress text itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Composition {
    /// A composition session began.
    Started,
    /// The in-progress (uncommitted) text changed.
    Updated {
        /// The whole current composition string.
        text: String,
        /// The caret position within `text`, in `char`s.
        cursor: usize,
    },
    /// The session ended by committing `text`.
    Committed {
        /// The committed text.
        text: String,
    },
    /// The session ended without committing anything.
    Cancelled,
}

/// A clipboard operation the person performed on the focused node.
///
/// This is a notification, not a request: for a native text control the
/// control performs the operation itself, and this event tells the
/// component it happened. For a node with no native clipboard behavior
/// (a canvas, a custom list), it is the component's cue to act.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardAction {
    /// Copy (Ctrl+C / Ctrl+Insert).
    Copy,
    /// Cut (Ctrl+X / Shift+Delete).
    Cut,
    /// Paste (Ctrl+V / Shift+Insert), carrying the clipboard's plain text
    /// at the moment of pasting, or `None` if it held no text.
    Paste {
        /// The clipboard's text content.
        text: Option<String>,
    },
}
