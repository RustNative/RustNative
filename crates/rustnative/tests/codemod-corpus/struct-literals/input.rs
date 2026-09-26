use rustnative_core::{EdgeInsets, LayoutStyle};

/// Padding written before Milestone 39.
fn padding(gutter: i32) -> EdgeInsets {
    // The start edge gets the gutter.
    EdgeInsets { top: 4, left: gutter, bottom: 4, right: 8 }
}

fn qualified(left: i32, right: i32) -> rustnative_core::EdgeInsets {
    rustnative_core::EdgeInsets { left, right, ..EdgeInsets::all(0) }
}

fn start_of() -> i32 {
    EdgeInsets::all(2).left + EdgeInsets { top: 0, left: 1, bottom: 0, right: 1 }.right
}

fn style() -> LayoutStyle {
    LayoutStyle::default().with_padding(EdgeInsets {
        top: 0,
        left: 12, // indented
        bottom: 0,
        right: 12,
    })
}
