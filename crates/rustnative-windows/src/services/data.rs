//! Windows answers to the data layer's host contracts (`PLAN.md`
//! Milestone 47): the device's power and network conditions for
//! constrained background work, and image decoding for the image loader.

use rustnative_core::ImageData;
use rustnative_data::{Conditions, ImageDecoder, fitted};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICBitmapSource, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
    WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows_sys::Win32::Networking::WinInet::InternetGetConnectedState;
use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

/// The device's conditions, asked of Windows: external power from
/// `GetSystemPowerStatus`, and the network from `WinINet`'s connected state.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsConditions;

impl Conditions for WindowsConditions {
    fn network(&self) -> bool {
        let mut flags = 0;
        // SAFETY: WinINet writes the connection flags into `flags`.
        unsafe { InternetGetConnectedState(&raw mut flags, 0) != 0 }
    }

    fn charging(&self) -> bool {
        let mut status = SYSTEM_POWER_STATUS::default();
        // SAFETY: Windows fills the structure given.
        let ok = unsafe { GetSystemPowerStatus(&raw mut status) } != 0;
        // A desktop without a battery reports AC line status 1 as well.
        ok && status.ACLineStatus == 1
    }
}

/// Decodes PNG, JPEG, GIF, BMP, TIFF, and whatever else the Windows
/// Imaging Component has codecs for, scaling down while decoding.
#[derive(Debug, Clone, Copy, Default)]
pub struct WicDecoder;

impl ImageDecoder for WicDecoder {
    fn decode(&self, bytes: &[u8], fit: Option<(u32, u32)>) -> Result<ImageData, String> {
        // The decoder runs on an executor thread; WIC needs COM there. The
        // multithreaded apartment is joined once and kept: "already
        // initialized" (in either model) is fine, since WIC's factory is
        // free-threaded.
        // SAFETY: no reserved argument; a documented apartment constant.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let error = |error: windows::core::Error| error.message();
        // SAFETY: a documented CLSID and interface; no outer object.
        let factory: IWICImagingFactory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
                .map_err(error)?;
        // SAFETY: each call is on a live WIC object; `bytes` outlives the
        // stream, which is dropped with this function.
        unsafe {
            let stream = factory.CreateStream().map_err(error)?;
            stream.InitializeFromMemory(bytes).map_err(error)?;
            let decoder = factory
                .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
                .map_err(error)?;
            let frame = decoder.GetFrame(0).map_err(error)?;
            let (mut width, mut height) = (0, 0);
            frame.GetSize(&raw mut width, &raw mut height).map_err(error)?;
            let (out_width, out_height) = fitted(width, height, fit);
            let source: IWICBitmapSource = if (out_width, out_height) == (width, height) {
                frame.into()
            } else {
                let scaler = factory.CreateBitmapScaler().map_err(error)?;
                scaler
                    .Initialize(&frame, out_width, out_height, WICBitmapInterpolationModeFant)
                    .map_err(error)?;
                scaler.into()
            };
            let converter = factory.CreateFormatConverter().map_err(error)?;
            converter
                .Initialize(
                    &source,
                    &GUID_WICPixelFormat32bppRGBA,
                    WICBitmapDitherTypeNone,
                    None,
                    0.0,
                    WICBitmapPaletteTypeCustom,
                )
                .map_err(error)?;
            let stride = out_width.checked_mul(4).ok_or("too wide")?;
            let mut pixels = vec![
                0_u8;
                usize::try_from(u64::from(stride) * u64::from(out_height))
                    .map_err(|_| "too large")?
            ];
            converter.CopyPixels(std::ptr::null(), stride, &mut pixels).map_err(error)?;
            ImageData::rgba(out_width, out_height, pixels, false).map_err(|error| error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4x2 BMP (24-bit, bottom-up), which every Windows can decode.
    fn bmp() -> Vec<u8> {
        let row = 4 * 3; // already a multiple of four
        let size = 54 + row * 2;
        let mut out = Vec::new();
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&u32::try_from(size).unwrap_or(0).to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&54_u32.to_le_bytes());
        out.extend_from_slice(&40_u32.to_le_bytes());
        out.extend_from_slice(&4_i32.to_le_bytes());
        out.extend_from_slice(&2_i32.to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&24_u16.to_le_bytes());
        out.extend_from_slice(&[0; 24]);
        for _ in 0..8 {
            out.extend_from_slice(&[0, 0, 255]); // BGR: red
        }
        out
    }

    #[test]
    fn wic_decodes_and_downscales() {
        let image =
            WicDecoder.decode(&bmp(), Some((2, 2))).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!((image.width(), image.height()), (2, 1));
        assert_eq!(&image.pixels()[..4], &[255, 0, 0, 255], "RGBA red");
        assert!(WicDecoder.decode(b"not an image", None).is_err());
    }

    #[test]
    fn conditions_answer_without_failing() {
        let conditions = WindowsConditions;
        let _ = (conditions.network(), conditions.charging());
    }
}
