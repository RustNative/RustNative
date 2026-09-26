//! Scope-bound cancellation as a property (`PLAN.md` Milestone 41): for
//! random sequences of mounting, unmounting, and time passing, no task ever
//! delivers to a component instance after that instance unmounted — and
//! every instance still mounted when its task comes due does receive it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use proptest::prelude::*;
use rustnative_core::{Component, ComponentContext, Event, Node, NodeId, Size, Window};
use rustnative_headless::{HeadlessApp, Query};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Record {
    Mounted(u32),
    Delivered(u32),
    Unmounted(u32),
}

#[derive(Clone, Default)]
struct Journal {
    records: Rc<RefCell<Vec<Record>>>,
    next: Rc<Cell<u32>>,
}

impl PartialEq for Journal {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.records, &other.records)
    }
}

struct Worker {
    props: (Journal, u64),
    id: u32,
}

impl Component for Worker {
    type Props = (Journal, u64);
    type Message = u32;
    fn new((journal, delay): (Journal, u64)) -> Self {
        let id = journal.next.get();
        journal.next.set(id + 1);
        journal.records.borrow_mut().push(Record::Mounted(id));
        Self { props: (journal, delay), id }
    }
    fn props(&self) -> &(Journal, u64) {
        &self.props
    }
    fn set_props(&mut self, _: (Journal, u64)) {}
    fn view(&self) -> Node {
        Node::label("worker", "working")
    }
    fn render(&mut self, context: &mut ComponentContext<'_, u32>) -> Node {
        let (id, delay) = (self.id, self.props.1);
        context.effect("work", (), move |effects| {
            let sleep = effects.sleep(Duration::from_millis(delay));
            effects.spawn(async move {
                sleep.await;
                id
            });
            Box::new(|| {})
        });
        self.view()
    }
    fn update(&mut self, _: Event) {}
    fn message(&mut self, id: u32) {
        self.props.0.records.borrow_mut().push(Record::Delivered(id));
    }
    fn unmounted(&mut self) {
        self.props.0.records.borrow_mut().push(Record::Unmounted(self.id));
    }
}

struct Board {
    journal: Journal,
    slots: [Option<u64>; 3],
    generation: [u32; 3],
}

impl Component for Board {
    type Props = Journal;
    type Message = ();
    fn new(journal: Journal) -> Self {
        Self { journal, slots: [None; 3], generation: [0; 3] }
    }
    fn props(&self) -> &Journal {
        &self.journal
    }
    fn set_props(&mut self, journal: Journal) {
        self.journal = journal;
    }
    fn view(&self) -> Node {
        Node::label("unused", "")
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let mut children: Vec<Node> =
            (0..3).map(|slot| Node::button(format!("toggle-{slot}"), "Toggle")).collect();
        for (slot, delay) in self.slots.iter().enumerate() {
            if let Some(delay) = delay {
                // A fresh key per mount, so a remount is a new instance.
                let key = format!("worker-{slot}-{}", self.generation[slot]);
                children.push(context.child_with_props(
                    key,
                    (self.journal.clone(), *delay),
                    Worker::new,
                ));
            }
        }
        Node::column("board", children)
    }
    fn update(&mut self, event: Event) {
        for slot in 0..3 {
            if matches!(&event, Event::Click { target } if *target == NodeId::from_key(&format!("toggle-{slot}")))
            {
                if self.slots[slot].is_some() {
                    self.slots[slot] = None;
                } else {
                    self.generation[slot] += 1;
                    self.slots[slot] = Some(10 + 17 * u64::from(self.generation[slot] % 5));
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
enum Step {
    Toggle(usize),
    Advance(u64),
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![(0..3_usize).prop_map(Step::Toggle), (0..60_u64).prop_map(Step::Advance)]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn no_task_delivers_after_its_owner_unmounts(steps in proptest::collection::vec(step(), 1..40)) {
        let journal = Journal::default();
        let root = journal.clone();
        let mut app = HeadlessApp::launch(Window::new("board", Size::new(300, 400)), move || Board::new(root.clone()));
        for step in &steps {
            match step {
                Step::Toggle(slot) => app.click(&Query::key(format!("toggle-{slot}"))).unwrap(),
                Step::Advance(ms) => app.advance(Duration::from_millis(*ms)),
            }
        }
        // Let everything still pending come due.
        app.advance(Duration::from_millis(500));
        let records = journal.records.borrow().clone();
        for (index, record) in records.iter().enumerate() {
            if let Record::Unmounted(id) = record {
                prop_assert!(
                    !records[index..].contains(&Record::Delivered(*id)),
                    "instance {id} received its task after unmounting: {records:?}"
                );
            }
        }
        // Not vacuous: every instance never unmounted received its task.
        for record in &records {
            if let Record::Mounted(id) = record {
                let unmounted = records.contains(&Record::Unmounted(*id));
                let delivered = records.contains(&Record::Delivered(*id));
                prop_assert!(unmounted || delivered, "instance {id} neither delivered nor unmounted: {records:?}");
            }
        }
    }
}
