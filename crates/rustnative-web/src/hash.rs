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

pub use rustnative_style::web::{class_name, cyrb53};

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
