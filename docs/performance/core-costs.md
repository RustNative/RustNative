# Core cost characteristics

`PLAN.md` Milestone 52. Measured by `crates/rustnative-core/tests/core_costs.rs`
(`RUSTNATIVE_WRITE_COSTS=1 cargo test -p rustnative-core --test core_costs --release`),
not estimated. The heap figures are checked on every run of the test suite.
Times are the slowest of 20 runs on the machine that wrote this page, in a
release build: read them as orders of magnitude, not as budgets.
The budgets are in `budgets/`.

## Heap

A flat tree of 10,000 labels, and its snapshot:

| Figure | Value |
|---|---|
| Tree, per node | 1241 B |
| Snapshot, per node | 2367 B |
| Allocations, per node | 5.0 |

## Stack

The smallest thread stack, to 4 KiB, on which a snapshot, a diff, a layout, and
the tree's drop complete for a chain of nested columns:

| Depth | Stack |
|---|---|
| 100 | 67 KiB |
| 1,000 | 579 KiB |

The cost is linear in depth: a real screen is tens of levels deep, well under
the default 1 MiB main-thread stack on Windows.

## Worst-case time

| Path | 1,000 nodes | 10,000 nodes |
|---|---|---|
| Snapshot | 8.34 ms | 74.72 ms |
| Diff (one leaf changed) | 0.68 ms | 9.81 ms |
| Layout | 2.06 ms | 12.53 ms |
