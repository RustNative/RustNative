//! Theme and style resolution.
//!
//! A [`Theme`] supplies per-kind, per-interaction-state defaults; a node may
//! additionally carry its own [`VisualStyle`] override. [`Theme::resolve`] is
//! the single place those two layers are merged into the value a backend
//! actually realizes as native fonts/colors — see
//! `crate::reconcile::TreeSnapshot::from_node_with_theme`, which calls it for
//! every node before a snapshot ever reaches a backend.

use super::phase::{ResolvedStyle, StyleOverride};
use crate::layout::EdgeInsets;
use crate::node::NodeKind;

pub use rustnative_style::{ShadowLayer, StyleValue, TokenTable};
pub use rustnative_types::Color;

/// Font family, size, and weight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Typography {
    /// The font family name (a platform-native lookup, not a bundled font).
    pub family: String,
    /// Point size.
    pub size: u16,
    /// CSS-style numeric weight (400 = regular, 700 = bold, ...).
    pub weight: u16,
}

impl Default for Typography {
    fn default() -> Self {
        Self { family: "system-ui".into(), size: 14, weight: 400 }
    }
}

/// A node's visual style: a set of independently optional overrides, each
/// one falling back to the active theme's corresponding value when absent
/// (see [`Theme::resolve`]).
///
/// Fields are private and read through accessors. Unlike the plain
/// geometry/color data carriers elsewhere in this crate, `VisualStyle` is a
/// natural fit for encapsulation: it already has a fluent builder for
/// *writing* (`foreground`, `background`, ...), so exposing the fields
/// directly for *reading* as well only doubles the API surface for the same
/// data — a shape the standards audit flags directly (P2.24).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualStyle {
    foreground: Option<Color>,
    background: Option<Color>,
    border: Option<Color>,
    border_radius: Option<u16>,
    typography: Option<Typography>,
    padding: Option<EdgeInsets>,
    shadow: Option<Vec<ShadowLayer>>,
}

impl VisualStyle {
    /// Creates a style with every property unset (fully deferring to the
    /// active theme).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            foreground: None,
            background: None,
            border: None,
            border_radius: None,
            typography: None,
            padding: None,
            shadow: None,
        }
    }

    /// Sets the shadow override: its layers, first painted last; an empty
    /// list is "no shadow". A backend that cannot draw shadows says so in
    /// its capability table (`Platform::style_capabilities`).
    #[must_use]
    pub fn shadow(mut self, layers: impl Into<Vec<ShadowLayer>>) -> Self {
        self.shadow = Some(layers.into());
        self
    }

    /// Returns the shadow override, if set.
    #[must_use]
    pub fn shadow_override(&self) -> Option<&[ShadowLayer]> {
        self.shadow.as_deref()
    }

    /// Sets the foreground (text/content) color override.
    #[must_use]
    pub const fn foreground(mut self, color: Color) -> Self {
        self.foreground = Some(color);
        self
    }

    /// Sets the background color override.
    #[must_use]
    pub const fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    /// Sets the border color override.
    #[must_use]
    pub const fn border(mut self, color: Color) -> Self {
        self.border = Some(color);
        self
    }

    /// Sets the border corner-radius override, in pixels.
    #[must_use]
    pub const fn border_radius(mut self, radius: u16) -> Self {
        self.border_radius = Some(radius);
        self
    }

    /// Sets the typography override.
    #[must_use]
    pub fn typography(mut self, typography: Typography) -> Self {
        self.typography = Some(typography);
        self
    }

    /// Sets the padding override.
    #[must_use]
    pub const fn padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = Some(padding);
        self
    }

    /// Returns the foreground color override, if set.
    #[must_use]
    pub const fn foreground_override(&self) -> Option<Color> {
        self.foreground
    }

    /// Returns the background color override, if set.
    #[must_use]
    pub const fn background_override(&self) -> Option<Color> {
        self.background
    }

    /// Returns the border color override, if set.
    #[must_use]
    pub const fn border_override(&self) -> Option<Color> {
        self.border
    }

    /// Returns the border corner-radius override, if set.
    #[must_use]
    pub const fn border_radius_override(&self) -> Option<u16> {
        self.border_radius
    }

    /// Returns the typography override, if set.
    #[must_use]
    pub fn typography_override(&self) -> Option<&Typography> {
        self.typography.as_ref()
    }

    /// Returns the padding override, if set.
    #[must_use]
    pub const fn padding_override(&self) -> Option<EdgeInsets> {
        self.padding
    }

    /// Layers `override_style`'s set properties on top of `self`, keeping
    /// `self`'s value for any property `override_style` leaves unset.
    pub(crate) fn merge(&self, override_style: &Self) -> Self {
        Self {
            foreground: override_style.foreground.or(self.foreground),
            background: override_style.background.or(self.background),
            border: override_style.border.or(self.border),
            border_radius: override_style.border_radius.or(self.border_radius),
            typography: override_style.typography.clone().or_else(|| self.typography.clone()),
            padding: override_style.padding.or(self.padding),
            shadow: override_style.shadow.clone().or_else(|| self.shadow.clone()),
        }
    }
}

