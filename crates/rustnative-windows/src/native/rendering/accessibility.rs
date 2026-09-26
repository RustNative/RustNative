//! The accessibility adapter: the one place the portable semantic model is
//! projected onto Win32.
//!
//! # What this closes
//!
//! The standards audit's P1.16 finding is that `rustnative-core` had a
//! perfectly good portable semantic model — role, name, description,
//! focusability — and the Windows backend consumed exactly one field of it:
//!
//! > The Windows backend currently relies heavily on the default semantics
//! > of native controls. [...] portable semantic data existing in Rust is
//! > not the same thing as platform accessibility being correct.
//!
//! `AccessibilityInfo::name_hint` and `description_hint` reached no Win32
//! API at all, so a button whose visible text was an icon or an
//! abbreviation announced that text and nothing else, and a node's declared
//! `AccessibilityRole` had no effect on what assistive technology reported.
//!
//! # What it does, and the boundary it draws
//!
//! The audit's instruction is to "define an explicit accessibility adapter
//! boundary now, even if the full UI Automation provider remains future
//! work". That boundary is [`AccessibilityBridge`]: realization code hands
//! it a node and an HWND and knows nothing else about accessibility, and
//! this module knows nothing about control creation, styling, or layout.
//!
//! Two mechanisms sit behind it:
//!
//! 1. **Keyboard focusability**, as the `WS_TABSTOP` window style. Applied
//!    symmetrically — a node that stops being focusable has the bit
//!    *removed*, which the audit specifically calls out as previously
//!    missing ("does not symmetrically remove native focus styles when
//!    focusability changes").
//! 2. **Name, description, and role**, through Microsoft's Dynamic
//!    Annotation API (`IAccPropServices`). This is the supported way to
//!    override the accessible properties of a *standard* control without
//!    writing a full provider: the control keeps its own native
//!    implementation and behavior, and the annotation supersedes individual
//!    properties on it. Annotations are addressed by
//!    `(HWND, OBJID_CLIENT, CHILDID_SELF)`, so they follow the window and
//!    are cleared when it is destroyed.
//!
//! A full UI Automation provider — which is what custom, non-`HWND`-backed
//! semantic nodes will eventually need — remains future work, exactly as
//! the audit allows. What is no longer true is that the portable model is
//! decorative.
//!
//! # Degradation
//!
//! Everything here is best effort by design. `IAccPropServices` needs COM
//! on the calling thread; if it is unavailable (COM initialization failed,
//! the object could not be created), annotation is skipped and controls
//! fall back to their native defaults — which is the behavior this backend
//! had before this module existed. An application never fails to start,
//! and no control ever fails to appear, because an annotation could not be
//! applied.

use rustnative_core::{AccessibilityRole, AccessibilityTree, NodeKind, TreeNode, TreeSnapshot};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetWindowLongPtrW, SetWindowLongPtrW, WS_TABSTOP,
};

use super::super::win32::informational;

/// The accessible properties this backend projects onto a native control,
/// derived from a node's portable [`AccessibilityInfo`] and its visible
/// text.
///
/// This is the "narrow data projection" the audit's dependency-direction
/// rule asks for: control realization consumes it without importing the
/// semantic model, and this module derives it without importing the
/// renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AccessibleProjection {
    /// Whether the control participates in keyboard tab order.
    pub(crate) focusable: bool,
    /// The name assistive technology should announce, if it differs from
    /// what the control would report on its own.
    pub(crate) name: Option<String>,
    /// Supplementary detail announced after the name.
    pub(crate) description: Option<String>,
    /// The MSAA role constant, if the portable role maps to one.
    pub(crate) role: Option<u32>,
}

