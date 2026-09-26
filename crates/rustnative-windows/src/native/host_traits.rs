//! Host traits, read from Windows and fed into the environment
//! (`rustnative_core::environment`), so no component queries the host.
//!
//! | Key | Windows source |
//! |---|---|
//! | `COLOR_SCHEME` | `HKCU\…\Themes\Personalize\AppsUseLightTheme` |
//! | `TEXT_SCALE` | `HKCU\Software\Microsoft\Accessibility\TextScaleFactor` (percent) |
//! | `CONTRAST` | `SPI_GETHIGHCONTRAST` |
//! | `REDUCED_MOTION` | `SPI_GETCLIENTAREAANIMATION` (see `native::animation`) |
//! | `LOCALE`, `LAYOUT_DIRECTION` | `GetUserDefaultLocaleName`, and its script's direction |
//! | `WINDOW_MODE` | the window's width against its monitor's work area, when snapped |
//! | `SAFE_AREA`, `POSTURE` | none: a desktop window has neither, so the defaults stand |
//! | host colors (`Application::set_host_palette`) | `DwmGetColorizationColor`, `GetSysColor` |
//!
//! Read once when the application starts and again on every
//! `WM_SETTINGCHANGE`. Setting an unchanged value invalidates nothing, so
//! re-reading everything on any change is cheap and cannot drift.

use rustnative_core::{
    Application, Color, ColorScheme, Contrast, HostPalette, Locale, Scalar, WindowId, WindowMode,
    keys,
};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
use windows_sys::Win32::Graphics::Dwm::DwmGetColorizationColor;
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_BTNSHADOW, COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW,
    COLOR_WINDOWTEXT, GetSysColor,
};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, IsZoomed, SPI_GETHIGHCONTRAST, SystemParametersInfoW,
};

use super::util::wide;

/// Everything this module reads, in one value — what a test compares.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HostTraits {
    pub(crate) scheme: ColorScheme,
    pub(crate) text_scale: Scalar,
    pub(crate) contrast: Contrast,
    pub(crate) locale: Locale,
    /// The colors host-following tokens take (`docs/tokens.md`).
    pub(crate) palette: HostPalette,
}

fn system_color(index: i32) -> Color {
    // SAFETY: a documented system-color index; no pointers.
    let value = unsafe { GetSysColor(index) };
    let [red, green, blue, _] = value.to_le_bytes();
    Color::rgb(red, green, blue)
}

/// The person's colors: the accent they chose (`DwmGetColorizationColor`)
/// and the system colors of windows, text, and selection.
pub(crate) fn palette() -> HostPalette {
    let mut colorization = 0_u32;
    let mut opaque = 0;
    // SAFETY: both out-parameters are valid for the call.
    let ok = unsafe { DwmGetColorizationColor(&raw mut colorization, &raw mut opaque) } >= 0;
    let fallback = HostPalette::default();
    let accent = if ok {
        let [blue, green, red, _] = colorization.to_le_bytes();
        Color::rgb(red, green, blue)
    } else {
        fallback.accent
    };
    // White or black text on the accent, whichever reads.
    let luminance =
        u32::from(accent.red) * 299 + u32::from(accent.green) * 587 + u32::from(accent.blue) * 114;
    let on_accent =
        if luminance > 150_000 { Color::rgb(0, 0, 0) } else { Color::rgb(255, 255, 255) };
    HostPalette {
        accent,
        on_accent,
        surface: system_color(COLOR_WINDOW),
        on_surface: system_color(COLOR_WINDOWTEXT),
        highlight: system_color(COLOR_HIGHLIGHT),
        on_highlight: system_color(COLOR_HIGHLIGHTTEXT),
        border: system_color(COLOR_BTNSHADOW),
        muted: system_color(COLOR_GRAYTEXT),
    }
}

/// Reads the host's current traits.
pub(crate) fn read() -> HostTraits {
    HostTraits {
        scheme: if read_dword(
            r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "AppsUseLightTheme",
        ) == Some(0)
        {
            ColorScheme::Dark
        } else {
            ColorScheme::Light
        },
        text_scale: read_dword(r"Software\Microsoft\Accessibility", "TextScaleFactor").map_or(
            Scalar::ONE,
            |percent| {
                #[allow(clippy::cast_precision_loss, reason = "a percentage, 100 to 225")]
                let scale = percent.clamp(100, 225) as f32 / 100.0;
                Scalar::new(scale)
            },
        ),
        contrast: if high_contrast() { Contrast::High } else { Contrast::Standard },
        locale: user_locale().map_or_else(Locale::default, Locale::new),
        palette: palette(),
    }
}

/// Feeds the host's traits into `application`'s environment.
pub(crate) fn apply(application: &mut Application, traits: &HostTraits) {
    application.set_environment(&keys::COLOR_SCHEME, traits.scheme);
    application.set_environment(&keys::TEXT_SCALE, traits.text_scale);
    application.set_environment(&keys::CONTRAST, traits.contrast);
    // A locale change also changes the direction; an unchanged one changes
    // neither, so this is safe to repeat on every settings change.
    if application.environment_for(WindowId::PRIMARY, &keys::LOCALE) != traits.locale {
        application.set_locale(traits.locale.clone());
    }
    application.set_host_palette(traits.palette);
}

