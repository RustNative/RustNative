//! Declaration values: the subset of CSS value syntax the vocabulary
//! admits, parsed per property kind (`PLAN.md` Milestone 58, "The
//! declaration vocabulary").

use std::borrow::Cow;

use crate::color::{ParsedColor, parse_color, split_top_level};
use crate::model::{Fixed, Keyword, Length, ShadowLayer, StyleValue, ValueKind};

/// `var(--name)` → `name`.
#[must_use]
pub fn var_name(text: &str) -> Option<&str> {
    let (function, arguments) = function(text)?;
    if function != "var" {
        return None;
    }
    let name = arguments.trim().strip_prefix("--")?;
    let valid = !name.is_empty()
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        });
    valid.then_some(name)
}

/// `name(arguments)` → `(name, arguments)`, when the parentheses close at
/// the end.
#[must_use]
pub fn function(text: &str) -> Option<(&str, &str)> {
    let text = text.trim();
    let open = text.find('(')?;
    let name = &text[..open];
    if name.is_empty()
        || !name.chars().all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return None;
    }
    let inner = text[open + 1..].strip_suffix(')')?;
    // The closing parenthesis must match the opening one.
    let mut depth = 0_i32;
    for character in inner.chars() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            _ => {}
        }
    }
    (depth == 0).then_some((name, inner))
}

/// A length, or a token-bearing length: what a length-kind property
/// accepts.
#[derive(Debug, Clone, PartialEq)]
enum LengthLike {
    Length(Length),
    Number(f64),
    Token(String, f64),
}

/// Parses a length: `0`, `12px`, `1.5rem`, `2em`, `var(--x)`, or `calc()`
/// over those.
fn length_like(text: &str) -> Result<LengthLike, String> {
    let text = text.trim();
    if let Some(name) = var_name(text) {
        return Ok(LengthLike::Token(name.to_owned(), 1.0));
    }
    if let Some(("calc", expression)) = function(text) {
        return Calc::new(expression).parse();
    }
    literal_length(text)
}

fn literal_length(text: &str) -> Result<LengthLike, String> {
    let split = text
        .char_indices()
        .find(|(_, character)| {
            !(character.is_ascii_digit() || matches!(character, '.' | '-' | '+'))
        })
        .map_or(text.len(), |(index, _)| index);
    let (digits, unit) = text.split_at(split);
    let value: f64 = digits.parse().map_err(|_| format!("`{text}` is not a length"))?;
    match unit.to_ascii_lowercase().as_str() {
        "" => Ok(LengthLike::Number(value)),
        "px" => Ok(LengthLike::Length(Length::Px(Fixed::from_f64(value)))),
        "rem" => Ok(LengthLike::Length(Length::Rem(Fixed::from_f64(value)))),
        "em" => Ok(LengthLike::Length(Length::Em(Fixed::from_f64(value)))),
        "%" => Err(format!(
            "`{text}`: a percentage has no typed equivalent here (sizes are fixed, `auto`, or `100%`)"
        )),
        unit => Err(format!("`{unit}` is not a unit the vocabulary admits (px, rem, em)")),
    }
}

/// A tiny `calc()` evaluator: `+ - * /` over numbers, lengths, and one
/// token scaled by a number — the forms the utility layer generates.
struct Calc<'a> {
    tokens: Vec<CalcToken<'a>>,
    position: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CalcToken<'a> {
    Value(&'a str),
    Operator(char),
    Open,
    Close,
}

#[derive(Debug, Clone, PartialEq)]
enum Term {
    Number(f64),
    /// px, rem, em.
    Length([f64; 3]),
    Token(String, f64),
}

