//! Pages on the server (Web milestone H): a handler returns a
//! [`rustnative_web::Page`], and the application renders it for the
//! request — on a thread of its own, with the request's content security
//! nonce and request-forgery token — whole or streamed, and serves the
//! runtime and the client modules its islands need.
//!
//! ```
//! use rustnative_core::{Component, Event, Node};
//! use rustnative_server::{ServerApp, get};
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
//! async fn home() -> Page {
//!     Page::new::<Home>(Head::new("Home", "The home page."), ())
//! }
//!
//! let app = ServerApp::new().route("/", get(home).public());
//! ```
//!
//! The framework's own paths are under `/_rn/`: the runtime
//! (`/_rn/rn.<hash>.js`) and each client module (`/_rn/m/<name>.<hash>.js`),
//! both cached forever (their names change when their contents do). In
//! development ([`crate::ServerApp::explain_pages`]), a page's address with
//! `?_rn_explain` answers what the page is made of instead of the page.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use bytes::Bytes;
use http::{HeaderValue, StatusCode, header};
use rustnative_core::Services;
use rustnative_web::page::{self, PageContext, Shell, Strategy};
use rustnative_web::{ClientModule, Page, RequestInfo};

use crate::request::RequestContext;
use crate::response::{IntoResponse, Response};

/// A page waiting in its response for the application to render it with
/// the request.
#[derive(Clone)]
pub(crate) struct PendingPage(Arc<Mutex<Option<Page>>>);

impl IntoResponse for Page {
    fn into_response(self) -> Response {
        let mut response = Response::new(Bytes::new());
        response.extensions_mut().insert(PendingPage(Arc::new(Mutex::new(Some(self)))));
        response
    }
}

/// The application's web assets: the client modules its pages have used,
/// the shells of partially prerendered routes, and the services pages
/// render with.
#[derive(Default)]
pub(crate) struct WebAssets {
    modules: Mutex<HashMap<String, &'static ClientModule>>,
    wasm: Mutex<HashMap<String, Bytes>>,
    pub(crate) images: Arc<rustnative_web::image::ImageStore>,
    fonts: Mutex<Vec<Arc<rustnative_web::fonts::FontFace>>>,
    shells: Mutex<HashMap<String, Arc<Shell>>>,
    pub(crate) services: Mutex<Services>,
    pub(crate) version: Mutex<Option<String>>,
    pub(crate) pwa: Mutex<Option<Arc<rustnative_web::pwa::Pwa>>>,
    pub(crate) explain: std::sync::atomic::AtomicBool,
    pub(crate) development: std::sync::atomic::AtomicBool,
}

impl WebAssets {
    fn remember(&self, modules: &[&'static ClientModule]) {
        let mut known = self.modules.lock().unwrap_or_else(PoisonError::into_inner);
        for module in modules {
            known.insert(format!("{}.{}.js", module.name, module.hash()), module);
        }
    }

    /// The build's version: the application's, or the runtime's own.
    fn version(&self) -> String {
        self.version.lock().unwrap_or_else(PoisonError::into_inner).clone().unwrap_or_else(|| {
            rustnative_web::hash::class_name("", rustnative_web::runtime::RUNTIME_JS)
        })
    }

    /// The offline application's own addresses: the manifest, the service
    /// worker, the offline page, and the framework's icons.
    fn pwa_asset(&self, path: &str) -> Option<Response> {
        use rustnative_web::pwa;
        let config = self.pwa.lock().unwrap_or_else(PoisonError::into_inner).clone()?;
        let (body, kind, cache): (Bytes, &'static str, &'static str) = match path {
            "/manifest.webmanifest" => (
                Bytes::from(config.manifest().to_string()),
                "application/manifest+json",
                "no-cache",
            ),
            pwa::SERVICE_WORKER => {
                let mut assets = vec![
                    rustnative_web::runtime::runtime_url("/_rn/"),
                    rustnative_web::runtime::worker_url("/_rn/"),
                    config.start_url.clone(),
                ];
                let modules = self.modules.lock().unwrap_or_else(PoisonError::into_inner);
                assets.extend(modules.keys().map(|name| format!("/_rn/m/{name}")));
                drop(modules);
                (
                    Bytes::from(config.service_worker(&self.version(), &assets)),
                    "text/javascript; charset=utf-8",
                    "no-cache",
                )
            }
            pwa::OFFLINE => {
                (Bytes::from(config.offline_page()), "text/html; charset=utf-8", "no-cache")
            }
            "/_rn/icon-192.png" => {
                (Bytes::from(config.icon_png(192)), "image/png", "public, max-age=86400")
            }
            "/_rn/icon-512.png" => {
                (Bytes::from(config.icon_png(512)), "image/png", "public, max-age=86400")
            }
            _ => return None,
        };
        let mut response = Response::new(body);
        let headers = response.headers_mut();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(kind));
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
        if path == pwa::SERVICE_WORKER {
            // Served below `/_rn/`, it controls the whole origin.
            headers.insert("service-worker-allowed", HeaderValue::from_static("/"));
        }
        Some(response)
    }

