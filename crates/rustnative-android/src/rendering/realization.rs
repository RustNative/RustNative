//! The renderer: applies a `TreeDiff` to Android views, then lays the
//! window out with the portable engine and places every view.
//!
//! The phases are the Windows and Linux renderers', in the same order and
//! for the same reason — every view that will exist this frame exists
//! before any of them is placed:
//!
//! ```text
//! render(root)
//!   ├─ snapshot the declarative tree, resolving theme styles
//!   ├─ diff against the previous snapshot
//!   ├─ apply each operation   → controls / styling
//!   ├─ put each touched container's children in declarative order
//!   └─ if the diff touched geometry, relayout
//!         ├─ run the platform-independent layout engine (in dp)
//!         ├─ mirror right-to-left containers
//!         └─ place every view in its container (in device pixels)
//! ```
//!
//! The whole render runs with the views' listeners muted
//! (`RnBridge.muted`), so nothing the framework sets echoes back.

use std::collections::{HashMap, HashSet};

use rustnative_core::virtualization::{index_of, item_at};
use rustnative_core::{
    AnimatedOverrides, LayoutDirection, LayoutEngine, LayoutResult, NodeId, NodeKind, Point, Rect,
    Size, Theme, TreeDiff, TreeNode, TreeOp, TreeSnapshot, VirtualLists, VirtualRange,
};

use super::{HostObject, NativeRegistry, controls, detach};
use crate::Error;
use crate::jni_host::{Arg, Class, JavaRef, call_static};
use crate::measure::{AndroidMeasurer, Measurements};
use crate::styling::StyleSheet;
use crate::units::to_px;

/// Reconciles one window's views against the framework's tree.
pub(crate) struct Renderer {
    window: u64,
    /// The activity views are created in (their `Context`).
    activity: JavaRef,
    /// Every view this window owns.
    pub(crate) registry: NativeRegistry,
    /// The tree currently realized, and the baseline the next render diffs
    /// against.
    pub(crate) snapshot: TreeSnapshot,
    /// Each node's rectangle in its native parent, as the engine computed
    /// it (logical: dp, before right-to-left mirroring).
    layout: HashMap<NodeId, Rect>,
    /// What each view was placed at (physical dp), before animation.
    pub(crate) placed: HashMap<NodeId, Rect>,
    /// The last layout's content sizes (dp).
    content_sizes: HashMap<NodeId, Size>,
    engine: LayoutEngine,
    measurements: Measurements,
    pub(crate) styles: StyleSheet,
    pub(crate) theme: Theme,
    direction: LayoutDirection,
    /// Set when every node must be restyled (a text-scale or density change).
    restyle_all: bool,
    pub(crate) density: f32,
    pub(crate) text_scale: f32,
    /// Per-frame animated values.
    pub(crate) animated: AnimatedOverrides,
    /// Every virtual list's extents and visible range.
    pub(crate) virtual_lists: VirtualLists,
    /// Views a virtual list's removed rows left, for the rows this same
    /// render inserts: keyed by the container's tag and the row's kind.
    pool: HashMap<(i32, i32), Vec<HostObject>>,
    /// Nodes removed this render, whose animations are over.
    pub(crate) removed_nodes: Vec<NodeId>,
    /// Transitions found, waiting for the window's timeline.
    pub(crate) pending_transitions: Vec<super::animated::TransitionRequest>,
    /// Nodes arriving in place of one with the same shared identity.
    pub(crate) matched: Vec<rustnative_core::MatchedGeometry>,
    /// The portable accessibility model, applied to the views.
    pub(crate) accessibility: crate::accessibility::AccessibilityBridge,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer").field("objects", &self.registry.len()).finish_non_exhaustive()
    }
}

impl Renderer {
    pub(crate) fn new(window: u64, activity: JavaRef, density: f32) -> Self {
        Self {
            window,
            activity,
            registry: NativeRegistry::default(),
            snapshot: TreeSnapshot::default(),
            layout: HashMap::new(),
            placed: HashMap::new(),
            content_sizes: HashMap::new(),
            engine: LayoutEngine,
            measurements: Measurements::default(),
            styles: StyleSheet::default(),
            theme: Theme::default(),
            direction: LayoutDirection::Ltr,
            restyle_all: false,
            density,
            text_scale: 1.0,
            animated: AnimatedOverrides::default(),
            virtual_lists: VirtualLists::default(),
            pool: HashMap::new(),
            removed_nodes: Vec::new(),
            pending_transitions: Vec::new(),
            matched: Vec::new(),
            accessibility: crate::accessibility::AccessibilityBridge::default(),
        }
    }

