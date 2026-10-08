//! The data layer's host contracts: the device's conditions for constrained
//! work, and image decoding for the image loader.

use rustnative_core::ImageData;
use rustnative_data::{Conditions, ImageDecoder};

use super::java;
use crate::jni_host::{Arg, Class, Ret};

/// The network (`ConnectivityManager`'s active network has internet) and
/// power (`BatteryManager.isCharging`).
#[derive(Debug, Clone, Copy, Default)]
pub struct AndroidConditions;

impl Conditions for AndroidConditions {
    fn network(&self) -> bool {
        java(Class::Services, "network", "()Z", &[]).is_ok_and(Ret::bool)
    }

    fn charging(&self) -> bool {
        java(Class::Services, "charging", "()Z", &[]).is_ok_and(Ret::bool)
    }
}

/// `BitmapFactory`: subsampled while decoding (memory stays near the
/// requested size, not the source's), then scaled to fit.
#[derive(Debug, Clone, Copy, Default)]
pub struct BitmapDecoder;

impl ImageDecoder for BitmapDecoder {
    fn decode(&self, bytes: &[u8], fit: Option<(u32, u32)>) -> Result<ImageData, String> {
        let (width, height) = fit.map_or((0, 0), |(width, height)| {
            (i32::try_from(width).unwrap_or(i32::MAX), i32::try_from(height).unwrap_or(i32::MAX))
        });
        let decoded = java(
            Class::Services,
            "decode",
            "([BII)[I",
            &[Arg::Bytes(bytes), Arg::Int(width), Arg::Int(height)],
        )
        .map_err(|error| error.to_string())?
        .ints();
        let [width, height, pixels @ ..] = decoded.as_slice() else {
            return Err("BitmapFactory decoded no image".to_owned());
        };
        let mut rgba = Vec::with_capacity(pixels.len() * 4);
        for pixel in pixels {
            // ARGB, unpremultiplied (`Bitmap.getPixels`).
            let [a, r, g, b] = pixel.to_be_bytes();
            rgba.extend_from_slice(&[r, g, b, a]);
        }
        let (width, height) = (
            u32::try_from(*width).map_err(|_| "a negative width")?,
            u32::try_from(*height).map_err(|_| "a negative height")?,
        );
        ImageData::rgba(width, height, rgba, false).map_err(|error| error.to_string())
    }
}
