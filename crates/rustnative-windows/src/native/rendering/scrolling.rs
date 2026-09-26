//! Scroll offsets and the viewport transform that realizes them.
//!
//! Scrolling is deliberately *not* part of layout. A scrollable container
//! is realized as two nested native windows — a fixed-size viewport and a
//! content host sized to the full content — and scrolling moves only the
//! content host inside the viewport. That keeps a scroll a pure native
//! transform: no component rerender, no reconciliation, no layout pass, no
//! matter how far or how fast the person scrolls.
//!
//! This module owns the state that makes that work (where each container is
//! scrolled to, and how far it *can* scroll) and nothing else. It knows
//! about `HWND`s and `NodeId`s but not about controls, styles, or the
//! component tree — see this subsystem's `mod.rs` for why the boundary is
//! drawn here.

use std::collections::HashMap;

use rustnative_core::{NodeId, Point, Rect, Size};
use windows_sys::Win32::UI::WindowsAndMessaging::{SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos};

use super::super::registry::{NativeObject, NativeObjectRegistry};
use super::super::win32::best_effort;

/// Converts a Win32 `RECT`-derived dimension (`i32`, and documented by
/// every caller here to already be non-negative in the cases that matter)
/// to the unsigned `Size`/scroll-range representation this framework's
/// layout types use. A negative input — which should not occur for a real
/// window's client-area dimensions — is treated defensively as zero rather
/// than panicking or wrapping.
#[allow(clippy::cast_sign_loss)]
pub(crate) fn dimension_to_u32(value: i32) -> u32 {
    value.max(0) as u32
}

/// The inverse of [`dimension_to_u32`]: saturates at `i32::MAX` rather than
/// wrapping if a `u32` dimension from this framework's layout types
/// somehow exceeded it, which no real window or scroll range legitimately
/// does.
#[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
pub(crate) fn dimension_to_i32(value: u32) -> i32 {
    value.min(i32::MAX as u32) as i32
}

/// Per-container scroll state: how far each scrollable container is
/// scrolled, how far it may scroll, and how large its content is.
#[derive(Debug, Default)]
pub(crate) struct ScrollState {
    offsets: HashMap<NodeId, Point>,
    ranges: HashMap<NodeId, Size>,
    content_sizes: HashMap<NodeId, Size>,
}

impl ScrollState {
    /// Replaces the ranges and content sizes a layout pass just produced,
    /// then reconciles existing offsets against them.
    ///
    /// Offsets survive layout rather than being recomputed by it: they are
    /// runtime viewport state, not layout output, so a relayout caused by
    /// (say) a text change must not jump a scrolled container back to the
    /// top. They are clamped to the new ranges instead, and dropped
    /// entirely for containers that are gone or no longer scrollable.
    pub(crate) fn adopt_layout(
        &mut self,
        ranges: HashMap<NodeId, Size>,
        content_sizes: HashMap<NodeId, Size>,
        still_present: impl Fn(NodeId) -> bool,
    ) {
        self.ranges = ranges;
        self.content_sizes = content_sizes;
        self.offsets.retain(|id, _| still_present(*id));
        for (id, offset) in &mut self.offsets {
            if let Some(range) = self.ranges.get(id) {
                offset.x = offset.x.clamp(0, dimension_to_i32(range.width));
                offset.y = offset.y.clamp(0, dimension_to_i32(range.height));
            } else {
                offset.x = 0;
                offset.y = 0;
            }
        }
    }

    /// This container's current scroll offset, defaulting to the origin.
    pub(crate) fn offset(&self, id: NodeId) -> Point {
        self.offsets.get(&id).copied().unwrap_or_default()
    }

    /// Puts `id` at `offset`, clamped to its scroll range, and reports
    /// whether the offset actually changed.
    ///
    /// Used by scroll anchoring, which computes where a container has to be
    /// for the item a person is looking at to stay where it is (see
    /// `rendering::virtual_list`).
    pub(crate) fn scroll_to(&mut self, id: NodeId, offset: Point) -> bool {
        let range = self.ranges.get(&id).copied().unwrap_or(Size::new(0, 0));
        let next = Point::new(
            offset.x.clamp(0, dimension_to_i32(range.width)),
            offset.y.clamp(0, dimension_to_i32(range.height)),
        );
        if next == self.offset(id) {
            return false;
        }
        self.offsets.insert(id, next);
        true
    }

    /// Moves `id` by `(delta_x, delta_y)`, clamped to its scroll range,
    /// and reports whether the offset actually changed.
    pub(crate) fn scroll_by(&mut self, id: NodeId, delta_x: i32, delta_y: i32) -> bool {
        let range = self.ranges.get(&id).copied().unwrap_or(Size::new(0, 0));
        let current = self.offset(id);
        let next = Point::new(
            current.x.saturating_add(delta_x).clamp(0, dimension_to_i32(range.width)),
            current.y.saturating_add(delta_y).clamp(0, dimension_to_i32(range.height)),
        );
        if next == current {
            return false;
        }
        self.offsets.insert(id, next);
        true
    }

