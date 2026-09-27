//! The layout and style mapping (`PLAN.md` Web milestone C): what the
//! framework computes in Rust, what it hands to the browser's CSS, and the
//! stylesheet that carries it. `docs/web/layout-mapping.md` is the prose
//! form of this module, with every approximation named.
//!
//! # One layout engine per subtree
//!
//! The browser owns layout on this backend: the core's layout engine does
//! not run, and nothing here positions a node in pixels. What the core
//! owns instead is the *meaning* of each layout property, and this module
//! states that meaning in CSS so the browser's flexbox and grid compute
//! what the core's engine would. There are never two engines deciding the
//! same subtree.
//!
//! # Classes, not attributes
//!
//! A strict content security policy refuses `style` attributes, so every
//! style a node needs is a class whose rule is in the page's stylesheet
//! (or, for a node the runtime renders in the browser, inserted through the
//! CSS object model, which the policy allows). A class is named by the hash
//! of its rule ([`crate::hash`]), so equal rules share one class and the
//! runtime names a node's class exactly as the server did.
//!
//! In cascade order — a later layer wins:
//!
//! | Layer | Class | From |
//! |---|---|---|
//! | base | `rn-*` | fixed resets and each kind's display |
//! | theme | `rn-label` … | the theme's per-kind defaults |
//! | layout | `l…` | the node's typed layout, in its parent's context |
//! | visual | `v…` | the node's typed visual override and state styles |
//! | declarations | `d…` | a `classes!`/`styles!` set, conditions as media queries and pseudo-classes |
//! | node | `n…` | a set's width, height, self-alignment, and display, which depend on the parent |

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use rustnative_core::style::decl::{ConditionalDeclaration, Keyword, StyleValue};
use rustnative_core::{
    Alignment, ColumnStyle, ComponentStyle, Cursor, DeclarationSet, EdgeInsets, GridStyle,
    LayoutStyle, Overflow, RowStyle, SizeMode, StateStyles, StyleProperty, Theme, Track,
    Typography, VisualStyle,
};

use crate::hash::class_name;
use rustnative_style::web::{SizeValue, condition_css, grouped, size_value, wrap};
pub use rustnative_style::web::{
    condition_json, declaration_rules, implies, is_contextual, set_descriptor,
};

/// How a node's parent lays it out: which axis is the main one, and the
/// alignment the parent gives children that do not choose their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// A column: height is the main axis.
    Column(Alignment),
    /// A row: width is the main axis.
    Row(Alignment),
    /// A grid: the node fills its cell.
    Grid,
    /// The page's root: it fills the viewport.
    Root,
}

/// A container's own layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container<'a> {
    /// Not a container.
    None,
    /// A column or a row.
    Linear {
        /// Padding, gap, alignment, overflow.
        padding: EdgeInsets,
        /// Gap between children.
        gap: i32,
        /// Cross-axis alignment of children.
        align_items: Alignment,
        /// Overflow.
        overflow: Overflow,
    },
    /// A grid.
    Grid(&'a GridStyle),
}

impl Container<'_> {
    /// A column's or row's style as a container.
    #[must_use]
    pub const fn column(style: ColumnStyle) -> Self {
        Self::Linear {
            padding: style.padding,
            gap: style.gap,
            align_items: style.align_items,
            overflow: style.overflow,
        }
    }

    /// A row's style as a container.
    #[must_use]
    pub const fn row(style: RowStyle) -> Self {
        Self::Linear {
            padding: style.padding,
            gap: style.gap,
            align_items: style.align_items,
            overflow: style.overflow,
        }
    }
}

const fn align_css(alignment: Alignment) -> &'static str {
    match alignment {
        Alignment::Start => "flex-start",
        Alignment::Center => "center",
        Alignment::End => "flex-end",
        Alignment::Stretch => "stretch",
    }
}