/// A node's own state styles: what `hover:`, `focus:`, `active:`, and
/// `disabled:` classes (or [`crate::Node::with_state_style`]) set, layered
/// over the node's normal style while it is in that state. The typed
/// spelling of a state variant, and the node-level counterpart of a
/// theme's [`ComponentStyle`] states.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StateStyles {
    hovered: Option<VisualStyle>,
    focused: Option<VisualStyle>,
    pressed: Option<VisualStyle>,
    disabled: Option<VisualStyle>,
}

impl StateStyles {
    /// No state styles.
    #[must_use]
    pub const fn new() -> Self {
        Self { hovered: None, focused: None, pressed: None, disabled: None }
    }

    /// The style for `state`, if one is set. [`ControlState::Normal`] has
    /// none: it is the node's own style.
    #[must_use]
    pub const fn get(&self, state: ControlState) -> Option<&VisualStyle> {
        match state {
            ControlState::Normal => None,
            ControlState::Hovered => self.hovered.as_ref(),
            ControlState::Focused => self.focused.as_ref(),
            ControlState::Pressed => self.pressed.as_ref(),
            ControlState::Disabled => self.disabled.as_ref(),
        }
    }

    /// The style for `state`, created empty if unset; `None` for
    /// [`ControlState::Normal`].
    pub(crate) fn get_or_insert(&mut self, state: ControlState) -> Option<&mut VisualStyle> {
        let slot = match state {
            ControlState::Normal => return None,
            ControlState::Hovered => &mut self.hovered,
            ControlState::Focused => &mut self.focused,
            ControlState::Pressed => &mut self.pressed,
            ControlState::Disabled => &mut self.disabled,
        };
        Some(slot.get_or_insert_with(VisualStyle::new))
    }

    /// Whether no state has a style.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.hovered.is_none()
            && self.focused.is_none()
            && self.pressed.is_none()
            && self.disabled.is_none()
    }
}

impl Default for VisualStyle {
    fn default() -> Self {
        Self::new()
    }
}

/// A control's current interaction state, used to select which of a
/// [`ComponentStyle`]'s state-specific overrides applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ControlState {
    /// No special interaction is occurring.
    #[default]
    Normal,
    /// The pointer is hovering the control.
    Hovered,
    /// The control has keyboard focus.
    Focused,
    /// The control is currently being pressed/activated.
    Pressed,
    /// The control is disabled and cannot be interacted with.
    Disabled,
}

/// A node kind's style across every [`ControlState`], each state falling
/// back to [`Self::normal`] for any property it does not itself override.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ComponentStyle {
    /// The baseline style, used directly for [`ControlState::Normal`] and
    /// as the fallback for every other state's unset properties.
    pub normal: VisualStyle,
    /// Overrides applied while the pointer hovers the control.
    pub hovered: Option<VisualStyle>,
    /// Overrides applied while the control has keyboard focus.
    pub focused: Option<VisualStyle>,
    /// Overrides applied while the control is being pressed/activated.
    pub pressed: Option<VisualStyle>,
    /// Overrides applied while the control is disabled.
    pub disabled: Option<VisualStyle>,
}

