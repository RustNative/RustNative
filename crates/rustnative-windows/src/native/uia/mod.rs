//! UI Automation: the Windows half of Milestone 26's accessibility bridge.
//!
//! # How assistive technology reaches a node
//!
//! UI Automation asks a window for its provider by sending it
//! `WM_GETOBJECT` with `UiaRootObjectId`. This crate answers for every
//! window that realizes a node: its own container windows handle the
//! message in `container_proc`, and the system controls it creates
//! (`BUTTON`, `EDIT`, `STATIC`) are subclassed ([`subclass`]) to do the
//! same. The answer is a provider (see [`provider`]) that reads the node's
//! semantics live from the window's `Runtime` on every call.
//!
//! # Threading and reentrancy
//!
//! Providers are created on the UI thread, which is an OLE single-threaded
//! apartment (see `native::app::OleApartment`), and declare
//! `ProviderOptions_UseComThreading`, so every call from an assistive
//! technology — in any process or thread — is marshaled onto the UI thread
//! and runs from the message loop's own dispatch, never while a `Runtime`
//! is borrowed.
//!
//! The one direction this crate calls *into* UI Automation — raising events
//! and disconnecting providers — is kept out of every runtime borrow: a
//! render only *queues* what changed ([`UiaState::commit`]) and posts
//! [`WM_FRAMEWORK_UIA`] to the window; the posted message drains the queue
//! inside a borrow and raises the events after it ends. An in-process
//! client could otherwise call back into a provider synchronously from
//! inside `UiaRaise*`, re-resolving the runtime the render is still using.
//!
//! # What is and is not announced
//!
//! Property changes on nodes and elements that UI Automation reports through
//! patterns or properties this bridge owns (name, description, toggle,
//! expand/collapse, selection, text and range values), structure changes
//! when a node's virtual elements change, and live-region changes. Changes
//! a system control reports on its own (its caption, its enabled state) are
//! left to it, as is the creation and destruction of native windows, which
//! UI Automation observes directly.

#![allow(
    non_upper_case_globals,
    reason = "matching on the `windows` crate's UI Automation id constants, which keep Microsoft's names"
)]

mod patterns;
mod properties;
mod provider;
pub(crate) mod subclass;
mod target;

use std::collections::{HashMap, HashSet};

use rustnative_core::{
    AccessibilityInfo, AccessibilityTree, AccessibleValue, LiveRegion, NodeId, NodeKind, Point,
    TreeSnapshot, VirtualElement,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    IRawElementProviderFragment, IRawElementProviderFragmentRoot, IRawElementProviderSimple,
    StructureChangeType_ChildrenInvalidated, UIA_ExpandCollapseExpandCollapseStatePropertyId,
    UIA_HelpTextPropertyId, UIA_LiveRegionChangedEventId, UIA_NamePropertyId, UIA_PROPERTY_ID,
    UIA_RangeValueValuePropertyId, UIA_SelectionItemIsSelectedPropertyId,
    UIA_ToggleToggleStatePropertyId, UIA_ValueValuePropertyId, UiaClientsAreListening,
    UiaDisconnectProvider, UiaRaiseAutomationEvent, UiaRaiseAutomationPropertyChangedEvent,
    UiaRaiseStructureChangedEvent, UiaReturnRawElementProvider, UiaRootObjectId,
};
use windows::core::{BSTR, Interface, Result};
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use self::provider::{ElementProvider, HostProvider, NodeProvider, StructureProvider};
use self::target::Target;
use super::runtime::Runtime;
use super::win32::best_effort;

/// Posted to a top-level window when its UI Automation event queue is not
/// empty — see the module documentation.
pub(crate) const WM_FRAMEWORK_UIA: u32 = WM_APP + 4;

/// Something to tell UI Automation, once no runtime borrow is live.
enum Pending {
    Property { target: Target, property: UIA_PROPERTY_ID, old: VARIANT, new: VARIANT },
    LiveRegion(Target),
    ChildrenInvalidated(Target),
    Disconnect(IRawElementProviderSimple),
}

/// A pending notification with its provider already created, ready to
/// raise outside the runtime borrow.
pub(crate) struct Ready {
    provider: IRawElementProviderSimple,
    kind: ReadyKind,
}

enum ReadyKind {
    Property { property: UIA_PROPERTY_ID, old: VARIANT, new: VARIANT },
    LiveRegion,
    ChildrenInvalidated,
    Disconnect,
}