    /// A node's laid-out size (dp), if it has been laid out.
    pub(crate) fn layout_size(&self, id: NodeId) -> Option<Size> {
        self.layout.get(&id).map(|rect| {
            Size::new(
                u32::try_from(rect.width.max(0)).unwrap_or(0),
                u32::try_from(rect.height.max(0)).unwrap_or(0),
            )
        })
    }

    /// Asks for every node to be restyled and re-measured on the next
    /// render.
    pub(crate) fn restyle_all(&mut self) {
        self.restyle_all = true;
    }

    /// Sets the density and text scale; a change restyles and re-measures.
    pub(crate) fn set_scales(&mut self, density: f32, text_scale: f32) {
        if self.styles.set_scales(text_scale, density) {
            self.density = density;
            self.text_scale = text_scale;
            self.restyle_all = true;
        }
    }

    /// Reconciles the views against `root`, then relayouts if the change
    /// could have moved anything. Returns whether it relaid out.
    pub(crate) fn render(
        &mut self,
        root: &rustnative_core::Node,
        theme: &Theme,
        direction: LayoutDirection,
        window_root: &JavaRef,
        size: Size,
    ) -> Result<bool, Error> {
        let _muted = Muted::new();
        let next =
            TreeSnapshot::from_node_with_theme(root, theme).map_err(|error| match error {
                rustnative_core::TreeError::DuplicateNodeId(id) => {
                    Error::DuplicateNodeId { node: id }
                }
            })?;
        let diff = TreeDiff::between(&self.snapshot, &next);
        if diff.invalidates_layout() && !self.virtual_lists.is_empty() {
            let Self { virtual_lists, snapshot, registry, .. } = self;
            virtual_lists.capture_anchors(
                |id| scroll_offset(registry, id, 1.0),
                |list, index| item_at(snapshot, list, index),
            );
        }
        // Before the operations: they apply the new tree, and a transition
        // must start from what is on screen.
        self.matched = rustnative_core::matched_geometry(&self.snapshot, &self.placed, &next);
        self.collect_appearance_transitions(&next);
        let theme_changed = &self.theme != theme;
        self.theme = theme.clone();
        let direction_changed = std::mem::replace(&mut self.direction, direction) != direction;
        let restyle_all = std::mem::take(&mut self.restyle_all) || theme_changed;
        if restyle_all {
            self.measurements.forget();
        }

        let mut touched_parents: HashSet<Option<NodeId>> = HashSet::new();
        let mut replaced = false;
        let previous = std::mem::replace(&mut self.snapshot, next);
        for operation in diff.operations() {
            match operation {
                TreeOp::Insert(node) => {
                    touched_parents.insert(node.parent);
                    if !self.reuse(node, window_root)? {
                        self.insert(node, window_root)?;
                    }
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
                        crate::mappers::apply(
                            &object.view,
                            node,
                            crate::mappers::MappedProperty::Text,
                            || controls::update(object, node),
                        )?;
                        self.apply_appearance(node)?;
                    }
                }
                TreeOp::Move { id, parent, .. } => {
                    touched_parents.insert(*parent);
                    touched_parents.insert(previous.get(*id).and_then(|node| node.parent));
                    self.reparent(*id, *parent, window_root)?;
                }
                TreeOp::Remove(node) => {
                    touched_parents.insert(node.parent);
                    if is_virtual_item(&previous, node) {
                        self.park(node, window_root);
                    } else {
                        self.remove(node.id);
                    }
                }
            }
        }
        for object in std::mem::take(&mut self.pool).into_values().flatten() {
            detach(&object.view);
            self.registry.destroyed += 1;
        }
        self.virtual_lists.sync(&self.snapshot);
        if restyle_all {
            for node in self.snapshot.nodes().cloned().collect::<Vec<_>>() {
                if let Some(object) = self.registry.get_mut(node.id) {
                    object.style = None;
                }
                self.apply_appearance(&node)?;
            }
        }
        for parent in touched_parents {
            self.reorder(parent, window_root)?;
        }
        self.accessibility.commit(&self.snapshot, &self.registry, self.window, self.density);
        let relayout = diff.invalidates_layout() || replaced || direction_changed || restyle_all;
        if relayout {
            self.relayout(window_root, size)?;
        }
        Ok(relayout)
    }

    /// The container view a node's children go into.
    pub(crate) fn content_of(&self, parent: Option<NodeId>, window_root: &JavaRef) -> JavaRef {
        parent
            .and_then(|parent| self.registry.get(parent))
            .and_then(|object| object.content.clone())
            .unwrap_or_else(|| window_root.clone())
    }

    fn container_tag(&self, parent: Option<NodeId>) -> i32 {
        parent.and_then(|parent| self.registry.get(parent)).map_or(0, |object| object.tag)
    }

    /// Parks a removed virtual-list row's view for a row this render
    /// inserts into the same list.
    fn park(&mut self, node: &TreeNode, _window_root: &JavaRef) {
        let key = (
            self.container_tag(node.parent),
            self.registry.get(node.id).map_or(0, |object| object.java_kind),
        );
        let Some(object) = self.registry.take(node.id) else { return };
        self.layout.remove(&node.id);
        self.placed.remove(&node.id);
        self.animated.forget(node.id);
        self.removed_nodes.push(node.id);
        self.pool.entry(key).or_default().push(object);
    }

    /// Realizes `node` on a parked view of the same shape in the same list.
    fn reuse(&mut self, node: &TreeNode, _window_root: &JavaRef) -> Result<bool, Error> {
        if node.foreign.is_some() || !is_virtual_item(&self.snapshot, node) {
            return Ok(false);
        }
        let key = (self.container_tag(node.parent), controls::java_kind(node));
        let Some(index) = self.pool.get(&key).and_then(|parked| {
            parked.iter().position(|object| !controls::needs_replacement(object, node))
        }) else {
            return Ok(false);
        };
        let Some(object) = self.pool.get_mut(&key).map(|parked| parked.swap_remove(index)) else {
            return Ok(false);
        };
        crate::mappers::apply(&object.view, node, crate::mappers::MappedProperty::Text, || {
            controls::update(&object, node)
        })?;
        self.registry.adopt(node.id, object);
        self.apply_appearance(node)?;
        Ok(true)
    }

    fn insert(&mut self, node: &TreeNode, window_root: &JavaRef) -> Result<(), Error> {
        let tag = self.registry.allocate_tag();
        let object = if let Some(kind) = &node.foreign {
            crate::foreign::create(&self.activity, kind, node, self.window, tag)?
        } else {
            controls::create(&self.activity, node, self.window, tag)?
        };
        controls::insert(&self.content_of(node.parent, window_root), &object.view, -1)?;
        crate::mappers::apply(&object.view, node, crate::mappers::MappedProperty::Text, || Ok(()))?;
        self.registry.insert(node.id, object);
        self.apply_appearance(node)
    }

    /// Replaces a node's view with a new one of the right class, moving its
    /// children's views into the new container.
    fn replace(&mut self, node: &TreeNode, window_root: &JavaRef) -> Result<(), Error> {
        let children: Vec<NodeId> =
            self.snapshot.children_of(node.id).map(|child| child.id).collect();
        if let Some(old) = self.registry.remove(node.id) {
            detach(&old.view);
        }
        self.insert(node, window_root)?;
        let content = self.content_of(Some(node.id), window_root);
        for child in children {
            if let Some(object) = self.registry.get(child) {
                controls::insert(&content, &object.view, -1)?;
            }
        }
        self.placed.retain(|id, _| *id != node.id);
        Ok(())
    }

    fn reparent(
        &mut self,
        id: NodeId,
        parent: Option<NodeId>,
        window_root: &JavaRef,
    ) -> Result<(), Error> {
        let content = self.content_of(parent, window_root);
        if let Some(object) = self.registry.get(id) {
            controls::insert(&content, &object.view, -1)?;
            self.placed.remove(&id);
        }
        Ok(())
    }

    fn remove(&mut self, id: NodeId) {
        if let Some(object) = self.registry.remove(id) {
            detach(&object.view);
        }
        self.layout.remove(&id);
        self.placed.remove(&id);
        self.animated.forget(id);
        self.removed_nodes.push(id);
    }

    /// Puts a container's children in declarative order, which is also the
    /// order accessibility services traverse them in.
    fn reorder(&self, parent: Option<NodeId>, window_root: &JavaRef) -> Result<(), Error> {
        let content = self.content_of(parent, window_root);
        let mut children: Vec<&TreeNode> = match parent {
            Some(parent) => self.snapshot.children_of(parent).collect(),
            None => self.snapshot.nodes().filter(|node| node.parent.is_none()).collect(),
        };
        children.sort_by_key(|node| node.index);
        let mut position = 0;
        for child in children {
            let Some(object) = self.registry.get(child.id) else { continue };
            controls::insert(&content, &object.view, position)?;
            position += 1;
        }
        Ok(())
    }

    /// Realizes a node's style, visibility, enabled state, and opacity.
    pub(crate) fn apply_appearance(&mut self, node: &TreeNode) -> Result<(), Error> {
        let mut spec = self.styles.spec_for(node.kind, &node.style_override, &self.theme);
        let overrides = self.animated.get(node.id);
        if overrides.background.is_some() || overrides.foreground.is_some() {
            spec.animate(overrides.background, overrides.foreground);
        }
        let Some(object) = self.registry.get_mut(node.id) else { return Ok(()) };
        if object.style != Some(spec.digest) && !(object.style.is_none() && spec.is_host_look()) {
            let view = object.view.clone();
            let java_kind = object.java_kind;
            crate::mappers::apply(&view, node, crate::mappers::MappedProperty::Style, || {
                apply_style(&view, java_kind, &spec)
            })?;
            object.style = Some(spec.digest);
        }
        let object =
            self.registry.get(node.id).map(|object| (object.view.clone(), object.java_kind));
        let Some((view, java_kind)) = object else { return Ok(()) };
        crate::mappers::apply(&view, node, crate::mappers::MappedProperty::Visibility, || {
            controls::set_visible(&view, !node.hidden)
        })?;
        controls::set_enabled(&view, !node.disabled)?;
        self.apply_opacity(node.id)?;
        let wants_drop = node.input.wants_drop();
        if let Some(object) = self.registry.get_mut(node.id) {
            if object.drop != wants_drop {
                object.drop = wants_drop;
                call_static(
                    Class::Views,
                    "setDropTarget",
                    "(Landroid/view/View;Z)V",
                    &[Arg::Obj(&view), Arg::Bool(wants_drop)],
                )?;
            }
        }
        if matches!(node.kind, NodeKind::Column | NodeKind::Row)
            && java_kind == crate::protocol::CONTAINER
        {
            controls::set_focusable(&view, node.accessibility.is_focusable() && !node.disabled)?;
        }
        Ok(())
    }

    /// Lays the whole window out at `size` (dp) and places every view.
    pub(crate) fn relayout(&mut self, _window_root: &JavaRef, size: Size) -> Result<(), Error> {
        let _muted = Muted::new();
        let output = self.lay_out(size);
        let lists = !self.virtual_lists.is_empty();
        let physical = output.physical_rects(&self.snapshot, self.direction);
        let directions = LayoutResult::directions(&self.snapshot, self.direction);
        // Layout is where a node's position and size change, so it is where
        // their transitions begin.
        self.collect_geometry_transitions(&physical);
        self.layout = output.rects;
        self.content_sizes = output.content_sizes;
        let density = self.density;
        for node in self.snapshot.nodes() {
            let Some(object) = self.registry.get(node.id) else { continue };
            let rtl = directions.get(&node.id).is_some_and(|direction| direction.is_rtl());
            controls::set_direction(&object.view, rtl)?;
            let Some(rect) = physical.get(&node.id).copied() else { continue };
            let shown = self.animated.rect_for(node.id, rect);
            let (left, top) = (to_px(shown.x, density), to_px(shown.y, density));
            let (right, bottom) = (
                to_px(shown.x.saturating_add(shown.width), density),
                to_px(shown.y.saturating_add(shown.height), density),
            );
            controls::place(&object.view, left, top, (right - left).max(0), (bottom - top).max(0))?;
            self.placed.insert(node.id, rect);
            if let (Some(content), true) =
                (&object.content, object.java_kind == crate::protocol::SCROLL)
            {
                let content_size = self.content_sizes.get(&node.id).copied().unwrap_or_default();
                let width = i32::try_from(content_size.width).unwrap_or(i32::MAX).max(rect.width);
                let height =
                    i32::try_from(content_size.height).unwrap_or(i32::MAX).max(rect.height);
                controls::set_content_size(content, to_px(width, density), to_px(height, density))?;
            }
        }
        if lists {
            self.resolve_anchors()?;
            self.update_visible_ranges();
        }
        Ok(())
    }

    /// Runs layout, and once more if measuring a virtual list's realized
    /// items moved any of its offsets.
    fn lay_out(&mut self, size: Size) -> LayoutResult {
        let measurer = AndroidMeasurer {
            activity: &self.activity,
            density: self.density,
            text_scale: self.text_scale,
            cache: &self.measurements,
        };
        let output = self.engine.layout_result_with(
            &self.snapshot,
            size,
            &measurer,
            self.virtual_lists.extents(),
        );
        if !self.virtual_lists.record(&output.measured_items) {
            return output;
        }
        self.engine.layout_result_with(
            &self.snapshot,
            size,
            &measurer,
            self.virtual_lists.extents(),
        )
    }

    /// Scrolls each virtual list back to its anchored item.
    fn resolve_anchors(&mut self) -> Result<(), Error> {
        let Self { virtual_lists, snapshot, .. } = self;
        let offsets = virtual_lists.resolve_anchors(|node| index_of(snapshot, node));
        for (id, offset) in offsets {
            let Some(object) = self.registry.get(id) else { continue };
            controls::scroll_to(
                &object.view,
                to_px(offset.x, self.density),
                to_px(offset.y, self.density),
            )?;
        }
        Ok(())
    }

    /// Recomputes every virtual list's visible range.
    pub(crate) fn update_visible_ranges(&mut self) {
        let density = self.density;
        let Self { virtual_lists, layout, registry, .. } = self;
        virtual_lists.update_ranges(
            |id| layout.get(&id).copied(),
            |id| scroll_offset(registry, id, density),
        );
    }

    /// Virtual lists that need a different window of items.
    pub(crate) fn take_range_changes(&mut self) -> Vec<(NodeId, VirtualRange)> {
        self.virtual_lists.take_changes()
    }

    /// The view realizing `id`.
    pub(crate) fn view(&self, id: NodeId) -> Option<&JavaRef> {
        self.registry.get(id).map(|object| &object.view)
    }

    /// Releases every view.
    pub(crate) fn release(&mut self) {
        let ids: Vec<NodeId> = self.registry.iter().map(|(id, _)| id).collect();
        for id in ids {
            self.remove(id);
        }
        self.snapshot = TreeSnapshot::default();
    }
}

