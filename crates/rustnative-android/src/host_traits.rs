//! The host's traits — night mode, font scale, contrast, animations,
//! locale and direction, and the theme's colours — read from the activity's
//! configuration and fed into the environment (`PLAN.md` 2.4).
//!
//! They change through `onConfigurationChanged` (the backend handles every
//! configuration change in place: `lifecycle::step`), and the same views are
//! restyled with no application render unless a component reads the
//! changed value.

use rustnative_core::{
    Application, Color, ColorScheme, Contrast, HostPalette, Locale, MotionPreference, Scalar, keys,
};

#[cfg(target_os = "android")]
use crate::Error;
#[cfg(target_os = "android")]
use crate::jni_host::{Arg, Class, JavaRef, call_static};

/// What the host says.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HostTraits {
    pub(crate) density: f32,
    pub(crate) scheme: ColorScheme,
    pub(crate) text_scale: Scalar,
    pub(crate) contrast: Contrast,
    pub(crate) motion: MotionPreference,
    pub(crate) locale: Locale,
    pub(crate) right_to_left: bool,
    pub(crate) multi_window: bool,
    pub(crate) palette: HostPalette,
}

impl Default for HostTraits {
    fn default() -> Self {
        Self {
            density: 1.0,
            scheme: ColorScheme::Light,
            text_scale: Scalar::ONE,
            contrast: Contrast::Standard,
            motion: MotionPreference::Full,
            locale: Locale::default(),
            right_to_left: false,
            multi_window: false,
            palette: HostPalette::default(),
        }
    }
}

/// Reads the traits from `activity`.
#[cfg(target_os = "android")]
pub(crate) fn read(activity: &JavaRef) -> Result<HostTraits, Error> {
    let values = call_static(
        Class::Platform,
        "traits",
        "(Landroid/app/Activity;)[F",
        &[Arg::Obj(activity)],
    )?
    .floats();
    let locale = call_static(
        Class::Platform,
        "locale",
        "(Landroid/app/Activity;)Ljava/lang/String;",
        &[Arg::Obj(activity)],
    )?
    .string()
    .unwrap_or_else(|| "en-US".to_owned());
    let colors = call_static(
        Class::Platform,
        "colors",
        "(Landroid/app/Activity;)[I",
        &[Arg::Obj(activity)],
    )?
    .ints();
    Ok(from_values(&values, &locale, &colors))
}

/// The traits from the host library's numbers (`RnPlatform.traits`,
/// `RnPlatform.colors`).
pub(crate) fn from_values(values: &[f32], locale: &str, colors: &[i32]) -> HostTraits {
    let value = |index: usize, default: f32| values.get(index).copied().unwrap_or(default);
    let contrast = value(5, -1.0);
    let high_text = value(6, 0.0) > 0.5;
    let mut palette = HostPalette::default();
    let color = |index: usize| colors.get(index).copied().filter(|argb| *argb != 0).map(from_argb);
    if let Some(accent) = color(0) {
        palette.accent = accent;
        palette.on_accent = readable_on(accent);
    }
    if let Some(surface) = color(2) {
        palette.surface = surface;
    }
    if let Some(on_surface) = color(3) {
        palette.on_surface = on_surface;
    }
    if let Some(highlight) = color(4) {
        palette.highlight = highlight;
        palette.on_highlight = readable_on(highlight);
    }
    if let Some(border) = color(5) {
        palette.border = border;
    }
    if let Some(muted) = color(6) {
        palette.muted = muted;
    }
    HostTraits {
        density: value(0, 1.0).max(0.1),
        scheme: if value(2, 0.0) > 0.5 { ColorScheme::Dark } else { ColorScheme::Light },
        text_scale: Scalar::new(value(1, 1.0).clamp(0.5, 3.0)),
        // `UiModeManager.getContrast` runs from -1 to 1; the person's "high"
        // is 1, "medium" 0.5. High text contrast is high contrast too.
        contrast: if high_text || contrast >= 0.5 { Contrast::High } else { Contrast::Standard },
        motion: if value(4, 1.0) > 0.5 {
            MotionPreference::Full
        } else {
            MotionPreference::Reduced
        },
        locale: Locale::new(locale),
        right_to_left: value(3, 0.0) > 0.5,
        multi_window: value(9, 0.0) > 0.5,
        palette,
    }
}

fn from_argb(argb: i32) -> Color {
    let [alpha, red, green, blue] = argb.to_be_bytes();
    Color::rgba(red, green, blue, alpha)
}

fn readable_on(color: Color) -> Color {
    let luminance =
        u32::from(color.red) * 299 + u32::from(color.green) * 587 + u32::from(color.blue) * 114;
    if luminance > 150_000 { Color::rgb(0, 0, 0) } else { Color::rgb(255, 255, 255) }
}

/// Feeds `traits` into the application's environment.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_librarys_numbers_become_traits() {
        let traits = from_values(
            &[2.75, 1.3, 1.0, 0.0, 0.0, 1.0, 0.0, 1080.0, 2400.0, 1.0],
            "ar-EG",
            &[i32::from_be_bytes([255, 0, 90, 200])],
        );
        assert!((traits.density - 2.75).abs() < f32::EPSILON);
        assert_eq!(traits.scheme, ColorScheme::Dark);
        assert_eq!(traits.text_scale, Scalar::new(1.3));
        assert_eq!(traits.contrast, Contrast::High);
        assert_eq!(traits.motion, MotionPreference::Reduced);
        assert_eq!(traits.locale, Locale::new("ar-EG"));
        assert!(traits.multi_window);
        assert_eq!(traits.palette.accent, Color::rgb(0, 90, 200));
        assert_eq!(traits.palette.on_accent, Color::rgb(255, 255, 255));
        let plain = from_values(&[], "en-US", &[]);
        assert_eq!(plain.scheme, ColorScheme::Light);
        assert_eq!(plain.contrast, Contrast::Standard);
    }
}