/// A cached node provider, and whether it is the fragment-root kind.
struct NodeEntry {
    provider: IRawElementProviderSimple,
    host: bool,
}

/// One window's UI Automation bookkeeping.
#[derive(Default)]
pub(crate) struct UiaState {
    tree: AccessibilityTree,
    nodes: HashMap<NodeId, NodeEntry>,
    elements: HashMap<(NodeId, NodeId), IRawElementProviderSimple>,
    /// Windows that answered `WM_GETOBJECT` with a provider and so must be
    /// told UI Automation is done with them when they are destroyed.
    returned: HashSet<usize>,
    pending: Vec<Pending>,
}

impl std::fmt::Debug for UiaState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiaState")
            .field("elements", &self.tree.len())
            .field("providers", &self.nodes.len())
            .field("pending", &self.pending.len())
            .finish_non_exhaustive()
    }
}

impl UiaState {
    /// The accessible projection of the most recent render.
    pub(crate) fn tree(&self) -> &AccessibilityTree {
        &self.tree
    }

    /// Adopts a freshly rendered tree: queues the notifications that turn
    /// the previous accessible state into the new one, and schedules
    /// disconnection of providers for nodes and elements that are gone.
    /// Returns whether anything was queued (and so needs posting).
    pub(crate) fn commit(
        &mut self,
        window: usize,
        snapshot: &TreeSnapshot,
        registry: &super::registry::NativeObjectRegistry,
    ) -> bool {
        let next = AccessibilityTree::from_snapshot(snapshot);
        let hwnd_of = |id: NodeId| registry.get(id).map_or(0, |object| object.hwnd() as usize);
        for node in snapshot.nodes() {
            let Some(before) = self.tree.node(node.id) else {
                continue;
            };
            let target = Target { window, hwnd: hwnd_of(node.id), node: node.id, element: None };
            let native_text =
                matches!(node.kind, NodeKind::Label | NodeKind::Button | NodeKind::TextInput);
            let old_name = self.tree.name_of(node.id);
            let new_name = next.name_of(node.id);
            queue_info_changes(
                &mut self.pending,
                target,
                (&before.info, old_name.as_deref()),
                (&node.accessibility, new_name.as_deref()),
                // A system control announces its own caption changes.
                !native_text,
            );
            if before.info.elements() != node.accessibility.elements() {
                diff_elements(
                    &mut self.pending,
                    target,
                    before.info.elements(),
                    node.accessibility.elements(),
                );
            }
        }

        let present: HashSet<NodeId> = snapshot.nodes().map(|node| node.id).collect();
        let gone: Vec<NodeId> =
            self.nodes.keys().filter(|id| !present.contains(id)).copied().collect();
        for id in gone {
            if let Some(entry) = self.nodes.remove(&id) {
                self.pending.push(Pending::Disconnect(entry.provider));
            }
        }
        let stale: Vec<(NodeId, NodeId)> = self
            .elements
            .keys()
            .filter(|(node, element)| {
                snapshot.get(*node).and_then(|n| n.accessibility.find_element(*element)).is_none()
            })
            .copied()
            .collect();
        for key in stale {
            if let Some(provider) = self.elements.remove(&key) {
                self.pending.push(Pending::Disconnect(provider));
            }
        }
        self.tree = next;
        !self.pending.is_empty()
    }

    /// Forgets that `hwnd` returned a provider; used when the window
    /// itself is destroyed (see [`release_window`]).
    fn take_returned(&mut self) -> Vec<usize> {
        self.returned.drain().collect()
    }
}

