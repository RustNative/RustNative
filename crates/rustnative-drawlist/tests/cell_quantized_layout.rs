//! The core's layout under a mapping whose unit is a terminal cell
//! (`docs/superpowers/plans/2026-10-13-terminal-milestone-38.md`, Phase 0):
//! whatever the portable engine lays out, nothing visible collapses to zero
//! cells, and siblings laid out apart stay apart.

use std::collections::HashMap;

use rustnative_core::{
    ColumnStyle, EdgeInsets, IntrinsicMeasurer, LayoutEngine, LayoutStyle, Node, NodeKind, Rect,
    RowStyle, Size, SizeMode, TreeSnapshot,
};
use rustnative_drawlist::{GridMapping, GridRect};

/// Text measured in cells, reported in logical pixels: what a terminal
/// backend's measurer does (one column per narrow character here).
struct Cells;

impl IntrinsicMeasurer for Cells {
    fn measure(&self, kind: NodeKind, text: Option<&str>, max_width: Option<i32>) -> Size {
        let columns = i32::try_from(text.map_or(0, |text| text.chars().count())).unwrap_or(0);
        let chrome = match kind {
            NodeKind::Button => 4,
            NodeKind::TextInput => 2,
            _ => 0,
        };
        let wanted = (columns + chrome).max(1);
        let available = max_width.map_or(wanted, |width| (width / 8).max(1));
        let lines = (wanted + available - 1) / available;
        Size::new(
            u32::try_from(wanted.min(available) * 8).unwrap_or(8),
            u32::try_from(lines * 16).unwrap_or(16),
        )
    }
}

fn tree() -> Node {
    let tight = ColumnStyle::new().padding(EdgeInsets::all(4)).gap(2);
    Node::column(
        "root",
        [
            Node::label("title", "A terminal application"),
            Node::row(
                "buttons",
                [Node::button("ok", "OK"), Node::button("cancel", "Cancel"), Node::label("x", "x")],
            ),
            Node::column_with_layout(
                "tight",
                [Node::label("a", "a"), Node::label("b", "b"), Node::text_input("field", "typed")],
                LayoutStyle::new(),
                tight,
            ),
            Node::row_with_layout(
                "narrow",
                [Node::label("wrapped", "a long label that has to wrap in a narrow row")],
                LayoutStyle::new().width(SizeMode::Fixed(120)),
                RowStyle::new().padding(EdgeInsets::all(0)),
            ),
        ],
    )
}

#[test]
fn nothing_visible_collapses_to_zero_cells_and_siblings_stay_apart() {
    let snapshot = TreeSnapshot::from_node(&tree()).expect("unique keys");
    let cells = GridMapping::new(8, 16);
    for (columns, rows) in [(80_u16, 24_u16), (40, 12), (132, 43)] {
        let size = cells.logical_size(columns, rows);
        let layout =
            LayoutEngine::new().layout_result_with(&snapshot, size, &Cells, &HashMap::new());
        let grid: HashMap<_, GridRect> =
            layout.rects.iter().map(|(id, rect)| (*id, cells.rect(*rect))).collect();
        for node in snapshot.nodes() {
            let logical: Rect = layout.rects[&node.id];
            if logical.width > 0 && logical.height > 0 {
                assert!(
                    !grid[&node.id].is_empty(),
                    "{:?} is {logical:?} but no cells at {columns}x{rows}",
                    node.id.local_key()
                );
            }
        }
        for parent in snapshot.nodes() {
            let children: Vec<_> = snapshot.children_of(parent.id).map(|child| child.id).collect();
            for (index, a) in children.iter().enumerate() {
                for b in &children[index + 1..] {
                    assert!(
                        grid[a].intersect(&grid[b]).is_empty(),
                        "{:?} and {:?} overlap in cells at {columns}x{rows}: {:?} {:?}",
                        a.local_key(),
                        b.local_key(),
                        grid[a],
                        grid[b]
                    );
                }
            }
        }
    }
}