impl ComponentStyle {
    /// Resolves the fully-merged style for `state`: [`Self::normal`] with
    /// that state's override (if any) layered on top.
    #[must_use]
    pub fn resolve(&self, state: ControlState) -> VisualStyle {
        let state_style = match state {
            ControlState::Normal => None,
            ControlState::Hovered => self.hovered.as_ref(),
            ControlState::Focused => self.focused.as_ref(),
            ControlState::Pressed => self.pressed.as_ref(),
            ControlState::Disabled => self.disabled.as_ref(),
        };
        state_style.map_or_else(|| self.normal.clone(), |style| self.normal.merge(style))
    }
}

/// The application-wide default styling for every node kind and
/// interaction state.
///
/// (The example below is about resolving the typed spelling —
/// single-spelling: a class string lowers to the same typed override before
/// it reaches here; `rustnative_core::classes!` shows the two side by side.)
///
/// Encapsulated with accessors (no direct external field access exists in
/// either this crate or `rustnative-windows`, which only ever calls
/// [`Theme::resolve`] — see the standards audit's P2.24 finding).
///
/// # Example
///
/// ```
/// use rustnative_core::{Color, ControlState, NodeKind, StyleOverride, Theme, VisualStyle};
///
/// let theme = Theme::default();
///
/// // Resolution merges the application's override onto the theme's own
/// // default for this node kind and state.
/// let authored = StyleOverride::new(VisualStyle::new().foreground(Color::rgb(0, 0, 0)));
/// let resolved = theme.resolve(NodeKind::Button, ControlState::Normal, &authored);
///
/// // The override wins where it says something...
/// assert_eq!(resolved.properties().foreground_override(), Some(Color::rgb(0, 0, 0)));
/// // ...and the theme shows through where it does not.
/// assert_eq!(
///     resolved.properties().background_override(),
///     theme.button().normal.background_override(),
/// );
/// // The resolved style remembers which state produced it.
/// assert_eq!(resolved.state(), ControlState::Normal);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    typography: Typography,
    foreground: Color,
    background: Color,
    primary: Color,
    spacing: u16,
    radius: u16,
    label: ComponentStyle,
    button: ComponentStyle,
    text_input: ComponentStyle,
    container: ComponentStyle,
    tokens: TokenTable,
    /// Tokens that follow a host color (Milestone 48).
    host_roles: Vec<(std::borrow::Cow<'static, str>, super::host::HostRole)>,
}

impl Default for Theme {
    fn default() -> Self {
        let foreground = Color::rgb(32, 32, 32);
        let background = Color::rgb(255, 255, 255);
        let primary = Color::rgb(0, 120, 212);
        Self {
            typography: Typography::default(),
            foreground,
            background,
            primary,
            spacing: 8,
            radius: 4,
            label: ComponentStyle {
                normal: VisualStyle::new().foreground(foreground).typography(Typography::default()),
                ..Default::default()
            },
            button: ComponentStyle {
                normal: VisualStyle::new()
                    .foreground(foreground)
                    .background(background)
                    .border(primary)
                    .border_radius(4)
                    .typography(Typography::default()),
                ..Default::default()
            },
            text_input: ComponentStyle {
                normal: VisualStyle::new()
                    .foreground(foreground)
                    .background(background)
                    .border(Color::rgb(128, 128, 128))
                    .typography(Typography::default()),
                ..Default::default()
            },
            container: ComponentStyle {
                normal: VisualStyle::new().background(background),
                ..Default::default()
            },
            tokens: TokenTable::defaults(),
            host_roles: Vec::new(),
        }
    }
}

impl Theme {
    /// Returns the default typography.
    #[must_use]
    pub const fn typography(&self) -> &Typography {
        &self.typography
    }