const fn overflow_css(overflow: Overflow) -> &'static str {
    match overflow {
        Overflow::Visible => "visible",
        Overflow::Clip => "hidden",
        Overflow::Scroll => "auto",
    }
}

fn insets(out: &mut String, property: &str, insets: EdgeInsets) {
    if insets != EdgeInsets::all(0) {
        let _ = write!(
            out,
            "{property}-block:{}px {}px;{property}-inline:{}px {}px;",
            insets.top, insets.bottom, insets.start, insets.end
        );
    }
}

fn main_axis(out: &mut String, property: &str, mode: SizeMode) {
    match mode {
        SizeMode::Fixed(value) => {
            let _ = write!(out, "flex:0 0 auto;{property}:{}px;", value.max(0));
        }
        SizeMode::Auto => out.push_str("flex:0 0 auto;"),
        // The engine gives a filling child its natural size plus an equal
        // share of what is left over, and never shrinks it below its
        // natural size: `flex-grow: 1` from an `auto` basis, no shrink.
        SizeMode::Fill => out.push_str("flex:1 0 auto;"),
    }
}

fn cross_axis(out: &mut String, property: &str, mode: SizeMode, alignment: Alignment) {
    match (alignment, mode) {
        (Alignment::Stretch, SizeMode::Auto) | (_, SizeMode::Fill) => {
            out.push_str("align-self:stretch;");
        }
        (alignment, SizeMode::Auto) => {
            let _ = write!(out, "align-self:{};", align_css(alignment));
        }
        (alignment, SizeMode::Fixed(value)) => {
            // A fixed cross size is capped at what the parent offers, and
            // sits at the start when the parent stretches.
            let alignment =
                if alignment == Alignment::Stretch { Alignment::Start } else { alignment };
            let _ = write!(
                out,
                "{property}:{}px;max-{property}:100%;align-self:{};",
                value.max(0),
                align_css(alignment)
            );
        }
    }
}

