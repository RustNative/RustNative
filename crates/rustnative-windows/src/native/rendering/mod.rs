//! Turning a `TreeDiff` into native Win32 objects, and keeping those
//! objects' geometry, appearance, and semantics current.
//!
//! # Why this is five modules rather than one type
//!
//! This subsystem used to be a single `Renderer` in one file, which the
//! standards audit's P1.21 finding singles out as "the clearest 'god
//! object'" in the backend: one type that simultaneously applied tree
//! operations, created and destroyed HWNDs, ran layout, positioned windows,
//! scrolled viewports, realized visual styles, owned GDI handles,
//! synchronized accessibility state, and synchronized interaction state.
//!
//! The audit is explicit that the fix is *not* to split by file size or to
//! give every method its own type ("Approach A — one type/function per
//! responsibility, split aggressively. **Rejected.** This creates
//! indirection without improving cohesion"). What it asks for is that the
//! pieces with genuinely different reasons to change stop sharing one:
//!
//! > changing font caching should not modify accessibility code; changing
//! > layout should not modify HWND creation; changing scroll behavior should
//! > not modify GDI ownership; changing accessibility semantics should not
//! > require understanding native menu construction.
//!
//! So the split follows the change drivers, and each module owns the state
//! its own invariant depends on:
//!
//! | Module | Owns | Changes when |
//! |---|---|---|
//! | [`realization`] | The [`Renderer`] coordinator, the current snapshot, layout rectangles | Reconciliation or layout integration changes |
//! | [`animated`] | Per-frame animated values, held beside the tree | Animation changes |
//! | [`controls`] | Creating, replacing, and destroying each native control kind | The set of supported `NodeKind`s or their Win32 classes changes |
//! | [`styling`] | Resolved styles as owned GDI resources, and the shared brush cache | Theming, painting, or GDI ownership changes |
//! | [`accessibility`] | The portable semantic model's projection onto Win32 | Accessibility semantics change |
//! | [`scrolling`] | Scroll offsets, ranges, and the viewport transform | Scroll behavior changes |
//! | [`virtual_list`] | Each virtual list's measured extents and visible range | Virtualization changes |
//! | [`pool`] | Native windows a virtual list's rows recycle through | Recycling changes |
//! | [`tabs`] | The system tab control behind a tab bar | Tab bar behavior changes |
//!
//! # Dependency direction
//!
//! [`realization`] is the only module here that knows about the others; the
//! other four know nothing about each other and nothing about the
//! `Runtime`, the message loop, or the `Application`. That is the audit's
//! "coordinators orchestrate; they do not implement every subsystem" rule
//! applied one level down from `Runtime`, and it is what lets each of them
//! be tested against a bare `HWND` without standing up a message loop.

pub(crate) mod accessibility;
pub(crate) mod animated;
pub(crate) mod controls;
pub(crate) mod direction;
pub(crate) mod native_controls;
pub(crate) mod pool;
pub(crate) mod realization;
pub(crate) mod scrolling;
pub(crate) mod shape;
pub(crate) mod styling;
pub(crate) mod tabs;
pub(crate) mod virtual_list;

pub(crate) use realization::Renderer;
