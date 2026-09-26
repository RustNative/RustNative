//! Native windows kept between a removal and the insertion that can reuse
//! them.
//!
//! # Why recycling, and only here
//!
//! Scrolling a virtual list removes the items that left the viewport and
//! inserts the ones that entered it — the same controls, one row over.
//! Creating and destroying a `HWND` per row per scroll step is the most
//! expensive thing a Win32 list can do, and it is pure churn: the window
//! that just left is exactly the window the new row wants.
//!
//! So a removal that belongs to a virtual list parks its native object here
//! instead of destroying it, keyed by the parent it lived under and the
//! kind it is, and the matching insertion takes it back. Anything still
//! parked when the render ends is destroyed: the pool never outlives the
//! render that filled it, so a window is either reused immediately or gone.
//!
//! Only virtual-list items are pooled. Recycling silently changes which
//! node a live `HWND` belongs to, which is safe when the two are
//! interchangeable rows of the same list and is not something to do to the
//! rest of a tree on the chance it helps.
//!
//! # Subtrees
//!
//! A pooled container keeps its own content window, and the pool is keyed
//! by parent handle, so the children removed from it are parked under that
//! same handle and are taken back when the container is. A whole item
//! subtree therefore recycles as a unit, in the order the diff already
//! produces: removals deepest-first, insertions shallowest-first.

use std::collections::HashMap;

use rustnative_core::NodeKind;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::IsWindow;

use super::super::registry::NativeObject;

/// Which parked objects a node can take back: the two container kinds are
/// realized by the same pair of windows, so a row and a column are
/// interchangeable; every other kind is its own window class and is not.
fn slot(kind: NodeKind) -> NodeKind {
    match kind {
        NodeKind::Row => NodeKind::Column,
        other => other,
    }
}

/// Native objects removed during this render that an insertion in the same
/// render may take back.
#[derive(Debug, Default)]
pub(crate) struct ControlPool {
    free: HashMap<(usize, NodeKind), Vec<NativeObject>>,
}

impl ControlPool {
    /// Parks `object`, which lived under `parent`.
    pub(crate) fn put(&mut self, parent: HWND, kind: NodeKind, object: NativeObject) {
        self.free.entry((parent as usize, slot(kind))).or_default().push(object);
    }

    /// Takes back an object of `kind` that lived under `parent`, if one is
    /// parked.
    pub(crate) fn take(&mut self, parent: HWND, kind: NodeKind) -> Option<NativeObject> {
        let key = (parent as usize, slot(kind));
        let objects = self.free.get_mut(&key)?;
        let object = objects.pop();
        if objects.is_empty() {
            self.free.remove(&key);
        }
        object
    }

    /// Destroys everything still parked, ending the render with no window
    /// that belongs to no node.
    ///
    /// A parked window whose parent was itself destroyed is already gone —
    /// Win32 destroys a window's children with it — so each handle is
    /// checked before it is destroyed rather than failing a `DestroyWindow`
    /// that was never going to work.
    pub(crate) fn drain(&mut self) {
        for (_, objects) in self.free.drain() {
            for object in objects {
                // `NativeObject` has no `Drop`, so skipping a dead one
                // releases the handle value and nothing else.
                // SAFETY: `object.hwnd()` is a handle this pool exclusively
                // owns; `IsWindow` accepts any handle value, including one
                // whose window is gone, which is the case being tested for.
                if unsafe { IsWindow(object.hwnd()) } != 0 {
                    object.destroy();
                }
            }
        }
    }

    /// Whether anything is parked.
    pub(crate) fn is_empty(&self) -> bool {
        self.free.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindow;

    use super::*;
    use crate::native::test_support::TestWindow;

    fn parent() -> HWND {
        0x1234 as HWND
    }

    #[test]
    fn a_parked_window_comes_back_for_the_same_parent_and_kind() {
        let window = TestWindow::new();
        let mut pool = ControlPool::default();
        pool.put(parent(), NodeKind::Label, NativeObject::Label(window.hwnd));

        assert!(pool.take(parent(), NodeKind::Button).is_none(), "a button is not a label");
        assert!(pool.take(0x5678 as HWND, NodeKind::Label).is_none(), "nor another list's row");

        let reused = pool.take(parent(), NodeKind::Label).expect("the parked label");
        assert_eq!(reused.hwnd(), window.hwnd, "the same window, not a new one");
        assert!(pool.is_empty(), "taken back, so nothing is left to destroy");
    }

    #[test]
    fn anything_left_parked_at_the_end_of_a_render_is_destroyed() {
        let window = TestWindow::new();
        let hwnd = window.hwnd;
        let mut pool = ControlPool::default();
        pool.put(parent(), NodeKind::Label, NativeObject::Label(hwnd));
        pool.drain();
        // SAFETY: `IsWindow` accepts any handle value, live or not.
        assert!(unsafe { IsWindow(hwnd) } == 0, "a window nothing took back must not leak");
        std::mem::forget(window);
    }
}
