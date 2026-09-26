use rustnative_core::{Node, styles};

fn main() {
    let _ = Node::label("l", "x").with_declarations(styles!("colr: red"));
}
