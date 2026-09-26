//! Theme and style resolution.
//!
//! - `theme`: the style model itself — colors, typography, per-kind and
//!   per-state defaults, the token table, and the merge that resolves them.
//! - `phase`: the two *phases* a node's styling passes through
//!   ([`StyleOverride`] before resolution, [`ResolvedStyle`] after), kept as
//!   distinct types so a backend cannot silently paint from the wrong one.
//! - [`decl`]: the second spelling of the same model (`PLAN.md` 2.14) — the
//!   declaration vocabulary and the utility classes above it, which
//!   `classes!`/`styles!` lower at compile time into a [`DeclarationSet`].
//! - `resolve`: folds a node's declarations into its typed properties
//!   against the theme's tokens and the environment, after every render and
//!   on every change to either.
//!
//! The two spellings are equal by construction: a declaration names exactly
//! one typed property, so `classes!("p-4 bg-blue-500")` and a builder chain
//! setting the same padding and background resolve to the same node.

mod host;
mod phase;
mod resolve;
mod theme;

pub use host::{HostPalette, HostRole};
pub use phase::{ResolvedStyle, StyleOverride};
pub(crate) use resolve::{ResolveEnv, resolve_tree};
pub use theme::{
    Color, ComponentStyle, ControlState, ShadowLayer, StateStyles, StyleValue, Theme, TokenTable,
    Typography, VisualStyle,
};

/// The declaration model, re-exported from `rustnative-style`: what
/// `classes!` and `styles!` expand to, and what a backend's capability
/// table answers for.
pub mod decl {
    pub use rustnative_style::{
        Color, Condition, ConditionEnv, ConditionalDeclaration, Declaration, DeclarationSet,
        Direction, Fixed, Keyword, Length, Pointer, Scheme, SetKind, ShadowLayer, State,
        StyleCapabilities, StyleProperty, StyleSupport, StyleValue, TokenTable, UnitMapping,
        ValueKind,
    };
}

pub use rustnative_style::{
    DeclarationSet, StyleCapabilities, StyleProperty, StyleSupport, UnitMapping,
};