/// The CSS declarations that lay a node out in `flow` and, if it is one, as
/// a `container` — the text its layout class is named from. The
/// JavaScript runtime's `rn.layoutCss` produces the same text for the same
/// input (`tests/runtime.rs`).
#[must_use]
pub fn layout_css(layout: &LayoutStyle, flow: Flow, container: &Container<'_>) -> String {
    let mut out = String::new();
    match flow {
        Flow::Column(align_items) => {
            main_axis(&mut out, "height", layout.height);
            let alignment = layout.align_self.unwrap_or(align_items);
            cross_axis(&mut out, "width", layout.width, alignment);
        }
        Flow::Row(align_items) => {
            main_axis(&mut out, "width", layout.width);
            let alignment = layout.align_self.unwrap_or(align_items);
            cross_axis(&mut out, "height", layout.height, alignment);
        }
        Flow::Grid => {
            for (property, mode, self_property) in
                [("width", layout.width, "justify-self"), ("height", layout.height, "align-self")]
            {
                if let SizeMode::Fixed(value) = mode {
                    let _ = write!(
                        out,
                        "{property}:{}px;max-{property}:100%;{self_property}:start;",
                        value.max(0)
                    );
                }
            }
            if let Some(placement) = layout.grid {
                let _ = write!(
                    out,
                    "grid-area:{} / {} / span {} / span {};",
                    placement.row + 1,
                    placement.column + 1,
                    placement.row_span.max(1),
                    placement.column_span.max(1)
                );
            }
        }
        Flow::Root => {
            // The root fills the viewport (a native root fills its window)
            // and grows with its content, which the document then scrolls.
            match layout.height {
                SizeMode::Fixed(value) => {
                    let _ = write!(out, "flex:0 0 auto;height:{}px;", value.max(0));
                }
                SizeMode::Auto | SizeMode::Fill => out.push_str("flex:1 0 auto;"),
            }
            match layout.width {
                SizeMode::Fixed(value) => {
                    let _ = write!(out, "width:{}px;align-self:flex-start;", value.max(0));
                }
                SizeMode::Auto | SizeMode::Fill => out.push_str("align-self:stretch;"),
            }
        }
    }
    insets(&mut out, "margin", layout.margin);
    let constraints = layout.constraints;
    if constraints.min_width() > 0 {
        let _ = write!(out, "min-width:{}px;", constraints.min_width());
    }
    if let Some(max) = constraints.max_width() {
        let _ = write!(out, "max-width:{max}px;");
    }
    if constraints.min_height() > 0 {
        let _ = write!(out, "min-height:{}px;", constraints.min_height());
    }
    if let Some(max) = constraints.max_height() {
        let _ = write!(out, "max-height:{max}px;");
    }
    match container {
        Container::None => {}
        Container::Linear { padding, gap, align_items, overflow } => {
            insets(&mut out, "padding", *padding);
            let _ = write!(
                out,
                "gap:{}px;align-items:{};overflow:{};",
                gap.max(&0),
                align_css(*align_items),
                overflow_css(*overflow)
            );
        }
        Container::Grid(grid) => {
            let tracks = |tracks: &[Track]| -> String {
                tracks
                    .iter()
                    .map(|track| match track {
                        Track::Fixed(value) => format!("{}px", value.max(&0)),
                        Track::Auto => "auto".to_owned(),
                        Track::Fraction(share) => format!("{share}fr"),
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            if !grid.columns.is_empty() {
                let _ = write!(out, "grid-template-columns:{};", tracks(&grid.columns));
            }
            if !grid.rows.is_empty() {
                let _ = write!(out, "grid-template-rows:{};", tracks(&grid.rows));
            }
            insets(&mut out, "padding", grid.padding);
            let _ = write!(out, "gap:{}px;", grid.gap.max(0));
        }
    }
    out
}

fn color(value: rustnative_core::Color) -> String {
    rustnative_style::model::hex(value)
}

fn family(name: &str) -> String {
    let generic = matches!(
        name,
        "system-ui"
            | "serif"
            | "sans-serif"
            | "monospace"
            | "cursive"
            | "fantasy"
            | "ui-serif"
            | "ui-sans-serif"
            | "ui-monospace"
            | "ui-rounded"
    );
    if generic || name.contains(',') {
        name.to_owned()
    } else {
        format!("\"{}\"", name.replace(['"', '\\', '\n'], ""))
    }
}

fn typography_css(out: &mut String, typography: &Typography) {
    let _ = write!(
        out,
        "font-family:{};font-size:{}px;font-weight:{};",
        family(&typography.family),
        typography.size,
        typography.weight
    );
}

/// The declarations for a visual style's set properties.
fn visual_declarations(style: &VisualStyle) -> String {
    let mut out = String::new();
    if let Some(value) = style.foreground_override() {
        let _ = write!(out, "color:{};", color(value));
    }
    if let Some(value) = style.background_override() {
        let _ = write!(out, "background-color:{};", color(value));
    }
    if let Some(value) = style.border_override() {
        let _ = write!(out, "border:1px solid {};", color(value));
    }
    if let Some(value) = style.border_radius_override() {
        let _ = write!(out, "border-radius:{value}px;");
    }
    if let Some(value) = style.typography_override() {
        typography_css(&mut out, value);
    }
    if let Some(value) = style.padding_override() {
        insets(&mut out, "padding", value);
    }
    if let Some(layers) = style.shadow_override() {
        let _ = write!(out, "box-shadow:{};", StyleValue::Shadow(layers.to_vec().into()));
    }
    out
}

const fn cursor_css(cursor: Cursor) -> &'static str {
    match cursor {
        Cursor::Default => "default",
        Cursor::Pointer => "pointer",
        Cursor::Text => "text",
        Cursor::Crosshair => "crosshair",
        Cursor::Move => "move",
        Cursor::NotAllowed => "not-allowed",
        Cursor::ResizeVertical => "ns-resize",
        Cursor::ResizeHorizontal => "ew-resize",
        Cursor::Wait => "wait",
        Cursor::Progress => "progress",
        Cursor::Help => "help",
    }
}

const STATE_SELECTORS: [(rustnative_core::ControlState, &str); 4] = [
    (rustnative_core::ControlState::Hovered, ":hover"),
    (rustnative_core::ControlState::Focused, ":focus"),
    (rustnative_core::ControlState::Pressed, ":active"),
    (rustnative_core::ControlState::Disabled, ":is(:disabled,[aria-disabled=true])"),
];

/// A node's typed visual override — its style, state styles, opacity, and
/// cursor — as rule text with `&` standing for the class, or `None` when it
/// sets nothing.
#[must_use]
pub fn visual_rules(
    style: &VisualStyle,
    states: &StateStyles,
    opacity: f32,
    cursor: Option<Cursor>,
) -> Option<String> {
    let mut normal = visual_declarations(style);
    if (opacity - 1.0).abs() > f32::EPSILON {
        let _ = write!(normal, "opacity:{};", fixed3(opacity));
    }
    if let Some(cursor) = cursor {
        let _ = write!(normal, "cursor:{};", cursor_css(cursor));
    }
    let mut rules = String::new();
    if !normal.is_empty() {
        let _ = write!(rules, "&{{{normal}}}");
    }
    for (state, selector) in STATE_SELECTORS {
        if let Some(style) = states.get(state) {
            let declarations = visual_declarations(style);
            if !declarations.is_empty() {
                let _ = write!(rules, "&{selector}{{{declarations}}}");
            }
        }
    }
    (!rules.is_empty()).then_some(rules)
}

/// A number with at most three decimals and no trailing zeros — how every
/// fractional value is written, on both sides.
#[must_use]
pub fn fixed3(value: f32) -> String {
    let milli = (f64::from(value) * 1000.0).round();
    #[allow(clippy::cast_possible_truncation, reason = "an opacity or a scale, well inside i32")]
    let fixed = rustnative_style::Fixed::from_milli(milli as i32);
    fixed.to_string()
}

/// The theme's defaults for a node kind, as the rules of its kind class.
fn component_rules(selector: &str, style: &ComponentStyle) -> String {
    let mut rules = String::new();
    let normal = visual_declarations(&style.normal);
    if !normal.is_empty() {
        let _ = write!(rules, "{selector}{{{normal}}}");
    }
    for (state, pseudo) in STATE_SELECTORS {
        let state_style = match state {
            rustnative_core::ControlState::Hovered => style.hovered.as_ref(),
            rustnative_core::ControlState::Focused => style.focused.as_ref(),
            rustnative_core::ControlState::Pressed => style.pressed.as_ref(),
            _ => style.disabled.as_ref(),
        };
        if let Some(state_style) = state_style {
            let declarations = visual_declarations(state_style);
            if !declarations.is_empty() {
                let _ = write!(rules, "{selector}{pseudo}{{{declarations}}}");
            }
        }
    }
    rules
}

/// The cascade layers' order, declared first: base, theme, then the four
/// class families. A page's own unlayered CSS comes after all of them.
pub const LAYER_ORDER: &str = "@layer rn-base,rn-theme,rn-l,rn-v,rn-d,rn-n;";

/// The fixed base rules every page with framework content carries.
pub const BASE_CSS: &str = "*,*::before,*::after{box-sizing:border-box}\
html,body{margin:0}\
body{display:flex;flex-direction:column;min-height:100vh}\
[hidden]{display:none!important}\
.rn{min-width:0;min-height:0;margin:0}\
.rn-col{display:flex;flex-direction:column}\
.rn-row{display:flex;flex-direction:row}\
.rn-grid{display:grid;grid-auto-rows:auto;grid-auto-flow:row dense;justify-content:start;align-content:start}\
.rn-vlist{display:flex;overflow:auto}\
.rn-spacer{flex:0 0 auto}\
.rn-tabs{display:flex;flex-direction:row}\
.rn-check{display:inline-flex;align-items:center;gap:6px}\
.rn-canvas>svg{display:block;width:100%;height:100%}\
dialog.rn{position:static;border:0;padding:0}";

/// The theme's rules: its per-kind defaults, one class per kind.
#[must_use]
pub fn theme_css(theme: &Theme) -> String {
    let mut out = String::new();
    out.push_str(&component_rules(".rn-label", theme.label()));
    out.push_str(&component_rules(".rn-button,.rn-control,.rn-tabs button", theme.button()));
    out.push_str(&component_rules(".rn-input", theme.text_input()));
    out.push_str(&component_rules(
        ".rn-col,.rn-row,.rn-grid,.rn-canvas,.rn-surface",
        theme.container(),
    ));
    out
}

/// What a node is, for the display its `display:` declarations restore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    /// A column or a row.
    Flex,
    /// A grid.
    Grid,
    /// A check box, radio, or switch with its label.
    InlineFlex,
    /// Anything else: the element's own display.
    Element,
}

impl Display {
    const fn css(self) -> &'static str {
        match self {
            Self::Flex => "flex",
            Self::Grid => "grid",
            Self::InlineFlex => "inline-flex",
            Self::Element => "revert",
        }
    }
}

