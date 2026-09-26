//! Benchmarks for the hot paths the standards audit's P1.11/P1.19 findings
//! were about: tree reconciliation, layout, and scheduling. Run with
//! `cargo bench -p rustnative-core`.
//!
//! These exist to make a future regression in these paths *visible* (a
//! reconciliation change that reintroduces O(n²) child lookups, a layout
//! change that adds allocation churn) rather than to assert a specific
//! absolute number — this crate does not currently gate CI on benchmark
//! output, matching `criterion`'s own guidance that meaningful thresholds
//! depend on the machine running them.
//!
//! `unwrap`/`expect` are allowed throughout this file for the same reason
//! `clippy.toml` allows them in tests: a benchmark's setup code asserts its
//! own preconditions, and a panic there is the assertion. Clippy's
//! `allow-unwrap-in-tests` does not reach a `benches/` target, which is a
//! separate crate with no `cfg(test)`, so the exemption is stated here.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main,
};

use rustnative_core::{
    Component, ComponentContext, ComponentTree, Event, ExtentCache, ItemExtent, LayoutEngine,
    LayoutStyle, Node, RowStyle, Size, SizeMode, TreeDiff, TreeSnapshot, VirtualListStyle,
    VirtualRange,
};

/// Builds a moderately wide-and-deep tree: `width` labelled children per
/// level, `depth` levels, which is representative of a real settings/form
/// screen rather than either extreme (a single deep chain, or a single wide
/// list) — see the individual benchmark functions below for where each
/// shape matters more.
fn build_tree(width: usize, depth: usize, text_prefix: &str) -> Node {
    fn build(width: usize, depth: usize, path: &str) -> Node {
        if depth == 0 {
            return Node::label(path, format!("{path}-label"));
        }
        let children =
            (0..width).map(|i| build(width, depth - 1, &format!("{path}-{i}"))).collect::<Vec<_>>();
        Node::column(path, children)
    }
    build(width, depth, text_prefix)
}

fn bench_snapshot_construction(c: &mut Criterion) {
    let tree = build_tree(8, 3, "n");
    c.bench_function("tree_snapshot_from_node/8x3", |b| {
        b.iter(|| black_box(TreeSnapshot::from_node(black_box(&tree)).unwrap()));
    });
}

fn bench_diff_no_changes(c: &mut Criterion) {
    let tree = build_tree(8, 3, "n");
    let previous = TreeSnapshot::from_node(&tree).unwrap();
    let next = TreeSnapshot::from_node(&tree).unwrap();
    c.bench_function("tree_diff_between/8x3/no_changes", |b| {
        b.iter(|| black_box(TreeDiff::between(black_box(&previous), black_box(&next))));
    });
}

fn bench_diff_leaf_text_change(c: &mut Criterion) {
    fn build(width: usize, depth: usize, path: &str, override_path: &str) -> Node {
        if depth == 0 {
            let text =
                if path == override_path { "changed".to_string() } else { format!("{path}-label") };
            return Node::label(path, text);
        }
        let children = (0..width)
            .map(|i| build(width, depth - 1, &format!("{path}-{i}"), override_path))
            .collect::<Vec<_>>();
        Node::column(path, children)
    }

    let previous_tree = build_tree(8, 3, "n");
    // Perturb exactly one leaf's text, which is the common case a real
    // application re-render produces: this benchmark's value is in showing
    // that a single-node change stays cheap regardless of overall tree
    // size, which an accidentally-reintroduced O(n^2) full-tree rescan
    // would not.
    let next_tree = build(8, 3, "n", "n-0-0-0");
    let previous = TreeSnapshot::from_node(&previous_tree).unwrap();
    let next = TreeSnapshot::from_node(&next_tree).unwrap();
    c.bench_function("tree_diff_between/8x3/single_leaf_text_change", |b| {
        b.iter(|| black_box(TreeDiff::between(black_box(&previous), black_box(&next))));
    });
}

fn bench_layout_wide_row(c: &mut Criterion) {
    let children = (0..200)
        .map(|i| {
            Node::label_with_layout(
                format!("item-{i}"),
                "item",
                LayoutStyle::new().width(SizeMode::Fixed(80)),
            )
        })
        .collect::<Vec<_>>();
    let tree = Node::row_with_layout("root", children, LayoutStyle::new(), RowStyle::new().gap(4));
    let snapshot = TreeSnapshot::from_node(&tree).unwrap();
    let engine = LayoutEngine::new();
    c.bench_function("layout/row_of_200_fixed_width_leaves", |b| {
        b.iter(|| black_box(engine.layout(black_box(&snapshot), Size::new(4000, 200))));
    });
}

