//! The UI Automation provider objects.
//!
//! Three kinds, because UI Automation treats three shapes of element
//! differently:
//!
//! - [`NodeProvider`] — a node backed by its own window. A *server-side*
//!   provider whose `HostRawElementProvider` is the window's default proxy,
//!   so UI Automation merges the two: this provider's answers where the
//!   portable model says something, the native control's everywhere else.
//! - [`HostProvider`] — the same, for a node that also has virtual elements.
//!   It is additionally a fragment root, whose fragment children are those
//!   elements. (It is a separate type rather than a mode of `NodeProvider`
//!   because UI Automation decides how to navigate an element by which
//!   interfaces it *has*: a window-backed provider that claims to be a
//!   fragment root is navigated through its fragments instead of its child
//!   windows, which would hide an ordinary container's children.)
//! - [`ElementProvider`] — a virtual element: a fragment with no window of
//!   its own, positioned by its bounds inside its node.
//! - [`StructureProvider`] — a window that realizes no node of its own: a
//!   container's inner content window, which exists only to make native
//!   scrolling a window move. It reports itself as neither a control nor
//!   content element, so UI Automation's control view — the one assistive
//!   technology reads — skips it and presents its node's real children (and
//!   virtual elements) directly under the node.

use rustnative_core::{NodeId, Point};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::SAFEARRAY;
use windows::Win32::System::Ole::{SafeArrayCreateVector, SafeArrayPutElement};
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Accessibility::{
    IRawElementProviderFragment, IRawElementProviderFragmentRoot, IRawElementProviderSimple,
    NavigateDirection, NavigateDirection_FirstChild, NavigateDirection_LastChild,
    NavigateDirection_NextSibling, NavigateDirection_Parent, NavigateDirection_PreviousSibling,
    ProviderOptions, ProviderOptions_ServerSideProvider, ProviderOptions_UseComThreading,
    UIA_PATTERN_ID, UIA_PROPERTY_ID, UiaAppendRuntimeId, UiaHostProviderFromHwnd, UiaRect,
};
use windows::core::{IUnknown, Result};

use super::patterns;
use super::properties::{has_native_proxy, value};
use super::target::{Target, invalid_operation};

fn options() -> ProviderOptions {
    // `UseComThreading`: calls reach these objects through COM, which
    // marshals them onto the UI thread that created them (an OLE
    // single-threaded apartment — see `native::app::OleApartment`), so a
    // provider only ever touches the runtime on the thread that owns it.
    ProviderOptions(ProviderOptions_ServerSideProvider.0 | ProviderOptions_UseComThreading.0)
}

fn pattern(target: &Target, id: UIA_PATTERN_ID) -> Result<IUnknown> {
    let resolved = target.resolve()?;
    let native = has_native_proxy(target, &resolved);
    if patterns::supports(&resolved, native, id) {
        patterns::provider(*target, id).ok_or_else(windows::core::Error::empty)
    } else {
        // `NULL` is how a provider says "not supported here", letting UI
        // Automation fall back to the native proxy's pattern, if any.
        Err(windows::core::Error::empty())
    }
}

fn property(target: &Target, id: UIA_PROPERTY_ID) -> Result<VARIANT> {
    let target = *target;
    target.with_runtime(|runtime| {
        let resolved = target.resolve_in(runtime).ok_or_else(|| {
            windows::core::Error::from_hresult(super::target::UIA_E_ELEMENTNOTAVAILABLE)
        })?;
        Ok(value(runtime, &target, &resolved, id).unwrap_or_default())
    })
}

fn host_for(hwnd: usize) -> Result<IRawElementProviderSimple> {
    // SAFETY: `hwnd` is a live window this crate created (a provider for it
    // is only handed out while it exists).
    unsafe { UiaHostProviderFromHwnd(HWND(hwnd as *mut core::ffi::c_void)) }
}

fn host_of(target: &Target) -> Result<IRawElementProviderSimple> {
    // SAFETY: `target.hwnd` is the node's live native window (a provider
    // for a node only exists while its window does; `UiaDisconnectProvider`
    // runs when the node is removed).
    unsafe { UiaHostProviderFromHwnd(HWND(target.hwnd as *mut core::ffi::c_void)) }
}