fn size_mode_value(mode: SizeMode) -> SizeValue {
    match mode {
        SizeMode::Auto => SizeValue::Auto,
        SizeMode::Fill => SizeValue::Fill,
        SizeMode::Fixed(value) => SizeValue::Length(format!("{}px", value.max(0))),
    }
}

/// One condition's resolved contextual state: the node's width, height,
/// and self-alignment after the declarations that hold under it.
fn contextual_body(
    flow: Flow,
    width: &SizeValue,
    height: &SizeValue,
    align_self: Option<Alignment>,
    display: Option<Display>,
) -> String {
    let mut out = String::new();
    let main = |out: &mut String, property: &str, value: &SizeValue| match value {
        SizeValue::Length(length) => {
            let _ = write!(out, "flex:0 0 auto;{property}:{length};");
        }
        SizeValue::Auto => {
            let _ = write!(out, "flex:0 0 auto;{property}:auto;");
        }
        SizeValue::Fill => {
            let _ = write!(out, "flex:1 0 auto;{property}:auto;");
        }
    };
    let cross = |out: &mut String, property: &str, value: &SizeValue, alignment: Alignment| match (
        alignment, value,
    ) {
        (Alignment::Stretch, SizeValue::Auto) | (_, SizeValue::Fill) => {
            let _ = write!(out, "{property}:auto;align-self:stretch;");
        }
        (alignment, SizeValue::Auto) => {
            let _ = write!(out, "{property}:auto;align-self:{};", align_css(alignment));
        }
        (alignment, SizeValue::Length(length)) => {
            let alignment =
                if alignment == Alignment::Stretch { Alignment::Start } else { alignment };
            let _ = write!(
                out,
                "{property}:{length};max-{property}:100%;align-self:{};",
                align_css(alignment)
            );
        }
    };
    match flow {
        Flow::Column(align_items) => {
            main(&mut out, "height", height);
            cross(&mut out, "width", width, align_self.unwrap_or(align_items));
        }
        Flow::Row(align_items) => {
            main(&mut out, "width", width);
            cross(&mut out, "height", height, align_self.unwrap_or(align_items));
        }
        Flow::Grid | Flow::Root => {
            for (property, value) in [("width", width), ("height", height)] {
                match value {
                    SizeValue::Length(length) => {
                        let _ = write!(out, "{property}:{length};");
                    }
                    SizeValue::Auto | SizeValue::Fill => {
                        let _ = write!(out, "{property}:auto;");
                    }
                }
            }
        }
    }
    if let Some(display) = display {
        let _ = write!(out, "display:{};", display.css());
    }
    out
}

