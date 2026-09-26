//! Tracks every native Win32 object created for a `NodeId`, in both
//! directions (`NodeId -> NativeObject` and `HWND -> NodeId`).

use std::collections::HashMap;

use rustnative_core::NodeId;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow;

use crate::Error;

#[derive(Debug, Default)]
pub(crate) struct NativeObjectRegistry {
    objects: HashMap<NodeId, NativeObject>,
    by_hwnd: HashMap<HWND, NodeId>,
    /// Each object's top-level window, recorded at insertion — by the time
    /// an object is removed its window may already be gone, so it cannot
    /// be asked then.
    top_levels: HashMap<NodeId, isize>,
    /// Objects created and destroyed, for the inspector's lifetimes view.
    lifetimes: rustnative_core::inspect::Lifetimes,
}

/// How many recent creations and destructions the inspector is shown.
const RECENT_LIFETIMES: usize = 64;

#[derive(Debug)]
pub(crate) enum NativeObject {
    Container {
        viewport: HWND,
        content: HWND,
    },
    Label(HWND),
    Button(HWND),
    TextInput(HWND),
    /// A Direct2D canvas window (see `native::graphics::canvas`).
    Canvas(HWND),
    /// A system tab control (see `rendering::tabs`).
    TabBar(HWND),
    /// A native control (see `rendering::native_controls`): its window, a
    /// spinner's up-down companion, and which control realizes it.
    Control {
        hwnd: HWND,
        companion: Option<HWND>,
        tag: &'static str,
    },
    /// A native surface window, and the id the application knows it by.
    Surface {
        hwnd: HWND,
        id: rustnative_core::SurfaceId,
    },
    /// A foreign object a registered factory made (see `native::foreign`).
    Foreign {
        hwnd: HWND,
        kind: String,
        ownership: super::foreign::Ownership,
    },
}

impl NativeObject {
    /// What kind of host object this is, for the inspector.
    pub(crate) fn host_type(&self) -> String {
        match self {
            Self::Container { .. } => "container (viewport + content windows)".to_owned(),
            Self::Label(_) => "STATIC".to_owned(),
            Self::Button(_) => "BUTTON".to_owned(),
            Self::TextInput(_) => "EDIT".to_owned(),
            Self::Canvas(_) => super::graphics::canvas::CANVAS_CLASS_NAME.to_owned(),
            Self::TabBar(_) => "SysTabControl32".to_owned(),
            Self::Control { tag, .. } => format!("control `{tag}`"),
            Self::Surface { .. } => "surface".to_owned(),
            Self::Foreign { kind, .. } => format!("foreign `{kind}`"),
        }
    }

    pub(crate) fn hwnd(&self) -> HWND {
        match self {
            Self::Container { viewport, .. } => *viewport,
            Self::Label(hwnd)
            | Self::Button(hwnd)
            | Self::TextInput(hwnd)
            | Self::Canvas(hwnd)
            | Self::TabBar(hwnd)
            | Self::Control { hwnd, .. }
            | Self::Surface { hwnd, .. }
            | Self::Foreign { hwnd, .. } => *hwnd,
        }
    }

    pub(crate) fn content_hwnd(&self) -> Option<HWND> {
        match self {
            Self::Container { content, .. } => Some(*content),
            _ => None,
        }
    }

    pub(crate) fn destroy(self) {
        if let Self::Foreign { hwnd, ownership, .. } = self {
            super::foreign::release(hwnd, ownership);
            return;
        }
        if let Self::Control { hwnd, companion, .. } = &self {
            super::rendering::native_controls::forget(*hwnd);
            if let Some(companion) = companion {
                // SAFETY: the registry exclusively owns the companion too.
                unsafe { DestroyWindow(*companion) };
            }
        }
        // SAFETY: the registry exclusively owns every HWND stored here.
        unsafe {
            DestroyWindow(self.hwnd());
        }
    }
}

