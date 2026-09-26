//! The utility vocabulary — Tailwind CSS v4's, named with its version
//! because a compatibility claim without one is not a claim — and the
//! declaration vocabulary beneath it.
//!
//! [`Vocabulary`] holds the tokens (the vendored default theme, then the
//! project's `app.css` over it), the project's utilities, and its custom
//! variants; [`Vocabulary::resolve_classes`] lowers a class string and
//! [`Vocabulary::resolve_declarations`] a declaration block, both to
//! [`ConditionalDeclaration`]s, or to spanned errors. An unknown class is an
//! error, never silently nothing — the one place the framework deliberately
//! diverges from the vocabulary it implements.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::LazyLock;

use crate::color::split_top_level;
use crate::model::{
    Color, Condition, ConditionalDeclaration, Declaration, Direction, Fixed, Keyword, Length,
    Pointer, Scheme, State, StyleProperty, StyleValue, ValueKind,
};
use crate::sheet::{self, Item, StyleError, UtilityItem};
use crate::token_table::TokenTable;
use crate::value::{parse_value, token_kind, type_token};

/// The pinned upstream version the utility vocabulary is compatible with.
pub const TAILWIND_VERSION: &str = "4.1.13";

/// The vendored default theme (`VENDORED.md`).
pub const DEFAULT_THEME: &str = include_str!("../vendor/tailwind-theme-4.1.13.css");

#[derive(Debug, Clone, PartialEq, Eq)]
struct Token {
    value: Option<StyleValue>,
    inline: bool,
}

/// The vocabulary one project's styles resolve against.
#[derive(Debug, Clone, Default)]
pub struct Vocabulary {
    tokens: BTreeMap<String, Token>,
    utilities: BTreeMap<String, Vec<ConditionalDeclaration>>,
    variants: BTreeMap<String, Condition>,
    cleared: Vec<String>,
    project_tokens: Vec<String>,
}

static DEFAULTS: LazyLock<Vocabulary> = LazyLock::new(|| {
    let mut vocabulary = Vocabulary::default();
    if let Ok(sheet) = sheet::parse(DEFAULT_THEME) {
        for item in sheet.items {
            if let Item::Theme { inline, tokens } = item {
                for token in tokens {
                    let value = type_token(&token.name, &token.value);
                    vocabulary.tokens.insert(token.name, Token { value, inline });
                }
            }
        }
    }
    vocabulary
});

const STATES: [(&str, State); 5] = [
    ("hover", State::Hover),
    ("focus", State::Focus),
    ("focus-visible", State::FocusVisible),
    ("active", State::Active),
    ("disabled", State::Disabled),
];

impl Vocabulary {
    /// The vocabulary with the vendored default theme and nothing else.
    #[must_use]
    pub fn defaults() -> Self {
        DEFAULTS.clone()
    }

    /// The defaults with a project's style file applied over them.
    ///
    /// # Errors
    ///
    /// Every problem in the file, each with its range in `source`.
    pub fn with_style_file(source: &str) -> Result<Self, Vec<StyleError>> {
        let sheet = sheet::parse(source)?;
        let mut vocabulary = Self::defaults();
        let mut errors = Vec::new();
        let mut pending = Vec::new();
        for item in sheet.items {
            match item {
                Item::Theme { inline, tokens } => {
                    for token in tokens {
                        vocabulary.theme_token(token, inline, &mut errors);
                    }
                }
                Item::CustomVariant { name, condition, range } => {
                    match custom_condition(&condition) {
                        Ok(condition) => {
                            vocabulary.variants.insert(name, condition);
                        }
                        Err(message) => errors.push(StyleError::new(message, range)),
                    }
                }
                Item::Utility { name, range, items } => pending.push((name, range, items)),
            }
        }
        // Utilities resolve after every token and variant is known, and in
        // dependency order: one may `@apply` another defined later.
        let mut remaining = pending;
        while !remaining.is_empty() {
            let names: Vec<String> = remaining.iter().map(|(name, _, _)| name.clone()).collect();
            let waits_on_another = |name: &str, items: &[UtilityItem]| {
                items.iter().any(|item| match item {
                    UtilityItem::Apply { classes, .. } => classes.split_whitespace().any(|class| {
                        let utility = class.rsplit(':').next().unwrap_or(class);
                        utility != name && names.iter().any(|pending| pending == utility)
                    }),
                    UtilityItem::Declaration { .. } => false,
                })
            };
            let (ready, waiting): (Vec<_>, Vec<_>) =
                remaining.into_iter().partition(|(name, _, items)| !waits_on_another(name, items));
            if ready.is_empty() {
                for (name, range, _) in waiting {
                    errors.push(StyleError::new(
                        format!(
                            "`@utility {name}` applies itself, directly or through another utility"
                        ),
                        range,
                    ));
                }
                break;
            }
            for (name, _, items) in ready {
                match vocabulary.project_utility(&items) {
                    Ok(declarations) => {
                        vocabulary.utilities.insert(name, declarations);
                    }
                    Err(mut problems) => errors.append(&mut problems),
                }
            }
            remaining = waiting;
        }
        if errors.is_empty() { Ok(vocabulary) } else { Err(errors) }
    }

    fn theme_token(&mut self, token: sheet::TokenDecl, inline: bool, errors: &mut Vec<StyleError>) {
        if token.value == "initial" {
            let prefix = token.name.trim_end_matches('*');
            if token.name.ends_with('*') {
                self.tokens.retain(|name, _| !name.starts_with(prefix));
                self.project_tokens.retain(|name| !name.starts_with(prefix));
                self.cleared.push(prefix.to_owned());
            } else {
                self.tokens.remove(&token.name);
            }
            return;
        }
        let value = type_token(&token.name, &token.value);
        if value.is_none() {
            if let Some(kind) = token_kind(&token.name) {
                let reason = parse_value(kind, &token.value).err().unwrap_or_default();
                errors.push(StyleError::new(format!("`--{}`: {reason}", token.name), token.range));
                return;
            }
        }
        self.project_tokens.retain(|name| *name != token.name);
        self.project_tokens.push(token.name.clone());
        self.tokens.insert(token.name, Token { value, inline });
    }

