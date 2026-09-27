//! The declaration vocabulary as CSS: what a browser applies for a
//! declaration set, shared by the Web backend's realizer (`rustnative_web`)
//! and the client-subset translator (`rustnative_webgen`), which compiles
//! `classes!` in client logic to the same class names and rules at build
//! time.
//!
//! A class is named by the hash of its rule: [`class_name`], `cyrb53` over
//! the text's UTF-16 code units — the browser runtime's `rn.className`
//! computes the same name for the same text.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::model::{
    Condition, ConditionalDeclaration, DeclarationSet, Direction, Keyword, Pointer, Scheme, State,
    StyleProperty, StyleValue,
};

/// `cyrb53` of `text`'s UTF-16 code units: a 53-bit value, exact as a
/// JavaScript number.
///
/// ```
/// use rustnative_style::web::cyrb53;
///
/// assert_eq!(cyrb53(""), 3_338_908_027_751_811);
/// ```
#[must_use]
pub fn cyrb53(text: &str) -> u64 {
    let mut h1: u32 = 0xdead_beef;
    let mut h2: u32 = 0x41c6_ce57;
    for unit in text.encode_utf16() {
        let unit = u32::from(unit);
        h1 = (h1 ^ unit).wrapping_mul(2_654_435_761);
        h2 = (h2 ^ unit).wrapping_mul(1_597_334_677);
    }
    h1 = (h1 ^ (h1 >> 16)).wrapping_mul(2_246_822_507);
    h1 ^= (h2 ^ (h2 >> 13)).wrapping_mul(3_266_489_909);
    h2 = (h2 ^ (h2 >> 16)).wrapping_mul(2_246_822_507);
    h2 ^= (h1 ^ (h1 >> 13)).wrapping_mul(3_266_489_909);
    (u64::from(h2 & 0x001f_ffff) << 32) | u64::from(h1)
}

/// `prefix` followed by `cyrb53(text)` in base 36 — JavaScript's
/// `prefix + hash.toString(36)`.
#[must_use]
pub fn class_name(prefix: &str, text: &str) -> String {
    let mut value = cyrb53(text);
    let mut digits = Vec::new();
    loop {
        let digit = u8::try_from(value % 36).unwrap_or(0);
        digits.push(if digit < 10 { b'0' + digit } else { b'a' + digit - 10 });
        value /= 36;
        if value == 0 {
            break;
        }
    }
    digits.reverse();
    let mut name = String::with_capacity(prefix.len() + digits.len());
    name.push_str(prefix);
    name.extend(digits.into_iter().map(char::from));
    name
}
/// A value as CSS text: tokens stay `var(--…)` references for the
/// browser to resolve, so a theme change is one re-resolution here too.
#[must_use]
pub fn value_css(property: StyleProperty, value: &StyleValue) -> String {
    match (property, value) {
        // The vocabulary's `overflow-scroll` and `overflow-auto` both mean
        // "reachable by scrolling"; the typed mapping writes `auto`.
        (StyleProperty::Overflow, StyleValue::Keyword(Keyword::Scroll)) => "auto".to_owned(),
        (StyleProperty::FontWeight, StyleValue::Number(weight)) => weight.to_string(),
        _ => value.to_string(),
    }
}

/// The media query and the selector suffix a condition becomes.
#[must_use]
pub fn condition_css(condition: &Condition) -> (Vec<String>, String) {
    let mut media = Vec::new();
    match condition.scheme {
        Some(Scheme::Dark) => media.push("(prefers-color-scheme: dark)".to_owned()),
        Some(Scheme::Light) => media.push("(prefers-color-scheme: light)".to_owned()),
        None => {}
    }
    if let Some(width) = condition.min_width {
        media.push(format!("(min-width: {width}px)"));
    }
    match condition.reduced_motion {
        Some(true) => media.push("(prefers-reduced-motion: reduce)".to_owned()),
        Some(false) => media.push("(prefers-reduced-motion: no-preference)".to_owned()),
        None => {}
    }
    match condition.pointer {
        Some(Pointer::Coarse) => media.push("(pointer: coarse)".to_owned()),
        Some(Pointer::Fine) => media.push("(pointer: fine)".to_owned()),
        None => {}
    }
    let mut selector = String::new();
    match condition.direction {
        Some(Direction::Rtl) => selector.push_str(":dir(rtl)"),
        Some(Direction::Ltr) => selector.push_str(":dir(ltr)"),
        None => {}
    }
    match condition.state {
        Some(State::Hover) => selector.push_str(":hover"),
        Some(State::Focus) => selector.push_str(":focus"),
        Some(State::FocusVisible) => selector.push_str(":focus-visible"),
        Some(State::Active) => selector.push_str(":active"),
        Some(State::Disabled) => selector.push_str(":is(:disabled,[aria-disabled=true])"),
        None => {}
    }
    (media, selector)
}