/// Mutes the views' listeners for as long as it lives.
pub(crate) struct Muted {
    previous: bool,
}

thread_local! {
    static MUTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl Muted {
    pub(crate) fn new() -> Self {
        let previous = MUTED.with(|muted| muted.replace(true));
        if !previous {
            let _ = crate::jni_host::set_static_bool(Class::Bridge, "muted", true);
        }
        Self { previous }
    }
}

impl Drop for Muted {
    fn drop(&mut self) {
        MUTED.with(|muted| muted.set(self.previous));
        if !self.previous {
            let _ = crate::jni_host::set_static_bool(Class::Bridge, "muted", false);
        }
    }
}

fn apply_style(
    view: &JavaRef,
    java_kind: i32,
    spec: &crate::styling::StyleSpec,
) -> Result<(), Error> {
    call_static(
        Class::Style,
        "apply",
        "(Landroid/view/View;I[I[FF[IFILjava/lang/String;)V",
        &[
            Arg::Obj(view),
            Arg::Int(java_kind),
            Arg::Ints(&spec.ints),
            Arg::Floats(&spec.floats),
            Arg::Float(spec.border_width),
            Arg::Null,
            Arg::Float(spec.font_size),
            Arg::Int(spec.weight),
            Arg::OptStr(spec.family.as_deref()),
        ],
    )
    .map(|_| ())
}

/// Whether `node` is inside a virtual list in `snapshot`.
fn is_virtual_item(snapshot: &TreeSnapshot, node: &TreeNode) -> bool {
    let mut parent = node.parent;
    while let Some(id) = parent {
        let Some(ancestor) = snapshot.get(id) else { return false };
        if ancestor.virtualization.is_some() {
            return true;
        }
        parent = ancestor.parent;
    }
    false
}

/// Where the scroll container realizing `id` is scrolled to, in dp.
fn scroll_offset(registry: &NativeRegistry, id: NodeId, density: f32) -> Point {
    let Some(object) =
        registry.get(id).filter(|object| object.java_kind == crate::protocol::SCROLL)
    else {
        return Point::new(0, 0);
    };
    let (x, y) = controls::scroll_offset(&object.view);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "a scroll offset in pixels, rounded"
    )]
    let dp = |pixels: i32| (pixels as f32 / density.max(0.1)).round() as i32;
    Point::new(dp(x), dp(y))
}