/// Queues property-change (and live-region) notifications for the
/// differences between two states of the same target.
fn queue_info_changes(
    pending: &mut Vec<Pending>,
    target: Target,
    (old, old_name): (&AccessibilityInfo, Option<&str>),
    (new, new_name): (&AccessibilityInfo, Option<&str>),
    announce_name: bool,
) {
    let mut changed = |property, old: VARIANT, new: VARIANT| {
        pending.push(Pending::Property { target, property, old, new });
    };
    let text = |value: Option<&str>| VARIANT::from(BSTR::from(value.unwrap_or_default()));
    let name_changed = old_name != new_name;
    if announce_name && name_changed {
        changed(UIA_NamePropertyId, text(old_name), text(new_name));
    }
    if old.description_hint() != new.description_hint() {
        changed(UIA_HelpTextPropertyId, text(old.description_hint()), text(new.description_hint()));
    }
    if old.checked_state() != new.checked_state() {
        if let (Some(before), Some(after)) = (old.checked_state(), new.checked_state()) {
            changed(
                UIA_ToggleToggleStatePropertyId,
                VARIANT::from(properties::toggle_state(before).0),
                VARIANT::from(properties::toggle_state(after).0),
            );
        }
    }
    if let (Some(before), Some(after)) = (old.expanded_state(), new.expanded_state()) {
        if before != after {
            changed(
                UIA_ExpandCollapseExpandCollapseStatePropertyId,
                VARIANT::from(properties::expand_collapse_state(before).0),
                VARIANT::from(properties::expand_collapse_state(after).0),
            );
        }
    }
    if let (Some(before), Some(after)) = (old.selected_state(), new.selected_state()) {
        if before != after {
            changed(
                UIA_SelectionItemIsSelectedPropertyId,
                VARIANT::from(before),
                VARIANT::from(after),
            );
        }
    }
    let value_changed = old.value() != new.value();
    match (old.value(), new.value()) {
        (Some(AccessibleValue::Text(before)), Some(AccessibleValue::Text(after)))
            if value_changed =>
        {
            changed(UIA_ValueValuePropertyId, text(Some(before)), text(Some(after)));
        }
        (before, after) if value_changed => {
            if let (Some(before), Some(after)) =
                (properties::range_current(before), properties::range_current(after))
            {
                changed(UIA_RangeValueValuePropertyId, VARIANT::from(before), VARIANT::from(after));
            }
        }
        _ => {}
    }
    if new.live_region() != LiveRegion::Off && (name_changed || value_changed) {
        pending.push(Pending::LiveRegion(target));
    }
}

