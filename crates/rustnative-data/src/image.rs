//! Image loading (`PLAN.md` Milestone 47): fetch, decode, downscale, cache
//! in memory and on disk, and cancel when no component shows the image any
//! more.
//!
//! An image is a query (keyed by its URL and target size), so it inherits
//! the query layer's behaviour: requests for the same image are
//! deduplicated, a decoded image stays in memory for its retention time
//! after the last component showing it goes, and a load nobody waits for
//! any more is cancelled. On top of that, fetched bytes are kept on disk
//! so a later run decodes without fetching.
//!
//! Decoding is the host's: `rustnative_windows::WicDecoder` decodes PNG,
//! JPEG, GIF, BMP, and TIFF with the Windows Imaging Component and
//! downscales while decoding. [`PortableDecoder`] decodes binary PPM, for
//! tests and headless runs.

use std::fmt;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use rustnative_core::{ComponentContext, HttpRequest, HttpService, ImageData};

use crate::query::{Query, QueryClient, QueryError, QueryState};

/// Turns encoded bytes into pixels.
pub trait ImageDecoder: Send + Sync {
    /// Decodes `bytes`, scaled down (never up, keeping the aspect ratio) to
    /// fit `fit` when given.
    ///
    /// # Errors
    ///
    /// The bytes are not an image this decoder reads.
    fn decode(&self, bytes: &[u8], fit: Option<(u32, u32)>) -> Result<ImageData, String>;
}

/// The size `width`×`height` scaled down to fit `fit`, keeping its aspect
/// ratio.
#[must_use]
pub fn fitted(width: u32, height: u32, fit: Option<(u32, u32)>) -> (u32, u32) {
    let Some((max_width, max_height)) = fit else { return (width, height) };
    if width <= max_width && height <= max_height {
        return (width, height);
    }
    let scale = f64::min(
        f64::from(max_width) / f64::from(width),
        f64::from(max_height) / f64::from(height),
    );
    let scaled = |value: u32| {
        let scaled = (f64::from(value) * scale).round();
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "in range: `scale` is below 1 and `value` is a `u32`"
        )]
        let scaled = scaled as u32;
        scaled.max(1)
    };
    (scaled(width), scaled(height))
}

/// Decodes binary PPM (`P6`, 8-bit), downscaling by nearest neighbour.
#[derive(Debug, Clone, Copy, Default)]
pub struct PortableDecoder;

impl ImageDecoder for PortableDecoder {
    fn decode(&self, bytes: &[u8], fit: Option<(u32, u32)>) -> Result<ImageData, String> {
        let mut fields = Vec::new();
        let mut at = 0;
        while fields.len() < 4 {
            while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
                at += 1;
            }
            let start = at;
            while bytes.get(at).is_some_and(|byte| !byte.is_ascii_whitespace()) {
                at += 1;
            }
            if start == at {
                return Err("not a PPM image".into());
            }
            fields.push(String::from_utf8_lossy(&bytes[start..at]).into_owned());
        }
        at += 1; // the single whitespace byte before the pixels
        let number = |field: &str| field.parse::<u32>().map_err(|_| "a bad PPM header".to_owned());
        if fields[0] != "P6" || number(&fields[3])? != 255 {
            return Err("only 8-bit binary PPM is supported".into());
        }
        let (width, height) = (number(&fields[1])?, number(&fields[2])?);
        let pixels = bytes.get(at..).ok_or("a truncated PPM image")?;
        let index = |x: u32, y: u32| {
            usize::try_from(u64::from(y) * u64::from(width) + u64::from(x)).unwrap_or(usize::MAX)
        };
        if pixels.len() < index(0, height).saturating_mul(3) {
            return Err("a truncated PPM image".into());
        }
        let (out_width, out_height) = fitted(width, height, fit);
        let mut rgba = Vec::with_capacity(
            usize::try_from(u64::from(out_width) * u64::from(out_height) * 4).unwrap_or(0),
        );
        for y in 0..out_height {
            let source_y = u32::try_from(u64::from(y) * u64::from(height) / u64::from(out_height))
                .unwrap_or(0);
            for x in 0..out_width {
                let source_x =
                    u32::try_from(u64::from(x) * u64::from(width) / u64::from(out_width))
                        .unwrap_or(0);
                let at = index(source_x, source_y) * 3;
                rgba.extend_from_slice(&pixels[at..at + 3]);
                rgba.push(255);
            }
        }
        ImageData::rgba(out_width, out_height, rgba, false).map_err(|error| error.to_string())
    }
}

