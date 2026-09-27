//! Static export (Web milestone J, the client-side mode): every route
//! rendered once, with no request, to files a static host serves — the
//! pages, their stylesheets, the runtime and client modules, WebAssembly
//! modules, the offline files, a sitemap, `_headers` with the security
//! headers static hosts apply, and a report of what each route ships.
//!
//! ```no_run
//! use rustnative_core::{Component, Event, Node};
//! use rustnative_web::export::Site;
//! use rustnative_web::{Head, Page};
//!
//! struct Home;
//! impl Component for Home {
//!     type Props = ();
//!     type Message = ();
//!     fn new(_: ()) -> Self { Self }
//!     fn props(&self) -> &() { &() }
//!     fn set_props(&mut self, _: ()) {}
//!     fn view(&self) -> Node { Node::label("hello", "Hello") }
//!     fn update(&mut self, _: Event) {}
//! }
//!
//! fn main() -> std::process::ExitCode {
//!     let site = Site::new().page("/", || Page::new::<Home>(Head::new("Home", "The home page."), ()));
//!     // `rustnative build web --mode client` runs this with `--export <dir>`.
//!     rustnative_web::export::run(&site)
//! }
//! ```
//!
//! A static host sends one policy for every file, so there is no per-response
//! nonce: pages link their stylesheet as a file (named by its hash) instead
//! of inlining it, and the policy allows scripts and styles from the site
//! itself only.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use crate::page::{self, Page, PageContext, Strategy};
use crate::pwa::Pwa;

type Build = Arc<dyn Fn() -> Page + Send + Sync>;

/// A site to export: its routes and what they need.
#[derive(Default, Clone)]
pub struct Site {
    pages: Vec<(String, Build)>,
    pwa: Option<Pwa>,
    wasm: Vec<(String, Vec<u8>)>,
    origin: Option<String>,
    policy: Option<String>,
    fonts: Vec<crate::fonts::Font>,
}

impl std::fmt::Debug for Site {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Site")
            .field("pages", &self.pages.iter().map(|(path, _)| path).collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

/// What one route ships.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteReport {
    /// The route.
    pub path: String,
    /// The page itself, in bytes.
    pub html_bytes: usize,
    /// Its stylesheet.
    pub css_bytes: usize,
    /// The JavaScript it loads: the runtime and its islands' modules (none
    /// without an island).
    pub script_bytes: usize,
    /// The WebAssembly it loads.
    pub wasm_bytes: usize,
}

/// What an export wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Each route.
    pub routes: Vec<RouteReport>,
    /// Every file written, relative to the export's folder.
    pub files: Vec<String>,
}

impl Site {
    /// A site with no routes.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the route `path`, rendered by `page`.
    #[must_use]
    pub fn page(mut self, path: &str, page: impl Fn() -> Page + Send + Sync + 'static) -> Self {
        self.pages.push((path.to_owned(), Arc::new(page)));
        self
    }

    /// Makes the site installable and able to work offline.
    #[must_use]
    pub fn pwa(mut self, pwa: Pwa) -> Self {
        self.pwa = Some(pwa);
        self
    }

    /// Uses `font`, subset to the characters the site's pages use.
    #[must_use]
    pub fn font(mut self, font: crate::fonts::Font) -> Self {
        self.fonts.push(font);
        self
    }

    /// Serves WebAssembly module `name` for the site's subtrees.
    #[must_use]
    pub fn wasm_module(mut self, name: &str, bytes: Vec<u8>) -> Self {
        self.wasm.push((name.to_owned(), bytes));
        self
    }

    /// The site's public origin (`https://notes.example.com`): the export
    /// then writes `sitemap.xml`.
    #[must_use]
    pub fn origin(mut self, origin: &str) -> Self {
        self.origin = Some(origin.trim_end_matches('/').to_owned());
        self
    }

    /// The `Permissions-Policy` to send (default: every powerful feature
    /// closed).
    #[must_use]
    pub fn permissions_policy(mut self, policy: &str) -> Self {
        self.policy = Some(policy.to_owned());
        self
    }

    /// The routes.
    pub fn routes(&self) -> impl Iterator<Item = &str> {
        self.pages.iter().map(|(path, _)| path.as_str())
    }

