//! Realizing resolved styles through GTK's own CSS engine.
//!
//! Every widget GTK draws is styled by CSS, so this backend realizes a
//! node's style the way a GTK application does: one display-wide
//! `GtkCssProvider` holds a rule per distinct style, named by a hash of
//! its content (`rn-s<hash>`), and a node's widget carries that class.
//! Colours, borders, radii, shadows, fonts, and the interaction states
//! (`:hover`, `:active`, `:focus-within`, `:disabled`) are CSS; GTK paints
//! them on the real widget. Nothing is owner-drawn (2.14's fourth rule),
//! and the capability table (`rustnative_style::LINUX`) answers every
//! property from this module.
//!
//! # What is applied
//!
//! A style is applied where it *differs* from the default theme's
//! resolution for the same kind and state. The default theme stands for
//! "the host's look", so an unstyled button is GTK's own button in the
//! desktop's own theme — light, dark, or high contrast — which is the
//! fidelity Milestone 41 measures against the host's first-party
//! applications. A property a component or a custom theme sets is realized
//! exactly.
//!
//! # Text scale
//!
//! GTK already scales its own fonts by the desktop's text-scaling factor
//! (through `gtk-xft-dpi`). A font this module sets is given in `px`, which
//! GTK does not scale, multiplied by the environment's text scale — so an
//! explicit font follows the person's setting exactly once.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use gtk::prelude::*;
use rustnative_core::{
    Color, ControlState, NodeKind, ShadowLayer, StyleOverride, Theme, Typography, VisualStyle,
};

/// The display-wide style sheet and the classes already written into it.
pub(crate) struct StyleSheet {
    provider: gtk::CssProvider,
    /// Every rule, by class name, in the order written.
    rules: Vec<(String, String)>,
    written: HashSet<String>,
    /// The theme whose resolutions are "the host's look": the default
    /// theme, with the host's palette.
    baseline: Theme,
    text_scale: f32,
    /// The desktop's UI font family, which `system-ui` means here.
    system_family: String,
    /// Classes computed for a node's style, so a re-render of an unchanged
    /// node does not hash its style again.
    cache: HashMap<StyleKey, Option<String>>,
    dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct StyleKey {
    kind: NodeKind,
    override_style: StyleOverrideKey,
    theme: u64,
}

/// `StyleOverride` has no `Hash`; its debug text is a faithful key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct StyleOverrideKey(String);

impl std::fmt::Debug for StyleSheet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StyleSheet").field("rules", &self.rules.len()).finish_non_exhaustive()
    }
}

impl StyleSheet {
    /// A style sheet installed on the default display at application
    /// priority, so its rules win over the desktop's theme.
    pub(crate) fn install() -> Self {
        let provider = gtk::CssProvider::new();
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        Self {
            provider,
            rules: Vec::new(),
            written: HashSet::new(),
            baseline: Theme::default(),
            text_scale: 1.0,
            system_family: system_font_family(),
            cache: HashMap::new(),
            dirty: false,
        }
    }

