//! The renderer proper: applies a `TreeDiff` to native objects, then runs
//! layout and positions them.
//!
//! [`Renderer`] is a **coordinator**, in the sense the standards audit's
//! P1.21 finding uses the word: it sequences the phases of a render and
//! owns the state that spans them (the current snapshot, the layout
//! rectangles), and it delegates every phase's actual work to the module
//! that owns that phase's invariants. Control creation lives in
//! [`super::controls`], GDI resources in [`super::styling`], semantics in
//! [`super::accessibility`], viewport transforms in [`super::scrolling`].
//!
//! # Phase order
//!
//! ```text
//! render(root)
//!   ├─ snapshot the declarative tree, resolving theme styles
//!   ├─ diff against the previous snapshot
//!   ├─ apply each operation   → controls / styling / accessibility
//!   └─ if the diff touched geometry, relayout
//!         ├─ measure the window's client area
//!         ├─ run the platform-independent layout engine
//!         ├─ reconcile scroll offsets against the new ranges
//!         └─ position every native object
//! ```
//!
//! Layout deliberately runs *after* the whole native tree is reconciled,
//! never interleaved with it: every object that will exist this frame
//! exists before any of them is positioned, so geometry does not depend on
//! the order operations happened to arrive in.

use std::collections::{HashMap, HashSet};

use rustnative_core::{
    AnimatedProperty, AnimatedValue, ControlState, Frame, LayoutEngine, LayoutResult, NodeId,
    NodeKind, Overflow, Point, Rect, Scalar, Size, StyleOverride, Theme, Transition, TreeDiff,
    TreeNode, TreeOp, TreeSnapshot, VirtualRange, VisualStyle,
};
use windows_sys::Win32::Foundation::{HWND, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::InvalidateRect;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetClientRect, GetParent, GetWindowLongPtrW, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE,
    SWP_NOZORDER, SendMessageW, SetParent, SetWindowPos, ShowWindow, WM_SETFONT, WS_VISIBLE,
};

use super::super::EnableWindow;
use super::super::measure::WindowsIntrinsicMeasurer;
use super::super::registry::{NativeObject, NativeObjectRegistry};
use super::super::user_data::BackgroundColorSlot;
use super::super::win32::{best_effort, ignored_by_contract, informational};
use super::accessibility::AccessibilityBridge;
use super::animated::AnimatedOverrides;
use super::controls;
use super::pool::ControlPool;
use super::scrolling::{ScrollState, dimension_to_u32};
use super::styling::StyleCache;
use super::virtual_list::VirtualLists;
use crate::Error;

/// Reconciles one window's native Win32 objects against the framework's
/// declarative tree.
#[derive(Debug)]
pub(crate) struct Renderer {
    /// Every native object this window owns, in both directions.
    pub(crate) registry: NativeObjectRegistry,
    /// Realized visual styles and live interaction state.
    pub(crate) styles: StyleCache,
    /// The tree currently realized natively, and the baseline the next
    /// render diffs against.
    pub(crate) snapshot: TreeSnapshot,
    /// Node identities whose next `EN_CHANGE` came from this renderer's own
    /// `SetWindowTextW` rather than from the person typing.
    pub(crate) suppress_text_change: HashSet<NodeId>,
    layout: HashMap<NodeId, Rect>,
    /// The rectangle each object was last moved to, with the object it was
    /// — so a relayout moves only what moved. Moving every object on every
    /// relayout cost 4–10 ms a click on a form of forty rows (`PLAN.md`
    /// Milestone 42's input-latency budget).
    positioned: HashMap<NodeId, (isize, Rect)>,
    /// Per-frame animated values — see `rendering::animated`.
    animated: AnimatedOverrides,
    /// Transitions the last render or layout pass found, waiting for
    /// `native::animation` to start them.
    pending_transitions: Vec<TransitionRequest>,
    /// Nodes that arrived in place of one with the same shared identity,
    /// waiting for layout to say where they move to (`C25`).
    matched: Vec<rustnative_core::MatchedGeometry>,
    /// Nodes removed since the last time animation state was reconciled.
    removed_nodes: Vec<NodeId>,
    scroll: ScrollState,
    /// Each virtual list's measured extents and current visible range — see
    /// `rendering::virtual_list`.
    pub(crate) virtual_lists: VirtualLists,
    /// Native windows a removal parked for an insertion in the same render
    /// to take back — see `rendering::pool`.
    pool: ControlPool,
    /// Each native surface's last reported size, so a relayout that did not
    /// change it does not tell the application to rebuild its swapchain.
    /// The size and DPI each surface was last reported at.
    surface_sizes: HashMap<NodeId, (Size, u32)>,
    /// The window's DPI as `WM_DPICHANGED` last announced it, which is what
    /// a surface's scale factor follows once the window has moved.
    dpi: Option<u32>,
    /// Set when the system settings styles follow changed.
    restyle_all: bool,
    /// Surface size changes not yet reported (see
    /// [`Renderer::take_surface_changes`]).
    surface_changes: Vec<SurfaceChange>,
    /// The semantic projection onto Win32 (tab stops, MSAA annotations) and
    /// UI Automation — see `rendering::accessibility` and `native::uia`.
    pub(crate) accessibility: AccessibilityBridge,
    layout_engine: LayoutEngine,
    /// Right-to-left, as applied to the native objects (see
    /// `rendering::direction`).
    pub(crate) direction: super::direction::DirectionState,
}

