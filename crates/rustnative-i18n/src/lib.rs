//! Message catalogues (`PLAN.md` Milestone 46): a documented subset of
//! Fluent, CLDR plural rules for the shipped locales, and resolution with
//! arguments, plural categories, and grammatical gender.
//!
//! Platform-free, and free of the framework: `rustnative-core` resolves
//! messages through it, `rustnative-build` generates typed message functions
//! from it, and `rustnative i18n` extracts into and merges its files.
//!
//! # The file format
//!
//! One file per locale (`locales/<tag>.ftl`), holding these constructs:
//!
//! ```text
//! # A comment above a message is the translator's context for it.
//! -brand = RustNative
//! hello = Hello, { $name }!
//! inbox-count = { $count ->
//!     [0] Your inbox is empty.
//!     [one] You have one message.
//!    *[other] You have { $count } messages.
//! }
//! invited = { $gender ->
//!     [feminine] { $name } invited you to her team.
//!     [masculine] { $name } invited you to his team.
//!    *[other] { $name } invited you to their team.
//! }
//! about = About { -brand }
//! ```
//!
//! - **Messages** (`name = pattern`) may continue on indented lines.
//! - **Placeables** are `{ $variable }` and `{ -term }`.
//! - **Selectors** are `{ $variable -> … }`. A numeric argument selects an
//!   exact number (`[0]`) first, then its plural category (`[one]`, `[few]`,
//!   …) by the locale's CLDR cardinal rules. A text argument selects by its
//!   text, which is how grammatical gender is expressed. The `*` variant is
//!   the default.
//! - **Terms** (`-name = …`) are shared text messages can reference.
//!
//! ```
//! use rustnative_i18n::{Arg, Catalogues};
//!
//! let catalogues = Catalogues::parse("en", &[
//!     ("en", "inbox = { $n ->\n    [one] One message\n   *[other] { $n } messages\n}\n"),
//!     ("pl", "inbox = { $n ->\n    [one] Jedna wiadomość\n    [few] { $n } wiadomości\n   *[many] { $n } wiadomości\n}\n"),
//! ]).unwrap();
//! assert_eq!(catalogues.format("en", "inbox", &[("n", Arg::Number(1))]), "One message");
//! assert_eq!(catalogues.format("en", "inbox", &[("n", Arg::Number(5))]), "5 messages");
//! assert_eq!(catalogues.format("pl", "inbox", &[("n", Arg::Number(3))]), "3 wiadomości");
//! ```

#![deny(missing_docs)]

use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Write as _;

mod plural;

pub use plural::{PluralCategory, plural_category};

/// An argument to a message.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    /// A number: selects plural categories.
    Number(i64),
    /// Text: selects by its value (grammatical gender, and anything else).
    Text(String),
}

impl fmt::Display for Arg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(value) => write!(f, "{value}"),
            Self::Text(value) => f.write_str(value),
        }
    }
}

impl From<i64> for Arg {
    fn from(value: i64) -> Self {
        Self::Number(value)
    }
}

impl From<i32> for Arg {
    fn from(value: i32) -> Self {
        Self::Number(i64::from(value))
    }
}

impl From<u32> for Arg {
    fn from(value: u32) -> Self {
        Self::Number(i64::from(value))
    }
}

impl From<usize> for Arg {
    fn from(value: usize) -> Self {
        Self::Number(i64::try_from(value).unwrap_or(i64::MAX))
    }
}

impl From<&str> for Arg {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<String> for Arg {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

/// One piece of a pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Element {
    /// Literal text.
    Text(String),
    /// `{ $name }`.
    Variable(String),
    /// `{ -name }`.
    Term(String),
    /// `{ $name -> … }`.
    Select {
        /// The variable selected on.
        selector: String,
        /// The variants, in order.
        variants: Vec<Variant>,
        /// Which variant is the default (`*`).
        default: usize,
    },
}

/// One variant of a selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    /// Its key: a plural category, a number, or any text.
    pub key: String,
    /// What it says.
    pub pattern: Vec<Element>,
}