    /// Removes this style sheet from the display.
    pub(crate) fn uninstall(&self) {
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_remove_provider_for_display(&display, &self.provider);
        }
    }

    /// Sets the theme that stands for the host's look.
    pub(crate) fn set_baseline(&mut self, baseline: Theme) {
        if self.baseline != baseline {
            self.baseline = baseline;
            self.cache.clear();
        }
    }

    /// Sets the text scale explicit fonts are multiplied by. Returns
    /// whether it changed (every node must then be restyled).
    pub(crate) fn set_text_scale(&mut self, scale: f32) -> bool {
        if (self.text_scale - scale).abs() < f32::EPSILON {
            return false;
        }
        self.text_scale = scale;
        self.cache.clear();
        true
    }

    /// The class realizing `override_style` on a node of `kind` under
    /// `theme`, writing its rule if it is new — or `None` when the node
    /// looks exactly as the host's own widget does.
    pub(crate) fn class_for(
        &mut self,
        kind: NodeKind,
        override_style: &StyleOverride,
        theme: &Theme,
    ) -> Option<String> {
        let key = StyleKey {
            kind,
            override_style: StyleOverrideKey(format!("{override_style:?}")),
            theme: theme_key(theme),
        };
        if let Some(class) = self.cache.get(&key) {
            return class.clone();
        }
        let class = self.compute(kind, override_style, theme);
        self.cache.insert(key, class.clone());
        class
    }

    fn compute(
        &mut self,
        kind: NodeKind,
        override_style: &StyleOverride,
        theme: &Theme,
    ) -> Option<String> {
        let mut body = String::new();
        for (state, selector) in [
            (ControlState::Normal, ""),
            (ControlState::Hovered, ":hover"),
            (ControlState::Focused, ":focus-within"),
            (ControlState::Pressed, ":active"),
            (ControlState::Disabled, ":disabled"),
        ] {
            let resolved = theme.resolve(kind, state, override_style);
            let host = self.baseline.resolve(kind, state, &StyleOverride::default());
            let declarations = self.declarations(resolved.properties(), host.properties());
            if !declarations.is_empty() {
                let _ = writeln!(body, "{{SELF}}{selector} {{ {declarations} }}");
            }
        }
        if body.is_empty() {
            return None;
        }
        let class = format!("rn-s{:016x}", fnv64(body.as_bytes()));
        if self.written.insert(class.clone()) {
            let rule = body.replace("{SELF}", &format!(".{class}"));
            self.rules.push((class.clone(), rule));
            self.dirty = true;
        }
        Some(class)
    }

    /// The CSS declarations for the properties of `style` that differ from
    /// `host`.
    fn declarations(&self, style: &VisualStyle, host: &VisualStyle) -> String {
        let mut out = String::new();
        if let Some(color) = changed(style.foreground_override(), host.foreground_override()) {
            let _ = write!(out, "color: {}; ", css_color(color));
        }
        if let Some(color) = changed(style.background_override(), host.background_override()) {
            // Themes paint buttons with gradients; a background colour
            // replaces the whole background.
            let _ = write!(out, "background-color: {}; background-image: none; ", css_color(color));
        }
        if let Some(color) = changed(style.border_override(), host.border_override()) {
            let _ = write!(out, "border: 1px solid {}; ", css_color(color));
        }
        if let Some(radius) = changed(style.border_radius_override(), host.border_radius_override())
        {
            let _ = write!(out, "border-radius: {radius}px; ");
        }
        if let Some(layers) = changed(style.shadow_override(), host.shadow_override()) {
            let _ = write!(out, "box-shadow: {}; ", css_shadow(layers));
        }
        if let Some(typography) = changed(style.typography_override(), host.typography_override()) {
            let _ = write!(out, "{}", self.css_font(typography));
        }
        out
    }

    /// A typography as CSS font declarations, in `px` × the text scale.
    pub(crate) fn css_font(&self, typography: &Typography) -> String {
        format!(
            "font-family: {}; font-size: {}px; font-weight: {}; ",
            css_family(&typography.family, &self.system_family),
            scaled_font_size(typography.size, self.text_scale),
            typography.weight.clamp(100, 900),
        )
    }

    /// The class realizing `typography` alone (for measuring text in the
    /// font a node will be drawn in), writing its rule if it is new — or
    /// `None` when it is the host's own font, which the widget keeps.
    pub(crate) fn font_class(&mut self, typography: &Typography) -> Option<String> {
        if typography == self.baseline.typography() {
            return None;
        }
        Some(self.write_font_class(typography))
    }

    fn write_font_class(&mut self, typography: &Typography) -> String {
        let body = self.css_font(typography);
        let class = format!("rn-f{:016x}", fnv64(body.as_bytes()));
        if self.written.insert(class.clone()) {
            self.rules.push((class.clone(), format!(".{class} {{ {body} }}")));
            self.dirty = true;
        }
        class
    }

    /// Writes new rules into the provider. One `load` per render, not one
    /// per rule: loading re-parses the whole sheet.
    pub(crate) fn flush(&mut self) {
        if !std::mem::take(&mut self.dirty) {
            return;
        }
        let css: String = self.rules.iter().map(|(_, rule)| rule.as_str()).collect();
        load(&self.provider, &css);
    }
}

#[allow(deprecated, reason = "`load_from_data` is the loader GTK 4.14 has in every minor version")]
pub(crate) fn load(provider: &gtk::CssProvider, css: &str) {
    provider.load_from_data(css);
}