/// Reports whether `window` shares its monitor with another window — a
/// snapped half or third — as `WindowMode::Split` with its share of the
/// work area's width.
pub(crate) fn window_mode(window: HWND) -> WindowMode {
    let mut rect = RECT::default();
    // SAFETY: `window` is live; `rect` is a valid out-parameter.
    let have_rect = unsafe { GetWindowRect(window, &raw mut rect) } != 0;
    // SAFETY: `window` is live.
    let monitor = unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: u32::try_from(std::mem::size_of::<MONITORINFO>()).unwrap_or(40),
        ..MONITORINFO::default()
    };
    // SAFETY: `info.cbSize` is set as the API requires.
    let have_info = unsafe { GetMonitorInfoW(monitor, &raw mut info) } != 0;
    // SAFETY: `window` is live.
    let maximized = unsafe { IsZoomed(window) } != 0;
    if !have_rect || !have_info || maximized {
        return WindowMode::Full;
    }
    let work = info.rcWork;
    let work_width = (work.right - work.left).max(1);
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    let full_height = (height - (work.bottom - work.top)).abs() <= 16;
    let fraction = f64::from(width) / f64::from(work_width);
    if full_height && fraction < 0.9 {
        #[allow(clippy::cast_possible_truncation, reason = "a fraction between 0 and 1")]
        let fraction = fraction as f32;
        WindowMode::Split { fraction: Scalar::new(fraction) }
    } else {
        WindowMode::Full
    }
}

fn read_dword(subkey: &str, value: &str) -> Option<u32> {
    let subkey = wide(subkey);
    let value = wide(value);
    let mut data: u32 = 0;
    let mut size = u32::try_from(std::mem::size_of::<u32>()).unwrap_or(4);
    // SAFETY: both strings are NUL-terminated wide strings that outlive the
    // call; `data`/`size` describe a writable `u32`.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut data).cast(),
            &raw mut size,
        )
    };
    (status == 0).then_some(data)
}

fn high_contrast() -> bool {
    let mut contrast = HIGHCONTRASTW {
        cbSize: u32::try_from(std::mem::size_of::<HIGHCONTRASTW>()).unwrap_or(16),
        dwFlags: 0,
        lpszDefaultScheme: std::ptr::null_mut(),
    };
    // SAFETY: `SPI_GETHIGHCONTRAST` fills a `HIGHCONTRASTW` whose `cbSize`
    // is set; `uiParam` must be the structure size.
    let read = unsafe {
        SystemParametersInfoW(SPI_GETHIGHCONTRAST, contrast.cbSize, (&raw mut contrast).cast(), 0)
    } != 0;
    read && contrast.dwFlags & HCF_HIGHCONTRASTON != 0
}

fn user_locale() -> Option<String> {
    let mut buffer = [0_u16; 85];
    // SAFETY: `buffer` holds `LOCALE_NAME_MAX_LENGTH` (85) wide characters.
    let length = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), 85) };
    let length = usize::try_from(length).ok().filter(|length| *length > 1)?;
    Some(String::from_utf16_lossy(&buffer[..length - 1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_palette_reads_without_failing() {
        let palette = palette();
        assert_eq!(palette.accent.alpha, 255);
    }

    #[test]
    fn the_host_answers_every_trait() {
        let traits = read();
        assert!(!traits.locale.tag().is_empty(), "Windows always has a user locale");
        let scale = traits.text_scale.get();
        assert!((1.0..=2.25).contains(&scale), "{scale}");
    }

    #[test]
    fn host_traits_reach_the_environment() {
        use rustnative_core::{Component, Event, Node, Size, Window};
        struct Empty;
        impl Component for Empty {
            type Props = ();
            type Message = ();
            fn new((): ()) -> Self {
                Self
            }
            fn props(&self) -> &() {
                &()
            }
            fn set_props(&mut self, (): ()) {}
            fn view(&self) -> Node {
                Node::label("x", "")
            }
            fn update(&mut self, _: Event) {}
        }
        let mut application = Application::new(Empty, Window::new("t", Size::new(10, 10)));
        let traits = HostTraits {
            scheme: ColorScheme::Dark,
            text_scale: Scalar::new(1.5),
            contrast: Contrast::High,
            locale: Locale::new("he-IL"),
            palette: HostPalette { accent: Color::rgb(1, 2, 3), ..HostPalette::default() },
        };
        application.set_theme(
            rustnative_core::Theme::default()
                .with_host_role("color-accent", rustnative_core::HostRole::Accent),
        );
        apply(&mut application, &traits);
        assert_eq!(
            application.theme().tokens().get("color-accent"),
            Some(&rustnative_core::StyleValue::Color(Color::rgb(1, 2, 3))),
            "a host-following token takes the host's color"
        );
        let id = WindowId::PRIMARY;
        assert_eq!(application.environment_for(id, &keys::COLOR_SCHEME), ColorScheme::Dark);
        assert_eq!(application.environment_for(id, &keys::CONTRAST), Contrast::High);
        assert_eq!(application.layout_direction(id), rustnative_core::LayoutDirection::Rtl);
    }
}