    /// Returns the default foreground color.
    #[must_use]
    pub const fn foreground(&self) -> Color {
        self.foreground
    }

    /// Returns the default background color.
    #[must_use]
    pub const fn background(&self) -> Color {
        self.background
    }

    /// Returns the accent/primary color, used for emphasis (e.g. a button's
    /// border).
    #[must_use]
    pub const fn primary(&self) -> Color {
        self.primary
    }

    /// Returns the default spacing unit, in pixels, used between sibling
    /// nodes in a layout container.
    #[must_use]
    pub const fn spacing(&self) -> u16 {
        self.spacing
    }

    /// Returns the default border corner-radius, in pixels.
    #[must_use]
    pub const fn radius(&self) -> u16 {
        self.radius
    }

    /// Returns the default style for [`NodeKind::Label`](crate::node::NodeKind::Label) nodes.
    #[must_use]
    pub const fn label(&self) -> &ComponentStyle {
        &self.label
    }

    /// Returns the default style for [`NodeKind::Button`](crate::node::NodeKind::Button) nodes.
    #[must_use]
    pub const fn button(&self) -> &ComponentStyle {
        &self.button
    }

    /// Returns the default style for [`NodeKind::TextInput`](crate::node::NodeKind::TextInput) nodes.
    #[must_use]
    pub const fn text_input(&self) -> &ComponentStyle {
        &self.text_input
    }

    /// Returns the default style for container ([`NodeKind::Column`](crate::node::NodeKind::Column)/
    /// [`NodeKind::Row`](crate::node::NodeKind::Row)) nodes.
    #[must_use]
    pub const fn container(&self) -> &ComponentStyle {
        &self.container
    }

    // Builder methods for constructing a customized theme from
    // `Theme::default()`. These exist specifically so encapsulating this
    // type's fields (see the struct's own doc comment) doesn't take away an
    // application's ability to build a custom theme — the same capability
    // the pre-encapsulation, public-field design offered through struct-
    // update syntax (`Theme { button: ..., ..Theme::default() }`).

    /// Returns `self` with the default typography replaced.
    #[must_use]
    pub fn with_typography(mut self, typography: Typography) -> Self {
        self.typography = typography;
        self
    }

    /// Returns `self` with the default foreground color replaced.
    #[must_use]
    pub const fn with_foreground(mut self, color: Color) -> Self {
        self.foreground = color;
        self
    }

    /// Returns `self` with the default background color replaced.
    #[must_use]
    pub const fn with_background(mut self, color: Color) -> Self {
        self.background = color;
        self
    }

    /// Returns `self` with the accent/primary color replaced.
    #[must_use]
    pub const fn with_primary(mut self, color: Color) -> Self {
        self.primary = color;
        self
    }

    /// Returns `self` with the default spacing unit replaced.
    #[must_use]
    pub const fn with_spacing(mut self, spacing: u16) -> Self {
        self.spacing = spacing;
        self
    }

    /// Returns `self` with the default border corner-radius replaced.
    #[must_use]
    pub const fn with_radius(mut self, radius: u16) -> Self {
        self.radius = radius;
        self
    }

    /// Returns `self` with the label style replaced.
    #[must_use]
    pub fn with_label(mut self, style: ComponentStyle) -> Self {
        self.label = style;
        self
    }

    /// Returns `self` with the button style replaced.
    #[must_use]
    pub fn with_button(mut self, style: ComponentStyle) -> Self {
        self.button = style;
        self
    }

    /// Returns `self` with the text-input style replaced.
    #[must_use]
    pub fn with_text_input(mut self, style: ComponentStyle) -> Self {
        self.text_input = style;
        self
    }

    /// Returns `self` with the container style replaced.
    #[must_use]
    pub fn with_container(mut self, style: ComponentStyle) -> Self {
        self.container = style;
        self
    }

    /// Returns the token table that `var(--…)` references in declarations
    /// resolve against — by default the utility vocabulary's default theme
    /// (Tailwind CSS v4's), or a project's `app.css` through
    /// `rustnative_core::app_theme!()`.
    #[must_use]
    pub const fn tokens(&self) -> &TokenTable {
        &self.tokens
    }