/// One message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Its identifier.
    pub id: String,
    /// What it says.
    pub pattern: Vec<Element>,
    /// The comment above it: the translator's context.
    pub comment: Option<String>,
    /// The line it starts on, 1-based.
    pub line: usize,
}

impl Message {
    /// Every variable it reads, and whether each selects plural categories
    /// (is a selector whose keys are all plural categories or numbers).
    #[must_use]
    pub fn variables(&self) -> BTreeMap<String, bool> {
        fn walk(pattern: &[Element], into: &mut BTreeMap<String, bool>) {
            for element in pattern {
                match element {
                    Element::Variable(name) => {
                        into.entry(name.clone()).or_insert(false);
                    }
                    Element::Select { selector, variants, .. } => {
                        // Plural when every key is a plural category or a
                        // number: `[feminine] … *[other]` selects on text.
                        let numeric = variants.iter().all(|variant| {
                            PluralCategory::parse(&variant.key).is_some()
                                || variant.key.parse::<i64>().is_ok()
                        });
                        let entry = into.entry(selector.clone()).or_insert(false);
                        *entry |= numeric;
                        for variant in variants {
                            walk(&variant.pattern, into);
                        }
                    }
                    Element::Text(_) | Element::Term(_) => {}
                }
            }
        }
        let mut variables = BTreeMap::new();
        walk(&self.pattern, &mut variables);
        variables
    }
}

/// A problem in a catalogue file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// The line, 1-based.
    pub line: usize,
    /// What is wrong.
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

/// One locale's messages and terms.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Bundle {
    /// Messages, by identifier.
    pub messages: BTreeMap<String, Message>,
    /// Terms, by name (without `-`).
    pub terms: BTreeMap<String, Vec<Element>>,
}

fn is_identifier(text: &str) -> bool {
    let mut characters = text.chars();
    characters.next().is_some_and(|first| first.is_ascii_alphabetic())
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
}

/// Parses a pattern's text (placeables and selectors included).
fn parse_pattern(text: &str, line: usize) -> Result<Vec<Element>, ParseError> {
    let error = |message: String| ParseError { line, message };
    let mut out = Vec::new();
    let mut literal = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        literal.push_str(&rest[..open]);
        rest = &rest[open + 1..];
        // The matching `}`, counting nesting (a selector's variants hold
        // placeables).
        let mut depth = 1;
        let mut close = None;
        for (index, character) in rest.char_indices() {
            match character {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(index);
                        break;
                    }
                }
                _ => {}
            }
        }
        let close = close.ok_or_else(|| error("a `{` is never closed".into()))?;
        let inside = rest[..close].trim();
        rest = &rest[close + 1..];
        if !literal.is_empty() {
            out.push(Element::Text(std::mem::take(&mut literal)));
        }
        if let Some((selector, variants)) = inside.split_once("->") {
            let selector = selector.trim().strip_prefix('$').ok_or_else(|| {
                error(format!(
                    "`{}` cannot be selected on: a selector is a `$variable`",
                    selector.trim()
                ))
            })?;
            out.push(parse_select(selector, variants, line)?);
        } else if let Some(variable) = inside.strip_prefix('$') {
            if !is_identifier(variable) {
                return Err(error(format!("`${variable}` is not a variable name")));
            }
            out.push(Element::Variable(variable.to_owned()));
        } else if let Some(term) = inside.strip_prefix('-') {
            out.push(Element::Term(term.to_owned()));
        } else if let Some(quoted) =
            inside.strip_prefix('"').and_then(|text| text.strip_suffix('"'))
        {
            literal.push_str(quoted);
        } else {
            return Err(error(format!(
                "`{{ {inside} }}` is not a placeable this subset has: use `{{ $variable }}`, `{{ -term }}`, or `{{ $variable -> … }}`"
            )));
        }
    }
    literal.push_str(rest);
    if !literal.is_empty() {
        out.push(Element::Text(literal));
    }
    Ok(out)
}

