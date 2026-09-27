//! Images as `data:` URIs: a PNG written without compression (stored
//! deflate blocks) and base64 — no dependency, and nothing to serve
//! separately. The server's content security policy allows `data:` images
//! (`img-src 'self' data:`); an application with large images serves them
//! as assets through the image pipeline instead (`crate::image`, `C42-1`).

use rustnative_core::ImageData;

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1_u32, 0_u32);
    for byte in bytes {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
    out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(&kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// `image` as a PNG file (straight alpha, uncompressed).
#[must_use]
pub fn encode(image: &ImageData) -> Vec<u8> {
    let (width, height) = (image.width(), image.height());
    let row = usize::try_from(width).unwrap_or(0) * 4;
    let mut raw = Vec::with_capacity((row + 1) * usize::try_from(height).unwrap_or(0));
    for line in image.pixels().chunks(row.max(1)) {
        raw.push(0); // filter: none
        if image.is_premultiplied() {
            for pixel in line.chunks(4) {
                let alpha = pixel.get(3).copied().unwrap_or(255);
                for channel in &pixel[..3.min(pixel.len())] {
                    let straight = if alpha == 0 {
                        0
                    } else {
                        (u16::from(*channel) * 255 / u16::from(alpha)).min(255)
                    };
                    raw.push(u8::try_from(straight).unwrap_or(255));
                }
                raw.push(alpha);
            }
        } else {
            raw.extend_from_slice(line);
        }
    }
    // zlib: a header, stored blocks of at most 65 535 bytes, the checksum.
    let mut zlib = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65_535).peekable();
    if blocks.peek().is_none() {
        zlib.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(block) = blocks.next() {
        zlib.push(u8::from(blocks.peek().is_none()));
        let length = u16::try_from(block.len()).unwrap_or(u16::MAX);
        zlib.extend_from_slice(&length.to_le_bytes());
        zlib.extend_from_slice(&(!length).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA, no interlace
    chunk(&mut png, *b"IHDR", &header);
    chunk(&mut png, *b"IDAT", &zlib);
    chunk(&mut png, *b"IEND", &[]);
    png
}

/// Standard base64 with padding.
#[must_use]
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let value = (u32::from(group[0]) << 16)
            | (u32::from(group.get(1).copied().unwrap_or(0)) << 8)
            | u32::from(group.get(2).copied().unwrap_or(0));
        for index in 0..4 {
            if index <= group.len() {
                let sextet = (value >> (18 - 6 * index)) & 0x3f;
                out.push(char::from(ALPHABET[usize::try_from(sextet).unwrap_or(0)]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `image` as a `data:image/png;base64,…` URI.
#[must_use]
pub fn data_uri(image: &ImageData) -> String {
    format!("data:image/png;base64,{}", base64(&encode(image)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_match_their_references() {
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }

    #[test]
    fn a_pixel_encodes_to_a_valid_file() {
        let image = ImageData::rgba(1, 1, vec![255, 0, 0, 255], false).expect("an image");
        let png = encode(&image);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert!(png.ends_with(&[0xae, 0x42, 0x60, 0x82]), "ends with IEND's checksum");
    }
}
