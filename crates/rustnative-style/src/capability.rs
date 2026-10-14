//! What each backend owes the style model (`PLAN.md` 2.14 and Milestone
//! 58, "What each backend owes"): a capability answer for every property,
//! and a unit mapping.
//!
//! The tables live here, beside the vocabulary, so that the one answer is
//! read in three places: the backend's `Platform::style_capabilities`, the
//! `classes!`/`styles!` macros (an unavailable property fails the build for
//! that target, at the class that set it), and the conformance test that
//! holds the Windows table against what the backend actually applies.

use crate::model::StyleProperty;

/// How a backend answers one property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StyleSupport {
    /// Realized as specified.
    Realized,
    /// Realized approximately; the text says how.
    Approximated(&'static str),
    /// Not realized; the text says why. Setting it for this target is a
    /// build error.
    Unavailable(&'static str),
}

/// A backend's answer for every property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyleCapabilities {
    /// The backend's name, for diagnostics.
    pub backend: &'static str,
    entries: &'static [(StyleProperty, StyleSupport)],
}

impl StyleCapabilities {
    /// A table: every property absent from `entries` is unavailable.
    #[must_use]
    pub const fn new(
        backend: &'static str,
        entries: &'static [(StyleProperty, StyleSupport)],
    ) -> Self {
        Self { backend, entries }
    }

    /// A backend that has not answered: everything unavailable.
    pub const NONE: Self = Self::new("unanswered", &[]);

    /// The answer for `property`.
    #[must_use]
    pub fn support(&self, property: StyleProperty) -> StyleSupport {
        self.entries.iter().find(|(entry, _)| *entry == property).map_or(
            StyleSupport::Unavailable("this backend's table does not answer it"),
            |(_, support)| *support,
        )
    }

    /// Every property with its answer, in [`StyleProperty::ALL`] order.
    pub fn rows(&self) -> impl Iterator<Item = (StyleProperty, StyleSupport)> + '_ {
        StyleProperty::ALL.iter().map(|property| (*property, self.support(*property)))
    }
}

/// How a backend maps the vocabulary's units onto its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitMapping {
    /// The backend's name.
    pub backend: &'static str,
    /// The host's own unit.
    pub host_unit: &'static str,
    /// How a logical pixel becomes a host unit.
    pub pixel: &'static str,
    /// How `rem` follows the host's text-size setting.
    pub rem: &'static str,
    /// The rounding rule.
    pub rounding: &'static str,
}

/// A table: the layout properties, which every backend realizes through
/// the shared layout engine, then the rest.
macro_rules! table {
    ($name:ident, $backend:literal, [$($property:ident => $support:expr,)*]) => {
        #[doc = concat!("The ", $backend, " backend's capability table.")]
        pub const $name: StyleCapabilities = StyleCapabilities::new($backend, &[
            (StyleProperty::PaddingTop, StyleSupport::Realized),
            (StyleProperty::PaddingEnd, StyleSupport::Realized),
            (StyleProperty::PaddingBottom, StyleSupport::Realized),
            (StyleProperty::PaddingStart, StyleSupport::Realized),
            (StyleProperty::MarginTop, StyleSupport::Realized),
            (StyleProperty::MarginEnd, StyleSupport::Realized),
            (StyleProperty::MarginBottom, StyleSupport::Realized),
            (StyleProperty::MarginStart, StyleSupport::Realized),
            (StyleProperty::Width, StyleSupport::Realized),
            (StyleProperty::Height, StyleSupport::Realized),
            (StyleProperty::MinWidth, StyleSupport::Realized),
            (StyleProperty::MinHeight, StyleSupport::Realized),
            (StyleProperty::MaxWidth, StyleSupport::Realized),
            (StyleProperty::MaxHeight, StyleSupport::Realized),
            (StyleProperty::Gap, StyleSupport::Realized),
            (StyleProperty::AlignItems, StyleSupport::Realized),
            (StyleProperty::AlignSelf, StyleSupport::Realized),
            $((StyleProperty::$property, $support),)*
        ]);
    };
}

table!(WINDOWS, "Windows", [
    Foreground => StyleSupport::Realized,
    Background => StyleSupport::Realized,
    BorderColor => StyleSupport::Approximated(
        "a container draws a one-pixel border in the colour; a native control keeps its system border, because the \
         framework never owner-draws a native control (2.14's fourth rule)"
    ),
    BorderRadius => StyleSupport::Approximated(
        "a container is clipped to a rounded window region (`SetWindowRgn`), without anti-aliasing; a native control \
         keeps its system shape"
    ),
    FontSize => StyleSupport::Realized,
    FontWeight => StyleSupport::Realized,
    FontFamily => StyleSupport::Approximated(
        "the first family in the list is used; the generic families map to Segoe UI (`system-ui`, `sans-serif`), \
         Cambria (`serif`), and Consolas (`monospace`)"
    ),
    Shadow => StyleSupport::Unavailable(
        "GDI child windows cannot cast shadows, and drawing one would mean owner-drawing the control"
    ),
    Overflow => StyleSupport::Realized,
    Opacity => StyleSupport::Realized,
    Display => StyleSupport::Realized,
]);

