//! Folds a node's declarations into its typed properties (`PLAN.md` 2.14):
//! tokens through the theme's table, conditions against the environment,
//! lengths to logical pixels by the one rounding rule.
//!
//! Resolution runs on every render's output and again whenever the theme
//! or the environment changes — always from the component's unresolved
//! output, so it is a pure function of (declarations, theme, environment)
//! and a condition that stops holding simply stops contributing. Its
//! output is ordinary typed properties, diffed like any other, which is
//! why a scheme or token switch reaches the native objects that already
//! exist rather than new ones.

use rustnative_style::{ConditionEnv, Keyword, State, StyleProperty, StyleValue};

use crate::identity::NodeId;
use crate::input::Scalar;
use crate::layout::{Alignment, Constraints, EdgeInsets, Overflow, SizeMode};
use crate::node::Node;
use crate::style::{ControlState, Theme, Typography, VisualStyle};

/// What a node's declarations are resolved against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ResolveEnv {
    /// What the conditions read.
    pub(crate) condition: ConditionEnv,
    /// One `rem` in logical pixels: 16 × the text scale.
    pub(crate) rem_px: f64,
}

/// Resolves the declarations of `node` and its descendants in place.
/// Returns whether any declaration's condition reads the environment —
/// whether an environment change can change the result.
pub(crate) fn resolve_tree(
    node: &mut Node,
    theme: &Theme,
    env_for: &mut dyn FnMut(NodeId) -> ResolveEnv,
) -> bool {
    let mut reads = false;
    if !node.declarations().is_empty() {
        let env = env_for(node.id());
        reads |= apply(node, theme, &env);
    }
    for child in node.child_nodes_mut() {
        reads |= resolve_tree(child, theme, env_for);
    }
    reads
}

const fn control_state(state: State) -> ControlState {
    match state {
        State::Hover => ControlState::Hovered,
        // Keyboard focus is focus on hosts that do not tell the two apart.
        State::Focus | State::FocusVisible => ControlState::Focused,
        State::Active => ControlState::Pressed,
        State::Disabled => ControlState::Disabled,
    }
}

fn apply(node: &mut Node, theme: &Theme, env: &ResolveEnv) -> bool {
    let sets = node.declarations().to_vec();
    let mut reads = false;
    for set in sets {
        for declaration in set.declarations() {
            reads |= declaration.condition.reads_environment();
            if !declaration.condition.holds_in(&env.condition) {
                continue;
            }
            // A token the current theme does not define contributes
            // nothing: the node keeps what it would have without the
            // declaration (the build already checked the name against the
            // theme it was compiled with).
            let Some(value) = theme.tokens().resolve(&declaration.declaration.value) else {
                continue;
            };
            let property = declaration.declaration.property;
            match declaration.condition.state.map(control_state) {
                Some(state) => {
                    let em = em_px(node.visual_style(), theme);
                    if let Some(style) = node.state_styles_mut().get_or_insert(state) {
                        apply_visual(style, property, &value, theme, env.rem_px, em);
                    }
                }
                None => apply_normal(node, property, &value, theme, env.rem_px),
            }
        }
    }
    reads
}

fn em_px(style: &VisualStyle, theme: &Theme) -> f64 {
    f64::from(
        style.typography_override().map_or(theme.typography().size, |typography| typography.size),
    )
}

fn px(value: &StyleValue, rem_px: f64, em_px: f64) -> Option<i32> {
    match value {
        StyleValue::Length(length) => Some(length.to_px(rem_px, em_px)),
        StyleValue::Number(number) if number.milli() == 0 => Some(0),
        _ => None,
    }
}

fn clamp_u16(value: i32) -> u16 {
    u16::try_from(value.clamp(0, i32::from(u16::MAX))).unwrap_or(u16::MAX)
}

