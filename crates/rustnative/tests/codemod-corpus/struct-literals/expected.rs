use rustnative_core::{EdgeInsets, LayoutStyle};

/// Padding written before Milestone 39.
fn padding(gutter: i32) -> EdgeInsets {
    // The start edge gets the gutter.
    EdgeInsets { top: 4, start: gutter, bottom: 4, end: 8 }
}

fn qualified(left: i32, right: i32) -> rustnative_core::EdgeInsets {
    rustnative_core::EdgeInsets { start: left, end: right, ..EdgeInsets::all(0) }
}

fn start_of() -> i32 {
    EdgeInsets::all(2).start + EdgeInsets { top: 0, start: 1, bottom: 0, end: 1 }.end
}

fn style() -> LayoutStyle {
    LayoutStyle::default().with_padding(EdgeInsets {
        top: 0,
        start: 12, // indented
        bottom: 0,
        end: 12,
    })
}
