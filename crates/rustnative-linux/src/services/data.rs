//! Linux answers to the data layer's host contracts (`PLAN.md` Milestone
//! 47): the device's power and network conditions for constrained
//! background work, and image decoding for the image loader.

use gtk::gdk_pixbuf;
use gtk::gio;
use gtk::prelude::*;
use rustnative_core::ImageData;
use rustnative_data::{Conditions, ImageDecoder, fitted};

/// The device's conditions: the network from GIO's network monitor
/// (NetworkManager's view where it runs), and external power from UPower.
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxConditions;

impl Conditions for LinuxConditions {
    fn network(&self) -> bool {
        gio::NetworkMonitor::default().is_network_available()
    }

    fn charging(&self) -> bool {
        // UPower's `OnBattery`; a machine without UPower (a desktop, a
        // container) is taken to be on external power, as Windows reports
        // a desktop.
        let reply =
            gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE).ok().and_then(|bus| {
                bus.call_sync(
                    Some("org.freedesktop.UPower"),
                    "/org/freedesktop/UPower",
                    "org.freedesktop.DBus.Properties",
                    "Get",
                    Some(&("org.freedesktop.UPower", "OnBattery").to_variant()),
                    None,
                    gio::DBusCallFlags::NO_AUTO_START,
                    1_000,
                    gio::Cancellable::NONE,
                )
                .ok()
            });
        let on_battery = reply
            .and_then(|reply| reply.child_value(0).as_variant())
            .and_then(|value| value.get::<bool>())
            .unwrap_or(false);
        !on_battery
    }
}

/// Decodes PNG, JPEG, GIF, BMP, TIFF, WebP, and whatever else gdk-pixbuf
/// has loaders for, scaling down while decoding.
#[derive(Debug, Clone, Copy, Default)]
pub struct PixbufDecoder;

impl ImageDecoder for PixbufDecoder {
    fn decode(&self, bytes: &[u8], fit: Option<(u32, u32)>) -> Result<ImageData, String> {
        let loader = gdk_pixbuf::PixbufLoader::new();
        // The loader decodes straight to the fitted size.
        loader.connect_size_prepared(move |loader, width, height| {
            let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
                return;
            };
            let (out_width, out_height) = fitted(width, height, fit);
            if (out_width, out_height) != (width, height) {
                loader.set_size(
                    i32::try_from(out_width).unwrap_or(1),
                    i32::try_from(out_height).unwrap_or(1),
                );
            }
        });
        loader.write(bytes).map_err(|error| error.to_string())?;
        loader.close().map_err(|error| error.to_string())?;
        let pixbuf = loader.pixbuf().ok_or("gdk-pixbuf decoded no image")?;
        let pixbuf = if pixbuf.has_alpha() {
            pixbuf
        } else {
            pixbuf.add_alpha(false, 0, 0, 0).map_err(|error| error.to_string())?
        };
        let width = u32::try_from(pixbuf.width()).map_err(|_| "a negative width")?;
        let height = usize::try_from(pixbuf.height()).map_err(|_| "a negative height")?;
        let stride = usize::try_from(pixbuf.rowstride()).map_err(|_| "a negative stride")?;
        let row = usize::try_from(width).map_err(|_| "too wide")? * 4;
        let raw = pixbuf.read_pixel_bytes();
        let mut pixels = Vec::with_capacity(row * height);
        // The last row is not padded to the stride.
        for line in raw.chunks(stride).take(height) {
            pixels.extend_from_slice(line.get(..row).ok_or("a short row")?);
        }
        let height = u32::try_from(height).map_err(|_| "too tall")?;
        // gdk-pixbuf's RGBA is unpremultiplied.
        ImageData::rgba(width, height, pixels, false).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4x2 BMP (24-bit, bottom-up), all red.
    fn bmp() -> Vec<u8> {
        let row = 4 * 3;
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
            out.extend_from_slice(&[0, 0, 255]);
        }
        out
    }

    #[test]
    fn pixbuf_decodes_and_downscales() {
        let image = PixbufDecoder.decode(&bmp(), Some((2, 2))).expect("decoded");
        assert_eq!((image.width(), image.height()), (2, 1));
        assert_eq!(&image.pixels()[..4], &[255, 0, 0, 255], "RGBA red");
        let whole = PixbufDecoder.decode(&bmp(), None).expect("decoded");
        assert_eq!((whole.width(), whole.height(), whole.pixels().len()), (4, 2, 32));
        assert!(PixbufDecoder.decode(b"not an image", None).is_err());
    }

    #[test]
    fn conditions_answer_without_failing() {
        let conditions = LinuxConditions;
        let _ = (conditions.network(), conditions.charging());
    }
}
