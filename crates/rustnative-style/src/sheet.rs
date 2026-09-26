//! The style file (`app.css`): the subset of Tailwind v4's directives that
//! survives without a cascade (`PLAN.md` Milestone 58, "The style file").
//!
//! - `@theme [default] [inline] [reference] [static] { --name: value; … }`
//!   defines tokens; `--namespace-*: initial` clears a namespace and
//!   `--*: initial` clears them all;
//! - `@utility name { declarations; @apply classes; }` defines a project
//!   utility;
//! - `@custom-variant name (condition);` names a condition the framework
//!   can evaluate.
//!
//! Everything else is refused with a diagnostic that says why — selectors
//! and `@media` blocks (no cascade, no matching), `@plugin` and `@config`
//! (no JavaScript toolchain), and `@import "tailwindcss"` (the theme ships
//! with the framework, and a native backend has no browser defaults to
//! reset).

use std::ops::Range;

/// A diagnostic in a style source, with the byte range it points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleError {
    /// What is wrong, and — where there is one — what to write instead.
    pub message: String,
    /// The bytes it concerns, within the source that was parsed.
    pub range: Range<usize>,
}

impl StyleError {
    pub(crate) fn new(message: impl Into<String>, range: Range<usize>) -> Self {
        Self { message: message.into(), range }
    }

    /// The 1-based line and column of the start of the range in `source`.
    #[must_use]
    pub fn line_column(&self, source: &str) -> (usize, usize) {
        let before = &source[..self.range.start.min(source.len())];
        let line = before.matches('\n').count() + 1;
        let column = before.rsplit('\n').next().map_or(0, |last| last.chars().count()) + 1;
        (line, column)
    }
}

impl std::fmt::Display for StyleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// One `--name: value` in a `@theme` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenDecl {
    /// The name without its `--`.
    pub name: String,
    /// The raw value.
    pub value: String,
    /// Where the declaration is.
    pub range: Range<usize>,
}

/// One line of a `@utility` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UtilityItem {
    /// `name: value`.
    Declaration {
        /// The property name.
        name: String,
        /// The raw value.
        value: String,
        /// Where it is.
        range: Range<usize>,
    },
    /// `@apply classes`.
    Apply {
        /// The class string.
        classes: String,
        /// Where the class string starts.
        range: Range<usize>,
    },
}

/// A top-level item of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// A `@theme` block.
    Theme {
        /// `inline`: values are folded at build time rather than kept as
        /// references.
        inline: bool,
        /// The block's tokens, in order.
        tokens: Vec<TokenDecl>,
    },
    /// `@utility`.
    Utility {
        /// Its name.
        name: String,
        /// Where the name is.
        range: Range<usize>,
        /// Its body.
        items: Vec<UtilityItem>,
    },
    /// `@custom-variant`.
    CustomVariant {
        /// Its name.
        name: String,
        /// The parenthesized condition, without the parentheses.
        condition: String,
        /// Where the condition is.
        range: Range<usize>,
    },
}

/// A parsed style file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sheet {
    /// Its items, in order.
    pub items: Vec<Item>,
}

/// Parses a style file.
///
/// # Errors
///
/// Every refused or malformed construct, each with its range.
pub fn parse(source: &str) -> Result<Sheet, Vec<StyleError>> {
    let text = blank_comments(source);
    let mut parser = Parser { text: &text, position: 0, errors: Vec::new() };
    let mut sheet = Sheet::default();
    loop {
        parser.skip_space();
        if parser.position >= text.len() {
            break;
        }
        if let Some(item) = parser.top_level() {
            sheet.items.push(item);
        }
    }
    if parser.errors.is_empty() { Ok(sheet) } else { Err(parser.errors) }
}

