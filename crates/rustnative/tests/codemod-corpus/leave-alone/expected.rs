use rustnative_core::EdgeInsets;

struct Margins {
    left: i32,
    right: i32,
}

fn unrelated() -> Margins {
    Margins { left: 1, right: 2 }
}

fn unknown(insets: &EdgeInsets) -> i32 {
    insets.left
}

fn destructure(insets: EdgeInsets) -> i32 {
    let EdgeInsets { left, .. } = insets;
    left
}
