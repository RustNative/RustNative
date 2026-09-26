//! Visual regression: capturing a realized window and comparing it with a
//! reviewed image (Milestone 45).
//!
//! The capture goes through `PrintWindow`, which asks the window to render
//! itself into a memory DC — so the result does not depend on the window
//! being on top, on screen, or uncovered. Goldens are stored as 32-bit
//! bottom-up BMP files: a format every image viewer opens and this module
//! can read and write in forty lines, with no image dependency.
//!
//! Pixel output depends on the machine's fonts, DPI, and theme, so a golden
//! holds on the machine class it was blessed on; the comparison allows a
//! per-channel tolerance for antialiasing noise and a small fraction of
//! differing pixels. `BUILD_STATUS.md` records where the goldens were made.

use std::path::Path;

use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject,
};
use windows_sys::Win32::Storage::Xps::{PW_CLIENTONLY, PrintWindow};
use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;

/// A captured image: `width × height` pixels, BGRA, top row first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Capture {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) bgra: Vec<u8>,
}

/// `PW_RENDERFULLCONTENT`: include content drawn by `DirectComposition` and
/// layered windows. Not exported by name from `windows-sys`.
const PW_RENDERFULLCONTENT: u32 = 0x0000_0002;

/// Captures `hwnd`'s client area.
pub(crate) fn capture_client(hwnd: HWND) -> Option<Capture> {
    let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: `hwnd` is a live window owned by the calling thread's harness.
    if unsafe { GetClientRect(hwnd, &raw mut client) } == 0 {
        return None;
    }
    let width = client.right - client.left;
    let height = client.bottom - client.top;
    if width <= 0 || height <= 0 {
        return None;
    }
    // SAFETY: a screen DC, released below.
    let screen = unsafe { GetDC(std::ptr::null_mut()) };
    // SAFETY: `screen` is a valid DC; the memory DC and bitmap are deleted
    // below on every path.
    let memory = unsafe { CreateCompatibleDC(screen) };
    // SAFETY: as above.
    let bitmap = unsafe { CreateCompatibleBitmap(screen, width, height) };
    // SAFETY: selecting a bitmap into the memory DC it was made for.
    let previous = unsafe { SelectObject(memory, bitmap) };
    // SAFETY: `hwnd` is live and `memory` has a bitmap of its client size.
    let printed = unsafe { PrintWindow(hwnd, memory, PW_CLIENTONLY | PW_RENDERFULLCONTENT) } != 0;
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>()).unwrap_or(40),
            biWidth: width,
            // Negative: top-down rows.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [windows_sys::Win32::Graphics::Gdi::RGBQUAD {
            rgbBlue: 0,
            rgbGreen: 0,
            rgbRed: 0,
            rgbReserved: 0,
        }],
    };
    let (w, h) = (width.unsigned_abs(), height.unsigned_abs());
    let mut bgra = vec![0_u8; (w as usize) * (h as usize) * 4];
    // SAFETY: the bitmap is deselected first, as `GetDIBits` requires, and
    // `bgra` holds exactly `height` rows of `width` 32-bit pixels.
    let rows = unsafe {
        SelectObject(memory, previous);
        GetDIBits(memory, bitmap, 0, h, bgra.as_mut_ptr().cast(), &raw mut info, DIB_RGB_COLORS)
    };
    // SAFETY: releasing exactly what was created above.
    unsafe {
        DeleteObject(bitmap);
        DeleteDC(memory);
        ReleaseDC(std::ptr::null_mut(), screen);
    }
    if !printed || rows == 0 {
        return None;
    }
    for pixel in bgra.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    Some(Capture { width: w, height: h, bgra })
}

impl Capture {
    /// Whether every pixel is the same colour — what an unpainted window
    /// captures as.
    pub(crate) fn is_uniform(&self) -> bool {
        let mut pixels = self.bgra.chunks_exact(4);
        let first = pixels.next();
        pixels.all(|pixel| Some(pixel) == first)
    }

