//! The realized model: what the headless backend "created" for a tree.

use std::collections::HashMap;
use std::fmt::Write as _;

use rustnative_core::{
    AccessibilityInfo, AccessibilityRole, AccessibilityTree, ExtentCache, LayoutEngine, Node,
    NodeId, NodeKind, Overflow, Point, Rect, ResolvedStyle, Size, Tabs, Theme, TreeDiff, TreeOp,
    TreeSnapshot, VirtualRange,
};

use crate::measure::HeadlessMeasurer;

/// One realized node: the headless counterpart of a native control.
#[derive(Debug, Clone, PartialEq)]
pub struct RealizedNode {
    /// The node's identity.
    pub id: NodeId,
    /// The key its author wrote, for diagnostics.
    pub key: Option<String>,
    /// What kind of control it is.
    pub kind: NodeKind,
    /// Its parent, `None` for a window root.
    pub parent: Option<NodeId>,
    /// Its children, in order.
    pub children: Vec<NodeId>,
    /// The text it shows. For a text field this is the *native* value —
    /// what the person typed — which, as on a real host, persists until a
    /// render supplies a different value.
    pub text: Option<String>,
    /// Its rectangle in its parent's coordinate space (what layout
    /// produced, and what a native backend would hand its host).
    pub rect: Rect,
    /// Its rectangle in window coordinates, after every ancestor's scroll
    /// offset — what hit-testing uses.
    pub window_rect: Rect,
    /// The portion of [`Self::window_rect`] actually visible through every
    /// clipping ancestor; empty when scrolled or clipped out of view.
    pub visible_rect: Rect,
    /// Its accessibility metadata.
    pub accessibility: AccessibilityInfo,
    /// Its accessible name, as assistive technology would announce it.
    pub name: Option<String>,
    /// Whether it is disabled.
    pub disabled: bool,
    /// Whether it (or an ancestor) is hidden.
    pub hidden: bool,
    /// Whether it has keyboard focus.
    pub focused: bool,
    /// Its theme-resolved style.
    pub style: ResolvedStyle,
    /// Its opacity.
    pub opacity: f32,
    /// Its tabs, for a tab bar.
    pub tabs: Option<Tabs>,
    /// Its control and state, for a native control (Milestone 48).
    pub control: Option<rustnative_core::Control>,
    /// Its scroll offset, for a scrolling container.
    pub scroll: Point,
    /// How far it can scroll on each axis, for a scrolling container.
    pub scroll_range: Size,
    /// Which item of its virtual list it realizes.
    pub item_index: Option<usize>,
}

impl RealizedNode {
    /// Whether a person could interact with it: shown, enabled, and at
    /// least partly on screen.
    #[must_use]
    pub fn is_interactable(&self) -> bool {
        !self.hidden
            && !self.disabled
            && self.visible_rect.width > 0
            && self.visible_rect.height > 0
    }

    /// The centre of its visible area, in window coordinates.
    #[must_use]
    pub fn center(&self) -> Point {
        Point::new(
            self.visible_rect.x + self.visible_rect.width / 2,
            self.visible_rect.y + self.visible_rect.height / 2,
        )
    }
}

/// Counts of realized objects created and destroyed, the headless answer to
/// "did this change create new native objects or reuse the old ones?".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RealizationStats {
    /// Objects created over this window's lifetime.
    pub created: u64,
    /// Objects destroyed over this window's lifetime.
    pub destroyed: u64,
    /// Realization passes that changed at least one object.
    pub updates: u64,
}

impl RealizationStats {
    /// Objects currently alive.
    #[must_use]
    pub const fn live(&self) -> u64 {
        self.created.saturating_sub(self.destroyed)
    }
}

/// The realized model of one window.
#[derive(Debug, Clone, Default)]
pub struct HeadlessTree {
    nodes: HashMap<NodeId, RealizedNode>,
    order: Vec<NodeId>,
    snapshot: TreeSnapshot,
    accessibility: AccessibilityTree,
    size: Size,
    stats: RealizationStats,
    native_values: HashMap<NodeId, String>,
    rendered_values: HashMap<NodeId, String>,
    scroll: HashMap<NodeId, Point>,
    focused: Option<NodeId>,
}