    fn project_utility(
        &self,
        items: &[UtilityItem],
    ) -> Result<Vec<ConditionalDeclaration>, Vec<StyleError>> {
        let mut out = Vec::new();
        let mut errors = Vec::new();
        for item in items {
            match item {
                UtilityItem::Declaration { name, value, range } => {
                    match self.declaration(name, value) {
                        Ok(mut declarations) => out.append(&mut declarations),
                        Err(message) => errors.push(StyleError::new(message, range.clone())),
                    }
                }
                UtilityItem::Apply { classes, range } => match self.resolve_classes(classes) {
                    Ok(mut declarations) => out.append(&mut declarations),
                    Err(problems) => errors.extend(problems.into_iter().map(|problem| {
                        StyleError::new(
                            problem.message,
                            range.start + problem.range.start..range.start + problem.range.end,
                        )
                    })),
                },
            }
        }
        if errors.is_empty() { Ok(out) } else { Err(errors) }
    }

    /// The theme's typed tokens, as the table a `Theme` carries.
    #[must_use]
    pub fn token_table(&self) -> TokenTable {
        let mut table = TokenTable::empty();
        for (name, token) in &self.tokens {
            if let Some(value) = &token.value {
                table.insert(Cow::Owned(name.clone()), value.clone());
            }
        }
        table
    }

    /// What the project's style file changed about the default theme: the
    /// namespaces it cleared (as prefixes) and the tokens it set — what
    /// `rustnative_build::compile_styles` writes over the defaults.
    #[must_use]
    pub fn project_theme(&self) -> (Vec<String>, Vec<(String, StyleValue)>) {
        let set = self
            .project_tokens
            .iter()
            .filter_map(|name| {
                let token = self.tokens.get(name)?;
                Some((name.clone(), token.value.clone()?))
            })
            .collect();
        (self.cleared.clone(), set)
    }

    /// Every token name, for completion and diagnostics.
    pub fn token_names(&self) -> impl Iterator<Item = &str> {
        self.tokens.keys().map(String::as_str)
    }

    /// The class names this vocabulary resolves, as suggestions: fixed
    /// utilities, one example of each spacing and sizing prefix (`p-4`),
    /// the theme's token-backed classes (`bg-blue-500`), and the project's
    /// own `@utility` names — what an editor completes in a class string.
    #[must_use]
    pub fn class_names(&self) -> Vec<String> {
        known_classes(self)
    }

    /// Every variant prefix this vocabulary accepts (`hover`, `dark`,
    /// `md`, and the project's own), for tools and suggestions.
    #[must_use]
    pub fn variant_names(&self) -> Vec<String> {
        let mut known: Vec<String> = STATES.iter().map(|(name, _)| (*name).to_owned()).collect();
        known.extend(
            [
                "dark",
                "rtl",
                "ltr",
                "motion-reduce",
                "motion-safe",
                "pointer-coarse",
                "pointer-fine",
            ]
            .map(str::to_owned),
        );
        known.extend(
            self.tokens
                .keys()
                .filter_map(|name| name.strip_prefix("breakpoint-").map(str::to_owned)),
        );
        known.extend(self.variants.keys().cloned());
        known
    }

    /// Lowers a class string.
    ///
    /// # Errors
    ///
    /// One error per class that does not resolve, with the class's range
    /// in `input` and the nearest thing it could have meant.
    pub fn resolve_classes(
        &self,
        input: &str,
    ) -> Result<Vec<ConditionalDeclaration>, Vec<StyleError>> {
        let mut out = Vec::new();
        let mut errors = Vec::new();
        for (start, class) in words(input) {
            match self.class(class) {
                Ok(mut declarations) => out.append(&mut declarations),
                Err(message) => errors.push(StyleError::new(message, start..start + class.len())),
            }
        }
        if errors.is_empty() { Ok(out) } else { Err(errors) }
    }

    /// Lowers a declaration block (`padding: 1rem; color: var(--color-red-500)`).
    ///
    /// # Errors
    ///
    /// One error per declaration that does not lower, with its range.
    pub fn resolve_declarations(
        &self,
        input: &str,
    ) -> Result<Vec<ConditionalDeclaration>, Vec<StyleError>> {
        let mut out = Vec::new();
        let mut errors = Vec::new();
        let mut offset = 0;
        for part in split_top_level(input, ';') {
            let start = offset + (part.len() - part.trim_start().len());
            let range = start..start + part.trim().len();
            offset += part.len() + 1;
            if part.trim().is_empty() {
                continue;
            }
            let Some((name, value)) = part.split_once(':') else {
                errors.push(StyleError::new(
                    format!("`{}` is not a declaration (`name: value`)", part.trim()),
                    range,
                ));
                continue;
            };
            match self.declaration(name.trim(), value.trim()) {
                Ok(mut declarations) => out.append(&mut declarations),
                Err(message) => errors.push(StyleError::new(message, range)),
            }
        }
        if errors.is_empty() { Ok(out) } else { Err(errors) }
    }

