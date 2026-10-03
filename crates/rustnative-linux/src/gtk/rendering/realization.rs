//! The renderer: applies a `TreeDiff` to GTK widgets, then lays the window
//! out with the portable engine and places every widget.
//!
//! The phases are the Windows renderer's, in the same order and for the
//! same reason — every widget that will exist this frame exists before any
//! of them is placed:
//!
//! ```text
//! render(root)
//!   ├─ snapshot the declarative tree, resolving theme styles
//!   ├─ diff against the previous snapshot
//!   ├─ apply each operation   → controls / styling
//!   ├─ put each touched container's children in declarative order
//!   └─ if the diff touched geometry, relayout
//!         ├─ run the platform-independent layout engine
//!         ├─ mirror right-to-left containers (GTK does not mirror a
//!         │  container it does not lay out)
//!         └─ place every widget in its container
//! ```

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gtk::prelude::*;
use rustnative_core::{
    AnimatedOverrides, LayoutDirection, LayoutEngine, LayoutResult, NodeId, NodeKind, Rect, Size,
    Theme, TreeDiff, TreeNode, TreeOp, TreeSnapshot,
};

use super::controls;
use super::styling::StyleSheet;
use super::{NativeRegistry, unparent};
use crate::Error;
use crate::gtk::layout_widget::RnLayout;
use crate::gtk::measure::{GtkMeasurer, Prototypes};

/// The CSS cursor name for a portable cursor, which GDK resolves through
/// the desktop's cursor theme.
const fn cursor_name(cursor: rustnative_core::Cursor) -> &'static str {
    use rustnative_core::Cursor;
    match cursor {
        Cursor::Pointer => "pointer",
        Cursor::Text => "text",
        Cursor::Crosshair => "crosshair",
        Cursor::Move => "move",
        Cursor::NotAllowed => "not-allowed",
        Cursor::ResizeVertical => "ns-resize",
        Cursor::ResizeHorizontal => "ew-resize",
        Cursor::Wait => "wait",
        Cursor::Progress => "progress",
        Cursor::Help => "help",
        Cursor::Default => "default",
    }
}

/// Reconciles one window's widgets against the framework's tree.
pub(crate) struct Renderer {
    window: rustnative_core::WindowId,
    /// Every widget this window owns.
    pub(crate) registry: NativeRegistry,
    /// The tree currently realized, and the baseline the next render diffs
    /// against.
    pub(crate) snapshot: TreeSnapshot,
    /// Each node's rectangle in its native parent, as the engine computed
    /// it (logical: before right-to-left mirroring).
    layout: HashMap<NodeId, Rect>,
    /// What each widget was placed at (physical), before animation.
    pub(super) placed: HashMap<NodeId, Rect>,
    /// The last layout's content sizes (for the inspector and scrolling).
    content_sizes: HashMap<NodeId, Size>,
    engine: LayoutEngine,
    prototypes: Prototypes,
    pub(super) styles: Rc<RefCell<StyleSheet>>,
    pub(super) theme: Theme,
    direction: LayoutDirection,
    /// Set when every node must be restyled (a text-scale change).
    restyle_all: bool,
    /// The portable accessibility model, applied to the widgets.
    accessibility: crate::gtk::accessibility::AccessibilityBridge,
    /// Per-frame animated values (`rustnative_core::AnimatedOverrides`).
    pub(super) animated: AnimatedOverrides,
    /// Transitions found, waiting for the window's timeline.
    pub(super) pending_transitions: Vec<super::animated::TransitionRequest>,
    /// Nodes removed, whose animations are over.
    pub(super) removed_nodes: Vec<NodeId>,
    /// Nodes arriving in place of one with the same shared identity.
    pub(super) matched: Vec<rustnative_core::MatchedGeometry>,
    /// The providers animated colours are applied through, per node.
    pub(super) animation_providers: HashMap<NodeId, gtk::CssProvider>,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer").field("objects", &self.registry.len()).finish_non_exhaustive()
    }
}

