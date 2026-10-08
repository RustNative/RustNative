//! Pixels in the layout Android's bitmaps take.

/// An image's pixels in the layout `Bitmap.Config.ARGB_8888` copies from a
/// buffer: RGBA bytes, premultiplied by alpha.
pub(crate) fn premultiplied_rgba(image: &rustnative_core::ImageData) -> Vec<u8> {
    let pixels = image.pixels();
    if image.is_premultiplied() {
        return pixels.to_vec();
    }
    let mut out = Vec::with_capacity(pixels.len());
    for pixel in pixels.chunks_exact(4) {
        let alpha = u16::from(pixel[3]);
        for channel in &pixel[..3] {
            out.push(u8::try_from((u16::from(*channel) * alpha + 127) / 255).unwrap_or(u8::MAX));
        }
        out.push(pixel[3]);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn pixels_are_premultiplied_once() {
        let image =
            rustnative_core::ImageData::rgba(1, 2, vec![255, 0, 0, 128, 10, 20, 30, 255], false)
                .expect("an image");
        assert_eq!(super::premultiplied_rgba(&image), vec![128, 0, 0, 128, 10, 20, 30, 255]);
        let already =
            rustnative_core::ImageData::rgba(1, 1, vec![1, 2, 3, 4], true).expect("an image");
        assert_eq!(super::premultiplied_rgba(&already), vec![1, 2, 3, 4]);
    }
}
