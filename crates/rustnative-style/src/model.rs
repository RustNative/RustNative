//! The declaration model: what a class string or a declaration block lowers
//! to, and what `rustnative-core` resolves against a theme (`PLAN.md` 2.14).
//!
//! Everything here is constructible in a `static` — the `classes!` and
//! `styles!` macros write their output as one — so a class string costs no
//! parse and no allocation at run time. That is why numbers are [`Fixed`]
//! rather than floats (a float is not `Eq`, and a node is), and why names
//! are `Cow<'static, str>`.

use std::borrow::Cow;
use std::fmt;

pub use rustnative_types::Color;

/// A decimal number with three fractional digits, stored as thousandths.
///
/// ```
/// use rustnative_style::Fixed;
///
/// assert_eq!(Fixed::from_milli(1_500).to_string(), "1.5");
/// assert_eq!(Fixed::from_f64(0.25), Fixed::from_milli(250));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Fixed(i32);

impl Fixed {
    /// `0`.
    pub const ZERO: Self = Self(0);
    /// `1`.
    pub const ONE: Self = Self(1_000);

    /// The number `milli / 1000`.
    #[must_use]
    pub const fn from_milli(milli: i32) -> Self {
        Self(milli)
    }

    /// A whole number.
    #[must_use]
    pub const fn from_int(value: i32) -> Self {
        Self(value.saturating_mul(1_000))
    }

    /// The nearest representable number to `value` (half away from zero,
    /// saturating).
    #[must_use]
    pub fn from_f64(value: f64) -> Self {
        Self(round_to_i32(value * 1_000.0))
    }

    /// Thousandths.
    #[must_use]
    pub const fn milli(self) -> i32 {
        self.0
    }

    /// As a float, for arithmetic.
    #[must_use]
    pub fn to_f64(self) -> f64 {
        f64::from(self.0) / 1_000.0
    }
}

impl fmt::Display for Fixed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let magnitude = self.0.unsigned_abs();
        let whole = magnitude / 1_000;
        let fraction = magnitude % 1_000;
        if fraction == 0 {
            return write!(f, "{sign}{whole}");
        }
        let digits = format!("{fraction:03}");
        write!(f, "{sign}{whole}.{}", digits.trim_end_matches('0'))
    }
}

/// Rounds half away from zero — the one rounding rule every unit
/// conversion in the style model uses — saturating at `i32`'s range.
#[must_use]
pub fn round_to_i32(value: f64) -> i32 {
    let rounded = value.round();
    if rounded.is_nan() {
        0
    } else if rounded >= f64::from(i32::MAX) {
        i32::MAX
    } else if rounded <= f64::from(i32::MIN) {
        i32::MIN
    } else {
        #[allow(clippy::cast_possible_truncation, reason = "the range was checked above")]
        let value = rounded as i32;
        value
    }
}

/// A length in one of the vocabulary's units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Length {
    /// Logical pixels: the host maps them to device pixels (its unit
    /// mapping, [`crate::UnitMapping`]).
    Px(Fixed),
    /// Multiples of the root font size: 16 logical pixels at the host's
    /// default text size, following the person's text-size setting.
    Rem(Fixed),
    /// Multiples of the node's own font size.
    Em(Fixed),
}

impl Length {
    /// `0px`.
    pub const ZERO: Self = Self::Px(Fixed::ZERO);

    /// In logical pixels, given the root and the node's font size in
    /// logical pixels, rounded half away from zero.
    #[must_use]
    pub fn to_px(self, rem_px: f64, em_px: f64) -> i32 {
        round_to_i32(self.to_px_f64(rem_px, em_px))
    }

    /// In logical pixels, unrounded.
    #[must_use]
    pub fn to_px_f64(self, rem_px: f64, em_px: f64) -> f64 {
        match self {
            Self::Px(value) => value.to_f64(),
            Self::Rem(value) => value.to_f64() * rem_px,
            Self::Em(value) => value.to_f64() * em_px,
        }
    }

    /// The same length scaled by `factor`.
    #[must_use]
    pub fn scaled(self, factor: Fixed) -> Self {
        let scale = |value: Fixed| Fixed::from_f64(value.to_f64() * factor.to_f64());
        match self {
            Self::Px(value) => Self::Px(scale(value)),
            Self::Rem(value) => Self::Rem(scale(value)),
            Self::Em(value) => Self::Em(scale(value)),
        }
    }
}

impl fmt::Display for Length {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Px(value) => write!(f, "{value}px"),
            Self::Rem(value) => write!(f, "{value}rem"),
            Self::Em(value) => write!(f, "{value}em"),
        }
    }
}