/// A stable 64-bit FNV-1a hash — the disk cache's file names, which must
/// be the same in every run and every build.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Loads images for components; see the [module documentation](self).
#[derive(Clone)]
pub struct ImageLoader {
    client: QueryClient,
    http: Arc<dyn HttpService>,
    decoder: Arc<dyn ImageDecoder>,
    disk: Option<PathBuf>,
    retain: Duration,
}

impl fmt::Debug for ImageLoader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImageLoader").field("disk", &self.disk).finish_non_exhaustive()
    }
}

impl PartialEq for ImageLoader {
    fn eq(&self, other: &Self) -> bool {
        self.client == other.client && self.disk == other.disk
    }
}

impl ImageLoader {
    /// A loader caching in `client`, fetching with `http`, and decoding with
    /// `decoder`. Decoded images stay in memory for a minute after the last
    /// component showing them goes.
    pub fn new(
        client: QueryClient,
        http: Arc<dyn HttpService>,
        decoder: Arc<dyn ImageDecoder>,
    ) -> Self {
        Self { client, http, decoder, disk: None, retain: Duration::from_secs(60) }
    }

    /// Keeps fetched bytes in `directory` too (on Windows, conventionally
    /// `%LOCALAPPDATA%\<app>\cache\images`).
    #[must_use]
    pub fn with_disk_cache(mut self, directory: impl Into<PathBuf>) -> Self {
        self.disk = Some(directory.into());
        self
    }

    /// How long a decoded image stays in memory once nothing shows it.
    // ponytail: memory is bounded by time, not bytes; add a byte budget with
    // least-recently-used eviction if image-heavy screens need one.
    #[must_use]
    pub fn retain_time(mut self, retain: Duration) -> Self {
        self.retain = retain;
        self
    }

    /// The image at `url`, scaled down to fit `fit`, for the component
    /// rendering with `context`.
    pub fn use_image<M: Send + 'static>(
        &self,
        context: &mut ComponentContext<'_, M>,
        url: &str,
        fit: Option<(u32, u32)>,
    ) -> QueryState<Rc<ImageData>> {
        let size = fit.map_or_else(|| "full".to_owned(), |(w, h)| format!("{w}x{h}"));
        let (http, decoder, disk, owned) =
            (Arc::clone(&self.http), Arc::clone(&self.decoder), self.disk.clone(), url.to_owned());
        let query = Query::new(["image", url, size.as_str()], move || {
            let (http, decoder, url) = (Arc::clone(&http), Arc::clone(&decoder), owned.clone());
            let file = disk.as_ref().map(|directory| directory.join(format!("{:016x}", fnv(&url))));
            async move {
                let cached = file.as_ref().and_then(|file| std::fs::read(file).ok());
                let bytes = if let Some(bytes) = cached {
                    bytes
                } else {
                    let response = http.execute(HttpRequest::get(url.clone())).await?;
                    if response.status() != 200 {
                        return Err(QueryError::fatal(format!(
                            "{url} answered {}",
                            response.status()
                        )));
                    }
                    let bytes = response.body_bytes().to_vec();
                    if let Some(file) = &file {
                        if let Some(directory) = file.parent() {
                            let _ = std::fs::create_dir_all(directory);
                        }
                        let _ = std::fs::write(file, &bytes);
                    }
                    bytes
                };
                decoder.decode(&bytes, fit).map_err(QueryError::fatal)
            }
        })
        .stale_time(Duration::MAX)
        .retain_time(self.retain);
        self.client.use_query(context, query)
    }
}