impl AccessibleProjection {
    /// Projects a node's semantics onto the Win32 concepts above.
    ///
    /// `disabled` participates because a disabled control must leave the
    /// tab order regardless of what the node declares: `EnableWindow(FALSE)`
    /// already stops Win32 routing keyboard input to it, and leaving
    /// `WS_TABSTOP` set would make Tab appear to skip unpredictably.
    pub(crate) fn of(node: &TreeNode) -> Self {
        Self {
            focusable: node.accessibility.is_focusable() && !node.disabled,
            // A name is only worth annotating when it says something the
            // control would not already report. A button labelled "Save"
            // whose accessible name is also "Save" needs no annotation, and
            // skipping it keeps the annotation table proportional to the
            // number of nodes that actually override their name.
            name: node
                .accessibility
                .name_hint()
                .filter(|name| Some(*name) != node.text.as_deref())
                .map(ToOwned::to_owned),
            description: node.accessibility.description_hint().map(ToOwned::to_owned),
            role: msaa_role(node.accessibility.role()),
        }
    }

    /// Whether any property here needs a dynamic annotation, as opposed to
    /// being fully served by the native control's own defaults.
    fn needs_annotation(&self) -> bool {
        self.name.is_some() || self.description.is_some() || self.role.is_some()
    }
}

/// Maps a portable [`AccessibilityRole`] to its MSAA `ROLE_SYSTEM_*`
/// constant.
///
/// The constants are inlined rather than imported because `windows-sys`
/// places them behind a feature this crate does not otherwise need, and
/// they are a frozen part of the MSAA ABI — `oleacc.h` has defined these
/// exact values since Windows 2000 and cannot change them without breaking
/// every existing screen reader.
///
/// [`AccessibilityRole::None`] maps to `None` rather than to a role
/// constant: it means "do not present this node as a distinct element",
/// which is expressed by leaving the control's own role alone, not by
/// annotating it with a different one. A future role this crate does not
/// yet map (the enum is `#[non_exhaustive]`) takes the same path, so adding
/// one upstream degrades to the control's native role rather than failing
/// to compile a backend that has not caught up.
const fn msaa_role(role: AccessibilityRole) -> Option<u32> {
    /// `ROLE_SYSTEM_GROUPING` — a container that groups other elements.
    const ROLE_SYSTEM_GROUPING: u32 = 0x14;
    /// `ROLE_SYSTEM_STATICTEXT` — read-only text.
    const ROLE_SYSTEM_STATICTEXT: u32 = 0x29;
    /// `ROLE_SYSTEM_TEXT` — editable text.
    const ROLE_SYSTEM_TEXT: u32 = 0x2A;
    /// `ROLE_SYSTEM_PUSHBUTTON` — an activatable button.
    const ROLE_SYSTEM_PUSHBUTTON: u32 = 0x2B;

    match role {
        AccessibilityRole::Label => Some(ROLE_SYSTEM_STATICTEXT),
        AccessibilityRole::Button => Some(ROLE_SYSTEM_PUSHBUTTON),
        AccessibilityRole::TextInput => Some(ROLE_SYSTEM_TEXT),
        AccessibilityRole::Group => Some(ROLE_SYSTEM_GROUPING),
        // `AccessibilityRole::None`, and any role a future version of
        // `rustnative-core` adds that this backend has not caught up with.
        _ => None,
    }
}

/// Applies a node's accessible projection to its native window.
///
/// This is the adapter's whole public surface: realization calls it after
/// creating or updating a control and after nothing else, so there is one
/// place to look for "what does this framework tell Windows about a node's
/// semantics?".
#[derive(Debug, Default)]
pub(crate) struct AccessibilityBridge {
    annotations: annotation::Annotator,
    uia: crate::native::uia::UiaState,
}

impl AccessibilityBridge {
    /// Gives a newly created native object its UI Automation provider hook:
    /// system controls are subclassed for `WM_GETOBJECT` (this crate's own
    /// container, canvas, and surface windows answer it in their own window
    /// procedures).
    pub(crate) fn attach(hwnd: HWND, kind: NodeKind) {
        if matches!(
            kind,
            NodeKind::Label | NodeKind::Button | NodeKind::TextInput | NodeKind::TabBar
        ) {
            crate::native::uia::subclass::install(hwnd);
        }
    }

    /// Adopts a freshly rendered tree for UI Automation; returns whether
    /// notifications were queued (see `native::uia`).
    pub(crate) fn commit(
        &mut self,
        window: HWND,
        snapshot: &TreeSnapshot,
        registry: &crate::native::registry::NativeObjectRegistry,
    ) -> bool {
        self.uia.commit(window as usize, snapshot, registry)
    }