impl Renderer {
    /// A node's laid-out size, if it has been laid out.
    pub(crate) fn layout_size(&self, id: NodeId) -> Option<Size> {
        self.layout
            .get(&id)
            .map(|rect| Size::new(dimension_to_u32(rect.width), dimension_to_u32(rect.height)))
    }

    /// Every laid-out node's rectangle, in its parent's coordinates.
    pub(crate) const fn layout_rects(&self) -> &HashMap<NodeId, Rect> {
        &self.layout
    }

    /// The snapshot currently realized.
    pub(crate) fn snapshot(&self) -> &TreeSnapshot {
        &self.snapshot
    }

    pub(crate) fn new() -> Self {
        Self {
            registry: NativeObjectRegistry::default(),
            styles: StyleCache::default(),
            snapshot: TreeSnapshot::default(),
            suppress_text_change: HashSet::new(),
            layout: HashMap::new(),
            positioned: HashMap::new(),
            animated: AnimatedOverrides::default(),
            pending_transitions: Vec::new(),
            matched: Vec::new(),
            removed_nodes: Vec::new(),
            scroll: ScrollState::default(),
            virtual_lists: VirtualLists::default(),
            pool: ControlPool::default(),
            surface_sizes: HashMap::new(),
            dpi: None,
            restyle_all: false,
            surface_changes: Vec::new(),
            accessibility: AccessibilityBridge::default(),
            layout_engine: LayoutEngine,
            direction: super::direction::DirectionState::default(),
        }
    }

    /// Reconciles the native tree against `root`, then relayouts if the
    /// change could have affected geometry.
    pub(crate) fn render(
        &mut self,
        root: &rustnative_core::Node,
        window: HWND,
        theme: &Theme,
    ) -> Result<(), Error> {
        let next =
            TreeSnapshot::from_node_with_theme(root, theme).map_err(|error| match error {
                rustnative_core::TreeError::DuplicateNodeId(id) => {
                    Error::DuplicateNodeId { node: id }
                }
            })?;

        let diff = TreeDiff::between(&self.snapshot, &next);
        self.styles.set_theme(theme.clone());
        let restyle_all = std::mem::take(&mut self.restyle_all);
        // Which item each list is scrolled to, captured against the tree
        // that is still on screen: once the operations below have run, the
        // items that answered that question may be gone.
        let layout_invalidated = diff.invalidates_layout();
        if layout_invalidated {
            self.capture_anchors();
        }
        // Before the operations: removing a node forgets its rectangle.
        self.matched = rustnative_core::matched_geometry(&self.snapshot, &self.layout, &next);
        for operation in diff.operations() {
            self.apply_operation(operation, window)?;
        }
        // Whatever no insertion took back is not coming back (see
        // `rendering::pool`).
        if !self.pool.is_empty() {
            self.pool.drain();
        }

        self.collect_appearance_transitions(&next);
        self.snapshot = next;
        // A system setting changed (high contrast, text scale): every
        // realized style follows it, on the objects that already exist.
        if restyle_all {
            for node in self.snapshot.nodes().cloned().collect::<Vec<_>>() {
                self.apply_control_style(&node);
            }
        }
        self.virtual_lists.sync(&self.snapshot);
        if self.accessibility.commit(window, &self.snapshot, &self.registry) {
            crate::native::uia::schedule(window);
        }
        // Interaction state (hover, press, focus) for nodes that no longer
        // exist would otherwise accumulate for the life of the window.
        let snapshot = &self.snapshot;
        self.styles.retain_interaction(|id| snapshot.contains(id));
        if layout_invalidated || restyle_all {
            self.relayout(window);
        }
        Ok(())
    }

    /// Records the system settings styles follow; a change restyles every
    /// node and lays the window out again at the next render.
    pub(crate) fn set_host_settings(&mut self, host: super::styling::HostSettings) {
        if self.styles.set_host_settings(host) {
            self.restyle_all = true;
        }
    }

    /// Runs layout for the whole window and applies the result to every
    /// native object.
    pub(crate) fn relayout(&mut self, window: HWND) {
        let mut client = RECT::default();
        // SAFETY: `window` is a live HWND owned by this renderer's window
        // for the duration of `relayout`; `client` is a valid, exclusively
        // borrowed `RECT` for `GetClientRect` to write into.
        let read_client_rect = unsafe { GetClientRect(window, &raw mut client) } != 0;
        // Best effort: a zeroed `RECT` lays the tree out at zero size,
        // which is the correct result for a window with no client area
        // (minimized, or mid-destruction) and self-corrects on the next
        // `WM_SIZE`.
        best_effort(
            read_client_rect,
            "GetClientRect(window)",
            "layout falls back to a zero-size client",
        );

        let size = Size::new(
            dimension_to_u32(client.right - client.left),
            dimension_to_u32(client.bottom - client.top),
        );
        let measurer = WindowsIntrinsicMeasurer { window, text_scale: self.styles.text_scale() };
        let output = self.measure_and_lay_out(size, &measurer);

        self.collect_geometry_transitions(&output.rects);
        self.layout = output.rects;
        let snapshot = &self.snapshot;
        self.scroll
            .adopt_layout(output.scroll_ranges, output.content_sizes, |id| snapshot.contains(id));
        if !self.virtual_lists.is_empty() {
            self.resolve_anchors();
            self.update_visible_ranges();
        }

        // Direction before geometry: a parent's mirroring decides how the
        // positions given to its children are read.
        self.direction.apply(window, &self.snapshot, &self.registry);

        // Layout is a distinct phase: only after the native tree is fully
        // reconciled do we apply geometry to every object. This makes layout
        // independent of reconciliation order.
        for id in self.snapshot.nodes().map(|node| node.id).collect::<Vec<_>>() {
            self.position_node(id);
        }
        self.collect_surface_changes();
    }

