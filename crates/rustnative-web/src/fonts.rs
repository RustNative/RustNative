//! Web fonts (`C42-2`): a font subset to the characters the site's text
//! uses, preloaded, and a fallback face whose metrics are adjusted to the
//! font's, so the page does not shift when the font arrives.
//!
//! ```no_run
//! use rustnative_web::fonts::Font;
//!
//! let font = Font::new("Brand", 400, std::fs::read("Brand.ttf").unwrap()).unwrap();
//! let face = font.face("Hello, world", "/_rn/").unwrap();
//! assert!(face.css.contains("font-family:\"Brand\""));
//! assert!(face.css.contains("size-adjust:"));
//! ```

use std::fmt::Write as _;
use std::sync::Arc;

use skrifa::instance::{LocationRef, Size};
use skrifa::{FontRef, MetadataProvider, Tag};

/// A font the site uses.
#[derive(Clone)]
pub struct Font {
    family: String,
    weight: u16,
    bytes: Arc<[u8]>,
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Font")
            .field("family", &self.family)
            .field("weight", &self.weight)
            .finish_non_exhaustive()
    }
}

/// A font face ready for a page: its subset file and the CSS that uses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFace {
    /// The subset file's address.
    pub url: String,
    /// The subset font.
    pub bytes: Vec<u8>,
    /// The `@font-face` rules: the font, and its metric-adjusted fallback
    /// (`"{family} fallback"`, over the system's Arial).
    pub css: String,
}

/// The fallback every system has, Arial, and its average width (see
/// [`average_width`]) in ems.
const FALLBACK: (&str, f64, f64) = ("Arial", ARIAL_AVERAGE, 1.0);
const ARIAL_AVERAGE: f64 = 0.481_698_5;

/// A `cmap` table (format 4, one segment per character) for `characters`,
/// each with its glyph in the subset.
fn cmap(characters: &mut [(char, u16)]) -> Vec<u8> {
    characters.sort_unstable();
    let codes: Vec<(u16, u16)> = characters
        .iter()
        .filter_map(|(character, glyph)| Some((u16::try_from(u32::from(*character)).ok()?, *glyph)))
        .collect();
    let segments = u16::try_from(codes.len() + 1).unwrap_or(u16::MAX);
    let (search, selector) = binary_search_fields(segments);
    let mut table = Vec::new();
    let push = |table: &mut Vec<u8>, value: u16| table.extend_from_slice(&value.to_be_bytes());
    // The header: version 0, one subtable (Windows, Unicode BMP).
    for value in [0, 1, 3, 1] {
        push(&mut table, value);
    }
    table.extend_from_slice(&12u32.to_be_bytes());
    let length = 16 + usize::from(segments) * 8;
    for value in [
        4,
        u16::try_from(length).unwrap_or(u16::MAX),
        0,
        segments * 2,
        search * 2,
        selector,
        segments * 2 - search * 2,
    ] {
        push(&mut table, value);
    }
    for (code, _) in &codes {
        push(&mut table, *code);
    }
    push(&mut table, 0xFFFF);
    push(&mut table, 0);
    for (code, _) in &codes {
        push(&mut table, *code);
    }
    push(&mut table, 0xFFFF);
    for (code, glyph) in &codes {
        push(&mut table, glyph.wrapping_sub(*code));
    }
    push(&mut table, 1);
    for _ in 0..segments {
        push(&mut table, 0);
    }
    table
}

/// The largest power of two not above `count`, and its log: the fields an
/// OpenType binary search header carries.
fn binary_search_fields(count: u16) -> (u16, u16) {
    let (mut search, mut selector) = (1u16, 0u16);
    while search * 2 <= count {
        search *= 2;
        selector += 1;
    }
    (search, selector)
}

fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

