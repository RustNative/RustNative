//! Plain geometric and layout-direction value types.
//!
//! Defined in the `no_std` crate `rustnative-types` so targets without an
//! operating system can share them, and re-exported here at their
//! historical paths.

pub use rustnative_types::geometry::{
    Alignment, EdgeInsets, LayoutDirection, Overflow, Point, Rect, Size, SizeMode,
};