    /// Queues a `SurfaceResized` for every native surface whose laid-out
    /// size differs from the one last reported — including a surface
    /// reported for the first time.
    pub(crate) fn collect_surface_changes(&mut self) {
        let snapshot = &self.snapshot;
        self.surface_sizes.retain(|id, _| snapshot.contains(*id));
        for (id, object) in self
            .snapshot
            .nodes()
            .filter_map(|node| self.registry.get(node.id).map(|object| (node.id, object)))
        {
            let NativeObject::Surface { hwnd, id: surface } = object else {
                continue;
            };
            let Some(rect) = self.layout.get(&id) else {
                continue;
            };
            let size = Size::new(dimension_to_u32(rect.width), dimension_to_u32(rect.height));
            // SAFETY: `hwnd` is a live window owned by this renderer's
            // registry; the call takes no pointers.
            let dpi = self.dpi.unwrap_or_else(|| unsafe { GetDpiForWindow(*hwnd) });
            // Reported again when either changes: a DPI change without a
            // size change still changes what a swapchain must render at.
            if self.surface_sizes.insert(id, (size, dpi)) == Some((size, dpi)) {
                continue;
            }
            #[allow(
                clippy::cast_precision_loss,
                reason = "a DPI value, far below f32's exact range"
            )]
            let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
            self.surface_changes.push(SurfaceChange {
                node: id,
                surface: *surface,
                size,
                scale_factor: Scalar::new(scale),
            });
        }
    }

    /// Forgets where every object was moved, so the next relayout moves
    /// them all — after a change that moves objects without changing their
    /// rectangles (the window's reading direction).
    pub(crate) fn forget_positions(&mut self) {
        self.positioned.clear();
    }

    /// Records the DPI `WM_DPICHANGED` announced for this window.
    pub(crate) fn set_dpi(&mut self, dpi: u32) {
        self.dpi = Some(dpi);
    }

    /// Native surfaces whose size changed, for the runtime to report.
    pub(crate) fn take_surface_changes(&mut self) -> Vec<SurfaceChange> {
        std::mem::take(&mut self.surface_changes)
    }

    /// Runs layout, and runs it once more if measuring the realized items
    /// of a virtual list moved any of that list's offsets.
    ///
    /// The second pass is bounded, not a loop: an item's measured extent
    /// does not depend on where the list decided to put it, so once those
    /// measurements are recorded, laying out again produces the same ones
    /// and the pass is idempotent. See
    /// [`rustnative_core::MeasuredItem`](rustnative_core::MeasuredItem).
    fn measure_and_lay_out(
        &mut self,
        size: Size,
        measurer: &WindowsIntrinsicMeasurer,
    ) -> LayoutResult {
        let output = self.layout_engine.layout_result_with(
            &self.snapshot,
            size,
            measurer,
            self.virtual_lists.extents(),
        );
        if !self.virtual_lists.record(&output.measured_items) {
            return output;
        }
        self.layout_engine.layout_result_with(
            &self.snapshot,
            size,
            measurer,
            self.virtual_lists.extents(),
        )
    }

    /// Remembers which item sits at the top of each virtual list's
    /// viewport, so the render about to run can be made to leave it there.
    fn capture_anchors(&mut self) {
        if self.virtual_lists.is_empty() {
            return;
        }
        let Self { virtual_lists, scroll, snapshot, .. } = self;
        virtual_lists
            .capture_anchors(|id| scroll.offset(id), |list, index| item_at(snapshot, list, index));
    }

    /// Puts each virtual list back where its anchored item is, now that the
    /// new tree is laid out.
    ///
    /// This is what makes an insertion above the viewport grow the list
    /// upward instead of sliding its content: the anchored node keeps its
    /// identity across the render, so the offset follows it to whatever
    /// item index it now occupies.
    fn resolve_anchors(&mut self) {
        let Self { virtual_lists, snapshot, .. } = self;
        let offsets = virtual_lists.resolve_anchors(|node| index_of(snapshot, node));
        for (id, offset) in offsets {
            if self.scroll.scroll_to(id, offset) {
                if let Some(rect) = self.layout.get(&id).copied() {
                    self.scroll.apply(id, &self.registry, rect);
                }
            }
        }
    }

    /// Recomputes every virtual list's visible range from where it is now
    /// scrolled and how big its viewport now is.
    fn update_visible_ranges(&mut self) {
        let Self { virtual_lists, scroll, layout, .. } = self;
        virtual_lists.update_ranges(|id| layout.get(&id).copied(), |id| scroll.offset(id));
    }

    /// Virtual lists that need a different window of items realized, for
    /// the runtime to report to their components.
    ///
    /// Detecting this here is deliberate, for the same reason transitions
    /// are: this is the only place that knows both the scroll offsets and
    /// the laid-out viewports. Dispatching belongs to `native::virtual_list`.
    pub(crate) fn take_range_changes(&mut self) -> Vec<(NodeId, VirtualRange)> {
        self.virtual_lists.take_changes()
    }

    /// Scrolls a container by a wheel/keyboard delta, without rerendering.
    pub(crate) fn scroll_container(&mut self, id: NodeId, delta_x: i32, delta_y: i32) {
        if self.overflow_of(id) != Some(Overflow::Scroll) {
            return;
        }
        if self.scroll.scroll_by(id, delta_x, delta_y) {
            let Some(rect) = self.layout.get(&id).copied() else {
                return;
            };
            self.scroll.apply(id, &self.registry, rect);
            // Scrolling inside a range changes nothing here; crossing out
            // of one queues the change the runtime then reports.
            if !self.virtual_lists.is_empty() {
                self.update_visible_ranges();
            }
        }
    }

    /// Walks up from `hwnd` to the nearest node whose container is
    /// scrollable, which is the one a wheel event over `hwnd` should move.
    pub(crate) fn scrollable_ancestor(&self, hwnd: HWND) -> Option<NodeId> {
        let mut current = hwnd;
        while !current.is_null() {
            if let Some(id) = self.registry.id_for_hwnd(current) {
                if self.overflow_of(id) == Some(Overflow::Scroll) {
                    return Some(id);
                }
            }
            // SAFETY: `current` was just checked non-null and is a live
            // HWND (either the caller's starting handle or a value
            // `GetParent` itself previously returned); a null return
            // (checked at the top of the loop) is the documented
            // "no parent" signal.
            current = unsafe { GetParent(current) };
        }
        None
    }

    /// The nearest scrollable container strictly between `hwnd` and the
    /// node `stop` (walking outward), if any — the one a wheel event over
    /// `hwnd` should scroll instead of reaching `stop`.
    pub(crate) fn scrollable_ancestor_below(&self, hwnd: HWND, stop: NodeId) -> Option<NodeId> {
        let mut current = hwnd;
        while !current.is_null() {
            if let Some(id) = self.registry.id_for_hwnd(current) {
                if id == stop {
                    return None;
                }
                if self.overflow_of(id) == Some(Overflow::Scroll) {
                    return Some(id);
                }
            }
            // SAFETY: `current` was just checked non-null and is a live
            // HWND (the caller's or one `GetParent` returned); a null return
            // ends the walk.
            current = unsafe { GetParent(current) };
        }
        None
    }

    /// Applies a live interaction transition (hover, press, focus) without
    /// rebuilding the declarative tree.
    ///
    /// Native focus and hover are transient interaction state, so a repaint
    /// caused by one must never cause an application render.
    pub(crate) fn set_control_state(&mut self, id: NodeId, state: ControlState) {
        let Some(node) = self.snapshot.get(id).cloned() else {
            return;
        };
        if node.disabled {
            return;
        }
        if self.styles.set_interaction(id, state) {
            self.apply_control_style(&node);
        }
    }

    /// A container node's overflow behavior, or `None` for anything that is
    /// not a container.
    fn overflow_of(&self, id: NodeId) -> Option<Overflow> {
        let node = self.snapshot.get(id)?;
        match node.kind {
            NodeKind::Column => node.column_style.map(|style| style.overflow),
            NodeKind::Row => node.row_style.map(|style| style.overflow),
            NodeKind::Label
            | NodeKind::Button
            | NodeKind::TextInput
            | NodeKind::TabBar
            | NodeKind::Control
            | NodeKind::Canvas
            | NodeKind::Surface => None,
        }
    }

    fn apply_operation(&mut self, operation: &TreeOp, window: HWND) -> Result<(), Error> {
        match operation {
            TreeOp::Insert(node) => self.insert_node(node, window),
            TreeOp::Update(node) => self.update_node(node, window),
            TreeOp::Move { id, parent, .. } => {
                self.reparent(*id, *parent, window);
                Ok(())
            }
            TreeOp::Remove(node) => {
                self.remove_or_park(node, window);
                Ok(())
            }
        }
    }

    fn insert_node(&mut self, node: &TreeNode, window: HWND) -> Result<(), Error> {
        let parent = self.native_parent(node, window);
        if self.reuse_node(node, parent)? {
            return Ok(());
        }
        controls::create(&mut self.registry, node, parent)?;
        // A foreign object's accessibility is its own: no subclass.
        if let Some(object) = self.registry.get(node.id).filter(|_| node.foreign.is_none()) {
            AccessibilityBridge::attach(object.hwnd(), node.kind);
            // Creation applied the text; registered text mappers still run
            // (a replacement overrides what creation wrote).
            crate::mappers::apply(object.hwnd(), node, crate::MappedProperty::Text, (), || {});
        }
        self.apply_semantics_and_style(node);
        Ok(())
    }

    /// Realizes `node` on a native window a removal in this same render
    /// parked, if one of the right kind lived under the same parent.
    ///
    /// The window keeps everything that is a property of the *window* — its
    /// class, its accessibility subclass, its UI Automation provider — and
    /// is given everything that is a property of the *node*: its text, its
    /// semantics, its style. Only the registry's idea of which node it
    /// belongs to changes, which is what makes a recycled row answer as the
    /// row it now is.
    fn reuse_node(&mut self, node: &TreeNode, parent: HWND) -> Result<bool, Error> {
        // A foreign object is one factory's, not interchangeable.
        if node.foreign.is_some() {
            return Ok(false);
        }
        let Some(object) = self.pool.take(parent, node.kind) else {
            return Ok(false);
        };
        if let Err(error) = self.registry.insert(node.id, object) {
            // The id is already taken, which a diff cannot produce; the
            // window is this method's to destroy rather than leak.
            if let Some(object) = self.registry.remove(node.id) {
                object.destroy();
            }
            return Err(error);
        }
        self.apply_semantics_and_style(node);
        if controls::update_text(&self.registry, node)? {
            self.suppress_text_change.insert(node.id);
        }
        Ok(true)
    }

    fn update_node(&mut self, node: &TreeNode, window: HWND) -> Result<(), Error> {
        if controls::needs_replacement(&self.registry, node) {
            self.remove_node(node.id);
            return self.insert_node(node, window);
        }

        self.apply_semantics_and_style(node);
        let Some(hwnd) = self.registry.get(node.id).map(NativeObject::hwnd) else {
            return Ok(());
        };
        let registry = &self.registry;
        let suppressed =
            crate::mappers::apply(hwnd, node, crate::MappedProperty::Text, Ok(false), || {
                controls::update_text(registry, node)
            })?;
        if suppressed {
            self.suppress_text_change.insert(node.id);
        }
        Ok(())
    }

    /// Both post-realization passes every insert and update needs, in the
    /// order they must run: semantics first (which can change window
    /// styles), then appearance (which repaints).
    fn apply_semantics_and_style(&mut self, node: &TreeNode) {
        if let Some(hwnd) = self.registry.get(node.id).map(NativeObject::hwnd) {
            let accessibility = &mut self.accessibility;
            if node.foreign.is_none() {
                crate::mappers::apply(hwnd, node, crate::MappedProperty::Accessibility, (), || {
                    accessibility.apply(hwnd, node);
                });
            }
            crate::mappers::apply(hwnd, node, crate::MappedProperty::Visibility, (), || {
                set_visible(hwnd, !node.hidden);
            });
            crate::mappers::apply(hwnd, node, crate::MappedProperty::Style, (), || {
                self.apply_control_style(node);
            });
        } else {
            self.apply_control_style(node);
        }
        self.apply_opacity(node.id);
    }

    /// Removes `id`'s native object, destroying it.
    fn remove_node(&mut self, id: NodeId) {
        self.forget_node(id);
        if let Some(object) = self.registry.remove(id) {
            self.accessibility.forget(object.hwnd());
            object.destroy();
        }
    }

    /// Removes `node`, parking its native object for reuse when `node` is
    /// part of a virtual list and destroying it otherwise.
    ///
    /// A parked window keeps its accessibility subclass and provider, so
    /// this deliberately does not `forget` them: the same window is about
    /// to answer for a different row, and tearing that down only to build
    /// it again is the churn recycling exists to avoid.
    fn remove_or_park(&mut self, node: &TreeNode, window: HWND) {
        if !self.is_virtual_item(node) || node.foreign.is_some() {
            self.remove_node(node.id);
            return;
        }
        let parent = self.native_parent(node, window);
        self.forget_node(node.id);
        if let Some(object) = self.registry.remove(node.id) {
            self.pool.put(parent, node.kind, object);
        }
    }

    /// Everything the renderer holds about a node that is leaving,
    /// independent of what happens to its native window.
    fn forget_node(&mut self, id: NodeId) {
        self.animated.forget(id);
        self.removed_nodes.push(id);
        self.styles.forget(id);
        self.suppress_text_change.remove(&id);
        self.layout.remove(&id);
        self.positioned.remove(&id);
    }

    /// Whether `node` is inside a virtual list, and so interchangeable with
    /// the rows around it.
    fn is_virtual_item(&self, node: &TreeNode) -> bool {
        let mut parent = node.parent;
        while let Some(id) = parent {
            let Some(ancestor) = self.snapshot.get(id) else {
                return false;
            };
            if ancestor.virtualization.is_some() {
                return true;
            }
            parent = ancestor.parent;
        }
        false
    }

    fn reparent(&mut self, id: NodeId, parent: Option<NodeId>, window: HWND) {
        let Some(object) = self.registry.get(id) else {
            return;
        };
        let parent_hwnd = self.parent_hwnd(parent, window);
        // SAFETY: both handles are live native windows owned by this
        // renderer's registry (or the top-level window it renders into).
        //
        // `SetParent` returns the *previous* parent handle, not a status.
        informational(unsafe { SetParent(object.hwnd(), parent_hwnd) });
    }

    /// Realizes `node`'s resolved `visual_style` (font, foreground, and
    /// background) on its native object, and its `disabled` flag as a
    /// native `EnableWindow` toggle. Called after both insertion and
    /// in-place update, since either can change a node's style.
    ///
    /// Labels, buttons, and text inputs are repainted through
    /// `WM_CTLCOLOR*`, handled by whichever ancestor window owns the style
    /// cache (see `message_loop::control_color`). Containers paint their own
    /// background directly through `WM_ERASEBKGND`; since that handler
    /// cannot reach this `Renderer`, the resolved background color is
    /// additionally cached on the container's own `GWLP_USERDATA`.
    fn apply_control_style(&mut self, node: &TreeNode) {
        let Some(object) = self.registry.get(node.id) else {
            return;
        };
        let hwnd = object.hwnd();
        let content_hwnd = object.content_hwnd();

        let style_override = self.animated_override(node);
        let style = self.styles.realize(node.id, node.kind, &style_override, node.disabled);
        let font = style.font;
        let background = style.background;
        let (border, radius) = (style.border, style.radius);

        if !font.is_null() {
            // SAFETY: `hwnd`/`content_hwnd` are live HWNDs owned by this
            // renderer's registry; `font` is a valid `HFONT` kept alive in
            // the style cache for at least as long as any control can still
            // reference it (until the next realization or node removal
            // replaces/frees it); `WM_SETFONT`'s `lParam` of `1` requests
            // an immediate redraw, a documented valid value.
            //
            // `WM_SETFONT` is documented to return no meaningful value.
            unsafe {
                informational(SendMessageW(hwnd, WM_SETFONT, font as WPARAM, 1));
                if let Some(content_hwnd) = content_hwnd {
                    informational(SendMessageW(content_hwnd, WM_SETFONT, font as WPARAM, 1));
                }
            }
        }

        match node.kind {
            // This container-only slot is reserved for the resolved
            // background `COLORREF` (read back by the `WM_ERASEBKGND`
            // handler in `container_proc`, through
            // `styling::background_brush_for`), never aliased with the
            // `Runtime` pointer stored there for top-level windows — see
            // `user_data`'s module docs.
            NodeKind::Column | NodeKind::Row => {
                super::shape::set_shape(hwnd, border, radius);
                BackgroundColorSlot::set(hwnd, background);
                if let Some(content_hwnd) = content_hwnd {
                    BackgroundColorSlot::set(content_hwnd, background);
                }
            }
            // A canvas clears to its node's background before drawing, so
            // an unstyled canvas matches the container it sits in.
            NodeKind::Canvas => crate::native::graphics::canvas::set_background(
                hwnd,
                Some(colorref_to_color(background)),
            ),
            NodeKind::Label
            | NodeKind::Button
            | NodeKind::TextInput
            | NodeKind::TabBar
            | NodeKind::Control
            | NodeKind::Surface => {}
        }

        // SAFETY: `hwnd` is a live HWND owned by this renderer's registry;
        // a null `lpRect` is `InvalidateRect`'s documented way to
        // invalidate the entire client area.
        unsafe {
            // `EnableWindow` returns the window's *previous* disabled state.
            ignored_by_contract(EnableWindow(hwnd, i32::from(!node.disabled)));
            let invalidated = InvalidateRect(hwnd, std::ptr::null(), 1) != 0;
            // Best effort: a missed invalidation means the control keeps
            // its current pixels until something else repaints it.
            best_effort(invalidated, "InvalidateRect", "the control repaints on the next paint");
        }
    }

    fn position_node(&mut self, id: NodeId) {
        let Some(object) = self.registry.get(id) else {
            return;
        };
        let Some(rect) = self.layout.get(&id).copied() else {
            return;
        };
        // Whatever is animating this node wins over the laid-out geometry
        // until its animation ends (see `rendering::animated`).
        let rect = self.animated.rect_for(id, rect);
        let placed = (object.hwnd() as isize, rect);
        if self.positioned.insert(id, placed) == Some(placed) {
            if object.content_hwnd().is_some() {
                self.scroll.apply(id, &self.registry, rect);
            }
            return;
        }

        // The layout engine always returns rectangles relative to the native
        // parent. Scrolling is deliberately NOT part of those rectangles;
        // see `super::scrolling`.
        // SAFETY: `object.hwnd()` is a live HWND owned by this renderer's
        // registry; a null `hWndInsertAfter` with `SWP_NOZORDER` leaves
        // z-order untouched, per its documented contract.
        let moved = unsafe {
            SetWindowPos(
                object.hwnd(),
                std::ptr::null_mut(),
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        } != 0;
        // Best effort: the control keeps its previous geometry until the
        // next layout pass, and there is no caller that could act on a
        // single control failing to move.
        best_effort(moved, "SetWindowPos(layout)", "the control keeps its previous geometry");
        if let super::super::registry::NativeObject::Control {
            hwnd,
            companion: Some(companion),
            ..
        } = object
        {
            super::native_controls::realign(*hwnd, *companion);
        }

        if object.content_hwnd().is_some() {
            self.scroll.apply(id, &self.registry, rect);
        }
    }

    /// A transition this renderer found: a property of a node that a
    /// render changed, and which the node asked to animate.
    ///
    /// Detecting it here is deliberate — this is the only place that knows
    /// both what was on screen and what the new tree says — while starting
    /// it belongs to `native::animation`, which owns the timeline.
    pub(crate) fn take_transitions(&mut self) -> Vec<TransitionRequest> {
        std::mem::take(&mut self.pending_transitions)
    }

    /// Nodes removed since this was last called, whose animations are over
    /// because the node is.
    pub(crate) fn forgotten_nodes(&mut self) -> Vec<NodeId> {
        std::mem::take(&mut self.removed_nodes)
    }

    /// A property's value right now: what is animating, or what the
    /// rendered tree says. This is where an explicit animation starts from
    /// when it does not say.
    pub(crate) fn current_value(
        &self,
        node: NodeId,
        property: AnimatedProperty,
    ) -> Option<AnimatedValue> {
        let overrides = self.animated.get(node);
        let rendered = self.snapshot.get(node)?;
        Some(match property {
            AnimatedProperty::Position => {
                let rect = self.layout.get(&node).copied()?;
                AnimatedValue::Offset(overrides.position.unwrap_or(Point::new(rect.x, rect.y)))
            }
            AnimatedProperty::Size => {
                let rect = self.layout.get(&node).copied()?;
                AnimatedValue::Size(overrides.size.unwrap_or(size_of(rect)))
            }
            AnimatedProperty::Translation => {
                AnimatedValue::Offset(overrides.translation.unwrap_or(Point::new(0, 0)))
            }
            AnimatedProperty::Opacity => {
                AnimatedValue::Scalar(overrides.opacity.unwrap_or(rendered.opacity))
            }
            AnimatedProperty::Background => AnimatedValue::Color(
                overrides
                    .background
                    .or(rendered.visual_style.properties().background_override())?,
            ),
            AnimatedProperty::Foreground => AnimatedValue::Color(
                overrides
                    .foreground
                    .or(rendered.visual_style.properties().foreground_override())?,
            ),
            // A property this backend does not realize yet: nothing to
            // animate from, so the animation starts at its declared value.
            _ => return None,
        })
    }

    /// Applies one frame to its native object, if anything actually
    /// changed.
    pub(crate) fn apply_animation_frame(&mut self, frame: &Frame) {
        if !self.animated.set(frame.node, frame.property, frame.value) {
            return;
        }
        match frame.property {
            AnimatedProperty::Position | AnimatedProperty::Size | AnimatedProperty::Translation => {
                self.position_node(frame.node);
            }
            AnimatedProperty::Opacity => self.apply_opacity(frame.node),
            AnimatedProperty::Background | AnimatedProperty::Foreground => {
                if let Some(node) = self.snapshot.get(frame.node).cloned() {
                    self.apply_control_style(&node);
                }
            }
            // As in `current_value`: a property this backend does not
            // realize is not applied rather than applied wrongly.
            _ => {}
        }
    }

    /// Realizes a node's opacity — its own, or whatever is animating it —
    /// as a layered window.
    pub(crate) fn apply_opacity(&self, id: NodeId) {
        let Some(object) = self.registry.get(id) else {
            return;
        };
        let declared = self.snapshot.get(id).map_or(1.0, |node| node.opacity.get());
        let opacity = self.animated.get(id).opacity.map_or(declared, Scalar::get);
        super::styling::set_opacity(object.hwnd(), opacity);
    }

    /// Finds the appearance transitions a render started: a node that
    /// declared one, whose property the new tree changed.
    ///
    /// Collected into a local list first because the comparison borrows the
    /// *old* snapshot while the result belongs to this renderer.
    fn collect_appearance_transitions(&mut self, next: &TreeSnapshot) {
        let mut found = Vec::new();
        for node in next.nodes() {
            if node.transitions.is_empty() {
                continue;
            }
            let Some(previous) = self.snapshot.get(node.id) else {
                // A node appearing does not transition: there is nothing on
                // screen to move from.
                continue;
            };
            let overrides = self.animated.get(node.id);
            for declared in &node.transitions {
                let (from, to) = match declared.property {
                    AnimatedProperty::Opacity => (
                        AnimatedValue::Scalar(overrides.opacity.unwrap_or(previous.opacity)),
                        AnimatedValue::Scalar(node.opacity),
                    ),
                    AnimatedProperty::Background => {
                        let before = overrides
                            .background
                            .or(previous.visual_style.properties().background_override());
                        let after = node.visual_style.properties().background_override();
                        match (before, after) {
                            (Some(before), Some(after)) => {
                                (AnimatedValue::Color(before), AnimatedValue::Color(after))
                            }
                            // Nothing resolved a color for this node, so
                            // there is nothing to interpolate between.
                            _ => continue,
                        }
                    }
                    AnimatedProperty::Foreground => {
                        let before = overrides
                            .foreground
                            .or(previous.visual_style.properties().foreground_override());
                        let after = node.visual_style.properties().foreground_override();
                        match (before, after) {
                            (Some(before), Some(after)) => {
                                (AnimatedValue::Color(before), AnimatedValue::Color(after))
                            }
                            _ => continue,
                        }
                    }
                    // Geometry transitions are found by the layout pass,
                    // which is the only place that knows the rectangles.
                    _ => continue,
                };
                if from != to {
                    found.push(TransitionRequest {
                        node: node.id,
                        property: declared.property,
                        from,
                        to,
                        transition: declared.transition,
                    });
                }
            }
        }
        for request in &found {
            self.pin_transition_start(request);
        }
        self.pending_transitions.extend(found);
    }

    /// Holds a transitioned property at the value it is animating *from*,
    /// so applying the new tree does not make it jump before the first
    /// frame arrives. The timeline takes over from here, and clears the
    /// override when it finishes.
    fn pin_transition_start(&mut self, request: &TransitionRequest) {
        self.animated.set(request.node, request.property, Some(request.from));
    }

    fn collect_geometry_transitions(&mut self, next: &HashMap<NodeId, Rect>) {
        let mut geometry = Vec::new();
        for node in self.snapshot.nodes() {
            if node.transitions.is_empty() {
                continue;
            }
            let (Some(previous), Some(new)) =
                (self.layout.get(&node.id).copied(), next.get(&node.id).copied())
            else {
                continue;
            };
            let overrides = self.animated.get(node.id);
            for declared in &node.transitions {
                let (from, to) = match declared.property {
                    AnimatedProperty::Position => (
                        AnimatedValue::Offset(
                            overrides.position.unwrap_or(Point::new(previous.x, previous.y)),
                        ),
                        AnimatedValue::Offset(Point::new(new.x, new.y)),
                    ),
                    AnimatedProperty::Size => (
                        AnimatedValue::Size(overrides.size.unwrap_or(size_of(previous))),
                        AnimatedValue::Size(size_of(new)),
                    ),
                    _ => continue,
                };
                geometry.push(TransitionRequest {
                    node: node.id,
                    property: declared.property,
                    from,
                    to,
                    transition: declared.transition,
                });
            }
        }
        // Matched geometry: Windows has no shared-element transition of its
        // own, so the arriving node's position and size animate from the
        // leaving node's rectangle like any other geometry transition.
        for matched in std::mem::take(&mut self.matched) {
            let Some(new) = next.get(&matched.node).copied() else { continue };
            let from = matched.from;
            geometry.push(TransitionRequest {
                node: matched.node,
                property: AnimatedProperty::Position,
                from: AnimatedValue::Offset(Point::new(from.x, from.y)),
                to: AnimatedValue::Offset(Point::new(new.x, new.y)),
                transition: matched.transition,
            });
            geometry.push(TransitionRequest {
                node: matched.node,
                property: AnimatedProperty::Size,
                from: AnimatedValue::Size(size_of(from)),
                to: AnimatedValue::Size(size_of(new)),
                transition: matched.transition,
            });
        }
        for request in &geometry {
            self.pin_transition_start(request);
        }
        self.pending_transitions.extend(geometry);
    }

    /// The node's style override with any animated colors applied, which is
    /// what a frame of a color transition changes.
    fn animated_override(&self, node: &TreeNode) -> StyleOverride {
        let overrides = self.animated.get(node.id);
        if overrides.background.is_none() && overrides.foreground.is_none() {
            return node.style_override.clone();
        }
        let mut style: VisualStyle = node.style_override.properties().clone();
        if let Some(background) = overrides.background {
            style = style.background(background);
        }
        if let Some(foreground) = overrides.foreground {
            style = style.foreground(foreground);
        }
        StyleOverride::new(style).with_states(node.style_override.states().clone())
    }

    /// The native window a node's control should be created inside: its
    /// parent's *content* host if the parent is a scrollable container,
    /// otherwise the parent's own window, falling back to the top-level
    /// window for a root node.
    fn native_parent(&self, node: &TreeNode, window: HWND) -> HWND {
        self.parent_hwnd(node.parent, window)
    }

    fn parent_hwnd(&self, parent: Option<NodeId>, window: HWND) -> HWND {
        parent
            .and_then(|parent_id| self.registry.get(parent_id))
            .map_or(window, |object| object.content_hwnd().unwrap_or_else(|| object.hwnd()))
    }
}