    /// A framework asset at `path` (below `/_rn/`), if there is one.
    pub(crate) fn asset(&self, path: &str) -> Option<Response> {
        if let Some(response) = self.pwa_asset(path) {
            return Some(response);
        }
        let rest = path.strip_prefix("/_rn/")?;
        if rest.starts_with("f/") {
            let fonts = self.fonts.lock().unwrap_or_else(PoisonError::into_inner);
            let face = fonts.iter().find(|face| face.url == path)?;
            let mut response = Response::new(Bytes::from(face.bytes.clone()));
            let headers = response.headers_mut();
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("font/ttf"));
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            );
            return Some(response);
        }
        if rest.starts_with("img/") {
            let mut response = Response::new(Bytes::from(self.images.get(path)?));
            let headers = response.headers_mut();
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/webp"));
            // Named by their contents' hash.
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            );
            return Some(response);
        }
        if let Some(name) = rest.strip_prefix("w/").and_then(|name| name.strip_suffix(".wasm")) {
            let bytes = self.wasm.lock().unwrap_or_else(PoisonError::into_inner).get(name)?.clone();
            let mut response = Response::new(bytes);
            let headers = response.headers_mut();
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/wasm"));
            // The module's address does not change with its contents:
            // revalidate it.
            headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
            return Some(response);
        }
        let body = if rest == rustnative_web::runtime::runtime_url("").as_str() {
            Bytes::from_static(rustnative_web::runtime::RUNTIME_JS.as_bytes())
        } else if rest == rustnative_web::runtime::worker_url("").as_str() {
            Bytes::from(rustnative_web::runtime::worker_js())
        } else {
            let name = rest.strip_prefix("m/")?;
            let development = self.development.load(std::sync::atomic::Ordering::Relaxed);
            let modules = self.modules.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(module) =
                name.strip_suffix(".map").and_then(|name| modules.get(name)).filter(|_| development)
            {
                let mut response = Response::new(Bytes::from(module.source_map()));
                response
                    .headers_mut()
                    .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
                return Some(response);
            }
            let module = modules.get(name)?;
            if development {
                Bytes::from(format!(
                    "{}
//# sourceMappingURL={name}.map
",
                    module.js
                ))
            } else {
                Bytes::from_static(module.js.as_bytes())
            }
        };
        let mut response = Response::new(body);
        let headers = response.headers_mut();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/javascript; charset=utf-8"),
        );
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
        Some(response)
    }
}

impl crate::ServerApp {
    /// The services pages render with (HTTP, storage, the data layer); the
    /// render adds the request and its island registry to them.
    #[must_use]
    pub fn page_services(self, services: Services) -> Self {
        *self.web.services.lock().unwrap_or_else(PoisonError::into_inner) = services;
        self
    }

    /// Serves the WebAssembly module `name` (built for
    /// `wasm32-unknown-unknown` with `rustnative_web::wasm_subtree!`) at
    /// `/_rn/w/{name}.wasm`, for the pages whose subtrees run it.
    #[must_use]
    pub fn wasm_module(self, name: &str, bytes: impl Into<Bytes>) -> Self {
        self.web
            .wasm
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(name.to_owned(), bytes.into());
        self
    }

    /// Uses `font` on every page, subset to printable ASCII and the
    /// characters of `text` (the application's other text: a language's
    /// letters, its catalogue's strings), preloaded, with a metric-adjusted
    /// fallback.
    ///
    /// # Errors
    ///
    /// The font could not be subset.
    pub fn font(self, font: &rustnative_web::fonts::Font, text: &str) -> Result<Self, String> {
        let ascii: String = (' '..='~').collect();
        let face = font.face(&format!("{ascii}{text}"), "/_rn/")?;
        self.web.fonts.lock().unwrap_or_else(PoisonError::into_inner).push(Arc::new(face));
        Ok(self)
    }