impl<'a> Calc<'a> {
    fn new(expression: &'a str) -> Self {
        let mut tokens = Vec::new();
        let bytes = expression.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            let character = bytes[index];
            match character {
                b' ' | b'\t' | b'\n' => index += 1,
                b'(' => {
                    tokens.push(CalcToken::Open);
                    index += 1;
                }
                b')' => {
                    tokens.push(CalcToken::Close);
                    index += 1;
                }
                b'*' | b'/' | b'+' => {
                    tokens.push(CalcToken::Operator(char::from(character)));
                    index += 1;
                }
                // A `-` is an operator after a value, a sign before one.
                b'-' if matches!(tokens.last(), Some(CalcToken::Value(_) | CalcToken::Close))
                    && bytes.get(index + 1) == Some(&b' ') =>
                {
                    tokens.push(CalcToken::Operator('-'));
                    index += 1;
                }
                _ => {
                    let start = index;
                    let mut depth = 0_i32;
                    while index < bytes.len() {
                        match bytes[index] {
                            b'(' => depth += 1,
                            b')' | b' ' | b'*' | b'/' | b'+' if depth == 0 => break,
                            b')' => depth -= 1,
                            _ => {}
                        }
                        index += 1;
                    }
                    tokens.push(CalcToken::Value(&expression[start..index]));
                }
            }
        }
        Self { tokens, position: 0 }
    }

    fn parse(mut self) -> Result<LengthLike, String> {
        let term = self.sum()?;
        if self.position != self.tokens.len() {
            return Err("`calc()` has trailing input".to_owned());
        }
        Ok(match term {
            Term::Number(value) => LengthLike::Number(value),
            Term::Token(name, factor) => LengthLike::Token(name, factor),
            Term::Length([px, rem, em]) => match (px != 0.0, rem != 0.0, em != 0.0) {
                (_, false, false) => LengthLike::Length(Length::Px(Fixed::from_f64(px))),
                (false, true, false) => LengthLike::Length(Length::Rem(Fixed::from_f64(rem))),
                (false, false, true) => LengthLike::Length(Length::Em(Fixed::from_f64(em))),
                _ => {
                    return Err(
                        "`calc()` mixes units the model cannot hold as one length".to_owned()
                    );
                }
            },
        })
    }

    fn sum(&mut self) -> Result<Term, String> {
        let mut left = self.product()?;
        while let Some(CalcToken::Operator(operator @ ('+' | '-'))) =
            self.tokens.get(self.position).copied()
        {
            self.position += 1;
            let right = self.product()?;
            let sign = if operator == '+' { 1.0 } else { -1.0 };
            left = match (left, right) {
                (Term::Number(a), Term::Number(b)) => Term::Number(a + sign * b),
                (Term::Length(a), Term::Length(b)) => {
                    Term::Length([a[0] + sign * b[0], a[1] + sign * b[1], a[2] + sign * b[2]])
                }
                _ => return Err("`calc()` adds a token or mixes a number with a length".to_owned()),
            };
        }
        Ok(left)
    }

    fn product(&mut self) -> Result<Term, String> {
        let mut left = self.factor()?;
        while let Some(CalcToken::Operator(operator @ ('*' | '/'))) =
            self.tokens.get(self.position).copied()
        {
            self.position += 1;
            let right = self.factor()?;
            left = match (left, right, operator) {
                (Term::Number(a), Term::Number(b), '*') => Term::Number(a * b),
                (Term::Number(a), Term::Number(b), _) => Term::Number(a / b),
                (Term::Length(l), Term::Number(n), '*')
                | (Term::Number(n), Term::Length(l), '*') => Term::Length(l.map(|v| v * n)),
                (Term::Length(l), Term::Number(n), _) => Term::Length(l.map(|v| v / n)),
                (Term::Token(name, f), Term::Number(n), '*')
                | (Term::Number(n), Term::Token(name, f), '*') => Term::Token(name, f * n),
                (Term::Token(name, f), Term::Number(n), _) => Term::Token(name, f / n),
                _ => return Err("`calc()` multiplies two dimensions".to_owned()),
            };
        }
        Ok(left)
    }

    fn factor(&mut self) -> Result<Term, String> {
        let token = self.tokens.get(self.position).copied();
        self.position += 1;
        match token {
            Some(CalcToken::Open) => {
                let inner = self.sum()?;
                if self.tokens.get(self.position) != Some(&CalcToken::Close) {
                    return Err("`calc()` has an unclosed parenthesis".to_owned());
                }
                self.position += 1;
                Ok(inner)
            }
            Some(CalcToken::Value(text)) => {
                if let Some(name) = var_name(text) {
                    return Ok(Term::Token(name.to_owned(), 1.0));
                }
                if let Some(("calc", inner)) = function(text) {
                    return Ok(match Calc::new(inner).parse()? {
                        LengthLike::Number(n) => Term::Number(n),
                        LengthLike::Token(name, f) => Term::Token(name, f),
                        LengthLike::Length(length) => Term::Length(components(length)),
                    });
                }
                if text == "infinity" {
                    return Ok(Term::Number(9_999.0));
                }
                Ok(match literal_length(text)? {
                    LengthLike::Number(n) => Term::Number(n),
                    LengthLike::Length(length) => Term::Length(components(length)),
                    LengthLike::Token(name, f) => Term::Token(name, f),
                })
            }
            _ => Err("`calc()` is missing a value".to_owned()),
        }
    }
}

