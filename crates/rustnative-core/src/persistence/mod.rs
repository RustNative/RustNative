//! State that outlives the process: component state restored on the next
//! launch, buffered in memory and written out at the moments the platform
//! says the application might stop.
//!
//! # The pieces
//!
//! - A [`StateStore`] is where bytes go: [`MemoryStateStore`] here, a file
//!   store on Windows (`rustnative_windows::FileStateStore`). It is set on
//!   [`crate::Services`] so it is present from the very first render — a
//!   component reading its saved state never sees a default first and the
//!   real value a render later.
//! - [`crate::ComponentContext::persisted`] gives a component a
//!   [`Persisted`] value under a key of its choosing. The full key is the
//!   component's **key path** — the chain of child keys from its window's
//!   root, which is stable across runs, unlike a runtime component id —
//!   plus that key, so two instances of the same component in different
//!   places never collide.
//! - Writes are **buffered**: [`Persisted::set`] costs a serialization and
//!   a map insert, never a disk write. The buffer is flushed by
//!   [`crate::Application::flush_state`], which the platform backend calls
//!   before delivering [`crate::Lifecycle::Suspending`] or
//!   [`crate::Lifecycle::Terminating`], when the last window closes, and
//!   shortly after a burst of writes goes quiet.
//!
//! Values are serialized as JSON: readable when a state file needs
//! debugging, and tolerant of a field added in a later version (with
//! `#[serde(default)]`). A value that no longer deserializes — its type
//! changed shape between versions — reads as the default rather than
//! failing the component.

mod persisted;
mod store;

pub use persisted::Persisted;
pub(crate) use persisted::StateCache;
pub use store::{MemoryStateStore, StateStore};