impl Renderer {
    pub(crate) fn new(window: rustnative_core::WindowId, styles: Rc<RefCell<StyleSheet>>) -> Self {
        Self {
            window,
            registry: NativeRegistry::default(),
            snapshot: TreeSnapshot::default(),
            layout: HashMap::new(),
            placed: HashMap::new(),
            content_sizes: HashMap::new(),
            engine: LayoutEngine,
            prototypes: Prototypes::default(),
            styles,
            theme: Theme::default(),
            direction: LayoutDirection::Ltr,
            restyle_all: false,
            accessibility: crate::gtk::accessibility::AccessibilityBridge::default(),
            animated: AnimatedOverrides::default(),
            pending_transitions: Vec::new(),
            removed_nodes: Vec::new(),
            matched: Vec::new(),
            animation_providers: HashMap::new(),
        }
    }

    /// A node's laid-out size, if it has been laid out.
    pub(crate) fn layout_size(&self, id: NodeId) -> Option<Size> {
        self.layout.get(&id).map(|rect| {
            Size::new(
                u32::try_from(rect.width.max(0)).unwrap_or(0),
                u32::try_from(rect.height.max(0)).unwrap_or(0),
            )
        })
    }

    /// Every laid-out node's rectangle, in its parent's coordinates
    /// (physical: as placed).
    #[cfg(test)]
    pub(crate) const fn placed_rects(&self) -> &HashMap<NodeId, Rect> {
        &self.placed
    }

    /// Asks for every node to be restyled on the next render.
    pub(crate) fn restyle_all(&mut self) {
        self.restyle_all = true;
    }

    /// Reconciles the widgets against `root`, then relayouts if the change
    /// could have moved anything. Returns whether it relaid out.
    pub(crate) fn render(
        &mut self,
        root: &rustnative_core::Node,
        theme: &Theme,
        direction: LayoutDirection,
        window_root: &RnLayout,
        size: Size,
    ) -> Result<bool, Error> {
        let next =
            TreeSnapshot::from_node_with_theme(root, theme).map_err(|error| match error {
                rustnative_core::TreeError::DuplicateNodeId(id) => {
                    Error::DuplicateNodeId { node: id }
                }
            })?;
        let diff = TreeDiff::between(&self.snapshot, &next);
        // Before the operations: they apply the new tree, and a transition
        // must start from what is on screen.
        self.matched = rustnative_core::matched_geometry(&self.snapshot, &self.placed, &next);
        self.collect_appearance_transitions(&next);
        let theme_changed = &self.theme != theme;
        self.theme = theme.clone();
        let direction_changed = std::mem::replace(&mut self.direction, direction) != direction;
        let restyle_all = std::mem::take(&mut self.restyle_all) || theme_changed;

        let mut touched_parents: HashSet<Option<NodeId>> = HashSet::new();
        let mut replaced = false;
        // The new snapshot answers "what are this node's children" while
        // operations run; the old one answers "what was it".
        let previous = std::mem::replace(&mut self.snapshot, next);
        for operation in diff.operations() {
            match operation {
                TreeOp::Insert(node) => {
                    touched_parents.insert(node.parent);
                    self.insert(node, window_root)?;
                }
                TreeOp::Update(node) => {
                    let replace = self
                        .registry
                        .get(node.id)
                        .is_some_and(|object| controls::needs_replacement(object, node));
                    if replace {
                        touched_parents.insert(node.parent);
                        self.replace(node, window_root)?;
                        replaced = true;
                    } else if let Some(object) = self.registry.get(node.id) {
                        controls::update(object, node);
                        self.apply_appearance(node);
                    }
                }
                TreeOp::Move { id, parent, .. } => {
                    touched_parents.insert(*parent);
                    touched_parents.insert(previous.get(*id).and_then(|node| node.parent));
                    self.reparent(*id, *parent, window_root);
                }
                TreeOp::Remove(node) => {
                    touched_parents.insert(node.parent);
                    self.remove(node.id);
                }
            }
        }
        if restyle_all {
            for node in self.snapshot.nodes().cloned().collect::<Vec<_>>() {
                self.apply_appearance(&node);
            }
        }
        for parent in touched_parents {
            self.reorder(parent, window_root);
        }
        self.group_radios();
        self.accessibility.commit(&self.snapshot, &self.registry, self.window);
        self.styles.borrow_mut().flush();
        let relayout = diff.invalidates_layout() || replaced || direction_changed || restyle_all;
        if relayout {
            self.relayout(window_root, size);
        }
        Ok(relayout)
    }

