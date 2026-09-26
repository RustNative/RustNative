//! Making an `.ico` out of what an application actually has.
//!
//! An application's icon is usually a PNG. Windows wants an `.ico`, which
//! since Windows Vista may hold PNG-compressed images directly — so an
//! `.ico` can be built by writing a six-byte header, a sixteen-byte
//! directory entry, and the PNG bytes unchanged. No image decoding, no
//! resizing, and therefore no image-processing dependency in a build
//! script.
//!
//! A file that is already an `.ico` is used as it is.

use std::path::Path;

/// Why an icon could not be prepared.
#[derive(Debug)]
pub enum IconError {
    /// The file could not be read.
    Unreadable(std::io::Error),
    /// The file is neither a PNG nor an ICO.
    UnknownFormat,
    /// The PNG's header does not say what size it is.
    Truncated,
    /// The PNG is larger than an icon directory can describe.
    TooLarge {
        /// Its width in pixels.
        width: u32,
        /// Its height in pixels.
        height: u32,
    },
}

impl std::fmt::Display for IconError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable(error) => write!(f, "could not read the icon: {error}"),
            Self::UnknownFormat => f.write_str("an icon must be a .png or a .ico file"),
            Self::Truncated => f.write_str("the PNG ends before its header does"),
            Self::TooLarge { width, height } => {
                write!(f, "a {width}x{height} PNG is too large for an icon (256 is the maximum)")
            }
        }
    }
}

impl std::error::Error for IconError {}

/// The eight bytes every PNG starts with.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Reads `path` and returns the bytes of an `.ico` holding it.
///
/// # Errors
///
/// [`IconError`] if the file cannot be read or is not a PNG or ICO.
pub fn to_ico(path: &Path) -> Result<Vec<u8>, IconError> {
    let bytes = std::fs::read(path).map_err(IconError::Unreadable)?;
    ico_from_bytes(&bytes)
}

/// The `.ico` form of `bytes`, which may already be one.
///
/// # Errors
///
/// As [`to_ico`].
pub fn ico_from_bytes(bytes: &[u8]) -> Result<Vec<u8>, IconError> {
    // An ICO starts with a zero reserved word and image type 1.
    if bytes.starts_with(&[0, 0, 1, 0]) {
        return Ok(bytes.to_vec());
    }
    if !bytes.starts_with(&PNG_SIGNATURE) {
        return Err(IconError::UnknownFormat);
    }
    let (width, height) = png_size(bytes)?;
    if width > 256 || height > 256 {
        return Err(IconError::TooLarge { width, height });
    }

    let mut ico = Vec::with_capacity(22 + bytes.len());
    // ICONDIR: reserved, type 1 (icon), one image.
    ico.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    // ICONDIRENTRY: 0 means 256 for both dimensions.
    ico.push(u8::try_from(width % 256).unwrap_or(0));
    ico.push(u8::try_from(height % 256).unwrap_or(0));
    // No colour palette, no reserved bits, one plane, 32 bits per pixel.
    ico.extend_from_slice(&[0, 0, 1, 0, 32, 0]);
    ico.extend_from_slice(&u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_le_bytes());
    // The image follows the header and this single entry.
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend_from_slice(bytes);
    Ok(ico)
}

/// A PNG's dimensions, from the `IHDR` chunk that must come first.
fn png_size(bytes: &[u8]) -> Result<(u32, u32), IconError> {
    let header = bytes.get(16..24).ok_or(IconError::Truncated)?;
    let read = |at: usize| -> u32 {
        u32::from_be_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]])
    };
    Ok((read(0), read(4)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16x16 PNG header followed by nothing in particular: enough for an
    /// icon directory, which never looks at the pixels.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes.extend_from_slice(&[0; 16]);
        bytes
    }

    #[test]
    fn a_png_becomes_a_single_entry_ico_that_points_at_it() {
        let source = png(32, 32);
        let ico = ico_from_bytes(&source).expect("a valid PNG");
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 1, 0], "one icon image");
        assert_eq!((ico[6], ico[7]), (32, 32), "its size is in the directory entry");
        let size = u32::from_le_bytes([ico[14], ico[15], ico[16], ico[17]]) as usize;
        let offset = u32::from_le_bytes([ico[18], ico[19], ico[20], ico[21]]) as usize;
        assert_eq!(offset, 22, "the image starts right after the directory");
        assert_eq!(&ico[offset..offset + size], source.as_slice(), "the PNG is embedded as it is");
    }

    #[test]
    fn a_256_pixel_icon_is_written_as_zero_which_is_how_ico_says_256() {
        let ico = ico_from_bytes(&png(256, 256)).expect("valid");
        assert_eq!((ico[6], ico[7]), (0, 0));
    }

    #[test]
    fn an_ico_is_passed_through_unchanged() {
        let source = vec![0, 0, 1, 0, 1, 0, 16, 16, 0, 0, 1, 0, 32, 0];
        assert_eq!(ico_from_bytes(&source).expect("an ICO"), source);
    }

    #[test]
    fn anything_else_is_refused() {
        assert!(matches!(ico_from_bytes(b"GIF89a"), Err(IconError::UnknownFormat)));
        assert!(matches!(ico_from_bytes(&PNG_SIGNATURE), Err(IconError::Truncated)));
        assert!(matches!(
            ico_from_bytes(&png(512, 512)),
            Err(IconError::TooLarge { width: 512, height: 512 })
        ));
    }
}
