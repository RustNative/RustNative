//! Pixels an application hands to a canvas.

use std::sync::Arc;

/// Why an [`ImageData`] could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageError {
    /// The pixel buffer is not `width * height * 4` bytes long.
    WrongLength {
        /// How many bytes the dimensions require.
        expected: usize,
        /// How many bytes were supplied.
        actual: usize,
    },
    /// A dimension is zero, or the image is too large to address.
    InvalidSize,
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongLength { expected, actual } => {
                write!(f, "image data is {actual} bytes; the dimensions need {expected}")
            }
            Self::InvalidSize => f.write_str("image dimensions are zero or too large"),
        }
    }
}

impl std::error::Error for ImageError {}

/// An RGBA8 bitmap: four bytes per pixel, rows top to bottom, no padding.
///
/// Cheap to clone — pixels are shared — so the same image can appear in
/// many draw lists, and a draw list that did not change compares equal
/// without comparing pixels twice.
///
/// # Example
///
/// ```
/// use rustnative_core::ImageData;
///
/// let red_pixel = ImageData::rgba(1, 1, vec![255, 0, 0, 255], false)?;
/// assert_eq!(red_pixel.width(), 1);
///
/// // A buffer that does not match its dimensions is refused, not truncated.
/// assert!(ImageData::rgba(2, 2, vec![0; 4], false).is_err());
/// # Ok::<(), rustnative_core::ImageError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(into = "ImageRepr", try_from = "ImageRepr")]
pub struct ImageData {
    width: u32,
    height: u32,
    pixels: Arc<[u8]>,
    premultiplied: bool,
}

/// The wire form of [`ImageData`], rebuilt through [`ImageData::rgba`] so
/// a transmitted image is checked like any other.
#[derive(serde::Serialize, serde::Deserialize)]
struct ImageRepr {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    premultiplied: bool,
}

impl From<ImageData> for ImageRepr {
    fn from(image: ImageData) -> Self {
        Self {
            width: image.width,
            height: image.height,
            pixels: image.pixels.to_vec(),
            premultiplied: image.premultiplied,
        }
    }
}

impl TryFrom<ImageRepr> for ImageData {
    type Error = ImageError;
    fn try_from(repr: ImageRepr) -> Result<Self, ImageError> {
        Self::rgba(repr.width, repr.height, repr.pixels, repr.premultiplied)
    }
}

impl ImageData {
    /// An image of `width * height` pixels from `pixels`.
    ///
    /// `premultiplied` says whether each color channel has already been
    /// multiplied by its pixel's alpha, which is what native compositors
    /// want; a backend premultiplies straight-alpha data itself.
    ///
    /// # Errors
    ///
    /// [`ImageError::InvalidSize`] for a zero dimension or one whose byte
    /// count overflows; [`ImageError::WrongLength`] when `pixels` is not
    /// exactly `width * height * 4` bytes.
    pub fn rgba(
        width: u32,
        height: u32,
        pixels: impl Into<Arc<[u8]>>,
        premultiplied: bool,
    ) -> Result<Self, ImageError> {
        let pixels = pixels.into();
        if width == 0 || height == 0 {
            return Err(ImageError::InvalidSize);
        }
        let expected = usize::try_from(width)
            .ok()
            .and_then(|width| width.checked_mul(usize::try_from(height).ok()?))
            .and_then(|count| count.checked_mul(4))
            .ok_or(ImageError::InvalidSize)?;
        if pixels.len() != expected {
            return Err(ImageError::WrongLength { expected, actual: pixels.len() });
        }
        Ok(Self { width, height, pixels, premultiplied })
    }

    /// Width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// The RGBA8 bytes.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Whether color channels are already multiplied by alpha.
    #[must_use]
    pub const fn is_premultiplied(&self) -> bool {
        self.premultiplied
    }

    /// The pixels premultiplied by alpha, in the order a native bitmap
    /// wants them: **BGRA**, which is the only 32-bit layout every Windows
    /// imaging path accepts.
    #[must_use]
    pub fn premultiplied_bgra(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pixels.len());
        for pixel in self.pixels.chunks_exact(4) {
            let (r, g, b, a) = (pixel[0], pixel[1], pixel[2], pixel[3]);
            let scale = |channel: u8| {
                if self.premultiplied {
                    channel
                } else {
                    // Rounded rather than truncated, so full-alpha pixels
                    // come out unchanged.
                    u8::try_from((u16::from(channel) * u16::from(a) + 127) / 255).unwrap_or(u8::MAX)
                }
            };
            out.extend_from_slice(&[scale(b), scale(g), scale(r), a]);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_alpha_is_premultiplied_and_reordered_for_native_bitmaps() {
        let image = ImageData::rgba(1, 1, vec![200, 100, 50, 128], false).unwrap();
        assert_eq!(image.premultiplied_bgra(), vec![25, 50, 100, 128]);
    }

    #[test]
    fn opaque_pixels_are_unchanged_by_premultiplication() {
        let image = ImageData::rgba(1, 1, vec![200, 100, 50, 255], false).unwrap();
        assert_eq!(image.premultiplied_bgra(), vec![50, 100, 200, 255]);
    }

    #[test]
    fn a_zero_sized_image_is_refused() {
        assert_eq!(ImageData::rgba(0, 4, Vec::new(), false), Err(ImageError::InvalidSize));
    }
}
