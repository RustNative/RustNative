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
    shells: Mutex<HashMap<String, Arc<Shell>>>,
    pub(crate) services: Mutex<Services>,
    pub(crate) version: Mutex<Option<String>>,
    pub(crate) explain: std::sync::atomic::AtomicBool,
}

impl WebAssets {
    fn remember(&self, modules: &[&'static ClientModule]) {
        let mut known = self.modules.lock().unwrap_or_else(PoisonError::into_inner);
        for module in modules {
            known.insert(format!("{}.{}.js", module.name, module.hash()), module);
        }
    }

    /// A framework asset at `path` (below `/_rn/`), if there is one.
    pub(crate) fn asset(&self, path: &str) -> Option<Response> {
        let rest = path.strip_prefix("/_rn/")?;
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
            Bytes::from_static(
                self.modules
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(name)?
                    .js
                    .as_bytes(),
            )
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

    /// The build's version, which the browser runtime sends with each
    /// server call (`x-rn-fn-version`): a call from a page of another build
    /// is answered `409` with this version, and the runtime reloads the
    /// page rather than speak an old wire format.
    #[must_use]
    pub fn version(self, version: impl Into<String>) -> Self {
        *self.web.version.lock().unwrap_or_else(PoisonError::into_inner) = Some(version.into());
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
                let shell =
                    tokio::task::spawn_blocking(move || page::prerender_shell(&page, &cx)).await;
                let Ok(shell) = shell else {
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
    let rendered = tokio::task::spawn_blocking(move || page::render(page, &cx)).await;
    let Ok(rendered) = rendered else {
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
