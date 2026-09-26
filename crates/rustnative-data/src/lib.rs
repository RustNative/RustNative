//! The application data layer (`PLAN.md` Milestone 47): what an
//! application does between its UI and its host.
//!
//! - [`query`]: cached, deduplicated, revalidated asynchronous data, with
//!   pagination and batching;
//! - [`mutation`]: optimistic changes with rollback, and an offline queue
//!   with a conflict policy;
//! - [`forms`]: one validation model for client and server, with a
//!   changeset, per-field errors, and a submission lifecycle;
//! - [`migrate`]: versioned persisted state, migrated up and down;
//! - [`history`]: undo and redo;
//! - [`machine`]: state machines whose states own their work;
//! - [`operation`]: long-running operations with progress and cancellation;
//! - [`work`]: background work under network, power, and deadline
//!   constraints;
//! - [`table`]: live queries over local storage, and a gap-filling paging
//!   source;
//! - [`http`]: an interceptor chain and declared endpoints;
//! - [`image`]: image loading with decode, downscale, and caches.
//!
//! Shared state, error boundaries, supervision, and streams are in
//! `rustnative-core` ([`rustnative_core::state`], [`rustnative_core::component::boundary`],
//! [`rustnative_core::scheduler`]), because every component can use them.
//! The guide is `docs/data.md`.

pub mod batch;
pub mod document;
pub mod forms;
pub mod history;
pub mod http;
pub mod image;
pub mod list;
pub mod machine;
pub mod migrate;
pub mod mutation;
pub mod operation;
pub mod query;
pub mod table;
pub mod work;

pub use batch::BatchLoader;
pub use forms::{
    Changeset, Field, FieldError, FieldErrors, FieldValue, Form, Kind, Rule, Schema, Submission,
    SubmitError, Valid,
};
pub use history::History;
pub use http::{
    BearerAuth, Endpoint, HttpClient, Interceptor, Logging, Next, ResponseCache, Retry,
};
pub use image::{ImageDecoder, ImageLoader, PortableDecoder, fitted};
pub use machine::{MachineState, StateMachine, StateScope};
pub use migrate::{MigrationError, Migrations, Planned, VersionedStore};
pub use mutation::{
    ConflictPolicy, Mutation, MutationError, MutationRequest, MutationStatus, QueuedMutation,
};
pub use operation::{Operation, OperationState, Progress};
pub use query::{
    Cache, LocalFuture, Page, Pages, Query, QueryClient, QueryError, QueryKey, QueryState,
    Revalidate,
};
pub use table::{LocalTable, PagingSource, Row};
pub use work::{BackgroundWork, Conditions, Constraints, FixedConditions};
