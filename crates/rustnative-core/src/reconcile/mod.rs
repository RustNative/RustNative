//! Declarative-tree → resolved-tree reconciliation: snapshotting and
//! diffing. See `snapshot` and `diff`.

mod diff;
mod snapshot;

pub use diff::{TreeDiff, TreeOp};
pub use snapshot::{TreeNode, TreeSnapshot};