/// Whether a property's CSS depends on the node's parent (its main axis,
/// its alignment) or on the node's kind (its display): those go in the
/// node's own class (`rustnative_web::css::node_rules`), everything else in the set's.
#[must_use]
pub const fn is_contextual(property: StyleProperty) -> bool {
    matches!(
        property,
        StyleProperty::Width
            | StyleProperty::Height
            | StyleProperty::AlignSelf
            | StyleProperty::Display
    )
}

/// Groups declarations by condition, in first-appearance order, keeping
/// source order within each group (a later one of a property wins).
pub fn grouped<'a>(
    declarations: impl Iterator<Item = &'a ConditionalDeclaration>,
) -> Vec<(Condition, Vec<&'a ConditionalDeclaration>)> {
    let mut groups: Vec<(Condition, Vec<&ConditionalDeclaration>)> = Vec::new();
    for declaration in declarations {
        match groups.iter_mut().find(|(condition, _)| *condition == declaration.condition) {
            Some((_, group)) => group.push(declaration),
            None => groups.push((declaration.condition, vec![declaration])),
        }
    }
    // A larger breakpoint's rule comes later, so it wins where both hold —
    // whichever order the classes were written in (as Tailwind orders
    // them).
    groups.sort_by_key(|(condition, _)| condition.min_width.unwrap_or(0));
    groups
}

/// Appends one condition's rule: & (the class) with selector, inside
/// media when it has any.
pub fn wrap(rules: &mut String, media: &[String], selector: &str, body: &str) {
    if media.is_empty() {
        let _ = write!(rules, "&{selector}{{{body}}}");
    } else {
        let _ = write!(rules, "@media {}{{&{selector}{{{body}}}}}", media.join(" and "));
    }
}

/// A declaration set's context-free declarations as rule text (`&` for the
/// class), or `None` when it has none. The `classes!` macro computes the
/// same text at build time for the client modules it writes.
#[must_use]
pub fn declaration_rules(set: DeclarationSet) -> Option<String> {
    let mut rules = String::new();
    for (condition, group) in grouped(
        set.declarations()
            .iter()
            .filter(|declaration| !is_contextual(declaration.declaration.property)),
    ) {
        let (media, selector) = condition_css(&condition);
        let mut body = String::new();
        for declaration in group {
            let property = declaration.declaration.property;
            let value = value_css(property, &declaration.declaration.value);
            let _ = write!(body, "{}:{value};", property.css_name());
        }
        wrap(&mut rules, &media, &selector, &body);
    }
    (!rules.is_empty()).then_some(rules)
}

/// Whether `other` holds whenever `condition` does: each part `other` sets
/// is set the same way in `condition`, except a minimum width, which holds
/// under any larger one.
#[must_use]
pub fn implies(condition: &Condition, other: &Condition) -> bool {
    fn same<T: PartialEq>(a: Option<&T>, b: Option<&T>) -> bool {
        b.is_none() || a == b
    }
    same(condition.state.as_ref(), other.state.as_ref())
        && same(condition.scheme.as_ref(), other.scheme.as_ref())
        && same(condition.direction.as_ref(), other.direction.as_ref())
        && same(condition.reduced_motion.as_ref(), other.reduced_motion.as_ref())
        && same(condition.pointer.as_ref(), other.pointer.as_ref())
        && match (condition.min_width, other.min_width) {
            (_, None) => true,
            (Some(width), Some(needed)) => width >= needed,
            (None, Some(_)) => false,
        }
}

