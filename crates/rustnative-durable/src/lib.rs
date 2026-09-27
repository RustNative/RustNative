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
//! - [`edge`]: the actor contract on an edge host (Web milestone K,
//!   `C46`): one instance per actor id, routed by the host, storage in the
//!   host's key-value store.
//!
//! Everything but the actor contract and its edge adapter needs the
//! `local` feature (the default): SQLite, and a multi-threaded runtime.

pub mod actor;
pub mod edge;
#[cfg(feature = "local")]
pub mod events;
#[cfg(feature = "local")]
pub mod operations;
pub mod supervise;
#[cfg(feature = "local")]
pub mod workflow;

#[cfg(feature = "local")]
pub use actor::LocalActorSystem;
pub use actor::{Actor, ActorContext, Backend, Storage, StorageError};
#[cfg(feature = "local")]
pub use events::{BatchResult, EventEnvelope, EventHandler, EventRunner};
#[cfg(feature = "local")]
pub use operations::{Operations, RemoteState, Reporter};
pub use supervise::{WorkerEvent, supervise};
#[cfg(feature = "local")]
pub use workflow::{LocalEngine, Status, Workflow, WorkflowContext, WorkflowEngine, WorkflowError};
