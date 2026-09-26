//! Text profiles (`PLAN.md` Milestone 48): what text a target can render,
//! declared rather than discovered.
//!
//! A desktop host shapes every script it has fonts for. A constrained
//! target (a microcontroller with a bitmap font, no shaper, no bidi) does
//! not, and the failure is silent: boxes, or letters in the wrong order.
//! A [`TextProfile`] states the limitation, so an application — or its
//! tests, against every string of its catalogs — finds unsupported text
//! before a person does.
//!
//! ```
//! use rustnative_core::{Script, TextProfile};
//!
//! let embedded = TextProfile::new([Script::Latin]);
//! assert!(embedded.check("Grüße, Zoë").is_ok());
//! let error = embedded.check("Привет").unwrap_err();
//! assert_eq!(error.script, Script::Cyrillic);
//! assert!(TextProfile::full().check("שלום مرحبا 你好").is_ok());
//! ```

use std::fmt;

/// A writing system, as far as a renderer's support is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Script {
    /// Latin, including its accented and extended letters.
    Latin,
    /// Greek.
    Greek,
    /// Cyrillic.
    Cyrillic,
    /// Arabic (right to left, joined: needs shaping and bidi).
    Arabic,
    /// Hebrew (right to left: needs bidi).
    Hebrew,
    /// Devanagari (needs shaping).
    Devanagari,
    /// Thai (needs line breaking by dictionary).
    Thai,
    /// Han ideographs.
    Han,
    /// Hiragana and Katakana.
    Kana,
    /// Hangul.
    Hangul,
    /// Emoji and pictographs.
    Emoji,
    /// Anything else.
    Other,
}

impl Script {
    /// The script `character` belongs to; punctuation, digits, and spaces
    /// are `None` (every profile renders them).
    #[must_use]
    pub fn of(character: char) -> Option<Self> {
        let code = u32::from(character);
        Some(match code {
            0x00..=0x40 | 0x5B..=0x60 | 0x7B..=0xBF | 0xD7 | 0xF7 | 0x2000..=0x206F => {
                return None;
            }
            0x41..=0x5A | 0x61..=0x7A | 0xC0..=0x24F | 0x1E00..=0x1EFF => Self::Latin,
            0x370..=0x3FF | 0x1F00..=0x1FFF => Self::Greek,
            0x400..=0x52F => Self::Cyrillic,
            0x590..=0x5FF | 0xFB1D..=0xFB4F => Self::Hebrew,
            0x600..=0x6FF | 0x750..=0x77F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => Self::Arabic,
            0x900..=0x97F => Self::Devanagari,
            0xE00..=0xE7F => Self::Thai,
            0x3040..=0x30FF => Self::Kana,
            0x3400..=0x4DBF | 0x4E00..=0x9FFF => Self::Han,
            0xAC00..=0xD7AF | 0x1100..=0x11FF => Self::Hangul,
            0x1F300..=0x1FAFF | 0x2600..=0x27BF => Self::Emoji,
            _ => Self::Other,
        })
    }
}

/// Which scripts a target renders correctly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextProfile {
    scripts: Option<Vec<Script>>,
}

/// Text a [`TextProfile`] cannot render: the first character outside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedText {
    /// The character.
    pub character: char,
    /// Its script.
    pub script: Script,
    /// Its byte offset in the text.
    pub offset: usize,
}

impl fmt::Display for UnsupportedText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:?} ({:?}) at byte {} is outside this target's text profile",
            self.character, self.script, self.offset
        )
    }
}

impl std::error::Error for UnsupportedText {}

impl TextProfile {
    /// Every script: a host with a full shaping and font stack.
    #[must_use]
    pub const fn full() -> Self {
        Self { scripts: None }
    }

    /// Only `scripts` (plus digits, punctuation, and spaces).
    #[must_use]
    pub fn new(scripts: impl IntoIterator<Item = Script>) -> Self {
        Self { scripts: Some(scripts.into_iter().collect()) }
    }

    /// Whether `script` is supported.
    #[must_use]
    pub fn supports(&self, script: Script) -> bool {
        self.scripts.as_ref().is_none_or(|scripts| scripts.contains(&script))
    }

    /// Checks that every character of `text` is renderable.
    ///
    /// # Errors
    ///
    /// The first character that is not.
    pub fn check(&self, text: &str) -> Result<(), UnsupportedText> {
        for (offset, character) in text.char_indices() {
            if let Some(script) = Script::of(character) {
                if !self.supports(script) {
                    return Err(UnsupportedText { character, script, offset });
                }
            }
        }
        Ok(())
    }
}

impl Default for TextProfile {
    fn default() -> Self {
        Self::full()
    }
}
