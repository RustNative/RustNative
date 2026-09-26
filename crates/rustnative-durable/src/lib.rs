//! Durable and event-driven execution (`PLAN.md` Milestone 56).
//!
//! - [`workflow`]: durable workflows on the local SQLite engine. Steps are
//!   recorded and replayed after a restart. Workflows have durable timers,
//!   signals and approvals, compensation, and versioning for in-flight
//!   executions.
//! - [`events`]: event handlers. A standard envelope, batches that report
//!   partial failures, retries, dead letters, and deduplication.
//! - [`actor`]: stateful actors on the local actor system. Each has an
//!   identity, handles one message at a time, and has private durable
//!   storage and alarms.
//! - [`supervise`](mod@supervise): restarting long-lived workers by policy.
//! - [`operations`]: long-running operations whose progress and
//!   cancellation cross the client/server boundary.

pub mod actor;
pub mod events;
pub mod operations;
pub mod supervise;
pub mod workflow;

pub use actor::{Actor, ActorContext, LocalActorSystem, Storage};
pub use events::{BatchResult, EventEnvelope, EventHandler, EventRunner};
pub use operations::{Operations, RemoteState, Reporter};
pub use supervise::{WorkerEvent, supervise};
pub use workflow::{LocalEngine, Status, Workflow, WorkflowContext, WorkflowEngine, WorkflowError};
