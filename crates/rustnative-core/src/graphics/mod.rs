//! Custom drawing without giving up native controls.
//!
//! The framework's UI is native controls, and stays that way. This module
//! is the explicit, isolated way out for the parts of an application that
//! are genuinely pictures — a chart, a waveform, a game view — in two
//! strengths:
//!
//! - **A canvas** ([`crate::Node::canvas`]) draws a [`DrawList`]: shapes,
//!   paths, text, images, transforms, clips, and opacity layers, described
//!   portably and realized by the platform's own 2D API (Direct2D on
//!   Windows). It is a node like any other: it takes part in layout, it is
//!   diffed (an unchanged list is not redrawn), and pointer input on it
//!   reports which [hit region](DrawList::hit_region) it landed in.
//! - **A native surface** ([`crate::Node::native_surface`]) is a bare
//!   platform window that the framework positions and sizes and never
//!   paints. The application attaches its own GPU swapchain to it, through
//!   the handle the backend gives out (on Windows,
//!   `rustnative_windows::native_surface`), and hears about size changes as
//!   [`crate::Event::SurfaceResized`].
//!
//! Neither is how ordinary UI should be built: a canvas has none of a
//! native control's accessibility, text editing, or theming for free. Use
//! one where a picture is the point.

mod draw_list;
mod geometry;
mod image;
mod path;

pub use draw_list::{DrawCommand, DrawList, Paint};
pub use geometry::{RectF, Transform2D, Vec2};
pub use image::{ImageData, ImageError};
pub use path::{Path, PathSegment};

/// Identifies one native surface to the backend that created it (see
/// [`crate::Event::SurfaceResized`]).
///
/// Opaque and backend-assigned: an application only ever passes it back to
/// the backend that handed it out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SurfaceId(u64);

impl SurfaceId {
    /// Wraps a backend's raw identifier. Only a backend creates these.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw identifier, for the backend that created it.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}
