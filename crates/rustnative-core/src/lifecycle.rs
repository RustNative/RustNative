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

/// A transition in one window's life, apart from the process's (see
/// [`crate::Event::WindowLifecycle`]).
///
/// A host that shows a window root at a time, or several the person
/// arranges — an Android activity, an iPad scene — backgrounds and discards
/// windows while the process keeps running, and re-creates a discarded one
/// from its saved state later. These are that window's own transitions;
/// the process's are [`Lifecycle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WindowLifecycle {
    /// The window can no longer be seen (another window or application is
    /// in front of it), though the process runs. Its persisted state is
    /// flushed before this is delivered.
    Backgrounded,
    /// The window can be seen again after [`Self::Backgrounded`].
    Foregrounded,
    /// The host discarded the window — the person dismissed it, or the
    /// host reclaimed it — and may restore it later from its saved state.
    /// Its persisted state is flushed before this is delivered.
    Disconnected,
    /// The window was re-created from saved state rather than opened fresh:
    /// what it shows is what the person left.
    Restored,
}

impl WindowLifecycle {
    /// Whether the window's persisted state is flushed before this is
    /// delivered: it may not be seen again by this process.
    #[must_use]
    pub const fn flushes(self) -> bool {
        matches!(self, Self::Backgrounded | Self::Disconnected)
    }
}
