//! Reconciliation beyond the screen (`PLAN.md` Milestone 55): the
//! framework's core idea — declare desired state, reconcile reality towards
//! it — applied to data replicated between devices and a server, to a UI
//! tree held on a server, and to a device fleet.
//!
//! - [`clock`]: hybrid logical clocks, the order of every write.
//! - [`crdt`]: conflict-free replicated counters, register, set, map, and
//!   sequence (collaborative text).
//! - [`replica`]: the sync service — local-first collections, background
//!   replication, server push, partial replication, per-collection
//!   conflict policy, and schema versioning.
//! - [`channel`]: channels and presence as service contracts.
//! - [`live`]: server-interactive mode — a component tree on the server,
//!   its trees sent to the client, events back, reconnection and draining.
//! - [`device`]: device desired and reported state, and messaging with
//!   stated delivery guarantees (an in-process broker and MQTT 3.1.1).

pub mod bus;
pub mod channel;
pub mod clock;
pub mod crdt;
pub mod device;
pub mod http;
pub mod live;
pub mod mqtt;
pub mod replica;

pub use channel::{Channel, LocalHub, Presence};
pub use clock::{Clock, Hlc, ReplicaId};
pub use crdt::{Crdt, GCounter, LwwMap, LwwRegister, OrSet, PnCounter, Rga};
pub use replica::{
    Change, ConflictPolicy, Filter, InMemory, Pull, PullReply, Push, PushReply, Record, SyncError,
    SyncReport, SyncServer, SyncTransport, SyncedCollection,
};