/// `font` with `extra` tables added, its directory, padding, and checksums
/// rewritten.
fn with_tables(font: &[u8], extra: Vec<([u8; 4], Vec<u8>)>) -> Result<Vec<u8>, String> {
    let broken = || "the subset font is malformed".to_owned();
    let read16 = |at: usize| {
        font.get(at..at + 2)
            .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
            .ok_or_else(broken)
    };
    let read32 = |at: usize| {
        font.get(at..at + 4)
            .map(|bytes| u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            .ok_or_else(broken)
    };
    let flavor = read32(0)?;
    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    for index in 0..usize::from(read16(4)?) {
        let record = 12 + index * 16;
        let tag: [u8; 4] =
            font.get(record..record + 4).ok_or_else(broken)?.try_into().map_err(|_| broken())?;
        let offset = usize::try_from(read32(record + 8)?).map_err(|_| broken())?;
        let length = usize::try_from(read32(record + 12)?).map_err(|_| broken())?;
        tables.push((tag, font.get(offset..offset + length).ok_or_else(broken)?.to_vec()));
    }
    for (tag, data) in extra {
        tables.retain(|(existing, _)| *existing != tag);
        tables.push((tag, data));
    }
    tables.sort_by_key(|(tag, _)| *tag);
    let count = u16::try_from(tables.len()).map_err(|_| broken())?;
    let (search, selector) = binary_search_fields(count);
    let mut out = Vec::new();
    out.extend_from_slice(&flavor.to_be_bytes());
    for value in [count, search * 16, selector, count * 16 - search * 16] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    let mut offset = 12 + tables.len() * 16;
    let mut head_at = None;
    for (tag, data) in &mut tables {
        if tag == b"head" && data.len() >= 12 {
            data[8..12].copy_from_slice(&[0; 4]);
            head_at = Some(offset);
        }
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum(data).to_be_bytes());
        out.extend_from_slice(&u32::try_from(offset).map_err(|_| broken())?.to_be_bytes());
        out.extend_from_slice(&u32::try_from(data.len()).map_err(|_| broken())?.to_be_bytes());
        offset += data.len().next_multiple_of(4);
    }
    for (_, data) in &tables {
        out.extend_from_slice(data);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    if let Some(head) = head_at {
        let adjustment = 0xB1B0_AFBA_u32.wrapping_sub(checksum(&out));
        out[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
    }
    Ok(out)
}

/// The mean advance of the lower-case Latin letters and the space, in ems:
/// what the width of running text follows.
fn average_width(font: &FontRef<'_>) -> f64 {
    let units = f64::from(font.metrics(Size::unscaled(), LocationRef::default()).units_per_em);
    let charmap = font.charmap();
    let glyphs = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    let advances: Vec<f64> = "abcdefghijklmnopqrstuvwxyz "
        .chars()
        .filter_map(|character| charmap.map(character))
        .filter_map(|glyph| glyphs.advance_width(glyph))
        .map(|advance| f64::from(advance) / units)
        .collect();
    // At most 27 advances: the count is exact as a float.
    let count = f64::from(u8::try_from(advances.len()).unwrap_or(u8::MAX));
    if advances.is_empty() { 0.5 } else { advances.iter().sum::<f64>() / count }
}

impl Font {
    /// `bytes` (TrueType or OpenType) as family `family` at `weight`.
    ///
    /// # Errors
    ///
    /// It is not a font.
    pub fn new(
        family: impl Into<String>,
        weight: u16,
        bytes: impl Into<Arc<[u8]>>,
    ) -> Result<Self, String> {
        let bytes = bytes.into();
        FontRef::new(&bytes).map_err(|error| error.to_string())?;
        Ok(Self { family: family.into(), weight, bytes })
    }

    /// The font reduced to the glyphs `text` needs (and the space and
    /// replacement glyphs).
    ///
    /// # Errors
    ///
    /// The font could not be subset.
    pub fn subset(&self, text: &str) -> Result<Vec<u8>, String> {
        let font = FontRef::new(&self.bytes).map_err(|error| error.to_string())?;
        let charmap = font.charmap();
        let mut remapper = subsetter::GlyphRemapper::new();
        remapper.remap(0);
        let mut characters: Vec<(char, u16)> = Vec::new();
        for character in text.chars().chain([' ', '\u{FFFD}']) {
            if let Some(glyph) =
                charmap.map(character).and_then(|glyph| u16::try_from(glyph.to_u32()).ok())
            {
                let new = remapper.remap(glyph);
                if u32::from(character) <= 0xFFFF
                    && !characters.iter().any(|(known, _)| *known == character)
                {
                    characters.push((character, new));
                }
            }
        }
        let subset =
            subsetter::subset(&self.bytes, 0, &remapper).map_err(|error| format!("{error:?}"))?;
        // The subsetter writes fonts for documents that address glyphs
        // directly; a browser maps characters, and requires OS/2.
        let mut extra = vec![(*b"cmap", cmap(&mut characters))];
        if let Some(os2) = font.table_data(Tag::new(b"OS/2")) {
            extra.push((*b"OS/2", os2.as_bytes().to_vec()));
        }
        with_tables(&subset, extra)
    }

    /// The face for a site whose text is `text`, its file under `base`.
    ///
    /// # Errors
    ///
    /// The font could not be subset.
    pub fn face(&self, text: &str, base: &str) -> Result<FontFace, String> {
        let bytes = self.subset(text)?;
        let url =
            format!("{base}f/{}.ttf", crate::hash::class_name("", &crate::png::base64(&bytes)));
        let font = FontRef::new(&self.bytes).map_err(|error| error.to_string())?;
        let metrics = font.metrics(Size::unscaled(), LocationRef::default());
        let units = f64::from(metrics.units_per_em);
        let average = average_width(&font);
        let adjust = if FALLBACK.1 > 0.0 { average / FALLBACK.1 * FALLBACK.2 } else { 1.0 };
        let ascent = f64::from(metrics.ascent) / units / adjust * 100.0;
        let descent = f64::from(-metrics.descent) / units / adjust * 100.0;
        let gap = f64::from(metrics.leading) / units / adjust * 100.0;
        let family = self.family.replace(['"', '\\'], "");
        let mut css = String::new();
        let _ = write!(
            css,
            "@font-face{{font-family:\"{family}\";src:url({url}) format(\"truetype\");font-weight:{};font-display:swap}}\
             @font-face{{font-family:\"{family} fallback\";src:local(\"{}\");size-adjust:{:.2}%;ascent-override:{ascent:.2}%;\
             descent-override:{descent:.2}%;line-gap-override:{gap:.2}%}}",
            self.weight,
            FALLBACK.0,
            adjust * 100.0,
        );
        Ok(FontFace { url, bytes, css })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_font_is_subset_to_its_text_with_a_matched_fallback() {
        // The system's own Arial, where this machine has one.
        let Ok(bytes) = std::fs::read("C:/Windows/Fonts/arial.ttf") else { return };
        let font = Font::new("Brand", 400, bytes.clone()).unwrap();
        let face = font.face("Hello", "/_rn/").unwrap();
        assert!(face.bytes.len() * 10 < bytes.len(), "{} of {}", face.bytes.len(), bytes.len());
        let subset = FontRef::new(&face.bytes).unwrap();
        let glyphs = skrifa::raw::TableProvider::maxp(&subset).unwrap().num_glyphs();
        assert!(glyphs < 12, "{glyphs}");
        // Characters still find their glyphs, through the rebuilt cmap.
        let h = subset.charmap().map('H').expect("H is mapped");
        assert!(
            subset
                .glyph_metrics(Size::unscaled(), LocationRef::default())
                .advance_width(h)
                .unwrap()
                > 0.0
        );
        assert!(subset.charmap().map('Z').is_none());
        assert!(subset.table_data(Tag::new(b"OS/2")).is_some());
        // Arial over Arial needs no adjusting.
        assert!(face.css.contains("size-adjust:100.00%"), "{}", face.css);
        assert!(face.url.starts_with("/_rn/f/") && face.css.contains(&face.url));
    }
}
