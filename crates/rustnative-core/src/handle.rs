//! The escape-hatch contract (`PLAN.md` 2.6, Milestone 39).
//!
//! 2.6 says an application must be able to reach the host object behind a
//! node when the portable model is not enough. This module is what that
//! means precisely:
//!
//! - **How it is obtained.** From the backend, by node id, on the UI thread
//!   ([`crate::affinity::UiThread`]): on Windows,
//!   `rustnative_windows::native_handle`. Nothing in `rustnative-core` hands
//!   one out, because nothing here knows a host.
//! - **What it is.** A [`NativeHandle`]: the raw host value (an `HWND`, an
//!   `NSView *`, a DOM element reference, as an integer), the node it
//!   belongs to, and the realization generation it was taken at.
//! - **What invalidates it.** The node's native object being destroyed —
//!   unmounted, or recreated because its kind changed. The generation
//!   changes when that happens, so a stale handle is detectable rather than
//!   a dangling pointer. Re-rendering with the same identity does *not*
//!   invalidate it (2.7: native objects are reused while identity is stable).
//! - **What the application may do.** Call host APIs that read the object
//!   or change what the framework does not own: custom drawing on a
//!   surface, host-only properties the portable model has no word for, a
//!   subclass that observes messages. It must not destroy the object,
//!   re-parent it, or change properties the framework applies (text,
//!   enabled state, geometry, style) — the next render would overwrite
//!   them, and the framework's bookkeeping would disagree with the host.
//! - **What the framework guarantees afterwards.** It keeps managing the
//!   object exactly as before, never caches anything the application might
//!   have changed behind its back, and destroys the object on the normal
//!   rules.
//!
//! The states a handle passes through are types (`C21`): a handle is taken
//! [`Unchecked`], and only [`NativeHandle::validate`] — which compares the
//! generation against the backend's current one — yields a [`Live`] handle,
//! the only kind whose raw value can be read.

use std::marker::PhantomData;

use crate::identity::NodeId;

/// Typestate: taken, not yet checked against the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unchecked;

/// Typestate: checked, and the object was alive at that moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Live;

/// A host object behind a node; see the module documentation for the
/// contract. `!Send`: it is only meaningful on the UI thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeHandle<State> {
    raw: isize,
    node: NodeId,
    generation: u64,
    _state: PhantomData<(State, *const ())>,
}

impl NativeHandle<Unchecked> {
    /// A handle to `raw`, the host object realizing `node` at realization
    /// `generation`. Backends construct these; applications receive them.
    #[must_use]
    pub const fn new(raw: isize, node: NodeId, generation: u64) -> Self {
        Self { raw, node, generation, _state: PhantomData }
    }

    /// Checks the handle against the backend's current generation for its
    /// node, yielding a [`Live`] handle if the object it names still exists.
    ///
    /// # Errors
    ///
    /// [`StaleHandle`] if the object was destroyed or recreated since the
    /// handle was taken.
    pub fn validate(
        self,
        current_generation: Option<u64>,
    ) -> Result<NativeHandle<Live>, StaleHandle> {
        if current_generation == Some(self.generation) {
            Ok(NativeHandle {
                raw: self.raw,
                node: self.node,
                generation: self.generation,
                _state: PhantomData,
            })
        } else {
            Err(StaleHandle { node: self.node })
        }
    }
}

impl<State> NativeHandle<State> {
    /// The node this handle belongs to.
    #[must_use]
    pub const fn node(&self) -> NodeId {
        self.node
    }

    /// The realization generation it was taken at.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }
}

impl NativeHandle<Live> {
    /// The raw host value — only readable once validated.
    #[must_use]
    pub const fn raw(&self) -> isize {
        self.raw
    }
}

/// The object a handle named no longer exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaleHandle {
    /// The node whose object was destroyed or recreated.
    pub node: NodeId,
}

impl std::fmt::Display for StaleHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the native object for node {} was destroyed or recreated", self.node.get())
    }
}

impl std::error::Error for StaleHandle {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_validated_handle_exposes_its_raw_value() {
        let handle = NativeHandle::new(0x1234, NodeId::from_key("canvas"), 3);
        let live = handle.validate(Some(3)).expect("same generation");
        assert_eq!(live.raw(), 0x1234);
        assert_eq!(handle.validate(Some(4)), Err(StaleHandle { node: NodeId::from_key("canvas") }));
        assert!(handle.validate(None).is_err(), "no object at all");
    }
}