    /// The size the content host should be given for `id`: its measured
    /// content size, never smaller than the viewport it sits in.
    ///
    /// Falling back to the viewport rectangle when no content size was
    /// recorded keeps a container that has not been measured yet from
    /// collapsing its children to nothing.
    fn content_extent(&self, id: NodeId, viewport: Rect) -> (i32, i32) {
        let viewport_size =
            Size::new(dimension_to_u32(viewport.width), dimension_to_u32(viewport.height));
        let content = self.content_sizes.get(&id).copied().unwrap_or(viewport_size);
        (
            dimension_to_i32(content.width.max(viewport_size.width)),
            dimension_to_i32(content.height.max(viewport_size.height)),
        )
    }

    /// Applies `id`'s scroll offset to its native content host.
    ///
    /// Only the content host moves; the viewport stays exactly where layout
    /// put it. That is what makes a scroll invisible to the rest of the
    /// framework.
    pub(crate) fn apply(&self, id: NodeId, registry: &NativeObjectRegistry, viewport: Rect) {
        let Some(NativeObject::Container { content, .. }) = registry.get(id) else {
            return;
        };
        let offset = self.offset(id);
        let (width, height) = self.content_extent(id, viewport);

        // SAFETY: `content` is a live HWND owned by this node's registry
        // entry (matched above); a null `hWndInsertAfter` combined with
        // `SWP_NOZORDER` is the documented way to leave z-order untouched,
        // so no window-handle argument beyond `*content` itself is
        // dereferenced by this call.
        let moved = unsafe {
            SetWindowPos(
                *content,
                std::ptr::null_mut(),
                -offset.x,
                -offset.y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        } != 0;
        // Best effort: a failed reposition leaves the content host where it
        // was, so the container shows a stale scroll position until the next
        // scroll or relayout — degraded, but not incorrect, and there is no
        // caller that could act on the failure.
        best_effort(moved, "SetWindowPos(scroll content)", "the viewport shows a stale offset");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_range(id: NodeId, range: Size) -> ScrollState {
        let mut state = ScrollState::default();
        state.adopt_layout(HashMap::from([(id, range)]), HashMap::new(), |_| true);
        state
    }

    #[test]
    fn scrolling_clamps_to_the_range_rather_than_running_past_it() {
        let id = NodeId::from_key("list");
        let mut state = state_with_range(id, Size::new(0, 100));
        assert!(state.scroll_by(id, 0, 250));
        assert_eq!(state.offset(id), Point::new(0, 100), "must stop at the bottom of the range");
        assert!(!state.scroll_by(id, 0, 50), "already clamped, so nothing changed");
    }

    #[test]
    fn scrolling_never_goes_above_the_origin() {
        let id = NodeId::from_key("list");
        let mut state = state_with_range(id, Size::new(0, 100));
        assert!(!state.scroll_by(id, 0, -10), "already at the top");
        assert_eq!(state.offset(id), Point::new(0, 0));
    }

    #[test]
    fn a_pathological_delta_saturates_instead_of_overflowing() {
        let id = NodeId::from_key("list");
        let mut state = state_with_range(id, Size::new(0, 100));
        state.scroll_by(id, 0, 50);
        state.scroll_by(id, i32::MAX, i32::MAX);
        assert_eq!(state.offset(id), Point::new(0, 100));
        state.scroll_by(id, i32::MIN, i32::MIN);
        assert_eq!(state.offset(id), Point::new(0, 0));
    }

    #[test]
    fn a_relayout_clamps_an_existing_offset_into_the_new_range() {
        let id = NodeId::from_key("list");
        let mut state = state_with_range(id, Size::new(0, 500));
        state.scroll_by(id, 0, 400);
        // The container grew, so less of it now overflows.
        state.adopt_layout(HashMap::from([(id, Size::new(0, 120))]), HashMap::new(), |_| true);
        assert_eq!(
            state.offset(id),
            Point::new(0, 120),
            "a shrinking scroll range must pull the offset back in, not leave it out of bounds"
        );
    }

    #[test]
    fn a_container_that_stopped_scrolling_returns_to_the_origin() {
        let id = NodeId::from_key("list");
        let mut state = state_with_range(id, Size::new(0, 500));
        state.scroll_by(id, 0, 400);
        state.adopt_layout(HashMap::new(), HashMap::new(), |_| true);
        assert_eq!(state.offset(id), Point::new(0, 0));
    }

    #[test]
    fn a_removed_container_stops_being_tracked() {
        let id = NodeId::from_key("list");
        let mut state = state_with_range(id, Size::new(0, 500));
        state.scroll_by(id, 0, 400);
        state.adopt_layout(HashMap::new(), HashMap::new(), |_| false);
        assert_eq!(state.offset(id), Point::new(0, 0), "state for a gone node must not linger");
    }
}
