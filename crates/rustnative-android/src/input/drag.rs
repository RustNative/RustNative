//! Drag and drop through `View.OnDragListener` and `startDragAndDrop`:
//! what a component answers a drag with.

use rustnative_core::{InputRequest, WindowId};

use crate::Error;
use crate::registry::WindowRegistry;

/// Applies a drag-feedback request (`InputRequest::SetDropEffect`).
#[allow(
    clippy::unnecessary_wraps,
    reason = "its implementation in a later phase of the Android plan can fail"
)]
pub(crate) fn apply_request(
    registry: &mut WindowRegistry,
    window: WindowId,
    request: InputRequest,
) -> Result<(), Error> {
    if let InputRequest::SetDropEffect(effect) = request {
        if let Some(runtime) = registry.windows.get_mut(&window) {
            runtime.input.drop_effect = Some(effect);
        }
    }
    Ok(())
}
