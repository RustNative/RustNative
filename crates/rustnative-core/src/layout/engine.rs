//! The platform-independent layout pass.
//!
//! Layout runs as a separate phase after reconciliation (see
//! `crate::reconcile`), consuming a [`crate::reconcile::TreeSnapshot`] and
//! producing absolute-within-parent rectangles, clip regions, content sizes,
//! and scroll ranges. A platform backend applies the result to native
//! objects; nothing in this module talks to a platform.
//!
//! # Overflow safety
//!
//! Every arithmetic operation in this module is saturating rather than
//! wrapping or panicking. An application can put arbitrarily large values
//! into `LayoutStyle`/`Constraints` (deliberately or through a bug), and a
//! production layout engine must degrade to a clamped, still-rectangular
//! geometry rather than let a single pathological node crash the whole
//! layout pass — see the standards audit's P1.19 finding, "integer-overflow
//! hazards".
//!
//! # A note on this module's `i32`/`u32` casts
//!
//! This module works in `i32` internally (negative intermediate values —
//! e.g. "how much space is left after subtracting margins" before it's
//! floored at zero — are a normal, meaningful part of the arithmetic) but
//! exchanges `u32` at its [`crate::layout::geometry::Size`] boundary. Every
//! `as u32`/`as i32` cast in this file is preceded by an explicit
//! `.max(0)`/`.min(i32::MAX as _)` that establishes the value is in range
//! for the target type — an invariant this module's own tests
//! (`extreme_constraints_do_not_panic_or_produce_negative_geometry`) check
//! directly, but that `clippy::cast_sign_loss`/`cast_possible_wrap`/
//! `cast_possible_truncation` cannot see through a `.max(0)` call to prove
//! statically. Allowed at the module level rather than per cast site, since
//! re-litigating the same already-tested invariant at every one of this
//! module's several dozen casts would be pure noise, not added safety.
#![allow(clippy::cast_sign_loss, clippy::cast_possible_wrap, clippy::cast_possible_truncation)]

use std::collections::HashMap;

use super::constraints::ResolvedContainerStyle;
use super::geometry::{Alignment, EdgeInsets, Overflow, Rect, Size, SizeMode};
use super::measure::{DefaultIntrinsicMeasurer, IntrinsicMeasurer};
use crate::identity::NodeId;
use crate::node::NodeKind;
use crate::reconcile::{TreeNode, TreeSnapshot};
use crate::virtualization::{Axis, ExtentCache, ItemExtent, VirtualListStyle};

/// Whether the most recent tree diff requires a full relayout.
///
/// Currently a coarse on/off flag; see [`crate::reconcile::TreeDiff`] for the
/// change-classification (layout-affecting vs. paint-only vs.
/// accessibility-only) that decides when `full` is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LayoutInvalidation {
    /// Whether every node's geometry must be recomputed from scratch.
    pub full: bool,
}

impl LayoutInvalidation {
    /// No relayout is required.
    #[must_use]
    pub const fn none() -> Self {
        Self { full: false }
    }
    /// A full relayout is required.
    #[must_use]
    pub const fn full() -> Self {
        Self { full: true }
    }
}

/// The complete output of one layout pass.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LayoutResult {
    /// Every node's rectangle, in its native parent's local coordinate
    /// space (children never inherit an ancestor's absolute position; see
    /// the module-level coordinate-space contract this mirrors from the
    /// original single-file design).
    pub rects: HashMap<NodeId, Rect>,
    /// Clip regions for containers whose overflow is not `Visible`.
    pub clips: HashMap<NodeId, Rect>,
    /// Each container's *unscrolled* content size — this must never be
    /// affected by the container's current scroll offset, or scrolling
    /// would feed back into its own available range.
    pub content_sizes: HashMap<NodeId, Size>,
    /// `content_size - viewport_size` (floored at zero) for every
    /// container, i.e. how far it can scroll along each axis.
    pub scroll_ranges: HashMap<NodeId, Size>,
    /// What each realized virtual-list item actually measured, for a
    /// backend to feed back into that list's [`ExtentCache`] (see
    /// [`MeasuredItem`]).
    ///
    /// Empty unless the tree contains a virtual list whose items are
    /// [`ItemExtent::Estimated`]: a list that states its item size has
    /// nothing to learn from measuring one.
    pub measured_items: Vec<MeasuredItem>,
}

impl LayoutResult {
    /// Each node's effective layout direction: its own override, else its
    /// parent's, with the window root taking `base`.
    #[must_use]
    pub fn directions(
        snapshot: &TreeSnapshot,
        base: super::LayoutDirection,
    ) -> HashMap<NodeId, super::LayoutDirection> {
        let mut directions = HashMap::new();
        for node in snapshot.ordered_nodes() {
            let inherited =
                node.parent.and_then(|parent| directions.get(&parent).copied()).unwrap_or(base);
            directions.insert(node.id, node.layout.direction.unwrap_or(inherited));
        }
        directions
    }

