//! The draw-list platform group (`docs/conformance/platform-groups.md`).
//!
//! Some hosts have no native controls: a terminal's host object is its cell
//! grid (`PLAN.md` Milestone 38), an embedded display's is its framebuffer
//! (Milestone 37), and the inspection overlay draws over whatever the host
//! is (Milestone 44). What those hosts share lives here, once, rather than
//! in each backend:
//!
//! | Module | For |
//! |---|---|
//! | [`grid`] | logical pixels ↔ the host's grid (cells or device pixels), with one rounding rule |
//! | [`damage`] | which parts of the grid changed, coalesced into few rectangles |
//! | [`focus`] | keyboard traversal and hit-testing for drawn widgets |
//! | [`raster`] | a [`rustnative_core::DrawList`] rasterized onto any grid |
//!
//! The crate depends on `rustnative-core` alone: a host member adds its own
//! I/O and nothing here knows which host it is.
#![deny(missing_docs)]

pub mod damage;
pub mod focus;
pub mod grid;
pub mod raster;

pub use damage::Region;
pub use focus::{FocusModel, hit_test};
pub use grid::{GridMapping, GridRect};
pub use raster::{Edges, RasterTarget, Rasterized, rasterize};