/// Element-level notifications for one node whose virtual elements changed:
/// a structure change if the set of elements did, and property changes for
/// elements present in both.
fn diff_elements(
    pending: &mut Vec<Pending>,
    host: Target,
    before: &[VirtualElement],
    after: &[VirtualElement],
) {
    let flatten = |elements: &[VirtualElement]| {
        let mut out = HashMap::new();
        let mut stack: Vec<&VirtualElement> = elements.iter().collect();
        while let Some(element) = stack.pop() {
            out.insert(element.id(), element.clone());
            stack.extend(element.info().elements());
        }
        out
    };
    let (old, new) = (flatten(before), flatten(after));
    let structure =
        |elements: &[VirtualElement]| elements.iter().map(VirtualElement::id).collect::<Vec<_>>();
    if old.len() != new.len()
        || structure(before) != structure(after)
        || old.keys().any(|id| !new.contains_key(id))
    {
        pending.push(Pending::ChildrenInvalidated(host));
    }
    for (id, after) in &new {
        if let Some(before) = old.get(id) {
            if before.info() != after.info() {
                queue_info_changes(
                    pending,
                    host.with_element(Some(*id)),
                    (before.info(), before.info().name_hint()),
                    (after.info(), after.info().name_hint()),
                    true,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------
// Provider lookup and creation (always inside a runtime borrow, on the UI
// thread; creating a COM object never calls into UI Automation).
// ---------------------------------------------------------------------

/// The provider for `node`, created on first use and cached so repeated
/// requests (and events) refer to the same object. Returns `None` for a
/// node with no native window.
pub(crate) fn provider_for_node(
    runtime: &mut Runtime,
    window: usize,
    node: NodeId,
) -> Option<IRawElementProviderSimple> {
    let renderer = &mut runtime.renderer;
    let hwnd = renderer.registry.get(node)?.hwnd() as usize;
    let host = !renderer.snapshot.get(node)?.accessibility.elements().is_empty();
    let uia = renderer.accessibility.uia_mut();
    if let Some(entry) = uia.nodes.get(&node) {
        if entry.host == host {
            return Some(entry.provider.clone());
        }
        // Elements appeared or disappeared: the provider's *kind* changes
        // (see `provider`'s module docs), so the old one is retired.
        if let Some(entry) = uia.nodes.remove(&node) {
            uia.pending.push(Pending::Disconnect(entry.provider));
        }
    }
    let target = Target { window, hwnd, node, element: None };
    let provider: IRawElementProviderSimple =
        if host { HostProvider { target }.into() } else { NodeProvider { target }.into() };
    uia.nodes.insert(node, NodeEntry { provider: provider.clone(), host });
    Some(provider)
}

fn element_provider(runtime: &mut Runtime, target: Target) -> Option<IRawElementProviderSimple> {
    let element = target.element?;
    let uia = runtime.renderer.accessibility.uia_mut();
    Some(
        uia.elements
            .entry((target.node, element))
            .or_insert_with(|| ElementProvider { target }.into())
            .clone(),
    )
}

fn host_provider(target: &Target) -> Result<IRawElementProviderFragmentRoot> {
    let target = *target;
    target.with_runtime(|runtime| {
        provider_for_node(runtime, target.window, target.node)
            .ok_or_else(|| windows::core::Error::from_hresult(target::UIA_E_ELEMENTNOTAVAILABLE))?
            .cast()
    })
}

/// The virtual elements under `parent` (the node itself for `None`).
fn children_of(info: &AccessibilityInfo, parent: Option<NodeId>) -> Option<&[VirtualElement]> {
    match parent {
        None => Some(info.elements()),
        Some(id) => info.find_element(id).map(|element| element.info().elements()),
    }
}

/// `id`'s parent element (`None` for a top-level element) and its index
/// among its siblings.
fn locate(info: &AccessibilityInfo, id: NodeId) -> Option<(Option<NodeId>, usize)> {
    fn search(
        elements: &[VirtualElement],
        parent: Option<NodeId>,
        id: NodeId,
    ) -> Option<(Option<NodeId>, usize)> {
        for (index, element) in elements.iter().enumerate() {
            if element.id() == id {
                return Some((parent, index));
            }
            if let Some(found) = search(element.info().elements(), Some(element.id()), id) {
                return Some(found);
            }
        }
        None
    }
    search(info.elements(), None, id)
}

fn fragment(runtime: &mut Runtime, target: Target) -> Result<IRawElementProviderFragment> {
    element_provider(runtime, target).ok_or_else(windows::core::Error::empty)?.cast()
}

fn node_info(runtime: &Runtime, node: NodeId) -> Result<AccessibilityInfo> {
    runtime
        .renderer
        .snapshot
        .get(node)
        .map(|node| node.accessibility.clone())
        .ok_or_else(|| windows::core::Error::from_hresult(target::UIA_E_ELEMENTNOTAVAILABLE))
}

fn navigate_children(
    target: &Target,
    parent: Option<NodeId>,
    first: bool,
) -> Result<IRawElementProviderFragment> {
    let target = *target;
    target.with_runtime(|runtime| {
        let info = node_info(runtime, target.node)?;
        let children = children_of(&info, parent).unwrap_or_default();
        let child = if first { children.first() } else { children.last() };
        match child {
            Some(child) => fragment(runtime, target.with_element(Some(child.id()))),
            None => Err(windows::core::Error::empty()),
        }
    })
}

fn navigate_parent(target: &Target) -> Result<IRawElementProviderFragment> {
    let target = *target;
    target.with_runtime(|runtime| {
        let info = node_info(runtime, target.node)?;
        let element = target.element.ok_or_else(windows::core::Error::empty)?;
        match locate(&info, element) {
            Some((Some(parent), _)) => fragment(runtime, target.with_element(Some(parent))),
            Some((None, _)) => provider_for_node(runtime, target.window, target.node)
                .ok_or_else(windows::core::Error::empty)?
                .cast(),
            None => Err(windows::core::Error::from_hresult(target::UIA_E_ELEMENTNOTAVAILABLE)),
        }
    })
}

fn navigate_sibling(target: &Target, next: bool) -> Result<IRawElementProviderFragment> {
    let target = *target;
    target.with_runtime(|runtime| {
        let info = node_info(runtime, target.node)?;
        let element = target.element.ok_or_else(windows::core::Error::empty)?;
        let (parent, index) = locate(&info, element)
            .ok_or_else(|| windows::core::Error::from_hresult(target::UIA_E_ELEMENTNOTAVAILABLE))?;
        let siblings = children_of(&info, parent).unwrap_or_default();
        let sibling = if next {
            siblings.get(index + 1)
        } else {
            index.checked_sub(1).and_then(|i| siblings.get(i))
        };
        match sibling {
            Some(sibling) => fragment(runtime, target.with_element(Some(sibling.id()))),
            None => Err(windows::core::Error::empty()),
        }
    })
}

/// The deepest virtual element of `target`'s node containing the screen
/// point `screen`.
fn element_at(target: &Target, screen: Point) -> Result<IRawElementProviderFragment> {
    let target = *target;
    target.with_runtime(|runtime| {
        let info = node_info(runtime, target.node)?;
        let hwnd = runtime
            .renderer
            .registry
            .get(target.node)
            .map(super::registry::NativeObject::hwnd)
            .ok_or_else(windows::core::Error::empty)?;
        let mut local = windows_sys::Win32::Foundation::POINT { x: screen.x, y: screen.y };
        // SAFETY: `hwnd` is a live window from this runtime's registry;
        // `local` is a valid, exclusively borrowed `POINT`.
        unsafe { windows_sys::Win32::Graphics::Gdi::ScreenToClient(hwnd, &raw mut local) };
        let mut found = None;
        let mut level = info.elements();
        while let Some(hit) = level.iter().rev().find(|element| {
            let b = element.bounds();
            local.x >= b.x && local.y >= b.y && local.x < b.x + b.width && local.y < b.y + b.height
        }) {
            found = Some(hit.id());
            level = hit.info().elements();
        }
        match found {
            Some(id) => fragment(runtime, target.with_element(Some(id))),
            None => Err(windows::core::Error::empty()),
        }
    })
}

// ---------------------------------------------------------------------
// The two message-level entry points.
// ---------------------------------------------------------------------

/// Answers `WM_GETOBJECT` for `hwnd`, a window realizing a node, if it asks
/// for the UI Automation root object. Returns `None` for any other request
/// (MSAA's `OBJID_CLIENT` and friends), which the caller passes on to the
/// window's default handling.
pub(crate) fn get_object(
    hwnd: windows_sys::Win32::Foundation::HWND,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    // The object id travels in the low 32 bits of `lParam`, as a signed
    // `DWORD`; `UiaRootObjectId` is negative.
    #[allow(clippy::cast_possible_truncation)]
    let object_id = lparam.0 as i32;
    if object_id != UiaRootObjectId {
        return None;
    }
    let root = super::context::root_window(hwnd);
    let provider = super::context::with_runtime(root, |runtime| {
        let node = runtime.renderer.registry.id_for_hwnd(hwnd)?;
        let provider = if runtime.renderer.registry.get(node)?.hwnd() == hwnd {
            provider_for_node(runtime, root as usize, node)?
        } else {
            // A container's inner content window: structure, not semantics
            // (see `provider::StructureProvider`).
            StructureProvider { hwnd: hwnd as usize }.into()
        };
        runtime.renderer.accessibility.uia_mut().returned.insert(hwnd as usize);
        Some(provider)
    })
    .flatten()?;
    // SAFETY: `hwnd` is the live window this message was sent to, and
    // `wparam`/`lparam` are forwarded unchanged, as the function requires;
    // it AddRefs the provider it hands to UI Automation.
    Some(unsafe { UiaReturnRawElementProvider(HWND(hwnd.cast()), wparam, lparam, &provider) })
}

/// Posts [`WM_FRAMEWORK_UIA`] so queued notifications are raised on a later,
/// unnested turn of the loop.
pub(crate) fn schedule(window: windows_sys::Win32::Foundation::HWND) {
    // SAFETY: `window` is this runtime's live top-level window.
    let posted = unsafe { PostMessageW(window, WM_FRAMEWORK_UIA, 0, 0) } != 0;
    best_effort(posted, "PostMessageW(UIA)", "the notifications are raised with the next batch");
}

/// Drains the queue for the window whose runtime this is, creating the
/// providers the notifications are about. Runs inside the runtime borrow;
/// [`raise`] runs after it.
pub(crate) fn take_ready(runtime: &mut Runtime) -> Vec<Ready> {
    let window = runtime.window as usize;
    let pending = std::mem::take(&mut runtime.renderer.accessibility.uia_mut().pending);
    // SAFETY: takes no arguments.
    let listening = unsafe { UiaClientsAreListening() }.as_bool();
    let mut ready = Vec::new();
    for item in pending {
        let (target, kind) = match item {
            Pending::Disconnect(provider) => {
                ready.push(Ready { provider, kind: ReadyKind::Disconnect });
                continue;
            }
            // Nobody is listening, so no event would be delivered; skipping
            // them avoids creating providers only to announce to no one.
            _ if !listening => continue,
            Pending::Property { target, property, old, new } => {
                (target, ReadyKind::Property { property, old, new })
            }
            Pending::LiveRegion(target) => (target, ReadyKind::LiveRegion),
            Pending::ChildrenInvalidated(target) => (target, ReadyKind::ChildrenInvalidated),
        };
        let provider = if target.is_native() {
            provider_for_node(runtime, window, target.node)
        } else {
            element_provider(runtime, target)
        };
        if let Some(provider) = provider {
            ready.push(Ready { provider, kind });
        }
    }
    ready
}

/// Raises drained notifications. Must run with no runtime borrowed.
pub(crate) fn raise(ready: Vec<Ready>) {
    for Ready { provider, kind } in ready {
        // Every raise is best effort: a client that went away between the
        // change and the notification is not an error in this application.
        // SAFETY: `provider` is a live provider this crate created;
        // variants are passed by reference for the duration of each call.
        let raised = unsafe {
            match kind {
                ReadyKind::Property { property, old, new } => {
                    UiaRaiseAutomationPropertyChangedEvent(&provider, property, &old, &new)
                }
                ReadyKind::LiveRegion => {
                    UiaRaiseAutomationEvent(&provider, UIA_LiveRegionChangedEventId)
                }
                ReadyKind::ChildrenInvalidated => {
                    // A children-invalidated notification on a window-hosted
                    // root carries no runtime id of its own; UI Automation
                    // uses the provider's.
                    let mut id = [0_i32; 0];
                    UiaRaiseStructureChangedEvent(
                        &provider,
                        StructureChangeType_ChildrenInvalidated,
                        id.as_mut_ptr(),
                        0,
                    )
                }
                ReadyKind::Disconnect => UiaDisconnectProvider(&provider),
            }
        };
        best_effort(
            raised.is_ok(),
            "UiaRaise*/UiaDisconnectProvider",
            "one notification is not delivered",
        );
    }
}

/// Tells UI Automation the window's providers are finished with, as
/// Microsoft documents a provider-hosting window must on `WM_DESTROY`
/// (`UiaReturnRawElementProvider` with all-null arguments), and disconnects
/// every provider the window created.
pub(crate) fn release_window(runtime: &mut Runtime) -> Vec<Ready> {
    let uia = runtime.renderer.accessibility.uia_mut();
    let mut ready: Vec<Ready> = uia
        .nodes
        .drain()
        .map(|(_, entry)| Ready { provider: entry.provider, kind: ReadyKind::Disconnect })
        .collect();
    ready.extend(
        uia.elements.drain().map(|(_, provider)| Ready { provider, kind: ReadyKind::Disconnect }),
    );
    for hwnd in uia.take_returned() {
        // SAFETY: the documented "release everything" call for a window
        // that returned a provider; `hwnd` may already be mid-destruction,
        // which the call tolerates.
        unsafe {
            UiaReturnRawElementProvider(
                HWND(hwnd as *mut core::ffi::c_void),
                WPARAM(0),
                LPARAM(0),
                None::<&IRawElementProviderSimple>,
            );
        }
    }
    ready
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustnative_core::{AccessibilityRole, Rect};

    fn element(key: &str, children: Vec<VirtualElement>) -> VirtualElement {
        let mut info = AccessibilityInfo::new(AccessibilityRole::Button);
        for child in children {
            info = info.element(child);
        }
        VirtualElement::new(key, info, Rect::new(0, 0, 1, 1))
    }

    #[test]
    fn elements_are_located_with_their_parent_and_index() {
        let info = AccessibilityInfo::new(AccessibilityRole::Canvas)
            .element(element("a", vec![element("a1", vec![]), element("a2", vec![])]))
            .element(element("b", vec![]));
        assert_eq!(locate(&info, NodeId::from_key("b")), Some((None, 1)));
        assert_eq!(locate(&info, NodeId::from_key("a2")), Some((Some(NodeId::from_key("a")), 1)));
        assert_eq!(locate(&info, NodeId::from_key("zzz")), None);
        assert_eq!(children_of(&info, Some(NodeId::from_key("a"))).map(<[_]>::len), Some(2));
    }
}
