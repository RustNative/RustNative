//! Host traits, read from the desktop and fed into the environment
//! (`rustnative_core::environment`), so no component queries the host.
//!
//! | Key | Source (portal first, then GTK's own settings) |
//! |---|---|
//! | `COLOR_SCHEME` | Settings portal `color-scheme`; `gtk-application-prefer-dark-theme`, a `-dark` theme |
//! | `TEXT_SCALE` | portal `text-scaling-factor`; `gtk-xft-dpi` / 96 |
//! | `CONTRAST` | portal `contrast`; a high-contrast theme name |
//! | `REDUCED_MOTION` | portal `reduced-motion`; `gtk-enable-animations` |
//! | `LOCALE`, `LAYOUT_DIRECTION` | `LC_ALL`/`LC_MESSAGES`/`LANG`, and the language's direction |
//! | host colours | portal `accent-color` |
//! | `SAFE_AREA`, `POSTURE` | none: a desktop window has neither, so the defaults stand |
//!
//! Read at startup and again whenever the portal reports a change or a GTK
//! setting changes. Setting an unchanged value invalidates nothing, so
//! re-reading everything on any change is cheap and cannot drift.
//!
//! GTK 4.14 does not follow the portal's colour scheme or contrast by
//! itself (libadwaita does that for its applications); this module does it
//! for GTK's own widgets, so an unstyled button is the desktop's button in
//! the desktop's scheme.

use rustnative_core::{
    Application, ColorScheme, Contrast, HostPalette, Locale, MotionPreference, Scalar, keys,
};

use crate::desktop::settings::SettingsPortal;

/// Everything this module reads, in one value — what a test compares.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HostTraits {
    pub(crate) scheme: ColorScheme,
    pub(crate) text_scale: Scalar,
    pub(crate) contrast: Contrast,
    pub(crate) motion: MotionPreference,
    pub(crate) locale: Locale,
    pub(crate) palette: HostPalette,
}

/// Reads the host's current traits.
pub(crate) fn read(portal: &SettingsPortal) -> HostTraits {
    let from_portal = portal.traits();
    let settings = gtk::Settings::default();
    let theme = settings
        .as_ref()
        .and_then(gtk::Settings::gtk_theme_name)
        .unwrap_or_default()
        .to_lowercase();
    let gtk_dark =
        settings.as_ref().is_some_and(gtk::Settings::is_gtk_application_prefer_dark_theme)
            || theme.ends_with("-dark")
            || theme.ends_with(":dark");
    let gtk_high_contrast = theme.contains("highcontrast") || theme.contains("-hc");
    let gtk_animations = settings.as_ref().is_none_or(gtk::Settings::is_gtk_enable_animations);
    // `gtk-xft-dpi` is 1024 × the DPI; 96 is a text scale of one.
    let gtk_scale =
        settings.as_ref().map(gtk::Settings::gtk_xft_dpi).filter(|dpi| *dpi > 0).map(|dpi| {
            #[allow(
                clippy::cast_precision_loss,
                reason = "a DPI × 1024, far inside f32's exact range"
            )]
            let scale = dpi as f32 / (96.0 * 1024.0);
            scale
        });
    let mut palette = HostPalette::default();
    if let Some(accent) = from_portal.accent {
        palette.accent = accent;
        palette.on_accent = readable_on(accent);
    }
    HostTraits {
        scheme: from_portal.scheme.unwrap_or(if gtk_dark {
            ColorScheme::Dark
        } else {
            ColorScheme::Light
        }),
        text_scale: Scalar::new(
            from_portal.text_scale.or(gtk_scale).unwrap_or(1.0).clamp(0.5, 3.0),
        ),
        contrast: from_portal.contrast.unwrap_or(if gtk_high_contrast {
            Contrast::High
        } else {
            Contrast::Standard
        }),
        motion: from_portal.motion.unwrap_or(if gtk_animations {
            MotionPreference::Full
        } else {
            MotionPreference::Reduced
        }),
        locale: crate::desktop::locale::current(),
        palette,
    }
}

/// White or black, whichever reads on `color`.
fn readable_on(color: rustnative_core::Color) -> rustnative_core::Color {
    let luminance =
        u32::from(color.red) * 299 + u32::from(color.green) * 587 + u32::from(color.blue) * 114;
    if luminance > 150_000 {
        rustnative_core::Color::rgb(0, 0, 0)
    } else {
        rustnative_core::Color::rgb(255, 255, 255)
    }
}

/// Feeds the host's traits into `application`'s environment.
pub(crate) fn apply(application: &mut Application, traits: &HostTraits) {
    application.set_environment(&keys::COLOR_SCHEME, traits.scheme);
    application.set_environment(&keys::TEXT_SCALE, traits.text_scale);
    application.set_environment(&keys::CONTRAST, traits.contrast);
    application.set_motion_preference(traits.motion);
    if application.environment_for(rustnative_core::WindowId::PRIMARY, &keys::LOCALE)
        != traits.locale
    {
        application.set_locale(traits.locale.clone());
    }
    application.set_host_palette(traits.palette);
}

/// Makes GTK's own widgets follow the traits the portal stated: its dark
/// variant for a dark scheme, its high-contrast theme for high contrast.
pub(crate) fn follow_in_gtk(traits: &HostTraits) {
    let Some(settings) = gtk::Settings::default() else { return };
    let dark = traits.scheme == ColorScheme::Dark;
    if settings.is_gtk_application_prefer_dark_theme() != dark {
        settings.set_gtk_application_prefer_dark_theme(dark);
    }
    let theme = settings.gtk_theme_name().map(|name| name.to_string()).unwrap_or_default();
    let is_high_contrast =
        theme.to_lowercase().contains("hc") || theme.to_lowercase().contains("highcontrast");
    if traits.contrast == Contrast::High && !is_high_contrast {
        // GTK 4's built-in high-contrast theme.
        settings.set_gtk_theme_name(Some(if dark { "Default-hc-dark" } else { "Default-hc" }));
    }
    if settings.is_gtk_enable_animations() != (traits.motion == MotionPreference::Full) {
        settings.set_gtk_enable_animations(traits.motion == MotionPreference::Full);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_on_an_accent_is_whichever_reads() {
        assert_eq!(
            readable_on(rustnative_core::Color::rgb(255, 255, 0)),
            rustnative_core::Color::rgb(0, 0, 0)
        );
        assert_eq!(
            readable_on(rustnative_core::Color::rgb(20, 40, 120)),
            rustnative_core::Color::rgb(255, 255, 255)
        );
    }
}