/// Shows or hides a node's native window.
///
/// Hiding the window hides everything inside it with no further calls —
/// Win32 does not draw a child of an invisible window — which is why a
/// hidden node's descendants need no flag of their own.
///
/// Decided by the window's *own* `WS_VISIBLE` bit, not `IsWindowVisible`,
/// which also answers "no" for every child of a window not yet shown — as
/// every window is during its first render, which is exactly when a hidden
/// screen most needs hiding.
fn set_visible(hwnd: HWND, visible: bool) {
    // SAFETY: `hwnd` is a live window owned by the renderer's registry;
    // `GWL_STYLE` is a documented index.
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };
    #[allow(clippy::cast_possible_wrap, reason = "a single fixed style bit")]
    let already = style & (WS_VISIBLE as isize) != 0;
    if already == visible {
        return;
    }
    // SAFETY: as above; `ShowWindow` returns the previous visibility, not
    // a status.
    informational(unsafe { ShowWindow(hwnd, if visible { SW_SHOWNA } else { SW_HIDE }) });
}

/// A native surface's new size, waiting to be reported to its component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceChange {
    pub(crate) node: NodeId,
    pub(crate) surface: rustnative_core::SurfaceId,
    pub(crate) size: Size,
    pub(crate) scale_factor: Scalar,
}

