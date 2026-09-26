//! The two phases a node's styling passes through, as distinct types.
//!
//! # Why these exist
//!
//! A node's appearance is described twice, and the two descriptions mean
//! very different things:
//!
//! 1. What the *application* asked for — a [`VisualStyle`] carrying only the
//!    properties it chose to override, with everything else left unset.
//! 2. What the node will actually *look like* — that override merged onto
//!    the active `Theme`'s default for the node's kind, in one specific
//!    [`ControlState`].
//!
//! Both used to be a bare `VisualStyle` sitting side by side in `TreeNode`,
//! distinguished only by field name and a doc comment. That is the standards
//! audit's P2.28 finding:
//!
//! > This is workable but makes semantic ownership unclear: which
//! > representation is authoritative for which phase? [...] make the phase
//! > boundary impossible to misunderstand.
//!
//! Comments cannot make it impossible. Types can: a backend that reaches for
//! the override where it needed the resolved style now fails to compile
//! instead of quietly painting a control with every themed property missing.
//!
//! # Why two types and not three
//!
//! The audit sketches three (`StyleOverride`, `ResolvedStyle`,
//! `InteractionResolvedStyle`). The third is not a different *shape* — an
//! interaction-resolved style is a resolved style; the only thing that
//! distinguishes it is which [`ControlState`] it was resolved for. So that
//! state is carried as data on [`ResolvedStyle`] rather than encoded as a
//! separate type. This is strictly more informative than a third newtype:
//! `resolved.state()` answers "hover or normal?" at runtime, which a type
//! could only answer at the definition site.

use crate::style::{ControlState, StateStyles, VisualStyle};

/// The visual properties an application explicitly set on a node, with
/// everything it did not set left unspecified.
///
/// This is the *input* to theme resolution, and the thing a backend keeps in
/// order to re-resolve a node against a transient interaction state without
/// losing the application's own customization. It is deliberately not
/// something to paint from directly: most of its properties are typically
/// unset, and painting from it would ignore the theme entirely.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StyleOverride {
    normal: VisualStyle,
    states: StateStyles,
}

impl StyleOverride {
    /// Wraps an application-authored style.
    #[must_use]
    pub const fn new(style: VisualStyle) -> Self {
        Self { normal: style, states: StateStyles::new() }
    }

    /// `self` with the node's own state styles, layered over its normal
    /// style in their states.
    #[must_use]
    pub fn with_states(mut self, states: StateStyles) -> Self {
        self.states = states;
        self
    }

    /// The underlying properties, for resolution against a theme.
    #[must_use]
    pub const fn properties(&self) -> &VisualStyle {
        &self.normal
    }

    /// The node's own state styles.
    #[must_use]
    pub const fn states(&self) -> &StateStyles {
        &self.states
    }
}

impl From<VisualStyle> for StyleOverride {
    fn from(style: VisualStyle) -> Self {
        Self::new(style)
    }
}

/// A node's fully resolved appearance: its [`StyleOverride`] merged onto the
/// active theme's default for its kind, in one specific [`ControlState`].
///
/// This is what a backend paints from. The state it was resolved for travels
/// with it, so a backend can tell an ordinary render's `Normal`/`Disabled`
/// resolution apart from a transient hover/press/focus one — the distinction
/// that decides whether a repaint may skip re-running the application's
/// component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStyle {
    style: VisualStyle,
    state: ControlState,
}

impl ResolvedStyle {
    /// Records `style` as the resolution of some node for `state`.
    ///
    /// Constructing one of these is a claim that theme resolution already
    /// happened; `Theme::resolve` is the only thing in this crate that makes
    /// that claim.
    #[must_use]
    pub const fn new(style: VisualStyle, state: ControlState) -> Self {
        Self { style, state }
    }

    /// The resolved properties, ready to paint.
    #[must_use]
    pub const fn properties(&self) -> &VisualStyle {
        &self.style
    }

    /// The interaction state this was resolved for.
    #[must_use]
    pub const fn state(&self) -> ControlState {
        self.state
    }
}

impl Default for ResolvedStyle {
    /// An unstyled node in its resting state — what a snapshot built without
    /// a theme (`TreeSnapshot::from_node`) carries until something resolves
    /// it properly.
    fn default() -> Self {
        Self::new(VisualStyle::default(), ControlState::Normal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Color;

    #[test]
    fn a_resolved_style_remembers_which_state_it_was_resolved_for() {
        let resolved = ResolvedStyle::new(
            VisualStyle::new().foreground(Color::rgb(1, 2, 3)),
            ControlState::Hovered,
        );
        assert_eq!(resolved.state(), ControlState::Hovered);
        assert_eq!(resolved.properties().foreground_override(), Some(Color::rgb(1, 2, 3)));
    }

    #[test]
    fn an_unresolved_snapshot_defaults_to_the_resting_state() {
        assert_eq!(ResolvedStyle::default().state(), ControlState::Normal);
    }

    #[test]
    fn the_two_phases_are_distinct_types_even_when_they_wrap_equal_properties() {
        // The point of the split is that this comparison cannot be written
        // by accident. Both phases are inspected through `properties()`,
        // which is explicit about crossing the boundary.
        let authored = VisualStyle::new().foreground(Color::rgb(9, 9, 9));
        let override_phase = StyleOverride::new(authored.clone());
        let resolved_phase = ResolvedStyle::new(authored, ControlState::Normal);
        assert_eq!(override_phase.properties(), resolved_phase.properties());
    }
}
