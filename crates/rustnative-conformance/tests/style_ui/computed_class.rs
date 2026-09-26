use rustnative_core::{Node, classes};

fn main() {
    let color = "bg-blue-500";
    let _ = Node::label("l", "x").with_class(classes!(color));
}
