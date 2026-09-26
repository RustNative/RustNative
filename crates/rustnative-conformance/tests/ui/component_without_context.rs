use rustnative_core::{Component, Event, Node, rsx};

struct Card;
impl Component for Card {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self { Self }
    fn props(&self) -> &() { &() }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node { Node::label("x", "") }
    fn update(&mut self, _: Event) {}
}

fn main() {
    let _ = rsx! { <Column key="root"><Card key="card" /></Column> };
}