/// What a width or height declaration's value says, if it is a size.
#[must_use]
pub fn size_value(value: &StyleValue) -> Option<SizeValue> {
    match value {
        StyleValue::Keyword(Keyword::Auto) => Some(SizeValue::Auto),
        StyleValue::Keyword(Keyword::Fill) => Some(SizeValue::Fill),
        StyleValue::Length(_) | StyleValue::Token(_) | StyleValue::Scaled(..) => {
            Some(SizeValue::Length(value.to_string()))
        }
        _ => None,
    }
}

/// A contextual size value: what a width or height declaration says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SizeValue {
    /// uto.
    Auto,
    /// Fill what the parent offers.
    Fill,
    /// A CSS length.
    Length(String),
}

/// A condition as the runtime reads it: its parts, and its media query and
/// selector.
#[must_use]
pub fn condition_json(condition: &Condition) -> serde_json::Value {
    let (media, selector) = condition_css(condition);
    serde_json::json!({
        "state": condition.state.map(State::variant),
        "scheme": condition.scheme.map(|scheme| match scheme { Scheme::Dark => "dark", Scheme::Light => "light" }),
        "min": condition.min_width,
        "dir": condition.direction.map(|direction| match direction { Direction::Rtl => "rtl", Direction::Ltr => "ltr" }),
        "motion": condition.reduced_motion,
        "pointer": condition.pointer.map(|pointer| match pointer { Pointer::Fine => "fine", Pointer::Coarse => "coarse" }),
        "media": media,
        "sel": selector,
    })
}

/// What the browser's runtime needs to know about a declaration set to
/// style a node it renders itself: the set's class and rules (which do not
/// depend on the node), its tokens, and its contextual declarations (width,
/// height, self-alignment, display), with their conditions, from which the
/// runtime's `rn.nodeRules` computes the node's own class exactly as
/// `rustnative_web::css::node_rules` does.
#[must_use]
pub fn set_descriptor(set: DeclarationSet) -> serde_json::Value {
    let rules = declaration_rules(set);
    let class = rules.as_deref().map(|rules| class_name("d", rules));
    let tokens: BTreeSet<&str> = set
        .declarations()
        .iter()
        .filter_map(|declaration| declaration.declaration.value.token())
        .collect();
    let contextual: Vec<serde_json::Value> = set
        .declarations()
        .iter()
        .filter(|declaration| is_contextual(declaration.declaration.property))
        .map(|declaration| {
            let value = &declaration.declaration.value;
            let value = match (declaration.declaration.property, value) {
                (StyleProperty::Display, StyleValue::Keyword(Keyword::Hidden)) => {
                    serde_json::json!({ "t": "hidden" })
                }
                (StyleProperty::Display, StyleValue::Keyword(Keyword::Shown)) => {
                    serde_json::json!({ "t": "shown" })
                }
                (StyleProperty::AlignSelf, StyleValue::Keyword(keyword)) => serde_json::json!({
                    "t": match keyword {
                        Keyword::Start => "start",
                        Keyword::Center => "center",
                        Keyword::End => "end",
                        Keyword::Stretch => "stretch",
                        Keyword::Auto => "auto",
                        _ => "none",
                    }
                }),
                (StyleProperty::Width | StyleProperty::Height, _) => match size_value(value) {
                    Some(SizeValue::Auto) => serde_json::json!({ "t": "auto" }),
                    Some(SizeValue::Fill) => serde_json::json!({ "t": "fill" }),
                    Some(SizeValue::Length(css)) => serde_json::json!({ "t": "len", "css": css }),
                    None => serde_json::json!({ "t": "none" }),
                },
                _ => serde_json::json!({ "t": "none" }),
            };
            serde_json::json!({
                "p": declaration.declaration.property.css_name(),
                "v": value,
                "c": condition_json(&declaration.condition),
            })
        })
        .collect();
    serde_json::json!({ "d": class, "rules": rules, "tokens": tokens, "ctx": contextual })
}
