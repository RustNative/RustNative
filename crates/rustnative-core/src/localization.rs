//! Pseudo-localization (`PLAN.md` Milestones 41 and 46): strings made to
//! look translated — longer, accented, and bracketed — so a layout can be
//! checked for the growth, diacritics, and truncation a real translation
//! brings, before there is one.
//!
//! ```
//! use rustnative_core::localization::pseudo_localize;
//!
//! let pseudo = pseudo_localize("Save changes");
//! assert!(pseudo.starts_with('[') && pseudo.ends_with(']'));
//! assert!(pseudo.chars().count() >= "Save changes".chars().count() * 14 / 10);
//! assert!(pseudo.contains("Ŝàṽé"));
//! ```

/// The accented form of an ASCII letter, or the character itself.
fn accented(character: char) -> char {
    match character {
        'a' => 'à',
        'b' => 'ƀ',
        'c' => 'ç',
        'd' => 'ð',
        'e' => 'é',
        'f' => 'ƒ',
        'g' => 'ĝ',
        'h' => 'ĥ',
        'i' => 'î',
        'j' => 'ĵ',
        'k' => 'ķ',
        'l' => 'ļ',
        'm' => 'ɱ',
        'n' => 'ñ',
        'o' => 'ö',
        'p' => 'þ',
        'q' => 'ǫ',
        'r' => 'ŕ',
        's' => 'š',
        't' => 'ţ',
        'u' => 'û',
        'v' => 'ṽ',
        'w' => 'ŵ',
        'x' => 'ẋ',
        'y' => 'ý',
        'z' => 'ž',
        'A' => 'Å',
        'B' => 'Ɓ',
        'C' => 'Ç',
        'D' => 'Ð',
        'E' => 'É',
        'F' => 'Ƒ',
        'G' => 'Ĝ',
        'H' => 'Ĥ',
        'I' => 'Î',
        'J' => 'Ĵ',
        'K' => 'Ķ',
        'L' => 'Ļ',
        'M' => 'Ṁ',
        'N' => 'Ñ',
        'O' => 'Ö',
        'P' => 'Þ',
        'Q' => 'Ǫ',
        'R' => 'Ŕ',
        'S' => 'Ŝ',
        'T' => 'Ţ',
        'U' => 'Û',
        'V' => 'Ṽ',
        'W' => 'Ŵ',
        'X' => 'Ẋ',
        'Y' => 'Ý',
        'Z' => 'Ž',
        other => other,
    }
}

/// `text` pseudo-localized: every ASCII letter accented, grown by 40 %
/// (at least two characters) with `~` padding, and bracketed, so a
/// truncated or concatenated string is visible at a glance.
#[must_use]
pub fn pseudo_localize(text: &str) -> String {
    let length = text.chars().count();
    let growth = (length * 2).div_ceil(5).max(2);
    let mut out = String::with_capacity(text.len() * 2 + growth + 2);
    out.push('[');
    out.extend(text.chars().map(accented));
    out.push(' ');
    out.extend(std::iter::repeat_n('~', growth.saturating_sub(1)));
    out.push(']');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pseudo_localization_grows_accents_and_brackets() {
        assert_eq!(pseudo_localize("Hi"), "[Ĥî ~]");
        let long = pseudo_localize("Preferences");
        assert_eq!(long.chars().count(), 1 + 11 + 5 + 1);
        assert_eq!(pseudo_localize(""), "[ ~]");
    }
}
