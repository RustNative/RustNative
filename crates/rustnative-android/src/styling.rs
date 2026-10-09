//! Realizing resolved styles on real widgets (`RnStyle.apply`).
//!
//! A style is applied where it *differs* from the default theme's
//! resolution for the same kind and state — the default theme stands for
//! "the host's look", so an unstyled button is the device theme's own
//! button, light or dark, Material or the manufacturer's. What a component
//! or a custom theme sets is realized exactly, per interaction state: a
//! `GradientDrawable` per state (fill, stroke, radius) in a
//! `StateListDrawable`, a `ColorStateList` for the text, the typeface and
//! size, and elevation for a shadow (approximated: the host draws its own
//! shadow; `rustnative_style::ANDROID` says so).
//!
//! # Units
//!
//! Lengths arrive in logical pixels (dp) and are sent in device pixels:
//! × the display density. Fonts are × the text scale too (the system font
//! scale, as `sp` are), rounded half away from zero once.

use std::collections::HashMap;

use rustnative_core::{Color, ControlState, NodeKind, StyleOverride, Theme, VisualStyle};

use crate::protocol::{
    ST_BACKGROUND, ST_BORDER, ST_ELEVATION, ST_FOREGROUND, ST_RADIUS, STATE_FLOATS, STATE_INTS,
    STATES,
};
use crate::units::to_px;

/// What `RnStyle.apply` takes for one node.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StyleSpec {
    /// [`STATES`] × [`STATE_INTS`]: flags, background, foreground, border
    /// (ARGB).
    pub(crate) ints: Vec<i32>,
    /// [`STATES`] × [`STATE_FLOATS`]: radius and elevation, in pixels.
    pub(crate) floats: Vec<f32>,
    /// The stroke width, in pixels.
    pub(crate) border_width: f32,
    /// The font size in pixels (0: the theme's).
    pub(crate) font_size: f32,
    /// The font weight (0: the theme's).
    pub(crate) weight: i32,
    /// The font family (`None`: the theme's).
    pub(crate) family: Option<String>,
    /// A leaf's own padding — start, top, end, bottom, in pixels — added to
    /// the widget's own (`None`: the widget's own alone).
    pub(crate) padding: Option<[i32; 4]>,
    /// A digest of everything above, so an unchanged style is not sent.
    pub(crate) digest: u64,
}

impl StyleSpec {
    /// The spec with a running animation's colours over the normal state.
    pub(crate) fn animate(&mut self, background: Option<Color>, foreground: Option<Color>) {
        if let Some(color) = background {
            self.ints[0] |= ST_BACKGROUND;
            self.ints[1] = argb(color);
        }
        if let Some(color) = foreground {
            self.ints[0] |= ST_FOREGROUND;
            self.ints[2] = argb(color);
        }
        self.digest = fnv64(format!("{:?}{:?}{}", self.ints, self.floats, self.digest).as_bytes());
    }

    /// Whether the node looks exactly as the host's own widget does.
    pub(crate) fn is_host_look(&self) -> bool {
        self.ints.chunks(STATE_INTS).all(|state| state[0] == 0)
            && self.font_size == 0.0
            && self.weight == 0
            && self.family.is_none()
            && self.padding.is_none()
    }
}

/// Computes and caches style specs for one window.
#[derive(Debug)]
pub(crate) struct StyleSheet {
    /// The theme whose resolutions are "the host's look".
    baseline: Theme,
    text_scale: f32,
    density: f32,
    cache: HashMap<String, StyleSpec>,
}

impl Default for StyleSheet {
    fn default() -> Self {
        Self { baseline: Theme::default(), text_scale: 1.0, density: 1.0, cache: HashMap::new() }
    }
}

impl StyleSheet {
    /// Sets the theme that stands for the host's look. Returns whether it
    /// changed.
    pub(crate) fn set_baseline(&mut self, baseline: Theme) -> bool {
        if self.baseline == baseline {
            return false;
        }
        self.baseline = baseline;
        self.cache.clear();
        true
    }

    /// Sets the text scale and density lengths and fonts are multiplied by.
    /// Returns whether either changed (every node must then be restyled).
    pub(crate) fn set_scales(&mut self, text_scale: f32, density: f32) -> bool {
        if (self.text_scale - text_scale).abs() < f32::EPSILON
            && (self.density - density).abs() < f32::EPSILON
        {
            return false;
        }
        self.text_scale = text_scale;
        self.density = density;
        self.cache.clear();
        true
    }

    /// The spec realizing `override_style` on a node of `kind` under
    /// `theme`.
    pub(crate) fn spec_for(
        &mut self,
        kind: NodeKind,
        override_style: &StyleOverride,
        theme: &Theme,
    ) -> StyleSpec {
        let key =
            format!("{kind:?}|{override_style:?}|{:016x}", fnv64(format!("{theme:?}").as_bytes()));
        if let Some(spec) = self.cache.get(&key) {
            return spec.clone();
        }
        let spec = self.compute(kind, override_style, theme);
        self.cache.insert(key, spec.clone());
        spec
    }