fn parse_select(selector: &str, body: &str, line: usize) -> Result<Element, ParseError> {
    let error = |message: String| ParseError { line, message };
    let mut variants = Vec::new();
    let mut default = None;
    // Each variant starts at `[` (or `*[`) at the start of a line.
    let mut current: Option<(String, bool, String)> = None;
    let mut finish = |current: Option<(String, bool, String)>,
                      variants: &mut Vec<Variant>|
     -> Result<(), ParseError> {
        if let Some((key, is_default, text)) = current {
            if is_default {
                if default.is_some() {
                    return Err(error(format!("`{selector}` has two default (`*`) variants")));
                }
                default = Some(variants.len());
            }
            variants.push(Variant { key, pattern: parse_pattern(text.trim(), line)? });
        }
        Ok(())
    };
    for raw in body.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (is_default, after) =
            trimmed.strip_prefix('*').map_or((false, trimmed), |after| (true, after));
        if let Some(after) = after.strip_prefix('[') {
            let (key, text) = after
                .split_once(']')
                .ok_or_else(|| error(format!("`[{after}` is never closed")))?;
            finish(current.take(), &mut variants)?;
            current = Some((key.trim().to_owned(), is_default, text.to_owned()));
        } else if let Some((_, _, text)) = current.as_mut() {
            text.push('\n');
            text.push_str(trimmed);
        } else {
            return Err(error(format!("`{trimmed}` is not a variant (`[key] text`)")));
        }
    }
    finish(current.take(), &mut variants)?;
    let default =
        default.ok_or_else(|| error(format!("`{selector}` has no default (`*`) variant")))?;
    Ok(Element::Select { selector: selector.to_owned(), variants, default })
}

/// Parses one locale's file.
///
/// # Errors
///
/// Every construct outside the subset, with its line.
pub fn parse(source: &str) -> Result<Bundle, Vec<ParseError>> {
    let mut bundle = Bundle::default();
    let mut errors = Vec::new();
    let mut comment: Vec<String> = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let number = index + 1;
        index += 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            comment.clear();
            continue;
        }
        if let Some(text) = trimmed.strip_prefix('#') {
            comment.push(text.trim_start_matches('#').trim().to_owned());
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            errors.push(ParseError {
                line: number,
                message: "an indented line outside a message".into(),
            });
            continue;
        }
        let Some((name, first)) = line.split_once('=') else {
            errors.push(ParseError {
                line: number,
                message: format!("`{trimmed}` is not `name = text`"),
            });
            continue;
        };
        // The value continues on indented lines (and a selector's closing
        // `}` on its own line).
        let mut value = first.trim().to_owned();
        while index < lines.len()
            && (lines[index].starts_with(char::is_whitespace) || lines[index].trim() == "}")
            && !lines[index].trim().is_empty()
        {
            value.push('\n');
            value.push_str(lines[index]);
            index += 1;
        }
        let name = name.trim();
        let (is_term, id) = name.strip_prefix('-').map_or((false, name), |id| (true, id));
        if !is_identifier(id) {
            errors.push(ParseError {
                line: number,
                message: format!("`{name}` is not a message name"),
            });
            continue;
        }
        match parse_pattern(&value, number) {
            Ok(pattern) if is_term => {
                bundle.terms.insert(id.to_owned(), pattern);
            }
            Ok(pattern) => {
                let context = (!comment.is_empty()).then(|| comment.join("\n"));
                if bundle.messages.contains_key(id) {
                    errors.push(ParseError {
                        line: number,
                        message: format!("`{id}` is defined twice"),
                    });
                }
                bundle.messages.insert(
                    id.to_owned(),
                    Message { id: id.to_owned(), pattern, comment: context, line: number },
                );
            }
            Err(error) => errors.push(error),
        }
        comment.clear();
    }
    if errors.is_empty() { Ok(bundle) } else { Err(errors) }
}