    /// The accessible projection of the most recent render.
    pub(crate) fn tree(&self) -> &AccessibilityTree {
        self.uia.tree()
    }

    /// The window's UI Automation state.
    pub(crate) fn uia_mut(&mut self) -> &mut crate::native::uia::UiaState {
        &mut self.uia
    }

    /// Synchronizes `hwnd`'s accessible state with `node`'s semantics.
    pub(crate) fn apply(&mut self, hwnd: HWND, node: &TreeNode) {
        let projection = AccessibleProjection::of(node);
        set_tab_stop(hwnd, projection.focusable);
        if projection.needs_annotation() {
            self.annotations.annotate(hwnd, &projection);
        } else {
            self.annotations.clear(hwnd);
        }
    }

    /// Forgets everything recorded for a window that is being destroyed.
    ///
    /// Windows itself discards annotations when the window goes away, so
    /// this exists to release the annotator's own bookkeeping rather than
    /// to undo anything on the Win32 side.
    pub(crate) fn forget(&mut self, hwnd: HWND) {
        self.annotations.clear(hwnd);
    }
}

/// Whether `hwnd` currently participates in Win32's keyboard tab order.
///
/// The read side of [`set_tab_stop`], exposed so the native integration
/// tests can assert the projection landed even in an environment that will
/// not grant the test process real keyboard focus. Nothing in the backend
/// itself needs to read the bit back — `set_tab_stop` is idempotent — hence
/// the `cfg(test)` gate rather than a permanent accessor.
#[cfg(test)]
pub(crate) fn has_tab_stop(hwnd: HWND) -> bool {
    // SAFETY: `hwnd` is a live HWND owned by this backend's registry;
    // `GWL_STYLE` is a documented, always-valid index for any window.
    let current = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };
    // `WS_TABSTOP` is a small, fixed Win32 style-bit constant well below
    // `isize::MAX`.
    #[allow(clippy::cast_possible_wrap)]
    let tabstop_bit = WS_TABSTOP as isize;
    current & tabstop_bit != 0
}

/// Adds or removes `WS_TABSTOP` on `hwnd`, leaving every other style bit
/// untouched.
///
/// Read-modify-write rather than a blanket assignment: the control was
/// created with class-specific style bits (`BS_PUSHBUTTON`, `ES_AUTOHSCROLL`,
/// ...) that must survive a focusability change.
fn set_tab_stop(hwnd: HWND, focusable: bool) {
    // SAFETY: `hwnd` is a live HWND owned by the caller's registry entry;
    // `GWL_STYLE` is a documented, always-valid index for any window.
    let current = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };
    // `WS_TABSTOP` is a small, fixed Win32 style-bit constant; it can never
    // approach `isize::MAX`/wrap the sign bit on either a 32-bit or 64-bit
    // `isize`.
    #[allow(clippy::cast_possible_wrap)]
    let tabstop_bit = WS_TABSTOP as isize;
    let next = if focusable { current | tabstop_bit } else { current & !tabstop_bit };
    if next == current {
        return;
    }
    // SAFETY: `hwnd` is the same live HWND validated above; `GWL_STYLE` is
    // a documented, always-valid index; `next` was derived from `current`
    // (itself just read back from this same slot) with only the single
    // documented `WS_TABSTOP` bit toggled.
    //
    // `SetWindowLongPtrW` returns the *previous* value, not a status.
    informational(unsafe { SetWindowLongPtrW(hwnd, GWL_STYLE, next) });
}

/// The Dynamic Annotation half of the bridge, isolated so the COM
/// dependency has one home and the rest of the module stays testable
/// without it.
mod annotation {
    use std::collections::HashSet;

    use windows::Win32::UI::Accessibility::{
        IAccPropServices, PROPID_ACC_DESCRIPTION, PROPID_ACC_NAME, PROPID_ACC_ROLE,
    };
    use windows::core::{GUID, PCWSTR};
    use windows_sys::Win32::Foundation::HWND;

