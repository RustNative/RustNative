//! Colour values: parsing the vocabulary's colour syntax and converting it
//! to 8-bit sRGB at build time.
//!
//! **The gamut rule** (one rule, every backend): a colour outside sRGB —
//! the v4 palette is defined in `oklch()` and much of it is — is converted
//! to *linear* sRGB, each channel is clamped to `[0, 1]`, and the result is
//! gamma-encoded and rounded half away from zero to 8 bits. A backend with
//! a wider colour type still receives this value; a narrower one (a
//! terminal) reduces it further by its own documented rule.

use crate::model::{Color, round_to_i32};

/// A parsed colour: a literal, or a reference the theme resolves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedColor {
    /// A literal colour.
    Literal(Color),
    /// `var(--name)`.
    Token(String),
    /// `color-mix(in oklab, var(--name) N%, transparent)`.
    Faded(String, u8),
}

/// Parses a colour: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`,
/// `rgba()`, `hsl()`, `hsla()`, `oklch()`, `color-mix()`, `var(--…)`,
/// `transparent`, `black`, `white`.
///
/// # Errors
///
/// The text is not a colour in that syntax, with the reason.
pub fn parse_color(text: &str) -> Result<ParsedColor, String> {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    match lower.as_str() {
        "transparent" => return Ok(ParsedColor::Literal(Color::rgba(0, 0, 0, 0))),
        "black" => return Ok(ParsedColor::Literal(Color::rgb(0, 0, 0))),
        "white" => return Ok(ParsedColor::Literal(Color::rgb(255, 255, 255))),
        "currentcolor" | "inherit" | "initial" | "unset" => {
            return Err(format!(
                "`{text}` depends on the cascade, which the style model does not have; name a colour"
            ));
        }
        _ => {}
    }
    if let Some(hex) = lower.strip_prefix('#') {
        return parse_hex(hex).map(ParsedColor::Literal);
    }
    if let Some(name) = crate::value::var_name(text) {
        return Ok(ParsedColor::Token(name.to_owned()));
    }
    let Some((function, arguments)) = crate::value::function(text) else {
        return Err(format!("`{text}` is not a colour"));
    };
    match function.to_ascii_lowercase().as_str() {
        "rgb" | "rgba" => rgb_function(arguments).map(ParsedColor::Literal),
        "hsl" | "hsla" => hsl_function(arguments).map(ParsedColor::Literal),
        "oklch" => oklch_function(arguments).map(ParsedColor::Literal),
        "color-mix" => color_mix(arguments),
        other => Err(format!("`{other}()` is not a colour function the vocabulary knows")),
    }
}

fn parse_hex(hex: &str) -> Result<Color, String> {
    let digit = |index: usize| {
        hex.get(index..=index)
            .and_then(|digit| u8::from_str_radix(digit, 16).ok())
            .ok_or_else(|| format!("`#{hex}` is not a hex colour"))
    };
    let pair = |index: usize| Ok::<u8, String>(digit(index)? * 16 + digit(index + 1)?);
    match hex.len() {
        3 | 4 => {
            let alpha = if hex.len() == 4 { digit(3)? * 17 } else { 255 };
            Ok(Color::rgba(digit(0)? * 17, digit(1)? * 17, digit(2)? * 17, alpha))
        }
        6 | 8 => {
            let alpha = if hex.len() == 8 { pair(6)? } else { 255 };
            Ok(Color::rgba(pair(0)?, pair(2)?, pair(4)?, alpha))
        }
        _ => Err(format!("`#{hex}` is not a hex colour (3, 4, 6, or 8 digits)")),
    }
}

/// Splits a colour function's arguments — commas or spaces, with an
/// optional `/ alpha` — into the channel texts and the alpha text.
fn channels(arguments: &str) -> (Vec<String>, Option<String>) {
    let (body, alpha) = match arguments.split_once('/') {
        Some((body, alpha)) => (body, Some(alpha.trim().to_owned())),
        None => (arguments, None),
    };
    let mut parts: Vec<String> = body
        .split([',', ' '])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect();
    // The legacy comma syntax carries alpha as a fourth argument.
    let alpha = alpha.or_else(|| (parts.len() == 4).then(|| parts.remove(3)));
    (parts, alpha)
}