    /// The rectangles as a host *without* its own mirroring must place
    /// them: every child of a right-to-left container reflected about the
    /// middle of that container, so start is on the right.
    ///
    /// [`Self::rects`] are logical — start is always the left edge of the
    /// parent — which is what a host that mirrors for itself (Windows'
    /// `WS_EX_LAYOUTRTL`, a browser's `dir="rtl"`) wants: handing it
    /// mirrored rectangles would mirror twice. A host that does not (the
    /// headless backend, a terminal) applies this instead. One layout
    /// model, and the mirroring is applied exactly once, by whichever layer
    /// owns it (`PLAN.md` Milestone 39).
    ///
    /// ```
    /// use rustnative_core::{
    ///     LayoutDirection, LayoutEngine, LayoutResult, LayoutStyle, Node, NodeId, RowStyle, Size,
    ///     SizeMode, TreeSnapshot,
    /// };
    ///
    /// let fixed = LayoutStyle::new().width(SizeMode::Fixed(40));
    /// let row = Node::row_with_layout(
    ///     "row",
    ///     [Node::label_with_layout("first", "1", fixed), Node::label_with_layout("second", "2", fixed)],
    ///     LayoutStyle::new(),
    ///     RowStyle::new().gap(0),
    /// );
    /// let snapshot = TreeSnapshot::from_node(&row)?;
    /// let result = LayoutEngine::new().layout_result_with(
    ///     &snapshot,
    ///     Size::new(200, 50),
    ///     &rustnative_core::DefaultIntrinsicMeasurer,
    ///     &Default::default(),
    /// );
    /// let physical = result.physical_rects(&snapshot, LayoutDirection::Rtl);
    /// // In right-to-left, the first child sits at the right edge.
    /// let first = physical[&NodeId::from_key("first")];
    /// assert_eq!(first.x + first.width, 200 - RowStyle::new().padding.start);
    ///
    /// // The same row in markup:
    /// let markup = rustnative_core::rsx! {
    ///     <Row key="row" gap=0>
    ///         <Label key="first" text="1" width={SizeMode::Fixed(40)} />
    ///         <Label key="second" text="2" width={SizeMode::Fixed(40)} />
    ///     </Row>
    /// };
    /// assert_eq!(markup, row);
    /// # Ok::<(), rustnative_core::TreeError>(())
    /// ```
    #[must_use]
    pub fn physical_rects(
        &self,
        snapshot: &TreeSnapshot,
        base: super::LayoutDirection,
    ) -> HashMap<NodeId, Rect> {
        let directions = Self::directions(snapshot, base);
        let mut physical = self.rects.clone();
        for node in snapshot.ordered_nodes() {
            let Some(parent) = node.parent else { continue };
            if !directions.get(&parent).is_some_and(|direction| direction.is_rtl()) {
                continue;
            }
            let (Some(parent_rect), Some(rect)) =
                (self.rects.get(&parent), physical.get_mut(&node.id))
            else {
                continue;
            };
            // Parent-local coordinates: reflect within the parent's width.
            let content_width = self.content_sizes.get(&parent).map_or(parent_rect.width, |size| {
                i32::try_from(size.width).unwrap_or(i32::MAX).max(parent_rect.width)
            });
            rect.x = content_width.saturating_sub(rect.x).saturating_sub(rect.width);
        }
        physical
    }
}

/// One realized virtual-list item's measured extent along its list's axis.
///
/// Layout places items at the offsets the list's [`ExtentCache`] currently
/// implies, and reports what each one measured. A backend records those
/// measurements and lays out again if any of them moved an offset — one
/// extra pass, after which the cache and the measurements agree and the
/// pass is idempotent. This is what "incremental measurement" means here:
/// a list of a hundred thousand items learns the size of the twenty it
/// realized, not of the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeasuredItem {
    /// The virtual list the item belongs to.
    pub list: NodeId,
    /// Which item of that list it realizes.
    pub index: usize,
    /// How long it measured along the list's axis.
    pub extent: u32,
}

/// Computes absolute-within-parent geometry for a [`TreeSnapshot`]; see
/// the module-level documentation for the full contract.
///
/// # Example
///
/// ```
/// use rustnative_core::{
///     Constraints, LayoutEngine, LayoutStyle, Node, NodeId, Size, SizeMode, TreeSnapshot,
/// };
///
/// let tree = Node::column(
///     "root",
///     [
///         Node::label_with_layout(
///             "header",
///             "Header",
///             LayoutStyle::new().height(SizeMode::Fixed(40)),
///         ),
///         Node::label_with_layout(
///             "body",
///             "Body",
///             LayoutStyle::new()
///                 .width(SizeMode::Fill)
///                 .constraints(Constraints::new().with_min_width(120)),
///         ),
///     ],
/// );
///
/// let snapshot = TreeSnapshot::from_node(&tree)?;
/// let rects = LayoutEngine::new().layout(&snapshot, Size::new(320, 240));
///
/// let header = rects[&NodeId::from_key("header")];
/// assert_eq!(header.height, 40, "a fixed height is honored exactly");
///
/// // A declared minimum is honored even where it would not fit, which is
/// // what makes a scrollable container possible.
/// let narrow = LayoutEngine::new().layout(&snapshot, Size::new(20, 240));
/// assert!(narrow[&NodeId::from_key("body")].width >= 120);
///
/// // The same tree in markup:
/// let markup = rustnative_core::rsx! {
///     <Column key="root">
///         <Label key="header" text="Header" height={SizeMode::Fixed(40)} />
///         <Label key="body" text="Body" width={SizeMode::Fill} constraints={Constraints::new().with_min_width(120)} />
///     </Column>
/// };
/// assert_eq!(markup, tree);
/// # Ok::<(), rustnative_core::TreeError>(())
/// ```
#[derive(Debug, Default)]
pub struct LayoutEngine;

impl LayoutEngine {
    /// Creates a layout engine.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Lays out `snapshot` within `size` using the default intrinsic-size
    /// measurer, returning each node's rectangle.
    #[must_use]
    pub fn layout(&self, snapshot: &TreeSnapshot, size: Size) -> HashMap<NodeId, Rect> {
        self.layout_with(snapshot, size, &DefaultIntrinsicMeasurer)
    }