    use super::AccessibleProjection;
    use crate::native::util::wide;

    /// `OBJID_CLIENT` — the client area of a window, which for a standard
    /// control is the control itself. Frozen MSAA ABI constant from
    /// `oleacc.h`; see [`super::msaa_role`] for why these are inlined.
    const OBJID_CLIENT: u32 = 0xFFFF_FFFC;
    /// `CHILDID_SELF` — the object itself rather than one of its children.
    const CHILDID_SELF: u32 = 0;

    /// Every property this crate ever annotates, and therefore every one it
    /// must clear.
    const ANNOTATED_PROPERTIES: [GUID; 3] =
        [PROPID_ACC_NAME, PROPID_ACC_DESCRIPTION, PROPID_ACC_ROLE];

    /// Applies accessible-property overrides to standard controls, and
    /// remembers which windows currently carry one so they can be cleared.
    ///
    /// The annotator is deliberately tolerant: if the COM service is
    /// unavailable it becomes a no-op rather than an error, and controls
    /// keep their native defaults.
    #[derive(Debug, Default)]
    pub(super) struct Annotator {
        /// Windows that currently carry at least one annotation, keyed by
        /// handle value, so [`Annotator::clear`] can skip the COM call for
        /// the overwhelming majority of controls that never had one.
        annotated: HashSet<usize>,
        /// The annotation service, created on first use (see
        /// [`Annotator::services`]).
        services: ServiceState,
    }

    /// Whether the annotation service has been created yet, and whether it
    /// could be.
    #[derive(Debug, Default)]
    enum ServiceState {
        #[default]
        Untried,
        Unavailable,
        Ready(IAccPropServices),
    }

    impl Annotator {
        pub(super) fn annotate(&mut self, hwnd: HWND, projection: &AccessibleProjection) {
            if self.services().is_some_and(|services| apply(services, hwnd, projection)) {
                self.annotated.insert(hwnd as usize);
            }
        }

        pub(super) fn clear(&mut self, hwnd: HWND) {
            if !self.annotated.remove(&(hwnd as usize)) {
                return;
            }
            if let Some(services) = self.services() {
                // SAFETY: `services` is a live COM interface obtained on
                // this thread; `hwnd` may already have been destroyed,
                // which `ClearHwndProps` reports as an `Err` rather than
                // treating as a dereference — the annotation table is keyed
                // by handle value, not by a pointer into the window.
                let _ = unsafe {
                    services.ClearHwndProps(
                        handle(hwnd),
                        OBJID_CLIENT,
                        CHILDID_SELF,
                        &ANNOTATED_PROPERTIES,
                    )
                };
            }
        }

        /// The window's `IAccPropServices`, created on first use, or `None`
        /// if COM could not provide one.
        ///
        /// Owned by this (per-window) annotator rather than kept in a
        /// `thread_local!`, which is what an earlier revision did. A
        /// thread-local is destroyed at thread exit — after the message loop
        /// has already torn down the thread's COM apartment
        /// (`native::app::OleApartment`) — so its final `Release` ran against
        /// an uninitialized apartment. That crashed the process with an
        /// access violation as soon as Milestone 25 initialized OLE on the UI
        /// thread (before which the service silently failed to create at
        /// all, hiding the problem). Owned here, it is released with the
        /// window's renderer, while the apartment is still alive.
        ///
        /// A failure — most often "this thread has no COM apartment" — is
        /// remembered rather than retried per control: it is a property of
        /// the environment, not of any one call.
        fn services(&mut self) -> Option<&IAccPropServices> {
            use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
            use windows::Win32::UI::Accessibility::CAccPropServices;

            if matches!(self.services, ServiceState::Untried) {
                // SAFETY: `CoCreateInstance` takes a class id, a null
                // outer-unknown (documented valid for a non-aggregated
                // object), and a context flag. The `windows` bindings return
                // a `Result`, so a failure — including an uninitialized
                // apartment — surfaces as `Err` rather than as an invalid
                // interface pointer.
                let created =
                    unsafe { CoCreateInstance(&CAccPropServices, None, CLSCTX_INPROC_SERVER) };
                self.services = created.map_or(ServiceState::Unavailable, ServiceState::Ready);
            }
            match &self.services {
                ServiceState::Ready(services) => Some(services),
                ServiceState::Untried | ServiceState::Unavailable => None,
            }
        }
    }

