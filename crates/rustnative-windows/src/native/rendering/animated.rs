//! Where an animation's per-frame values live between the timeline and the
//! native objects.
//!
//! A frame does not change the declarative tree — that is the whole point
//! (see `rustnative_core::animation`) — so the values it produces have to be
//! held *beside* the tree and consulted wherever the backend would
//! otherwise use the rendered value: geometry in `Renderer::position_node`,
//! appearance in `Renderer::apply_control_style`.
//!
//! The type itself is portable, and shared with the other backends:
//! `rustnative_core::AnimatedOverrides`.

pub(crate) use rustnative_core::AnimatedOverrides;