/// Every locale's bundle, and which locale is the source.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Catalogues {
    source: String,
    bundles: BTreeMap<String, Bundle>,
}

/// A text shown in place of a missing message: its key, bracketed, so it is
/// found on screen rather than silently empty.
#[must_use]
pub fn missing(id: &str) -> String {
    format!("⟦{id}⟧")
}

impl Catalogues {
    /// Parses each `(locale, source)`; `source_locale` is the one messages
    /// are written in first and fall back to.
    ///
    /// # Errors
    ///
    /// Each file's errors, prefixed with its locale.
    pub fn parse(source_locale: &str, files: &[(&str, &str)]) -> Result<Self, Vec<String>> {
        let mut bundles = BTreeMap::new();
        let mut errors = Vec::new();
        for (locale, text) in files {
            match parse(text) {
                Ok(bundle) => {
                    bundles.insert((*locale).to_owned(), bundle);
                }
                Err(found) => {
                    errors.extend(found.iter().map(|error| format!("{locale}.ftl {error}")));
                }
            }
        }
        if errors.is_empty() {
            Ok(Self { source: source_locale.to_owned(), bundles })
        } else {
            Err(errors)
        }
    }

    /// These catalogues with `locale`'s file replaced by `source` — a
    /// translator's edit, applied to a running application.
    ///
    /// # Errors
    ///
    /// The file's errors.
    pub fn with_file(&self, locale: &str, source: &str) -> Result<Self, Vec<String>> {
        let bundle = parse(source).map_err(|errors| {
            errors.iter().map(|error| format!("{locale}.ftl {error}")).collect::<Vec<_>>()
        })?;
        let mut replaced = self.clone();
        replaced.bundles.insert(locale.to_owned(), bundle);
        Ok(replaced)
    }

    /// The source locale.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The locales with a bundle.
    pub fn locales(&self) -> impl Iterator<Item = &str> {
        self.bundles.keys().map(String::as_str)
    }

    /// One locale's bundle.
    #[must_use]
    pub fn bundle(&self, locale: &str) -> Option<&Bundle> {
        self.bundles.get(locale)
    }

    /// The bundle that answers for `locale`: its own, its language's
    /// (`pl` for `pl-PL`), then the source's.
    fn chain<'a>(&'a self, locale: &str) -> impl Iterator<Item = (&'a str, &'a Bundle)> {
        let language = locale.split(['-', '_']).next().unwrap_or(locale).to_ascii_lowercase();
        [locale.to_owned(), language, self.source.clone()]
            .into_iter()
            .filter_map(move |tag| self.bundles.get_key_value(tag.as_str()))
            .map(|(tag, bundle)| (tag.as_str(), bundle))
    }

    /// Formats message `id` for `locale` with `args`. A missing message is
    /// shown as its key (`⟦id⟧`); a missing argument as `{$name}`.
    #[must_use]
    pub fn format(&self, locale: &str, id: &str, args: &[(&str, Arg)]) -> String {
        for (tag, bundle) in self.chain(locale) {
            if let Some(message) = bundle.messages.get(id) {
                let mut out = String::new();
                write_pattern(&mut out, &message.pattern, tag, bundle, args, 0);
                return out;
            }
        }
        missing(id)
    }
}