    /// One declaration, with the box shorthands and physical aliases
    /// expanded.
    fn declaration(&self, name: &str, value: &str) -> Result<Vec<ConditionalDeclaration>, String> {
        use StyleProperty as P;
        let name = name.to_ascii_lowercase();
        let boxed =
            |properties: [StyleProperty; 4]| -> Result<Vec<ConditionalDeclaration>, String> {
                let values: Vec<&str> =
                    split_top_level(value, ' ').into_iter().filter(|v| !v.is_empty()).collect();
                // top, end, bottom, start
                let [top, end, bottom, start] = match values.as_slice() {
                    [all] => [*all; 4],
                    [vertical, horizontal] => [*vertical, *horizontal, *vertical, *horizontal],
                    [top, horizontal, bottom] => [*top, *horizontal, *bottom, *horizontal],
                    [top, right, bottom, left] => [*top, *right, *bottom, *left],
                    _ => return Err(format!("`{name}` takes one to four lengths")),
                };
                properties
                    .iter()
                    .zip([top, end, bottom, start])
                    .map(|(property, value)| self.lowered(*property, value))
                    .collect()
            };
        let pair = |first: StyleProperty,
                    second: StyleProperty|
         -> Result<Vec<ConditionalDeclaration>, String> {
            let values: Vec<&str> =
                split_top_level(value, ' ').into_iter().filter(|v| !v.is_empty()).collect();
            let (a, b) = match values.as_slice() {
                [both] => (*both, *both),
                [a, b] => (*a, *b),
                _ => return Err(format!("`{name}` takes one or two lengths")),
            };
            Ok(vec![self.lowered(first, a)?, self.lowered(second, b)?])
        };
        match name.as_str() {
            "padding" => boxed([P::PaddingTop, P::PaddingEnd, P::PaddingBottom, P::PaddingStart]),
            "margin" => boxed([P::MarginTop, P::MarginEnd, P::MarginBottom, P::MarginStart]),
            "padding-inline" => pair(P::PaddingStart, P::PaddingEnd),
            "padding-block" => pair(P::PaddingTop, P::PaddingBottom),
            "margin-inline" => pair(P::MarginStart, P::MarginEnd),
            "margin-block" => pair(P::MarginTop, P::MarginBottom),
            // The model is logical: physical left/right are its start/end,
            // equal in left-to-right and mirrored in right-to-left.
            "padding-left" => Ok(vec![self.lowered(P::PaddingStart, value)?]),
            "padding-right" => Ok(vec![self.lowered(P::PaddingEnd, value)?]),
            "margin-left" => Ok(vec![self.lowered(P::MarginStart, value)?]),
            "margin-right" => Ok(vec![self.lowered(P::MarginEnd, value)?]),
            "background" => Ok(vec![self.lowered(P::Background, value)?]),
            "row-gap" | "column-gap" => {
                Err(format!("`{name}`: a container has one gap, along its axis — write `gap`"))
            }
            _ => match StyleProperty::ALL.iter().find(|property| property.css_name() == name) {
                Some(property) => Ok(vec![self.lowered(*property, value)?]),
                None => Err(format!(
                    "`{name}` is not a property in the vocabulary{}",
                    nearest(&name, StyleProperty::ALL.iter().map(|property| property.css_name()))
                        .map(|near| format!(" — did you mean `{near}`?"))
                        .unwrap_or_default()
                )),
            },
        }
    }

    /// Parses, checks, and folds one value for `property`.
    fn lowered(
        &self,
        property: StyleProperty,
        value: &str,
    ) -> Result<ConditionalDeclaration, String> {
        let value = parse_value(property.kind(), value)
            .map_err(|reason| format!("`{property}`: {reason}"))?;
        self.checked(property, value)
    }

    fn checked(
        &self,
        property: StyleProperty,
        value: StyleValue,
    ) -> Result<ConditionalDeclaration, String> {
        if let Some(name) = value.token() {
            self.check_token(name, property)?;
        }
        let value = self.fold_inline(value);
        Ok(ConditionalDeclaration {
            condition: Condition::ALWAYS,
            declaration: Declaration { property, value },
        })
    }

    fn check_token(&self, name: &str, property: StyleProperty) -> Result<(), String> {
        let Some(token) = self.tokens.get(name) else {
            return Err(format!(
                "`--{name}` is not defined in the theme{}",
                nearest(name, self.token_names())
                    .map(|near| format!(" — did you mean `--{near}`?"))
                    .unwrap_or_default()
            ));
        };
        let Some(value) = &token.value else {
            return Err(format!("`--{name}` has a value the style model cannot type"));
        };
        let fits = matches!(
            (property.kind(), value),
            (ValueKind::Color, StyleValue::Color(_))
                | (
                    ValueKind::Length | ValueKind::Size,
                    StyleValue::Length(_) | StyleValue::Number(_)
                )
                | (ValueKind::Weight | ValueKind::Ratio, StyleValue::Number(_))
                | (ValueKind::Family, StyleValue::Family(_))
                | (ValueKind::Shadow, StyleValue::Shadow(_))
        ) || value.token().is_some();
        if fits {
            Ok(())
        } else {
            Err(format!("`--{name}` ({value}) is not a value `{property}` accepts"))
        }
    }

    /// Replaces a reference to an `inline` token with its value.
    fn fold_inline(&self, value: StyleValue) -> StyleValue {
        let Some(name) = value.token() else { return value };
        let Some(Token { value: Some(literal), inline: true }) = self.tokens.get(name) else {
            return value;
        };
        match (&value, literal) {
            (StyleValue::Token(_), _) => literal.clone(),
            (StyleValue::Scaled(_, factor), StyleValue::Length(length)) => {
                StyleValue::Length(length.scaled(*factor))
            }
            (StyleValue::Faded(_, percent), StyleValue::Color(color)) => {
                StyleValue::Color(faded(*color, *percent))
            }
            _ => value,
        }
    }

    fn class(&self, class: &str) -> Result<Vec<ConditionalDeclaration>, String> {
        if class.starts_with('!') || class.ends_with('!') {
            return Err(format!(
                "`{class}`: `!important` has nothing to be important over — there is no cascade"
            ));
        }
        let parts = split_top_level(class, ':');
        let (utility, variants) = parts.split_last().ok_or_else(|| "an empty class".to_owned())?;
        let mut condition = Condition::ALWAYS;
        for variant in variants {
            self.variant(&mut condition, variant)?;
        }
        let declarations = self.utility(utility)?;
        declarations
            .into_iter()
            .map(|declaration| {
                let merged = merge(condition, declaration.condition)
                    .ok_or_else(|| format!("`{class}` combines two conditions that cannot both hold"))?;
                if merged.state.is_some() && !declaration.declaration.property.is_visual() {
                    return Err(format!(
                        "`{class}`: a state variant applies to visual properties (colours, radius, font, shadow); \
                         `{utility}` sets `{}`, which would re-run layout on every hover",
                        declaration.declaration.property
                    ));
                }
                Ok(ConditionalDeclaration { condition: merged, declaration: declaration.declaration })
            })
            .collect()
    }

