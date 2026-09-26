//! The escape hatch on Windows: a node's `HWND`, under the contract in
//! `rustnative_core::handle`.
//!
//! Every native object this backend creates is recorded here — its window,
//! its `HWND`, and a generation that changes whenever the object for that
//! node is destroyed or recreated — by the object registry itself, so the
//! table cannot disagree with what exists. It is thread-local, like every
//! `HWND` this crate owns, and reading it never touches a `Runtime`, so it
//! is safe to call from inside a component's render.

use std::cell::RefCell;
use std::collections::HashMap;

use rustnative_core::{Live, NativeHandle, NodeId, StaleHandle, UiThread, Unchecked, WindowId};

#[derive(Debug, Clone, Copy)]
struct Entry {
    top_level: isize,
    hwnd: isize,
    generation: u64,
}

thread_local! {
    static TABLE: RefCell<HashMap<NodeId, Vec<Entry>>> = RefCell::new(HashMap::new());
    static NEXT_GENERATION: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// Records that `hwnd` (whose top-level window is `top_level`) now realizes
/// `node`. Called by the object registry.
#[cfg(windows)]
pub(crate) fn record(node: NodeId, top_level: isize, hwnd: isize) {
    let generation = NEXT_GENERATION.with(|next| {
        let generation = next.get();
        next.set(generation + 1);
        generation
    });
    TABLE.with(|table| {
        let mut table = table.borrow_mut();
        let entries = table.entry(node).or_default();
        entries.retain(|entry| entry.top_level != top_level);
        entries.push(Entry { top_level, hwnd, generation });
    });
}

/// Records that the object realizing `node` under `top_level` is gone.
#[cfg(windows)]
pub(crate) fn forget(node: NodeId, top_level: isize) {
    TABLE.with(|table| {
        let mut table = table.borrow_mut();
        if let Some(entries) = table.get_mut(&node) {
            entries.retain(|entry| entry.top_level != top_level);
            if entries.is_empty() {
                table.remove(&node);
            }
        }
    });
}

fn lookup(window: WindowId, node: NodeId) -> Option<Entry> {
    let top_level = crate::native_window_handle(window)?;
    TABLE.with(|table| {
        table.borrow().get(&node)?.iter().find(|entry| entry.top_level == top_level).copied()
    })
}

/// The `HWND` realizing `node` in `window`, as an unchecked handle.
///
/// `node` is the node's framework identity — what a component's events
/// carry after scoping, and what `Application::view_for` shows — not the
/// key it was written with. Validate it with [`validate_native_handle`]
/// before use; see `rustnative_core::handle` for what may be done with it.
#[must_use]
pub fn native_handle(
    _ui: &UiThread,
    window: WindowId,
    node: NodeId,
) -> Option<NativeHandle<Unchecked>> {
    lookup(window, node).map(|entry| NativeHandle::new(entry.hwnd, node, entry.generation))
}

/// Checks `handle` against the object that realizes its node now.
///
/// # Errors
///
/// [`StaleHandle`] if that object was destroyed or recreated since the
/// handle was taken.
pub fn validate_native_handle(
    _ui: &UiThread,
    window: WindowId,
    handle: NativeHandle<Unchecked>,
) -> Result<NativeHandle<Live>, StaleHandle> {
    let current = lookup(window, handle.node()).map(|entry| entry.generation);
    handle.validate(current)
}