    /// Serves `T`'s client module from the start, rather than once a page
    /// that uses it has rendered. An instance that lives for one request —
    /// an edge module, a function scaled out — never rendered the page that
    /// asks for the module, so a serverless application declares each
    /// client component its pages use.
    #[must_use]
    pub fn client<T: rustnative_web::ClientLogic>(self) -> Self {
        self.web.remember(&[T::MODULE]);
        self
    }

    /// Makes the application installable and able to work offline
    /// (`rustnative_web::pwa`): every page links the manifest and registers
    /// the service worker.
    #[must_use]
    pub fn pwa(self, pwa: rustnative_web::pwa::Pwa) -> Self {
        *self.web.pwa.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(pwa));
        self
    }

    /// The build's version, which the browser runtime sends with each
    /// server call (`x-rn-fn-version`): a call from a page of another build
    /// is answered `409` with this version, and the runtime reloads the
    /// page rather than speak an old wire format.
    #[must_use]
    pub fn version(self, version: impl Into<String>) -> Self {
        *self.web.version.lock().unwrap_or_else(PoisonError::into_inner) = Some(version.into());
        self
    }

    /// Development mode: client modules link their source maps (served
    /// beside them), so the browser shows client logic as the Rust it was
    /// written in. Off in production, where the maps would only cost bytes.
    #[must_use]
    pub fn development(self) -> Self {
        self.web.development.store(true, std::sync::atomic::Ordering::Relaxed);
        self
    }

    /// Answers a page's address with `?_rn_explain` (for development) with
    /// what the page is made of — which of its boundaries are static, and
    /// how many of its components read the request — as JSON.
    #[must_use]
    pub fn explain_pages(self) -> Self {
        self.web.explain.store(true, std::sync::atomic::Ordering::Relaxed);
        self
    }
}

/// The request as a page render sees it.
pub(crate) fn request_info(context: &RequestContext, nonce: &str, csrf: &str) -> RequestInfo {
    // The headers a page may read: never the cookie or authorization.
    let headers = context
        .headers
        .iter()
        .filter(|(name, _)| {
            !matches!(name.as_str(), "cookie" | "authorization" | "proxy-authorization")
        })
        .filter_map(|(name, value)| {
            Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
        })
        .collect();
    let params = context
        .param_order
        .iter()
        .filter_map(|name| Some((name.clone(), context.params.get(name)?.to_owned())))
        .collect();
    RequestInfo {
        path: context.path.clone(),
        query: context.query.clone(),
        headers,
        params,
        csrf: csrf.to_owned(),
        nonce: nonce.to_owned(),
        limits: context.value::<rustnative_web::HostLimits>().cloned().unwrap_or_default(),
    }
}

/// Renders the page `response` carries, if it carries one.
pub(crate) async fn render_pending(
    web: &Arc<WebAssets>,
    prefix: &str,
    response: Response,
    request: RequestInfo,
) -> Response {
    let Some(pending) = response.extensions().get::<PendingPage>().cloned() else {
        return response;
    };
    let Some(page) = pending.0.lock().unwrap_or_else(PoisonError::into_inner).take() else {
        return response;
    };
    let services = web.services.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let mut cx = PageContext::new(Some(request.clone())).services(services);
    cx.assets = format!("{prefix}/_rn/");
    cx.base = prefix.to_owned();
    cx.version.clone_from(&web.version.lock().unwrap_or_else(PoisonError::into_inner));
    cx.pwa.clone_from(&web.pwa.lock().unwrap_or_else(PoisonError::into_inner));
    cx.images = Some(Arc::clone(&web.images));
    cx.dev = web.development.load(std::sync::atomic::Ordering::Relaxed)
        && std::env::var_os("RUSTNATIVE_DEV").is_some();
    cx.fonts.clone_from(&web.fonts.lock().unwrap_or_else(PoisonError::into_inner));
    let status = StatusCode::from_u16(page.response_status()).unwrap_or(StatusCode::OK);
    let web = Arc::clone(web);
    if page.render_strategy() == Strategy::Streamed {
        let shell = if page.is_partial() {
            let key = request.path.clone();
            let cached =
                web.shells.lock().unwrap_or_else(PoisonError::into_inner).get(&key).cloned();
            if let Some(shell) = cached {
                Some(shell)
            } else {
                let page = page.clone();
                let cx = cx.clone();
                let shell = blocking(move || page::prerender_shell(&page, &cx)).await;
                let Some(shell) = shell else {
                    return crate::ServerError::internal("the page's shell did not render")
                        .render(false);
                };
                let shell = Arc::new(shell);
                web.shells
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(key, Arc::clone(&shell));
                Some(shell)
            }
        } else {
            None
        };
        if !threaded() {
            // One invocation on one thread: the page is rendered whole, in
            // the order it would have streamed.
            let mut html = String::new();
            let rendered = page::render_streamed(page, &cx, shell.as_deref(), &mut |chunk| {
                html.push_str(&chunk);
            });
            web.remember(&rendered.modules);
            let mut response = Response::new(Bytes::from(html));
            *response.status_mut() = status;
            html_headers(&mut response);
            return response;
        }
        let mut head = Response::new(Bytes::new());
        *head.status_mut() = status;
        html_headers(&mut head);
        let (response, chunks) = crate::body::streamed(head);
        tokio::task::spawn_blocking(move || {
            let rendered = page::render_streamed(page, &cx, shell.as_deref(), &mut |chunk| {
                let _ = chunks.send(chunk);
            });
            web.remember(&rendered.modules);
        });
        return response;
    }
    let rendered = blocking(move || page::render(page, &cx)).await;
    let Some(rendered) = rendered else {
        return crate::ServerError::internal("the page did not render").render(false);
    };
    web.remember(&rendered.modules);
    let mut response = Response::new(Bytes::from(rendered.html));
    *response.status_mut() = status;
    html_headers(&mut response);
    if rendered.wasm {
        response.extensions_mut().insert(crate::security::WasmPage);
    }
    response
}

