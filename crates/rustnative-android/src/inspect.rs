//! Inspection (Milestone 44) on Android (Phase 7 of the Android plan fills
//! this in).

use rustnative_core::WindowId;

use crate::registry::WindowRegistry;

/// Answers the inspector's pending requests; whether any changed the tree.
pub(crate) fn poll(_registry: &mut WindowRegistry, _window: WindowId) -> bool {
    false
}
