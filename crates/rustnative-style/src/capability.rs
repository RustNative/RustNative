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

/// The Windows backend's unit mapping.
pub const WINDOWS_UNITS: UnitMapping = UnitMapping {
    backend: "Windows",
    host_unit: "device pixels",
    pixel: "one logical pixel is one device pixel: layout is not yet scaled by `GetDpiForWindow / 96` (a surface \
            reports that ratio as its `scale_factor`; scaling layout itself is recorded as owed in BUILD_STATUS.md)",
    rem: "16 logical pixels × the text-scale factor (`keys::TEXT_SCALE`, fed from the system text-size setting) for \
          lengths; font sizes are specified at the default text size and every font — themed or declared — is \
          scaled by the text scale when it is realized",
    rounding: "half away from zero, once, where a length becomes whole pixels",
};

/// The headless backend's unit mapping.
pub const HEADLESS_UNITS: UnitMapping = UnitMapping {
    backend: "headless",
    host_unit: "logical pixels",
    pixel: "one logical pixel is one unit (there is no device)",
    rem: "16 logical pixels × the text-scale factor for lengths; fonts are measured at the text scale",
    rounding: "half away from zero, once, where a length becomes whole pixels",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_table_answers_every_property() {
        for table in [WINDOWS, HEADLESS] {
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