/// Whether rendering may leave the request's thread for the runtime's
/// blocking pool (so the runtime keeps driving the render's timers and
/// sockets); on `wasm32-wasip1` there are no other threads, and a render
/// runs where it is.
fn threaded() -> bool {
    !cfg!(target_family = "wasm") && tokio::runtime::Handle::try_current().is_ok()
}

/// Runs `work` off the runtime's threads when there are others, in place
/// when there are not; `None` when it panicked on another thread.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    if threaded() { tokio::task::spawn_blocking(work).await.ok() } else { Some(work()) }
}

fn html_headers(response: &mut Response) {
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
    // A page carries the request's nonce and token: never shared.
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
}

/// What the page `response` carries is made of, as JSON (development).
pub(crate) fn explain(
    web: &Arc<WebAssets>,
    prefix: &str,
    response: Response,
    request: RequestInfo,
) -> Response {
    let Some(page) = response
        .extensions()
        .get::<PendingPage>()
        .and_then(|pending| pending.0.lock().unwrap_or_else(PoisonError::into_inner).take())
    else {
        return response;
    };
    let services = web.services.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let mut cx = PageContext::new(Some(request)).services(services);
    cx.assets = format!("{prefix}/_rn/");
    crate::response::Json(page::explain(&page, &cx)).into_response()
}

/// Where the application should listen: `RUSTNATIVE_WEB_ADDR` when
/// `rustnative run web` or `rustnative dev web` started it, `default`
/// otherwise.
#[must_use]
pub fn address(default: &str) -> String {
    std::env::var("RUSTNATIVE_WEB_ADDR").unwrap_or_else(|_| default.to_owned())
}

/// The `[web.pwa]` table of an application's `rustnative.toml`, if it has
/// one.
///
/// # Errors
///
/// The document is not valid TOML, or the table is not a valid [`Pwa`].
///
/// [`Pwa`]: rustnative_web::pwa::Pwa
pub fn pwa_config(
    text: &str,
) -> Result<Option<rustnative_web::pwa::Pwa>, crate::config::ConfigError> {
    let document: toml::Value =
        toml::from_str(text).map_err(|error| crate::config::ConfigError(error.to_string()))?;
    let Some(table) = document.get("web").and_then(|web| web.get("pwa")) else { return Ok(None) };
    table
        .clone()
        .try_into()
        .map(Some)
        .map_err(|error| crate::config::ConfigError(error.to_string()))
}

impl crate::AppService {
    /// Replaces the build's version while the application runs — a deploy
    /// in place: pages of the old build are told to reload on their next
    /// server call, and offline applications get the new service worker.
    pub fn set_version(&self, version: impl Into<String>) {
        *self.0.app.web.version.lock().unwrap_or_else(PoisonError::into_inner) =
            Some(version.into());
    }
}
