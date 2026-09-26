//! The core's cost characteristics (`PLAN.md` Milestone 52), measured
//! rather than estimated: heap per node, the stack the hot paths need, and
//! worst-case times at 1k and 10k nodes.
//!
//! `RUSTNATIVE_WRITE_COSTS=1 cargo test -p rustnative-core --test core_costs
//! --release` rewrites `docs/performance/core-costs.md` from a fresh
//! measurement. Otherwise the test measures again and fails when the
//! committed heap figures no longer hold: allocation is deterministic,
//! times are not, so only the heap is checked.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    reason = "a failed expectation in an integration test is the test failing; figures are reported, not computed with"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use rustnative_core::{LayoutEngine, Node, Size, TreeDiff, TreeSnapshot};

/// Counts live and total heap bytes.
struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards to the system allocator unchanged, only counting.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::SeqCst);
        ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        // SAFETY: the caller's contract, passed through.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
        // SAFETY: the caller's contract, passed through.
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn flat(count: usize, changed: bool) -> Node {
    Node::column(
        "root",
        (0..count)
            .map(|index| {
                Node::label(
                    format!("item-{index}"),
                    if changed && index + 1 == count { "changed" } else { "item" },
                )
            })
            .collect::<Vec<_>>(),
    )
}

fn chain(depth: usize) -> Node {
    let mut node = Node::label("leaf", "leaf");
    for level in 0..depth {
        node = Node::column(format!("level-{level}"), [node]);
    }
    node
}

/// Bytes held per node by a tree and its snapshot, and allocations per node.
fn heap_per_node(count: usize) -> (f64, f64, f64) {
    let (before, calls) = (LIVE.load(Ordering::SeqCst), ALLOCATIONS.load(Ordering::SeqCst));
    let tree = flat(count, false);
    let after_tree = LIVE.load(Ordering::SeqCst);
    let snapshot = TreeSnapshot::from_node(&tree).unwrap();
    let after_snapshot = LIVE.load(Ordering::SeqCst);
    let allocations = ALLOCATIONS.load(Ordering::SeqCst) - calls;
    let per = |bytes: usize| bytes as f64 / count as f64;
    let result = (
        per(after_tree - before),
        per(after_snapshot - after_tree),
        allocations as f64 / count as f64,
    );
    drop((tree, snapshot));
    result
}

/// The slowest of 20 runs of `work`.
fn worst(mut work: impl FnMut()) -> Duration {
    (0..20)
        .map(|_| {
            let started = Instant::now();
            work();
            started.elapsed()
        })
        .max()
        .unwrap_or_default()
}

fn timings(count: usize) -> [Duration; 3] {
    let (tree, changed) = (flat(count, false), flat(count, true));
    let previous = TreeSnapshot::from_node(&tree).unwrap();
    let next = TreeSnapshot::from_node(&changed).unwrap();
    let engine = LayoutEngine::new();
    [
        worst(|| drop(TreeSnapshot::from_node(&tree).unwrap())),
        worst(|| drop(TreeDiff::between(&previous, &next))),
        worst(|| drop(engine.layout(&previous, Size::new(1200, 40_000)))),
    ]
}

/// The child side of the stack measurement: snapshot, diff, and lay out a
/// chain `RUSTNATIVE_COSTS_DEPTH` deep on a thread with a stack of
/// `RUSTNATIVE_COSTS_STACK` bytes. A stack overflow ends the process.
#[test]
fn stack_child() {
    let (Ok(stack), Ok(depth)) =
        (std::env::var("RUSTNATIVE_COSTS_STACK"), std::env::var("RUSTNATIVE_COSTS_DEPTH"))
    else {
        return;
    };
    let (stack, depth): (usize, usize) = (stack.parse().unwrap(), depth.parse().unwrap());
    // The chain is built on the main thread: only the core's paths run on
    // the measured stack.
    let (previous, next) = (
        TreeSnapshot::from_node(&chain(depth)).unwrap(),
        TreeSnapshot::from_node(&chain(depth)).unwrap(),
    );
    std::thread::Builder::new()
        .stack_size(stack)
        .spawn(move || {
            let tree = chain(depth);
            drop(TreeSnapshot::from_node(&tree).unwrap());
            drop(TreeDiff::between(&previous, &next));
            drop(LayoutEngine::new().layout(&previous, Size::new(800, 600)));
            // The chain's own drop is recursive too, and part of the cost.
            drop(tree);
        })
        .unwrap()
        .join()
        .unwrap();
}