/// A runtime id for a virtual element: UI Automation's
/// "append to the host's id" marker followed by the element's id.
fn element_runtime_id(element: NodeId) -> Result<*mut SAFEARRAY> {
    // An element id is an interned local key, a small number; its low 32
    // bits identify it uniquely within its node (the host's own runtime id
    // is what UI Automation prefixes, via `UiaAppendRuntimeId`).
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let parts = [UiaAppendRuntimeId as i32, element.get() as i32];
    // SAFETY: a two-element `VT_I4` vector; null is allocation failure.
    let array = unsafe { SafeArrayCreateVector(VT_I4, 0, 2) };
    if array.is_null() {
        return Err(windows::Win32::Foundation::E_OUTOFMEMORY.into());
    }
    for (index, part) in (0_i32..).zip(parts) {
        // SAFETY: `index` is within the two elements just allocated;
        // `part` is copied in.
        unsafe { SafeArrayPutElement(array, &raw const index, (&raw const part).cast()) }?;
    }
    Ok(array)
}

/// See the module documentation.
#[allow(
    clippy::ref_as_ptr,
    clippy::inline_always,
    clippy::wildcard_imports,
    reason = "`windows::core::implement` generates this module's COM plumbing; the module is \
              a thin COM face on its parent and speaks its parent's whole vocabulary"
)]
mod com {
    use super::*;
    use windows::Win32::UI::Accessibility::{
        IRawElementProviderFragment_Impl, IRawElementProviderFragmentRoot_Impl,
        IRawElementProviderSimple_Impl,
    };
    use windows::core::implement;

    // `Agile = false`: `#[implement]` objects are agile by default —
    // they answer `IAgileObject` and aggregate the free-threaded marshaler,
    // so COM hands callers in *other* apartments a direct pointer and they
    // call in on their own threads. This object reaches the window's
    // `Runtime`, which belongs to the UI thread alone, so it must be
    // marshaled into the UI thread's single-threaded apartment instead.
    #[implement(IRawElementProviderSimple, Agile = false)]
    pub(crate) struct NodeProvider {
        pub(crate) target: Target,
    }

