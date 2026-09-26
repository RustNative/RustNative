//! Nodes handed from one component to another keep their identity: a
//! parent's child components passed in a sibling's props (a card whose
//! body holds the parent's badges) are not scoped a second time, which
//! would keep only their local keys and make them collide.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use rustnative_core::{Component, ComponentContext, ComponentTree, Event, Node, TreeSnapshot};

#[derive(Clone, PartialEq)]
struct Text(String);

struct Badge(Text);

impl Component for Badge {
    type Props = Text;
    type Message = ();
    fn new(props: Text) -> Self {
        Self(props)
    }
    fn props(&self) -> &Text {
        &self.0
    }
    fn set_props(&mut self, props: Text) {
        self.0 = props;
    }
    fn view(&self) -> Node {
        Node::label("badge", self.0.0.clone())
    }
    fn update(&mut self, _: Event) {}
}

#[derive(Clone, PartialEq)]
struct Body(Vec<Node>);

struct Card(Body);

impl Component for Card {
    type Props = Body;
    type Message = ();
    fn new(props: Body) -> Self {
        Self(props)
    }
    fn props(&self) -> &Body {
        &self.0
    }
    fn set_props(&mut self, props: Body) {
        self.0 = props;
    }
    fn view(&self) -> Node {
        Node::column("card", self.0.0.clone())
    }
    fn update(&mut self, _: Event) {}
}

struct Page;

impl Component for Page {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("page", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let badges = ["one", "two"]
            .map(|key| context.child_with_props::<Badge, _>(key, Text(key.into()), Badge::new));
        let card = context.child_with_props::<Card, _>("card", Body(badges.to_vec()), Card::new);
        Node::column("page", [card])
    }
}

#[test]
fn nodes_passed_through_props_keep_their_identity() {
    let mut tree = ComponentTree::new(Page::new(()));
    let _ = tree.render();
    let view = tree.view();
    let snapshot = TreeSnapshot::from_node(&view).expect("no two nodes share an identity");
    let badges =
        snapshot.nodes().filter(|node| node.id.local_key().as_deref() == Some("badge")).count();
    assert_eq!(badges, 2);
}