    /// Renders route `path` as the export would (for a development server).
    #[must_use]
    pub fn render(&self, path: &str) -> Option<page::RenderedPage> {
        let (_, build) = self.pages.iter().find(|(candidate, _)| candidate == path)?;
        Some(page::render(build().strategy(Strategy::Static), &self.context()))
    }

    fn context(&self) -> PageContext {
        let mut cx = PageContext::new(None);
        cx.external_css = true;
        cx.pwa = self.pwa.clone().map(Arc::new);
        cx.images = Some(Arc::new(crate::image::ImageStore::new()));
        cx
    }

    /// Writes the site to `folder`.
    ///
    /// # Errors
    ///
    /// A file could not be written.
    pub fn export(&self, folder: &Path) -> std::io::Result<Report> {
        let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let mut routes = Vec::new();
        let mut wasm_pages = Vec::new();
        let mut cx = self.context();
        if !self.fonts.is_empty() {
            // The site's text: every character its pages set.
            let mut text = String::new();
            for (_, build) in &self.pages {
                text.push_str(&visible_text(
                    &page::render(build().strategy(Strategy::Static), &cx).html,
                ));
            }
            for font in &self.fonts {
                let face = font.face(&text, "/_rn/").map_err(std::io::Error::other)?;
                files.insert(face.url.clone(), face.bytes.clone());
                cx.fonts.push(Arc::new(face));
            }
        }
        let runtime = crate::runtime::runtime_url("/_rn/");
        let runtime_bytes = crate::runtime::RUNTIME_JS.len();
        for (path, build) in &self.pages {
            let rendered = page::render(build().strategy(Strategy::Static), &cx);
            let mut script_bytes = 0;
            if rendered.html.contains(&runtime) {
                script_bytes += runtime_bytes;
            }
            for module in &rendered.modules {
                script_bytes += module.js.len();
                files.insert(module.url("/_rn/"), module.js.as_bytes().to_vec());
            }
            let mut wasm_bytes = 0;
            for (name, bytes) in &self.wasm {
                if rendered.html.contains(&format!("/_rn/w/{name}.wasm")) {
                    wasm_bytes += bytes.len();
                }
            }
            if rendered.wasm {
                wasm_pages.push(path.clone());
            }
            let css_bytes = rendered.stylesheet.as_ref().map_or(0, |(_, css)| css.len());
            if let Some((url, css)) = rendered.stylesheet {
                files.insert(url, css.into_bytes());
            }
            routes.push(RouteReport {
                path: path.clone(),
                html_bytes: rendered.html.len(),
                css_bytes,
                script_bytes,
                wasm_bytes,
            });
            files.insert(page_file(path), rendered.html.into_bytes());
        }
        if let Some(images) = &cx.images {
            files.extend(images.files());
        }
        files.insert(runtime, crate::runtime::RUNTIME_JS.as_bytes().to_vec());
        files.insert(crate::runtime::worker_url("/_rn/"), crate::runtime::worker_js().into_bytes());
        for (name, bytes) in &self.wasm {
            files.insert(format!("/_rn/w/{name}.wasm"), bytes.clone());
        }
        if let Some(pwa) = &self.pwa {
            let assets: Vec<String> =
                files.keys().filter(|file| file.starts_with("/_rn/")).cloned().collect();
            let version = crate::hash::class_name("", &assets.join(","));
            files.insert(
                crate::pwa::SERVICE_WORKER.to_owned(),
                pwa.service_worker(&version, &assets).into_bytes(),
            );
            files.insert(
                format!("{}/index.html", crate::pwa::OFFLINE),
                pwa.offline_page().into_bytes(),
            );
            files.insert(
                "/manifest.webmanifest".to_owned(),
                pwa.manifest().to_string().into_bytes(),
            );
            for size in [192, 512] {
                files.insert(format!("/_rn/icon-{size}.png"), pwa.icon_png(size));
            }
        }
        if let Some(origin) = &self.origin {
            let sitemap = self
                .pages
                .iter()
                .fold(crate::head::Sitemap::new(origin.clone()), |map, (path, _)| {
                    map.page(path, None)
                });
            files.insert("/sitemap.xml".to_owned(), sitemap.xml().into_bytes());
        }
        files.insert("/_headers".to_owned(), self.headers(&wasm_pages).into_bytes());
        let report = Report { routes, files: files.keys().cloned().collect() };
        files.insert(
            "/_rn/report.json".to_owned(),
            serde_json::to_vec_pretty(&report).unwrap_or_default(),
        );
        for (file, bytes) in &files {
            let target = folder.join(file.trim_start_matches('/'));
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(target, bytes)?;
        }
        Ok(report)
    }