    impl IRawElementProviderSimple_Impl for NodeProvider_Impl {
        fn ProviderOptions(&self) -> Result<ProviderOptions> {
            Ok(options())
        }
        fn GetPatternProvider(&self, id: UIA_PATTERN_ID) -> Result<IUnknown> {
            pattern(&self.target, id)
        }
        fn GetPropertyValue(&self, id: UIA_PROPERTY_ID) -> Result<VARIANT> {
            property(&self.target, id)
        }
        fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
            host_of(&self.target)
        }
    }

    // Not agile, for the reason given on `NodeProvider`.
    #[implement(
        IRawElementProviderSimple,
        IRawElementProviderFragment,
        IRawElementProviderFragmentRoot,
        Agile = false
    )]
    pub(crate) struct HostProvider {
        pub(crate) target: Target,
    }

    impl IRawElementProviderSimple_Impl for HostProvider_Impl {
        fn ProviderOptions(&self) -> Result<ProviderOptions> {
            Ok(options())
        }
        fn GetPatternProvider(&self, id: UIA_PATTERN_ID) -> Result<IUnknown> {
            pattern(&self.target, id)
        }
        fn GetPropertyValue(&self, id: UIA_PROPERTY_ID) -> Result<VARIANT> {
            property(&self.target, id)
        }
        fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
            host_of(&self.target)
        }
    }

    impl IRawElementProviderFragment_Impl for HostProvider_Impl {
        fn Navigate(&self, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
            // A window-hosted fragment root's parent and siblings are its
            // window's, which UI Automation already knows; only its
            // fragment children are this provider's to report.
            match direction {
                NavigateDirection_FirstChild | NavigateDirection_LastChild => {
                    let first = direction == NavigateDirection_FirstChild;
                    super::super::navigate_children(&self.target, None, first)
                }
                _ => Err(windows::core::Error::empty()),
            }
        }
        fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
            // Documented: a window-hosted fragment root returns NULL and UI
            // Automation derives the id from the window.
            Ok(std::ptr::null_mut())
        }
        fn BoundingRectangle(&self) -> Result<UiaRect> {
            // Likewise supplied by the host window.
            Ok(UiaRect::default())
        }
        fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
            Ok(std::ptr::null_mut())
        }
        fn SetFocus(&self) -> Result<()> {
            // The host window takes focus through its own proxy.
            Ok(())
        }
        fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
            super::super::host_provider(&self.target)
        }
    }

    impl IRawElementProviderFragmentRoot_Impl for HostProvider_Impl {
        fn ElementProviderFromPoint(&self, x: f64, y: f64) -> Result<IRawElementProviderFragment> {
            // Screen coordinates arrive as `f64`; pixel positions fit `i32`.
            #[allow(clippy::cast_possible_truncation)]
            let point = Point::new(x as i32, y as i32);
            super::super::element_at(&self.target, point)
        }
        fn GetFocus(&self) -> Result<IRawElementProviderFragment> {
            // Virtual elements do not hold keyboard focus themselves; focus
            // stays with the host window.
            Err(windows::core::Error::empty())
        }
    }

    // Holds no reference to any runtime, but is still created and used on
    // the UI thread only, like every provider here.
    #[implement(IRawElementProviderSimple, Agile = false)]
    pub(crate) struct StructureProvider {
        pub(crate) hwnd: usize,
    }

    impl IRawElementProviderSimple_Impl for StructureProvider_Impl {
        fn ProviderOptions(&self) -> Result<ProviderOptions> {
            Ok(options())
        }
        fn GetPatternProvider(&self, _id: UIA_PATTERN_ID) -> Result<IUnknown> {
            Err(windows::core::Error::empty())
        }
        fn GetPropertyValue(&self, id: UIA_PROPERTY_ID) -> Result<VARIANT> {
            use windows::Win32::UI::Accessibility::{
                UIA_IsContentElementPropertyId, UIA_IsControlElementPropertyId,
            };
            Ok(if id == UIA_IsControlElementPropertyId || id == UIA_IsContentElementPropertyId {
                VARIANT::from(false)
            } else {
                VARIANT::default()
            })
        }
        fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
            host_for(self.hwnd)
        }
    }

    // Not agile, for the reason given on `NodeProvider`.
    #[implement(IRawElementProviderSimple, IRawElementProviderFragment, Agile = false)]
    pub(crate) struct ElementProvider {
        pub(crate) target: Target,
    }

    impl IRawElementProviderSimple_Impl for ElementProvider_Impl {
        fn ProviderOptions(&self) -> Result<ProviderOptions> {
            Ok(options())
        }
        fn GetPatternProvider(&self, id: UIA_PATTERN_ID) -> Result<IUnknown> {
            pattern(&self.target, id)
        }
        fn GetPropertyValue(&self, id: UIA_PROPERTY_ID) -> Result<VARIANT> {
            property(&self.target, id)
        }
        fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
            // A fragment that is not a root has no window of its own.
            Err(windows::core::Error::empty())
        }
    }

    impl IRawElementProviderFragment_Impl for ElementProvider_Impl {
        fn Navigate(&self, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
            let target = &self.target;
            match direction {
                NavigateDirection_Parent => super::super::navigate_parent(target),
                NavigateDirection_FirstChild => {
                    super::super::navigate_children(target, target.element, true)
                }
                NavigateDirection_LastChild => {
                    super::super::navigate_children(target, target.element, false)
                }
                NavigateDirection_NextSibling => super::super::navigate_sibling(target, true),
                NavigateDirection_PreviousSibling => super::super::navigate_sibling(target, false),
                _ => Err(windows::core::Error::empty()),
            }
        }
        fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
            let element = self.target.element.ok_or_else(invalid_operation)?;
            element_runtime_id(element)
        }
        fn BoundingRectangle(&self) -> Result<UiaRect> {
            let resolved = self.target.resolve()?;
            let bounds = resolved.screen_bounds.unwrap_or(rustnative_core::Rect::new(0, 0, 0, 0));
            Ok(UiaRect {
                left: f64::from(bounds.x),
                top: f64::from(bounds.y),
                width: f64::from(bounds.width),
                height: f64::from(bounds.height),
            })
        }
        fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
            Ok(std::ptr::null_mut())
        }
        fn SetFocus(&self) -> Result<()> {
            let resolved = self.target.resolve()?;
            if resolved.info.supports(rustnative_core::AccessibleActionKind::Focus) {
                self.target.act(rustnative_core::AccessibleAction::Focus)
            } else {
                Err(invalid_operation())
            }
        }
        fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
            super::super::host_provider(&self.target)
        }
    }
}

pub(crate) use com::{ElementProvider, HostProvider, NodeProvider, StructureProvider};
