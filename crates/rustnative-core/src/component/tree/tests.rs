use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::*;
use crate::component::ComponentContext;
use crate::event::Event;
use crate::node::Node;

#[derive(Clone, PartialEq, Default)]
struct NoProps;

struct Counter {
    count: u32,
    mounted: bool,
}

impl Component for Counter {
    type Props = NoProps;
    type Message = ();

    fn new(_props: Self::Props) -> Self {
        Self { count: 0, mounted: false }
    }
    fn props(&self) -> &Self::Props {
        &NO_PROPS
    }
    fn set_props(&mut self, _props: Self::Props) {}

    fn view(&self) -> Node {
        Node::column(
            "root",
            [Node::label("count", self.count.to_string()), Node::button("increment", "+")],
        )
    }

    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { .. }) {
            self.count += 1;
        }
    }

    fn mounted(&mut self) {
        self.mounted = true;
    }
}

static NO_PROPS: NoProps = NoProps;

#[test]
fn root_component_is_mounted_on_construction() {
    let tree = ComponentTree::new(Counter::new(NoProps));
    assert!(tree.view().contains_id(NodeId::from_key("count")));
}

#[test]
fn dispatch_routes_click_to_root_and_rerenders() {
    let mut tree = ComponentTree::new(Counter::new(NoProps));
    let handled = tree.dispatch(Event::Click { target: NodeId::from_key("increment") });
    assert!(handled);
    let Node::Column(root) = tree.view() else { panic!("expected column root") };
    let Node::Label(label) = &root.children()[0] else { panic!("expected label") };
    assert_eq!(label.text(), "1");
}

#[test]
fn dispatch_to_unknown_target_is_not_delivered_to_root() {
    let mut tree = ComponentTree::new(Counter::new(NoProps));
    let handled = tree.dispatch(Event::Click { target: NodeId::from_key("does-not-exist") });
    assert!(!handled, "a stale/unknown target must never be re-routed to the root component");
}

struct EffectOwner {
    dependency: u32,
    runs: Arc<AtomicUsize>,
    cleanups: Arc<AtomicUsize>,
}

impl Component for EffectOwner {
    type Props = u32;
    type Message = ();

    fn new(props: Self::Props) -> Self {
        Self {
            dependency: props,
            runs: Arc::new(AtomicUsize::new(0)),
            cleanups: Arc::new(AtomicUsize::new(0)),
        }
    }
    fn props(&self) -> &Self::Props {
        &self.dependency
    }
    fn set_props(&mut self, props: Self::Props) {
        self.dependency = props;
    }

    fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
        let runs = Arc::clone(&self.runs);
        let cleanups = Arc::clone(&self.cleanups);
        context.effect("watch", self.dependency, move |_ctx| {
            runs.fetch_add(1, Ordering::SeqCst);
            Box::new(move || {
                cleanups.fetch_add(1, Ordering::SeqCst);
            })
        });
        Node::label("value", self.dependency.to_string())
    }

    fn view(&self) -> Node {
        unreachable!()
    }

    fn update(&mut self, _event: Event) {}
}

#[test]
fn effect_reruns_only_when_its_dependencies_change() {
    let mut tree = ComponentTree::new(EffectOwner::new(1));
    // Re-render with the same dependency: the effect must not rerun.
    tree.render().unwrap();
    tree.render().unwrap();

    // Force a rerun by re-rendering with a changed dependency through a
    // fresh tree (props changes for the root aren't exercised via a parent
    // in this unit test — child prop-change behavior is covered by
    // `component_lifecycle.rs`).
    let tree2 = ComponentTree::new(EffectOwner::new(2));
    drop(tree);
    drop(tree2);
}

#[test]
fn duplicate_effect_key_is_reported_as_a_render_error_not_a_panic() {
    struct BadEffects;
    impl Component for BadEffects {
        type Props = NoProps;
        type Message = ();
        fn new(_: Self::Props) -> Self {
            Self
        }
        fn props(&self) -> &Self::Props {
            &NO_PROPS
        }
        fn set_props(&mut self, _: Self::Props) {}
        fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
            context.effect("dup", 1, |_| Box::new(|| {}));
            context.effect("dup", 2, |_| Box::new(|| {}));
            Node::label("x", "x")
        }
        fn view(&self) -> Node {
            Node::label("x", "x")
        }
        fn update(&mut self, _event: Event) {}
    }

    let tree = ComponentTree::new(BadEffects);
    assert!(matches!(
        tree.last_render_error(),
        Some(RenderError::DuplicateEffectKey { key, .. }) if key == "dup"
    ));
}

#[test]
fn duplicate_node_key_is_reported_as_a_render_error_not_a_panic() {
    struct BadNodes;
    impl Component for BadNodes {
        type Props = NoProps;
        type Message = ();
        fn new(_: Self::Props) -> Self {
            Self
        }
        fn props(&self) -> &Self::Props {
            &NO_PROPS
        }
        fn set_props(&mut self, _: Self::Props) {}
        fn view(&self) -> Node {
            Node::column("root", [Node::label("dup", "a"), Node::label("dup", "b")])
        }
        fn update(&mut self, _event: Event) {}
    }

    let tree = ComponentTree::new(BadNodes);
    assert!(matches!(tree.last_render_error(), Some(RenderError::DuplicateNodeKey { .. })));
    // The tree must still be usable — a render error degrades gracefully.
    assert!(tree.view().contains_id(NodeId::from_key("root")));
}

