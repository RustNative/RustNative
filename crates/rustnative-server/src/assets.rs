//! Single-artifact deployment: static assets compiled into the binary.
//!
//! The build script embeds a directory (`rustnative_build::embed_assets`),
//! naming each file by its content hash (`app.3f2a1b9c.css`) and computing
//! its subresource-integrity digest. The server serves them with an
//! immutable cache header — a changed file is a new name — and a page
//! links them with their integrity attribute, so a tampered copy is
//! refused by the browser (`C69`).

use bytes::Bytes;
use http::{HeaderValue, header};

use crate::response::{Html, Response};

/// One embedded file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asset {
    /// Its path in the assets directory (`app.css`).
    pub path: &'static str,
    /// Its content-hashed name (`app.3f2a1b9c.css`).
    pub hashed: &'static str,
    /// Its media type.
    pub content_type: &'static str,
    /// Its subresource-integrity digest (`sha256-…`).
    pub integrity: &'static str,
    /// Its bytes.
    pub bytes: &'static [u8],
}

/// The URL of `path` among `assets`.
#[must_use]
pub fn url(assets: &[Asset], path: &str) -> Option<String> {
    assets.iter().find(|asset| asset.path == path).map(|asset| format!("/assets/{}", asset.hashed))
}

/// A stylesheet link for `path`, with its integrity digest.
#[must_use]
pub fn stylesheet(assets: &[Asset], path: &str) -> Option<Html> {
    let asset = assets.iter().find(|asset| asset.path == path)?;
    Some(
        Html::trusted("<link rel=\"stylesheet\" href=\"/assets/")
            .text(asset.hashed)
            .and_trusted("\" integrity=\"")
            .text(asset.integrity)
            .and_trusted("\" crossorigin=\"anonymous\">"),
    )
}

/// A script for `path`, with its integrity digest and the response's CSP
/// nonce.
#[must_use]
pub fn script(assets: &[Asset], path: &str, nonce: &str) -> Option<Html> {
    let asset = assets.iter().find(|asset| asset.path == path)?;
    Some(
        Html::trusted("<script src=\"/assets/")
            .text(asset.hashed)
            .and_trusted("\" integrity=\"")
            .text(asset.integrity)
            .and_trusted("\" nonce=\"")
            .text(nonce)
            .and_trusted("\" crossorigin=\"anonymous\"></script>"),
    )
}

/// The response for an asset: its bytes, type, and an immutable cache
/// header.
#[must_use]
pub fn respond(asset: &Asset) -> Response {
    let mut response = http::Response::new(Bytes::from_static(asset.bytes));
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(asset.content_type));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    response
}

impl crate::ServerApp {
    /// Serves `assets` at `/assets/<hashed name>`.
    #[must_use]
    pub fn assets(mut self, assets: &'static [Asset]) -> Self {
        for asset in assets {
            self = self.resource(&format!("/assets/{}", asset.hashed), move || respond(asset));
        }
        self
    }
}