    /// Lays out `snapshot` within `size` using `measurer` for any node
    /// whose size depends on measuring its own content (e.g. `Auto`-sized
    /// text), returning each node's rectangle. See
    /// [`Self::layout_result_with`] for the complete [`LayoutResult`]
    /// (clip regions, content sizes, and scroll ranges as well).
    pub fn layout_with<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        size: Size,
        measurer: &M,
    ) -> HashMap<NodeId, Rect> {
        self.layout_result_with(snapshot, size, measurer, &HashMap::new()).rects
    }

    /// Runs a full layout pass.
    ///
    /// `extents` supplies each virtual list's [`ExtentCache`] — what the
    /// backend has measured about that list's items so far — keyed by the
    /// list's node. A list with no entry (or one whose cache disagrees with
    /// the item count the tree now declares) is laid out from the estimate
    /// its [`VirtualListStyle`] states, so a core-only caller needs to
    /// supply nothing.
    ///
    /// Scroll offsets are deliberately *not* an input: a scroll offset is a
    /// viewport transform a backend applies when placing native objects,
    /// not an input to how much content there is or how large it naturally
    /// wants to be (see [`LayoutResult::content_sizes`]). An earlier
    /// revision took one anyway, unused, "for API symmetry"; this takes
    /// what layout genuinely needs instead.
    pub fn layout_result_with<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        size: Size,
        measurer: &M,
        extents: &HashMap<NodeId, ExtentCache>,
    ) -> LayoutResult {
        let mut result = LayoutResult::default();
        let Some(root) = snapshot.ordered_nodes().into_iter().next() else {
            return result;
        };

        let root_rect = Rect::new(
            0,
            0,
            size.width.min(i32::MAX as u32) as i32,
            size.height.min(i32::MAX as u32) as i32,
        );

        self.layout_node(snapshot, root, root_rect, &mut result, measurer, extents);
        result
    }

    fn layout_node<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        node: &TreeNode,
        rect: Rect,
        result: &mut LayoutResult,
        measurer: &M,
        extents: &HashMap<NodeId, ExtentCache>,
    ) {
        // Rectangles are always expressed in the coordinate space of the
        // node's native parent. Descendants therefore start from (0, 0)
        // inside their own native container instead of inheriting the
        // parent's absolute position.
        result.rects.insert(node.id, rect);
        let content_rect = Rect::new(0, 0, rect.width, rect.height);

        // Hidden children keep their native objects and component state but
        // give up their space: they are laid out at zero size, apart from
        // the flow their visible siblings share.
        let (hidden, children): (Vec<_>, Vec<_>) =
            snapshot.children_of(node.id).partition(|child| child.hidden);
        for child in hidden {
            self.layout_node(snapshot, child, Rect::new(0, 0, 0, 0), result, measurer, extents);
        }

        match node.kind {
            NodeKind::Column => {
                let style = node.column_style.unwrap_or_default();
                if !matches!(style.overflow, Overflow::Visible) {
                    result.clips.insert(node.id, Rect::new(0, 0, rect.width, rect.height));
                }
                let content_size = if let Some(grid) = &node.grid {
                    self.layout_grid(
                        snapshot,
                        content_rect,
                        &children,
                        grid,
                        result,
                        measurer,
                        extents,
                    )
                } else if let Some(virtualization) = node.virtualization {
                    self.layout_virtual(
                        snapshot,
                        node,
                        content_rect,
                        &children,
                        virtualization,
                        style.align_items,
                        result,
                        measurer,
                        extents,
                    )
                } else {
                    self.layout_column(
                        snapshot,
                        content_rect,
                        &children,
                        style.into(),
                        result,
                        measurer,
                        extents,
                    )
                };
                result.content_sizes.insert(node.id, content_size);
                result.scroll_ranges.insert(
                    node.id,
                    Size::new(
                        content_size.width.saturating_sub(rect.width.max(0) as u32),
                        content_size.height.saturating_sub(rect.height.max(0) as u32),
                    ),
                );
            }
            NodeKind::Row => {
                let style = node.row_style.unwrap_or_default();
                if !matches!(style.overflow, Overflow::Visible) {
                    result.clips.insert(node.id, Rect::new(0, 0, rect.width, rect.height));
                }
                let content_size = if let Some(virtualization) = node.virtualization {
                    self.layout_virtual(
                        snapshot,
                        node,
                        content_rect,
                        &children,
                        virtualization,
                        style.align_items,
                        result,
                        measurer,
                        extents,
                    )
                } else {
                    self.layout_row(
                        snapshot,
                        content_rect,
                        &children,
                        style.into(),
                        result,
                        measurer,
                        extents,
                    )
                };
                result.content_sizes.insert(node.id, content_size);
                result.scroll_ranges.insert(
                    node.id,
                    Size::new(
                        content_size.width.saturating_sub(rect.width.max(0) as u32),
                        content_size.height.saturating_sub(rect.height.max(0) as u32),
                    ),
                );
            }
            NodeKind::Label
            | NodeKind::Button
            | NodeKind::TextInput
            | NodeKind::TabBar
            | NodeKind::Control
            | NodeKind::Canvas
            | NodeKind::Surface => {}
        }
    }

    /// Where each visible child of a grid goes, and how many rows there
    /// are.
    fn grid_cells(
        grid: &super::GridStyle,
        children: &[&TreeNode],
    ) -> (Vec<super::GridPlacement>, usize) {
        let columns = grid.columns.len().max(1);
        let placements = super::grid::place(
            columns,
            &children.iter().map(|child| child.layout.grid).collect::<Vec<_>>(),
        );
        let rows = placements
            .iter()
            .map(|placement| placement.row + placement.row_span)
            .max()
            .unwrap_or(0)
            .max(grid.rows.len());
        (placements, rows)
    }

    /// The tracks' sizes: columns from the children's natural widths within
    /// `width` (none when measuring), then rows from their heights at those
    /// widths within `height`.
    #[allow(clippy::too_many_arguments, reason = "one grid pass's inputs")]
    fn grid_tracks<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        grid: &super::GridStyle,
        children: &[&TreeNode],
        placements: &[super::GridPlacement],
        rows: usize,
        width: Option<i32>,
        height: Option<i32>,
        measurer: &M,
    ) -> (Vec<i32>, Vec<i32>) {
        let columns = grid.columns.len().max(1);
        let mut natural_widths = vec![0; columns];
        for (child, placement) in children.iter().zip(placements) {
            if placement.column_span == 1 {
                let wanted = self.preferred_width(snapshot, child, measurer);
                natural_widths[placement.column] = natural_widths[placement.column].max(wanted);
            }
        }
        let widths =
            super::grid::size_tracks(&grid.columns, columns, &natural_widths, grid.gap, width);
        let span = |sizes: &[i32], start: usize, count: usize| {
            let end = (start + count).min(sizes.len());
            sizes[start.min(end)..end].iter().copied().fold(0, i32::saturating_add)
                + grid.gap.max(0) * i32::try_from(count.saturating_sub(1)).unwrap_or(0)
        };
        let mut natural_heights = vec![0; rows];
        for (child, placement) in children.iter().zip(placements) {
            if placement.row_span == 1 {
                let available = span(&widths, placement.column, placement.column_span)
                    .saturating_sub(child.layout.margin.horizontal());
                let wanted =
                    self.preferred_height(snapshot, child, measurer, Some(available.max(0)));
                natural_heights[placement.row] = natural_heights[placement.row].max(wanted);
            }
        }
        let heights =
            super::grid::size_tracks(&grid.rows, rows, &natural_heights, grid.gap, height);
        (widths, heights)
    }

    /// A grid's natural `(width, height)`.
    fn grid_natural<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        node: &TreeNode,
        measurer: &M,
    ) -> (i32, i32) {
        let Some(grid) = &node.grid else { return (0, 0) };
        let children = ordered_children(snapshot, node.id)
            .into_iter()
            .filter(|child| !child.hidden)
            .collect::<Vec<_>>();
        let (placements, rows) = Self::grid_cells(grid, &children);
        let (widths, heights) =
            self.grid_tracks(snapshot, grid, &children, &placements, rows, None, None, measurer);
        let total = |sizes: &[i32]| {
            sizes.iter().copied().fold(0, i32::saturating_add)
                + grid.gap.max(0) * i32::try_from(sizes.len().saturating_sub(1)).unwrap_or(0)
        };
        (
            total(&widths).saturating_add(grid.padding.horizontal()),
            total(&heights).saturating_add(grid.padding.vertical()),
        )
    }

    /// Lays a grid's visible children out in its cells (`C18-1`).
    #[allow(
        clippy::too_many_arguments,
        reason = "one layout pass's inputs, not a decomposable set"
    )]
    fn layout_grid<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        rect: Rect,
        children: &[&TreeNode],
        grid: &super::GridStyle,
        result: &mut LayoutResult,
        measurer: &M,
        extents: &HashMap<NodeId, ExtentCache>,
    ) -> Size {
        let content = inner_rect(rect, grid.padding);
        let (placements, rows) = Self::grid_cells(grid, children);
        let (widths, heights) = self.grid_tracks(
            snapshot,
            grid,
            children,
            &placements,
            rows,
            Some(content.width),
            Some(content.height),
            measurer,
        );
        let gap = grid.gap.max(0);
        let starts = |sizes: &[i32], origin: i32| {
            let mut at = origin;
            sizes
                .iter()
                .map(|size| {
                    let start = at;
                    at = at.saturating_add(*size).saturating_add(gap);
                    start
                })
                .collect::<Vec<_>>()
        };
        let (xs, ys) = (starts(&widths, content.x), starts(&heights, content.y));
        let extent = |sizes: &[i32], start: usize, count: usize| {
            let end = (start + count).min(sizes.len());
            sizes[start.min(end)..end].iter().copied().fold(0, i32::saturating_add)
                + gap * i32::try_from(count.saturating_sub(1)).unwrap_or(0)
        };
        for (child, placement) in children.iter().zip(&placements) {
            let cell = Rect::new(
                xs.get(placement.column).copied().unwrap_or(content.x),
                ys.get(placement.row).copied().unwrap_or(content.y),
                extent(&widths, placement.column, placement.column_span),
                extent(&heights, placement.row, placement.row_span),
            );
            let margin = child.layout.margin;
            let inner = inner_rect(cell, margin);
            let width = match child.layout.width {
                SizeMode::Fixed(value) => value.max(0).min(inner.width),
                _ => inner.width,
            };
            let height = match child.layout.height {
                SizeMode::Fixed(value) => value.max(0).min(inner.height),
                _ => inner.height,
            };
            let child_rect = Rect::new(
                inner.x,
                inner.y,
                child.layout.constraints.clamp_width(width),
                child.layout.constraints.clamp_height(height),
            );
            self.layout_node(snapshot, child, child_rect, result, measurer, extents);
        }
        let total = |sizes: &[i32]| {
            sizes.iter().copied().fold(0, i32::saturating_add)
                + gap * i32::try_from(sizes.len().saturating_sub(1)).unwrap_or(0)
        };
        Size::new(
            u32::try_from(total(&widths).saturating_add(grid.padding.horizontal())).unwrap_or(0),
            u32::try_from(total(&heights).saturating_add(grid.padding.vertical())).unwrap_or(0),
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "one layout pass's inputs, not a decomposable set"
    )]
    fn layout_column<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        rect: Rect,
        children: &[&TreeNode],
        style: ResolvedContainerStyle,
        result: &mut LayoutResult,
        measurer: &M,
        extents: &HashMap<NodeId, ExtentCache>,
    ) -> Size {
        let ResolvedContainerStyle { padding, gap, align_items } = style;
        let content = inner_rect(rect, padding);
        if children.is_empty() {
            return Size::new(padding.horizontal().max(0) as u32, padding.vertical().max(0) as u32);
        }

        let gap_total = gap.max(0).saturating_mul(child_gap_count(children.len()));
        let usable_height = content.height.saturating_sub(gap_total).max(0);
        // The width each child will get, computed exactly as the placement
        // below computes it: a child's height is measured at that width, so
        // wrapping text is as tall as it wraps (Milestone 41's layout
        // conformance found labels measured at one line).
        let child_width = |child: &TreeNode| {
            let margin = child.layout.margin;
            let available_width = content.width.saturating_sub(margin.horizontal()).max(0);
            let alignment = child.layout.align_self.unwrap_or(align_items);
            let width = match (alignment, child.layout.width) {
                (Alignment::Stretch, SizeMode::Auto | SizeMode::Fill) => available_width,
                (_, SizeMode::Auto) => {
                    self.preferred_width(snapshot, child, measurer).min(available_width)
                }
                (_, mode) => resolve_width(mode, available_width),
            };
            child.layout.constraints.clamp_width(width.max(0).min(available_width))
        };
        let preferred_height = children
            .iter()
            .map(|child| self.preferred_height(snapshot, child, measurer, Some(child_width(child))))
            .fold(0, i32::saturating_add);
        let fill_count =
            children.iter().filter(|child| matches!(child.layout.height, SizeMode::Fill)).count();
        let distributable = usable_height.saturating_sub(preferred_height).max(0);
        let fill_count_i32 = fill_count.min(i32::MAX as usize) as i32;
        let share = if fill_count == 0 { 0 } else { distributable / fill_count_i32 };
        let remainder = if fill_count == 0 { 0 } else { distributable % fill_count_i32 };

        let mut y = content.y;
        let mut fill_index = 0;
        for (index, child) in children.iter().enumerate() {
            let margin = child.layout.margin;
            y = y.saturating_add(margin.top);
            let height = match child.layout.height {
                SizeMode::Fixed(value) => value.max(0),
                SizeMode::Auto => {
                    self.preferred_height(snapshot, child, measurer, Some(child_width(child)))
                }
                SizeMode::Fill => {
                    let extra = if fill_index == 0 { remainder } else { 0 };
                    fill_index += 1;
                    (self
                        .preferred_height(snapshot, child, measurer, Some(child_width(child)))
                        .saturating_add(share)
                        .saturating_add(extra))
                    .max(0)
                }
            };

            let available_width = content.width.saturating_sub(margin.horizontal()).max(0);
            let alignment = child.layout.align_self.unwrap_or(align_items);
            let width = match (alignment, child.layout.width) {
                (Alignment::Stretch, SizeMode::Auto | SizeMode::Fill) => available_width,
                (_, SizeMode::Auto) => {
                    self.preferred_width(snapshot, child, measurer).min(available_width)
                }
                (_, mode) => resolve_width(mode, available_width),
            };
            // The available-space cap is applied *before* the constraint
            // clamp, never after. Applied after, it would silently override
            // a declared minimum — a child with `min_width: 150` in a
            // narrower parent would come out narrower than 150, which is
            // not "constrained", it is "ignored". Overflowing the parent is
            // the correct outcome and the one the rest of the engine
            // already expects: `content_sizes`/`scroll_ranges` below are
            // computed from exactly this kind of overflow, which is what
            // makes a scrollable container work at all.
            let width = child.layout.constraints.clamp_width(width.max(0).min(available_width));
            let height = child.layout.constraints.clamp_height(height.max(0));
            let x = aligned_start(content.x, margin.start, available_width, width, alignment);
            let child_rect = Rect::new(x, y, width, height);
            self.layout_node(snapshot, child, child_rect, result, measurer, extents);

            y = y.saturating_add(height).saturating_add(margin.bottom);
            if index + 1 < children.len() {
                y = y.saturating_add(gap.max(0));
            }
        }

        // Content size must be computed from the unscrolled layout. Scrolling
        // changes the viewport position of children; it must never change the
        // size of the content itself or the available scroll range.
        let natural_height = y.saturating_add(padding.bottom).max(rect.height);
        let natural_width = children
            .iter()
            .map(|child| {
                result.rects.get(&child.id).map_or(0, |r| {
                    r.x.saturating_add(r.width).saturating_add(child.layout.margin.end)
                })
            })
            .max()
            .unwrap_or(0)
            .max(rect.width);

        Size::new(natural_width.max(0) as u32, natural_height.max(0) as u32)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "one layout pass's inputs, not a decomposable set"
    )]
    fn layout_row<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        rect: Rect,
        children: &[&TreeNode],
        style: ResolvedContainerStyle,
        result: &mut LayoutResult,
        measurer: &M,
        extents: &HashMap<NodeId, ExtentCache>,
    ) -> Size {
        let ResolvedContainerStyle { padding, gap, align_items } = style;
        let content = inner_rect(rect, padding);
        if children.is_empty() {
            return Size::new(padding.horizontal().max(0) as u32, padding.vertical().max(0) as u32);
        }

        let gap_total = gap.max(0).saturating_mul(child_gap_count(children.len()));
        let usable_width = content.width.saturating_sub(gap_total).max(0);
        let preferred_width = children
            .iter()
            .map(|child| self.preferred_width(snapshot, child, measurer))
            .fold(0, i32::saturating_add);
        let fill_count =
            children.iter().filter(|child| matches!(child.layout.width, SizeMode::Fill)).count();
        let distributable = usable_width.saturating_sub(preferred_width).max(0);
        let fill_count_i32 = fill_count.min(i32::MAX as usize) as i32;
        let share = if fill_count == 0 { 0 } else { distributable / fill_count_i32 };
        let remainder = if fill_count == 0 { 0 } else { distributable % fill_count_i32 };

        let mut x = content.x;
        let mut fill_index = 0;
        for (index, child) in children.iter().enumerate() {
            let margin = child.layout.margin;
            x = x.saturating_add(margin.start);
            let width = match child.layout.width {
                SizeMode::Fixed(value) => value.max(0),
                SizeMode::Auto => self.preferred_width(snapshot, child, measurer),
                SizeMode::Fill => {
                    let extra = if fill_index == 0 { remainder } else { 0 };
                    fill_index += 1;
                    self.preferred_width(snapshot, child, measurer)
                        .saturating_add(share)
                        .saturating_add(extra)
                        .max(0)
                }
            };

            let available_height = content.height.saturating_sub(margin.vertical()).max(0);
            let alignment = child.layout.align_self.unwrap_or(align_items);
            let height = match (alignment, child.layout.height) {
                (Alignment::Stretch, SizeMode::Auto | SizeMode::Fill) => available_height,
                (_, SizeMode::Auto) => self
                    .preferred_height(snapshot, child, measurer, Some(width))
                    .min(available_height),
                (_, mode) => resolve_width(mode, available_height),
            };
            let width = child.layout.constraints.clamp_width(width.max(0));
            // Cap before clamp, for the same reason as the column axis
            // above: a declared minimum must survive a parent too small to
            // hold it.
            let height = child.layout.constraints.clamp_height(height.max(0).min(available_height));
            let y = aligned_start(content.y, margin.top, available_height, height, alignment);
            let child_rect = Rect::new(x, y, width, height);
            self.layout_node(snapshot, child, child_rect, result, measurer, extents);

            x = x.saturating_add(width).saturating_add(margin.end);
            if index + 1 < children.len() {
                x = x.saturating_add(gap.max(0));
            }
        }

        // As with Column, measure the unscrolled content first. The scroll
        // offset is a viewport transform and must not feed back into the
        // intrinsic content size.
        let natural_width = x.saturating_add(padding.end).max(rect.width);
        let natural_height = children
            .iter()
            .map(|child| {
                result.rects.get(&child.id).map_or(0, |r| {
                    r.y.saturating_add(r.height).saturating_add(child.layout.margin.bottom)
                })
            })
            .max()
            .unwrap_or(0)
            .max(rect.height);

        Size::new(natural_width.max(0) as u32, natural_height.max(0) as u32)
    }

    /// Lays out a virtual list: every realized child at the offset its item
    /// index implies, and a content size covering *every* item, realized or
    /// not.
    ///
    /// That content size is the whole point. It is what gives the container
    /// a scroll range covering the full list, so scrolling a hundred
    /// thousand items works with twenty of them realized — see
    /// [`crate::virtualization`].
    ///
    /// Container padding and gap are deliberately not applied here; see
    /// that module for why.
    #[allow(
        clippy::too_many_arguments,
        reason = "one layout pass's inputs, not a decomposable set"
    )]
    fn layout_virtual<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        node: &TreeNode,
        rect: Rect,
        children: &[&TreeNode],
        style: VirtualListStyle,
        align_items: Alignment,
        result: &mut LayoutResult,
        measurer: &M,
        extents: &HashMap<NodeId, ExtentCache>,
    ) -> Size {
        // The backend's cache when it has one for this list and it still
        // describes a list of this length; otherwise what the style states,
        // so a core-only caller (or a list whose item count just changed)
        // is laid out from the estimate rather than from stale offsets.
        let fallback;
        let cache = match extents.get(&node.id) {
            Some(cache) if cache.count() == style.item_count => cache,
            _ => {
                fallback = style.extents();
                &fallback
            }
        };

        let mut cross_end = 0;
        for child in children {
            // An item that did not say which index it realizes is taken to
            // be at its position among its realized siblings, which is what
            // a small, fully realized list looks like.
            let index = child.item_index.unwrap_or(child.index);
            let margin = child.layout.margin;
            let main_start = to_i32(cache.offset_of(index));
            let available_cross = match style.axis {
                Axis::Vertical => rect.width.saturating_sub(margin.horizontal()).max(0),
                Axis::Horizontal => rect.height.saturating_sub(margin.vertical()).max(0),
            };
            let alignment = child.layout.align_self.unwrap_or(align_items);

            let (main, cross) = match style.axis {
                Axis::Vertical => {
                    let width = self.cross_extent(
                        snapshot,
                        child,
                        measurer,
                        alignment,
                        child.layout.width,
                        available_cross,
                    );
                    let height = self.item_extent(
                        snapshot,
                        child,
                        measurer,
                        style.extent,
                        child.layout.height,
                        Some(width),
                        result,
                        node.id,
                        index,
                    );
                    (height, width)
                }
                Axis::Horizontal => {
                    let width = self.item_extent(
                        snapshot,
                        child,
                        measurer,
                        style.extent,
                        child.layout.width,
                        None,
                        result,
                        node.id,
                        index,
                    );
                    let height = self.cross_extent(
                        snapshot,
                        child,
                        measurer,
                        alignment,
                        child.layout.height,
                        available_cross,
                    );
                    (width, height)
                }
            };

            let child_rect = match style.axis {
                Axis::Vertical => {
                    let width = child.layout.constraints.clamp_width(cross.max(0));
                    let height = child.layout.constraints.clamp_height(main.max(0));
                    let x = aligned_start(rect.x, margin.start, available_cross, width, alignment);
                    cross_end = cross_end.max(x.saturating_add(width));
                    Rect::new(x, rect.y.saturating_add(main_start), width, height)
                }
                Axis::Horizontal => {
                    let width = child.layout.constraints.clamp_width(main.max(0));
                    let height = child.layout.constraints.clamp_height(cross.max(0));
                    let y = aligned_start(rect.y, margin.top, available_cross, height, alignment);
                    cross_end = cross_end.max(y.saturating_add(height));
                    Rect::new(rect.x.saturating_add(main_start), y, width, height)
                }
            };
            self.layout_node(snapshot, child, child_rect, result, measurer, extents);
        }

        // Every item, not just the realized ones: this is what the
        // container scrolls over.
        let total = to_i32(cache.total());
        match style.axis {
            Axis::Vertical => Size::new(
                cross_end.max(rect.width).max(0) as u32,
                total.max(rect.height).max(0) as u32,
            ),
            Axis::Horizontal => Size::new(
                total.max(rect.width).max(0) as u32,
                cross_end.max(rect.height).max(0) as u32,
            ),
        }
    }

    /// How long one virtual-list item is along the list's axis, recording
    /// what it measured when the list's items are estimated rather than
    /// fixed.
    #[allow(clippy::too_many_arguments, reason = "one item's inputs, not a decomposable set")]
    fn item_extent<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        child: &TreeNode,
        measurer: &M,
        policy: ItemExtent,
        mode: SizeMode,
        width_hint: Option<i32>,
        result: &mut LayoutResult,
        list: NodeId,
        index: usize,
    ) -> i32 {
        match policy {
            // The application stated the size; nothing is measured, and an
            // item that would rather be another size is given this one.
            ItemExtent::Fixed(extent) => to_i32(extent),
            ItemExtent::Estimated(_) => {
                // An item that declares its own size along the axis is that
                // size, exactly as in an ordinary column or row;
                // `Fill` has nothing to fill in a list whose length is the
                // sum of its items, so it measures like `Auto`.
                let natural = match mode {
                    SizeMode::Fixed(value) => value.max(0),
                    SizeMode::Auto | SizeMode::Fill => match width_hint {
                        Some(width) => {
                            self.preferred_height(snapshot, child, measurer, Some(width))
                        }
                        None => self.preferred_width(snapshot, child, measurer),
                    },
                }
                .max(0);
                result.measured_items.push(MeasuredItem { list, index, extent: natural as u32 });
                // Drawn at what it measured rather than at the estimate the
                // cache still holds, so an item is never clipped by an
                // estimate that was too small; the cache catches up on the
                // pass the backend runs after recording this.
                natural
            }
        }
    }

    /// How long one virtual-list item is across the list's axis.
    fn cross_extent<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        child: &TreeNode,
        measurer: &M,
        alignment: Alignment,
        mode: SizeMode,
        available: i32,
    ) -> i32 {
        match (alignment, mode) {
            (Alignment::Stretch, SizeMode::Auto | SizeMode::Fill) => available,
            (_, SizeMode::Auto) => self.preferred_width(snapshot, child, measurer).min(available),
            (_, mode) => resolve_width(mode, available),
        }
    }

    fn preferred_width<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        node: &TreeNode,
        measurer: &M,
    ) -> i32 {
        let base = match node.kind {
            NodeKind::Label | NodeKind::Button | NodeKind::TextInput | NodeKind::TabBar => {
                (measurer
                    .measure_styled(
                        node.kind,
                        node.text.as_deref(),
                        None,
                        node.visual_style.properties().typography_override(),
                    )
                    .width as i32)
                    .saturating_add(leaf_padding(node).horizontal())
            }
            NodeKind::Control => node
                .control
                .as_ref()
                .map_or(0, |control| measurer.measure_control(control).width as i32),
            // A foreign object has the size its factory reports.
            NodeKind::Surface if node.foreign.is_some() => {
                measurer.measure_foreign(node.foreign.as_deref().unwrap_or_default()).width as i32
            }
            // A picture has no natural size: it is as big as layout makes
            // it, so it must be given one.
            NodeKind::Canvas | NodeKind::Surface => 0,
            NodeKind::Column | NodeKind::Row => {
                self.container_preferred_width(snapshot, node, measurer)
            }
        };
        node.layout.constraints.clamp_width(base.saturating_add(node.layout.margin.horizontal()))
    }

    fn preferred_height<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        node: &TreeNode,
        measurer: &M,
        max_width: Option<i32>,
    ) -> i32 {
        let height = self
            .preferred_content_height(snapshot, node, measurer, max_width)
            .saturating_add(node.layout.margin.vertical());
        node.layout.constraints.clamp_height(height)
    }

    fn preferred_content_height<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        node: &TreeNode,
        measurer: &M,
        max_width: Option<i32>,
    ) -> i32 {
        if node.virtualization.is_some() {
            // A virtual list does not want to be as tall as its content —
            // being *shorter* than its content is the entire point, and
            // asking for the sum of a hundred thousand items would size the
            // window to the data. It takes whatever it is given (`Fill` or
            // a fixed size); its content length reaches the backend through
            // `LayoutResult::content_sizes`, which is what it scrolls over.
            return 0;
        }
        match node.kind {
            NodeKind::Label | NodeKind::Button | NodeKind::TextInput | NodeKind::TabBar => {
                let padding = leaf_padding(node);
                (measurer
                    .measure_styled(
                        node.kind,
                        node.text.as_deref(),
                        max_width.map(|width| width.saturating_sub(padding.horizontal())),
                        node.visual_style.properties().typography_override(),
                    )
                    .height as i32)
                    .saturating_add(padding.vertical())
            }
            NodeKind::Control => node
                .control
                .as_ref()
                .map_or(0, |control| measurer.measure_control(control).height as i32),
            NodeKind::Surface if node.foreign.is_some() => {
                measurer.measure_foreign(node.foreign.as_deref().unwrap_or_default()).height as i32
            }
            NodeKind::Canvas | NodeKind::Surface => 0,
            NodeKind::Column if node.grid.is_some() => {
                self.grid_natural(snapshot, node, measurer).1
            }
            NodeKind::Column => {
                let style = node.column_style.unwrap_or_default();
                let children = ordered_children(snapshot, node.id);
                style
                    .padding
                    .vertical()
                    .saturating_add(
                        children
                            .iter()
                            .map(|child| {
                                self.preferred_height(snapshot, child, measurer, max_width)
                            })
                            .fold(0, i32::saturating_add),
                    )
                    .saturating_add(
                        style.gap.max(0).saturating_mul(child_gap_count(children.len())),
                    )
            }
            NodeKind::Row => {
                let style = node.row_style.unwrap_or_default();
                let children = ordered_children(snapshot, node.id);
                style.padding.vertical().saturating_add(
                    children
                        .iter()
                        .map(|child| {
                            self.preferred_height(
                                snapshot,
                                child,
                                measurer,
                                preferred_width_hint(child).or(max_width),
                            )
                        })
                        .max()
                        .unwrap_or(0),
                )
            }
        }
    }

    fn container_preferred_width<M: IntrinsicMeasurer>(
        &self,
        snapshot: &TreeSnapshot,
        node: &TreeNode,
        measurer: &M,
    ) -> i32 {
        if node.virtualization.is_some() {
            // As in `preferred_content_height`: a virtual list is sized by
            // its parent, not by its items.
            return 0;
        }
        let children = ordered_children(snapshot, node.id);
        match node.kind {
            NodeKind::Column if node.grid.is_some() => {
                self.grid_natural(snapshot, node, measurer).0
            }
            NodeKind::Column => {
                let style = node.column_style.unwrap_or_default();
                style.padding.horizontal().saturating_add(
                    children
                        .iter()
                        .map(|child| self.preferred_width(snapshot, child, measurer))
                        .max()
                        .unwrap_or(0),
                )
            }
            NodeKind::Row => {
                let style = node.row_style.unwrap_or_default();
                style
                    .padding
                    .horizontal()
                    .saturating_add(
                        children
                            .iter()
                            .map(|child| self.preferred_width(snapshot, child, measurer))
                            .fold(0, i32::saturating_add),
                    )
                    .saturating_add(
                        style.gap.max(0).saturating_mul(child_gap_count(children.len())),
                    )
            }
            _ => 0,
        }
    }
}

