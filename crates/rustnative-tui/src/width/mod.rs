//! Unicode display width: how many cells text takes (`PLAN.md` Milestone 38,
//! "measurement is Unicode display width … never byte or character
//! counts").
//!
//! The unit is the **grapheme cluster** (UAX #29, through
//! `unicode-segmentation`), because that is what a terminal draws in one
//! place: `e` + a combining acute is one cell, a family emoji joined by
//! zero-width joiners is one wide glyph, a flag is two regional indicators
//! in one wide glyph. A cluster's width is its first visible scalar's:
//!
//! | Scalar | Cells |
//! |---|---|
//! | East Asian Wide or Fullwidth, emoji presentation | 2 |
//! | an emoji with text presentation, followed by VS16 (U+FE0F) | 2 |
//! | East Asian Ambiguous | 1, or 2 when [`Width::ambiguous_wide`] (CJK locales) |
//! | a nonspacing or enclosing mark, a format or control character, a Hangul medial vowel or final consonant | 0 |
//! | everything else | 1 |
//!
//! and a pair of regional indicators (a flag) is 2. The table is generated
//! from Unicode 17.0.0 by [`generate`] (an ignored test).
//!
//! ```
//! use rustnative_tui::Width;
//!
//! let width = Width::default();
//! assert_eq!(width.of("abc"), 3);
//! assert_eq!(width.of("日本"), 4);
//! assert_eq!(width.of("e\u{301}"), 1, "a combining mark adds nothing");
//! assert_eq!(width.of("👩‍👩‍👧"), 2, "one joined emoji");
//! assert_eq!(width.of("🇯🇵"), 2, "one flag");
//! assert_eq!(width.of("☺"), 1);
//! assert_eq!(width.of("☺\u{fe0f}"), 2, "emoji presentation selected");
//! ```

#[cfg(test)]
mod generate;
mod table;

use unicode_segmentation::UnicodeSegmentation;

/// A code point's width class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    /// One cell (not stored in the table).
    Narrow,
    /// No cells.
    Zero,
    /// Two cells.
    Wide,
    /// East Asian Ambiguous: one cell, two in a CJK context.
    Ambiguous,
    /// A pictograph with text presentation by default: one cell, two with
    /// VS16.
    TextEmoji,
}

fn class(scalar: char) -> Class {
    let code = u32::from(scalar);
    if code < 0x7F {
        return if code < 0x20 { Class::Zero } else { Class::Narrow };
    }
    table::RANGES
        .binary_search_by(|&(first, last, _)| {
            if last < code {
                std::cmp::Ordering::Less
            } else if first > code {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .map_or(Class::Narrow, |index| table::RANGES[index].2)
}

fn is_regional_indicator(scalar: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&scalar)
}

/// The width rules in force: whether East Asian Ambiguous characters take
/// two cells, as terminals in CJK locales draw them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Width {
    ambiguous_wide: bool,
}

impl Width {
    /// The rules for `locale` (a BCP 47 tag or POSIX locale name): ambiguous
    /// characters are wide under Chinese, Japanese, and Korean, as a CJK
    /// locale's `wcwidth` makes them, and narrow everywhere else.
    /// `RUSTNATIVE_TUI_AMBIGUOUS=wide|narrow` overrides it, for a terminal
    /// configured against its locale.
    #[must_use]
    pub fn for_locale(locale: &str) -> Self {
        match std::env::var("RUSTNATIVE_TUI_AMBIGUOUS").ok().as_deref() {
            Some("wide") => return Self { ambiguous_wide: true },
            Some("narrow") => return Self { ambiguous_wide: false },
            _ => {}
        }
        let language = locale.split(['-', '_', '.']).next().unwrap_or("").to_ascii_lowercase();
        Self { ambiguous_wide: matches!(language.as_str(), "zh" | "ja" | "ko") }
    }

    /// Rules with ambiguous characters `wide` or not.
    #[must_use]
    pub const fn with_ambiguous_wide(wide: bool) -> Self {
        Self { ambiguous_wide: wide }
    }

    /// Whether ambiguous characters take two cells.
    #[must_use]
    pub const fn ambiguous_wide(&self) -> bool {
        self.ambiguous_wide
    }

    /// The cells one grapheme cluster takes: 0, 1, or 2.
    #[must_use]
    pub fn of_grapheme(&self, grapheme: &str) -> usize {
        let mut scalars = grapheme.chars();
        let Some(first) = scalars.next() else { return 0 };
        if is_regional_indicator(first) {
            return if grapheme.chars().filter(|c| is_regional_indicator(*c)).count() >= 2 {
                2
            } else {
                1
            };
        }
        let presentation = grapheme.chars().find_map(|scalar| match scalar {
            '\u{FE0F}' => Some(2),
            '\u{FE0E}' => Some(1),
            _ => None,
        });
        // The first scalar that takes space sets the cluster's width.
        for scalar in std::iter::once(first).chain(scalars) {
            match class(scalar) {
                Class::Zero => {}
                Class::Narrow => return 1,
                Class::Wide => return if presentation == Some(1) { 1 } else { 2 },
                Class::Ambiguous => return if self.ambiguous_wide { 2 } else { 1 },
                Class::TextEmoji => return presentation.unwrap_or(1),
            }
        }
        0
    }

    /// The cells `text` takes on one line.
    #[must_use]
    pub fn of(&self, text: &str) -> usize {
        text.graphemes(true).map(|grapheme| self.of_grapheme(grapheme)).sum()
    }

    /// `text` cut to at most `columns` cells, whole clusters only, with `…`
    /// in the last cell when anything was cut. Returns the text and its
    /// width.
    #[must_use]
    pub fn truncate(&self, text: &str, columns: usize) -> (String, usize) {
        if self.of(text) <= columns {
            return (text.to_owned(), self.of(text));
        }
        if columns == 0 {
            return (String::new(), 0);
        }
        let mut out = String::new();
        let mut used = 0;
        for grapheme in text.graphemes(true) {
            let width = self.of_grapheme(grapheme);
            if used + width > columns - 1 {
                break;
            }
            out.push_str(grapheme);
            used += width;
        }
        out.push('…');
        (out, used + 1)
    }

    /// `text` broken into lines of at most `columns` cells: at line breaks
    /// first, then at spaces, then between wide characters (which CJK text
    /// breaks between), and only then inside a word, at a cluster boundary.
    /// Spaces where a line breaks are not carried to the next line.
    #[must_use]
    pub fn wrap(&self, text: &str, columns: usize) -> Vec<String> {
        let columns = columns.max(1);
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            let paragraph = paragraph.strip_suffix('\r').unwrap_or(paragraph);
            if paragraph.is_empty() {
                lines.push(String::new());
                continue;
            }
            let mut line = String::new();
            let mut used = 0;
            for word in split_breakable(paragraph, *self) {
                let width = self.of(&word);
                let is_space = word.chars().all(char::is_whitespace);
                if used + width <= columns {
                    line.push_str(&word);
                    used += width;
                    continue;
                }
                if is_space {
                    // A break here: the space is dropped.
                    lines.push(std::mem::take(&mut line));
                    used = 0;
                    continue;
                }
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line).trim_end().to_owned());
                    used = 0;
                }
                // A word longer than a line breaks between clusters.
                for grapheme in word.graphemes(true) {
                    let width = self.of_grapheme(grapheme);
                    if used + width > columns && !line.is_empty() {
                        lines.push(std::mem::take(&mut line));
                        used = 0;
                    }
                    line.push_str(grapheme);
                    used += width;
                }
            }
            lines.push(line.trim_end().to_owned());
        }
        lines
    }
}

