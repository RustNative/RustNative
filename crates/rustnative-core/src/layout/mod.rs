//! Platform-independent layout: geometry, per-node constraints, intrinsic
//! measurement, and the layout engine itself.
//!
//! This is one of the clearest "feature module" boundaries in the crate
//! (see the standards audit's P1.21/P2.22 findings): everything here is
//! pure data and pure computation over a [`crate::reconcile::TreeSnapshot`].
//! Nothing in this module knows about components, effects, scheduling, or
//! any platform — a platform backend consumes [`engine::LayoutResult`] and
//! applies it to native objects, but this module never reaches back toward
//! the platform.

mod constraints;
mod engine;
mod geometry;
mod grid;
mod measure;

pub use constraints::{ColumnStyle, Constraints, LayoutStyle, RowStyle};
pub use engine::{LayoutEngine, LayoutInvalidation, LayoutResult, MeasuredItem};
pub use geometry::{Alignment, EdgeInsets, LayoutDirection, Overflow, Point, Rect, Size, SizeMode};
pub use grid::{GridPlacement, GridStyle, Track};
pub use measure::{DefaultIntrinsicMeasurer, IntrinsicMeasurer};
