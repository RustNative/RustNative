//! The one hash that names generated classes, on both sides of the wire.
//!
//! A class the server writes (`l1x2y3…` for a layout, `v…` for a visual
//! style) must be the class the browser's runtime computes for the same
//! node when it renders it itself, or a patched node would lose its style.
//! Both sides therefore hash the same canonical text with the same
//! function: `cyrb53` over the text's UTF-16 code units, which is cheap in
//! JavaScript (`Math.imul`, no `BigInt`) and exact in Rust (wrapping `u32`
//! arithmetic is `Math.imul`). The runtime's copy is `rn.hash` in
//! `runtime/rn.js`; `tests/runtime.rs` checks the two agree.

/// `cyrb53` of `text`'s UTF-16 code units: a 53-bit value, so it is exact
/// as a JavaScript number.
///
/// ```
/// use rustnative_web::hash::cyrb53;
///
/// assert_eq!(cyrb53(""), 3_338_908_027_751_811);
/// assert_ne!(cyrb53("a"), cyrb53("b"));
/// ```
#[must_use]
pub fn cyrb53(text: &str) -> u64 {
    let mut h1: u32 = 0xdead_beef;
    let mut h2: u32 = 0x41c6_ce57;
    for unit in text.encode_utf16() {
        let unit = u32::from(unit);
        h1 = (h1 ^ unit).wrapping_mul(2_654_435_761);
        h2 = (h2 ^ unit).wrapping_mul(1_597_334_677);
    }
    h1 = (h1 ^ (h1 >> 16)).wrapping_mul(2_246_822_507);
    h1 ^= (h2 ^ (h2 >> 13)).wrapping_mul(3_266_489_909);
    h2 = (h2 ^ (h2 >> 16)).wrapping_mul(2_246_822_507);
    h2 ^= (h1 ^ (h1 >> 13)).wrapping_mul(3_266_489_909);
    (u64::from(h2 & 0x001f_ffff) << 32) | u64::from(h1)
}

/// `prefix` followed by `cyrb53(text)` in base 36 — JavaScript's
/// `prefix + hash.toString(36)`.
///
/// ```
/// use rustnative_web::hash::class_name;
///
/// assert!(class_name("l", "width:10px").starts_with('l'));
/// assert_eq!(class_name("l", "x"), class_name("l", "x"));
/// ```
#[must_use]
pub fn class_name(prefix: &str, text: &str) -> String {
    let mut value = cyrb53(text);
    let mut digits = Vec::new();
    loop {
        let digit = u8::try_from(value % 36).unwrap_or(0);
        digits.push(if digit < 10 { b'0' + digit } else { b'a' + digit - 10 });
        value /= 36;
        if value == 0 {
            break;
        }
    }
    digits.reverse();
    let mut name = String::with_capacity(prefix.len() + digits.len());
    name.push_str(prefix);
    name.extend(digits.into_iter().map(char::from));
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_match_the_javascript_reference() {
        // Computed with the reference implementation in Node:
        // cyrb53("hello") and cyrb53("héllo 🚀") (UTF-16, so the emoji is
        // two code units).
        assert_eq!(cyrb53("hello"), 4_625_896_200_565_286);
        assert_eq!(cyrb53("h\u{e9}llo \u{1f680}"), 7_362_285_971_470_032);
    }

    #[test]
    fn a_class_name_is_base_36() {
        let name = class_name("v", "hello");
        assert!(name[1..].chars().all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()));
    }
}