/// A node's contextual declarations — width, height, self-alignment, and
/// display from its sets — as rule text (`&` for its class), given its
/// typed layout and its parent's flow, or `None` when its sets have none.
///
/// Each condition's rule states the node's full contextual layout under
/// that condition (with every declaration that also holds there applied in
/// order), because in flexbox a size means something different on the
/// main and the cross axis and a self-alignment changes what a cross size
/// does.
#[must_use]
pub fn node_rules(
    layout: &LayoutStyle,
    flow: Flow,
    sets: &[DeclarationSet],
    kind: Display,
) -> Option<String> {
    let contextual: Vec<&ConditionalDeclaration> = sets
        .iter()
        .flat_map(|set| set.declarations().iter())
        .filter(|declaration| is_contextual(declaration.declaration.property))
        .collect();
    if contextual.is_empty() {
        return None;
    }
    let mut rules = String::new();
    for (condition, _) in grouped(contextual.iter().copied()) {
        let mut width = size_mode_value(layout.width);
        let mut height = size_mode_value(layout.height);
        let mut align_self = layout.align_self;
        let mut display = None;
        // Every declaration that holds whenever this condition does — the
        // unconditional ones, a smaller breakpoint's, this condition's own
        // — in source order.
        for declaration in &contextual {
            if !implies(&condition, &declaration.condition) {
                continue;
            }
            let value = &declaration.declaration.value;
            match declaration.declaration.property {
                StyleProperty::Width => width = size_value(value).unwrap_or(width),
                StyleProperty::Height => height = size_value(value).unwrap_or(height),
                StyleProperty::AlignSelf => {
                    align_self = match value {
                        StyleValue::Keyword(Keyword::Start) => Some(Alignment::Start),
                        StyleValue::Keyword(Keyword::Center) => Some(Alignment::Center),
                        StyleValue::Keyword(Keyword::End) => Some(Alignment::End),
                        StyleValue::Keyword(Keyword::Stretch) => Some(Alignment::Stretch),
                        StyleValue::Keyword(Keyword::Auto) => None,
                        _ => align_self,
                    };
                }
                _ => {
                    display = match value {
                        StyleValue::Keyword(Keyword::Hidden) => Some(None),
                        StyleValue::Keyword(Keyword::Shown) => Some(Some(kind)),
                        _ => display,
                    };
                }
            }
        }
        let body = contextual_body(flow, &width, &height, align_self, None);
        let body = match display {
            Some(None) => format!("{body}display:none;"),
            Some(Some(kind)) => format!("{body}display:{};", kind.css()),
            None => body,
        };
        let (media, selector) = condition_css(&condition);
        wrap(&mut rules, &media, &selector, &body);
    }
    Some(rules)
}