    /// Reinterprets this crate's `windows-sys` handle as the `windows`
    /// crate's newtype over the same pointer.
    fn handle(hwnd: HWND) -> windows::Win32::Foundation::HWND {
        windows::Win32::Foundation::HWND(hwnd)
    }

    /// Applies `projection`'s overrides to `hwnd`, returning whether any
    /// were actually recorded.
    fn apply(services: &IAccPropServices, hwnd: HWND, projection: &AccessibleProjection) -> bool {
        let mut applied = false;

        if let Some(name) = &projection.name {
            applied |= set_string(services, hwnd, PROPID_ACC_NAME, name);
        }
        if let Some(description) = &projection.description {
            applied |= set_string(services, hwnd, PROPID_ACC_DESCRIPTION, description);
        }
        if let Some(role) = projection.role {
            applied |= set_role(services, hwnd, role);
        }

        applied
    }

    /// Sets one string-valued accessible property.
    fn set_string(services: &IAccPropServices, hwnd: HWND, property: GUID, value: &str) -> bool {
        let encoded = wide(value);
        // SAFETY: `services` is a live COM interface obtained on this
        // thread; `handle(hwnd)` names a live control; `encoded` is a
        // NUL-terminated wide buffer that outlives this synchronous call,
        // and `SetHwndPropStr` is documented to copy the string rather than
        // retain the caller's pointer.
        unsafe {
            services
                .SetHwndPropStr(
                    handle(hwnd),
                    OBJID_CLIENT,
                    CHILDID_SELF,
                    property,
                    PCWSTR(encoded.as_ptr()),
                )
                .is_ok()
        }
    }

    /// Sets the MSAA role, which — unlike name and description — is a
    /// numeric property and so travels in a `VARIANT`.
    fn set_role(services: &IAccPropServices, hwnd: HWND, role: u32) -> bool {
        // `VARIANT::from` has no `u32` impl; every `ROLE_SYSTEM_*` constant
        // is far below `i32::MAX`, so the conversion cannot actually fail,
        // and a hypothetical failure is reported as "not applied" rather
        // than annotating a wrong role.
        let Ok(role) = i32::try_from(role) else {
            return false;
        };
        let variant = windows::Win32::System::Variant::VARIANT::from(role);
        // SAFETY: as `set_string` above; `variant` is an owned `VARIANT`
        // holding a plain integer — no pointer for the callee to outlive —
        // and is borrowed only for the duration of this call.
        unsafe {
            services
                .SetHwndProp(handle(hwnd), OBJID_CLIENT, CHILDID_SELF, PROPID_ACC_ROLE, &variant)
                .is_ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::test_support::TestWindow;
    use rustnative_core::{AccessibilityInfo, Node, TreeSnapshot};

    fn node_for(node: &Node, key: &str) -> TreeNode {
        TreeSnapshot::from_node(node)
            .expect("test tree has unique keys")
            .get(rustnative_core::NodeId::from_key(key))
            .expect("the node under test is in its own snapshot")
            .clone()
    }

    #[test]
    fn every_portable_role_maps_to_a_distinct_msaa_role() {
        let mapped = [
            AccessibilityRole::Label,
            AccessibilityRole::Button,
            AccessibilityRole::TextInput,
            AccessibilityRole::Group,
        ]
        .map(|role| msaa_role(role).expect("every real role maps"));
        let mut unique = mapped.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), mapped.len(), "distinct portable roles must not collapse");
        assert_eq!(
            msaa_role(AccessibilityRole::None),
            None,
            "`None` means leave the control's own role alone, not override it with another"
        );
    }

    #[test]
    fn a_default_button_projects_as_focusable_with_its_native_name() {
        let projection = AccessibleProjection::of(&node_for(&Node::button("go", "Go"), "go"));
        assert!(projection.focusable, "a button is focusable by default");
        assert_eq!(
            projection.name, None,
            "a button whose accessible name equals its visible text needs no annotation"
        );
        assert_eq!(projection.role, msaa_role(AccessibilityRole::Button));
    }