impl HeadlessTree {
    /// Realizes `view` for a window of `size`, reconciling against what was
    /// realized before: objects whose identity survives are updated in
    /// place, the rest are created or destroyed, exactly as a native
    /// backend applies a [`TreeDiff`].
    pub(crate) fn realize(
        &mut self,
        view: &Node,
        theme: &Theme,
        size: Size,
        measurer: HeadlessMeasurer,
        direction: rustnative_core::LayoutDirection,
        safe_area: rustnative_core::EdgeInsets,
    ) {
        let Ok(snapshot) = TreeSnapshot::from_node_with_theme(view, theme) else {
            // A tree with a duplicate id never reaches a backend; the
            // component tree has already reported it as a render error.
            return;
        };
        let diff = TreeDiff::between(&self.snapshot, &snapshot);
        let mut changed = false;
        for op in diff.operations() {
            match op {
                TreeOp::Insert(_) => {
                    self.stats.created += 1;
                    changed = true;
                }
                TreeOp::Remove(node) => {
                    self.stats.destroyed += 1;
                    self.native_values.remove(&node.id);
                    self.rendered_values.remove(&node.id);
                    self.scroll.remove(&node.id);
                    changed = true;
                }
                _ => changed = true,
            }
        }
        if changed {
            self.stats.updates += 1;
        }

        // A text field keeps what the person typed until a render supplies
        // a *different* value — the controlled-input contract every native
        // backend implements.
        for node in snapshot.nodes() {
            if node.kind == NodeKind::TextInput {
                let rendered = node.text.clone().unwrap_or_default();
                if self.rendered_values.get(&node.id) != Some(&rendered) {
                    self.native_values.insert(node.id, rendered.clone());
                    self.rendered_values.insert(node.id, rendered);
                }
            }
        }

        let extents: HashMap<NodeId, ExtentCache> = snapshot
            .nodes()
            .filter_map(|node| {
                node.virtualization
                    .map(|style| (node.id, ExtentCache::new(style.item_count, style.extent)))
            })
            .collect();
        // Content stays clear of the safe area (`keys::SAFE_AREA`): the
        // root is laid out in what remains, and placed inside the insets.
        let inner = Size::new(
            size.width.saturating_sub(u32::try_from(safe_area.horizontal().max(0)).unwrap_or(0)),
            size.height.saturating_sub(u32::try_from(safe_area.vertical().max(0)).unwrap_or(0)),
        );
        let mut layout =
            LayoutEngine::new().layout_result_with(&snapshot, inner, &measurer, &extents);
        if let Some(root) = snapshot.ordered_nodes().first().map(|node| node.id) {
            if let Some(rect) = layout.rects.get_mut(&root) {
                rect.x += safe_area.left(direction);
                rect.y += safe_area.top;
            }
        }
        let accessibility = AccessibilityTree::from_snapshot(&snapshot);
        // This backend has no host mirroring of its own, so it places the
        // physical (mirrored) rectangles — see `LayoutResult::physical_rects`.
        let physical = layout.physical_rects(&snapshot, direction);

        if self
            .focused
            .is_some_and(|id| !snapshot.contains(id) || snapshot.is_effectively_hidden(id))
        {
            self.focused = None;
        }

        let mut nodes = HashMap::new();
        let mut order = Vec::new();
        for node in snapshot.ordered_nodes() {
            let rect = physical.get(&node.id).copied().unwrap_or_default();
            let (parent_origin, parent_visible) =
                node.parent.and_then(|parent| nodes.get(&parent)).map_or(
                    (Point::new(0, 0), Rect::new(0, 0, i32::MAX, i32::MAX)),
                    |parent: &RealizedNode| {
                        let scroll = parent.scroll;
                        (
                            Point::new(
                                parent.window_rect.x - scroll.x,
                                parent.window_rect.y - scroll.y,
                            ),
                            parent.visible_rect,
                        )
                    },
                );
            let window_rect = Rect::new(
                parent_origin.x + rect.x,
                parent_origin.y + rect.y,
                rect.width,
                rect.height,
            );
            let visible_rect = intersect(window_rect, parent_visible);
            let visible_rect = match node
                .column_style
                .map(|style| style.overflow)
                .or(node.row_style.map(|style| style.overflow))
            {
                Some(Overflow::Visible) => window_rect,
                _ => visible_rect,
            };
            let scroll_range =
                layout.scroll_ranges.get(&node.id).copied().unwrap_or(Size::new(0, 0));
            let scroll = self
                .scroll
                .get(&node.id)
                .copied()
                .map_or(Point::new(0, 0), |offset| clamp_scroll(offset, scroll_range));
            let text = if node.kind == NodeKind::TextInput {
                self.native_values.get(&node.id).cloned()
            } else {
                node.text.clone()
            };
            let realized = RealizedNode {
                id: node.id,
                key: node.id.local_key(),
                kind: node.kind,
                parent: node.parent,
                children: snapshot.children_of(node.id).map(|child| child.id).collect(),
                text,
                rect,
                window_rect,
                visible_rect,
                accessibility: node.accessibility.clone(),
                name: accessibility.name_of(node.id),
                disabled: node.disabled,
                hidden: snapshot.is_effectively_hidden(node.id),
                focused: self.focused == Some(node.id),
                style: node.visual_style.clone(),
                opacity: node.opacity.get(),
                tabs: node.tabs.clone(),
                control: node.control.clone(),
                scroll,
                scroll_range,
                item_index: node.item_index,
            };
            order.push(node.id);
            nodes.insert(node.id, realized);
        }
        self.nodes = nodes;
        self.order = order;
        self.snapshot = snapshot;
        self.accessibility = accessibility;
        self.size = size;
    }