    fn variant(&self, condition: &mut Condition, variant: &str) -> Result<(), String> {
        let mut add = Condition::ALWAYS;
        if let Some((_, state)) = STATES.iter().find(|(name, _)| *name == variant) {
            add.state = Some(*state);
        } else if let Some(custom) = self.variants.get(variant) {
            add = *custom;
        } else {
            match variant {
                "dark" => add.scheme = Some(Scheme::Dark),
                "rtl" => add.direction = Some(Direction::Rtl),
                "ltr" => add.direction = Some(Direction::Ltr),
                "motion-reduce" => add.reduced_motion = Some(true),
                "motion-safe" => add.reduced_motion = Some(false),
                "pointer-coarse" => add.pointer = Some(Pointer::Coarse),
                "pointer-fine" => add.pointer = Some(Pointer::Fine),
                _ => {
                    if let Some(width) = self.breakpoint(variant)? {
                        add.min_width = Some(width);
                    } else {
                        return Err(unknown_variant(variant, self));
                    }
                }
            }
        }
        *condition = merge(*condition, add)
            .ok_or_else(|| format!("`{variant}:` conflicts with another variant on the class"))?;
        Ok(())
    }

    /// `sm`, `md`, … (from `--breakpoint-*`) or `min-[640px]`, in px.
    fn breakpoint(&self, variant: &str) -> Result<Option<u32>, String> {
        let length = if let Some(arbitrary) =
            variant.strip_prefix("min-[").and_then(|rest| rest.strip_suffix(']'))
        {
            match parse_value(ValueKind::Length, &arbitrary.replace('_', " "))? {
                StyleValue::Length(length) => length,
                _ => return Err(format!("`{variant}:` needs a length")),
            }
        } else if let Some(Token { value: Some(StyleValue::Length(length)), .. }) =
            self.tokens.get(&format!("breakpoint-{variant}"))
        {
            *length
        } else {
            return Ok(None);
        };
        // Breakpoints are fixed at 16px per rem: they key to the window's
        // logical width, not to the text-size setting.
        Ok(u32::try_from(length.to_px(16.0, 16.0)).ok())
    }