    /// Returns `self` with the token table replaced. Declarations keep
    /// referring to tokens by name, so switching the table (through
    /// `Application::set_theme`) re-resolves every node against it — on the
    /// native objects that already exist (2.14).
    #[must_use]
    pub fn with_tokens(mut self, tokens: TokenTable) -> Self {
        self.tokens = tokens;
        self
    }

    /// Returns `self` with one token set.
    #[must_use]
    pub fn with_token(
        mut self,
        name: impl Into<std::borrow::Cow<'static, str>>,
        value: StyleValue,
    ) -> Self {
        self.tokens.insert(name, value);
        self
    }

    /// Returns `self` with token `name` following the host's `role`: its
    /// value stays the fallback until a backend applies the host's palette
    /// ([`Self::with_host_palette`]), and changes with it (`docs/tokens.md`).
    #[must_use]
    pub fn with_host_role(
        mut self,
        name: impl Into<std::borrow::Cow<'static, str>>,
        role: super::host::HostRole,
    ) -> Self {
        let name = name.into();
        self.host_roles.retain(|(existing, _)| *existing != name);
        self.host_roles.push((name, role));
        self
    }

    /// The tokens that follow host colors, and the roles they follow.
    #[must_use]
    pub fn host_roles(&self) -> &[(std::borrow::Cow<'static, str>, super::host::HostRole)] {
        &self.host_roles
    }

    /// Returns `self` with every host-following token set from `palette`.
    #[must_use]
    pub fn with_host_palette(mut self, palette: &super::host::HostPalette) -> Self {
        for (name, role) in &self.host_roles {
            self.tokens.insert(name.clone(), StyleValue::Color(palette.color(*role)));
        }
        self
    }

    /// Resolves the fully-merged style for one node: this theme's default
    /// for `kind`/`state`, with the application's own override layered on
    /// top.
    ///
    /// This is the only thing in this crate that produces a
    /// [`ResolvedStyle`], which is what makes that type's existence mean
    /// something: holding one is proof that theme resolution happened.
    #[must_use]
    pub fn resolve(
        &self,
        kind: NodeKind,
        state: ControlState,
        override_style: &StyleOverride,
    ) -> ResolvedStyle {
        let base = match kind {
            NodeKind::Label => &self.label,
            // A tab strip's labels are the platform control's own; what the
            // theme contributes is its font and colors, a button's concern.
            // A control's text is the system control's own, in the font a
            // button uses.
            NodeKind::Button | NodeKind::TabBar | NodeKind::Control => &self.button,
            NodeKind::TextInput => &self.text_input,
            // A canvas paints itself and a surface is painted by the
            // application; what the theme contributes to either is the
            // background behind them, which is a container's concern.
            NodeKind::Column | NodeKind::Row | NodeKind::Canvas | NodeKind::Surface => {
                &self.container
            }
        }
        .resolve(state);
        let normal = base.merge(override_style.properties());
        let resolved = match override_style.states().get(state) {
            Some(state_style) => normal.merge(state_style),
            None => normal,
        };
        ResolvedStyle::new(resolved, state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_merges_node_overrides_and_state_styles() {
        let theme = Theme::default();
        let override_style = StyleOverride::new(VisualStyle::new().foreground(Color::rgb(1, 2, 3)));
        let resolved = theme.resolve(NodeKind::Button, ControlState::Normal, &override_style);
        assert_eq!(resolved.properties().foreground_override(), Some(Color::rgb(1, 2, 3)));
        // The override didn't specify a background, so the theme's own
        // button default should still show through.
        assert_eq!(
            resolved.properties().background_override(),
            theme.button().normal.background_override()
        );
        assert_eq!(resolved.state(), ControlState::Normal);
    }

    #[test]
    fn color_from_packed_hex_matches_rgb_components() {
        let color = Color::from(0xFF_00_FF);
        assert_eq!(color, Color::rgb(0xFF, 0x00, 0xFF));
    }
}