fn components(length: Length) -> [f64; 3] {
    match length {
        Length::Px(v) => [v.to_f64(), 0.0, 0.0],
        Length::Rem(v) => [0.0, v.to_f64(), 0.0],
        Length::Em(v) => [0.0, 0.0, v.to_f64()],
    }
}

fn length_value(text: &str) -> Result<StyleValue, String> {
    Ok(match length_like(text)? {
        LengthLike::Length(length) => StyleValue::Length(length),
        LengthLike::Number(0.0) => StyleValue::Length(Length::ZERO),
        LengthLike::Number(_) => return Err(format!("`{text}` needs a unit (px, rem, em)")),
        LengthLike::Token(name, factor) if (factor - 1.0).abs() < f64::EPSILON => {
            StyleValue::Token(name.into())
        }
        LengthLike::Token(name, factor) => StyleValue::Scaled(name.into(), Fixed::from_f64(factor)),
    })
}

/// Parses `text` as a value for a property of `kind`.
///
/// # Errors
///
/// The value is not one the kind admits, with the reason.
pub fn parse_value(kind: ValueKind, text: &str) -> Result<StyleValue, String> {
    let text = text.trim();
    if let Some(name) = var_name(text) {
        return Ok(StyleValue::Token(name.to_owned().into()));
    }
    let keyword = text.to_ascii_lowercase();
    let pick = |pairs: &[(&str, Keyword)]| {
        pairs
            .iter()
            .find(|(name, _)| *name == keyword)
            .map(|(_, keyword)| StyleValue::Keyword(*keyword))
            .ok_or_else(|| {
                let names: Vec<&str> = pairs.iter().map(|(name, _)| *name).collect();
                format!("`{text}` is not one of: {}", names.join(", "))
            })
    };
    match kind {
        ValueKind::Color => match parse_color(text)? {
            ParsedColor::Literal(color) => Ok(StyleValue::Color(color)),
            ParsedColor::Token(name) => Ok(StyleValue::Token(name.into())),
            ParsedColor::Faded(name, percent) => Ok(StyleValue::Faded(name.into(), percent)),
        },
        ValueKind::Length => length_value(text),
        ValueKind::Size => match keyword.as_str() {
            "auto" => Ok(StyleValue::Keyword(Keyword::Auto)),
            "100%" => Ok(StyleValue::Keyword(Keyword::Fill)),
            _ => length_value(text),
        },
        ValueKind::Weight => match keyword.as_str() {
            "normal" => Ok(StyleValue::Number(Fixed::from_int(400))),
            "bold" => Ok(StyleValue::Number(Fixed::from_int(700))),
            _ => match text.parse::<f64>() {
                Ok(weight) if (1.0..=1000.0).contains(&weight) => {
                    Ok(StyleValue::Number(Fixed::from_f64(weight)))
                }
                _ => Err(format!("`{text}` is not a font weight (1–1000, `normal`, `bold`)")),
            },
        },
        ValueKind::Ratio => {
            let value = match text.strip_suffix('%') {
                Some(percent) => percent.trim().parse::<f64>().map(|value| value / 100.0),
                None => text.parse::<f64>(),
            }
            .map_err(|_| format!("`{text}` is not a number or percentage"))?;
            Ok(StyleValue::Number(Fixed::from_f64(value.clamp(0.0, 1.0))))
        }
        ValueKind::Family => {
            if text.is_empty() {
                return Err("an empty font family".to_owned());
            }
            Ok(StyleValue::Family(text.to_owned().into()))
        }
        ValueKind::Shadow => parse_shadow(text).map(|layers| StyleValue::Shadow(layers.into())),
        ValueKind::Alignment => pick(&[
            ("start", Keyword::Start),
            ("flex-start", Keyword::Start),
            ("center", Keyword::Center),
            ("end", Keyword::End),
            ("flex-end", Keyword::End),
            ("stretch", Keyword::Stretch),
        ]),
        ValueKind::SelfAlignment => pick(&[
            ("auto", Keyword::Auto),
            ("start", Keyword::Start),
            ("flex-start", Keyword::Start),
            ("center", Keyword::Center),
            ("end", Keyword::End),
            ("flex-end", Keyword::End),
            ("stretch", Keyword::Stretch),
        ]),
        ValueKind::Overflow => pick(&[
            ("visible", Keyword::Visible),
            ("hidden", Keyword::Clip),
            ("clip", Keyword::Clip),
            ("scroll", Keyword::Scroll),
            ("auto", Keyword::Scroll),
        ]),
        ValueKind::Display => {
            pick(&[("none", Keyword::Hidden), ("block", Keyword::Shown), ("flex", Keyword::Shown)])
        }
    }
}

