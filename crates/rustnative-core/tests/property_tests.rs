//! Property-based tests (standards audit P2.34).
//!
//! Unlike the example-based unit/integration tests elsewhere in this crate
//! (which pin down specific, hand-picked scenarios — including
//! deliberately extreme ones, e.g. `layout::engine`'s
//! `extreme_constraints_do_not_panic_or_produce_negative_geometry`), these
//! tests generate hundreds of *randomized* inputs per run and check that a
//! small set of invariants holds for all of them. That is a meaningfully
//! different kind of coverage: it catches interaction bugs between inputs
//! no one thought to hand-write a test for, at the cost of being slower and
//! less specific about *which* input failed (`proptest` prints a minimized
//! failing case when one is found, which mitigates this).

use std::collections::HashSet;

use proptest::prelude::*;

use rustnative_core::{
    Alignment, ColumnStyle, Constraints, DefaultIntrinsicMeasurer, LayoutEngine, LayoutStyle, Node,
    NodeId, RowStyle, Size, SizeMode, TreeDiff, TreeOp, TreeSnapshot,
};

// ---------------------------------------------------------------------
// Identity: distinct keys never collide (crate::identity, P0.1).
// ---------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// For any set of distinct key strings, [`NodeId::from_key`] must never
    /// map two of them to the same id. This is the property the interning
    /// design in `crate::identity` exists to make *structurally*
    /// impossible (see that module's docs) rather than merely unlikely;
    /// this test exercises it against proptest-generated input rather than
    /// only the hand-picked sequential keys in `identity`'s own unit test.
    #[test]
    fn distinct_keys_never_collide(keys in proptest::collection::hash_set("[a-zA-Z0-9_-]{1,16}", 1..64)) {
        let mut seen = HashSet::new();
        for key in &keys {
            let id = NodeId::from_key(key);
            prop_assert!(seen.insert(id), "key {key:?} collided with a previously interned key");
        }
    }
}

// ---------------------------------------------------------------------
// Reconciliation: a diff's operations, replayed against a plain model,
// always reconstruct exactly the target snapshot's node set
// (crate::reconcile, P1.11/P1.12).
// ---------------------------------------------------------------------

/// A small, bounded vocabulary of keys so generated trees produce a
/// meaningful number of structural overlaps (shared keys, reordering, ...)
/// between "previous" and "next" instead of almost always being two
/// entirely disjoint trees.
fn small_key() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("a".to_string()),
        Just("b".to_string()),
        Just("c".to_string()),
        Just("d".to_string()),
    ]
}

fn small_text() -> impl Strategy<Value = String> {
    prop_oneof![Just("x".to_string()), Just("y".to_string()), Just("xy".to_string())]
}

fn arbitrary_tree() -> impl Strategy<Value = Node> {
    let leaf = (small_key(), small_text()).prop_map(|(key, text)| Node::label(key, text));
    leaf.prop_recursive(3, 16, 4, |inner| {
        proptest::collection::vec(inner, 0..4).prop_map(|children| Node::column("root", children))
    })
}

fn node_ids(node: &Node) -> HashSet<NodeId> {
    let mut ids = HashSet::new();
    node.visit(&mut |n, _, _| {
        ids.insert(n.id());
    });
    ids
}

