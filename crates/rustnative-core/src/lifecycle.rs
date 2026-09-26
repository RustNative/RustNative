//! Where the application is in its life, as the platform reports it.

/// A transition in the application's life (see [`crate::Event::Lifecycle`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Lifecycle {
    /// The application is about to stop running without being closed: the
    /// machine is sleeping, or the platform is suspending it. It may never
    /// be resumed, so anything worth keeping must be saved now.
    Suspending,
    /// The application is running again after [`Self::Suspending`].
    Resuming,
    /// The application is about to exit: its last window closed, or the
    /// person is signing out or shutting down.
    Terminating,
    /// The host is short of memory. Persisted state is flushed before this
    /// is delivered; a component drops what it can rebuild — caches,
    /// decoded images, prefetched pages — and keeps what it cannot.
    LowMemory,
}
