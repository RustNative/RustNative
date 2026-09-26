//! Grids (`PLAN.md` Milestone 48, `C18-1`): children land in their tracks,
//! fractions share the leftover width, spans cover their tracks and gaps,
//! and unplaced children flow into the next free cell.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use rustnative_core::{
    GridPlacement, GridStyle, LayoutEngine, LayoutStyle, Node, NodeId, Size, Track, TreeSnapshot,
};

#[test]
fn children_fill_their_cells() {
    let tree = Node::grid(
        "form",
        GridStyle::new([Track::Fixed(120), Track::Fraction(1), Track::Fraction(3)])
            .rows([Track::Fixed(30), Track::Fixed(40)])
            .gap(8),
        LayoutStyle::new(),
        [
            Node::label("a", "A"),
            Node::label("b", "B"),
            Node::label("c", "C"),
            Node::label_with_layout(
                "wide",
                "Wide",
                LayoutStyle::new().grid(GridPlacement::at(1, 1).span(1, 2)),
            ),
            Node::label("flowed", "Flowed"),
        ],
    );
    let snapshot = TreeSnapshot::from_node(&tree).unwrap();
    let rects = LayoutEngine::new().layout(&snapshot, Size::new(400, 300));
    let rect = |key: &str| *rects.get(&NodeId::from_key(key)).expect(key);

    let (a, b, c) = (rect("a"), rect("b"), rect("c"));
    assert_eq!((a.x, a.y, a.width, a.height), (0, 0, 120, 30));
    // 400 - 120 - 2 gaps of 8 = 264, shared 1:3.
    assert_eq!((b.x, b.width), (128, 66));
    assert_eq!((c.x, c.width), (202, 198));

    let wide = rect("wide");
    assert_eq!((wide.x, wide.y, wide.width, wide.height), (128, 38, 66 + 8 + 198, 40));
    let flowed = rect("flowed");
    assert_eq!((flowed.x, flowed.y), (0, 38), "the first free cell is row 1, column 0");
}