    fn compute(&self, kind: NodeKind, override_style: &StyleOverride, theme: &Theme) -> StyleSpec {
        let mut ints = vec![0; STATES * STATE_INTS];
        let mut floats = vec![0.0; STATES * STATE_FLOATS];
        let mut border_width = 0.0;
        let mut font = (0.0, 0, None);
        let mut padding = None;
        for (index, state) in [
            ControlState::Normal,
            ControlState::Hovered,
            ControlState::Focused,
            ControlState::Pressed,
            ControlState::Disabled,
        ]
        .into_iter()
        .enumerate()
        {
            let resolved = theme.resolve(kind, state, override_style);
            let host = self.baseline.resolve(kind, state, &StyleOverride::default());
            let (style, host) = (resolved.properties(), host.properties());
            let at = index * STATE_INTS;
            let mut flags = 0;
            if let Some(color) = changed(style.background_override(), host.background_override()) {
                flags |= ST_BACKGROUND;
                ints[at + 1] = argb(color);
            }
            if let Some(color) = changed(style.foreground_override(), host.foreground_override()) {
                flags |= ST_FOREGROUND;
                ints[at + 2] = argb(color);
            }
            if let Some(color) = changed(style.border_override(), host.border_override()) {
                flags |= ST_BORDER;
                ints[at + 3] = argb(color);
                border_width = self.density;
            }
            if let Some(radius) =
                changed(style.border_radius_override(), host.border_radius_override())
            {
                flags |= ST_RADIUS;
                floats[index * STATE_FLOATS] = f32::from(radius) * self.density;
            }
            if let Some(elevation) = elevation(style, host) {
                flags |= ST_ELEVATION;
                floats[index * STATE_FLOATS + 1] = elevation * self.density;
            }
            ints[at] = flags;
            if index == 0 {
                padding = style.padding_override().map(|insets| {
                    [insets.start, insets.top, insets.end, insets.bottom]
                        .map(|length| to_px(length, self.density))
                });
                if let Some(typography) =
                    changed(style.typography_override(), host.typography_override())
                {
                    font = (
                        font_pixels(typography.size, self.text_scale, self.density),
                        i32::from(typography.weight.clamp(100, 900)),
                        family(&typography.family),
                    );
                }
            }
        }
        let digest =
            fnv64(format!("{ints:?}{floats:?}{border_width}{font:?}{padding:?}").as_bytes());
        StyleSpec {
            ints,
            floats,
            border_width,
            font_size: font.0,
            weight: font.1,
            family: font.2,
            padding,
            digest,
        }
    }
}

/// The elevation (dp) a shadow is realized as: its blur, the closest
/// single number the host's own shadow takes. `Some(0)` removes a shadow
/// the host would draw.
fn elevation(style: &VisualStyle, host: &VisualStyle) -> Option<f32> {
    let layers = changed(style.shadow_override(), host.shadow_override())?;
    #[allow(clippy::cast_precision_loss, reason = "a blur radius in thousandths, small")]
    let blur = layers
        .iter()
        .filter(|layer| !layer.inset)
        .map(|layer| layer.blur.milli() as f32 / 1000.0)
        .fold(0.0_f32, f32::max);
    Some(blur)
}

fn changed<T: PartialEq>(value: Option<T>, host: Option<T>) -> Option<T> {
    match host {
        Some(host) if value.as_ref() == Some(&host) => None,
        _ => value,
    }
}

/// A colour as Android's packed ARGB `int`.
pub(crate) fn argb(color: Color) -> i32 {
    i32::from_be_bytes([color.alpha, color.red, color.green, color.blue])
}

/// A font size in device pixels: `size` logical pixels × the text scale ×
/// the density, rounded half away from zero (the unit mapping's one
/// rounding rule).
pub(crate) fn font_pixels(size: u16, text_scale: f32, density: f32) -> f32 {
    (f32::from(size) * text_scale * density).round().max(1.0)
}

/// A family list's first family as `Typeface.create` takes it; `None` for
/// the system UI font (the theme's).
pub(crate) fn family(family: &str) -> Option<String> {
    let first = family
        .split(',')
        .map(|name| name.trim().trim_matches(['"', '\'']))
        .next()
        .unwrap_or_default();
    match first {
        "" | "system-ui" | "ui-sans-serif" => None,
        "ui-serif" => Some("serif".to_owned()),
        "ui-monospace" => Some("monospace".to_owned()),
        name => Some(name.to_owned()),
    }
}

/// FNV-1a, 64 bits.
pub(crate) fn fnv64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_pack_as_argb() {
        assert_eq!(
            argb(Color::rgb(0x12, 0x34, 0x56)),
            i32::from_be_bytes([0xff, 0x12, 0x34, 0x56])
        );
        assert_eq!(argb(Color::rgba(0, 0, 0, 0)), 0);
    }

    #[test]
    fn fonts_scale_once_and_round_half_away_from_zero() {
        assert!((font_pixels(14, 1.0, 2.75) - 39.0).abs() < f32::EPSILON); // 38.5 rounds up
        assert!((font_pixels(14, 1.3, 1.0) - 18.0).abs() < f32::EPSILON); // 18.2
        assert!((font_pixels(10, 0.01, 1.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn families_resolve_to_what_typeface_create_takes() {
        assert_eq!(family("system-ui"), None);
        assert_eq!(family("'Roboto Mono', monospace").as_deref(), Some("Roboto Mono"));
        assert_eq!(family("ui-monospace").as_deref(), Some("monospace"));
        assert_eq!(family("serif").as_deref(), Some("serif"));
    }

    #[test]
    fn the_default_theme_is_the_host_look_and_an_override_is_not() {
        let mut sheet = StyleSheet::default();
        let theme = Theme::default();
        let plain = sheet.spec_for(NodeKind::Button, &StyleOverride::default(), &theme);
        assert!(plain.is_host_look());
        let red = StyleOverride::new(VisualStyle::default().background(Color::rgb(255, 0, 0)));
        let styled = sheet.spec_for(NodeKind::Button, &red, &theme);
        assert!(!styled.is_host_look());
        assert_eq!(styled.ints[0] & ST_BACKGROUND, ST_BACKGROUND);
        assert_eq!(styled.ints[1], argb(Color::rgb(255, 0, 0)));
        assert_ne!(plain.digest, styled.digest);
    }
}
