//! Milestone 29's escape hatch on Windows: canvases drawn with Direct2D,
//! and native surfaces handed to the application's own renderer.
//!
//! | Module | Owns |
//! |---|---|
//! | [`d2d`] | The one translation from a portable `DrawList` to Direct2D calls |
//! | [`canvas`] | The canvas window class: its draw list, render target, and device-loss recovery |
//! | [`surface`] | The surface window class and the `SurfaceId -> HWND` table |
//!
//! Everything else about these nodes — creation, layout, input, semantics
//! — goes through the same renderer paths every other node does.

pub(crate) mod canvas;
pub(crate) mod d2d;
pub(crate) mod surface;
#[cfg(test)]
mod tests;
