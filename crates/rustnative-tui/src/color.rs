//! The terminal's colour depth, detected rather than assumed, and colours
//! brought down to it (`PLAN.md` Milestone 38).
//!
//! # Detection
//!
//! In order; the first rule that answers decides:
//!
//! 1. `RUSTNATIVE_TUI_COLORS` = `truecolor`, `256`, `16`, or `none`: the
//!    person's override.
//! 2. `NO_COLOR` set and not empty (<https://no-color.org>): no colour;
//!    text attributes still apply.
//! 3. `TERM=dumb`: no colour.
//! 4. `COLORTERM` = `truecolor` or `24bit`: true colour.
//! 5. A terminal known to draw true colour from its own variables:
//!    Windows Terminal (`WT_SESSION`), `TERM_PROGRAM` of iTerm2, WezTerm,
//!    VS Code, Ghostty, or Hyper; a `TERM` naming kitty, Alacritty, foot,
//!    WezTerm, Ghostty, or a `-direct` entry.
//! 6. The Windows console with virtual-terminal processing enabled (it
//!    draws true colour since Windows 10 1703).
//! 7. `TERM_PROGRAM=Apple_Terminal`, or a `TERM` ending in `256color`: 256.
//! 8. Any other `TERM` (`xterm`, `screen`, `linux`, `vt220` …): 16.
//! 9. Nothing at all: 16, the least a VT-compatible terminal draws.
//!
//! # Downsampling
//!
//! True colour is sent as given. For 256 colours, the nearest of the
//! 6×6×6 cube (levels 0, 95, 135, 175, 215, 255) and the 24-step grey ramp
//! is chosen; for 16, the nearest of xterm's default palette. "Nearest" is
//! the weighted ("redmean") distance, which tracks perceived difference far
//! better than plain RGB distance at no cost.

use rustnative_core::Color;

/// How many colours the terminal draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ColorDepth {
    /// No colour: the terminal's own foreground and background only.
    None,
    /// The 16 ANSI colours.
    Ansi16,
    /// The xterm 256-colour palette.
    Ansi256,
    /// 24-bit colour.
    TrueColor,
}

/// A colour as the terminal is told it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TermColor {
    /// The terminal's own default (SGR 39 / 49).
    Default,
    /// One of the 16 ANSI colours (0–7 normal, 8–15 bright).
    Ansi(u8),
    /// An index into the 256-colour palette.
    Indexed(u8),
    /// A 24-bit colour.
    Rgb(u8, u8, u8),
}

/// xterm's default 16-colour palette.
pub const ANSI_PALETTE: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (205, 0, 0),
    (0, 205, 0),
    (205, 205, 0),
    (0, 0, 238),
    (205, 0, 205),
    (0, 205, 205),
    (229, 229, 229),
    (127, 127, 127),
    (255, 0, 0),
    (0, 255, 0),
    (255, 255, 0),
    (92, 92, 255),
    (255, 0, 255),
    (0, 255, 255),
    (255, 255, 255),
];

const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Where detection reads from: the environment, and whether the Windows
/// console has virtual-terminal processing on.
#[derive(Debug, Clone, Default)]
pub struct DetectInput {
    /// Environment variables (name, value).
    pub vars: Vec<(String, String)>,
    /// Whether this is the Windows console in virtual-terminal mode.
    pub windows_vt: bool,
}

impl DetectInput {
    /// The process's own environment.
    #[must_use]
    pub fn from_process(windows_vt: bool) -> Self {
        Self { vars: std::env::vars().collect(), windows_vt }
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.vars.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }
}

