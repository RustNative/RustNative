use rustnative_core::{Node, classes};

fn main() {
    let _ = Node::label("l", "x").with_class(classes!("hover:p-4"));
}