/// Parses `none` or comma-separated shadow layers:
/// `[inset] <x> <y> [<blur> [<spread>]] <colour>`, lengths in px.
///
/// # Errors
///
/// A layer is malformed, or uses a token (shadow layers are folded at
/// build time).
pub fn parse_shadow(text: &str) -> Result<Vec<ShadowLayer>, String> {
    if text.trim().eq_ignore_ascii_case("none") {
        return Ok(Vec::new());
    }
    split_top_level(text, ',')
        .into_iter()
        .map(|layer| {
            let mut inset = false;
            let mut lengths = Vec::new();
            let mut color = None;
            for part in
                split_top_level(layer.trim(), ' ').into_iter().filter(|part| !part.is_empty())
            {
                if part.eq_ignore_ascii_case("inset") {
                    inset = true;
                } else if let Ok(length) = literal_length(part) {
                    let px = match length {
                        LengthLike::Length(Length::Px(value)) => value,
                        LengthLike::Number(0.0) => Fixed::ZERO,
                        _ => return Err(format!("shadow offsets are pixel lengths: `{part}`")),
                    };
                    lengths.push(px);
                } else {
                    match parse_color(part)? {
                        ParsedColor::Literal(value) => color = Some(value),
                        _ => {
                            return Err(format!(
                                "a shadow's colour is folded at build time: `{part}`"
                            ));
                        }
                    }
                }
            }
            let (x, y, blur, spread) = match lengths.as_slice() {
                [x, y] => (*x, *y, Fixed::ZERO, Fixed::ZERO),
                [x, y, blur] => (*x, *y, *blur, Fixed::ZERO),
                [x, y, blur, spread] => (*x, *y, *blur, *spread),
                _ => return Err(format!("`{layer}` needs two to four lengths")),
            };
            Ok(ShadowLayer {
                x,
                y,
                blur,
                spread,
                color: color.unwrap_or(crate::model::Color::rgba(0, 0, 0, 255)),
                inset,
            })
        })
        .collect()
}

