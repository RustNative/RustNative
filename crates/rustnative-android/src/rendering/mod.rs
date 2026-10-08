//! Realizing one window's tree as Android views: the host objects, their
//! registry, and the renderer that reconciles them against each new tree.

pub(crate) mod animated;
pub(crate) mod controls;
pub(crate) mod realization;

use std::collections::HashMap;

use rustnative_core::{NodeId, NodeKind};

use crate::jni_host::JavaRef;

/// One node's native realization.
#[derive(Debug)]
pub(crate) struct HostObject {
    /// The node's kind when it was realized.
    pub(crate) kind: NodeKind,
    /// The host library's kind it was created as (`protocol`).
    pub(crate) java_kind: i32,
    /// The view its parent places.
    pub(crate) view: JavaRef,
    /// Where its children go: the container itself, or the content layout
    /// inside a scroll container. `None` for a leaf.
    pub(crate) content: Option<JavaRef>,
    /// What decided the view's class beyond its kind, so a change to it
    /// replaces the view.
    pub(crate) shape: Shape,
    /// The number the view reports its events with.
    pub(crate) tag: i32,
    /// The style last applied, so an unchanged one is not sent again.
    pub(crate) style: Option<u64>,
    /// Whether the view is a drop target now.
    pub(crate) drop: bool,
    /// The pointer icon applied (`None`: the view's own).
    pub(crate) cursor: Option<rustnative_core::Cursor>,
}

/// What, beyond its kind, decided which view realizes a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Shape {
    /// Nothing more: the kind decides.
    Plain,
    /// A container that scrolls along these axes (1 horizontal, 2
    /// vertical, 3 both).
    Scrolling(i32),
    /// A control of this variant.
    Control(&'static str),
    /// A foreign view of this factory kind.
    Foreign(String),
}

/// Every host object of one window, by node and by tag.
#[derive(Debug, Default)]
pub(crate) struct NativeRegistry {
    objects: HashMap<NodeId, HostObject>,
    by_tag: HashMap<i32, NodeId>,
    next_tag: i32,
    /// Host objects created and destroyed, for the lifetime census and the
    /// inspector.
    pub(crate) created: u64,
    pub(crate) destroyed: u64,
}

impl NativeRegistry {
    /// A fresh tag for a view about to be created.
    pub(crate) fn allocate_tag(&mut self) -> i32 {
        self.next_tag = self.next_tag.wrapping_add(1).max(1);
        self.next_tag
    }

    pub(crate) fn get(&self, id: NodeId) -> Option<&HostObject> {
        self.objects.get(&id)
    }

    pub(crate) fn get_mut(&mut self, id: NodeId) -> Option<&mut HostObject> {
        self.objects.get_mut(&id)
    }

    /// The node whose view reports with `tag`.
    pub(crate) fn node_for_tag(&self, tag: i32) -> Option<NodeId> {
        self.by_tag.get(&tag).copied()
    }

    pub(crate) fn insert(&mut self, id: NodeId, object: HostObject) {
        self.created += 1;
        self.by_tag.insert(object.tag, id);
        if let Some(previous) = self.objects.insert(id, object) {
            self.by_tag.remove(&previous.tag);
            detach(&previous.view);
            self.destroyed += 1;
        }
    }

    pub(crate) fn remove(&mut self, id: NodeId) -> Option<HostObject> {
        let removed = self.objects.remove(&id);
        if let Some(object) = &removed {
            self.by_tag.remove(&object.tag);
            self.destroyed += 1;
        }
        removed
    }

    /// Removes `id`'s object without counting it destroyed: a recycled
    /// virtual-list row, about to be [`Self::adopt`]ed by another.
    pub(crate) fn take(&mut self, id: NodeId) -> Option<HostObject> {
        let taken = self.objects.remove(&id);
        if let Some(object) = &taken {
            self.by_tag.remove(&object.tag);
        }
        taken
    }

    /// Registers a recycled object for `id`, without counting it created.
    /// The view keeps reporting with its tag, which now names `id`.
    pub(crate) fn adopt(&mut self, id: NodeId, object: HostObject) {
        self.by_tag.insert(object.tag, id);
        self.objects.insert(id, object);
    }

    pub(crate) fn len(&self) -> usize {
        self.objects.len()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (NodeId, &HostObject)> {
        self.objects.iter().map(|(id, object)| (*id, object))
    }
}

/// Takes `view` out of whichever container holds it.
pub(crate) fn detach(view: &JavaRef) {
    use crate::jni_host::{Arg, Class, call_static};
    let _ = call_static(Class::Layout, "detach", "(Landroid/view/View;)V", &[Arg::Obj(view)]);
}