    fn utility(&self, utility: &str) -> Result<Vec<ConditionalDeclaration>, String> {
        use StyleProperty as P;
        if let Some(declarations) = self.utilities.get(utility) {
            return Ok(declarations.clone());
        }
        let one =
            |property, value| self.checked(property, value).map(|declaration| vec![declaration]);
        let keyword = |property, keyword| one(property, StyleValue::Keyword(keyword));
        match utility {
            "hidden" => return keyword(P::Display, Keyword::Hidden),
            "block" | "flex" => return keyword(P::Display, Keyword::Shown),
            "overflow-visible" => return keyword(P::Overflow, Keyword::Visible),
            "overflow-hidden" | "overflow-clip" => return keyword(P::Overflow, Keyword::Clip),
            "overflow-scroll" | "overflow-auto" => return keyword(P::Overflow, Keyword::Scroll),
            "rounded" => return one(P::BorderRadius, StyleValue::Token("radius".into())),
            "rounded-none" => return one(P::BorderRadius, StyleValue::Length(Length::ZERO)),
            "rounded-full" => {
                return one(
                    P::BorderRadius,
                    StyleValue::Length(Length::Px(Fixed::from_int(9_999))),
                );
            }
            "shadow" => return one(P::Shadow, StyleValue::Token("shadow".into())),
            "shadow-none" => return one(P::Shadow, StyleValue::Shadow(Cow::Borrowed(&[]))),
            "flex-row" | "flex-col" | "flex-row-reverse" | "flex-col-reverse" => {
                return Err(format!(
                    "`{utility}`: a container's axis is its element — write `<Row>` or `<Column>`"
                ));
            }
            "border" => return Err(border_width(utility)),
            _ => {}
        }
        for (prefix, property) in [("items-", P::AlignItems), ("self-", P::AlignSelf)] {
            if let Some(value) = utility.strip_prefix(prefix) {
                return self
                    .lowered(property, value)
                    .map(|declaration| vec![declaration])
                    .map_err(|_| unknown_class(utility, self));
            }
        }
        let (negative, body) = match utility.strip_prefix('-') {
            Some(body) => (true, body),
            None => (false, utility),
        };
        let spacing: &[(&str, &[StyleProperty])] = &[
            ("p", &[P::PaddingTop, P::PaddingEnd, P::PaddingBottom, P::PaddingStart]),
            ("px", &[P::PaddingStart, P::PaddingEnd]),
            ("py", &[P::PaddingTop, P::PaddingBottom]),
            ("pt", &[P::PaddingTop]),
            ("pb", &[P::PaddingBottom]),
            ("ps", &[P::PaddingStart]),
            ("pe", &[P::PaddingEnd]),
            ("pl", &[P::PaddingStart]),
            ("pr", &[P::PaddingEnd]),
            ("m", &[P::MarginTop, P::MarginEnd, P::MarginBottom, P::MarginStart]),
            ("mx", &[P::MarginStart, P::MarginEnd]),
            ("my", &[P::MarginTop, P::MarginBottom]),
            ("mt", &[P::MarginTop]),
            ("mb", &[P::MarginBottom]),
            ("ms", &[P::MarginStart]),
            ("me", &[P::MarginEnd]),
            ("ml", &[P::MarginStart]),
            ("mr", &[P::MarginEnd]),
            ("gap", &[P::Gap]),
            ("w", &[P::Width]),
            ("h", &[P::Height]),
            ("size", &[P::Width, P::Height]),
            ("min-w", &[P::MinWidth]),
            ("min-h", &[P::MinHeight]),
            ("max-w", &[P::MaxWidth]),
            ("max-h", &[P::MaxHeight]),
        ];
        let split = body.rfind('-').and_then(|_| {
            spacing
                .iter()
                .filter_map(|(prefix, properties)| {
                    body.strip_prefix(prefix)
                        .and_then(|rest| rest.strip_prefix('-'))
                        .map(|rest| (*prefix, *properties, rest))
                })
                .max_by_key(|(prefix, _, _)| prefix.len())
        });
        if let Some((prefix, properties, value)) = split {
            if negative && !prefix.starts_with('m') {
                return Err(format!("`{utility}`: only margins can be negative"));
            }
            let value =
                self.spacing_value(prefix, properties[0], value, negative).map_err(|reason| {
                    if reason.is_empty() {
                        unknown_class(utility, self)
                    } else {
                        format!("`{utility}`: {reason}")
                    }
                })?;
            return properties
                .iter()
                .map(|property| self.checked(*property, value.clone()))
                .collect();
        }
        if negative {
            return Err(unknown_class(utility, self));
        }
        if let Some(value) = body.strip_prefix("bg-") {
            return self.color_utility(utility, P::Background, value);
        }
        if let Some(value) = body.strip_prefix("border-") {
            if value.chars().all(|c| c.is_ascii_digit())
                || value.starts_with('[') && value.ends_with("px]")
            {
                return Err(border_width(utility));
            }
            if ["t", "b", "l", "r", "s", "e", "x", "y"]
                .iter()
                .any(|side| value == *side || value.starts_with(&format!("{side}-")))
            {
                return Err(format!(
                    "`{utility}`: a border is one colour on every side; per-side borders have no typed property"
                ));
            }
            return self.color_utility(utility, P::BorderColor, value);
        }
        if let Some(value) = body.strip_prefix("text-") {
            if self.tokens.contains_key(&format!("text-{value}")) {
                return one(P::FontSize, StyleValue::Token(format!("text-{value}").into()));
            }
            if let Some(arbitrary) = bracketed(value) {
                if let Ok(StyleValue::Length(length)) = parse_value(ValueKind::Length, &arbitrary) {
                    return one(P::FontSize, StyleValue::Length(length));
                }
            }
            return self.color_utility(utility, P::Foreground, value);
        }
        if let Some(value) = body.strip_prefix("font-") {
            if self.tokens.contains_key(&format!("font-weight-{value}")) {
                return one(
                    P::FontWeight,
                    StyleValue::Token(format!("font-weight-{value}").into()),
                );
            }
            if self.tokens.contains_key(&format!("font-{value}")) {
                return one(P::FontFamily, StyleValue::Token(format!("font-{value}").into()));
            }
            if let Some(arbitrary) = bracketed(value) {
                return match parse_value(ValueKind::Weight, &arbitrary) {
                    Ok(weight) => one(P::FontWeight, weight),
                    Err(_) => one(P::FontFamily, StyleValue::Family(arbitrary.into())),
                };
            }
            return Err(unknown_class(utility, self));
        }
        if let Some(value) = body.strip_prefix("rounded-") {
            if let Some(arbitrary) = bracketed(value) {
                return self
                    .lowered(P::BorderRadius, &arbitrary)
                    .map(|declaration| vec![declaration]);
            }
            if self.tokens.contains_key(&format!("radius-{value}")) {
                return one(P::BorderRadius, StyleValue::Token(format!("radius-{value}").into()));
            }
            if ["t", "b", "l", "r", "s", "e", "tl", "tr", "bl", "br", "ss", "se", "es", "ee"]
                .iter()
                .any(|corner| value == *corner || value.starts_with(&format!("{corner}-")))
            {
                return Err(format!(
                    "`{utility}`: the radius is one value for every corner; per-corner radii have no typed property"
                ));
            }
            return Err(unknown_class(utility, self));
        }
        if let Some(value) = body.strip_prefix("shadow-") {
            if let Some(arbitrary) = bracketed(value) {
                return self.lowered(P::Shadow, &arbitrary).map(|declaration| vec![declaration]);
            }
            if self.tokens.contains_key(&format!("shadow-{value}")) {
                return one(P::Shadow, StyleValue::Token(format!("shadow-{value}").into()));
            }
            if self.tokens.contains_key(&format!("color-{value}")) {
                return Err(format!(
                    "`{utility}`: a shadow's colour is part of its layers; write an arbitrary shadow, `shadow-[…]`"
                ));
            }
            return Err(unknown_class(utility, self));
        }
        if let Some(value) = body.strip_prefix("opacity-") {
            if let Some(arbitrary) = bracketed(value) {
                return self.lowered(P::Opacity, &arbitrary).map(|declaration| vec![declaration]);
            }
            return match value.parse::<u16>() {
                Ok(percent) if percent <= 100 => {
                    one(P::Opacity, StyleValue::Number(Fixed::from_milli(i32::from(percent) * 10)))
                }
                _ => Err(unknown_class(utility, self)),
            };
        }
        Err(unknown_class(utility, self))
    }