/// The keywords a property may take instead of a length or colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Keyword {
    /// `auto`: sized to content, or the parent's alignment.
    Auto,
    /// `100%`: fill what the parent offers (`SizeMode::Fill`).
    Fill,
    /// `start`.
    Start,
    /// `center`.
    Center,
    /// `end`.
    End,
    /// `stretch`.
    Stretch,
    /// `overflow: visible`.
    Visible,
    /// `overflow: hidden`/`clip`.
    Clip,
    /// `overflow: scroll`/`auto`.
    Scroll,
    /// `display: none`.
    Hidden,
    /// `display: block`/`flex`: shown.
    Shown,
}

impl Keyword {
    /// Its CSS spelling.
    #[must_use]
    pub const fn css(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Fill => "100%",
            Self::Start => "start",
            Self::Center => "center",
            Self::End => "end",
            Self::Stretch => "stretch",
            Self::Visible => "visible",
            Self::Clip => "hidden",
            Self::Scroll => "scroll",
            Self::Hidden => "none",
            Self::Shown => "flex",
        }
    }
}

/// One layer of a shadow, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShadowLayer {
    /// Horizontal offset.
    pub x: Fixed,
    /// Vertical offset.
    pub y: Fixed,
    /// Blur radius.
    pub blur: Fixed,
    /// Spread distance.
    pub spread: Fixed,
    /// The colour.
    pub color: Color,
    /// Drawn inside the box rather than behind it.
    pub inset: bool,
}

impl fmt::Display for ShadowLayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.inset {
            f.write_str("inset ")?;
        }
        write!(f, "{}px {}px {}px {}px {}", self.x, self.y, self.blur, self.spread, hex(self.color))
    }
}

/// A declaration's value, before the theme resolves it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StyleValue {
    /// A colour, already converted to 8-bit sRGB at build time.
    Color(Color),
    /// A length.
    Length(Length),
    /// A number (a font weight, an opacity as a fraction).
    Number(Fixed),
    /// A keyword.
    Keyword(Keyword),
    /// A font family list, as written (`ui-sans-serif, system-ui, …`).
    Family(Cow<'static, str>),
    /// Shadow layers, first painted last; empty is `none`.
    Shadow(Cow<'static, [ShadowLayer]>),
    /// A reference to a theme token (`var(--color-primary)`; the name is
    /// stored without its `--`). Resolved against the theme each time the
    /// theme changes, which is what makes a theme switch a re-resolution
    /// (2.14's third rule).
    Token(Cow<'static, str>),
    /// A token multiplied by a factor: `calc(var(--spacing) * 4)`, what the
    /// spacing utilities generate.
    Scaled(Cow<'static, str>, Fixed),
    /// A colour token at a percentage of its opacity:
    /// `color-mix(in oklab, var(--color-sky-500) 50%, transparent)`, what the
    /// `/50` opacity modifier generates.
    Faded(Cow<'static, str>, u8),
}

impl StyleValue {
    /// The token this value refers to, if it refers to one.
    #[must_use]
    pub fn token(&self) -> Option<&str> {
        match self {
            Self::Token(name) | Self::Scaled(name, _) | Self::Faded(name, _) => Some(name),
            _ => None,
        }
    }
}

impl fmt::Display for StyleValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Color(color) => f.write_str(&hex(*color)),
            Self::Length(length) => write!(f, "{length}"),
            Self::Number(number) => write!(f, "{number}"),
            Self::Keyword(keyword) => f.write_str(keyword.css()),
            Self::Family(family) => f.write_str(family),
            Self::Shadow(layers) if layers.is_empty() => f.write_str("none"),
            Self::Shadow(layers) => {
                let layers: Vec<String> = layers.iter().map(ToString::to_string).collect();
                f.write_str(&layers.join(", "))
            }
            Self::Token(name) => write!(f, "var(--{name})"),
            Self::Scaled(name, factor) => write!(f, "calc(var(--{name}) * {factor})"),
            Self::Faded(name, percent) => {
                write!(f, "color-mix(in oklab, var(--{name}) {percent}%, transparent)")
            }
        }
    }
}

/// A colour as `#rrggbb`, or `#rrggbbaa` when not opaque.
#[must_use]
pub fn hex(color: Color) -> String {
    if color.alpha == 255 {
        format!("#{:02x}{:02x}{:02x}", color.red, color.green, color.blue)
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", color.red, color.green, color.blue, color.alpha)
    }
}

/// What a property accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueKind {
    /// A colour.
    Color,
    /// A length.
    Length,
    /// A length, `auto`, or `100%`.
    Size,
    /// A font weight, 1–1000.
    Weight,
    /// A fraction 0–1 (or a percentage).
    Ratio,
    /// A font family list.
    Family,
    /// Shadow layers, or `none`.
    Shadow,
    /// `start`, `center`, `end`, `stretch`.
    Alignment,
    /// `auto` or an [`ValueKind::Alignment`].
    SelfAlignment,
    /// `visible`, `hidden`/`clip`, `scroll`/`auto`.
    Overflow,
    /// `none`, `block`, `flex`.
    Display,
}

