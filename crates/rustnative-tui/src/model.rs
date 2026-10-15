//! The realized model of one window: what the terminal backend "created"
//! for each node (`PLAN.md` §8, "native host objects with stable identity
//! and reuse").
//!
//! A terminal has no host objects, so a node's host object is its entry
//! here — keyed by its [`NodeId`], updated in place while its identity
//! survives a render and destroyed when it does not, exactly as a native
//! backend applies a [`TreeDiff`]. Each entry carries what a native control
//! would keep for itself: the text a person typed (until a render supplies
//! a different value), the caret and selection, a list's cursor, whether a
//! select is open, and a container's scroll position.

use std::collections::HashMap;

use rustnative_core::{
    AccessibilityInfo, AccessibilityTree, Control, DrawList, EdgeInsets, ExtentCache,
    InputInterest, LayoutDirection, LayoutEngine, Node, NodeId, NodeKind, Overflow, Point, Rect,
    Size, StyleOverride, Tabs, Theme, TreeDiff, TreeOp, TreeSnapshot, VirtualListStyle,
};
use rustnative_drawlist::{FocusModel, GridMapping, GridRect};

use crate::edit::Edit;
use crate::measure::TuiMeasurer;

/// One realized node.
#[derive(Debug, Clone, PartialEq)]
pub struct RealizedNode {
    /// Its identity.
    pub id: NodeId,
    /// The key its author wrote.
    pub key: Option<String>,
    /// Its kind.
    pub kind: NodeKind,
    /// Its parent (`None` for the root).
    pub parent: Option<NodeId>,
    /// Its children, in order.
    pub children: Vec<NodeId>,
    /// The text it shows; for a text field or multi-line text, what the
    /// person typed.
    pub text: Option<String>,
    /// Its rectangle in its parent's coordinates, in logical pixels.
    pub rect: Rect,
    /// Its rectangle in window coordinates after every ancestor's scroll.
    pub window_rect: Rect,
    /// The part of `window_rect` visible through every clipping ancestor.
    pub visible_rect: Rect,
    /// `window_rect` in cells.
    pub cells: GridRect,
    /// `visible_rect` in cells: where it may draw.
    pub visible_cells: GridRect,
    /// Its accessibility metadata.
    pub accessibility: AccessibilityInfo,
    /// Its accessible name.
    pub name: Option<String>,
    /// Whether it is disabled.
    pub disabled: bool,
    /// Whether it or an ancestor is hidden.
    pub hidden: bool,
    /// Its style override (resolved against the theme when painted, per
    /// interaction state).
    pub style_override: StyleOverride,
    /// Its opacity.
    pub opacity: f32,
    /// A tab bar's tabs.
    pub tabs: Option<Tabs>,
    /// A control and its state.
    pub control: Option<Control>,
    /// A canvas's drawing.
    pub draw_list: Option<DrawList>,
    /// What input it wants.
    pub input: InputInterest,
    /// A container's overflow.
    pub overflow: Option<Overflow>,
    /// A container's padding.
    pub padding: Option<EdgeInsets>,
    /// Its scroll offset.
    pub scroll: Point,
    /// How far it scrolls.
    pub scroll_range: Size,
    /// A virtual list's description.
    pub virtualization: Option<VirtualListStyle>,
    /// Which virtual-list item it is.
    pub item_index: Option<usize>,
    /// The properties it transitions when they change.
    pub transitions: Vec<rustnative_core::NodeTransition>,
    /// Its layout direction: its own, or its parent's, or the window's.
    pub direction: LayoutDirection,
}

impl RealizedNode {
    /// Whether a person can interact with it.
    #[must_use]
    pub fn is_interactable(&self) -> bool {
        !self.hidden && !self.disabled && !self.visible_cells.is_empty() && self.opacity > 0.0
    }

    /// Whether typing goes into it.
    #[must_use]
    pub fn is_text_field(&self) -> bool {
        self.kind == NodeKind::TextInput
            || matches!(self.control, Some(Control::MultilineText { .. }))
    }