fn number(text: &str) -> Result<f64, String> {
    text.trim().parse::<f64>().map_err(|_| format!("`{text}` is not a number"))
}

/// A number or a percentage of `full`.
fn number_or_percent(text: &str, full: f64) -> Result<f64, String> {
    match text.strip_suffix('%') {
        Some(percent) => Ok(number(percent)? / 100.0 * full),
        None => number(text),
    }
}

fn alpha(text: Option<&str>) -> Result<f64, String> {
    text.map_or(Ok(1.0), |text| number_or_percent(text, 1.0)).map(|alpha| alpha.clamp(0.0, 1.0))
}

fn to_byte(unit: f64) -> u8 {
    u8::try_from(round_to_i32(unit.clamp(0.0, 1.0) * 255.0).clamp(0, 255)).unwrap_or(u8::MAX)
}

fn rgb_function(arguments: &str) -> Result<Color, String> {
    let (parts, alpha_text) = channels(arguments);
    let [red, green, blue] = parts.as_slice() else {
        return Err(format!("`rgb({arguments})` needs three channels"));
    };
    let channel = |text: &str| number_or_percent(text, 255.0).map(|value| value / 255.0);
    Ok(Color::rgba(
        to_byte(channel(red)?),
        to_byte(channel(green)?),
        to_byte(channel(blue)?),
        to_byte(alpha(alpha_text.as_deref())?),
    ))
}

fn hue(text: &str) -> Result<f64, String> {
    let text = text.trim();
    let degrees = text.strip_suffix("deg").map_or_else(|| number(text), number)?;
    Ok(degrees.rem_euclid(360.0))
}