macro_rules! properties {
    ($($variant:ident => $css:literal, $kind:ident, $visual:literal, $doc:literal;)*) => {
        /// A typed style or layout property: every declaration names exactly
        /// one, and each one is a field `rustnative-core` already has
        /// (`VisualStyle`, `LayoutStyle`, a container's style, a node's
        /// opacity or visibility). Adding a property adds its declaration
        /// name, its utility spelling, and every backend's capability answer
        /// in the same change (CONTRIBUTING).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum StyleProperty {
            $(
                #[doc = $doc]
                $variant,
            )*
        }

        impl StyleProperty {
            /// Every property, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];

            /// Its CSS declaration name.
            #[must_use]
            pub const fn css_name(self) -> &'static str {
                match self {
                    $(Self::$variant => $css,)*
                }
            }

            /// What it accepts.
            #[must_use]
            pub const fn kind(self) -> ValueKind {
                match self {
                    $(Self::$variant => ValueKind::$kind,)*
                }
            }

            /// Whether it may appear under a state variant (`hover:`, …):
            /// state styles are visual (`VisualStyle`), because a hover never
            /// re-runs layout (2.10).
            #[must_use]
            pub const fn is_visual(self) -> bool {
                match self {
                    $(Self::$variant => $visual,)*
                }
            }
        }
    };
}

properties! {
    Foreground => "color", Color, true, "The text/content colour (`VisualStyle::foreground`).";
    Background => "background-color", Color, true, "The background colour (`VisualStyle::background`).";
    BorderColor => "border-color", Color, true, "The border colour (`VisualStyle::border`).";
    BorderRadius => "border-radius", Length, true, "The corner radius (`VisualStyle::border_radius`).";
    FontSize => "font-size", Length, true, "The font size (`Typography::size`).";
    FontWeight => "font-weight", Weight, true, "The font weight (`Typography::weight`).";
    FontFamily => "font-family", Family, true, "The font family (`Typography::family`).";
    Shadow => "box-shadow", Shadow, true, "The shadow (`VisualStyle::shadow`).";
    PaddingTop => "padding-top", Length, false, "Top padding.";
    PaddingEnd => "padding-inline-end", Length, false, "End padding (right in left-to-right).";
    PaddingBottom => "padding-bottom", Length, false, "Bottom padding.";
    PaddingStart => "padding-inline-start", Length, false, "Start padding (left in left-to-right).";
    MarginTop => "margin-top", Length, false, "Top margin (`LayoutStyle::margin`).";
    MarginEnd => "margin-inline-end", Length, false, "End margin.";
    MarginBottom => "margin-bottom", Length, false, "Bottom margin.";
    MarginStart => "margin-inline-start", Length, false, "Start margin.";
    Width => "width", Size, false, "The width (`LayoutStyle::width`).";
    Height => "height", Size, false, "The height (`LayoutStyle::height`).";
    MinWidth => "min-width", Length, false, "The minimum width (`Constraints`).";
    MinHeight => "min-height", Length, false, "The minimum height (`Constraints`).";
    MaxWidth => "max-width", Length, false, "The maximum width (`Constraints`).";
    MaxHeight => "max-height", Length, false, "The maximum height (`Constraints`).";
    Gap => "gap", Length, false, "A container's gap between children.";
    AlignItems => "align-items", Alignment, false, "A container's cross-axis alignment of its children.";
    AlignSelf => "align-self", SelfAlignment, false, "A node's own cross-axis alignment (`LayoutStyle::align_self`).";
    Overflow => "overflow", Overflow, false, "A container's overflow behaviour.";
    Opacity => "opacity", Ratio, false, "The node's opacity (`Node::with_opacity`).";
    Display => "display", Display, false, "Whether the node is shown (`Node::hidden`).";
}

impl fmt::Display for StyleProperty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.css_name())
    }
}