fn bench_layout_nested_columns(c: &mut Criterion) {
    let tree = build_tree(6, 4, "n");
    let snapshot = TreeSnapshot::from_node(&tree).unwrap();
    let engine = LayoutEngine::new();
    c.bench_function("layout/nested_columns_6x4", |b| {
        b.iter(|| black_box(engine.layout(black_box(&snapshot), Size::new(1200, 1200))));
    });
}

// --- Component tree / scheduler ------------------------------------------

#[derive(Clone, PartialEq, Default)]
struct NoProps;

struct Counter {
    count: u32,
}

impl Component for Counter {
    type Props = NoProps;
    type Message = ();

    fn new(_props: Self::Props) -> Self {
        Self { count: 0 }
    }
    fn props(&self) -> &Self::Props {
        &NoProps
    }
    fn set_props(&mut self, _props: Self::Props) {}

    fn view(&self) -> Node {
        Node::column(
            "root",
            [Node::label("count", self.count.to_string()), Node::button("increment", "+")],
        )
    }

    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { .. }) {
            self.count += 1;
        }
    }
}

fn bench_component_dispatch(c: &mut Criterion) {
    c.bench_function("component_tree/dispatch_click_and_rerender", |b| {
        b.iter_batched(
            || ComponentTree::new(Counter::new(NoProps)),
            |mut tree| {
                black_box(tree.dispatch(Event::Click {
                    target: rustnative_core::NodeId::from_key("increment"),
                }));
            },
            BatchSize::SmallInput,
        );
    });
}

struct SpawnsManyTasks;

impl Component for SpawnsManyTasks {
    type Props = NoProps;
    type Message = u32;

    fn new(_props: Self::Props) -> Self {
        Self
    }
    fn props(&self) -> &Self::Props {
        &NoProps
    }
    fn set_props(&mut self, _props: Self::Props) {}

    fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
        context.effect("spawn-many", (), |ctx| {
            for i in 0..100u32 {
                let _ = ctx.spawn(async move { i });
            }
            Box::new(|| {})
        });
        Node::label("root", "root")
    }
    fn view(&self) -> Node {
        unreachable!()
    }
    fn update(&mut self, _event: Event) {}
}

fn bench_scheduler_spawn_and_drain(c: &mut Criterion) {
    // Exercised through `ComponentTree`'s public `pump_tasks` rather than
    // `Scheduler::drain` directly: the latter is `pub(crate)` (an
    // application only ever observes task completion through a component's
    // `message`, per `crate::scheduler`'s design), so this benchmark
    // measures the same code path a real application actually takes.
    c.bench_function("component_tree/spawn_100_tasks_then_pump", |b| {
        b.iter_batched(
            || ComponentTree::new(SpawnsManyTasks::new(NoProps)),
            |mut tree| {
                // Give the executor a chance to actually complete the
                // (trivially ready) tasks before pumping.
                std::thread::sleep(std::time::Duration::from_millis(20));
                black_box(tree.pump_tasks())
            },
            BatchSize::SmallInput,
        );
    });
}

// --- Scaling sweeps -------------------------------------------------------
//
// The benchmarks above measure one representative tree each, which answers
// "is this fast enough?" but not "how does it grow?". The standards audit
// asks for the second question explicitly — "use representative sizes such
// as 10, 100, 1k, 10k nodes" — because the regression these paths are most
// exposed to is *asymptotic*, not constant-factor: P1.11 was about
// reconciliation rebuilding tree topology by rescanning the whole node map,
// which looks merely slow at 500 nodes and is ruinous at 10,000. A single
// data point cannot show that shape; a sweep can, and the ratio between
// consecutive sizes is what a reader should watch.

/// The node counts every sweep below runs at.
const SCALING_SIZES: [usize; 4] = [10, 100, 1_000, 10_000];

/// A flat list of `count` labelled leaves under one column — the shape a
/// virtualized list or a large table degenerates to, and the one that most
/// directly exposes per-sibling work.
fn flat_tree(count: usize) -> Node {
    Node::column(
        "root",
        (0..count).map(|i| Node::label(format!("item-{i}"), "item")).collect::<Vec<_>>(),
    )
}

