//! The accessibility model (Milestone 26) through the component runtime:
//! relationship keys are scoped like node keys, and assistive-technology
//! actions reach the owning component under its own local keys.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::cell::RefCell;
use std::rc::Rc;

use rustnative_core::{
    AccessibilityInfo, AccessibilityRole, AccessibilityTree, AccessibleAction,
    AccessibleActionKind, Application, Component, ComponentContext, Event, Node, NodeId, Relation,
    Size, TreeSnapshot, Window,
};

type Log = Rc<RefCell<Vec<String>>>;

/// A reusable field: a caption and an input labelled by it, both keyed
/// with ordinary local keys that another instance also uses.
struct Field {
    props: (String, Log),
}

impl Component for Field {
    type Props = (String, Log);
    type Message = ();

    fn new(props: Self::Props) -> Self {
        Self { props }
    }
    fn props(&self) -> &Self::Props {
        &self.props
    }
    fn set_props(&mut self, props: Self::Props) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::column(
            "field",
            [
                Node::label("caption", self.props.0.clone()),
                Node::column("input", []).with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::Slider)
                        .labelled_by("caption")
                        .range(0.0, 10.0, 5.0, 1.0)
                        .action(AccessibleActionKind::SetValue),
                ),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        if let Event::AccessibilityAction { target, element, action } = event {
            self.props
                .1
                .borrow_mut()
                .push(format!("{}:{target:?}:{element:?}:{action:?}", self.props.0));
        }
    }
}

struct Form {
    log: Log,
}

impl Component for Form {
    type Props = Log;
    type Message = ();

    fn new(log: Log) -> Self {
        Self { log }
    }
    fn props(&self) -> &Log {
        &self.log
    }
    fn set_props(&mut self, log: Log) {
        self.log = log;
    }
    fn view(&self) -> Node {
        Node::column("form", [])
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let first =
            context.child_with_props("first", ("First".into(), self.log.clone()), Field::new);
        let second =
            context.child_with_props("second", ("Second".into(), self.log.clone()), Field::new);
        Node::column("form", [first, second])
    }
    fn update(&mut self, _event: Event) {}
}

fn sliders(application: &Application) -> (AccessibilityTree, Vec<NodeId>) {
    let snapshot = TreeSnapshot::from_node(&application.view()).unwrap();
    let tree = AccessibilityTree::from_snapshot(&snapshot);
    let ids = snapshot
        .ordered_nodes()
        .into_iter()
        .filter(|node| node.accessibility.role() == AccessibilityRole::Slider)
        .map(|node| node.id)
        .collect();
    (tree, ids)
}

#[test]
fn a_relationship_inside_a_reused_component_resolves_to_that_instances_node() {
    let log = Log::default();
    let application = Application::new(Form::new(log), Window::new("t", Size::new(10, 10)));
    let (tree, ids) = sliders(&application);
    assert_eq!(ids.len(), 2);
    let names: Vec<_> = ids.iter().map(|id| tree.name_of(*id)).collect();
    assert_eq!(
        names,
        [Some("First".to_owned()), Some("Second".to_owned())],
        "each instance's `labelled_by(\"caption\")` must reach its own caption, not the other's"
    );
    for id in ids {
        let label = tree.resolve(id, Relation::LabelledBy);
        assert_eq!(label.len(), 1);
        assert_ne!(label[0], NodeId::from_key("caption"), "the target was scoped");
    }
}

#[test]
fn an_accessibility_action_reaches_the_owner_under_its_local_key() {
    let log = Log::default();
    let mut application =
        Application::new(Form::new(log.clone()), Window::new("t", Size::new(10, 10)));
    let (_, ids) = sliders(&application);
    assert!(application.dispatch(Event::AccessibilityAction {
        target: ids[1],
        element: None,
        action: AccessibleAction::SetRangeValue(7.0.into()),
    }));
    let entry = log.borrow()[0].clone();
    assert!(entry.starts_with("Second:"), "{entry}");
    assert!(
        entry.contains(&format!("{:?}", NodeId::from_key("input"))),
        "the component sees its own local key: {entry}"
    );
    assert!(entry.ends_with("SetRangeValue(Scalar(7.0))"), "{entry}");
}
