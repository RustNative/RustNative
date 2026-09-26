//! The single-threaded executor seam and the host clock, through the public
//! API: a component spawns a `!Send` future, which is delivered as a
//! message, cancelled with its owner, and timed on the same virtual clock
//! as its delays.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use rustnative_core::{
    Component, ComponentContext, ComponentTree, Event, ManualExecutor, Node, Scheduler, Services,
    Theme,
};

thread_local! {
    /// How many times any worker's local future has been polled past each
    /// await point, observed from outside the component.
    static STEPS: Cell<u32> = const { Cell::new(0) };
}

#[derive(Clone, Default, PartialEq)]
struct ChildProps;

struct LocalWorker {
    polled: Rc<Cell<u32>>,
    started: bool,
    received: Option<Duration>,
}

impl Component for LocalWorker {
    type Props = ChildProps;
    type Message = Duration;

    fn new(_props: ChildProps) -> Self {
        Self { polled: Rc::new(Cell::new(0)), started: false, received: None }
    }
    fn props(&self) -> &ChildProps {
        static PROPS: ChildProps = ChildProps;
        &PROPS
    }
    fn set_props(&mut self, _props: ChildProps) {}
    fn view(&self) -> Node {
        Node::label("worker", format!("{:?}", self.received))
    }
    fn update(&mut self, _event: Event) {}
    fn message(&mut self, at: Duration) {
        self.received = Some(at);
    }
    fn render(&mut self, context: &mut ComponentContext<'_, Duration>) -> Node {
        if !self.started {
            self.started = true;
            // `Rc` makes this future `!Send`: only the local executor can
            // run it.
            let polled = Rc::clone(&self.polled);
            let delay = context.sleep(Duration::from_secs(2));
            context.spawn_local(async move {
                polled.set(polled.get() + 1);
                STEPS.with(|steps| steps.set(steps.get() + 1));
                delay.await;
                polled.set(polled.get() + 1);
                STEPS.with(|steps| steps.set(steps.get() + 1));
                Duration::from_secs(2)
            });
        }
        self.view()
    }
}

#[derive(Clone, Default, PartialEq)]
struct RootProps;

struct Root {
    show: bool,
}

impl Component for Root {
    type Props = RootProps;
    type Message = ();

    fn new(_props: RootProps) -> Self {
        Self { show: true }
    }
    fn props(&self) -> &RootProps {
        static PROPS: RootProps = RootProps;
        &PROPS
    }
    fn set_props(&mut self, _props: RootProps) {}
    fn view(&self) -> Node {
        Node::column("root", [])
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { .. }) {
            self.show = false;
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let mut children = vec![Node::button("hide", "Hide")];
        if self.show {
            children.push(context.child::<LocalWorker>("worker"));
        }
        Node::column("root", children)
    }
}

fn tree(executor: &ManualExecutor) -> ComponentTree {
    ComponentTree::with_scheduler(
        Root::new(RootProps),
        Services::default(),
        Theme::default(),
        Scheduler::with_executor(Arc::new(executor.clone())),
    )
}

fn worker_text(tree: &ComponentTree) -> String {
    let mut text = String::new();
    tree.view().visit(&mut |node, _, _| {
        if let Node::Label(label) = node {
            label.text().clone_into(&mut text);
        }
    });
    text
}

#[test]
fn a_local_task_is_delivered_as_a_message_on_virtual_time() {
    let executor = ManualExecutor::new();
    let mut tree = tree(&executor);
    tree.pump_tasks();
    assert_eq!(worker_text(&tree), "None", "the delay has not elapsed");

    executor.advance(Duration::from_secs(2));
    assert!(tree.pump_tasks(), "the completed local task changes state");
    assert_eq!(worker_text(&tree), "Some(2s)");
}

#[test]
fn a_local_task_is_cancelled_when_its_owner_unmounts() {
    let executor = ManualExecutor::new();
    let mut tree = tree(&executor);
    tree.pump_tasks();
    STEPS.with(|steps| assert_eq!(steps.get(), 1, "the task ran up to its delay"));

    tree.dispatch(Event::Click { target: rustnative_core::NodeId::from_key("hide") });
    executor.advance(Duration::from_secs(5));
    tree.pump_tasks();
    // Nothing to observe in the view (the worker is gone); what matters is
    // that no message reached a component that no longer exists — which a
    // debug assertion inside `pump_tasks` would report — and that the
    // task left nothing behind.
    assert!(!worker_text(&tree).contains("Some"));
    STEPS.with(|steps| assert_eq!(steps.get(), 1, "the cancelled future never resumed"));
}

#[test]
fn now_and_sleep_read_the_same_clock() {
    let executor = ManualExecutor::new();
    let scheduler = Scheduler::with_executor(Arc::new(executor.clone()));
    assert_eq!(scheduler.now(), Duration::ZERO);
    executor.advance(Duration::from_millis(1_500));
    assert_eq!(scheduler.now(), Duration::from_millis(1_500));
}
