//! Pointer cursors: a desktop affordance, answered as a capability.

/// The pointer shape shown over a node (see [`crate::Node::with_cursor`]).
///
/// Named by what it means rather than by how any host draws it; each
/// backend maps it to its host's own cursor, so it looks the way every
/// other application on that host does. A host without a pointer cursor
/// (touch, terminal) does not advertise [`crate::Capability::Cursors`] and
/// ignores it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Cursor {
    /// The host's default arrow.
    #[default]
    Default,
    /// Something activatable — a link, a clickable card.
    Pointer,
    /// Editable or selectable text.
    Text,
    /// Precise selection.
    Crosshair,
    /// Something that can be moved.
    Move,
    /// An action that is not allowed here.
    NotAllowed,
    /// A vertical resize edge.
    ResizeVertical,
    /// A horizontal resize edge.
    ResizeHorizontal,
    /// The application is busy and cannot take input.
    Wait,
    /// Busy, but still taking input.
    Progress,
    /// Contextual help is available.
    Help,
}