    /// A spacing-scale value: `4` (`calc(var(--spacing) * 4)`), `px`,
    /// `[12px]`, `(--token)`; for sizes also `auto` and `full`; for
    /// `max-w-*` the container scale. An empty error means "not a value".
    fn spacing_value(
        &self,
        prefix: &str,
        property: StyleProperty,
        value: &str,
        negative: bool,
    ) -> Result<StyleValue, String> {
        let sign = if negative { -1 } else { 1 };
        if let Some(arbitrary) = bracketed(value) {
            let parsed = parse_value(property.kind(), &arbitrary)?;
            return Ok(match (negative, parsed) {
                (true, StyleValue::Length(length)) => {
                    StyleValue::Length(length.scaled(Fixed::from_int(-1)))
                }
                (_, parsed) => parsed,
            });
        }
        if let Some(name) = value.strip_prefix("(--").and_then(|rest| rest.strip_suffix(')')) {
            return Ok(if negative {
                StyleValue::Scaled(name.to_owned().into(), Fixed::from_int(-1))
            } else {
                StyleValue::Token(name.to_owned().into())
            });
        }
        let sized = matches!(property, StyleProperty::Width | StyleProperty::Height);
        match value {
            "px" => return Ok(StyleValue::Length(Length::Px(Fixed::from_int(sign)))),
            "auto" if sized => return Ok(StyleValue::Keyword(Keyword::Auto)),
            "full" if sized => return Ok(StyleValue::Keyword(Keyword::Fill)),
            "full" | "screen" | "svh" | "dvh" | "min" | "max" | "fit" => {
                return Err(format!(
                    "`{value}` has no typed equivalent (sizes are fixed, `auto`, or `full`)"
                ));
            }
            _ => {}
        }
        if value.contains('/') {
            return Err(
                "a fractional size has no typed equivalent (sizes are fixed, `auto`, or `full`)"
                    .to_owned(),
            );
        }
        if prefix == "max-w" && self.tokens.contains_key(&format!("container-{value}")) {
            return Ok(StyleValue::Token(format!("container-{value}").into()));
        }
        let Ok(number) = value.parse::<f64>() else { return Err(String::new()) };
        // v4 accepts the spacing scale in quarter steps: `p-13` and `p-1.5`,
        // not `p-1.3`.
        if (number * 4.0).fract() != 0.0 || number < 0.0 {
            return Err(String::new());
        }
        Ok(StyleValue::Scaled("spacing".into(), Fixed::from_f64(number * f64::from(sign))))
    }

    fn color_utility(
        &self,
        utility: &str,
        property: StyleProperty,
        value: &str,
    ) -> Result<Vec<ConditionalDeclaration>, String> {
        // `bg-sky-500/50`: an opacity modifier after the last `/` outside brackets.
        let (color, modifier) = match split_top_level(value, '/').as_slice() {
            [color, modifier] => (*color, Some(*modifier)),
            [color] => (*color, None),
            _ => return Err(unknown_class(utility, self)),
        };
        let base = if let Some(arbitrary) = bracketed(color) {
            parse_value(ValueKind::Color, &arbitrary)
                .map_err(|reason| format!("`{utility}`: {reason}"))?
        } else if let Some(name) = color.strip_prefix("(--").and_then(|rest| rest.strip_suffix(')'))
        {
            StyleValue::Token(name.to_owned().into())
        } else if color == "transparent" {
            StyleValue::Color(Color::rgba(0, 0, 0, 0))
        } else if matches!(color, "current" | "inherit") {
            return Err(format!(
                "`{utility}` depends on the cascade, which the style model does not have"
            ));
        } else if self.tokens.contains_key(&format!("color-{color}")) {
            StyleValue::Token(format!("color-{color}").into())
        } else {
            return Err(unknown_class(utility, self));
        };
        let value = match modifier {
            None => base,
            Some(modifier) => {
                let percent = match bracketed(modifier) {
                    Some(arbitrary) => match parse_value(ValueKind::Ratio, &arbitrary) {
                        Ok(StyleValue::Number(ratio)) => ratio.milli() / 10,
                        _ => return Err(format!("`{utility}`: `{modifier}` is not an opacity")),
                    },
                    None => {
                        modifier.parse::<i32>().ok().filter(|p| (0..=100).contains(p)).ok_or_else(
                            || format!("`{utility}`: `/{modifier}` is not an opacity (0–100)"),
                        )?
                    }
                };
                let percent = u8::try_from(percent.clamp(0, 100)).unwrap_or(100);
                match base {
                    StyleValue::Token(name) => StyleValue::Faded(name, percent),
                    StyleValue::Color(color) => StyleValue::Color(faded(color, percent)),
                    _ => return Err(format!("`{utility}`: an opacity modifier needs a colour")),
                }
            }
        };
        self.checked(property, value).map(|declaration| vec![declaration])
    }
}

/// A colour at `percent` of its opacity.
#[must_use]
pub fn faded(color: Color, percent: u8) -> Color {
    // Rounded half up, as the colour functions round alpha.
    let alpha = (u16::from(color.alpha) * u16::from(percent.min(100)) + 50) / 100;
    Color::rgba(color.red, color.green, color.blue, u8::try_from(alpha).unwrap_or(u8::MAX))
}

fn border_width(utility: &str) -> String {
    format!(
        "`{utility}`: border width has no typed property (a node's border is the host's own); the colour is `border-{{colour}}`"
    )
}

fn bracketed(value: &str) -> Option<String> {
    value
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .map(|inner| inner.replace('_', " "))
}

/// Both conditions at once, or `None` when they contradict.
fn merge(a: Condition, b: Condition) -> Option<Condition> {
    #[allow(
        clippy::option_option,
        reason = "outer: whether they agree; inner: the agreed value, if any"
    )]
    fn one<T: PartialEq + Copy>(a: Option<T>, b: Option<T>) -> Option<Option<T>> {
        match (a, b) {
            (Some(x), Some(y)) if x != y => None,
            (x, y) => Some(x.or(y)),
        }
    }
    Some(Condition {
        state: one(a.state, b.state)?,
        scheme: one(a.scheme, b.scheme)?,
        min_width: match (a.min_width, b.min_width) {
            (Some(x), Some(y)) => Some(x.max(y)),
            (x, y) => x.or(y),
        },
        direction: one(a.direction, b.direction)?,
        reduced_motion: one(a.reduced_motion, b.reduced_motion)?,
        pointer: one(a.pointer, b.pointer)?,
    })
}