/// Replaces `/* … */` with spaces, keeping every byte offset.
fn blank_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        let end = after.find("*/").map_or(after.len(), |end| end + 2);
        out.extend(
            after[..end].chars().map(|character| if character == '\n' { '\n' } else { ' ' }),
        );
        // Keep byte offsets exact for multi-byte characters in comments.
        let replaced: usize =
            after[..end].chars().map(char::len_utf8).sum::<usize>() - after[..end].chars().count();
        out.extend(std::iter::repeat_n(' ', replaced));
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

struct Parser<'a> {
    text: &'a str,
    position: usize,
    errors: Vec<StyleError>,
}

impl Parser<'_> {
    fn rest(&self) -> &str {
        // Past the end after an unterminated block: nothing is left.
        self.text.get(self.position..).unwrap_or_default()
    }

    fn skip_space(&mut self) {
        let rest = self.rest();
        self.position += rest.len() - rest.trim_start().len();
    }

    /// The end of the statement or block starting here: the index just past
    /// a top-level `;`, or past the `}` matching the first top-level `{`.
    /// Quotes and parentheses are respected.
    fn statement_end(&self) -> (usize, Option<usize>) {
        let bytes = self.text.as_bytes();
        let mut depth = 0_i32;
        let mut brace: Option<usize> = None;
        let mut braces = 0_i32;
        let mut quote: Option<u8> = None;
        let mut index = self.position;
        while index < bytes.len() {
            let byte = bytes[index];
            if let Some(open) = quote {
                if byte == open {
                    quote = None;
                }
            } else {
                match byte {
                    b'"' | b'\'' => quote = Some(byte),
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    b';' if depth == 0 && braces == 0 => return (index + 1, None),
                    b'{' if depth == 0 => {
                        if braces == 0 {
                            brace = Some(index);
                        }
                        braces += 1;
                    }
                    b'}' if depth == 0 => {
                        braces -= 1;
                        if braces == 0 {
                            return (index + 1, brace);
                        }
                    }
                    _ => {}
                }
            }
            index += 1;
        }
        (bytes.len(), brace)
    }

    fn top_level(&mut self) -> Option<Item> {
        let start = self.position;
        let (end, brace) = self.statement_end();
        self.position = end;
        // A block that runs off the end of the file (half-typed in an
        // editor) is an error, not a block.
        if let Some(open) = brace {
            if !self.text[..end].ends_with('}') {
                self.errors.push(StyleError::new("this `{` is never closed", open..open + 1));
                return None;
            }
        }
        let statement = &self.text[start..end];
        let range = start..end;
        let Some(at_rule) = statement.strip_prefix('@') else {
            self.errors.push(StyleError::new(
                "a selector rule: the style model has no selectors — a declaration block is attached to a node \
                 by the code that writes it (`class=`, `classes!`, `styles!`); define a reusable one with \
                 `@utility`",
                start..start + statement.find('{').unwrap_or(statement.len()),
            ));
            return None;
        };
        let keyword: String = at_rule
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '-')
            .collect();
        let prelude_end = brace.unwrap_or(end);
        let prelude =
            self.text[start + 1 + keyword.len()..prelude_end].trim().trim_end_matches(';').trim();
        let body = brace.map(|open| (open + 1, end - 1));
        match keyword.as_str() {
            "theme" => self.theme(prelude, body, range),
            "utility" => self.utility(prelude, start + 1 + keyword.len(), body, range),
            "custom-variant" => self.custom_variant(prelude, start, body, range),
            "import" => {
                let message = if prelude.contains("tailwindcss") {
                    "`@import \"tailwindcss\"`: the default theme ships with the framework, and the upstream browser \
                     reset has nothing to reset on a native backend — remove the import"
                } else {
                    "`@import`: a project has one style file (`app.css`); move the rules into it"
                };
                self.errors.push(StyleError::new(message, range));
                None
            }
            "plugin" | "config" => {
                self.errors.push(StyleError::new(
                    format!(
                        "`@{keyword}` loads JavaScript: the framework's build is Cargo and a build script, and a \
                         plugin built on selectors could not be honoured without a cascade"
                    ),
                    range,
                ));
                None
            }
            "media" | "supports" | "container" | "layer" | "variant" | "source" | "reference"
            | "keyframes" => {
                self.errors.push(StyleError::new(
                    format!(
                        "`@{keyword}` is not in the style file's subset: conditions are written as variants \
                         (`dark:`, `md:`, `@custom-variant`), and nothing is matched against the tree"
                    ),
                    range,
                ));
                None
            }
            other => {
                self.errors.push(StyleError::new(
                    format!("`@{other}` is not a directive the style file knows"),
                    range,
                ));
                None
            }
        }
    }

    fn theme(
        &mut self,
        prelude: &str,
        body: Option<(usize, usize)>,
        range: Range<usize>,
    ) -> Option<Item> {
        let mut inline = false;
        for word in prelude.split_whitespace() {
            match word {
                "inline" => inline = true,
                "default" | "reference" | "static" => {}
                other => {
                    self.errors.push(StyleError::new(
                        format!("`@theme {other}` is not a theme option"),
                        range.clone(),
                    ));
                }
            }
        }
        let Some((open, close)) = body else {
            self.errors.push(StyleError::new("`@theme` needs a `{ … }` block", range));
            return None;
        };
        let mut tokens = Vec::new();
        for (text, span) in statements(self.text, open, close) {
            let trimmed = text.trim();
            if trimmed.starts_with("@keyframes") {
                // Animation keyframes have no typed consumer in the style
                // model; the vendored theme's are skipped, not an error.
                continue;
            }
            match trimmed.strip_prefix("--").and_then(|rest| rest.split_once(':')) {
                Some((name, value)) => tokens.push(TokenDecl {
                    name: name.trim().to_owned(),
                    value: value.trim().trim_end_matches(';').trim().to_owned(),
                    range: span,
                }),
                None => self.errors.push(StyleError::new(
                    "a `@theme` block holds only custom properties (`--name: value;`)",
                    span,
                )),
            }
        }
        Some(Item::Theme { inline, tokens })
    }

    fn utility(
        &mut self,
        prelude: &str,
        name_start: usize,
        body: Option<(usize, usize)>,
        range: Range<usize>,
    ) -> Option<Item> {
        let name = prelude.to_owned();
        let offset =
            self.text[name_start..].find(prelude).map_or(name_start, |found| name_start + found);
        let name_range = offset..offset + name.len();
        if name.ends_with("-*") {
            self.errors.push(StyleError::new(
                "functional utilities (`name-*` with `--value()`) are not in the style file's subset; define each \
                 utility by name",
                name_range,
            ));
            return None;
        }
        let valid = !name.is_empty()
            && name.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            });
        if !valid {
            self.errors
                .push(StyleError::new(format!("`{name}` is not a utility name"), name_range));
            return None;
        }
        let Some((open, close)) = body else {
            self.errors.push(StyleError::new("`@utility` needs a `{ … }` block", range));
            return None;
        };
        let mut items = Vec::new();
        for (text, span) in statements(self.text, open, close) {
            let trimmed = text.trim().trim_end_matches(';').trim();
            if let Some(classes) = trimmed.strip_prefix("@apply") {
                let classes_start = span.start + text.find("@apply").unwrap_or(0) + "@apply".len();
                let leading = classes.len() - classes.trim_start().len();
                items.push(UtilityItem::Apply {
                    classes: classes.trim().to_owned(),
                    range: classes_start + leading..classes_start + leading + classes.trim().len(),
                });
            } else if let Some((name, value)) = trimmed.split_once(':') {
                items.push(UtilityItem::Declaration {
                    name: name.trim().to_owned(),
                    value: value.trim().to_owned(),
                    range: span,
                });
            } else {
                self.errors.push(StyleError::new(
                    "a `@utility` body holds declarations (`name: value;`) and `@apply`; nested rules would be \
                     selectors",
                    span,
                ));
            }
        }
        Some(Item::Utility { name, range: name_range, items })
    }

    fn custom_variant(
        &mut self,
        prelude: &str,
        start: usize,
        body: Option<(usize, usize)>,
        range: Range<usize>,
    ) -> Option<Item> {
        if body.is_some() {
            self.errors.push(StyleError::new(
                "the block form of `@custom-variant` (with `@slot`) wraps selectors; write the short form, \
                 `@custom-variant name (condition);`",
                range,
            ));
            return None;
        }
        let Some((name, condition)) = prelude.split_once('(') else {
            self.errors.push(StyleError::new("`@custom-variant` needs `name (condition)`", range));
            return None;
        };
        let condition = condition.trim().strip_suffix(')').unwrap_or(condition).trim().to_owned();
        let offset = self.text[start..].find(&condition).map_or(start, |found| start + found);
        Some(Item::CustomVariant {
            name: name.trim().to_owned(),
            range: offset..offset + condition.len(),
            condition,
        })
    }
}