/// The top-level window `hwnd` belongs to, as an integer.
fn top_level(hwnd: HWND) -> isize {
    // The framework root, which for an embedded tree is not `GA_ROOT`.
    let root = super::context::root_window(hwnd);
    if root.is_null() { hwnd as isize } else { root as isize }
}

impl NativeObjectRegistry {
    /// Hands back every borrowed foreign object — called as the window
    /// closes, before Windows destroys its children with it.
    pub(crate) fn release_borrowed_foreign(&self) {
        for object in self.objects.values() {
            if let NativeObject::Foreign {
                hwnd,
                ownership: super::foreign::Ownership::Borrowed,
                ..
            } = object
            {
                super::foreign::release(*hwnd, super::foreign::Ownership::Borrowed);
            }
        }
    }

    /// How many native objects are realized.
    #[cfg_attr(not(test), allow(dead_code, reason = "read by the guarantee suites' leak gate"))]
    pub(crate) fn len(&self) -> usize {
        self.objects.len()
    }

    pub(crate) fn get(&self, id: NodeId) -> Option<&NativeObject> {
        self.objects.get(&id)
    }

    /// Every realized object.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&NodeId, &NativeObject)> {
        self.objects.iter()
    }

    /// Objects created and destroyed, most recent last.
    pub(crate) fn lifetimes(&self) -> rustnative_core::inspect::Lifetimes {
        self.lifetimes.clone()
    }

    fn log(&mut self, created: bool, id: NodeId, object: &NativeObject) {
        let lifetimes = &mut self.lifetimes;
        if created {
            lifetimes.created += 1;
        } else {
            lifetimes.destroyed += 1;
        }
        lifetimes.live = lifetimes.created.saturating_sub(lifetimes.destroyed);
        if lifetimes.recent.len() == RECENT_LIFETIMES {
            lifetimes.recent.remove(0);
        }
        lifetimes.recent.push(rustnative_core::inspect::LifetimeEvent {
            created,
            node: rustnative_core::inspect::node_name(id),
            key: id.local_key(),
            host_type: object.host_type(),
        });
    }

    pub(crate) fn insert(&mut self, id: NodeId, object: NativeObject) -> Result<(), Error> {
        if self.objects.contains_key(&id) {
            return Err(Error::DuplicateNodeId { node: id });
        }

        self.by_hwnd.insert(object.hwnd(), id);
        if let Some(content) = object.content_hwnd() {
            self.by_hwnd.insert(content, id);
        }
        let root = top_level(object.hwnd());
        self.top_levels.insert(id, root);
        crate::handle::record(id, root, object.hwnd() as isize);
        self.log(true, id, &object);
        self.objects.insert(id, object);
        Ok(())
    }

    pub(crate) fn remove(&mut self, id: NodeId) -> Option<NativeObject> {
        let object = self.objects.remove(&id)?;
        self.log(false, id, &object);
        if let Some(root) = self.top_levels.remove(&id) {
            crate::handle::forget(id, root);
        }
        self.by_hwnd.remove(&object.hwnd());
        if let Some(content) = object.content_hwnd() {
            self.by_hwnd.remove(&content);
        }
        Some(object)
    }

    pub(crate) fn id_for_hwnd(&self, hwnd: HWND) -> Option<NodeId> {
        self.by_hwnd.get(&hwnd).copied()
    }
}

impl Drop for NativeObjectRegistry {
    fn drop(&mut self) {
        self.by_hwnd.clear();
        for (id, root) in self.top_levels.drain() {
            crate::handle::forget(id, root);
        }
        for (_, object) in self.objects.drain() {
            object.destroy();
        }
    }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindow;

    use super::*;
    use crate::native::test_support::TestWindow;

    fn node_id(local: u64) -> NodeId {
        NodeId::from_key(&local.to_string())
    }

    #[test]
    fn insert_then_get_round_trips_by_node_id() {
        let window = TestWindow::new();
        let mut registry = NativeObjectRegistry::default();
        let id = node_id(1);
        registry.insert(id, NativeObject::Label(window.hwnd)).expect("first insert must succeed");
        assert_eq!(registry.get(id).map(NativeObject::hwnd), Some(window.hwnd));
    }