fn write_pattern(
    out: &mut String,
    pattern: &[Element],
    locale: &str,
    bundle: &Bundle,
    args: &[(&str, Arg)],
    depth: usize,
) {
    let argument = |name: &str| args.iter().find(|(arg, _)| *arg == name).map(|(_, value)| value);
    for element in pattern {
        match element {
            Element::Text(text) => out.push_str(text),
            Element::Variable(name) => match argument(name) {
                Some(value) => out.push_str(&value.to_string()),
                None => {
                    let _ = write!(out, "{{${name}}}");
                }
            },
            Element::Term(name) => match bundle.terms.get(name) {
                // Terms may reference terms; a cycle stops.
                Some(term) if depth < 8 => {
                    write_pattern(out, term, locale, bundle, args, depth + 1);
                }
                _ => {
                    let _ = write!(out, "{{-{name}}}");
                }
            },
            Element::Select { selector, variants, default } => {
                let chosen = match argument(selector) {
                    Some(Arg::Number(number)) => variants
                        .iter()
                        .position(|variant| variant.key.parse::<i64>().ok() == Some(*number))
                        .or_else(|| {
                            let category = plural_category(locale, *number).name();
                            variants.iter().position(|variant| variant.key == category)
                        }),
                    Some(Arg::Text(text)) => {
                        variants.iter().position(|variant| variant.key == *text)
                    }
                    None => None,
                }
                .unwrap_or(*default);
                write_pattern(out, &variants[chosen].pattern, locale, bundle, args, depth);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: &str = "# Shown on the inbox button.\ninbox = { $count ->\n    [0] Your inbox is empty.\n    [one] You have one message.\n   *[other] You have { $count } messages.\n}\ninvited = { $gender ->\n    [feminine] { $name } invited you to her team.\n    [masculine] { $name } invited you to his team.\n   *[other] { $name } invited you to their team.\n}\n-brand = RustNative\nabout = About { -brand }\n";
    const AR: &str = "inbox = { $count ->\n    [zero] لا رسائل\n    [one] رسالة واحدة\n    [two] رسالتان\n    [few] { $count } رسائل\n    [many] { $count } رسالة\n   *[other] { $count } رسالة\n}\n";

    #[test]
    fn plurals_numbers_and_gender_select_their_variants() {
        let catalogues = Catalogues::parse("en", &[("en", EN), ("ar", AR)]).unwrap();
        let inbox = |locale: &str, count: i64| {
            catalogues.format(locale, "inbox", &[("count", Arg::Number(count))])
        };
        assert_eq!(inbox("en", 0), "Your inbox is empty.", "an exact number before the category");
        assert_eq!(inbox("en", 1), "You have one message.");
        assert_eq!(
            inbox("en-GB", 7),
            "You have 7 messages.",
            "the language's bundle answers a region"
        );
        assert_eq!(inbox("ar", 2), "رسالتان");
        assert_eq!(inbox("ar", 3), "3 رسائل");
        assert_eq!(inbox("ar", 11), "11 رسالة");
        let invited = |gender: &str| {
            catalogues.format("en", "invited", &[("gender", gender.into()), ("name", "Ada".into())])
        };
        assert_eq!(invited("feminine"), "Ada invited you to her team.");
        assert_eq!(invited("nonbinary"), "Ada invited you to their team.", "the default");
        assert_eq!(
            catalogues.format("ar", "about", &[]),
            "About RustNative",
            "the source answers what a locale lacks"
        );
        assert_eq!(catalogues.format("en", "nowhere", &[]), "⟦nowhere⟧");
        assert_eq!(catalogues.format("en", "inbox", &[]), "You have {$count} messages.");
        let inbox = &catalogues.bundle("en").unwrap().messages["inbox"];
        assert_eq!(inbox.comment.as_deref(), Some("Shown on the inbox button."));
        assert_eq!(inbox.variables(), BTreeMap::from([("count".to_owned(), true)]));
        let invited = &catalogues.bundle("en").unwrap().messages["invited"];
        assert_eq!(
            invited.variables(),
            BTreeMap::from([("gender".to_owned(), false), ("name".to_owned(), false)]),
            "a gender selector selects on text, though it has an `other`"
        );
    }

    #[test]
    fn what_the_subset_lacks_is_an_error_with_its_line() {
        let errors =
            parse("ok = fine\nbad = { NUMBER($n) }\nsel = { $n ->\n    [one] x\n}\n").unwrap_err();
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert_eq!(errors[0].line, 2);
        assert!(errors[1].message.contains("no default"));
        assert!(parse("x = { $n").is_err());
        assert!(parse("a = 1\na = 2\n").is_err());
    }
}