/// Splits a paragraph into breakable pieces: runs of spaces, words, and
/// each wide character on its own.
fn split_breakable(paragraph: &str, width: Width) -> Vec<String> {
    let mut pieces: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_space = false;
    for grapheme in paragraph.graphemes(true) {
        let space = grapheme.chars().all(char::is_whitespace);
        let wide = width.of_grapheme(grapheme) == 2;
        if wide {
            if !current.is_empty() {
                pieces.push(std::mem::take(&mut current));
            }
            pieces.push(grapheme.to_owned());
            continue;
        }
        if !current.is_empty() && space != current_space {
            pieces.push(std::mem::take(&mut current));
        }
        current_space = space;
        current.push_str(grapheme);
    }
    if !current.is_empty() {
        pieces.push(current);
    }
    pieces
}

/// The grapheme clusters of `text`, in order.
pub fn graphemes(text: &str) -> impl Iterator<Item = &str> {
    text.graphemes(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_follow_the_unicode_classes() {
        let width = Width::default();
        for (text, cells) in [
            ("", 0),
            ("a", 1),
            ("\t", 0),
            ("中", 2),
            ("ｱ", 1),                // halfwidth katakana
            ("Ａ", 2),               // fullwidth A
            ("한", 2),               // a precomposed syllable
            ("\u{1100}\u{1161}", 2), // a syllable from jamo
            ("🙂", 2),
            ("👍🏽", 2), // a skin-tone modifier joins its base
            ("❤", 1),
            ("❤\u{fe0f}", 2),
            ("\u{200b}", 0), // zero-width space
            ("\u{00ad}", 1), // soft hyphen, as wcwidth counts it
            ("🇫", 1),        // a lone regional indicator
        ] {
            assert_eq!(width.of(text), cells, "{text:?}");
        }
    }

    #[test]
    fn ambiguous_characters_follow_the_locale() {
        assert_eq!(Width::default().of("±"), 1);
        assert_eq!(Width::with_ambiguous_wide(true).of("±"), 2);
        assert!(
            !Width::for_locale("en_US.UTF-8").ambiguous_wide()
                || std::env::var("RUSTNATIVE_TUI_AMBIGUOUS").is_ok()
        );
        assert!(
            Width::for_locale("ja-JP").ambiguous_wide()
                || std::env::var("RUSTNATIVE_TUI_AMBIGUOUS").is_ok()
        );
    }

    #[test]
    fn wrapping_breaks_at_spaces_then_between_wide_characters_then_inside_words() {
        let width = Width::default();
        assert_eq!(width.wrap("hello wide world", 10), ["hello wide", "world"]);
        assert_eq!(width.wrap("日本語の文章", 5), ["日本", "語の", "文章"]);
        assert_eq!(width.wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(width.wrap("one\ntwo", 10), ["one", "two"]);
        assert_eq!(width.wrap("", 10), [""]);
        for line in width.wrap("mixed 日本 text with a verylongword", 6) {
            assert!(width.of(&line) <= 6, "{line:?}");
        }
    }

    #[test]
    fn truncation_keeps_whole_clusters_and_marks_the_cut() {
        let width = Width::default();
        assert_eq!(width.truncate("short", 10), ("short".to_owned(), 5));
        assert_eq!(width.truncate("a longer text", 6), ("a lon…".to_owned(), 6));
        assert_eq!(width.truncate("日本語", 4), ("日…".to_owned(), 3));
        assert_eq!(width.truncate("abc", 0), (String::new(), 0));
    }

    #[test]
    fn the_table_is_sorted_and_disjoint() {
        for pair in table::RANGES.windows(2) {
            assert!(pair[0].0 <= pair[0].1 && pair[0].1 < pair[1].0, "{:?}", pair);
        }
    }
}