/// Applies `class` (replacing any previous framework style class) to
/// `widget`.
pub(crate) fn apply_class(widget: &gtk::Widget, class: Option<&str>) {
    for existing in widget.css_classes() {
        let existing = existing.as_str();
        if (existing.starts_with("rn-s") || existing.starts_with("rn-f")) && Some(existing) != class
        {
            widget.remove_css_class(existing);
        }
    }
    if let Some(class) = class {
        if !widget.has_css_class(class) {
            widget.add_css_class(class);
        }
    }
}

fn changed<T: PartialEq>(value: Option<T>, host: Option<T>) -> Option<T> {
    match host {
        Some(host) if value.as_ref() == Some(&host) => None,
        _ => value,
    }
}

/// A font size in pixels at `scale`, rounded half away from zero (the unit
/// mapping's one rounding rule).
pub(crate) fn scaled_font_size(size: u16, scale: f32) -> u32 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a font size times a text scale: small, positive, and rounded first"
    )]
    let pixels = (f32::from(size) * scale).round().max(1.0) as u32;
    pixels
}

/// A CSS colour, with alpha.
pub(crate) fn css_color(color: Color) -> String {
    if color.alpha == 255 {
        format!("rgb({},{},{})", color.red, color.green, color.blue)
    } else {
        format!(
            "rgba({},{},{},{:.3})",
            color.red,
            color.green,
            color.blue,
            f32::from(color.alpha) / 255.0
        )
    }
}

fn css_shadow(layers: &[ShadowLayer]) -> String {
    if layers.is_empty() {
        return "none".to_owned();
    }
    layers
        .iter()
        .map(|layer| {
            format!(
                "{}{}px {}px {}px {}px {}",
                if layer.inset { "inset " } else { "" },
                layer.x,
                layer.y,
                layer.blur,
                layer.spread,
                css_color(layer.color),
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A family list's first family, with the generic families resolved the
/// way this desktop resolves them.
fn css_family(family: &str, system: &str) -> String {
    let first = family.split(',').map(|name| name.trim().trim_matches(['"', '\''])).next();
    match first.unwrap_or_default() {
        "" | "system-ui" | "ui-sans-serif" => quoted(system),
        generic @ ("sans-serif" | "serif" | "monospace" | "cursive" | "fantasy") => {
            generic.to_owned()
        }
        "ui-serif" => "serif".to_owned(),
        "ui-monospace" => "monospace".to_owned(),
        name => quoted(name),
    }
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace(['"', '\\'], ""))
}

/// The family of the desktop's UI font (`gtk-font-name`, "Cantarell 11" →
/// "Cantarell"), or `sans-serif` when GTK has none.
fn system_font_family() -> String {
    gtk::Settings::default()
        .and_then(|settings| settings.gtk_font_name())
        .map(|name| gtk::pango::FontDescription::from_string(&name))
        .and_then(|description| description.family().map(|family| family.to_string()))
        .filter(|family| !family.is_empty())
        .unwrap_or_else(|| "sans-serif".to_owned())
}

fn theme_key(theme: &Theme) -> u64 {
    fnv64(format!("{theme:?}").as_bytes())
}

/// FNV-1a, 64 bits: a stable name for a rule's content.
pub(crate) fn fnv64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_resolve_through_the_desktop() {
        assert_eq!(css_family("system-ui", "Cantarell"), "\"Cantarell\"");
        assert_eq!(css_family("'Fira Sans', sans-serif", "X"), "\"Fira Sans\"");
        assert_eq!(css_family("monospace", "X"), "monospace");
        assert_eq!(css_family("ui-monospace", "X"), "monospace");
    }

    #[test]
    fn fonts_scale_once_and_round_half_away_from_zero() {
        assert_eq!(scaled_font_size(14, 1.0), 14);
        assert_eq!(scaled_font_size(14, 1.25), 18); // 17.5 rounds up
        assert_eq!(scaled_font_size(10, 0.05), 1);
    }

    #[test]
    fn colours_keep_their_alpha() {
        assert_eq!(css_color(Color::rgb(1, 2, 3)), "rgb(1,2,3)");
        assert_eq!(css_color(Color::rgba(1, 2, 3, 0)), "rgba(1,2,3,0.000)");
    }
}