table!(LINUX, "Linux", [
    // GTK 4 draws every widget through its own CSS engine, so each property
    // is a rule in the backend's style provider applied to the real widget
    // (`rustnative_linux`'s styling module) — the host's own mechanism, not
    // owner drawing.
    Foreground => StyleSupport::Realized,
    Background => StyleSupport::Realized,
    BorderColor => StyleSupport::Realized,
    BorderRadius => StyleSupport::Realized,
    FontSize => StyleSupport::Realized,
    FontWeight => StyleSupport::Realized,
    FontFamily => StyleSupport::Approximated(
        "the first family in the list is used; the generic families (`system-ui`, `sans-serif`, `serif`, \
         `monospace`) resolve through fontconfig's own aliases"
    ),
    Shadow => StyleSupport::Realized,
    Overflow => StyleSupport::Realized,
    Opacity => StyleSupport::Realized,
    Display => StyleSupport::Realized,
]);

table!(ANDROID, "Android", [
    // Every property is applied to the real platform widget through the
    // host's own mechanisms (`rustnative_android`'s styling module): a
    // `GradientDrawable` background with its stroke and corner radii
    // (wrapped in the host's ripple for pressable controls), `TextView`'s
    // colour, typeface, and size, `setPadding`, `setAlpha`, and
    // `setVisibility` — the way Android applications style their own views,
    // not owner drawing. An unstyled control keeps the device theme's look.
    Foreground => StyleSupport::Realized,
    Background => StyleSupport::Realized,
    BorderColor => StyleSupport::Realized,
    BorderRadius => StyleSupport::Realized,
    FontSize => StyleSupport::Realized,
    FontWeight => StyleSupport::Realized,
    FontFamily => StyleSupport::Approximated(
        "the first family in the list is used, through `Typeface.create`; the generic families map to the          system's `sans-serif`, `serif`, and `monospace` families, and `system-ui` to the device's default"
    ),
    Shadow => StyleSupport::Approximated(
        "realized as elevation (`View.setElevation`) over an outline that follows the corner radius: the host          draws its own ambient and key-light shadow, so the declared blur sets the elevation and the declared          offset and colour are not used"
    ),
    Overflow => StyleSupport::Realized,
    Opacity => StyleSupport::Realized,
    Display => StyleSupport::Realized,
]);

table!(HEADLESS, "headless", [
    Foreground => StyleSupport::Realized,
    Background => StyleSupport::Realized,
    BorderColor => StyleSupport::Realized,
    BorderRadius => StyleSupport::Realized,
    FontSize => StyleSupport::Realized,
    FontWeight => StyleSupport::Realized,
    FontFamily => StyleSupport::Realized,
    Shadow => StyleSupport::Realized,
    Overflow => StyleSupport::Realized,
    Opacity => StyleSupport::Realized,
    Display => StyleSupport::Realized,
]);

table!(TUI, "terminal", [
    // The terminal is the host (`PLAN.md` Milestone 38): its cell grid has
    // colours at its own depth and text attributes, and nothing else. What
    // a cell has no notion of is unavailable rather than imitated (2.14).
    Foreground => StyleSupport::Approximated(
        "the terminal's colour depth, detected rather than assumed: true colour exactly, otherwise the nearest \
         colour of the 256- or 16-colour palette; none at all under `NO_COLOR`. A colour equal to the default \
         theme's is the terminal's own default colour"
    ),
    Background => StyleSupport::Approximated(
        "as `color`: the cells' background at the terminal's colour depth; translucent colours are blended over \
         what is beneath before they reach the terminal"
    ),
    BorderColor => StyleSupport::Approximated(
        "box-drawing characters in the colour (`┌─┐│└┘`) on the edge cells of a box that declares a border, over \
         whatever content reaches its edge — give the box a cell of padding to keep content clear; where the \
         terminal's font has no box drawing they are still written, and look as that font draws them"
    ),
    BorderRadius => StyleSupport::Unavailable("a character cell has no corner radius"),
    FontSize => StyleSupport::Unavailable(
        "a terminal draws every cell in one font at one size, both chosen by the person, not the application"
    ),
    FontWeight => StyleSupport::Approximated(
        "600 and above is bold (SGR 1), 300 and below is faint (SGR 2), everything between is the terminal's \
         normal weight"
    ),
    FontFamily => StyleSupport::Unavailable("the terminal's font is the person's choice, one for every cell"),
    Shadow => StyleSupport::Unavailable("a character cell casts no shadow, and drawing one would fake a window"),
    Overflow => StyleSupport::Realized,
    Opacity => StyleSupport::Approximated(
        "zero draws nothing; below one half the content is faint (SGR 2); otherwise it is opaque — a cell has \
         no partial transparency"
    ),
    Display => StyleSupport::Realized,
]);

