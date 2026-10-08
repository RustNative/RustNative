//! Constrained background work through `JobScheduler` (`M-FP-2`): a job
//! waits for the network, external power, or a deadline, and runs even
//! when the application is not — in a process `JobScheduler` starts with
//! no activity.
//!
//! Because that process never runs `main`, a job's handler is registered
//! where every process start reaches it: in the function named by
//! `export_main!(main, work = register_work)`, which runs when the library
//! loads.
//!
//! ```no_run
//! # #[cfg(target_os = "android")]
//! # mod example {
//! use rustnative_android::AndroidWork;
//! use rustnative_data::Constraints;
//!
//! fn register_work() {
//!     AndroidWork::register("sync", || {
//!         // Upload what changed; `false`: done.
//!         false
//!     });
//! }
//!
//! fn schedule() {
//!     let constraints = Constraints { network: true, ..Constraints::default() };
//!     AndroidWork::schedule("sync", constraints).expect("scheduled");
//! }
//! # }
//! ```

use std::collections::HashMap;
use std::sync::Mutex;

use rustnative_core::ServiceError;
use rustnative_data::Constraints;

use super::{checked, java};
use crate::jni_host::{Arg, Class};

/// A job's handler: runs on a thread of its own; `true` asks
/// `JobScheduler` to run it again later (with back-off).
pub type JobHandler = fn() -> bool;

static HANDLERS: Mutex<Option<HashMap<String, JobHandler>>> = Mutex::new(None);

/// Jobs `JobScheduler` runs.
#[derive(Debug, Clone, Copy, Default)]
pub struct AndroidWork;

/// The job id of `name`: stable, so scheduling a name again replaces it.
pub(crate) fn job_id(name: &str) -> i32 {
    let hash = name
        .bytes()
        .fold(0x811c_9dc5_u32, |hash, byte| (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193));
    i32::try_from(hash >> 1).unwrap_or(i32::MAX)
}

impl AndroidWork {
    /// Registers `handler` as job `name`'s work. Call it from the function
    /// `export_main!(main, work = …)` names, so a process started for the
    /// job knows it.
    pub fn register(name: &str, handler: JobHandler) {
        let mut handlers = HANDLERS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        handlers.get_or_insert_with(HashMap::new).insert(name.to_owned(), handler);
    }

    /// Schedules job `name` to run once `constraints` hold (or at their
    /// deadline). Scheduling it again replaces the pending one.
    ///
    /// # Errors
    ///
    /// `JobScheduler` refused it.
    pub fn schedule(name: &str, constraints: Constraints) -> Result<(), ServiceError> {
        let deadline = constraints
            .deadline
            .map_or(-1, |deadline| i64::try_from(deadline.as_millis()).unwrap_or(i64::MAX));
        checked(java(
            Class::Jobs,
            "schedule",
            "(ILjava/lang/String;ZZJ)Ljava/lang/String;",
            &[
                Arg::Int(job_id(name)),
                Arg::Str(name),
                Arg::Bool(constraints.network),
                Arg::Bool(constraints.charging),
                Arg::Long(deadline),
            ],
        )?)
    }

    /// Cancels job `name` if it is pending.
    ///
    /// # Errors
    ///
    /// The scheduler could not be reached.
    pub fn cancel(name: &str) -> Result<(), ServiceError> {
        java(Class::Jobs, "cancel", "(I)V", &[Arg::Int(job_id(name))]).map(drop)
    }

    /// Whether job `name` is waiting to run.
    #[must_use]
    pub fn is_pending(name: &str) -> bool {
        java(Class::Jobs, "pending", "(I)Z", &[Arg::Int(job_id(name))])
            .is_ok_and(crate::jni_host::Ret::bool)
    }
}

/// `RnJobService` runs job `name` (on its own thread): its handler's
/// answer, or `false` when none is registered.
pub(crate) fn run(name: &str) -> bool {
    let handler = HANDLERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .and_then(|handlers| handlers.get(name).copied());
    if let Some(handler) = handler {
        handler()
    } else {
        crate::log::error(&format!(
            "job `{name}` ran, but no handler is registered (export_main!(main, work = …))"
        ));
        false
    }
}