/// Which layer a rule belongs to; see the module documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// `l…`.
    Layout,
    /// `v…`.
    Visual,
    /// `d…`.
    Declarations,
    /// `n…`.
    Node,
}

impl Layer {
    /// The prefix of this layer's class names.
    #[must_use]
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Layout => "l",
            Self::Visual => "v",
            Self::Declarations => "d",
            Self::Node => "n",
        }
    }
}

/// The rules one render needs, collected as it goes: each distinct rule
/// once, named by its hash, in cascade order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StyleSheet {
    rules: BTreeMap<Layer, BTreeMap<String, String>>,
    tokens: BTreeSet<String>,
}

impl StyleSheet {
    /// An empty sheet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `text` (rule text with `&` for the class) in `layer`, returning
    /// the class it is named by.
    pub fn add(&mut self, layer: Layer, text: &str) -> String {
        let class = class_name(layer.prefix(), text);
        self.rules
            .entry(layer)
            .or_default()
            .entry(class.clone())
            // Each family of classes is its own cascade layer, so a rule the
            // runtime inserts later keeps its place in the cascade.
            .or_insert_with(|| {
                format!(
                    "@layer rn-{}{{{}}}",
                    layer.prefix(),
                    text.replace('&', &format!(".{class}"))
                )
            });
        class
    }