    /// Encodes as a 32-bit bottom-up BMP.
    pub(crate) fn to_bmp(&self) -> Vec<u8> {
        let pixels = self.bgra.len();
        let file_size = 54 + pixels;
        let mut out = Vec::with_capacity(file_size);
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&u32::try_from(file_size).unwrap_or(u32::MAX).to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&54_u32.to_le_bytes());
        out.extend_from_slice(&40_u32.to_le_bytes());
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&32_u16.to_le_bytes());
        out.extend_from_slice(&[0; 24]);
        let stride = self.width as usize * 4;
        for row in self.bgra.chunks_exact(stride).rev() {
            out.extend_from_slice(row);
        }
        out
    }

    /// Decodes what [`Self::to_bmp`] wrote.
    pub(crate) fn from_bmp(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 54 || &bytes[..2] != b"BM" {
            return None;
        }
        let read_u32 =
            |at: usize| bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        let offset = read_u32(10)? as usize;
        let width = read_u32(18)?;
        let height = read_u32(22)?;
        let stride = width as usize * 4;
        let data = bytes.get(offset..offset + stride * height as usize)?;
        let mut bgra = Vec::with_capacity(data.len());
        for row in data.chunks_exact(stride).rev() {
            bgra.extend_from_slice(row);
        }
        Some(Self { width, height, bgra })
    }

    /// The fraction of pixels differing from `other` by more than
    /// `tolerance` in any channel, or `None` if the sizes differ.
    pub(crate) fn difference(&self, other: &Self, tolerance: u8) -> Option<f64> {
        if (self.width, self.height) != (other.width, other.height) {
            return None;
        }
        let differing = self
            .bgra
            .chunks_exact(4)
            .zip(other.bgra.chunks_exact(4))
            .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > tolerance))
            .count();
        #[allow(clippy::cast_precision_loss, reason = "a ratio of pixel counts, for a threshold")]
        let ratio = differing as f64 / (self.bgra.len() / 4).max(1) as f64;
        Some(ratio)
    }
}

/// Compares `actual` with the golden BMP at `path`, blessing it instead when
/// `RUSTNATIVE_BLESS=1`. Panics — this is an assertion — when the sizes
/// differ or more than 1% of pixels differ by more than 12 per channel.
pub(crate) fn assert_matches_golden(path: &Path, actual: &Capture) {
    if std::env::var("RUSTNATIVE_BLESS").is_ok_and(|value| value == "1") {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create the golden directory");
        }
        std::fs::write(path, actual.to_bmp()).expect("write the golden image");
        return;
    }
    let expected =
        std::fs::read(path).ok().and_then(|bytes| Capture::from_bmp(&bytes)).unwrap_or_else(|| {
            panic!("golden {} is missing; bless it with RUSTNATIVE_BLESS=1", path.display())
        });
    let difference = actual.difference(&expected, 12).unwrap_or_else(|| {
        panic!(
            "golden {} is {}x{}, the capture is {}x{}",
            path.display(),
            expected.width,
            expected.height,
            actual.width,
            actual.height
        )
    });
    if difference > 0.01 {
        let failed = path.with_extension("actual.bmp");
        let _ = std::fs::write(&failed, actual.to_bmp());
        panic!(
            "{:.2}% of pixels differ from golden {}; the capture was written to {}",
            difference * 100.0,
            path.display(),
            failed.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bmp_round_trips() {
        let capture = Capture { width: 2, height: 2, bgra: (0..16).collect() };
        assert_eq!(Capture::from_bmp(&capture.to_bmp()), Some(capture));
    }

    #[test]
    fn difference_counts_only_pixels_beyond_the_tolerance() {
        let a = Capture { width: 2, height: 1, bgra: vec![10, 10, 10, 255, 10, 10, 10, 255] };
        let b = Capture { width: 2, height: 1, bgra: vec![12, 10, 10, 255, 90, 10, 10, 255] };
        assert_eq!(a.difference(&b, 4), Some(0.5));
    }
}