    /// The container widget a node's children go into.
    pub(super) fn content_of(&self, parent: Option<NodeId>, window_root: &RnLayout) -> RnLayout {
        parent
            .and_then(|parent| self.registry.get(parent))
            .and_then(|object| object.content.clone())
            .unwrap_or_else(|| window_root.clone())
    }

    fn insert(&mut self, node: &TreeNode, window_root: &RnLayout) -> Result<(), Error> {
        let object = controls::create(node, self.window)?;
        self.content_of(node.parent, window_root).append(&object.widget);
        self.registry.insert(node.id, object);
        self.apply_appearance(node);
        Ok(())
    }

    /// Replaces a node's widget with a new one of the right type, moving
    /// its children's widgets into the new container.
    fn replace(&mut self, node: &TreeNode, window_root: &RnLayout) -> Result<(), Error> {
        let children: Vec<NodeId> =
            self.snapshot.children_of(node.id).map(|child| child.id).collect();
        if let Some(old) = self.registry.remove(node.id) {
            unparent(&old.widget);
        }
        self.insert(node, window_root)?;
        let content = self.content_of(Some(node.id), window_root);
        for child in children {
            if let Some(object) = self.registry.get(child) {
                unparent(&object.widget);
                content.append(&object.widget);
            }
        }
        self.placed.retain(|id, _| *id != node.id);
        Ok(())
    }

    fn reparent(&mut self, id: NodeId, parent: Option<NodeId>, window_root: &RnLayout) {
        let content = self.content_of(parent, window_root);
        if let Some(object) = self.registry.get(id) {
            if object.widget.parent().as_ref() != Some(content.upcast_ref()) {
                unparent(&object.widget);
                content.append(&object.widget);
                self.placed.remove(&id);
            }
        }
    }

    fn remove(&mut self, id: NodeId) {
        if let Some(object) = self.registry.remove(id) {
            unparent(&object.widget);
        }
        self.layout.remove(&id);
        self.placed.remove(&id);
        self.animated.forget(id);
        self.removed_nodes.push(id);
        self.animation_providers.remove(&id);
    }

    /// Puts a container's children in declarative order, which is also the
    /// order GTK — and so AT-SPI — reports them in.
    fn reorder(&self, parent: Option<NodeId>, window_root: &RnLayout) {
        let content = self.content_of(parent, window_root);
        let mut children: Vec<&TreeNode> = match parent {
            Some(parent) => self.snapshot.children_of(parent).collect(),
            None => self.snapshot.nodes().filter(|node| node.parent.is_none()).collect(),
        };
        children.sort_by_key(|node| node.index);
        let mut previous: Option<gtk::Widget> = None;
        for child in children {
            let Some(object) = self.registry.get(child.id) else { continue };
            if object.widget.parent().as_ref() != Some(content.upcast_ref()) {
                continue;
            }
            let in_place = match &previous {
                Some(previous) => object.widget.prev_sibling().as_ref() == Some(previous),
                None => object.widget.prev_sibling().is_none(),
            };
            if !in_place {
                object.widget.insert_after(&content, previous.as_ref());
            }
            previous = Some(object.widget.clone());
        }
    }

    /// Joins the radio buttons of each container into one GTK group, so
    /// they draw as radios and AT-SPI reads them as one set.
    fn group_radios(&self) {
        let mut groups: HashMap<Option<NodeId>, Vec<gtk::CheckButton>> = HashMap::new();
        let mut radios: Vec<&TreeNode> = self
            .snapshot
            .nodes()
            .filter(|node| matches!(node.control, Some(rustnative_core::Control::Radio { .. })))
            .collect();
        radios.sort_by_key(|node| node.index);
        for node in radios {
            if let Some(check) = self
                .registry
                .get(node.id)
                .and_then(|object| object.widget.downcast_ref::<gtk::CheckButton>())
            {
                groups.entry(node.parent).or_default().push(check.clone());
            }
        }
        for members in groups.values() {
            let Some((first, rest)) = members.split_first() else { continue };
            for member in rest {
                let widget = member.upcast_ref::<gtk::Widget>();
                if let Some((_, object)) =
                    self.registry.iter().find(|(_, object)| object.widget == *widget)
                {
                    object.quietly(|| member.set_group(Some(first)));
                }
            }
        }
    }