    /// Whether it scrolls.
    #[must_use]
    pub fn scrolls(&self) -> bool {
        self.scroll_range != Size::new(0, 0) || self.virtualization.is_some()
    }
}

/// What a node's widget remembers between renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WidgetState {
    /// A text field's caret, selection, and scroll.
    pub edit: Edit,
    /// A list box's (or an open select's) highlighted item.
    pub cursor: Option<usize>,
    /// A list box's first visible item.
    pub first: usize,
    /// Whether a select's list is open.
    pub open: bool,
    /// A date picker's segment: 0 year, 1 month, 2 day.
    pub segment: u8,
}

/// Creation and destruction counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RealizationStats {
    /// Created over the window's lifetime.
    pub created: u64,
    /// Destroyed over the window's lifetime.
    pub destroyed: u64,
    /// Realizations that changed something.
    pub updates: u64,
}

impl RealizationStats {
    /// Alive now.
    #[must_use]
    pub const fn live(&self) -> u64 {
        self.created.saturating_sub(self.destroyed)
    }
}

/// The realized model of one window.
#[derive(Debug, Clone, Default)]
pub struct TuiModel {
    nodes: HashMap<NodeId, RealizedNode>,
    order: Vec<NodeId>,
    snapshot: TreeSnapshot,
    accessibility: AccessibilityTree,
    size: Size,
    stats: RealizationStats,
    native_values: HashMap<NodeId, String>,
    rendered_values: HashMap<NodeId, String>,
    widgets: HashMap<NodeId, WidgetState>,
    scroll: HashMap<NodeId, Point>,
    rendered_selections: HashMap<NodeId, Option<usize>>,
    focused: Option<NodeId>,
    hovered: Option<NodeId>,
    pressed: Option<NodeId>,
    direction: LayoutDirection,
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

fn clamp_scroll(offset: Point, range: Size) -> Point {
    let max_x = i32::try_from(range.width).unwrap_or(i32::MAX);
    let max_y = i32::try_from(range.height).unwrap_or(i32::MAX);
    Point::new(offset.x.clamp(0, max_x), offset.y.clamp(0, max_y))
}

/// The value a node's widget edits, as rendered.
fn rendered_value(
    kind: NodeKind,
    text: Option<&String>,
    control: Option<&Control>,
) -> Option<String> {
    match (kind, control) {
        (NodeKind::TextInput, _) => Some(text.cloned().unwrap_or_default()),
        (_, Some(Control::MultilineText { value })) => Some(value.clone()),
        _ => None,
    }
}

impl TuiModel {
    /// Realizes `view` in a window of `size` logical pixels.
    #[allow(
        clippy::too_many_arguments,
        reason = "the realization inputs, each from a different source"
    )]
    pub fn realize(
        &mut self,
        view: &Node,
        theme: &Theme,
        size: Size,
        measurer: &TuiMeasurer,
        mapping: GridMapping,
        direction: LayoutDirection,
        safe_area: EdgeInsets,
    ) -> bool {
        let Ok(mut snapshot) = TreeSnapshot::from_node_with_theme(view, theme) else {
            return false;
        };
        // Lengths become whole cells at the boundary (`PLAN.md` 2.11).
        mapping.quantize(&mut snapshot);
        let diff = TreeDiff::between(&self.snapshot, &snapshot);
        let mut changed = self.size != size || self.direction != direction;
        for op in diff.operations() {
            match op {
                TreeOp::Insert(_) => self.stats.created += 1,
                TreeOp::Remove(node) => {
                    self.stats.destroyed += 1;
                    self.native_values.remove(&node.id);
                    self.rendered_values.remove(&node.id);
                    self.widgets.remove(&node.id);
                    self.rendered_selections.remove(&node.id);
                    self.scroll.remove(&node.id);
                }
                _ => {}
            }
            changed = true;
        }
        if changed {
            self.stats.updates += 1;
        }
        // The controlled-input contract: what the person typed stays until
        // a render supplies a different value.
        for node in snapshot.nodes() {
            if let Some(rendered) =
                rendered_value(node.kind, node.text.as_ref(), node.control.as_ref())
            {
                if self.rendered_values.get(&node.id) != Some(&rendered) {
                    let widget = self.widgets.entry(node.id).or_default();
                    if self.rendered_values.contains_key(&node.id) {
                        widget.edit.clamp(&rendered);
                    } else {
                        widget.edit = Edit::at_end(&rendered);
                    }
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
        let inner = Size::new(
            size.width.saturating_sub(u32::try_from(safe_area.horizontal().max(0)).unwrap_or(0)),
            size.height.saturating_sub(u32::try_from(safe_area.vertical().max(0)).unwrap_or(0)),
        );
        let mut layout =
            LayoutEngine::new().layout_result_with(&snapshot, inner, measurer, &extents);
        if let Some(root) = snapshot.ordered_nodes().first().map(|node| node.id) {
            if let Some(rect) = layout.rects.get_mut(&root) {
                rect.x += safe_area.left(direction);
                rect.y += safe_area.top;
            }
        }
        let accessibility = AccessibilityTree::from_snapshot(&snapshot);
        // The terminal does not mirror; this backend places the mirrored
        // rectangles itself.
        let physical = layout.physical_rects(&snapshot, direction);
        if self
            .focused
            .is_some_and(|id| !snapshot.contains(id) || snapshot.is_effectively_hidden(id))
        {
            self.focused = None;
        }
        for slot in [&mut self.hovered, &mut self.pressed] {
            if slot.is_some_and(|id| !snapshot.contains(id)) {
                *slot = None;
            }
        }
        let mut nodes: HashMap<NodeId, RealizedNode> = HashMap::new();
        let mut order = Vec::new();
        for node in snapshot.ordered_nodes() {
            let rect = physical.get(&node.id).copied().unwrap_or_default();
            let (origin, parent_visible) = node
                .parent
                .and_then(|parent| nodes.get(&parent))
                .map_or((Point::new(0, 0), Rect::new(0, 0, i32::MAX, i32::MAX)), |parent| {
                    (
                        Point::new(
                            parent.window_rect.x - parent.scroll.x,
                            parent.window_rect.y - parent.scroll.y,
                        ),
                        if parent.overflow == Some(Overflow::Visible) {
                            Rect::new(i32::MIN / 2, i32::MIN / 2, i32::MAX, i32::MAX)
                        } else {
                            parent.visible_rect
                        },
                    )
                });
            let window_rect =
                Rect::new(origin.x + rect.x, origin.y + rect.y, rect.width, rect.height);
            let visible_rect = intersect(window_rect, parent_visible);
            let overflow = node
                .column_style
                .map(|style| style.overflow)
                .or(node.row_style.map(|style| style.overflow));
            let padding = node
                .column_style
                .map(|style| style.padding)
                .or(node.row_style.map(|style| style.padding));
            let scroll_range =
                layout.scroll_ranges.get(&node.id).copied().unwrap_or(Size::new(0, 0));
            let scroll = self
                .scroll
                .get(&node.id)
                .copied()
                .map_or(Point::new(0, 0), |offset| clamp_scroll(offset, scroll_range));
            let text =
                if rendered_value(node.kind, node.text.as_ref(), node.control.as_ref()).is_some() {
                    self.native_values.get(&node.id).cloned()
                } else {
                    node.text.clone()
                };
            let cells = mapping.rect(window_rect);
            let node_direction = node.layout.direction.unwrap_or_else(|| {
                node.parent
                    .and_then(|parent| nodes.get(&parent))
                    .map_or(direction, |parent| parent.direction)
            });
            let visible_cells = if visible_rect.width > 0 && visible_rect.height > 0 {
                mapping.rect(visible_rect).intersect(&cells)
            } else {
                GridRect::default()
            };
            order.push(node.id);
            nodes.insert(
                node.id,
                RealizedNode {
                    id: node.id,
                    key: node.id.local_key(),
                    kind: node.kind,
                    parent: node.parent,
                    children: snapshot.children_of(node.id).map(|child| child.id).collect(),
                    text,
                    rect,
                    window_rect,
                    visible_rect,
                    cells,
                    visible_cells,
                    accessibility: node.accessibility.clone(),
                    name: accessibility.name_of(node.id),
                    disabled: node.disabled,
                    hidden: snapshot.is_effectively_hidden(node.id),
                    style_override: node.style_override.clone(),
                    opacity: node.opacity.get(),
                    tabs: node.tabs.clone(),
                    control: node.control.clone(),
                    draw_list: node.draw_list.clone(),
                    input: node.input,
                    overflow,
                    padding,
                    scroll,
                    scroll_range,
                    virtualization: node.virtualization,
                    item_index: node.item_index,
                    transitions: node.transitions.clone(),
                    direction: node_direction,
                },
            );
        }
        // A list box's cursor follows its selection whenever a render changes
        // the selection (the application chose an item), and where it has
        // none or points past the items.
        for node in nodes.values() {
            if let Some(Control::ListBox { selected, items }) = &node.control {
                let changed =
                    self.rendered_selections.insert(node.id, *selected) != Some(*selected);
                let widget = self.widgets.entry(node.id).or_default();
                if changed || widget.cursor.is_none_or(|cursor| cursor >= items.len()) {
                    widget.cursor = selected.filter(|index| *index < items.len());
                }
            }
        }
        self.nodes = nodes;
        self.order = order;
        self.snapshot = snapshot;
        self.accessibility = accessibility;
        self.size = size;
        self.direction = direction;
        changed
    }

    /// The node `id`.
    #[must_use]
    pub fn get(&self, id: NodeId) -> Option<&RealizedNode> {
        self.nodes.get(&id)
    }

    /// Every node, in tree (painting) order.
    pub fn nodes(&self) -> impl Iterator<Item = &RealizedNode> {
        self.order.iter().filter_map(|id| self.nodes.get(id))
    }

    /// The snapshot last realized.
    #[must_use]
    pub const fn snapshot(&self) -> &TreeSnapshot {
        &self.snapshot
    }

    /// The accessibility tree.
    #[must_use]
    pub const fn accessibility(&self) -> &AccessibilityTree {
        &self.accessibility
    }

    /// Creation and destruction counts.
    #[must_use]
    pub const fn stats(&self) -> RealizationStats {
        self.stats
    }

    /// The window's logical size.
    #[must_use]
    pub const fn size(&self) -> Size {
        self.size
    }

    /// The layout direction realized.
    #[must_use]
    pub const fn direction(&self) -> LayoutDirection {
        self.direction
    }

    /// The focused node.
    #[must_use]
    pub const fn focused(&self) -> Option<NodeId> {
        self.focused
    }

    /// The hovered node.
    #[must_use]
    pub const fn hovered(&self) -> Option<NodeId> {
        self.hovered
    }

    /// The pressed node.
    #[must_use]
    pub const fn pressed(&self) -> Option<NodeId> {
        self.pressed
    }

    pub(crate) fn set_focused(&mut self, id: Option<NodeId>) {
        if self.focused != id {
            if let Some(previous) = self.focused {
                if let Some(widget) = self.widgets.get_mut(&previous) {
                    widget.open = false;
                }
            }
        }
        self.focused = id;
    }

    pub(crate) fn set_hovered(&mut self, id: Option<NodeId>) {
        self.hovered = id;
    }

    pub(crate) fn set_pressed(&mut self, id: Option<NodeId>) {
        self.pressed = id;
    }

    /// A node's widget state.
    #[must_use]
    pub fn widget(&self, id: NodeId) -> WidgetState {
        self.widgets.get(&id).copied().unwrap_or_default()
    }

    pub(crate) fn widget_mut(&mut self, id: NodeId) -> &mut WidgetState {
        self.widgets.entry(id).or_default()
    }

    /// The text a field holds now.
    #[must_use]
    pub fn value(&self, id: NodeId) -> String {
        self.native_values.get(&id).cloned().unwrap_or_default()
    }

    pub(crate) fn set_value(&mut self, id: NodeId, value: String) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.text = Some(value.clone());
        }
        self.native_values.insert(id, value);
    }

    /// The traversal order.
    #[must_use]
    pub fn focus_model(&self) -> FocusModel {
        FocusModel::from_order(
            self.nodes()
                .filter(|node| {
                    node.accessibility.is_focusable()
                        && !node.disabled
                        && !node.hidden
                        && node.opacity > 0.0
                })
                .map(|node| node.id)
                .collect(),
        )
    }

    /// The deepest shown node whose visible cells contain (`x`, `y`).
    #[must_use]
    pub fn hit_test(&self, x: i32, y: i32) -> Option<NodeId> {
        self.nodes()
            .filter(|node| !node.hidden && node.opacity > 0.0 && node.visible_cells.contains(x, y))
            .last()
            .map(|node| node.id)
    }

    /// `id` and its ancestors, nearest first.
    #[must_use]
    pub fn ancestry(&self, id: NodeId) -> Vec<NodeId> {
        let mut chain = vec![id];
        let mut at = id;
        while let Some(parent) = self.nodes.get(&at).and_then(|node| node.parent) {
            chain.push(parent);
            at = parent;
        }
        chain
    }

    /// The nearest node from `id` up that scrolls (vertically when
    /// `vertical`), and can still move in direction `sign`.
    #[must_use]
    pub fn scroller(&self, id: NodeId, vertical: bool, sign: i32) -> Option<NodeId> {
        self.ancestry(id).into_iter().find(|candidate| {
            self.nodes.get(candidate).is_some_and(|node| {
                let (offset, range) = if vertical {
                    (node.scroll.y, i32::try_from(node.scroll_range.height).unwrap_or(i32::MAX))
                } else {
                    (node.scroll.x, i32::try_from(node.scroll_range.width).unwrap_or(i32::MAX))
                };
                range > 0 && ((sign > 0 && offset < range) || (sign < 0 && offset > 0))
            })
        })
    }

    /// Scrolls `id` to `offset`, clamped, with no render (`PLAN.md` 2.10).
    /// Returns whether it moved. The next realization places its content.
    pub fn scroll_to(&mut self, id: NodeId, offset: Point) -> bool {
        let range = self.nodes.get(&id).map_or(Size::new(0, 0), |node| node.scroll_range);
        let clamped = clamp_scroll(offset, range);
        let previous = self.scroll.insert(id, clamped).unwrap_or(Point::new(0, 0));
        if let Some(node) = self.nodes.get_mut(&id) {
            node.scroll = clamped;
        }
        previous != clamped
    }

    /// Scrolls every ancestor of `id` so its rectangle is visible. Returns
    /// whether anything moved.
    pub fn reveal(&mut self, id: NodeId) -> bool {
        let mut moved = false;
        let Some(target) = self.nodes.get(&id).map(|node| node.window_rect) else { return false };
        for ancestor in self.ancestry(id).into_iter().skip(1) {
            let Some(node) = self.nodes.get(&ancestor).cloned() else { continue };
            if !node.scrolls() || node.scroll_range == Size::new(0, 0) {
                continue;
            }
            let viewport = node.window_rect;
            let mut offset = node.scroll;
            if target.y < viewport.y {
                offset.y -= viewport.y - target.y;
            } else if target.y + target.height > viewport.y + viewport.height {
                offset.y += (target.y + target.height) - (viewport.y + viewport.height);
            }
            if target.x < viewport.x {
                offset.x -= viewport.x - target.x;
            } else if target.x + target.width > viewport.x + viewport.width {
                offset.x += (target.x + target.width) - (viewport.x + viewport.width);
            }
            moved |= self.scroll_to(ancestor, offset);
        }
        moved
    }

    /// A stable description of the model, for goldens and inspection.
    #[must_use]
    pub fn describe(&self) -> String {
        use std::fmt::Write as _;
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
            let cells = node.cells;
            let _ = write!(out, " cells[{},{} {}x{}]", cells.x, cells.y, cells.width, cells.height);
            if self.focused == Some(node.id) {
                out.push_str(" focused");
            }
            if node.disabled {
                out.push_str(" disabled");
            }
            if node.hidden {
                out.push_str(" hidden");
            }
            out.push('\n');
        }
        out
    }
}