    /// The realized node for `id`.
    #[must_use]
    pub fn get(&self, id: NodeId) -> Option<&RealizedNode> {
        self.nodes.get(&id)
    }

    /// Every realized node, in tree (preorder) order.
    pub fn nodes(&self) -> impl Iterator<Item = &RealizedNode> {
        self.order.iter().filter_map(|id| self.nodes.get(id))
    }

    /// The accessibility tree assistive technology would see.
    #[must_use]
    pub fn accessibility(&self) -> &AccessibilityTree {
        &self.accessibility
    }

    /// The snapshot most recently realized.
    #[must_use]
    pub fn snapshot(&self) -> &TreeSnapshot {
        &self.snapshot
    }

    /// Creation and destruction counts.
    #[must_use]
    pub const fn stats(&self) -> RealizationStats {
        self.stats
    }

    /// The window's client size.
    #[must_use]
    pub const fn size(&self) -> Size {
        self.size
    }

    /// The focused node, if any.
    #[must_use]
    pub const fn focused(&self) -> Option<NodeId> {
        self.focused
    }

    pub(crate) fn set_focused(&mut self, id: Option<NodeId>) {
        if let Some(previous) = self.focused.and_then(|id| self.nodes.get_mut(&id)) {
            previous.focused = false;
        }
        self.focused = id;
        if let Some(next) = id.and_then(|id| self.nodes.get_mut(&id)) {
            next.focused = true;
        }
    }

