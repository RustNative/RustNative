//! The desktop's appearance settings through the Settings portal
//! (`org.freedesktop.portal.Settings`), the one interface every current
//! desktop answers them through — GNOME, KDE, and the others each implement
//! the portal over their own configuration.
//!
//! | Trait | Portal key |
//! |---|---|
//! | colour scheme | `org.freedesktop.appearance` `color-scheme` (1 dark, 2 light) |
//! | contrast | `org.freedesktop.appearance` `contrast` (1 high) |
//! | reduced motion | `org.freedesktop.appearance` `reduced-motion` (1 reduced) |
//! | accent colour | `org.freedesktop.appearance` `accent-color` (r, g, b in 0–1) |
//! | text scale | `org.gnome.desktop.interface` `text-scaling-factor` |
//!
//! Each answer is optional: a desktop without a portal, or a portal
//! without a key, answers nothing, and the toolkit's own settings fill in
//! (`gtk::host_traits`).

use gio::prelude::*;
use rustnative_core::{Color, ColorScheme, Contrast, MotionPreference};

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const INTERFACE: &str = "org.freedesktop.portal.Settings";
const APPEARANCE: &str = "org.freedesktop.appearance";
const GNOME_INTERFACE: &str = "org.gnome.desktop.interface";

/// What the portal said, each trait optional.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PortalTraits {
    /// The colour scheme, when the desktop states a preference.
    pub scheme: Option<ColorScheme>,
    /// The contrast setting.
    pub contrast: Option<Contrast>,
    /// The reduced-motion setting.
    pub motion: Option<MotionPreference>,
    /// The accent colour.
    pub accent: Option<Color>,
    /// The text-scaling factor.
    pub text_scale: Option<f32>,
}

/// A connection to the Settings portal.
#[derive(Debug)]
pub struct SettingsPortal {
    proxy: Option<gio::DBusProxy>,
}

impl SettingsPortal {
    /// Connects to the portal on the session bus. A session with no bus,
    /// or no portal, gives a portal that answers nothing.
    #[must_use]
    pub fn connect() -> Self {
        let proxy = gio::DBusProxy::for_bus_sync(
            gio::BusType::Session,
            gio::DBusProxyFlags::DO_NOT_LOAD_PROPERTIES,
            None,
            PORTAL,
            PATH,
            INTERFACE,
            gio::Cancellable::NONE,
        )
        .ok();
        Self { proxy }
    }

    /// Reads `namespace`/`key`, trying the portal's `ReadOne` (version 2)
    /// and then `Read` (version 1, which wraps the value once more).
    #[must_use]
    pub fn read(&self, namespace: &str, key: &str) -> Option<glib::Variant> {
        let proxy = self.proxy.as_ref()?;
        let arguments = (namespace, key).to_variant();
        let call = |method: &str| {
            proxy
                .call_sync(
                    method,
                    Some(&arguments),
                    gio::DBusCallFlags::NONE,
                    500,
                    gio::Cancellable::NONE,
                )
                .ok()
        };
        if let Some(reply) = call("ReadOne") {
            return reply.child_value(0).as_variant();
        }
        let reply = call("Read")?;
        let outer = reply.child_value(0).as_variant()?;
        // `Read` returns `v`, whose value some portals wrap in a second `v`.
        Some(outer.as_variant().unwrap_or(outer))
    }

    /// Everything this module reads.
    #[must_use]
    pub fn traits(&self) -> PortalTraits {
        let number = |namespace: &str, key: &str| {
            self.read(namespace, key).and_then(|value| value.get::<u32>())
        };
        PortalTraits {
            scheme: number(APPEARANCE, "color-scheme").and_then(scheme),
            contrast: number(APPEARANCE, "contrast")
                .map(|value| if value == 1 { Contrast::High } else { Contrast::Standard }),
            motion: number(APPEARANCE, "reduced-motion").map(|value| {
                if value == 1 { MotionPreference::Reduced } else { MotionPreference::Full }
            }),
            accent: self.read(APPEARANCE, "accent-color").and_then(|value| accent(&value)),
            text_scale: self
                .read(GNOME_INTERFACE, "text-scaling-factor")
                .and_then(|value| value.get::<f64>())
                .and_then(text_scale),
        }
    }

    /// Calls `changed` whenever the portal reports a setting this module
    /// reads changed.
    pub fn connect_changed(&self, changed: impl Fn() + 'static) {
        let Some(proxy) = &self.proxy else { return };
        proxy.connect_local("g-signal", false, move |values| {
            let signal = values.get(2).and_then(|value| value.get::<String>().ok());
            if signal.as_deref() == Some("SettingChanged") {
                changed();
            }
            None
        });
    }

    /// Whether a portal answered at all.
    #[must_use]
    pub fn is_available(&self) -> bool {
        self.read(APPEARANCE, "color-scheme").is_some()
    }
}

const fn scheme(value: u32) -> Option<ColorScheme> {
    match value {
        1 => Some(ColorScheme::Dark),
        2 => Some(ColorScheme::Light),
        // 0: no preference, which leaves the toolkit's own answer.
        _ => None,
    }
}

fn accent(value: &glib::Variant) -> Option<Color> {
    let (red, green, blue) = value.get::<(f64, f64, f64)>()?;
    // Out of range means "no accent" in the portal's documentation.
    let channel = |value: f64| {
        (0.0..=1.0).contains(&value).then(|| {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a channel in 0–1, scaled to 0–255 and rounded"
            )]
            let byte = (value * 255.0).round() as u8;
            byte
        })
    };
    Some(Color::rgb(channel(red)?, channel(green)?, channel(blue)?))
}

fn text_scale(value: f64) -> Option<f32> {
    #[allow(clippy::cast_possible_truncation, reason = "a text scale, 0.5 to 3")]
    let scale = value as f32;
    (scale.is_finite() && scale > 0.0).then_some(scale.clamp(0.5, 3.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_values_map_to_the_portable_traits() {
        assert_eq!(scheme(1), Some(ColorScheme::Dark));
        assert_eq!(scheme(2), Some(ColorScheme::Light));
        assert_eq!(scheme(0), None);
        assert_eq!(
            accent(&(1.0_f64, 0.5_f64, 0.0_f64).to_variant()),
            Some(Color::rgb(255, 128, 0))
        );
        assert_eq!(accent(&(2.0_f64, 0.0_f64, 0.0_f64).to_variant()), None);
        assert_eq!(text_scale(1.25), Some(1.25));
        assert_eq!(text_scale(-1.0), None);
    }
}