/// A `@custom-variant` condition the framework can evaluate.
fn custom_condition(text: &str) -> Result<Condition, String> {
    let mut condition = Condition::ALWAYS;
    let normalized: String =
        text.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase();
    if let Some(query) = normalized.strip_prefix("@media ") {
        let query = query.trim().trim_start_matches('(').trim_end_matches(')');
        let (feature, value) =
            query.split_once(':').map_or((query, ""), |(f, v)| (f.trim(), v.trim()));
        match (feature, value) {
            ("prefers-color-scheme", "dark") => condition.scheme = Some(Scheme::Dark),
            ("prefers-color-scheme", "light") => condition.scheme = Some(Scheme::Light),
            ("prefers-reduced-motion", "reduce") => condition.reduced_motion = Some(true),
            ("prefers-reduced-motion", "no-preference") => condition.reduced_motion = Some(false),
            ("pointer", "coarse") => condition.pointer = Some(Pointer::Coarse),
            ("pointer", "fine") => condition.pointer = Some(Pointer::Fine),
            ("min-width", width) => match parse_value(ValueKind::Length, width)? {
                StyleValue::Length(length) => {
                    condition.min_width = u32::try_from(length.to_px(16.0, 16.0)).ok();
                }
                _ => return Err(format!("`{text}`: a min-width needs a length")),
            },
            _ => {
                return Err(format!(
                    "`{text}`: the framework evaluates the colour scheme, reduced motion, pointer, and min-width media features"
                ));
            }
        }
        return Ok(condition);
    }
    let state = match normalized.as_str() {
        "&:hover" => Some(State::Hover),
        "&:focus" => Some(State::Focus),
        "&:focus-visible" => Some(State::FocusVisible),
        "&:active" => Some(State::Active),
        "&:disabled" => Some(State::Disabled),
        "&:dir(rtl)" => {
            condition.direction = Some(Direction::Rtl);
            None
        }
        "&:dir(ltr)" => {
            condition.direction = Some(Direction::Ltr);
            None
        }
        _ => {
            return Err(format!(
                "`{text}` selects by other nodes or by markup the framework does not match; a custom variant can name \
                 a state (`&:hover`), `&:dir(rtl)`, or a media feature (colour scheme, reduced motion, pointer, \
                 min-width)"
            ));
        }
    };
    condition.state = state;
    Ok(condition)
}

fn unknown_variant(variant: &str, vocabulary: &Vocabulary) -> String {
    if variant.starts_with("group-")
        || variant.starts_with("peer-")
        || variant.starts_with("has-")
        || variant.starts_with("in-")
    {
        return format!(
            "`{variant}:` is a relational variant — selector matching, which the style model refuses (`PLAN.md` Milestone 58)"
        );
    }
    if variant.starts_with('@') {
        return format!(
            "`{variant}:` is a container query, deferred until layout can answer it (`PLAN.md` Milestone 58)"
        );
    }
    if variant.starts_with("max-") {
        return format!(
            "`{variant}:`: breakpoints are minimums (mobile first); write the smaller style unprefixed and override it at `sm:`/`md:`/…"
        );
    }
    let mut known: Vec<String> = STATES.iter().map(|(name, _)| (*name).to_owned()).collect();
    known.extend(
        ["dark", "rtl", "ltr", "motion-reduce", "motion-safe", "pointer-coarse", "pointer-fine"]
            .map(str::to_owned),
    );
    known.extend(
        vocabulary
            .tokens
            .keys()
            .filter_map(|name| name.strip_prefix("breakpoint-").map(str::to_owned)),
    );
    known.extend(vocabulary.variants.keys().cloned());
    format!(
        "`{variant}:` is not a variant{}",
        nearest(variant, known.iter().map(String::as_str))
            .map(|near| format!(" — did you mean `{near}:`?"))
            .unwrap_or_default()
    )
}

/// Class names the vocabulary resolves, for suggestions and completion:
/// the fixed utilities, one example of each spacing and sizing prefix, one
/// per theme token a utility reads, and the project's own utilities.
fn known_classes(vocabulary: &Vocabulary) -> Vec<String> {
    let mut known: Vec<String> = [
        "hidden",
        "block",
        "flex",
        "overflow-hidden",
        "overflow-scroll",
        "overflow-visible",
        "rounded",
        "rounded-full",
        "rounded-none",
        "shadow",
        "shadow-none",
        "items-center",
        "items-start",
        "items-end",
        "items-stretch",
        "self-center",
        "self-start",
        "self-end",
        "self-stretch",
        "w-full",
        "h-full",
        "w-auto",
        "h-auto",
    ]
    .map(str::to_owned)
    .to_vec();
    for prefix in [
        "p", "px", "py", "pt", "pb", "ps", "pe", "m", "mx", "my", "mt", "mb", "ms", "me", "gap",
        "w", "h", "size", "min-w", "min-h", "max-w", "max-h",
    ] {
        known.push(format!("{prefix}-4"));
    }
    for name in vocabulary.tokens.keys() {
        if let Some(color) = name.strip_prefix("color-") {
            known.extend([
                format!("bg-{color}"),
                format!("text-{color}"),
                format!("border-{color}"),
            ]);
        } else if let Some(size) = name.strip_prefix("text-") {
            known.push(format!("text-{size}"));
        } else if let Some(weight) = name.strip_prefix("font-weight-") {
            known.push(format!("font-{weight}"));
        } else if let Some(radius) = name.strip_prefix("radius-") {
            known.push(format!("rounded-{radius}"));
        } else if let Some(shadow) = name.strip_prefix("shadow-") {
            known.push(format!("shadow-{shadow}"));
        } else if let Some(family) = name.strip_prefix("font-") {
            known.push(format!("font-{family}"));
        }
    }
    known.extend(vocabulary.utilities.keys().cloned());
    known
}

fn unknown_class(utility: &str, vocabulary: &Vocabulary) -> String {
    let known = known_classes(vocabulary);
    format!(
        "`{utility}` is not a class in the vocabulary{}",
        nearest(utility, known.iter().map(String::as_str))
            .map(|near| format!(" — did you mean `{near}`?"))
            .unwrap_or_default()
    )
}

/// The closest candidate by edit distance, when it is close enough to be a
/// plausible typo.
#[must_use]
pub fn nearest<'a>(word: &str, candidates: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let limit = (word.len() / 3).max(2);
    candidates
        .map(|candidate| (distance(word, candidate), candidate))
        .filter(|(d, _)| *d <= limit)
        .min()
        .map(|(_, c)| c)
}

fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let current = row[j + 1];
            row[j + 1] = (previous + usize::from(ca != *cb)).min(row[j] + 1).min(current + 1);
            previous = current;
        }
    }
    row[b.len()]
}

/// Whitespace-separated words with their byte offsets.
fn words(input: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    for (index, character) in input.char_indices() {
        if character.is_whitespace() {
            if let Some(begin) = start.take() {
                out.push((begin, &input[begin..index]));
            }
        } else if start.is_none() {
            start = Some(index);
        }
    }
    if let Some(begin) = start {
        out.push((begin, &input[begin..]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(classes: &str) -> Vec<String> {
        Vocabulary::defaults()
            .resolve_classes(classes)
            .unwrap()
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    fn error(classes: &str) -> StyleError {
        Vocabulary::defaults().resolve_classes(classes).unwrap_err().remove(0)
    }

    #[test]
    fn spacing_colour_and_type_utilities_lower_as_v4_does() {
        assert_eq!(
            resolve("p-4"),
            [
                "padding-top: calc(var(--spacing) * 4)",
                "padding-inline-end: calc(var(--spacing) * 4)",
                "padding-bottom: calc(var(--spacing) * 4)",
                "padding-inline-start: calc(var(--spacing) * 4)"
            ]
        );
        assert_eq!(resolve("bg-blue-500"), ["background-color: var(--color-blue-500)"]);
        assert_eq!(
            resolve("bg-blue-500/50"),
            ["background-color: color-mix(in oklab, var(--color-blue-500) 50%, transparent)"]
        );
        assert_eq!(
            resolve("text-sm font-bold"),
            ["font-size: var(--text-sm)", "font-weight: var(--font-weight-bold)"]
        );
        assert_eq!(resolve("text-red-600"), ["color: var(--color-red-600)"]);
        assert_eq!(resolve("-mt-2"), ["margin-top: calc(var(--spacing) * -2)"]);
        assert_eq!(resolve("w-[37px] h-full"), ["width: 37px", "height: 100%"]);
        assert_eq!(resolve("bg-[oklch(0.62_0.19_259)]").len(), 1);
        assert_eq!(
            resolve("rounded-lg shadow-md"),
            ["border-radius: var(--radius-lg)", "box-shadow: var(--shadow-md)"]
        );
        assert_eq!(resolve("opacity-50"), ["opacity: 0.5"]);
        assert_eq!(resolve("max-w-md"), ["max-width: var(--container-md)"]);
    }

    #[test]
    fn variants_become_conditions() {
        assert_eq!(resolve("hover:bg-blue-600"), ["hover:background-color: var(--color-blue-600)"]);
        assert_eq!(resolve("dark:md:text-white"), ["dark:min-[768px]:color: var(--color-white)"]);
        assert_eq!(resolve("rtl:ps-2"), ["rtl:padding-inline-start: calc(var(--spacing) * 2)"]);
    }

    #[test]
    fn an_unknown_class_is_an_error_at_its_range_naming_the_nearest() {
        let input = "p-4 bg-bleu-500";
        let problem = Vocabulary::defaults().resolve_classes(input).unwrap_err().remove(0);
        assert_eq!(&input[problem.range.clone()], "bg-bleu-500");
        assert!(problem.message.contains("did you mean `bg-blue-500`"), "{}", problem.message);
        assert!(error("hover:p-4").message.contains("visual properties"));
        assert!(error("group-hover:bg-red-500").message.contains("relational"));
        assert!(error("flex-col").message.contains("<Column>"));
        assert!(error("p-1.3").message.contains("not a class"));
    }

    #[test]
    fn declarations_lower_with_shorthands_and_tokens() {
        let vocabulary = Vocabulary::defaults();
        let lowered: Vec<String> = vocabulary
            .resolve_declarations("padding: 1rem 2px; background: var(--color-red-500)")
            .unwrap()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            lowered,
            [
                "padding-top: 1rem",
                "padding-inline-end: 2px",
                "padding-bottom: 1rem",
                "padding-inline-start: 2px",
                "background-color: var(--color-red-500)"
            ]
        );
        let problem = vocabulary.resolve_declarations("colr: red").unwrap_err().remove(0);
        assert!(problem.message.contains("did you mean `color`"), "{}", problem.message);
        let problem =
            vocabulary.resolve_declarations("color: var(--color-nope)").unwrap_err().remove(0);
        assert!(problem.message.contains("not defined"), "{}", problem.message);
    }

    #[test]
    fn a_style_file_adds_tokens_utilities_and_variants() {
        let source = "@theme {\n  --color-*: initial;\n  --color-primary: #0a84ff;\n}\n\
                      @theme inline {\n  --color-ink: #111111;\n}\n\
                      @utility card {\n  @apply p-2 bg-primary;\n  border-radius: 8px;\n}\n\
                      @custom-variant touch (@media (pointer: coarse));\n";
        let vocabulary = Vocabulary::with_style_file(source).unwrap();
        let lowered: Vec<String> = vocabulary
            .resolve_classes("card touch:text-ink")
            .unwrap()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(lowered.last().map(String::as_str), Some("pointer-coarse:color: #111111"));
        assert!(lowered.contains(&"background-color: var(--color-primary)".to_owned()));
        assert!(
            vocabulary.resolve_classes("bg-blue-500").is_err(),
            "the colour namespace was cleared"
        );
        let (cleared, set) = vocabulary.project_theme();
        assert_eq!(cleared, ["color-"]);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn a_bad_style_file_reports_positions() {
        let source =
            "@theme {\n  --color-primary: nonsense;\n}\n@utility x {\n  @apply p-4 bg-nope;\n}\n";
        let errors = Vocabulary::with_style_file(source).unwrap_err();
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].line_column(source), (2, 3));
        assert_eq!(&source[errors[1].range.clone()], "bg-nope");
    }
}