    pub(crate) fn set_native_value(&mut self, id: NodeId, value: String) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.text = Some(value.clone());
        }
        self.native_values.insert(id, value);
    }

    /// Scrolls `id` to `offset` (clamped to its range) without a render —
    /// a native viewport change, as `PLAN.md` 2.10 requires. Returns the
    /// offset actually applied.
    pub(crate) fn scroll_to(&mut self, id: NodeId, offset: Point) -> Point {
        let range = self.nodes.get(&id).map_or(Size::new(0, 0), |node| node.scroll_range);
        let clamped = clamp_scroll(offset, range);
        self.scroll.insert(id, clamped);
        if let Some(node) = self.nodes.get_mut(&id) {
            node.scroll = clamped;
        }
        clamped
    }

    /// The keyboard traversal order: focusable, enabled, shown nodes in tree
    /// order — the order Tab visits on every host this framework targets.
    #[must_use]
    pub fn tab_order(&self) -> Vec<NodeId> {
        self.nodes()
            .filter(|node| node.accessibility.is_focusable() && !node.disabled && !node.hidden)
            .map(|node| node.id)
            .collect()
    }

    /// The deepest interactable node under `point`, in window coordinates.
    #[must_use]
    pub fn hit_test(&self, point: Point) -> Option<NodeId> {
        self.nodes()
            .filter(|node| !node.hidden && contains(node.visible_rect, point))
            .last()
            .map(|node| node.id)
    }

    /// The range of item indices a virtual list currently realizes.
    #[must_use]
    pub fn realized_range(&self, list: NodeId) -> VirtualRange {
        let indices: Vec<usize> = self
            .get(list)
            .map(|node| {
                node.children.iter().filter_map(|child| self.get(*child)?.item_index).collect()
            })
            .unwrap_or_default();
        match (indices.iter().min(), indices.iter().max()) {
            (Some(&first), Some(&final_index)) => {
                VirtualRange { first, last_exclusive: final_index + 1 }
            }
            _ => VirtualRange::EMPTY,
        }
    }

    /// A stable, human-readable description of the realized tree — the
    /// content of a golden file.
    #[must_use]
    pub fn describe(&self) -> String {
        let mut out = String::new();
        let mut depth: HashMap<NodeId, usize> = HashMap::new();
        for node in self.nodes() {
            let level =
                node.parent.and_then(|parent| depth.get(&parent)).map_or(0, |level| level + 1);
            depth.insert(node.id, level);
            let _ = write!(out, "{:indent$}{:?}", "", node.kind, indent = level * 2);
            if let Some(key) = &node.key {
                let _ = write!(out, " #{key}");
            }
            if let Some(text) = &node.text {
                let _ = write!(out, " {text:?}");
            }
            let rect = node.rect;
            let _ = write!(out, " [{},{} {}x{}]", rect.x, rect.y, rect.width, rect.height);
            let role = node.accessibility.role();
            if role != default_role(node.kind) {
                let _ = write!(out, " role={role:?}");
            }
            if let Some(name) = &node.name {
                if node.text.as_ref() != Some(name) {
                    let _ = write!(out, " name={name:?}");
                }
            }
            if let Some(tabs) = &node.tabs {
                let _ = write!(out, " tabs={:?} selected={}", tabs.labels(), tabs.selected());
            }
            if let Some(control) = &node.control {
                let _ = write!(out, " control={}", control_summary(control));
            }
            for (flag, set) in
                [("disabled", node.disabled), ("hidden", node.hidden), ("focused", node.focused)]
            {
                if set {
                    let _ = write!(out, " {flag}");
                }
            }
            if node.scroll != Point::new(0, 0) {
                let _ = write!(out, " scroll={},{}", node.scroll.x, node.scroll.y);
            }
            out.push('\n');
        }
        out
    }
}

/// A control's state in a golden: its kind and what it shows, not its
/// pixels.
fn control_summary(control: &rustnative_core::Control) -> String {
    use rustnative_core::Control;
    match control {
        Control::Image { image } => format!("Image {}x{}", image.width(), image.height()),
        other => format!("{other:?}"),
    }
}

fn default_role(kind: NodeKind) -> AccessibilityRole {
    match kind {
        NodeKind::Label => AccessibilityRole::Label,
        NodeKind::Button => AccessibilityRole::Button,
        NodeKind::TextInput => AccessibilityRole::TextInput,
        NodeKind::Canvas => AccessibilityRole::Canvas,
        NodeKind::TabBar => AccessibilityRole::TabList,
        _ => AccessibilityRole::Group,
    }
}

fn clamp_scroll(offset: Point, range: Size) -> Point {
    let max_x = i32::try_from(range.width).unwrap_or(i32::MAX);
    let max_y = i32::try_from(range.height).unwrap_or(i32::MAX);
    Point::new(offset.x.clamp(0, max_x), offset.y.clamp(0, max_y))
}

fn intersect(a: Rect, b: Rect) -> Rect {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = a.x.saturating_add(a.width).min(b.x.saturating_add(b.width));
    let bottom = a.y.saturating_add(a.height).min(b.y.saturating_add(b.height));
    if right <= left || bottom <= top {
        Rect::new(left, top, 0, 0)
    } else {
        Rect::new(left, top, right - left, bottom - top)
    }
}

fn contains(rect: Rect, point: Point) -> bool {
    point.x >= rect.x
        && point.y >= rect.y
        && point.x < rect.x.saturating_add(rect.width)
        && point.y < rect.y.saturating_add(rect.height)
}