table!(WEB, "Web", [
    // The one backend whose host speaks the declaration vocabulary
    // natively: every property is a CSS property, applied by the browser
    // from the generated stylesheet (`rustnative_web::css`).
    Foreground => StyleSupport::Realized,
    Background => StyleSupport::Realized,
    BorderColor => StyleSupport::Realized,
    BorderRadius => StyleSupport::Realized,
    FontSize => StyleSupport::Realized,
    FontWeight => StyleSupport::Realized,
    FontFamily => StyleSupport::Approximated(
        "the whole family list is given to the browser, which uses the first installed family; other backends use \
         the first family named"
    ),
    Shadow => StyleSupport::Realized,
    Overflow => StyleSupport::Realized,
    Opacity => StyleSupport::Realized,
    Display => StyleSupport::Realized,
]);

/// The Web backend's unit mapping.
pub const WEB_UNITS: UnitMapping = UnitMapping {
    backend: "Web",
    host_unit: "CSS pixels",
    pixel: "one logical pixel is one CSS pixel; the browser maps CSS pixels to device pixels by its \
            device-pixel ratio, which follows the display and the page zoom",
    rem: "the CSS `rem`: the root font size, which the browser sets from the person's text-size setting (16 CSS \
          pixels by default); declared `rem` lengths reach the browser as `rem`, so they follow that setting live",
    rounding: "none by the framework: the browser lays out in fractional CSS pixels and snaps to device pixels \
               itself",
};

/// The Windows backend's unit mapping.
pub const WINDOWS_UNITS: UnitMapping = UnitMapping {
    backend: "Windows",
    host_unit: "device pixels",
    pixel: "one logical pixel is `GetDpiForWindow / 96` device pixels, per window: layout runs in logical pixels and \
            control rectangles, scroll offsets, padding, corner radii, fonts, and canvases are scaled where they \
            reach Win32, following `WM_DPICHANGED` on the same windows; a container's border stays a one-device-pixel \
            hairline",
    rem: "16 logical pixels × the text-scale factor (`keys::TEXT_SCALE`, fed from the system text-size setting) for \
          lengths; font sizes are specified at the default text size and every font — themed or declared — is \
          scaled by the text scale when it is realized",
    rounding: "half away from zero, once, where a length becomes whole pixels",
};

/// The Linux backend's unit mapping.
pub const LINUX_UNITS: UnitMapping = UnitMapping {
    backend: "Linux",
    host_unit: "GTK application pixels",
    pixel: "one logical pixel is one GTK application pixel; GTK and the compositor scale application pixels to \
            device pixels by the surface's scale, which is per monitor and may be fractional, and a native \
            surface reports that scale as its `scale_factor`",
    rem: "16 logical pixels × the text-scale factor (`keys::TEXT_SCALE`, fed from the desktop's text-scaling \
          factor through the Settings portal or GTK's Xft DPI) for lengths; fonts are realized at their specified \
          size × the text scale, and Pango measures them at that size",
    rounding: "half away from zero, once, where a length becomes whole application pixels",
};

/// The Android backend's unit mapping.
pub const ANDROID_UNITS: UnitMapping = UnitMapping {
    backend: "Android",
    host_unit: "density-independent pixels (dp)",
    pixel: "one logical pixel is one dp; the backend places views in device pixels at dp ×             `DisplayMetrics.density` (2.75 on a typical phone), and a native surface reports that density as its             `scale_factor`",
    rem: "16 dp × the system font scale (`Configuration.fontScale`, fed into `keys::TEXT_SCALE`) for lengths;           fonts are realized at their specified size × the font scale (as `sp` are), and `StaticLayout` measures           them at that size",
    rounding: "half away from zero, once, where a length becomes whole device pixels",
};

/// The headless backend's unit mapping.
pub const HEADLESS_UNITS: UnitMapping = UnitMapping {
    backend: "headless",
    host_unit: "logical pixels",
    pixel: "one logical pixel is one unit (there is no device)",
    rem: "16 logical pixels × the text-scale factor for lengths; fonts are measured at the text scale",
    rounding: "half away from zero, once, where a length becomes whole pixels",
};

/// The terminal backend's unit mapping.
pub const TUI_UNITS: UnitMapping = UnitMapping {
    backend: "terminal",
    host_unit: "character cells",
    pixel: "one cell is 8 logical pixels wide and 16 high by default (`TuiPlatform::with_cell_size` sets another \
            ratio); terminals do not reliably report their font's pixel size, so the ratio is the backend's \
            documented one rather than measured, and text is measured in cells by Unicode display width",
    rem: "one line: 16 logical pixels. A terminal's text size is the person's own setting, applied by the terminal \
          to every cell and never reported, so the text scale is 1",
    rounding: "half away from zero where a length becomes cells, with a rectangle's two edges snapped separately \
               so touching boxes still touch, and a nonzero length at least one cell",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_table_answers_every_property() {
        for table in [WINDOWS, LINUX, ANDROID, HEADLESS, WEB, TUI] {
            for property in StyleProperty::ALL {
                assert!(
                    table.entries.iter().any(|(entry, _)| entry == property),
                    "{} does not answer `{property}`",
                    table.backend
                );
            }
        }
        assert!(matches!(WINDOWS.support(StyleProperty::Shadow), StyleSupport::Unavailable(_)));
        assert!(matches!(
            StyleCapabilities::NONE.support(StyleProperty::Width),
            StyleSupport::Unavailable(_)
        ));
    }
}