/// `count - 1` gaps between `count` siblings, saturated at zero and clamped
/// into `i32` range so a pathologically large sibling count cannot overflow
/// the later `saturating_mul` by itself producing a wrapped *negative* count.
fn child_gap_count(sibling_count: usize) -> i32 {
    sibling_count.saturating_sub(1).min(i32::MAX as usize) as i32
}

fn preferred_width_hint(node: &TreeNode) -> Option<i32> {
    match node.layout.width {
        SizeMode::Fixed(value) => Some(value.max(0)),
        SizeMode::Auto | SizeMode::Fill => node.layout.constraints.max_width(),
    }
}

fn ordered_children(snapshot: &TreeSnapshot, parent: NodeId) -> Vec<&TreeNode> {
    snapshot.children_of(parent).collect()
}

/// A text leaf's own padding (its visual style's): the host insets its text
/// by it inside the leaf's box, so the box is measured with it.
fn leaf_padding(node: &TreeNode) -> EdgeInsets {
    node.visual_style.properties().padding_override().unwrap_or_default()
}

fn inner_rect(rect: Rect, padding: EdgeInsets) -> Rect {
    Rect::new(
        rect.x.saturating_add(padding.start),
        rect.y.saturating_add(padding.top),
        rect.width.saturating_sub(padding.horizontal()).max(0),
        rect.height.saturating_sub(padding.vertical()).max(0),
    )
}