/// An interaction state a declaration can be conditional on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    /// `hover:`.
    Hover,
    /// `focus:`.
    Focus,
    /// `focus-visible:` — the same state as `focus:` on hosts that do not
    /// distinguish keyboard focus (the Windows backend's approximation).
    FocusVisible,
    /// `active:` — pressed.
    Active,
    /// `disabled:`.
    Disabled,
}

impl State {
    /// Its variant spelling.
    #[must_use]
    pub const fn variant(self) -> &'static str {
        match self {
            Self::Hover => "hover",
            Self::Focus => "focus",
            Self::FocusVisible => "focus-visible",
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }
}

/// A colour scheme a declaration can be conditional on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Scheme {
    /// Light.
    #[default]
    Light,
    /// Dark (`dark:`).
    Dark,
}

/// A layout direction a declaration can be conditional on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Direction {
    /// `ltr:`.
    #[default]
    Ltr,
    /// `rtl:`.
    Rtl,
}

/// The precision of the primary pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Pointer {
    /// A mouse or pen (`pointer-fine:`).
    #[default]
    Fine,
    /// A finger (`pointer-coarse:`).
    Coarse,
}

/// When a declaration applies. Every set field must hold; an empty
/// condition always holds. Stacked variants (`dark:md:hover:`) set several
/// fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Condition {
    /// An interaction state (`hover:`, …): the declaration becomes part of
    /// the node's state style for it.
    pub state: Option<State>,
    /// A colour scheme (`dark:`).
    pub scheme: Option<Scheme>,
    /// A minimum window width in logical pixels (`sm:`, `md:`, …).
    pub min_width: Option<u32>,
    /// A layout direction (`rtl:`, `ltr:`).
    pub direction: Option<Direction>,
    /// Reduced motion requested (`motion-reduce:`) or not
    /// (`motion-safe:`).
    pub reduced_motion: Option<bool>,
    /// A pointer precision (`pointer-coarse:`, `pointer-fine:`).
    pub pointer: Option<Pointer>,
}

impl Condition {
    /// Always holds.
    pub const ALWAYS: Self = Self {
        state: None,
        scheme: None,
        min_width: None,
        direction: None,
        reduced_motion: None,
        pointer: None,
    };

    /// Whether every non-state part holds in `env`. (The state part is
    /// the backend's: it decides which state style it paints.)
    #[must_use]
    pub fn holds_in(&self, env: &ConditionEnv) -> bool {
        self.scheme.is_none_or(|scheme| scheme == env.scheme)
            && self.min_width.is_none_or(|width| env.width >= width)
            && self.direction.is_none_or(|direction| direction == env.direction)
            && self.reduced_motion.is_none_or(|reduced| reduced == env.reduced_motion)
            && self.pointer.is_none_or(|pointer| pointer == env.pointer)
    }

    /// Whether the condition reads anything but the node's own state —
    /// i.e. whether a change to the environment can change its outcome.
    #[must_use]
    pub const fn reads_environment(&self) -> bool {
        self.scheme.is_some()
            || self.min_width.is_some()
            || self.direction.is_some()
            || self.reduced_motion.is_some()
            || self.pointer.is_some()
    }
}

impl fmt::Display for Condition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.scheme == Some(Scheme::Dark) {
            f.write_str("dark:")?;
        }
        if self.scheme == Some(Scheme::Light) {
            f.write_str("light:")?;
        }
        if let Some(width) = self.min_width {
            write!(f, "min-[{width}px]:")?;
        }
        match self.direction {
            Some(Direction::Ltr) => f.write_str("ltr:")?,
            Some(Direction::Rtl) => f.write_str("rtl:")?,
            None => {}
        }
        match self.reduced_motion {
            Some(true) => f.write_str("motion-reduce:")?,
            Some(false) => f.write_str("motion-safe:")?,
            None => {}
        }
        match self.pointer {
            Some(Pointer::Fine) => f.write_str("pointer-fine:")?,
            Some(Pointer::Coarse) => f.write_str("pointer-coarse:")?,
            None => {}
        }
        if let Some(state) = self.state {
            write!(f, "{}:", state.variant())?;
        }
        Ok(())
    }
}

/// What a [`Condition`] is evaluated against: the parts of the
/// environment the variants read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConditionEnv {
    /// The colour scheme.
    pub scheme: Scheme,
    /// The window's width in logical pixels.
    pub width: u32,
    /// The layout direction.
    pub direction: Direction,
    /// Whether reduced motion is requested.
    pub reduced_motion: bool,
    /// The primary pointer's precision.
    pub pointer: Pointer,
}