    /// Realizes a node's style, visibility, sensitivity, and opacity.
    fn apply_appearance(&mut self, node: &TreeNode) {
        self.apply_style_class(node);
        self.apply_opacity(node.id);
        let Some(object) = self.registry.get(node.id) else { return };
        if object.widget.is_visible() == node.hidden {
            object.widget.set_visible(!node.hidden);
        }
        if object.widget.is_sensitive() == node.disabled {
            object.widget.set_sensitive(!node.disabled);
        }
        // A widget without a cursor of its own shows its parent's, which is
        // "the nearest declared cursor walking up" by construction.
        let cursor = node.cursor.map(cursor_name);
        if object.widget.cursor().and_then(|cursor| cursor.name()).as_deref() != cursor {
            object.widget.set_cursor_from_name(cursor);
        }
        // A container that declares itself focusable takes focus like a
        // control does: from Tab (GTK's traversal) and from a click.
        if Self::is_container(node.kind) {
            let focusable = node.accessibility.is_focusable() && !node.disabled;
            let focus_target =
                object.content.as_ref().map_or(&object.widget, |content| content.upcast_ref());
            if focus_target.is_focusable() != focusable {
                focus_target.set_focusable(focusable);
                focus_target.set_focus_on_click(focusable);
            }
        }
        let window = self.window;
        if let Some(object) = self.registry.get_mut(node.id) {
            crate::gtk::input::sync_drop_target(object, node, window);
        }
    }

    /// Lays the whole window out at `size` and places every widget.
    pub(crate) fn relayout(&mut self, window_root: &RnLayout, size: Size) {
        let output = self.lay_out(size);
        let physical = output.physical_rects(&self.snapshot, self.direction);
        let directions = LayoutResult::directions(&self.snapshot, self.direction);
        // Layout is where a node's position and size change, so it is where
        // their transitions begin.
        self.collect_geometry_transitions(&physical);
        self.layout = output.rects;
        self.content_sizes = output.content_sizes;
        for node in self.snapshot.nodes() {
            let Some(object) = self.registry.get(node.id) else { continue };
            // The layout engine mirrors where widgets go; GTK mirrors what
            // is drawn inside each one (text alignment, a check box's side).
            let direction = match directions.get(&node.id) {
                Some(direction) if direction.is_rtl() => gtk::TextDirection::Rtl,
                _ => gtk::TextDirection::Ltr,
            };
            if object.widget.direction() != direction {
                object.widget.set_direction(direction);
            }
            let Some(rect) = physical.get(&node.id).copied() else { continue };
            let container = self.content_of(node.parent, window_root);
            // Whatever animates the node's geometry wins until it ends.
            container.place(&object.widget, self.animated.rect_for(node.id, rect));
            self.placed.insert(node.id, rect);
            if let (Some(content), true) = (&object.content, controls::scrolls(node)) {
                let content_size = self.content_sizes.get(&node.id).copied().unwrap_or_default();
                content.set_content_size(Some((
                    i32::try_from(content_size.width).unwrap_or(i32::MAX).max(rect.width),
                    i32::try_from(content_size.height).unwrap_or(i32::MAX).max(rect.height),
                )));
            }
        }
    }

    fn lay_out(&self, size: Size) -> LayoutResult {
        let measurer = GtkMeasurer::new(&self.prototypes, &self.styles);
        self.engine.layout_result_with(&self.snapshot, size, &measurer, &HashMap::new())
    }

    /// The widget realizing `id`.
    pub(crate) fn widget(&self, id: NodeId) -> Option<&gtk::Widget> {
        self.registry.get(id).map(|object| &object.widget)
    }

    /// Releases every widget.
    pub(crate) fn release(&mut self) {
        let ids: Vec<NodeId> = self.registry.iter().map(|(id, _)| id).collect();
        for id in ids {
            self.remove(id);
        }
        self.snapshot = TreeSnapshot::default();
    }

    /// Whether `kind` is a container (its focusability is the framework's).
    pub(crate) const fn is_container(kind: NodeKind) -> bool {
        matches!(kind, NodeKind::Column | NodeKind::Row)
    }
}
