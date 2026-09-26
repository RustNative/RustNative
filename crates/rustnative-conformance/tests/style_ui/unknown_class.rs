use rustnative_core::{Node, classes};

fn main() {
    let _ = Node::label("l", "x").with_class(classes!("p-4 bg-bleu-500"));
}