impl ColorDepth {
    /// The depth detection answers for `input` (see the module docs).
    #[must_use]
    pub fn detect(input: &DetectInput) -> Self {
        match input.get("RUSTNATIVE_TUI_COLORS").map(str::to_ascii_lowercase).as_deref() {
            Some("truecolor" | "24bit") => return Self::TrueColor,
            Some("256") => return Self::Ansi256,
            Some("16") => return Self::Ansi16,
            Some("none" | "0") => return Self::None,
            _ => {}
        }
        if input.get("NO_COLOR").is_some_and(|value| !value.is_empty()) {
            return Self::None;
        }
        let term = input.get("TERM").unwrap_or("").to_ascii_lowercase();
        if term == "dumb" {
            return Self::None;
        }
        if matches!(
            input.get("COLORTERM").map(str::to_ascii_lowercase).as_deref(),
            Some("truecolor" | "24bit")
        ) {
            return Self::TrueColor;
        }
        let program = input.get("TERM_PROGRAM").unwrap_or("");
        if input.get("WT_SESSION").is_some()
            || matches!(program, "iTerm.app" | "WezTerm" | "vscode" | "ghostty" | "Hyper")
            || ["kitty", "alacritty", "foot", "wezterm", "ghostty", "-direct"]
                .iter()
                .any(|name| term.contains(name))
        {
            return Self::TrueColor;
        }
        if input.windows_vt && term.is_empty() {
            return Self::TrueColor;
        }
        if program == "Apple_Terminal" || term.ends_with("256color") {
            return Self::Ansi256;
        }
        Self::Ansi16
    }

    /// `color` as this depth can send it, or `None` when it sends no
    /// colour at all.
    #[must_use]
    pub fn encode(self, color: Color) -> Option<TermColor> {
        let rgb = (color.red, color.green, color.blue);
        match self {
            Self::None => None,
            Self::TrueColor => Some(TermColor::Rgb(rgb.0, rgb.1, rgb.2)),
            Self::Ansi256 => Some(TermColor::Indexed(nearest_256(rgb))),
            Self::Ansi16 => Some(TermColor::Ansi(nearest_16(rgb))),
        }
    }

    /// The name `docs/tui.md` and the inspection protocol use.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Ansi16 => "16",
            Self::Ansi256 => "256",
            Self::TrueColor => "truecolor",
        }
    }
}

/// The weighted ("redmean") squared distance between two colours.
fn distance(a: (u8, u8, u8), b: (u8, u8, u8)) -> i64 {
    let mean = (i64::from(a.0) + i64::from(b.0)) / 2;
    let (dr, dg, db) = (
        i64::from(a.0) - i64::from(b.0),
        i64::from(a.1) - i64::from(b.1),
        i64::from(a.2) - i64::from(b.2),
    );
    ((512 + mean) * dr * dr >> 8) + 4 * dg * dg + ((767 - mean) * db * db >> 8)
}

fn nearest_level(value: u8) -> usize {
    CUBE.iter()
        .enumerate()
        .min_by_key(|(_, level)| (i16::from(**level) - i16::from(value)).abs())
        .map_or(0, |(index, _)| index)
}

/// The 256-palette colour of index `index`.
#[must_use]
pub fn palette_256(index: u8) -> (u8, u8, u8) {
    match index {
        0..=15 => ANSI_PALETTE[usize::from(index)],
        16..=231 => {
            let cube = index - 16;
            (
                CUBE[usize::from(cube / 36)],
                CUBE[usize::from(cube / 6 % 6)],
                CUBE[usize::from(cube % 6)],
            )
        }
        232..=255 => {
            let grey = 8 + (index - 232) * 10;
            (grey, grey, grey)
        }
    }
}

/// The nearest colour of the 256-colour palette's cube and grey ramp.
#[must_use]
pub fn nearest_256(rgb: (u8, u8, u8)) -> u8 {
    let (r, g, b) = (nearest_level(rgb.0), nearest_level(rgb.1), nearest_level(rgb.2));
    let cube = u8::try_from(16 + 36 * r + 6 * g + b).unwrap_or(16);
    let average = (u16::from(rgb.0) + u16::from(rgb.1) + u16::from(rgb.2)) / 3;
    let step = u8::try_from(((average.saturating_sub(3)) / 10).min(23)).unwrap_or(23);
    let grey = 232 + step;
    if distance(palette_256(grey), rgb) < distance(palette_256(cube), rgb) { grey } else { cube }
}

/// The nearest of the 16 ANSI colours.
#[must_use]
pub fn nearest_16(rgb: (u8, u8, u8)) -> u8 {
    (0_u8..16).min_by_key(|index| distance(ANSI_PALETTE[usize::from(*index)], rgb)).unwrap_or(7)
}

