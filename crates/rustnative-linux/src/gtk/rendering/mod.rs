//! Realizing one window's tree as GTK widgets: the host objects, their
//! registry, and the renderer that reconciles them against each new tree.

pub(crate) mod animated;
pub(crate) mod controls;
pub(crate) mod realization;
pub(crate) mod styling;

use std::collections::HashMap;

use gtk::glib;
use gtk::prelude::*;
use rustnative_core::{NodeId, NodeKind};

use super::layout_widget::RnLayout;

/// One node's native realization.
pub(crate) struct HostObject {
    /// The node's kind when it was realized.
    pub(crate) kind: NodeKind,
    /// The widget its parent places.
    pub(crate) widget: gtk::Widget,
    /// Where its children go: the container itself, or the content inside
    /// a scroll container's viewport. `None` for a leaf.
    pub(crate) content: Option<RnLayout>,
    /// The role the widget was constructed with (GTK fixes it then).
    pub(crate) role: gtk::AccessibleRole,
    /// What decided the widget's type beyond its kind (a control's variant,
    /// a container's scrolling), so a change to it replaces the widget.
    pub(crate) shape: Shape,
    /// The signal handlers that report the person's changes, blocked while
    /// the framework makes a change of its own so it does not echo back.
    pub(crate) signals: Vec<(glib::Object, glib::SignalHandlerId)>,
    /// The node's drop target, while it declares drop interest.
    pub(crate) drop: Option<gtk::DropTarget>,
}

impl std::fmt::Debug for HostObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostObject")
            .field("kind", &self.kind)
            .field("widget", &self.widget.type_().name())
            .field("shape", &self.shape)
            .finish_non_exhaustive()
    }
}

/// What, beyond its kind, decided which widget realizes a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Shape {
    /// Nothing more: the kind decides.
    Plain,
    /// A container that scrolls (a `GtkScrolledWindow` around its content).
    Scrolling,
    /// A container that is a custom control to assistive technology
    /// (`RnControl`).
    Interactive,
    /// A control of this variant (its discriminant's name).
    Control(&'static str),
    /// A tab bar with this many tabs.
    Tabs(usize),
    /// A foreign widget of this factory kind.
    Foreign(String),
}

impl HostObject {
    /// Runs `f` with this object's handlers blocked, so a change the
    /// framework makes is not reported back as the person's.
    pub(crate) fn quietly<R>(&self, f: impl FnOnce() -> R) -> R {
        for (object, handler) in &self.signals {
            object.block_signal(handler);
        }
        let result = f();
        for (object, handler) in &self.signals {
            object.unblock_signal(handler);
        }
        result
    }
}

/// Every host object of one window, by node.
#[derive(Debug, Default)]
pub(crate) struct NativeRegistry {
    objects: HashMap<NodeId, HostObject>,
    /// Which node each realized widget belongs to, for input hit-testing.
    by_widget: HashMap<gtk::Widget, NodeId>,
    /// Host objects created and destroyed, for the lifetime census and the
    /// inspector.
    pub(crate) created: u64,
    pub(crate) destroyed: u64,
}

impl NativeRegistry {
    pub(crate) fn get(&self, id: NodeId) -> Option<&HostObject> {
        self.objects.get(&id)
    }

    pub(crate) fn get_mut(&mut self, id: NodeId) -> Option<&mut HostObject> {
        self.objects.get_mut(&id)
    }

    pub(crate) fn insert(&mut self, id: NodeId, object: HostObject) {
        self.created += 1;
        self.by_widget.insert(object.widget.clone(), id);
        if let Some(previous) = self.objects.insert(id, object) {
            // The diff never inserts an id twice; if it did, the old widget
            // is this registry's to release rather than leak.
            self.by_widget.remove(&previous.widget);
            unparent(&previous.widget);
            self.destroyed += 1;
        }
    }

    pub(crate) fn remove(&mut self, id: NodeId) -> Option<HostObject> {
        let removed = self.objects.remove(&id);
        if let Some(object) = &removed {
            self.by_widget.remove(&object.widget);
            self.destroyed += 1;
        }
        removed
    }

    /// Removes `id`'s object without counting it destroyed: a recycled
    /// virtual-list row, about to be [`Self::adopt`]ed by another.
    pub(crate) fn take(&mut self, id: NodeId) -> Option<HostObject> {
        let taken = self.objects.remove(&id);
        if let Some(object) = &taken {
            self.by_widget.remove(&object.widget);
        }
        taken
    }

    /// Registers a recycled object for `id`, without counting it created.
    pub(crate) fn adopt(&mut self, id: NodeId, object: HostObject) {
        self.by_widget.insert(object.widget.clone(), id);
        self.objects.insert(id, object);
    }

    pub(crate) fn len(&self) -> usize {
        self.objects.len()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (NodeId, &HostObject)> {
        self.objects.iter().map(|(id, object)| (*id, object))
    }

    /// The node realized by `widget` (or by the nearest ancestor of it that
    /// realizes a node).
    pub(crate) fn node_for_widget(&self, widget: &gtk::Widget) -> Option<NodeId> {
        let mut current = Some(widget.clone());
        while let Some(candidate) = current {
            if let Some(id) = self.by_widget.get(&candidate) {
                return Some(*id);
            }
            current = candidate.parent();
        }
        None
    }
}

/// Takes `widget` out of whichever container holds it.
pub(crate) fn unparent(widget: &gtk::Widget) {
    if let Some(parent) = widget.parent() {
        if let Some(layout) = parent.downcast_ref::<RnLayout>() {
            layout.remove(widget);
        } else {
            widget.unparent();
        }
    }
}