/// Replays `diff` against a plain `HashSet<NodeId>` model seeded with
/// `previous`'s node ids, exactly as a minimal backend would track "which
/// native objects currently exist".
fn apply_diff_to_model(model: &mut HashSet<NodeId>, diff: &TreeDiff) {
    for op in diff.operations() {
        match op {
            TreeOp::Insert(node) => {
                model.insert(node.id);
            }
            TreeOp::Remove(node) => {
                model.remove(&node.id);
            }
            TreeOp::Update(_) | TreeOp::Move { .. } => {
                // Neither operation changes *which* ids exist, only their
                // data or position — nothing for a pure existence model to
                // do.
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn diff_operations_reconstruct_exactly_the_next_node_set(
        previous_tree in arbitrary_tree(),
        next_tree in arbitrary_tree(),
    ) {
        // Skip the (rare, since keys are drawn from a 4-symbol vocabulary
        // shared across siblings) inputs where a single tree reuses the
        // same local key twice — that is a distinct, separately-tested
        // condition (`node::tests::duplicate_local_keys_are_detected`), not
        // what this property is about.
        let Ok(previous) = TreeSnapshot::from_node(&previous_tree) else { return Ok(()) };
        let Ok(next) = TreeSnapshot::from_node(&next_tree) else { return Ok(()) };

        let mut model: HashSet<NodeId> = node_ids(&previous_tree);
        let diff = TreeDiff::between(&previous, &next);
        apply_diff_to_model(&mut model, &diff);

        prop_assert_eq!(model, node_ids(&next_tree));
    }

    /// Every `Update`/`Move` operation's node id must already have existed
    /// in the previous snapshot (an `Update`/`Move` for a node the backend
    /// never created would be a use-after-free-shaped bug in any real
    /// backend), and every `Remove`'s node id must not appear in `next`.
    #[test]
    fn diff_operations_target_only_nodes_that_could_legitimately_exist(
        previous_tree in arbitrary_tree(),
        next_tree in arbitrary_tree(),
    ) {
        let Ok(previous) = TreeSnapshot::from_node(&previous_tree) else { return Ok(()) };
        let Ok(next) = TreeSnapshot::from_node(&next_tree) else { return Ok(()) };
        let previous_ids = node_ids(&previous_tree);
        let next_ids = node_ids(&next_tree);

        let diff = TreeDiff::between(&previous, &next);
        for op in diff.operations() {
            match op {
                TreeOp::Update(node) => {
                    prop_assert!(previous_ids.contains(&node.id));
                }
                TreeOp::Move { id, .. } => {
                    prop_assert!(previous_ids.contains(id));
                }
                TreeOp::Remove(node) => {
                    prop_assert!(!next_ids.contains(&node.id));
                }
                TreeOp::Insert(_) => {}
            }
        }
    }
}

// ---------------------------------------------------------------------
// Layout: geometry is always non-negative and finite, across a wide
// randomized parameter space (crate::layout, P1.19).
// ---------------------------------------------------------------------

fn size_mode() -> impl Strategy<Value = SizeMode> {
    prop_oneof![Just(SizeMode::Auto), Just(SizeMode::Fill), (0i32..2000).prop_map(SizeMode::Fixed)]
}

fn layout_style() -> impl Strategy<Value = LayoutStyle> {
    (size_mode(), size_mode(), 0i32..64, 0i32..64).prop_map(|(width, height, min_w, min_h)| {
        LayoutStyle::new()
            .width(width)
            .height(height)
            .constraints(Constraints::new().with_min_width(min_w).with_min_height(min_h))
    })
}

fn leaf_with_layout() -> impl Strategy<Value = Node> {
    (small_key(), small_text(), layout_style())
        .prop_map(|(key, text, layout)| Node::label_with_layout(key, text, layout))
}

fn arbitrary_layout_tree() -> impl Strategy<Value = Node> {
    leaf_with_layout().prop_recursive(3, 20, 5, |inner| {
        (
            proptest::collection::vec(inner, 0..5),
            0i32..32,
            prop_oneof![
                Just(Alignment::Start),
                Just(Alignment::Center),
                Just(Alignment::End),
                Just(Alignment::Stretch),
            ],
            any::<bool>(),
        )
            .prop_map(|(children, gap, align, is_row)| {
                if is_row {
                    Node::row_with_layout(
                        "container",
                        children,
                        LayoutStyle::new(),
                        RowStyle::new().gap(gap).align_items(align),
                    )
                } else {
                    Node::column_with_layout(
                        "container",
                        children,
                        LayoutStyle::new(),
                        ColumnStyle::new().gap(gap).align_items(align),
                    )
                }
            })
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn layout_geometry_is_always_non_negative(
        tree in arbitrary_layout_tree(),
        width in 0u32..4000,
        height in 0u32..4000,
    ) {
        let Ok(snapshot) = TreeSnapshot::from_node(&tree) else { return Ok(()) };
        let engine = LayoutEngine::new();
        let rects = engine.layout(&snapshot, Size::new(width, height));
        for rect in rects.values() {
            prop_assert!(rect.width >= 0, "negative width: {rect:?}");
            prop_assert!(rect.height >= 0, "negative height: {rect:?}");
        }
    }
}

// ---------------------------------------------------------------------
// Layout: declared constraints are respected, and scroll ranges only exist
// where content actually overflows (P2.34's remaining named properties).
// ---------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// A node's resolved rectangle must honor the minimum and maximum
    /// bounds its `Constraints` declare.
    ///
    /// This is a different claim from `layout_geometry_is_always_non_negative`
    /// above: that one says the engine never produces nonsense, this one
    /// says it produces the *specific* geometry the caller asked for. A
    /// clamp applied in one branch of the engine but not another — the
    /// realistic way this breaks, given how many branches
    /// `column`/`row`/`Fill`/`Fixed`/`Auto` multiply into — passes the first
    /// property and fails this one.
    ///
    /// The maximum is checked against the constraint itself, not against the
    /// available space: a node whose minimum exceeds the viewport is
    /// legitimately allowed to overflow it (that is what a scrollable
    /// container is for), so asserting containment would assert the wrong
    /// thing.
    #[test]
    fn layout_respects_declared_constraints(
        min_width in 0i32..500,
        extra_width in 0i32..500,
        min_height in 0i32..500,
        extra_height in 0i32..500,
        viewport_width in 0u32..2000,
        viewport_height in 0u32..2000,
    ) {
        let max_width = min_width + extra_width;
        let max_height = min_height + extra_height;
        let constraints = Constraints::new()
            .with_min_width(min_width)
            .with_max_width(max_width)
            .with_min_height(min_height)
            .with_max_height(max_height);
        let tree = Node::column(
            "root",
            [Node::label_with_layout(
                "item",
                "text",
                LayoutStyle::new()
                    .width(SizeMode::Fill)
                    .height(SizeMode::Fill)
                    .constraints(constraints),
            )],
        );

        let Ok(snapshot) = TreeSnapshot::from_node(&tree) else { return Ok(()) };
        let rects =
            LayoutEngine::new().layout(&snapshot, Size::new(viewport_width, viewport_height));
        let Some(rect) = rects.get(&NodeId::from_key("item")) else { return Ok(()) };

        prop_assert!(
            rect.width >= min_width,
            "width {} is below the declared minimum {}",
            rect.width,
            min_width
        );
        prop_assert!(
            rect.width <= max_width,
            "width {} exceeds the declared maximum {}",
            rect.width,
            max_width
        );
        prop_assert!(
            rect.height >= min_height,
            "height {} is below the declared minimum {}",
            rect.height,
            min_height
        );
        prop_assert!(
            rect.height <= max_height,
            "height {} exceeds the declared maximum {}",
            rect.height,
            max_height
        );
    }

    /// A node only reports a scroll range where its content genuinely
    /// overflows its own rectangle.
    ///
    /// A backend clamps a scroll offset into `0..=range` (see
    /// `rustnative-windows`'s `native::rendering::scrolling`). A range
    /// invented for a node whose content fits would let the viewport scroll
    /// past its own content into blank space, and a range derived from
    /// arithmetic that underflowed would produce an inverted clamp range —
    /// which `i32::clamp` panics on rather than silently mishandling,
    /// turning a layout slip into a crash on an ordinary mouse wheel.
    #[test]
    fn scroll_ranges_exist_only_where_content_overflows(
        tree in arbitrary_layout_tree(),
        width in 0u32..2000,
        height in 0u32..2000,
    ) {
        let Ok(snapshot) = TreeSnapshot::from_node(&tree) else { return Ok(()) };
        let output = LayoutEngine::new().layout_result_with(
            &snapshot,
            Size::new(width, height),
            &DefaultIntrinsicMeasurer,
            &std::collections::HashMap::new(),
        );
        for (id, range) in &output.scroll_ranges {
            let content = output.content_sizes.get(id).copied().unwrap_or(Size::new(0, 0));
            // A scroll range for a node with no rectangle would itself be
            // a bug, but this property is about the range/content
            // relationship; treat a missing rectangle as zero-sized so the
            // assertion below still says something rather than skipping.
            let (rect_width, rect_height) = output
                .rects
                .get(id)
                .map_or((0, 0), |rect| {
                    (
                        u32::try_from(rect.width.max(0)).unwrap_or(0),
                        u32::try_from(rect.height.max(0)).unwrap_or(0),
                    )
                });
            prop_assert!(
                range.width == 0 || content.width > rect_width,
                "node {:?} reports a horizontal scroll range of {} with content {} in a \
                 {}-wide rectangle",
                id,
                range.width,
                content.width,
                rect_width
            );
            prop_assert!(
                range.height == 0 || content.height > rect_height,
                "node {:?} reports a vertical scroll range of {} with content {} in a \
                 {}-tall rectangle",
                id,
                range.height,
                content.height,
                rect_height
            );
        }
    }
}

// ---------------------------------------------------------------------
// Reconciliation: identity stability and removal ordering (P2.34).
// ---------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Snapshotting the same tree twice produces the same identities, and
    /// diffing the two produces no work at all.
    ///
    /// "Identity survives rerender" is the assumption every other part of
    /// reconciliation rests on. If a key resolved to a different id on a
    /// second render, every node would look inserted-and-removed, and a
    /// backend would tear down and rebuild its entire native tree on every
    /// frame — while still appearing to work, which is what makes this worth
    /// asserting rather than trusting.
    #[test]
    fn identity_survives_a_rerender_and_produces_an_empty_diff(tree in arbitrary_tree()) {
        let Ok(first) = TreeSnapshot::from_node(&tree) else { return Ok(()) };
        let Ok(second) = TreeSnapshot::from_node(&tree) else { return Ok(()) };
        prop_assert_eq!(node_ids(&tree), node_ids(&tree));

        let diff = TreeDiff::between(&first, &second);
        prop_assert!(
            diff.operations().is_empty(),
            "an unchanged tree must produce no operations, got {:?}",
            diff.operations()
        );
        prop_assert!(!diff.invalidates_layout(), "an unchanged tree must not invalidate layout");
    }

    /// Removals are emitted deepest-first, and every removal precedes every
    /// insertion.
    ///
    /// A backend destroys native objects in the order it receives them.
    /// Destroying a parent before its child means the child's handle is
    /// already dead when its own `Remove` arrives — on Win32 that is a
    /// `DestroyWindow` against a stale `HWND`, exactly the use-after-free
    /// this ordering exists to prevent. The insert side is the mirror: a
    /// child created before its parent has nowhere to be created.
    #[test]
    fn removals_are_ordered_deepest_first_and_precede_insertions(
        previous in arbitrary_tree(),
        next in arbitrary_tree(),
    ) {
        let Ok(previous_snapshot) = TreeSnapshot::from_node(&previous) else { return Ok(()) };
        let Ok(next_snapshot) = TreeSnapshot::from_node(&next) else { return Ok(()) };
        let diff = TreeDiff::between(&previous_snapshot, &next_snapshot);

        let mut removed: HashSet<NodeId> = HashSet::new();
        let mut seen_insert = false;
        for operation in diff.operations() {
            match operation {
                TreeOp::Remove(node) => {
                    prop_assert!(!seen_insert, "removal of {:?} came after an insertion", node.id);
                    if let Some(parent) = node.parent {
                        prop_assert!(
                            !removed.contains(&parent),
                            "node {:?} was removed after its own parent {:?}",
                            node.id,
                            parent
                        );
                    }
                    removed.insert(node.id);
                }
                TreeOp::Insert(node) => {
                    seen_insert = true;
                    if let Some(parent) = node.parent {
                        // A parent that is not itself being inserted must
                        // already exist in the previous tree.
                        let parent_inserted = diff.operations().iter().any(|other| {
                            matches!(other, TreeOp::Insert(other) if other.id == parent)
                        });
                        prop_assert!(
                            parent_inserted || previous_snapshot.contains(parent),
                            "node {:?} is inserted under a parent {:?} that neither exists nor \
                             is being created",
                            node.id,
                            parent
                        );
                    }
                }
                TreeOp::Update(_) | TreeOp::Move { .. } => {}
            }
        }
    }
}

// ---------------------------------------------------------------------
// Gesture recognition (crate::input, Milestone 25).
// ---------------------------------------------------------------------

mod gestures {
    use std::time::Duration;

    use proptest::prelude::*;

    use rustnative_core::{
        Gesture, GesturePhase, GestureRecognizer, Point, PointerEvent, PointerKind, PointerPhase,
    };

    fn phase_strategy() -> impl Strategy<Value = PointerPhase> {
        prop_oneof![
            Just(PointerPhase::Down),
            Just(PointerPhase::Move),
            Just(PointerPhase::Up),
            Just(PointerPhase::Cancel),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        /// Any interleaving of up to three contacts, including physically
        /// impossible ones (an up with no down, a down twice), must never
        /// panic, and once every contact has lifted the recognizer must be
        /// idle again — a stuck recognizer would swallow every later
        /// gesture on that node.
        #[test]
        fn arbitrary_pointer_streams_never_panic_and_always_settle(
            steps in proptest::collection::vec(
                (0u32..3, phase_strategy(), -500i32..500, -500i32..500, 0u64..50),
                0..80,
            )
        ) {
            let mut recognizer = GestureRecognizer::default();
            let mut now = 0u64;
            for (id, phase, x, y, dt) in steps {
                now += dt;
                let sample = PointerEvent::new(
                    id, PointerKind::Touch, Point::new(x, y), Duration::from_millis(now),
                );
                let _ = recognizer.handle(phase, &sample);
                let _ = recognizer.tick(Duration::from_millis(now));
            }
            for id in 0..3 {
                let sample = PointerEvent::new(
                    id, PointerKind::Touch, Point::new(0, 0), Duration::from_millis(now),
                );
                let _ = recognizer.handle(PointerPhase::Up, &sample);
            }
            prop_assert!(!recognizer.is_active());
            prop_assert_eq!(recognizer.next_deadline(), None);
        }

        /// For a single-contact drag, the per-event pan deltas always sum
        /// to the reported total — so a component accumulating deltas and
        /// one reading totals agree on where the content is.
        #[test]
        fn single_contact_pan_deltas_sum_to_the_total(
            path in proptest::collection::vec((-300i32..300, -300i32..300), 1..40)
        ) {
            let mut recognizer = GestureRecognizer::default();
            let at = |x, y, ms| {
                PointerEvent::new(0, PointerKind::Mouse, Point::new(x, y), Duration::from_millis(ms))
            };
            recognizer.handle(PointerPhase::Down, &at(0, 0, 0));
            let mut sum = Point::new(0, 0);
            let mut total = Point::new(0, 0);
            let mut observe = |gestures: Vec<Gesture>| {
                for gesture in gestures {
                    if let Gesture::Pan { delta, total: t, .. } = gesture {
                        sum = Point::new(sum.x + delta.x, sum.y + delta.y);
                        total = t;
                    }
                }
            };
            for (step, (x, y)) in path.iter().enumerate() {
                observe(recognizer.handle(PointerPhase::Move, &at(*x, *y, step as u64 + 1)));
            }
            let (x, y) = *path.last().unwrap();
            let ended = recognizer.handle(PointerPhase::Up, &at(x, y, 1_000));
            let was_pan = ended.iter().any(|g| matches!(g, Gesture::Pan { phase: GesturePhase::Ended, .. }));
            observe(ended);
            if was_pan {
                prop_assert_eq!(sum, total);
                prop_assert_eq!(total, Point::new(x, y));
            }
        }
    }
}

// ---------------------------------------------------------------------
// Animation (crate::animation, Milestone 27).
// ---------------------------------------------------------------------

mod animation {
    use std::time::Duration;

    use proptest::prelude::*;

    use rustnative_core::{
        AnimatedProperty, AnimatedValue, Animation, AnimationOwner, Easing, Fill, NodeId, Point,
        Timeline, Transition,
    };

    fn easing_strategy() -> impl Strategy<Value = Easing> {
        prop_oneof![
            Just(Easing::Linear),
            Just(Easing::EaseIn),
            Just(Easing::EaseOut),
            Just(Easing::EaseInOut),
            (0.0f32..1.0, -2.0f32..2.0, 0.0f32..1.0, -2.0f32..2.0)
                .prop_map(|(x1, y1, x2, y2)| Easing::cubic_bezier(x1, y1, x2, y2)),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// However a curve is shaped, it starts at its start and ends at
        /// its end: an animation that did not actually arrive where the
        /// rendered tree says it belongs would leave the UI permanently
        /// wrong.
        #[test]
        fn every_curve_lands_exactly_on_both_endpoints(easing in easing_strategy()) {
            prop_assert!(easing.progress(0.0).abs() < 1e-3);
            prop_assert!((easing.progress(1.0) - 1.0).abs() < 1e-3);
        }

        /// A timed animation always finishes by the end of its duration,
        /// whatever the curve and however coarse the frames — a timeline
        /// that never finished would animate forever and keep the frame
        /// driver awake.
        #[test]
        fn a_timed_animation_finishes_within_its_duration(
            millis in 1u64..500,
            frame in 1u64..80,
            easing in easing_strategy(),
        ) {
            let node = NodeId::from_key("p");
            let mut timeline = Timeline::new();
            timeline.transition(
                node,
                AnimatedProperty::Translation,
                AnimatedValue::Offset(Point::new(0, 0)),
                AnimatedValue::Offset(Point::new(100, 0)),
                Transition::new(Duration::from_millis(millis)).easing(easing),
                Duration::ZERO,
            );
            let mut now = Duration::ZERO;
            let mut finished = None;
            while now <= Duration::from_millis(millis * 2 + frame) {
                now += Duration::from_millis(frame);
                let output = timeline.tick(now);
                if let Some(done) = output.finished.first() {
                    finished = Some((now, output.frames[0].value));
                    prop_assert_eq!(done.node, node);
                    break;
                }
            }
            let (at, value) = finished.expect("a timed animation must finish");
            prop_assert!(at <= Duration::from_millis(millis + frame));
            // A finished transition releases the property back to the
            // rendered tree rather than pinning its target value.
            prop_assert_eq!(value, None);
            prop_assert!(!timeline.is_active());
        }

        /// Interrupting an animation never makes the value jump: the
        /// replacement starts from exactly where the old one had reached.
        #[test]
        fn retargeting_is_continuous(
            first in 1i32..400,
            second in -400i32..400,
            interrupt_at in 1u64..99,
        ) {
            let node = NodeId::from_key("p");
            let mut timeline = Timeline::new();
            let linear = Transition::new(Duration::from_millis(100)).easing(Easing::Linear);
            let at = |x| AnimatedValue::Offset(Point::new(x, 0));
            timeline.transition(node, AnimatedProperty::Position, at(0), at(first), linear, Duration::ZERO);

            let before = timeline
                .tick(Duration::from_millis(interrupt_at))
                .frames
                .first()
                .and_then(|frame| frame.value)
                .unwrap_or(at(0));
            timeline.transition(
                node,
                AnimatedProperty::Position,
                at(second),
                at(second),
                linear,
                Duration::from_millis(interrupt_at),
            );
            let after = timeline
                .tick(Duration::from_millis(interrupt_at))
                .frames
                .first()
                .and_then(|frame| frame.value)
                .unwrap_or(before);
            prop_assert_eq!(after, before, "the value must not jump when retargeted");
        }

        /// An animation that fills forwards holds its final value, and one
        /// that does not returns the property to rest — either way the
        /// timeline ends up empty rather than leaking a track.
        #[test]
        fn every_animation_ends_and_leaves_the_timeline_empty(
            millis in 1u64..200,
            forwards in any::<bool>(),
        ) {
            let node = NodeId::from_key("p");
            let mut timeline = Timeline::new();
            let animation = Animation::new(
                AnimatedProperty::Translation,
                AnimatedValue::Offset(Point::new(30, 0)),
                Transition::new(Duration::from_millis(millis)).easing(Easing::EaseOut),
            )
            .from(AnimatedValue::Offset(Point::new(0, 0)))
            .fill(if forwards { Fill::Forwards } else { Fill::None });
            timeline.start(node, &animation, AnimationOwner::Transition(node), None, Duration::ZERO);

            let end = timeline.tick(Duration::from_millis(millis));
            prop_assert_eq!(end.finished.len(), 1);
            let expected = if forwards { Point::new(30, 0) } else { Point::new(0, 0) };
            prop_assert_eq!(end.frames[0].value, Some(AnimatedValue::Offset(expected)));
            prop_assert!(timeline.is_empty());
        }
    }
}

// ---------------------------------------------------------------------
// Virtualization (crate::virtualization, Milestone 28).
// ---------------------------------------------------------------------

mod virtualization {
    use proptest::prelude::*;

    use rustnative_core::{ExtentCache, ItemExtent, NodeId, ScrollAnchor, VirtualRange};

    /// A measured cache and the plain extents it should agree with.
    fn measured() -> impl Strategy<Value = (ExtentCache, Vec<u32>)> {
        (1usize..300, 1u32..60).prop_flat_map(|(count, estimate)| {
            proptest::collection::vec((0..count, 0u32..120), 0..64).prop_map(move |measurements| {
                let mut cache = ExtentCache::new(count, ItemExtent::Estimated(estimate));
                let mut plain = vec![estimate; count];
                for (index, extent) in measurements {
                    cache.record(index, extent);
                    plain[index] = extent;
                }
                (cache, plain)
            })
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// The Fenwick tree's offsets are exactly the prefix sums a naive
        /// scan would compute, after any sequence of measurements — the
        /// structure is an optimization, never a different answer.
        #[test]
        fn fenwick_offsets_equal_naive_prefix_sums((cache, plain) in measured()) {
            let mut sum = 0u32;
            for (index, extent) in plain.iter().enumerate() {
                prop_assert_eq!(cache.offset_of(index), sum);
                sum += extent;
            }
            prop_assert_eq!(cache.total(), sum);
        }

        /// `index_at` finds the item that actually contains an offset.
        #[test]
        fn the_item_at_an_offset_contains_it((cache, _) in measured(), fraction in 0.0f64..1.0) {
            let total = cache.total();
            prop_assume!(total > 0);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let offset = (f64::from(total - 1) * fraction) as u32;
            let index = cache.index_at(offset);
            prop_assert!(cache.offset_of(index) <= offset);
            prop_assert!(offset < cache.offset_of(index) + cache.extent_of(index));
        }

        /// Whatever the scroll position and viewport, the range realizes
        /// every item the viewport touches, and no more than overscan
        /// beyond it.
        #[test]
        fn a_range_always_covers_its_viewport(
            (cache, _) in measured(),
            offset in 0u32..20_000,
            viewport in 1u32..600,
            overscan in 0usize..5,
        ) {
            let range = VirtualRange::compute(offset, viewport, &cache, overscan);
            let count = cache.count();
            prop_assert!(range.last_exclusive <= count);
            let first_visible = cache.index_at(offset);
            let last_visible = cache.index_at(offset.saturating_add(viewport - 1));
            for index in first_visible..=last_visible {
                prop_assert!(range.contains(index), "item {} is visible but not realized", index);
            }
            prop_assert!(range.first + overscan >= first_visible);
            prop_assert!(range.last_exclusive <= last_visible + 1 + overscan);
        }

        /// Scrolling further down never moves the range up.
        #[test]
        fn ranges_are_monotone_in_the_scroll_offset(
            (cache, _) in measured(),
            offset in 0u32..20_000,
            delta in 0u32..500,
            viewport in 1u32..600,
        ) {
            let before = VirtualRange::compute(offset, viewport, &cache, 2);
            let after = VirtualRange::compute(offset + delta, viewport, &cache, 2);
            prop_assert!(after.first >= before.first);
            prop_assert!(after.last_exclusive >= before.last_exclusive);
        }

        /// However many items are inserted above the viewport, the
        /// anchored item resolves to the same distance into itself.
        #[test]
        fn anchoring_survives_insertions_above(
            count in 10usize..500,
            offset in 0u32..5_000,
            inserted in 0usize..200,
        ) {
            let row = |datum: usize| NodeId::from_key(&format!("row-{datum}"));
            let before = ExtentCache::new(count, ItemExtent::Fixed(20));
            let anchor = ScrollAnchor::capture(offset, &before, |index| Some(row(index)))
                .expect("the list has items");
            let anchored_index = before.index_at(offset);
            let within = offset - before.offset_of(anchored_index);

            let after = ExtentCache::new(count + inserted, ItemExtent::Fixed(20));
            let resolved = anchor
                .resolve(&after, |node| (node == row(anchored_index)).then_some(anchored_index + inserted))
                .expect("the anchored row still exists");
            prop_assert_eq!(resolved - after.offset_of(anchored_index + inserted), within);
        }
    }
}

// ---------------------------------------------------------------------
// Routes (crate::navigation, Milestone 30).
// ---------------------------------------------------------------------

mod routes {
    use proptest::prelude::*;

    use rustnative_core::Route;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// Any parameter values — spaces, slashes, `%`, `?`, `#`, non-ASCII
        /// — survive building a path and matching it back: encoding and
        /// decoding are inverses, so a deep link built by one part of an
        /// application routes correctly in another.
        #[test]
        fn a_built_path_matches_back_to_its_parameters(
            user in r"\PC{1,12}",
            rest in proptest::collection::vec(r"\PC{1,8}", 0..4),
        ) {
            let route = Route::parse("/users/:user/files/*rest").unwrap();
            let rest = rest.into_iter().filter(|part| !part.contains('/')).collect::<Vec<_>>().join("/");
            let path = route.build(&[("user", &user), ("rest", &rest)]).expect("every parameter given");
            let matched = route.matches(&path).expect("a built path matches its own route");
            prop_assert_eq!(matched.get("user"), Some(user.as_str()));
            prop_assert_eq!(matched.get("rest"), Some(rest.as_str()));
        }
    }
}
