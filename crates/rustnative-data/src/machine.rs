//! State machines on enums (`PLAN.md` Milestone 47, `C10`), whose states
//! own their work: entering a state may start tasks, and leaving it cancels
//! them.

use std::cell::RefCell;
use std::fmt;
use std::future::Future;
use std::rc::Rc;

use rustnative_core::{Background, TaskHandle};

/// A state of a [`StateMachine`], usually an enum.
pub trait MachineState: Clone + PartialEq + fmt::Debug + 'static {
    /// What the machine reacts to.
    type Event;

    /// The state `event` leads to from this one, or `None` when this state
    /// ignores it.
    fn next(&self, event: &Self::Event) -> Option<Self>;

    /// The state's name in the diagram.
    fn name(&self) -> &'static str;

    /// Every transition, as `(from, event, to)` names, for the diagram.
    fn transitions() -> &'static [(&'static str, &'static str, &'static str)];

    /// Runs on entering this state. Tasks started through `scope` are
    /// cancelled when the machine leaves it.
    fn enter(&self, scope: &StateScope) {
        let _ = scope;
    }
}

/// The work belonging to one visit to one state.
pub struct StateScope {
    background: Background,
    tasks: RefCell<Vec<TaskHandle>>,
}

impl fmt::Debug for StateScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StateScope")
            .field("tasks", &self.tasks.borrow().len())
            .finish_non_exhaustive()
    }
}

impl StateScope {
    /// Starts `work`, cancelled when the machine leaves the state.
    pub fn spawn(&self, work: impl Future<Output = ()> + 'static) {
        let handle = self.background.spawn_local(work);
        self.tasks.borrow_mut().push(handle);
    }

    /// The background work handle, for delays and offloading.
    #[must_use]
    pub fn background(&self) -> &Background {
        &self.background
    }
}

impl Drop for StateScope {
    fn drop(&mut self) {
        for task in self.tasks.borrow_mut().drain(..) {
            task.cancel();
        }
    }
}

/// A current state and its scope.
///
/// ```
/// use rustnative_data::{MachineState, StateMachine};
///
/// #[derive(Debug, Clone, PartialEq)]
/// enum Upload { Idle, Sending, Done }
/// enum Event { Start, Finish, Reset }
///
/// impl MachineState for Upload {
///     type Event = Event;
///     fn next(&self, event: &Event) -> Option<Self> {
///         match (self, event) {
///             (Self::Idle, Event::Start) => Some(Self::Sending),
///             (Self::Sending, Event::Finish) => Some(Self::Done),
///             (_, Event::Reset) => Some(Self::Idle),
///             _ => None,
///         }
///     }
///     fn name(&self) -> &'static str {
///         match self { Self::Idle => "Idle", Self::Sending => "Sending", Self::Done => "Done" }
///     }
///     fn transitions() -> &'static [(&'static str, &'static str, &'static str)] {
///         &[("Idle", "Start", "Sending"), ("Sending", "Finish", "Done"), ("Done", "Reset", "Idle")]
///     }
/// }
///
/// let diagram = StateMachine::<Upload>::to_mermaid();
/// assert!(diagram.contains("Idle --> Sending : Start"));
/// ```
pub struct StateMachine<S: MachineState> {
    state: S,
    scope: Option<Rc<StateScope>>,
    background: Option<Background>,
}

impl<S: MachineState> fmt::Debug for StateMachine<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StateMachine").field("state", &self.state).finish_non_exhaustive()
    }
}

impl<S: MachineState> StateMachine<S> {
    /// A machine in `initial`, which is entered now when `background` is
    /// given (the state's work needs somewhere to run).
    pub fn new(initial: S, background: Option<Background>) -> Self {
        let mut machine = Self { state: initial, scope: None, background };
        machine.enter();
        machine
    }

    fn enter(&mut self) {
        self.scope = None; // leaving: the previous state's tasks end here
        if let Some(background) = &self.background {
            let scope = Rc::new(StateScope {
                background: background.clone(),
                tasks: RefCell::new(Vec::new()),
            });
            self.state.enter(&scope);
            self.scope = Some(scope);
        }
    }

    /// The current state.
    pub fn state(&self) -> &S {
        &self.state
    }

    /// Feeds `event`; returns whether the state changed.
    pub fn send(&mut self, event: &S::Event) -> bool {
        match self.state.next(event) {
            Some(next) if next != self.state => {
                self.state = next;
                self.enter();
                true
            }
            _ => false,
        }
    }

    /// The machine's diagram, in Mermaid's state-diagram syntax.
    #[must_use]
    pub fn to_mermaid() -> String {
        use std::fmt::Write as _;
        let mut diagram = String::from("stateDiagram-v2\n");
        for (from, event, to) in S::transitions() {
            let _ = writeln!(diagram, "    {from} --> {to} : {event}");
        }
        diagram
    }
}
