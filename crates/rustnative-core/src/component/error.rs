//! Structured errors for user-triggerable invalid component composition.
//!
//! The standards audit (P1.10) draws a deliberate line: an *internal*
//! impossible state (a bookkeeping map that must contain an entry the
//! runtime itself just inserted) is a framework bug and stays a `panic!`/
//! `debug_assert!` — no caller-facing API could do anything useful with it
//! anyway. A state an *application author* can trigger by writing an
//! ordinary — if mistaken — component is different: it deserves a value the
//! caller can inspect, log, or degrade against, not an opaque abort.
//!
//! [`RenderError`] covers exactly the latter category. Both variants here
//! were `assert!`/`debug_assert!` before this pass; see
//! [`crate::ComponentTree::render`] and [`crate::ComponentTree::last_render_error`]
//! for how they are produced and surfaced.

use std::error::Error;
use std::fmt;

use crate::identity::ComponentId;

/// A problem detected while rendering the declarative component tree that
/// originates from how an application composed its own components, rather
/// than from a framework-internal invariant violation.
///
/// Rendering does not abort when one of these is detected: the tree still
/// completes a structurally consistent (if not fully correct) pass — the
/// offending duplicate is simply not registered a second time — so a host
/// can keep the application on screen and surface the error through its own
/// diagnostics rather than lose the whole session to one composition bug.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RenderError {
    /// A component's own declarative `view`/`render` output used the same
    /// local node key more than once. Every node a component authors
    /// directly must have a unique key among that component's *own* output;
    /// nodes contributed by a nested managed child are scoped independently
    /// (see [`crate::identity`]) and never conflict with their parent's keys.
    DuplicateNodeKey {
        /// The component whose authored view contained the duplicate.
        component: ComponentId,
    },
    /// A component called `ComponentContext::effect` with the same key more
    /// than once during a single render. Effect keys, like node keys, must
    /// be unique within one component's render.
    DuplicateEffectKey {
        /// The component that declared the duplicate effect.
        component: ComponentId,
        /// The key that was declared more than once.
        key: String,
    },
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateNodeKey { component } => write!(
                f,
                "component {component:?} used the same local node key more than once in a \
                 single render; every node a component authors directly must have a unique key"
            ),
            Self::DuplicateEffectKey { component, key } => write!(
                f,
                "component {component:?} registered the effect key {key:?} more than once in a \
                 single render; effect keys must be unique within one component's render"
            ),
        }
    }
}

impl Error for RenderError {}