fn aligned_start(
    origin: i32,
    margin_start: i32,
    available: i32,
    size: i32,
    alignment: Alignment,
) -> i32 {
    let remaining = available.saturating_sub(size).max(0);
    origin.saturating_add(margin_start).saturating_add(match alignment {
        Alignment::Start | Alignment::Stretch => 0,
        Alignment::Center => remaining / 2,
        Alignment::End => remaining,
    })
}

fn resolve_width(mode: SizeMode, available: i32) -> i32 {
    match mode {
        SizeMode::Fixed(value) => value.max(0).min(available.max(0)),
        SizeMode::Auto | SizeMode::Fill => available.max(0),
    }
}

/// Converts an extent or offset from the unsigned representation
/// [`ExtentCache`] uses into this module's `i32` working type, saturating
/// rather than wrapping (see the module's note on casts).
fn to_i32(value: u32) -> i32 {
    value.min(i32::MAX as u32) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{ColumnStyle, Constraints, LayoutStyle};
    use crate::node::Node;

    fn snapshot_of(node: &Node) -> TreeSnapshot {
        TreeSnapshot::from_node(node).expect("test tree must have unique ids")
    }

    fn virtual_rows(extent: ItemExtent, rows: std::ops::Range<usize>, height: i32) -> Node {
        Node::column(
            "root",
            [Node::virtual_list(
                "list",
                VirtualListStyle::new(100_000, extent),
                rows.map(|index| {
                    Node::column_with_layout(
                        format!("row-{index}"),
                        [],
                        LayoutStyle::new().height(SizeMode::Fixed(height)),
                        ColumnStyle::new(),
                    )
                    .with_item_index(index)
                }),
            )],
        )
    }

    #[test]
    fn a_virtual_list_places_each_item_at_its_index_offset_and_scrolls_over_all_of_them() {
        let tree = virtual_rows(ItemExtent::Fixed(20), 500..505, 20);
        let output = LayoutEngine::new().layout_result_with(
            &snapshot_of(&tree),
            Size::new(300, 400),
            &DefaultIntrinsicMeasurer,
            &HashMap::new(),
        );
        let list = NodeId::from_key("list");
        for index in 500_i32..505 {
            let rect = output.rects[&NodeId::from_key(&format!("row-{index}"))];
            assert_eq!(rect.y, index * 20, "row {index} sits at its offset");
            assert_eq!(rect.height, 20);
        }
        assert_eq!(output.content_sizes[&list].height, 2_000_000, "all 100,000 rows, not 5");
        assert!(output.measured_items.is_empty(), "fixed rows are never measured");
    }

    #[test]
    fn a_virtual_list_is_sized_by_its_parent_not_by_its_items() {
        // Five realized rows would be 100 px tall; a hundred thousand would
        // be two million. The list gets neither: it fills what is left.
        let tree = virtual_rows(ItemExtent::Fixed(20), 0..5, 20);
        let rects = LayoutEngine::new().layout(&snapshot_of(&tree), Size::new(300, 400));
        let list = rects[&NodeId::from_key("list")];
        assert_eq!(list.height, 400 - ColumnStyle::new().padding.vertical());
    }

    #[test]
    fn estimated_items_report_what_they_measured() {
        let tree = virtual_rows(ItemExtent::Estimated(20), 0..3, 45);
        let output = LayoutEngine::new().layout_result_with(
            &snapshot_of(&tree),
            Size::new(300, 400),
            &DefaultIntrinsicMeasurer,
            &HashMap::new(),
        );
        let list = NodeId::from_key("list");
        assert_eq!(
            output.measured_items,
            (0..3).map(|index| MeasuredItem { list, index, extent: 45 }).collect::<Vec<_>>()
        );
        // Placed from the estimate this pass; the backend records the
        // measurements and lays out again.
        assert_eq!(output.rects[&NodeId::from_key("row-1")].y, 20);

        let mut cache = ExtentCache::new(100_000, ItemExtent::Estimated(20));
        for item in &output.measured_items {
            cache.record(item.index, item.extent);
        }
        let settled = LayoutEngine::new().layout_result_with(
            &snapshot_of(&tree),
            Size::new(300, 400),
            &DefaultIntrinsicMeasurer,
            &HashMap::from([(list, cache)]),
        );
        assert_eq!(settled.rects[&NodeId::from_key("row-1")].y, 45, "the second pass has settled");
        assert_eq!(settled.rects[&NodeId::from_key("row-2")].y, 90);
    }

    #[test]
    fn layout_respects_fixed_height_and_gap() {
        let tree = Node::column(
            "root",
            [
                Node::label_with_layout("a", "a", LayoutStyle::new().height(SizeMode::Fixed(10))),
                Node::label_with_layout("b", "b", LayoutStyle::new().height(SizeMode::Fixed(20))),
            ],
        );
        let snapshot = snapshot_of(&tree);
        let engine = LayoutEngine::new();
        let rects = engine.layout(&snapshot, Size::new(200, 200));
        let a = rects[&NodeId::from_key("a")];
        let b = rects[&NodeId::from_key("b")];
        assert_eq!(a.height, 10);
        assert_eq!(b.height, 20);
        assert!(b.y > a.y);
    }

    #[test]
    fn nested_layout_coordinates_are_relative_to_parent() {
        let tree = Node::column(
            "root",
            [Node::column_with_layout(
                "inner",
                [Node::label("leaf", "leaf")],
                LayoutStyle::new(),
                ColumnStyle::new().padding(EdgeInsets::all(5)),
            )],
        );
        let snapshot = snapshot_of(&tree);
        let engine = LayoutEngine::new();
        let rects = engine.layout(&snapshot, Size::new(200, 200));
        let leaf = rects[&NodeId::from_key("leaf")];
        // Relative to `inner`'s own content rect, not the root's.
        assert_eq!(leaf.x, 5);
        assert_eq!(leaf.y, 5);
    }

    #[test]
    fn a_leafs_padding_is_measured_into_its_box() {
        let tree = Node::row(
            "root",
            [
                Node::label("plain", "badge"),
                Node::label("padded", "badge").with_style(
                    crate::style::VisualStyle::new().padding(EdgeInsets {
                        top: 2,
                        end: 8,
                        bottom: 2,
                        start: 8,
                    }),
                ),
            ],
        );
        let snapshot =
            TreeSnapshot::from_node_with_theme(&tree, &crate::style::Theme::default()).unwrap();
        let rects = LayoutEngine::new().layout(&snapshot, Size::new(400, 100));
        let plain = rects[&NodeId::from_key("plain")];
        let padded = rects[&NodeId::from_key("padded")];
        assert_eq!(padded.width, plain.width + 16);
    }

    #[test]
    fn extreme_constraints_do_not_panic_or_produce_negative_geometry() {
        let tree = Node::column(
            "root",
            [Node::label_with_layout(
                "huge",
                "x".repeat(10_000),
                LayoutStyle::new()
                    .width(SizeMode::Fixed(i32::MAX))
                    .height(SizeMode::Fixed(i32::MAX))
                    .constraints(Constraints::new().with_max_width(i32::MAX)),
            )],
        );
        let snapshot = snapshot_of(&tree);
        let engine = LayoutEngine::new();
        let rects = engine.layout(&snapshot, Size::new(u32::MAX, u32::MAX));
        let huge = rects[&NodeId::from_key("huge")];
        assert!(huge.width >= 0);
        assert!(huge.height >= 0);
    }
}
