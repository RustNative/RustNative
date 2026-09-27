//! Responsive images (`C42-1`): an image node on a server-rendered page is
//! served as WebP files at the widths a screen needs, chosen by the browser
//! through `srcset` and `sizes`, with its intrinsic size reserved (no layout
//! shift), loaded lazily unless it is the page's first image, which is
//! fetched first instead (it is likely the largest content paint).
//!
//! ```
//! use rustnative_core::ImageData;
//! use rustnative_web::image::{ImageStore, variants};
//!
//! let image = ImageData::rgba(800, 400, vec![200; 800 * 400 * 4], false).unwrap();
//! let widths: Vec<u32> = variants(&image).iter().map(|variant| variant.width).collect();
//! assert_eq!(widths, [200, 400, 800]);
//! let store = ImageStore::new();
//! let (src, srcset) = store.add(&image);
//! assert!(src.ends_with("-800.webp") && srcset.contains(" 400w"));
//! assert!(store.get(&src).unwrap().starts_with(b"RIFF"));
//! ```

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::{Mutex, PoisonError};

use rustnative_core::ImageData;

/// An image this small is inlined as it always was: a request would cost
/// more than its bytes.
pub const INLINE_PIXELS: u32 = 64 * 64;

/// One width of an image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    /// Its width, in pixels.
    pub width: u32,
    /// Its height, in pixels.
    pub height: u32,
    /// The image at that width, as WebP (lossless).
    pub webp: Vec<u8>,
}

/// `image` at its own width and at half and a quarter of it (down to 160
/// pixels), each as WebP.
#[must_use]
pub fn variants(image: &ImageData) -> Vec<Variant> {
    let mut widths = vec![image.width()];
    let mut width = image.width() / 2;
    while width >= 160 {
        widths.push(width);
        width /= 2;
    }
    widths.sort_unstable();
    widths
        .into_iter()
        .filter_map(|width| {
            let resized = resize(image, width)?;
            Some(Variant { width, height: resized.height(), webp: webp(&resized)? })
        })
        .collect()
}

/// `image` scaled to `width` (box filter: each pixel the average of what it
/// covers), keeping its aspect ratio.
#[must_use]
pub fn resize(image: &ImageData, width: u32) -> Option<ImageData> {
    let (source_width, source_height) = (image.width(), image.height());
    if width == 0 || width > source_width {
        return None;
    }
    if width == source_width {
        return Some(image.clone());
    }
    let height = (u64::from(source_height) * u64::from(width) / u64::from(source_width)).max(1);
    let height = u32::try_from(height).ok()?;
    let pixels = image.pixels();
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let (top, bottom) = (
            y * source_height / height,
            ((y + 1) * source_height / height).max(y * source_height / height + 1),
        );
        for x in 0..width {
            let (left, right) = (
                x * source_width / width,
                ((x + 1) * source_width / width).max(x * source_width / width + 1),
            );
            let mut sum = [0u64; 4];
            let mut count = 0u64;
            for source_y in top..bottom.min(source_height) {
                for source_x in left..right.min(source_width) {
                    let at = ((source_y * source_width + source_x) * 4) as usize;
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += u64::from(pixels.get(at + channel).copied().unwrap_or(0));
                    }
                    count += 1;
                }
            }
            for total in sum {
                out.push(u8::try_from(total / count.max(1)).unwrap_or(u8::MAX));
            }
        }
    }
    ImageData::rgba(width, height, out, image.is_premultiplied()).ok()
}

/// `image` as a lossless WebP file (straight alpha).
#[must_use]
pub fn webp(image: &ImageData) -> Option<Vec<u8>> {
    let straight: Vec<u8> = if image.is_premultiplied() {
        image
            .pixels()
            .chunks(4)
            .flat_map(|pixel| {
                let alpha = pixel.get(3).copied().unwrap_or(255);
                let unmultiply = |channel: u8| {
                    if alpha == 0 {
                        0
                    } else {
                        u8::try_from(u16::from(channel) * 255 / u16::from(alpha)).unwrap_or(255)
                    }
                };
                [unmultiply(pixel[0]), unmultiply(pixel[1]), unmultiply(pixel[2]), alpha]
            })
            .collect()
    } else {
        image.pixels().to_vec()
    };
    let mut out = Vec::new();
    image_webp::WebPEncoder::new(&mut out)
        .encode(&straight, image.width(), image.height(), image_webp::ColorType::Rgba8)
        .ok()?;
    Some(out)
}

/// The image files a render made, by address, for the server (or an
/// export) to serve.
#[derive(Debug, Default)]
pub struct ImageStore {
    files: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl ImageStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores `image`'s variants (once per image: they are named by its
    /// contents' hash); its largest variant's address and its `srcset`.
    pub fn add(&self, image: &ImageData) -> (String, String) {
        let hash = crate::hash::class_name("", &crate::png::base64(image.pixels()));
        let mut files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        let largest = format!("/_rn/img/{hash}-{}.webp", image.width());
        let mut srcset = String::new();
        let widths: Vec<u32> = if files.contains_key(&largest) {
            files
                .keys()
                .filter_map(|path| {
                    path.strip_prefix(&format!("/_rn/img/{hash}-"))?
                        .strip_suffix(".webp")?
                        .parse()
                        .ok()
                })
                .collect()
        } else {
            let variants = variants(image);
            let widths = variants.iter().map(|variant| variant.width).collect();
            for variant in variants {
                files.insert(format!("/_rn/img/{hash}-{}.webp", variant.width), variant.webp);
            }
            widths
        };
        let mut widths = widths;
        widths.sort_unstable();
        for width in widths {
            if !srcset.is_empty() {
                srcset.push_str(", ");
            }
            let _ = write!(srcset, "/_rn/img/{hash}-{width}.webp {width}w");
        }
        (largest, srcset)
    }

    /// The file at `path`.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<Vec<u8>> {
        self.files.lock().unwrap_or_else(PoisonError::into_inner).get(path).cloned()
    }

    /// Every file, by address.
    #[must_use]
    pub fn files(&self) -> BTreeMap<String, Vec<u8>> {
        self.files.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}