/// The smallest stack (to 4 KiB) on which the core's paths handle a chain
/// `depth` deep.
fn stack_needed(depth: usize) -> usize {
    let fits = |stack: usize| {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "stack_child", "--nocapture", "--test-threads=1"])
            .env("RUSTNATIVE_COSTS_STACK", stack.to_string())
            .env("RUSTNATIVE_COSTS_DEPTH", depth.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };
    let (mut low, mut high) = (4 * 1024, 64 * 1024 * 1024);
    assert!(fits(high), "a chain {depth} deep does not fit 64 MiB");
    while high - low > 4 * 1024 {
        let middle = usize::midpoint(low, high);
        if fits(middle) {
            high = middle;
        } else {
            low = middle;
        }
    }
    high
}

fn millis(duration: Duration) -> String {
    format!("{:.2} ms", duration.as_secs_f64() * 1000.0)
}

#[test]
fn the_documented_costs_hold() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/performance/core-costs.md");
    let (tree, snapshot, allocations) = heap_per_node(10_000);
    if std::env::var_os("RUSTNATIVE_WRITE_COSTS").is_none() {
        let text =
            std::fs::read_to_string(&path).expect("docs/performance/core-costs.md is committed");
        let committed = |label: &str| -> f64 {
            let line =
                text.lines().find(|line| line.starts_with(&format!("| {label} |"))).expect(label);
            line.split('|').nth(2).unwrap().trim().split(' ').next().unwrap().parse().unwrap()
        };
        // Allocation is deterministic for a given build; a change past a
        // tenth either way is a change the document must record.
        for (label, measured) in [("Tree, per node", tree), ("Snapshot, per node", snapshot)] {
            let documented = committed(label);
            assert!(
                (measured - documented).abs() <= documented / 10.0,
                "{label}: measured {measured:.0} B, documented {documented:.0} B"
            );
        }
        return;
    }
    let (small, large) = (timings(1_000), timings(10_000));
    let (stack_100, stack_1000) = (stack_needed(100), stack_needed(1_000));
    let document = format!(
        "# Core cost characteristics\n\n\
         `PLAN.md` Milestone 52. Measured by `crates/rustnative-core/tests/core_costs.rs`\n\
         (`RUSTNATIVE_WRITE_COSTS=1 cargo test -p rustnative-core --test core_costs --release`),\n\
         not estimated. The heap figures are checked on every run of the test suite.\n\
         Times are the slowest of 20 runs on the machine that wrote this page, in a\n\
         release build: read them as orders of magnitude, not as budgets.\n\
         The budgets are in `budgets/`.\n\n\
         ## Heap\n\n\
         A flat tree of 10,000 labels, and its snapshot:\n\n\
         | Figure | Value |\n|---|---|\n\
         | Tree, per node | {tree:.0} B |\n\
         | Snapshot, per node | {snapshot:.0} B |\n\
         | Allocations, per node | {allocations:.1} |\n\n\
         ## Stack\n\n\
         The smallest thread stack, to 4 KiB, on which a snapshot, a diff, a layout, and\n\
         the tree's drop complete for a chain of nested columns:\n\n\
         | Depth | Stack |\n|---|---|\n\
         | 100 | {} KiB |\n\
         | 1,000 | {} KiB |\n\n\
         The cost is linear in depth: a real screen is tens of levels deep, well under\n\
         the default 1 MiB main-thread stack on Windows.\n\n\
         ## Worst-case time\n\n\
         | Path | 1,000 nodes | 10,000 nodes |\n|---|---|---|\n\
         | Snapshot | {} | {} |\n\
         | Diff (one leaf changed) | {} | {} |\n\
         | Layout | {} | {} |\n",
        stack_100 / 1024,
        stack_1000 / 1024,
        millis(small[0]),
        millis(large[0]),
        millis(small[1]),
        millis(large[1]),
        millis(small[2]),
        millis(large[2]),
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, document).unwrap();
}