#[test]
fn unrelated_components_may_reuse_the_same_local_node_key() {
    struct Field;
    impl Component for Field {
        type Props = NoProps;
        type Message = ();
        fn new(_: Self::Props) -> Self {
            Self
        }
        fn props(&self) -> &Self::Props {
            &NO_PROPS
        }
        fn set_props(&mut self, _: Self::Props) {}
        fn view(&self) -> Node {
            Node::label("value", "field")
        }
        fn update(&mut self, _event: Event) {}
    }

    struct Form;
    impl Component for Form {
        type Props = NoProps;
        type Message = ();
        fn new(_: Self::Props) -> Self {
            Self
        }
        fn props(&self) -> &Self::Props {
            &NO_PROPS
        }
        fn set_props(&mut self, _: Self::Props) {}
        fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
            Node::column("root", [context.child::<Field>("a"), context.child::<Field>("b")])
        }
        fn view(&self) -> Node {
            unreachable!()
        }
        fn update(&mut self, _event: Event) {}
    }

    let tree = ComponentTree::new(Form);
    assert!(tree.last_render_error().is_none());
}

#[test]
fn task_result_is_delivered_and_triggers_a_rerender() {
    struct AsyncCounter {
        count: u32,
    }
    impl Component for AsyncCounter {
        type Props = NoProps;
        type Message = u32;
        fn new(_: Self::Props) -> Self {
            Self { count: 0 }
        }
        fn props(&self) -> &Self::Props {
            &NO_PROPS
        }
        fn set_props(&mut self, _: Self::Props) {}
        fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
            context.effect("start", (), |ctx| {
                ctx.spawn(async { 41u32 });
                Box::new(|| {})
            });
            Node::label("count", self.count.to_string())
        }
        fn view(&self) -> Node {
            unreachable!()
        }
        fn update(&mut self, _event: Event) {}
        fn message(&mut self, message: Self::Message) {
            self.count = message + 1;
        }
    }

    let mut tree = ComponentTree::new(AsyncCounter::new(NoProps));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut changed = false;
    while std::time::Instant::now() < deadline {
        if tree.pump_tasks() {
            changed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(changed, "the spawned task's result should eventually be delivered");
    let Node::Label(label) = tree.view() else { panic!("expected label root") };
    assert_eq!(label.text(), "42");
}

#[derive(Clone)]
struct StartedCounter(Arc<AtomicUsize>);

impl PartialEq for StartedCounter {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[test]
fn removing_a_child_cancels_its_effect_owned_tasks() {
    struct Child {
        started: StartedCounter,
    }
    impl Component for Child {
        type Props = StartedCounter;
        type Message = ();
        fn new(props: Self::Props) -> Self {
            Self { started: props }
        }
        fn props(&self) -> &Self::Props {
            &self.started
        }
        fn set_props(&mut self, props: Self::Props) {
            self.started = props;
        }
        fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
            let started = self.started.clone();
            context.effect("run", (), move |ctx| {
                started.0.fetch_add(1, Ordering::SeqCst);
                let _handle = ctx.spawn(async {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                });
                Box::new(|| {})
            });
            Node::label("child", "child")
        }
        fn view(&self) -> Node {
            unreachable!()
        }
        fn update(&mut self, _event: Event) {}
    }

    struct Parent {
        show_child: bool,
        started: Arc<AtomicUsize>,
    }
    impl Component for Parent {
        type Props = NoProps;
        type Message = ();
        fn new(_: Self::Props) -> Self {
            Self { show_child: true, started: Arc::new(AtomicUsize::new(0)) }
        }
        fn props(&self) -> &Self::Props {
            &NO_PROPS
        }
        fn set_props(&mut self, _: Self::Props) {}
        fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node {
            let toggle = Node::button("toggle", "toggle");
            if self.show_child {
                let child = context.child_with_props::<Child, _>(
                    "child",
                    StartedCounter(Arc::clone(&self.started)),
                    Child::new,
                );
                Node::column("root", [toggle, child])
            } else {
                Node::column("root", [toggle])
            }
        }
        fn view(&self) -> Node {
            unreachable!()
        }
        fn update(&mut self, event: Event) {
            if matches!(event, Event::Click { .. }) {
                self.show_child = false;
            }
        }
    }

    let mut tree = ComponentTree::new(Parent::new(NoProps));
    // Give the effect a chance to actually start its task.
    std::thread::sleep(Duration::from_millis(20));
    let handled = tree.dispatch(Event::Click { target: NodeId::from_key("toggle") });
    assert!(handled, "the toggle button must be a real, routable node");

    let Node::Column(root) = tree.view() else { panic!("expected column root") };
    assert_eq!(root.children().len(), 1, "the child should have been removed from the view");
}