/// The kind a token's namespace promises, for the namespaces the style
/// model types (`--color-*`, `--spacing`, `--radius-*`, `--text-*`,
/// `--font-*`, `--font-weight-*`, `--shadow-*`, `--breakpoint-*`,
/// `--container-*`). A project token in one of these whose value does not
/// parse is an error; the rest are kept untyped.
#[must_use]
pub fn token_kind(name: &str) -> Option<ValueKind> {
    if name.contains("--") {
        return None;
    }
    if name.starts_with("color-") {
        Some(ValueKind::Color)
    } else if name.starts_with("font-weight-") {
        Some(ValueKind::Weight)
    } else if name.starts_with("font-") {
        Some(ValueKind::Family)
    } else if name == "shadow" || name.starts_with("shadow-") {
        Some(ValueKind::Shadow)
    } else if name == "spacing"
        || name == "radius"
        || ["radius-", "text-", "breakpoint-", "container-", "spacing-"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
    {
        Some(ValueKind::Length)
    } else {
        None
    }
}

/// Types a theme token's raw value from its namespace, or `None` when the
/// style model has no typed use for it (`--ease-*` curves, `--animate-*`,
/// line heights, …).
#[must_use]
pub fn type_token(name: &str, raw: &str) -> Option<StyleValue> {
    let raw = raw.trim();
    if raw.starts_with("--theme(") || name.contains("--") {
        return None;
    }
    let Some(kind) = token_kind(name) else {
        if let Ok(ParsedColor::Literal(color)) = parse_color(raw) {
            return Some(StyleValue::Color(color));
        }
        return length_value(raw).ok();
    };
    let value = parse_value(kind, raw).ok()?;
    Some(match value {
        StyleValue::Family(family) => StyleValue::Family(Cow::Owned(family.into_owned())),
        other => other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_and_calc_forms() {
        assert_eq!(
            parse_value(ValueKind::Length, "12px"),
            Ok(StyleValue::Length(Length::Px(Fixed::from_int(12))))
        );
        assert_eq!(
            parse_value(ValueKind::Length, "calc(var(--spacing) * 4)"),
            Ok(StyleValue::Scaled("spacing".into(), Fixed::from_int(4)))
        );
        assert_eq!(
            parse_value(ValueKind::Length, "calc(var(--spacing) * -2.5)"),
            Ok(StyleValue::Scaled("spacing".into(), Fixed::from_milli(-2_500)))
        );
        assert_eq!(
            parse_value(ValueKind::Length, "calc(1rem + 0.5rem)"),
            Ok(StyleValue::Length(Length::Rem(Fixed::from_milli(1_500))))
        );
        assert_eq!(
            parse_value(ValueKind::Length, "calc(infinity * 1px)"),
            Ok(StyleValue::Length(Length::Px(Fixed::from_int(9_999))))
        );
        assert!(parse_value(ValueKind::Length, "calc(1rem + 1px)").is_err());
        assert!(parse_value(ValueKind::Length, "50%").is_err());
        assert_eq!(parse_value(ValueKind::Length, "0"), Ok(StyleValue::Length(Length::ZERO)));
    }

    #[test]
    fn sizes_take_auto_and_full() {
        assert_eq!(parse_value(ValueKind::Size, "100%"), Ok(StyleValue::Keyword(Keyword::Fill)));
        assert_eq!(parse_value(ValueKind::Size, "auto"), Ok(StyleValue::Keyword(Keyword::Auto)));
    }

    #[test]
    fn shadows_parse_every_layer() {
        let layers =
            parse_shadow("0 1px 3px 0 rgb(0 0 0 / 0.1), 0 1px 2px -1px rgb(0 0 0 / 0.1)").unwrap();
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[1].spread, Fixed::from_int(-1));
        assert_eq!(layers[0].color.alpha, 26);
        assert_eq!(parse_shadow("none"), Ok(Vec::new()));
    }

    #[test]
    fn tokens_are_typed_by_namespace() {
        assert_eq!(
            type_token("spacing", "0.25rem"),
            Some(StyleValue::Length(Length::Rem(Fixed::from_milli(250))))
        );
        assert_eq!(
            type_token("font-weight-bold", "700"),
            Some(StyleValue::Number(Fixed::from_int(700)))
        );
        assert_eq!(type_token("text-sm--line-height", "calc(1.25 / 0.875)"), None);
        assert_eq!(type_token("ease-in", "cubic-bezier(0.4, 0, 1, 1)"), None);
        assert!(matches!(
            type_token("font-sans", "ui-sans-serif, system-ui"),
            Some(StyleValue::Family(_))
        ));
    }
}