    /// `_headers` (the format Netlify and Cloudflare Pages read): the
    /// security headers for every file, immutable caching for hashed
    /// assets, and what the service worker and WebAssembly pages need.
    fn headers(&self, wasm_pages: &[String]) -> String {
        let policy = self.policy.clone().unwrap_or_else(|| {
            "accelerometer=(), ambient-light-sensor=(), bluetooth=(), camera=(), clipboard-read=(), display-capture=(), \
             geolocation=(), gyroscope=(), hid=(), magnetometer=(), microphone=(), midi=(), payment=(), serial=(), usb=(), \
             web-share=()"
                .to_owned()
        });
        let csp = |wasm: bool| {
            format!(
                "default-src 'self'; script-src 'self'{}; style-src 'self'; img-src 'self' data:; object-src 'none'; \
                 base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
                if wasm { " 'wasm-unsafe-eval'" } else { "" }
            )
        };
        let mut out = String::new();
        let _ = write!(
            out,
            "/*\n  Content-Security-Policy: {}\n  X-Content-Type-Options: nosniff\n  X-Frame-Options: DENY\n  \
             Referrer-Policy: strict-origin-when-cross-origin\n  Permissions-Policy: {policy}\n\n",
            csp(false)
        );
        for path in wasm_pages {
            let _ = write!(
                out,
                "{path}\n  ! Content-Security-Policy\n  Content-Security-Policy: {}\n\n",
                csp(true)
            );
        }
        out.push_str("/_rn/*\n  Cache-Control: public, max-age=31536000, immutable\n\n");
        out.push_str("/_rn/w/*\n  ! Cache-Control\n  Cache-Control: no-cache\n\n");
        if self.pwa.is_some() {
            out.push_str("/_rn/sw.js\n  ! Cache-Control\n  Cache-Control: no-cache\n  Service-Worker-Allowed: /\n\n");
            out.push_str("/manifest.webmanifest\n  Content-Type: application/manifest+json\n");
        }
        out
    }
}

/// The text of a document: what is outside its tags (and its scripts).
fn visible_text(html: &str) -> String {
    let mut text = String::new();
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        text.push_str(&rest[..open]);
        let after = &rest[open..];
        let skip = if after.starts_with("<script") {
            after.find("</script>").map(|end| end + 9)
        } else {
            after.find('>').map(|end| end + 1)
        };
        rest = &after[skip.unwrap_or(after.len())..];
    }
    text.push_str(rest);
    text
}

/// The file a route's page is written to: `/` is `index.html`, `/about` is
/// `about/index.html`.
fn page_file(path: &str) -> String {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() { "/index.html".to_owned() } else { format!("/{trimmed}/index.html") }
}

/// The folder `--export <folder>` names in this process's arguments.
#[must_use]
pub fn export_folder() -> Option<PathBuf> {
    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--export" {
            return arguments.next().map(PathBuf::from);
        }
    }
    None
}

/// A client-side application's entry point: with `--export <folder>`, writes
/// the site there (what `rustnative build web --mode client` runs) and
/// prints the report; otherwise says how to use it.
#[must_use]
pub fn run(site: &Site) -> std::process::ExitCode {
    let Some(folder) = export_folder() else {
        eprintln!("usage: --export <folder>  (write the site's static files)");
        return std::process::ExitCode::FAILURE;
    };
    match site.export(&folder) {
        Ok(report) => {
            println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("the export failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_become_folders() {
        assert_eq!(page_file("/"), "/index.html");
        assert_eq!(page_file("/notes/"), "/notes/index.html");
        assert_eq!(page_file("/a/b"), "/a/b/index.html");
    }
}
