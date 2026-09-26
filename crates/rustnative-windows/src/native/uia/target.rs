//! What a UI Automation provider stands for — a node, or a virtual element
//! inside one — and how it reads that node's *current* state.
//!
//! A provider never caches a node's semantics. Assistive technology can
//! hold an element for as long as it likes, and the component tree
//! re-renders underneath it, so every property read and every pattern
//! call resolves the target afresh through the window's `Runtime`. A target
//! whose node (or element) no longer exists answers
//! `UIA_E_ELEMENTNOTAVAILABLE`, which is UI Automation's contract for
//! exactly that situation.

use rustnative_core::{AccessibilityInfo, AccessibleAction, Event, NodeId, NodeKind, Rect};
use windows::Win32::Foundation::E_FAIL;
use windows::core::{Error, HRESULT, Result};
use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;

use super::super::context::with_runtime;
use super::super::runtime::Runtime;

/// `UIA_E_ELEMENTNOTAVAILABLE` (`0x80040201`), as the signed value an
/// `HRESULT` stores.
pub(crate) const UIA_E_ELEMENTNOTAVAILABLE: HRESULT = HRESULT(-2_147_220_991);
/// `UIA_E_INVALIDOPERATION` (`0x80131509`).
pub(crate) const UIA_E_INVALIDOPERATION: HRESULT = HRESULT(-2_146_233_079);

/// A node (and optionally one of its virtual elements), addressed by the
/// top-level window whose `Runtime` owns it.
///
/// Handles are stored as integers because a COM object must not assume
/// which thread drops it; they are only ever used again on the UI thread,
/// inside provider calls that COM marshals there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Target {
    pub(crate) window: usize,
    pub(crate) hwnd: usize,
    pub(crate) node: NodeId,
    pub(crate) element: Option<NodeId>,
}

/// A target's current semantics, read out of the runtime.
#[derive(Debug, Clone)]
pub(crate) struct Resolved {
    /// The accessibility metadata of the node, or of the element.
    pub(crate) info: AccessibilityInfo,
    /// The owning node's kind (an element is always "custom").
    pub(crate) kind: NodeKind,
    /// Whether the owning node is disabled.
    pub(crate) disabled: bool,
    /// The computed accessible name (see `AccessibilityTree::name_of`).
    pub(crate) name: Option<String>,
    /// For an element: its bounds in screen coordinates.
    pub(crate) screen_bounds: Option<Rect>,
}

impl Target {
    /// Whether this target is backed by its own native window (as opposed
    /// to being a virtual element inside one).
    pub(crate) const fn is_native(&self) -> bool {
        self.element.is_none()
    }

    /// The same node, addressed as a different element (or as itself).
    pub(crate) const fn with_element(self, element: Option<NodeId>) -> Self {
        Self { element, ..self }
    }

    /// Runs `f` against the window's runtime, or reports the element as
    /// gone if the window is.
    pub(crate) fn with_runtime<R>(&self, f: impl FnOnce(&mut Runtime) -> Result<R>) -> Result<R> {
        with_runtime(self.window as HWND, f)
            .unwrap_or_else(|| Err(Error::from_hresult(UIA_E_ELEMENTNOTAVAILABLE)))
    }

    /// Reads the target's current semantics.
    pub(crate) fn resolve(&self) -> Result<Resolved> {
        let target = *self;
        self.with_runtime(|runtime| {
            target.resolve_in(runtime).ok_or_else(|| Error::from_hresult(UIA_E_ELEMENTNOTAVAILABLE))
        })
    }

    /// [`Self::resolve`], for a caller already holding the runtime.
    pub(crate) fn resolve_in(&self, runtime: &Runtime) -> Option<Resolved> {
        let node = runtime.renderer.snapshot.get(self.node)?;
        let Some(element_id) = self.element else {
            return Some(Resolved {
                info: node.accessibility.clone(),
                kind: node.kind,
                disabled: node.disabled,
                name: runtime.renderer.accessibility.tree().name_of(self.node),
                screen_bounds: None,
            });
        };
        let element = node.accessibility.find_element(element_id)?;
        let object = runtime.renderer.registry.get(self.node)?;
        let bounds = element.bounds();
        let mut origin = POINT { x: bounds.x, y: bounds.y };
        // SAFETY: `object.hwnd()` is a live HWND from this window's
        // registry; `origin` is a valid, exclusively borrowed `POINT`.
        let converted = unsafe { ClientToScreen(object.hwnd(), &raw mut origin) } != 0;
        Some(Resolved {
            info: element.info().clone(),
            kind: node.kind,
            disabled: node.disabled,
            name: element.info().name_hint().map(ToOwned::to_owned),
            screen_bounds: converted
                .then(|| Rect::new(origin.x, origin.y, bounds.width, bounds.height)),
        })
    }

    /// Delivers an assistive-technology action to the owning component.
    pub(crate) fn act(&self, action: AccessibleAction) -> Result<()> {
        let target = *self;
        self.with_runtime(|runtime| {
            if runtime.dispatch_or_quit(Event::AccessibilityAction {
                target: target.node,
                element: target.element,
                action,
            }) {
                Ok(())
            } else {
                Err(Error::from_hresult(E_FAIL))
            }
        })
    }
}

/// An `Err` meaning "this operation is not available on this element".
pub(crate) fn invalid_operation() -> Error {
    Error::from_hresult(UIA_E_INVALIDOPERATION)
}