/// A GDI `COLORREF` (`0x00BBGGRR`) as a portable color.
fn colorref_to_color(color: u32) -> rustnative_core::Color {
    let [red, green, blue, _] = color.to_le_bytes();
    rustnative_core::Color::rgb(red, green, blue)
}

/// The node realizing item `index` of the virtual list `list`, if it is
/// realized at all.
fn item_at(snapshot: &TreeSnapshot, list: NodeId, index: usize) -> Option<NodeId> {
    snapshot
        .children_of(list)
        .find(|child| child.item_index.unwrap_or(child.index) == index)
        .map(|child| child.id)
}

/// Which item of its virtual list `node` realizes.
fn index_of(snapshot: &TreeSnapshot, node: NodeId) -> Option<usize> {
    snapshot.get(node).map(|node| node.item_index.unwrap_or(node.index))
}

/// A transition the renderer found and `native::animation` will start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TransitionRequest {
    /// The node to animate.
    pub(crate) node: NodeId,
    /// Which property moved.
    pub(crate) property: AnimatedProperty,
    /// What it is showing now.
    pub(crate) from: AnimatedValue,
    /// What the new tree says it should be.
    pub(crate) to: AnimatedValue,
    /// How the node asked it to move.
    pub(crate) transition: Transition,
}

/// A rectangle's size as the animatable value.
fn size_of(rect: Rect) -> Size {
    Size::new(dimension_to_u32(rect.width), dimension_to_u32(rect.height))
}