    /// Adds the layout class for `css` (declarations with no selector).
    pub fn layout(&mut self, css: &str) -> Option<String> {
        (!css.is_empty()).then(|| self.add(Layer::Layout, &format!("&{{{css}}}")))
    }

    /// Records the tokens a declaration set refers to, so the sheet
    /// defines them.
    pub fn note_tokens(&mut self, set: DeclarationSet) {
        for declaration in set.declarations() {
            if let Some(token) = declaration.declaration.value.token() {
                self.tokens.insert(token.to_owned());
            }
        }
    }

    /// Records a token name directly (a client module's tokens).
    pub fn note_token(&mut self, name: &str) {
        self.tokens.insert(name.to_owned());
    }

    /// Takes every rule and token of `other`.
    pub fn merge(&mut self, other: Self) {
        for (layer, rules) in other.rules {
            self.rules.entry(layer).or_default().extend(rules);
        }
        self.tokens.extend(other.tokens);
    }

    /// Every class this sheet has a rule for.
    pub fn classes(&self) -> impl Iterator<Item = &str> {
        self.rules.values().flat_map(|rules| rules.keys().map(String::as_str))
    }

    /// Whether the sheet has no rules.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.values().all(BTreeMap::is_empty)
    }

    /// The tokens the rules refer to, with the tokens those tokens refer
    /// to, and their values in `theme`.
    #[must_use]
    pub fn token_css(&self, theme: &Theme) -> String {
        let mut pending: Vec<String> = self.tokens.iter().cloned().collect();
        let mut seen = BTreeSet::new();
        let mut values = BTreeMap::new();
        while let Some(name) = pending.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            if let Some(value) = theme.tokens().get(&name) {
                if let Some(inner) = value.token() {
                    pending.push(inner.to_owned());
                }
                values.insert(name, value.to_string());
            }
        }
        if values.is_empty() {
            return String::new();
        }
        let mut out = String::from(":root{");
        for (name, value) in values {
            let _ = write!(out, "--{name}:{value};");
        }
        out.push('}');
        out
    }

    /// The whole stylesheet: the base rules, the theme's, its tokens, then
    /// every collected rule in cascade order.
    #[must_use]
    pub fn css(&self, theme: &Theme) -> String {
        let mut out = String::from(LAYER_ORDER);
        let _ = write!(
            out,
            "@layer rn-base{{{BASE_CSS}}}@layer rn-theme{{{}{}}}",
            theme_css(theme),
            self.token_css(theme)
        );
        out.push_str(&self.rules_css());
        out
    }

    /// Only the collected rules, in cascade order (what a client module or
    /// a streamed fragment carries beside the page's own sheet).
    #[must_use]
    pub fn rules_css(&self) -> String {
        let mut out = String::new();
        for rules in self.rules.values() {
            for rule in rules.values() {
                out.push_str(rule);
            }
        }
        out
    }

    /// The rules as `(class, rule text)` pairs, in cascade order, for a
    /// client module that inserts them one by one.
    #[must_use]
    pub fn entries(&self) -> Vec<(String, String)> {
        self.rules
            .values()
            .flat_map(|rules| rules.iter().map(|(class, rule)| (class.clone(), rule.clone())))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustnative_core::{Constraints, GridPlacement, classes};

    #[test]
    fn a_column_child_fills_by_growing_from_its_natural_size() {
        let css = layout_css(
            &LayoutStyle::new().height(SizeMode::Fill),
            Flow::Column(Alignment::Stretch),
            &Container::None,
        );
        assert_eq!(css, "flex:1 0 auto;align-self:stretch;");
    }

    #[test]
    fn a_fixed_cross_size_is_capped_and_sits_at_the_start_under_stretch() {
        let css = layout_css(
            &LayoutStyle::new().width(SizeMode::Fixed(120)),
            Flow::Column(Alignment::Stretch),
            &Container::None,
        );
        assert_eq!(css, "flex:0 0 auto;width:120px;max-width:100%;align-self:flex-start;");
    }

    #[test]
    fn a_row_child_uses_width_as_its_main_axis() {
        let css = layout_css(
            &LayoutStyle::new().width(SizeMode::Fixed(40)).height(SizeMode::Auto),
            Flow::Row(Alignment::Center),
            &Container::None,
        );
        assert_eq!(css, "flex:0 0 auto;width:40px;align-self:center;");
    }

    #[test]
    fn margins_padding_and_constraints_are_logical() {
        let css = layout_css(
            &LayoutStyle::new()
                .margin(EdgeInsets::logical(1, 2, 3, 4))
                .constraints(Constraints::new().with_min_width(10).with_max_height(90)),
            Flow::Column(Alignment::Stretch),
            &Container::column(ColumnStyle::new().padding(EdgeInsets::all(8)).gap(4)),
        );
        assert_eq!(
            css,
            "flex:0 0 auto;align-self:stretch;margin-block:1px 3px;margin-inline:4px 2px;\
             min-width:10px;max-height:90px;padding-block:8px 8px;padding-inline:8px 8px;gap:4px;\
             align-items:stretch;overflow:hidden;"
        );
    }

    #[test]
    fn grid_tracks_and_placements() {
        let grid = GridStyle::new([Track::Fixed(80), Track::Fraction(2), Track::Auto]).gap(6);
        let css = layout_css(
            &LayoutStyle::new().grid(GridPlacement::at(1, 2)),
            Flow::Grid,
            &Container::Grid(&grid),
        );
        assert_eq!(
            css,
            "grid-area:2 / 3 / span 1 / span 1;grid-template-columns:80px 2fr auto;gap:6px;"
        );
    }

    #[test]
    fn declarations_become_rules_with_media_queries_and_pseudo_classes() {
        let rules = declaration_rules(classes!("p-4 md:p-8 hover:bg-[#ff0000] dark:text-white"))
            .expect("rules");
        assert!(rules.contains("&{padding-top:calc(var(--spacing) * 4);"), "{rules}");
        assert!(
            rules.contains("@media (min-width: 768px){&{padding-top:calc(var(--spacing) * 8)"),
            "{rules}"
        );
        assert!(rules.contains("&:hover{background-color:#ff0000;}"), "{rules}");
        assert!(rules.contains("@media (prefers-color-scheme: dark){&{color:"), "{rules}");
    }

    #[test]
    fn contextual_declarations_depend_on_the_parent() {
        let set = classes!("w-32 md:w-full hidden md:flex");
        let rules =
            node_rules(&LayoutStyle::new(), Flow::Row(Alignment::Stretch), &[set], Display::Flex)
                .expect("rules");
        // In a row, width is the main axis: a fixed width stops growing.
        assert!(rules.starts_with("&{flex:0 0 auto;width:calc(var(--spacing) * 32);"), "{rules}");
        assert!(rules.contains("display:none;"), "{rules}");
        assert!(rules.contains("@media (min-width: 768px){&{flex:1 0 auto;width:auto;"), "{rules}");
        assert!(rules.contains("display:flex;"), "{rules}");
    }

    #[test]
    fn a_sheet_names_equal_rules_once_and_defines_their_tokens() {
        let mut sheet = StyleSheet::new();
        let first = sheet.layout("gap:4px;").expect("class");
        let second = sheet.layout("gap:4px;").expect("class");
        assert_eq!(first, second);
        sheet.note_tokens(classes!("bg-blue-500"));
        let css = sheet.css(&Theme::default());
        assert!(css.contains(&format!(".{first}{{gap:4px;}}")));
        assert!(css.contains(":root{--color-blue-500:"), "{css}");
    }
}