/// One property set to one value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Declaration {
    /// The property.
    pub property: StyleProperty,
    /// The value.
    pub value: StyleValue,
}

impl fmt::Display for Declaration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.property, self.value)
    }
}

/// A declaration and when it applies.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConditionalDeclaration {
    /// When.
    pub condition: Condition,
    /// What.
    pub declaration: Declaration,
}

impl fmt::Display for ConditionalDeclaration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.condition, self.declaration)
    }
}

/// How a [`DeclarationSet`] was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SetKind {
    /// Utility classes (`classes!`, `class=`).
    #[default]
    Classes,
    /// Declarations (`styles!`, `style=`).
    Declarations,
}

/// The declarations one `classes!`/`styles!` call lowered to, in source
/// order (a later declaration of the same property, under the same
/// condition, wins), with the class or declaration each came from — what
/// the inspector reports as its provenance. Built at compile time; copying
/// one copies two pointers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DeclarationSet {
    declarations: &'static [ConditionalDeclaration],
    sources: &'static [&'static str],
    kind: SetKind,
}

impl DeclarationSet {
    /// No declarations.
    pub const EMPTY: Self = Self { declarations: &[], sources: &[], kind: SetKind::Classes };

    /// Wraps declarations a macro wrote into a `static`, with no record of
    /// where each came from.
    #[must_use]
    pub const fn from_static(declarations: &'static [ConditionalDeclaration]) -> Self {
        Self { declarations, sources: &[], kind: SetKind::Classes }
    }

    /// Wraps declarations with, for each, the class or declaration it came
    /// from (`sources` has one entry per declaration).
    #[must_use]
    pub const fn from_sources(
        declarations: &'static [ConditionalDeclaration],
        sources: &'static [&'static str],
        kind: SetKind,
    ) -> Self {
        Self { declarations, sources, kind }
    }

    /// The declarations, in order.
    #[must_use]
    pub const fn declarations(self) -> &'static [ConditionalDeclaration] {
        self.declarations
    }

    /// The class or declaration declaration `index` came from, when
    /// recorded.
    #[must_use]
    pub fn source(self, index: usize) -> Option<&'static str> {
        self.sources.get(index).copied()
    }

    /// How the set was written.
    #[must_use]
    pub const fn kind(self) -> SetKind {
        self.kind
    }

    /// Whether any declaration depends on the environment.
    #[must_use]
    pub fn reads_environment(self) -> bool {
        self.declarations.iter().any(|declaration| declaration.condition.reads_environment())
    }
}

impl fmt::Display for DeclarationSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, declaration) in self.declarations.iter().enumerate() {
            if index > 0 {
                f.write_str("; ")?;
            }
            write!(f, "{declaration}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_prints_without_trailing_zeros_and_rounds_half_away_from_zero() {
        assert_eq!(Fixed::from_milli(-250).to_string(), "-0.25");
        assert_eq!(Fixed::from_int(4).to_string(), "4");
        assert_eq!(round_to_i32(2.5), 3);
        assert_eq!(round_to_i32(-2.5), -3);
        assert_eq!(round_to_i32(f64::INFINITY), i32::MAX);
    }

    #[test]
    fn lengths_convert_through_the_root_and_node_font_sizes() {
        assert_eq!(Length::Rem(Fixed::from_milli(250)).to_px(16.0, 20.0), 4);
        assert_eq!(Length::Em(Fixed::from_milli(1_500)).to_px(16.0, 20.0), 30);
        assert_eq!(Length::Rem(Fixed::ONE).to_px(16.0 * 1.25, 0.0), 20);
    }

    #[test]
    fn a_condition_holds_only_where_every_part_does() {
        let dark_md =
            Condition { scheme: Some(Scheme::Dark), min_width: Some(768), ..Condition::ALWAYS };
        let env = ConditionEnv { scheme: Scheme::Dark, width: 800, ..ConditionEnv::default() };
        assert!(dark_md.holds_in(&env));
        assert!(!dark_md.holds_in(&ConditionEnv { width: 700, ..env }));
        assert!(!dark_md.holds_in(&ConditionEnv { scheme: Scheme::Light, ..env }));
        assert_eq!(dark_md.to_string(), "dark:min-[768px]:");
    }

    #[test]
    fn every_property_has_a_unique_css_name() {
        let mut names: Vec<&str> =
            StyleProperty::ALL.iter().map(|property| property.css_name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), StyleProperty::ALL.len());
    }
}
