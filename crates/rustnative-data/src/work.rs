//! Constrained background work (`PLAN.md` Milestone 47): jobs that wait
//! for the network, for external power, or until a deadline — the
//! contract both mobile hosts impose on background work, stated portably.
//!
//! On a desktop there is no scheduler outside the process for an ordinary
//! application: work runs while the application runs, when its constraints
//! hold. The host answers the constraints through [`Conditions`] — on
//! Windows, `rustnative_windows::WindowsConditions` asks the power and
//! network APIs.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use rustnative_core::{Background, TaskHandle};

use crate::query::LocalFuture;

/// What must hold before a job runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Constraints {
    /// The network is reachable.
    pub network: bool,
    /// The device is on external power.
    pub charging: bool,
    /// Run by this time (on the scheduler's clock) even if the other
    /// constraints never hold.
    pub deadline: Option<Duration>,
}

/// The host's answers about the device.
pub trait Conditions: Send + Sync {
    /// Whether the network is reachable.
    fn network(&self) -> bool;
    /// Whether the device is on external power.
    fn charging(&self) -> bool;
}

/// Conditions that are whatever they are set to — for tests and hosts
/// without the APIs.
#[derive(Debug, Default)]
pub struct FixedConditions {
    network: std::sync::atomic::AtomicBool,
    charging: std::sync::atomic::AtomicBool,
}

impl FixedConditions {
    /// Conditions with these answers.
    #[must_use]
    pub fn new(network: bool, charging: bool) -> Self {
        Self { network: network.into(), charging: charging.into() }
    }

    /// Changes the network answer.
    pub fn set_network(&self, network: bool) {
        self.network.store(network, std::sync::atomic::Ordering::Relaxed);
    }

    /// Changes the power answer.
    pub fn set_charging(&self, charging: bool) {
        self.charging.store(charging, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Conditions for FixedConditions {
    fn network(&self) -> bool {
        self.network.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn charging(&self) -> bool {
        self.charging.load(std::sync::atomic::Ordering::Relaxed)
    }
}

type Job = Box<dyn FnOnce() -> LocalFuture<()>>;

struct Waiting {
    name: String,
    constraints: Constraints,
    job: Job,
}

/// Jobs waiting for their constraints; see the [module
/// documentation](self).
pub struct BackgroundWork {
    background: Background,
    conditions: Arc<dyn Conditions>,
    waiting: Rc<RefCell<Vec<Waiting>>>,
    ran: Rc<RefCell<Vec<String>>>,
    poller: RefCell<Option<TaskHandle>>,
}

impl fmt::Debug for BackgroundWork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BackgroundWork")
            .field("waiting", &self.waiting.borrow().len())
            .finish_non_exhaustive()
    }
}

impl BackgroundWork {
    /// A work queue asking `conditions`, checked every `every`.
    pub fn new(
        background: &Background,
        conditions: Arc<dyn Conditions>,
        every: Duration,
    ) -> Rc<Self> {
        let work = Rc::new(Self {
            background: background.clone(),
            conditions,
            waiting: Rc::new(RefCell::new(Vec::new())),
            ran: Rc::new(RefCell::new(Vec::new())),
            poller: RefCell::new(None),
        });
        let weak = Rc::downgrade(&work);
        let sleeper = background.clone();
        let poller = background.spawn_local(async move {
            loop {
                sleeper.sleep(every).await;
                match weak.upgrade() {
                    Some(work) => work.check(),
                    None => break,
                }
            }
        });
        *work.poller.borrow_mut() = Some(poller);
        work
    }

    /// Queues `job` to run once `constraints` hold.
    pub fn schedule<F, Fut>(&self, name: impl Into<String>, constraints: Constraints, job: F)
    where
        F: FnOnce() -> Fut + 'static,
        Fut: std::future::Future<Output = ()> + 'static,
    {
        self.waiting.borrow_mut().push(Waiting {
            name: name.into(),
            constraints,
            job: Box::new(move || Box::pin(job())),
        });
        self.check();
    }

    /// Runs every job whose constraints hold now.
    pub fn check(&self) {
        let now = self.background.now();
        let network = self.conditions.network();
        let charging = self.conditions.charging();
        let ready = {
            let mut waiting = self.waiting.borrow_mut();
            let (ready, rest): (Vec<_>, Vec<_>) = waiting.drain(..).partition(|job| {
                let due = job.constraints.deadline.is_some_and(|deadline| now >= deadline);
                due || ((!job.constraints.network || network)
                    && (!job.constraints.charging || charging))
            });
            *waiting = rest;
            ready
        };
        for job in ready {
            self.ran.borrow_mut().push(job.name);
            self.background.spawn_local((job.job)());
        }
    }

    /// The names of the jobs started so far, in order.
    #[must_use]
    pub fn started(&self) -> Vec<String> {
        self.ran.borrow().clone()
    }

    /// How many jobs are waiting.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.waiting.borrow().len()
    }
}

impl Drop for BackgroundWork {
    fn drop(&mut self) {
        if let Some(poller) = self.poller.borrow_mut().take() {
            poller.cancel();
        }
    }
}
