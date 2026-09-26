//! Right-to-left, realized with Windows' own mirroring.
//!
//! The core's layout rectangles are *logical*: start is always the parent's
//! left edge (`rustnative_core::LayoutResult::physical_rects`). Windows
//! mirrors for itself — a window with `WS_EX_LAYOUTRTL` measures its
//! children's positions from its right edge, draws its own non-client area
//! (title bar, scroll bars) reversed, and its text controls read
//! right-to-left — so this backend hands Windows the logical rectangles and
//! sets the style on each window whose effective direction is
//! right-to-left. Mirroring happens once, in the host, and the core's
//! rectangles are never flipped here as well.
//!
//! Direction is per native object, not per window: a subtree may override
//! it (`LayoutStyle::direction`), and Windows inherits the style into a
//! child only at creation, so every object's style is set explicitly and
//! re-set when it changes. Styles are applied *before* positions, because a
//! parent's mirroring decides how the positions given to its children are
//! read.

use std::collections::HashMap;

use rustnative_core::{LayoutDirection, LayoutResult, NodeId, TreeSnapshot};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi::InvalidateRect;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WS_EX_LAYOUTRTL, WS_EX_RTLREADING,
};

use super::super::registry::NativeObjectRegistry;
use super::super::win32::{best_effort, ignored_by_contract};

/// What was last applied, so only changes touch the native objects.
#[derive(Debug, Default)]
pub(crate) struct DirectionState {
    base: LayoutDirection,
    window_rtl: Option<bool>,
    applied: HashMap<NodeId, bool>,
}

impl DirectionState {
    /// Records the window's base direction; returns whether it changed.
    pub(crate) fn set_base(&mut self, base: LayoutDirection) -> bool {
        let changed = self.base != base;
        self.base = base;
        changed
    }

    /// Sets or clears the right-to-left styles on `window` and every
    /// realized object, as their effective directions require.
    pub(crate) fn apply(
        &mut self,
        window: HWND,
        snapshot: &TreeSnapshot,
        registry: &NativeObjectRegistry,
    ) {
        let window_rtl = self.base.is_rtl();
        if self.window_rtl != Some(window_rtl) {
            set_rtl(window, window_rtl);
            self.window_rtl = Some(window_rtl);
        }
        let directions = LayoutResult::directions(snapshot, self.base);
        self.applied.retain(|id, _| snapshot.contains(*id));
        for (id, direction) in directions {
            let Some(object) = registry.get(id) else { continue };
            let rtl = direction.is_rtl();
            if self.applied.get(&id) == Some(&rtl) {
                continue;
            }
            set_rtl(object.hwnd(), rtl);
            if let Some(content) = object.content_hwnd() {
                set_rtl(content, rtl);
            }
            self.applied.insert(id, rtl);
        }
    }
}

/// Whether `hwnd` currently has `WS_EX_LAYOUTRTL`.
#[cfg(test)]
pub(crate) fn is_rtl(hwnd: HWND) -> bool {
    // SAFETY: `hwnd` is a live window; reading its extended style has no
    // other precondition.
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    style & isize::try_from(WS_EX_LAYOUTRTL).unwrap_or(0) != 0
}

fn set_rtl(hwnd: HWND, rtl: bool) {
    // SAFETY: as in `is_rtl`.
    let current = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    let bits = isize::try_from(WS_EX_LAYOUTRTL | WS_EX_RTLREADING).unwrap_or(0);
    let next = if rtl { current | bits } else { current & !bits };
    if next == current {
        return;
    }
    // SAFETY: `hwnd` is live; writing a style bit mask back to
    // `GWL_EXSTYLE` is the documented way to change it, after which
    // `SWP_FRAMECHANGED` makes the window apply it.
    unsafe {
        ignored_by_contract(SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next));
        let refreshed = SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        ) != 0;
        best_effort(
            refreshed,
            "SetWindowPos(frame changed)",
            "the old direction shows until repaint",
        );
        let invalidated = InvalidateRect(hwnd, std::ptr::null(), 1) != 0;
        best_effort(
            invalidated,
            "InvalidateRect(direction)",
            "the old direction shows until repaint",
        );
    }
}