    #[test]
    fn insert_rejects_a_duplicate_node_id() {
        let window_a = TestWindow::new();
        let window_b = TestWindow::new();
        let mut registry = NativeObjectRegistry::default();
        let id = node_id(1);
        registry.insert(id, NativeObject::Label(window_a.hwnd)).expect("first insert succeeds");
        let result = registry.insert(id, NativeObject::Label(window_b.hwnd));
        assert!(matches!(result, Err(Error::DuplicateNodeId { .. })));
        // The rejected second insert must not have clobbered the first.
        assert_eq!(registry.get(id).map(NativeObject::hwnd), Some(window_a.hwnd));
    }

    #[test]
    fn id_for_hwnd_is_a_real_reverse_lookup() {
        let window = TestWindow::new();
        let mut registry = NativeObjectRegistry::default();
        let id = node_id(7);
        registry.insert(id, NativeObject::Button(window.hwnd)).expect("insert succeeds");
        assert_eq!(registry.id_for_hwnd(window.hwnd), Some(id));
    }

    #[test]
    fn container_reverse_lookup_resolves_both_viewport_and_content_hwnds() {
        let viewport = TestWindow::new();
        let content = TestWindow::new();
        let mut registry = NativeObjectRegistry::default();
        let id = node_id(3);
        registry
            .insert(id, NativeObject::Container { viewport: viewport.hwnd, content: content.hwnd })
            .expect("insert succeeds");
        assert_eq!(registry.id_for_hwnd(viewport.hwnd), Some(id));
        assert_eq!(
            registry.id_for_hwnd(content.hwnd),
            Some(id),
            "a container's inner content HWND must resolve to the same NodeId as its viewport"
        );
    }

    #[test]
    fn remove_clears_both_the_forward_and_reverse_lookup() {
        let window = TestWindow::new();
        let mut registry = NativeObjectRegistry::default();
        let id = node_id(4);
        registry.insert(id, NativeObject::Label(window.hwnd)).expect("insert succeeds");
        // `NativeObject` has no `Drop` impl of its own — destruction is
        // always the explicit, manual `.destroy()` call
        // `NativeObjectRegistry::drop` makes, never an automatic side
        // effect of a value going out of scope — so simply letting
        // `removed` go out of scope here is harmless and does not
        // double-destroy `window.hwnd`.
        let _removed = registry.remove(id).expect("remove must return the just-inserted object");

        assert!(registry.get(id).is_none());
        assert!(registry.id_for_hwnd(window.hwnd).is_none());
    }

    #[test]
    fn dropping_the_registry_destroys_every_native_window_it_still_owns() {
        // Deliberately does not use `TestWindow` here: the whole point of
        // this test is to prove `NativeObjectRegistry::drop` itself
        // destroys the HWND, so ownership must transfer to the registry,
        // not stay with a `TestWindow` guard that would destroy it again.
        let window = TestWindow::new();
        let hwnd = window.hwnd;
        std::mem::forget(window);

        {
            let mut registry = NativeObjectRegistry::default();
            registry.insert(node_id(9), NativeObject::Label(hwnd)).expect("insert succeeds");
            // SAFETY: `hwnd` is still alive here — the registry has not
            // been dropped yet — and `IsWindow` takes no pointer
            // arguments beyond the handle itself.
            let is_window_before_drop = unsafe { IsWindow(hwnd) };
            assert_ne!(is_window_before_drop, 0, "hwnd must be a live window before drop");
        }

        // SAFETY: querying a handle after it may have been destroyed is
        // exactly what `IsWindow` is documented to make safe to do.
        let is_window_after_drop = unsafe { IsWindow(hwnd) };
        assert_eq!(
            is_window_after_drop, 0,
            "NativeObjectRegistry::drop must have destroyed every HWND it owned"
        );
    }
}
