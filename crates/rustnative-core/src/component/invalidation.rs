//! Why a component rendered: the invalidation contract, recorded.
//!
//! A component renders when, and only when, something it depends on
//! changed: an event or message it handled, new props, an environment value
//! it read, or a change that invalidates every component (a new theme, a
//! forced render). Everything else is skipped and its previous output
//! reused. `docs/invalidation.md` states the contract; the render log is how
//! a test holds the framework to it, and how the inspector answers "why did
//! this render?" (`C04-2`).

use crate::identity::ComponentId;

/// What caused one component to render.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RenderCause {
    /// It was just created.
    Initial,
    /// It handled an event.
    Event,
    /// It received a message: a child callback or a task result.
    Message,
    /// Its parent gave it props unequal to the previous ones.
    Props,
    /// An environment value it read changed; carries the key's name.
    Environment(&'static str),
    /// A preference it read, published by a descendant, changed.
    Preference(&'static str),
    /// The theme or motion preference changed, which every component may
    /// depend on.
    Theme,
    /// A full render was requested explicitly.
    Forced,
    /// The slice of a store it selected changed.
    Store,
    /// A failure in its subtree was contained at it, an error boundary.
    Failure,
}

impl std::fmt::Display for RenderCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Initial => f.write_str("initial"),
            Self::Event => f.write_str("event"),
            Self::Message => f.write_str("message"),
            Self::Props => f.write_str("props"),
            Self::Environment(key) => write!(f, "environment({key})"),
            Self::Preference(key) => write!(f, "preference({key})"),
            Self::Theme => f.write_str("theme"),
            Self::Forced => f.write_str("forced"),
            Self::Store => f.write_str("store"),
            Self::Failure => f.write_str("failure"),
        }
    }
}

/// One component's render in the most recent render pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderRecord {
    /// The component.
    pub component: ComponentId,
    /// Its key path from the window root (the same path persisted state is
    /// keyed by).
    pub path: String,
    /// Why it rendered.
    pub cause: RenderCause,
}