/// The `;`-terminated statements (and nested blocks) between `open` and
/// `close`, with their ranges.
fn statements(text: &str, open: usize, close: usize) -> Vec<(&str, Range<usize>)> {
    let mut out = Vec::new();
    let mut parser = Parser { text: &text[..close], position: open, errors: Vec::new() };
    loop {
        parser.skip_space();
        if parser.position >= close {
            break;
        }
        let start = parser.position;
        let (end, _) = parser.statement_end();
        parser.position = end;
        out.push((&text[start..end], start..end));
    }
    out
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_file_cut_off_anywhere_is_an_error_not_a_panic() {
        // What an editor holds between keystrokes: every prefix of a real
        // style file (the development loop and the language server parse
        // them; a panic here stopped both).
        let file = "@theme {
  --color-accent: oklch(0.55 0.19 255);
}

@utility headline {
  @apply text-lg font-semibold text-accent;
}
@custom-variant pointer-coarse (@media (pointer: coarse));
";
        for end in 0..=file.len() {
            if file.is_char_boundary(end) {
                let _ = parse(&file[..end]);
            }
        }
        assert!(parse("@theme {").is_err());
    }

    use super::*;

    #[test]
    fn a_theme_a_utility_and_a_variant_parse() {
        let source = "/* tokens */\n@theme {\n  --color-primary: oklch(0.6 0.2 250);\n  --color-*: initial;\n}\n\
                      @utility card {\n  padding: 1rem;\n  @apply rounded-lg bg-white;\n}\n\
                      @custom-variant touch (@media (pointer: coarse));\n";
        let sheet = parse(source).unwrap();
        assert_eq!(sheet.items.len(), 3);
        let Item::Theme { tokens, inline } = &sheet.items[0] else { panic!("a theme") };
        assert!(!inline);
        assert_eq!(tokens[0].name, "color-primary");
        assert_eq!(tokens[1].value, "initial");
        let Item::Utility { name, items, .. } = &sheet.items[1] else { panic!("a utility") };
        assert_eq!(name, "card");
        let UtilityItem::Apply { classes, range } = &items[1] else { panic!("an apply") };
        assert_eq!(&source[range.clone()], classes);
        let Item::CustomVariant { condition, .. } = &sheet.items[2] else { panic!("a variant") };
        assert_eq!(condition, "@media (pointer: coarse)");
    }

    #[test]
    fn refused_constructs_say_why_and_where() {
        let source = "@import \"tailwindcss\";\n.button { color: red; }\n@plugin \"x\";\n";
        let errors = parse(source).unwrap_err();
        assert_eq!(errors.len(), 3);
        assert!(errors[0].message.contains("ships with the framework"));
        assert!(errors[1].message.contains("no selectors"));
        assert_eq!(errors[1].line_column(source), (2, 1));
        assert!(errors[2].message.contains("JavaScript"));
    }

    #[test]
    fn the_vendored_theme_parses() {
        let sheet = parse(crate::vocabulary::DEFAULT_THEME).unwrap();
        assert!(sheet.items.len() >= 2);
    }
}
