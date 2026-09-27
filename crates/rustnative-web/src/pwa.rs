//! Offline applications and installation (Web milestone I).
//!
//! An application that declares a [`Pwa`] gets, on every page:
//!
//! - a service worker (`/_rn/sw.js`, versioned by the build): the runtime
//!   and client modules precached and served from the cache (they are
//!   immutable), pages network-first with their last copy and then an
//!   offline page when there is no network, and server calls made offline
//!   queued and delivered, once each, when the network is back — through
//!   Background Sync where the browser has it, and when the page sees it is
//!   online otherwise;
//! - an update flow: a new build's worker waits; the runtime announces it
//!   (`rn:update`), and applying it (`rn.applyUpdate()`) reloads the page
//!   with its islands' state kept;
//! - a web app manifest (`/manifest.webmanifest`), linked from the head,
//!   with the application's name, colours, and icons.
//!
//! ```
//! use rustnative_web::pwa::Pwa;
//!
//! let pwa = Pwa::new("Notes").short_name("Notes").theme_color("#1f6feb");
//! let manifest = pwa.manifest();
//! assert_eq!(manifest["name"], "Notes");
//! assert_eq!(manifest["display"], "standalone");
//! ```

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// An installable, offline-capable application (`[web.pwa]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pwa {
    /// The application's name.
    pub name: String,
    /// Its short name, under an installed icon.
    pub short_name: String,
    /// Where an installed application starts.
    pub start_url: String,
    /// How it is shown once installed (`standalone`, `minimal-ui`, …).
    pub display: String,
    /// The colour of the browser's chrome around it.
    pub theme_color: String,
    /// The colour of its splash screen.
    pub background_color: String,
    /// Its icons; empty: the framework draws plain ones in the theme colour.
    pub icons: Vec<Icon>,
    /// Further pages to cache when the worker installs.
    pub precache: Vec<String>,
}

/// An icon of the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Icon {
    /// Its address.
    pub src: String,
    /// Its sizes (`192x192`).
    pub sizes: String,
    /// Its media type.
    #[serde(rename = "type")]
    pub kind: String,
}

impl Default for Pwa {
    fn default() -> Self {
        Self {
            name: "Application".into(),
            short_name: String::new(),
            start_url: "/".into(),
            display: "standalone".into(),
            theme_color: "#1f6feb".into(),
            background_color: "#ffffff".into(),
            icons: Vec::new(),
            precache: Vec::new(),
        }
    }
}

impl Pwa {
    /// An application named `name`.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ..Self::default() }
    }

    /// Its short name.
    #[must_use]
    pub fn short_name(mut self, name: impl Into<String>) -> Self {
        self.short_name = name.into();
        self
    }

    /// Its theme colour (`#rrggbb`).
    #[must_use]
    pub fn theme_color(mut self, color: impl Into<String>) -> Self {
        self.theme_color = color.into();
        self
    }

    /// Where it starts.
    #[must_use]
    pub fn start_url(mut self, url: impl Into<String>) -> Self {
        self.start_url = url.into();
        self
    }

    /// Caches `url` too when the worker installs.
    #[must_use]
    pub fn precache(mut self, url: impl Into<String>) -> Self {
        self.precache.push(url.into());
        self
    }

    /// The icons, the framework's when none are given.
    #[must_use]
    pub fn icons(&self) -> Vec<Icon> {
        if !self.icons.is_empty() {
            return self.icons.clone();
        }
        [192, 512]
            .map(|size| Icon {
                src: format!("/_rn/icon-{size}.png"),
                sizes: format!("{size}x{size}"),
                kind: "image/png".into(),
            })
            .to_vec()
    }

    /// The web app manifest.
    #[must_use]
    pub fn manifest(&self) -> Value {
        let short = if self.short_name.is_empty() { &self.name } else { &self.short_name };
        json!({
            "name": self.name,
            "short_name": short,
            "id": self.start_url,
            "start_url": self.start_url,
            "scope": "/",
            "display": self.display,
            "theme_color": self.theme_color,
            "background_color": self.background_color,
            "icons": self.icons(),
        })
    }

    /// A plain icon `size` pixels square in the theme colour, as PNG.
    #[must_use]
    pub fn icon_png(&self, size: u32) -> Vec<u8> {
        let hex = self.theme_color.trim_start_matches('#');
        let channel = |index: usize| {
            hex.get(index..index + 2)
                .and_then(|text| u8::from_str_radix(text, 16).ok())
                .unwrap_or(0)
        };
        let pixel = [channel(0), channel(2), channel(4), 255];
        let pixels: Vec<u8> =
            pixel.iter().copied().cycle().take((size * size * 4) as usize).collect();
        rustnative_core::ImageData::rgba(size, size, pixels, false)
            .map(|image| crate::png::encode(&image))
            .unwrap_or_default()
    }

    /// The service worker: `sw.js` with this build's `version`, the assets
    /// to precache, and the offline page.
    #[must_use]
    pub fn service_worker(&self, version: &str, assets: &[String]) -> String {
        let mut precache: Vec<String> = assets.to_vec();
        precache.push(OFFLINE.to_owned());
        precache.extend(self.precache.iter().cloned());
        crate::runtime::SW_JS
            .replace("__RN_VERSION__", &version.replace(['"', '\\'], ""))
            .replace(
                "__RN_PRECACHE__",
                &serde_json::to_string(&precache).unwrap_or_else(|_| "[]".into()),
            )
            .replace("__RN_OFFLINE__", OFFLINE)
    }

    /// The page shown for an address never visited, with no network.
    #[must_use]
    pub fn offline_page(&self) -> String {
        let mut name = String::new();
        crate::html::escape_into(&mut name, &self.name);
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
             <title>{name}</title></head><body><main><h1>{name}</h1><p>You are offline, and this page has not been saved for offline use. \
             It will load when the network is back.</p></main></body></html>"
        )
    }

    /// The head markup: the manifest link and the theme colour.
    #[must_use]
    pub fn head(&self) -> String {
        let mut color = String::new();
        crate::html::escape_into(&mut color, &self.theme_color);
        format!(
            "<link rel=\"manifest\" href=\"/manifest.webmanifest\"><meta name=\"theme-color\" content=\"{color}\">"
        )
    }
}

/// Where the offline page is served.
pub const OFFLINE: &str = "/_rn/offline";

/// Where the service worker is served (its scope is the whole origin).
pub const SERVICE_WORKER: &str = "/_rn/sw.js";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_worker_carries_its_version_and_what_to_precache() {
        let pwa = Pwa::new("Notes").precache("/about");
        let script = pwa.service_worker("build-7", &["/_rn/rn.abc.js".to_owned()]);
        assert!(script.contains("const VERSION = \"build-7\";"));
        assert!(
            script.contains("const PRECACHE = [\"/_rn/rn.abc.js\",\"/_rn/offline\",\"/about\"];")
        );
        assert!(!script.contains("__RN_"));
    }

    #[test]
    fn the_framework_draws_icons_in_the_theme_colour() {
        let pwa = Pwa::new("Notes").theme_color("#ff0000");
        let png = pwa.icon_png(192);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(pwa.manifest()["icons"][1]["sizes"], "512x512");
    }
}