/// `top` over `bottom` by `top`'s alpha (the result is opaque).
#[must_use]
pub fn blend(top: Color, bottom: Color) -> Color {
    let alpha = u16::from(top.alpha);
    let mix = |a: u8, b: u8| {
        u8::try_from((u16::from(a) * alpha + u16::from(b) * (255 - alpha) + 127) / 255).unwrap_or(a)
    };
    Color::rgb(mix(top.red, bottom.red), mix(top.green, bottom.green), mix(top.blue, bottom.blue))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(vars: &[(&str, &str)]) -> DetectInput {
        DetectInput {
            vars: vars.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
            windows_vt: false,
        }
    }

    #[test]
    fn detection_follows_the_documented_order() {
        assert_eq!(ColorDepth::detect(&input(&[])), ColorDepth::Ansi16);
        assert_eq!(ColorDepth::detect(&input(&[("TERM", "dumb")])), ColorDepth::None);
        assert_eq!(ColorDepth::detect(&input(&[("TERM", "xterm-256color")])), ColorDepth::Ansi256);
        assert_eq!(
            ColorDepth::detect(&input(&[("TERM", "xterm-256color"), ("COLORTERM", "truecolor")])),
            ColorDepth::TrueColor
        );
        assert_eq!(
            ColorDepth::detect(&input(&[("COLORTERM", "truecolor"), ("NO_COLOR", "1")])),
            ColorDepth::None,
            "NO_COLOR wins over what the terminal could do"
        );
        assert_eq!(
            ColorDepth::detect(&input(&[("NO_COLOR", ""), ("TERM", "xterm")])),
            ColorDepth::Ansi16,
            "an empty NO_COLOR is unset"
        );
        assert_eq!(ColorDepth::detect(&input(&[("WT_SESSION", "x")])), ColorDepth::TrueColor);
        assert_eq!(ColorDepth::detect(&input(&[("TERM", "xterm-kitty")])), ColorDepth::TrueColor);
        assert_eq!(
            ColorDepth::detect(&input(&[("TERM_PROGRAM", "Apple_Terminal"), ("TERM", "xterm")])),
            ColorDepth::Ansi256
        );
        assert_eq!(ColorDepth::detect(&input(&[("TERM", "linux")])), ColorDepth::Ansi16);
        assert_eq!(
            ColorDepth::detect(&input(&[
                ("RUSTNATIVE_TUI_COLORS", "256"),
                ("COLORTERM", "truecolor")
            ])),
            ColorDepth::Ansi256
        );
        let console = DetectInput { vars: Vec::new(), windows_vt: true };
        assert_eq!(ColorDepth::detect(&console), ColorDepth::TrueColor);
    }

    #[test]
    fn downsampling_finds_the_nearest_palette_colour() {
        assert_eq!(nearest_256((0, 0, 0)), 16);
        assert_eq!(nearest_256((255, 255, 255)), 231);
        assert_eq!(nearest_256((255, 0, 0)), 196);
        assert_eq!(nearest_256((128, 128, 128)), 244, "a grey takes the grey ramp");
        assert_eq!(nearest_16((250, 10, 10)), 9);
        assert_eq!(nearest_16((10, 10, 10)), 0);
        assert_eq!(nearest_16((250, 250, 250)), 15);
        assert_eq!(nearest_16((230, 230, 230)), 7);
        assert_eq!(ColorDepth::None.encode(Color::rgb(1, 2, 3)), None);
        assert_eq!(
            ColorDepth::TrueColor.encode(Color::rgb(1, 2, 3)),
            Some(TermColor::Rgb(1, 2, 3))
        );
        for index in 0..=255 {
            let rgb = palette_256(index);
            if index >= 16 {
                assert_eq!(palette_256(nearest_256(rgb)), rgb, "{index} maps to itself");
            }
        }
    }

    #[test]
    fn blending_is_exact_at_the_ends() {
        let white = Color::rgb(255, 255, 255);
        let black = Color::rgb(0, 0, 0);
        assert_eq!(blend(Color::rgba(0, 0, 0, 255), white), black);
        assert_eq!(blend(Color::rgba(0, 0, 0, 0), white), white);
        assert_eq!(blend(Color::rgba(0, 0, 0, 128), white), Color::rgb(127, 127, 127));
    }
}