fn bench_snapshot_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("scaling/tree_snapshot_from_node");
    for size in SCALING_SIZES {
        let tree = flat_tree(size);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &tree, |b, tree| {
            b.iter(|| black_box(TreeSnapshot::from_node(black_box(tree)).unwrap()));
        });
    }
    group.finish();
}

fn bench_diff_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("scaling/tree_diff_between");
    for size in SCALING_SIZES {
        // One leaf's text differs, so the diff must walk everything to find
        // the single `Update` — the worst realistic case for a rerender that
        // changes almost nothing, and the one a topology rescan punishes.
        let previous = TreeSnapshot::from_node(&flat_tree(size)).unwrap();
        let mut children =
            (0..size).map(|i| Node::label(format!("item-{i}"), "item")).collect::<Vec<_>>();
        if let Some(last) = children.last_mut() {
            *last = Node::label(format!("item-{}", size - 1), "changed");
        }
        let next = TreeSnapshot::from_node(&Node::column("root", children)).unwrap();
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &(previous, next),
            |b, (previous, next)| {
                b.iter(|| black_box(TreeDiff::between(black_box(previous), black_box(next))));
            },
        );
    }
    group.finish();
}

fn bench_layout_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("scaling/layout");
    let engine = LayoutEngine::new();
    for size in SCALING_SIZES {
        let snapshot = TreeSnapshot::from_node(&flat_tree(size)).unwrap();
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &snapshot, |b, snapshot| {
            b.iter(|| black_box(engine.layout(black_box(snapshot), Size::new(1200, 40_000))));
        });
    }
    group.finish();
}

fn bench_deep_tree_scaling(c: &mut Criterion) {
    // Depth rather than width: `TreeSnapshot`'s depth index and the diff's
    // deepest-first removal ordering are both depth-sensitive, and a chain
    // is the shape that isolates them from sibling-count effects.
    let mut group = c.benchmark_group("scaling/deep_chain_snapshot");
    for depth in [10usize, 100, 1_000] {
        let mut node = Node::label("leaf", "leaf");
        for level in 0..depth {
            node = Node::column(format!("level-{level}"), [node]);
        }
        group.throughput(Throughput::Elements(depth as u64));
        group.bench_with_input(BenchmarkId::from_parameter(depth), &node, |b, node| {
            b.iter(|| black_box(TreeSnapshot::from_node(black_box(node)).unwrap()));
        });
    }
    group.finish();
}

fn bench_virtual_range(c: &mut Criterion) {
    let mut extents = ExtentCache::new(100_000, ItemExtent::Estimated(24));
    for index in (0..100_000).step_by(7) {
        extents.record(index, 30);
    }
    c.bench_function("virtual_list/range_100k", |b| {
        b.iter(|| {
            black_box(VirtualRange::compute(
                black_box(1_200_000),
                black_box(800),
                black_box(&extents),
                2,
            ))
        });
    });
}

fn bench_layout_virtual(c: &mut Criterion) {
    // 10,000 items, 40 of them realized: layout must cost the realized
    // items, not the list's length.
    let rows = (5_000..5_040)
        .map(|index| Node::label(format!("row-{index}"), "row").with_item_index(index));
    let tree =
        Node::virtual_list("list", VirtualListStyle::new(10_000, ItemExtent::Estimated(24)), rows);
    let snapshot = TreeSnapshot::from_node(&tree).unwrap();
    let engine = LayoutEngine::new();
    let extents = std::collections::HashMap::from([(
        rustnative_core::NodeId::from_key("list"),
        ExtentCache::new(10_000, ItemExtent::Estimated(24)),
    )]);
    c.bench_function("layout/virtual_10k", |b| {
        b.iter(|| {
            black_box(engine.layout_result_with(
                black_box(&snapshot),
                Size::new(400, 800),
                &rustnative_core::DefaultIntrinsicMeasurer,
                &extents,
            ))
        });
    });
}

criterion_group!(
    benches,
    bench_snapshot_construction,
    bench_diff_no_changes,
    bench_diff_leaf_text_change,
    bench_layout_wide_row,
    bench_layout_nested_columns,
    bench_component_dispatch,
    bench_scheduler_spawn_and_drain,
    bench_snapshot_scaling,
    bench_diff_scaling,
    bench_layout_scaling,
    bench_deep_tree_scaling,
    bench_virtual_range,
    bench_layout_virtual,
);
criterion_main!(benches);
