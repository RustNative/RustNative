//! Colour.

/// RGBA color token used by a theme or explicit node style.
///
/// Kept as a plain public-field data carrier rather than an encapsulated
/// type: it has no invariant beyond "four bytes", and every consumer (theme
/// definitions, a backend's native colour conversion) needs direct field
/// access. See [`crate::geometry`]'s module doc for the same
/// reasoning applied to geometric types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Color {
    /// The red channel, 0-255.
    pub red: u8,
    /// The green channel, 0-255.
    pub green: u8,
    /// The blue channel, 0-255.
    pub blue: u8,
    /// The alpha (opacity) channel, 0-255; 255 is fully opaque.
    pub alpha: u8,
}

impl Color {
    /// Builds a fully-opaque color from its red/green/blue components.
    #[must_use]
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue, alpha: 255 }
    }

    /// Builds a color from its red/green/blue/alpha components.
    #[must_use]
    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self { red, green, blue, alpha }
    }
}

/// Builds an opaque color from a packed `0xRRGGBB` hex value, e.g.
/// `Color::from(0xFF00FF)` for magenta. Theme tokens defined this way stay
/// `const`-constructible, matching `Color::rgb`/`Color::rgba` above.
impl From<u32> for Color {
    fn from(packed: u32) -> Self {
        Self::rgb(
            ((packed >> 16) & 0xFF) as u8,
            ((packed >> 8) & 0xFF) as u8,
            (packed & 0xFF) as u8,
        )
    }
}
