//! The theme's tokens at run time: what a `var(--…)` in a declaration
//! resolves against, and what a theme switch replaces.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};

use crate::model::{Fixed, Length, StyleValue};
use crate::vocabulary::{Vocabulary, faded};

/// Named token values. Cloning one is cheap (the map is shared until
/// written), so every window's theme can carry the same table.
///
/// ```
/// use rustnative_style::{Length, StyleValue, TokenTable};
///
/// let tokens = TokenTable::defaults();
/// // `p-4` is `calc(var(--spacing) * 4)`; with the default 0.25rem
/// // spacing, that is 1rem.
/// let four = StyleValue::Scaled("spacing".into(), rustnative_style::Fixed::from_int(4));
/// assert_eq!(tokens.resolve(&four), Some(StyleValue::Length(Length::Rem(rustnative_style::Fixed::ONE))));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TokenTable {
    tokens: Arc<BTreeMap<Cow<'static, str>, StyleValue>>,
}

static DEFAULTS: LazyLock<TokenTable> = LazyLock::new(|| Vocabulary::defaults().token_table());

impl TokenTable {
    /// No tokens.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// The vendored default theme's tokens (Tailwind CSS v4's defaults).
    #[must_use]
    pub fn defaults() -> Self {
        DEFAULTS.clone()
    }

    /// Sets a token.
    pub fn insert(&mut self, name: impl Into<Cow<'static, str>>, value: StyleValue) {
        Arc::make_mut(&mut self.tokens).insert(name.into(), value);
    }

    /// `self` with a token set.
    #[must_use]
    pub fn with(mut self, name: impl Into<Cow<'static, str>>, value: StyleValue) -> Self {
        self.insert(name, value);
        self
    }

    /// `self` without every token whose name starts with `prefix` — what
    /// `--color-*: initial` does.
    #[must_use]
    pub fn without_namespace(mut self, prefix: &str) -> Self {
        Arc::make_mut(&mut self.tokens).retain(|name, _| !name.starts_with(prefix));
        self
    }

    /// A token's value as written (it may itself reference a token).
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&StyleValue> {
        self.tokens.get(name)
    }

    /// Every token, by name.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &StyleValue)> {
        self.tokens.iter().map(|(name, value)| (name.as_ref(), value))
    }

    /// How many tokens there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// `value` with every token reference followed to a literal, or `None`
    /// when a token is missing, has the wrong kind for the operation, or
    /// refers in a cycle.
    #[must_use]
    pub fn resolve(&self, value: &StyleValue) -> Option<StyleValue> {
        self.resolve_depth(value, 0)
    }

    fn resolve_depth(&self, value: &StyleValue, depth: usize) -> Option<StyleValue> {
        if depth > 16 {
            return None;
        }
        let lookup = |name: &str| self.resolve_depth(self.tokens.get(name)?, depth + 1);
        match value {
            StyleValue::Token(name) => lookup(name),
            StyleValue::Scaled(name, factor) => match lookup(name)? {
                StyleValue::Length(length) => Some(StyleValue::Length(length.scaled(*factor))),
                StyleValue::Number(number) => {
                    Some(StyleValue::Number(Fixed::from_f64(number.to_f64() * factor.to_f64())))
                }
                _ => None,
            },
            StyleValue::Faded(name, percent) => match lookup(name)? {
                StyleValue::Color(color) => Some(StyleValue::Color(faded(color, *percent))),
                _ => None,
            },
            literal => Some(literal.clone()),
        }
    }

    /// A resolved length, if `value` resolves to one (`0` counts).
    #[must_use]
    pub fn length(&self, value: &StyleValue) -> Option<Length> {
        match self.resolve(value)? {
            StyleValue::Length(length) => Some(length),
            StyleValue::Number(number) if number == Fixed::ZERO => Some(Length::ZERO),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Color;

    #[test]
    fn references_chain_and_cycles_stop() {
        let table = TokenTable::empty()
            .with("color-brand", StyleValue::Color(Color::rgb(1, 2, 3)))
            .with("color-primary", StyleValue::Token("color-brand".into()))
            .with("loop", StyleValue::Token("loop".into()));
        assert_eq!(
            table.resolve(&StyleValue::Token("color-primary".into())),
            Some(StyleValue::Color(Color::rgb(1, 2, 3)))
        );
        assert_eq!(
            table.resolve(&StyleValue::Faded("color-primary".into(), 50)),
            Some(StyleValue::Color(Color::rgba(1, 2, 3, 128)))
        );
        assert_eq!(table.resolve(&StyleValue::Token("loop".into())), None);
        assert_eq!(table.resolve(&StyleValue::Token("missing".into())), None);
    }

    #[test]
    fn the_defaults_carry_the_v4_scales() {
        let table = TokenTable::defaults();
        assert_eq!(
            table.get("spacing"),
            Some(&StyleValue::Length(Length::Rem(Fixed::from_milli(250))))
        );
        assert!(matches!(table.get("color-blue-500"), Some(StyleValue::Color(_))));
        assert!(
            matches!(table.get("shadow-md"), Some(StyleValue::Shadow(layers)) if layers.len() == 2)
        );
        assert_eq!(
            table.get("breakpoint-md"),
            Some(&StyleValue::Length(Length::Rem(Fixed::from_int(48))))
        );
        assert_eq!(table.get("font-weight-bold"), Some(&StyleValue::Number(Fixed::from_int(700))));
    }
}