/// The first family of a CSS family list, with the generic families named
/// the way the backends look them up.
fn first_family(list: &str) -> String {
    let first =
        list.split(',').next().unwrap_or(list).trim().trim_matches(|c| c == '"' || c == '\'');
    match first.to_ascii_lowercase().as_str() {
        "ui-sans-serif" | "system-ui" | "sans-serif" | "-apple-system" => "system-ui".to_owned(),
        "ui-serif" | "serif" => "serif".to_owned(),
        "ui-monospace" | "monospace" => "monospace".to_owned(),
        _ => first.to_owned(),
    }
}

fn apply_visual(
    style: &mut VisualStyle,
    property: StyleProperty,
    value: &StyleValue,
    theme: &Theme,
    rem_px: f64,
    em_px: f64,
) {
    let current = std::mem::take(style);
    let typography =
        || current.typography_override().cloned().unwrap_or_else(|| theme.typography().clone());
    *style = match (property, value) {
        (StyleProperty::Foreground, StyleValue::Color(color)) => current.foreground(*color),
        (StyleProperty::Background, StyleValue::Color(color)) => current.background(*color),
        (StyleProperty::BorderColor, StyleValue::Color(color)) => current.border(*color),
        (StyleProperty::BorderRadius, _) => match px(value, rem_px, em_px) {
            Some(radius) => current.border_radius(clamp_u16(radius)),
            None => current,
        },
        // A font size is specified at the default text size: the backend
        // scales every font — declared or themed — by the person's text
        // scale, so scaling it here too would scale it twice.
        (StyleProperty::FontSize, _) => match px(value, 16.0, em_px) {
            Some(size) => {
                let typography = Typography { size: clamp_u16(size), ..typography() };
                current.typography(typography)
            }
            None => current,
        },
        (StyleProperty::FontWeight, StyleValue::Number(weight)) => {
            let weight = clamp_u16(weight.milli() / 1_000);
            let typography = Typography { weight, ..typography() };
            current.typography(typography)
        }
        (StyleProperty::FontFamily, StyleValue::Family(family)) => {
            let typography = Typography { family: first_family(family), ..typography() };
            current.typography(typography)
        }
        (StyleProperty::Shadow, StyleValue::Shadow(layers)) => current.shadow(layers.to_vec()),
        _ => current,
    };
}

fn edge(insets: &mut EdgeInsets, property: StyleProperty, value: i32) {
    match property {
        StyleProperty::PaddingTop | StyleProperty::MarginTop => insets.top = value,
        StyleProperty::PaddingEnd | StyleProperty::MarginEnd => insets.end = value,
        StyleProperty::PaddingBottom | StyleProperty::MarginBottom => insets.bottom = value,
        StyleProperty::PaddingStart | StyleProperty::MarginStart => insets.start = value,
        _ => {}
    }
}

const fn alignment(keyword: Keyword) -> Option<Alignment> {
    match keyword {
        Keyword::Start => Some(Alignment::Start),
        Keyword::Center => Some(Alignment::Center),
        Keyword::End => Some(Alignment::End),
        Keyword::Stretch => Some(Alignment::Stretch),
        _ => None,
    }
}

fn size(value: &StyleValue, rem_px: f64, em_px: f64) -> Option<SizeMode> {
    match value {
        StyleValue::Keyword(Keyword::Auto) => Some(SizeMode::Auto),
        StyleValue::Keyword(Keyword::Fill) => Some(SizeMode::Fill),
        _ => px(value, rem_px, em_px).map(SizeMode::Fixed),
    }
}

