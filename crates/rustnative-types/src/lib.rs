//! `no_std` value types shared by every Rust Native target.
//!
//! `PLAN.md`'s roadmap puts a `no_std`-capable core subset ahead of the
//! embedded and browser backends: the values every layer passes around —
//! geometry, colour, and totally ordered scalars — must not drag an
//! operating system along with them. This crate is that subset. It uses
//! neither `std` nor `alloc`, and `rustnative-core` re-exports every item at
//! its historical path (`rustnative_core::Size`, `rustnative_core::Color`,
//! ...), so moving them here changed no call site.
//!
//! ```
//! use rustnative_types::{Color, EdgeInsets, Rect};
//!
//! let inset = EdgeInsets::symmetric(4, 8);
//! assert_eq!(inset.horizontal(), 16);
//! assert_eq!(Rect::new(0, 0, 10, 10).width, 10);
//! assert_eq!(Color::from(0x00FF00), Color::rgb(0, 255, 0));
//! ```
#![no_std]
#![deny(missing_docs)]

pub mod color;
pub mod geometry;
pub mod scalar;

pub use color::Color;
pub use geometry::{Alignment, EdgeInsets, LayoutDirection, Overflow, Point, Rect, Size, SizeMode};
pub use scalar::Scalar;