#[allow(clippy::many_single_char_names, reason = "the conventional colour-science channel names")]
fn hsl_function(arguments: &str) -> Result<Color, String> {
    let (parts, alpha_text) = channels(arguments);
    let [h, s, l] = parts.as_slice() else {
        return Err(format!("`hsl({arguments})` needs three channels"));
    };
    let (h, s, l) =
        (hue(h)?, number_or_percent(s, 100.0)? / 100.0, number_or_percent(l, 100.0)? / 100.0);
    let chroma = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let segment = h / 60.0;
    let x = chroma * (1.0 - (segment.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match segment {
        s if s < 1.0 => (chroma, x, 0.0),
        s if s < 2.0 => (x, chroma, 0.0),
        s if s < 3.0 => (0.0, chroma, x),
        s if s < 4.0 => (0.0, x, chroma),
        s if s < 5.0 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = l - chroma / 2.0;
    Ok(Color::rgba(
        to_byte(r + m),
        to_byte(g + m),
        to_byte(b + m),
        to_byte(alpha(alpha_text.as_deref())?),
    ))
}

fn oklch_function(arguments: &str) -> Result<Color, String> {
    let (parts, alpha_text) = channels(arguments);
    let [l, c, h] = parts.as_slice() else {
        return Err(format!("`oklch({arguments})` needs three channels"));
    };
    let lightness = number_or_percent(l, 1.0)?;
    let chroma = number_or_percent(c, 0.4)?;
    let hue = hue(h)?.to_radians();
    let lab = [lightness, chroma * hue.cos(), chroma * hue.sin()];
    Ok(from_linear(oklab_to_linear_srgb(lab), alpha(alpha_text.as_deref())?))
}

/// Oklab to linear sRGB (Björn Ottosson's matrices, as CSS Color 4 uses).
#[must_use]
pub fn oklab_to_linear_srgb([l, a, b]: [f64; 3]) -> [f64; 3] {
    let l_ = l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
    let m_ = l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
    let s_ = l - 0.089_484_177_5 * a - 1.291_485_548_0 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    [
        4.076_741_662_1 * l3 - 3.307_711_591_3 * m3 + 0.230_969_929_2 * s3,
        -1.268_438_004_6 * l3 + 2.609_757_401_1 * m3 - 0.341_319_396_5 * s3,
        -0.004_196_086_3 * l3 - 0.703_418_614_7 * m3 + 1.707_614_701_0 * s3,
    ]
}

/// Linear sRGB to Oklab.
#[must_use]
#[allow(clippy::many_single_char_names, reason = "the conventional colour-science channel names")]
pub fn linear_srgb_to_oklab([r, g, b]: [f64; 3]) -> [f64; 3] {
    let l = 0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
    let m = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
    let s = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;
    let (l_, m_, s_) = (l.cbrt(), m.cbrt(), s.cbrt());
    [
        0.210_454_255_3 * l_ + 0.793_617_785_0 * m_ - 0.004_072_046_8 * s_,
        1.977_998_495_1 * l_ - 2.428_592_205_0 * m_ + 0.450_593_709_9 * s_,
        0.025_904_037_1 * l_ + 0.782_771_766_2 * m_ - 0.808_675_766_0 * s_,
    ]
}

fn decode(channel: u8) -> f64 {
    let value = f64::from(channel) / 255.0;
    if value <= 0.040_45 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
}

fn encode(linear: f64) -> f64 {
    if linear <= 0.003_130_8 { linear * 12.92 } else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 }
}

/// Linear sRGB (clamped per channel — the gamut rule) to an 8-bit colour.
#[must_use]
pub fn from_linear(linear: [f64; 3], alpha: f64) -> Color {
    let [r, g, b] = linear.map(|channel| to_byte(encode(channel.clamp(0.0, 1.0))));
    Color::rgba(r, g, b, to_byte(alpha))
}

/// An 8-bit colour as linear sRGB.
#[must_use]
pub fn to_linear(color: Color) -> [f64; 3] {
    [decode(color.red), decode(color.green), decode(color.blue)]
}

/// Splits `text` on commas outside parentheses.
pub(crate) fn split_top_level(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0_i32;
    let mut start = 0;
    for (index, character) in text.char_indices() {
        match character {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            c if c == separator && depth == 0 => {
                parts.push(&text[start..index]);
                start = index + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// `color-mix(in <space>, <colour> [<p>%], <colour> [<p>%])`, in `oklab`
/// or `srgb`, with premultiplied alpha as CSS Color 5 specifies.
fn color_mix(arguments: &str) -> Result<ParsedColor, String> {
    let parts = split_top_level(arguments, ',');
    let [space, first, second] = parts.as_slice() else {
        return Err(format!("`color-mix({arguments})` needs a space and two colours"));
    };
    let space =
        space.trim().strip_prefix("in ").map(str::trim).unwrap_or_default().to_ascii_lowercase();
    if space != "oklab" && space != "srgb" {
        return Err(format!("`color-mix` in `{space}`: the vocabulary mixes in `oklab` or `srgb`"));
    }
    let split = |part: &str| -> Result<(ParsedColor, Option<f64>), String> {
        let part = part.trim();
        match part.rsplit_once(' ') {
            Some((color, percent)) if percent.ends_with('%') && !color.trim().is_empty() => {
                Ok((parse_color(color)?, Some(number_or_percent(percent, 1.0)?)))
            }
            _ => Ok((parse_color(part)?, None)),
        }
    };
    let (first, p1) = split(first)?;
    let (second, p2) = split(second)?;
    let (p1, p2) = match (p1, p2) {
        (Some(p1), Some(p2)) => (p1, p2),
        (Some(p1), None) => (p1, 1.0 - p1),
        (None, Some(p2)) => (1.0 - p2, p2),
        (None, None) => (0.5, 0.5),
    };
    let transparent = ParsedColor::Literal(Color::rgba(0, 0, 0, 0));
    match (first, second) {
        (ParsedColor::Token(name), other) if other == transparent => {
            let percent = u8::try_from(round_to_i32(p1 * 100.0).clamp(0, 100)).unwrap_or(100);
            Ok(ParsedColor::Faded(name, percent))
        }
        (ParsedColor::Literal(a), ParsedColor::Literal(b)) => {
            Ok(ParsedColor::Literal(mix(a, b, p1, p2, space == "oklab")))
        }
        _ => Err("`color-mix` of a theme token is supported only as `color-mix(in oklab, var(--token) N%, \
                  transparent)` — the opacity modifier's form"
            .to_owned()),
    }
}

fn mix(a: Color, b: Color, p1: f64, p2: f64, oklab: bool) -> Color {
    let total = p1 + p2;
    let (p1, p2) = if total > 0.0 { (p1 / total, p2 / total) } else { (0.5, 0.5) };
    let (alpha_a, alpha_b) = (f64::from(a.alpha) / 255.0, f64::from(b.alpha) / 255.0);
    let alpha = alpha_a * p1 + alpha_b * p2;
    let space = |color: Color| {
        if oklab {
            linear_srgb_to_oklab(to_linear(color))
        } else {
            [color.red, color.green, color.blue].map(|channel| f64::from(channel) / 255.0)
        }
    };
    let (ca, cb) = (space(a), space(b));
    let mixed: [f64; 3] = std::array::from_fn(|index| {
        if alpha > 0.0 {
            (ca[index] * alpha_a * p1 + cb[index] * alpha_b * p2) / alpha
        } else {
            0.0
        }
    });
    if oklab {
        from_linear(oklab_to_linear_srgb(mixed), alpha)
    } else {
        let [r, g, bl] = mixed.map(to_byte);
        Color::rgba(r, g, bl, to_byte(alpha))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn literal(text: &str) -> Color {
        match parse_color(text) {
            Ok(ParsedColor::Literal(color)) => color,
            other => panic!("{text}: {other:?}"),
        }
    }

    #[test]
    fn hex_rgb_and_hsl_agree() {
        assert_eq!(literal("#1e90ff"), Color::rgb(0x1e, 0x90, 0xff));
        assert_eq!(literal("#abc"), Color::rgb(0xaa, 0xbb, 0xcc));
        assert_eq!(literal("rgb(30 144 255)"), Color::rgb(0x1e, 0x90, 0xff));
        assert_eq!(literal("rgba(30, 144, 255, 0.5)"), Color::rgba(0x1e, 0x90, 0xff, 128));
        assert_eq!(literal("rgb(0 0 0 / 0.1)"), Color::rgba(0, 0, 0, 26));
        assert_eq!(literal("hsl(0 100% 50%)"), Color::rgb(255, 0, 0));
        assert_eq!(literal("hsl(120deg, 100%, 25%)"), Color::rgb(0, 128, 0));
    }

    /// The sRGB primaries and white in Oklch (CSS Color 4's reference
    /// values) convert back exactly.
    #[test]
    fn oklch_reference_values_round_trip_to_srgb() {
        assert_eq!(literal("oklch(62.8% 0.2577 29.23)"), Color::rgb(255, 0, 0));
        assert_eq!(literal("oklch(86.64% 0.2948 142.5)"), Color::rgb(0, 255, 0));
        assert_eq!(literal("oklch(45.2% 0.3132 264.05)"), Color::rgb(0, 0, 255));
        assert_eq!(literal("oklch(100% 0 0)"), Color::rgb(255, 255, 255));
        assert_eq!(literal("oklch(0% 0 0)"), Color::rgb(0, 0, 0));
    }

    /// Palette values from the vendored v4 theme, against the sRGB values
    /// the same oklch coordinates give under the gamut rule (computed with
    /// the CSS Color 4 reference conversion and per-channel clipping).
    #[test]
    fn palette_colours_convert_under_the_gamut_rule() {
        assert_eq!(literal("oklch(62.3% 0.214 259.815)"), Color::rgb(0x2b, 0x7f, 0xff)); // blue-500
        assert_eq!(literal("oklch(63.7% 0.237 25.331)"), Color::rgb(0xfb, 0x2c, 0x36)); // red-500
        assert_eq!(literal("oklch(98.5% 0 0)"), Color::rgb(0xfa, 0xfa, 0xfa)); // neutral-50
    }

    #[test]
    fn color_mix_folds_literals_and_keeps_a_faded_token() {
        assert_eq!(literal("color-mix(in srgb, #ff0000 50%, #0000ff)"), Color::rgb(128, 0, 128));
        assert_eq!(literal("color-mix(in oklab, white 100%, black)"), Color::rgb(255, 255, 255));
        assert_eq!(
            parse_color("color-mix(in oklab, var(--color-blue-500) 50%, transparent)"),
            Ok(ParsedColor::Faded("color-blue-500".into(), 50))
        );
    }

    #[test]
    fn the_cascade_keywords_are_refused_with_a_reason() {
        let error = parse_color("currentColor").unwrap_err();
        assert!(error.contains("cascade"), "{error}");
    }
}