    #[test]
    fn an_overriding_accessible_name_is_projected_for_annotation() {
        let node = Node::button("go", "OK").with_accessibility(
            AccessibilityInfo::new(AccessibilityRole::Button)
                .name("Confirm the payment")
                .description("Charges the saved card")
                .focusable(true),
        );
        let projection = AccessibleProjection::of(&node_for(&node, "go"));
        assert_eq!(projection.name.as_deref(), Some("Confirm the payment"));
        assert_eq!(projection.description.as_deref(), Some("Charges the saved card"));
        assert!(projection.needs_annotation());
    }

    #[test]
    fn a_disabled_node_is_never_projected_as_focusable() {
        let node = Node::button("go", "Go").disabled(true);
        assert!(
            !AccessibleProjection::of(&node_for(&node, "go")).focusable,
            "a disabled control must leave the tab order even though it declares focusability"
        );
    }

    #[test]
    fn a_label_is_projected_as_non_focusable_static_text() {
        let projection =
            AccessibleProjection::of(&node_for(&Node::label("caption", "Total"), "caption"));
        assert!(!projection.focusable, "static text is not a keyboard stop");
        assert_eq!(projection.role, msaa_role(AccessibilityRole::Label));
    }

    #[test]
    fn tab_stop_is_applied_and_removed_symmetrically_on_a_real_window() {
        let window = TestWindow::new();
        // `WS_TABSTOP` is a fixed Win32 constant well below `isize::MAX`.
        #[allow(clippy::cast_possible_wrap)]
        let bit = WS_TABSTOP as isize;
        // SAFETY: `window.hwnd` is a live, test-owned message-only HWND;
        // `GWL_STYLE` is a documented, always-valid index for any window.
        let style = || unsafe { GetWindowLongPtrW(window.hwnd, GWL_STYLE) };

        set_tab_stop(window.hwnd, true);
        assert_ne!(style() & bit, 0, "a focusable node must carry WS_TABSTOP");

        set_tab_stop(window.hwnd, false);
        assert_eq!(
            style() & bit,
            0,
            "losing focusability must remove WS_TABSTOP, not just stop adding it (P1.16)"
        );
    }

    #[test]
    fn toggling_tab_stop_preserves_every_other_style_bit() {
        let window = TestWindow::new();
        // SAFETY: as in the test above — a live, test-owned HWND and a
        // documented style index.
        let before = unsafe { GetWindowLongPtrW(window.hwnd, GWL_STYLE) };
        #[allow(clippy::cast_possible_wrap)]
        let bit = WS_TABSTOP as isize;

        set_tab_stop(window.hwnd, true);
        set_tab_stop(window.hwnd, false);
        // SAFETY: as above.
        let after = unsafe { GetWindowLongPtrW(window.hwnd, GWL_STYLE) };
        assert_eq!(
            before & !bit,
            after & !bit,
            "a focusability change must not disturb class or frame style bits"
        );
    }

    #[test]
    fn the_bridge_applies_to_a_real_window_without_requiring_com() {
        // The annotation half degrades to a no-op when `IAccPropServices`
        // is unavailable (see the module docs). This asserts the bridge as
        // a whole still does its non-COM work — the focusability
        // projection — either way, since that is what keyboard navigation
        // depends on.
        let window = TestWindow::new();
        let mut bridge = AccessibilityBridge::default();
        let node = Node::button("go", "Go").with_accessibility(
            AccessibilityInfo::new(AccessibilityRole::Button).name("Go now").focusable(true),
        );
        bridge.apply(window.hwnd, &node_for(&node, "go"));

        #[allow(clippy::cast_possible_wrap)]
        let bit = WS_TABSTOP as isize;
        // SAFETY: as above.
        let style = unsafe { GetWindowLongPtrW(window.hwnd, GWL_STYLE) };
        assert_ne!(style & bit, 0);
        bridge.forget(window.hwnd);
    }
}