fn apply_normal(
    node: &mut Node,
    property: StyleProperty,
    value: &StyleValue,
    theme: &Theme,
    rem_px: f64,
) {
    let em = em_px(node.visual_style(), theme);
    if property.is_visual() {
        apply_visual(node.visual_style_mut(), property, value, theme, rem_px, em);
        return;
    }
    let length = px(value, rem_px, em);
    match property {
        StyleProperty::PaddingTop
        | StyleProperty::PaddingEnd
        | StyleProperty::PaddingBottom
        | StyleProperty::PaddingStart => {
            let Some(length) = length else { return };
            if matches!(node, Node::Column(_) | Node::Row(_)) {
                node.with_container_fields(|padding, _, _, _| edge(padding, property, length));
            } else {
                // A leaf's padding is its visual style's (the host's own
                // content inset, when it has one).
                let style = node.visual_style_mut();
                let mut padding = style.padding_override().unwrap_or_default();
                edge(&mut padding, property, length);
                *style = std::mem::take(style).padding(padding);
            }
        }
        StyleProperty::MarginTop
        | StyleProperty::MarginEnd
        | StyleProperty::MarginBottom
        | StyleProperty::MarginStart => {
            if let Some(length) = length {
                edge(&mut node.layout_mut().margin, property, length);
            }
        }
        StyleProperty::Width | StyleProperty::Height => {
            if let Some(mode) = size(value, rem_px, em) {
                let layout = node.layout_mut();
                if property == StyleProperty::Width {
                    layout.width = mode;
                } else {
                    layout.height = mode;
                }
            }
        }
        StyleProperty::MinWidth
        | StyleProperty::MinHeight
        | StyleProperty::MaxWidth
        | StyleProperty::MaxHeight => {
            if let Some(length) = length {
                let layout = node.layout_mut();
                let constraints: Constraints = layout.constraints;
                layout.constraints = match property {
                    StyleProperty::MinWidth => constraints.with_min_width(length),
                    StyleProperty::MinHeight => constraints.with_min_height(length),
                    StyleProperty::MaxWidth => constraints.with_max_width(length),
                    _ => constraints.with_max_height(length),
                };
            }
        }
        StyleProperty::Gap => {
            if let Some(length) = length {
                node.with_container_fields(|_, gap, _, _| *gap = length);
            }
        }
        StyleProperty::AlignItems => {
            if let StyleValue::Keyword(keyword) = value {
                if let Some(alignment) = alignment(*keyword) {
                    node.with_container_fields(|_, _, align_items, _| *align_items = alignment);
                }
            }
        }
        StyleProperty::AlignSelf => {
            if let StyleValue::Keyword(keyword) = value {
                node.layout_mut().align_self = alignment(*keyword);
            }
        }
        StyleProperty::Overflow => {
            let overflow = match value {
                StyleValue::Keyword(Keyword::Visible) => Overflow::Visible,
                StyleValue::Keyword(Keyword::Clip) => Overflow::Clip,
                StyleValue::Keyword(Keyword::Scroll) => Overflow::Scroll,
                _ => return,
            };
            node.with_container_fields(|_, _, _, target| *target = overflow);
        }
        StyleProperty::Opacity => {
            if let StyleValue::Number(ratio) = value {
                let milli = u16::try_from(ratio.milli().clamp(0, 1_000)).unwrap_or(1_000);
                *node.opacity_mut() = Scalar::new(f32::from(milli) / 1_000.0);
            }
        }
        StyleProperty::Display => match value {
            StyleValue::Keyword(Keyword::Hidden) => *node.hidden_mut() = true,
            StyleValue::Keyword(Keyword::Shown) => *node.hidden_mut() = false,
            _ => {}
        },
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::decl::{Fixed, Length};

    fn default_px(length: Length) -> i32 {
        length.to_px(16.0, 16.0)
    }

    #[test]
    fn families_pick_the_first_and_name_the_generics() {
        assert_eq!(first_family("ui-sans-serif, system-ui, sans-serif"), "system-ui");
        assert_eq!(first_family("'Inter', sans-serif"), "Inter");
        assert_eq!(first_family("ui-monospace, Consolas"), "monospace");
    }

    #[test]
    fn a_rem_follows_the_text_scale_and_rounds_half_away_from_zero() {
        assert_eq!(default_px(Length::Rem(Fixed::from_milli(1_500))), 24);
        assert_eq!(Length::Rem(Fixed::ONE).to_px(16.0 * 1.5, 16.0), 24);
        assert_eq!(Length::Px(Fixed::from_milli(2_500)).to_px(16.0, 16.0), 3);
    }
}
