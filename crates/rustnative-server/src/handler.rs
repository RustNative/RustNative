//! Handlers and method routing.
//!
//! A handler is an `async fn` whose parameters are extractors
//! ([`crate::FromRequest`]) and whose result is a response
//! ([`crate::IntoResponse`]). A route's handlers are grouped by method
//! ([`get`], [`post`], …) and then **guarded**: a [`MethodRouter`] is only
//! accepted by `ServerApp::route` once it says who may use it —
//! [`MethodRouter::public`], or an authorization policy (`crate::auth`).
//! A route that forgets does not compile:
//!
//! ```compile_fail
//! use rustnative_server::{ServerApp, get};
//!
//! async fn secret() -> &'static str { "the plans" }
//! // error: expected `MethodRouter<Guarded>`, found `MethodRouter<Unguarded>`
//! let app = ServerApp::new().route("/plans", get(secret));
//! ```

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

use http::Method;

use crate::request::{FromRequest, RequestContext};
use crate::response::{IntoResponse, Response};

/// A boxed future answering a request.
pub type BoxFuture = Pin<Box<dyn Future<Output = Response> + Send>>;

/// A handler with its extractors erased.
pub(crate) type Erased = Arc<dyn Fn(RequestContext) -> BoxFuture + Send + Sync>;

/// An `async fn` taking extractors `Args` and returning a response.
pub trait Handler<Args>: Clone + Send + Sync + 'static {
    /// Answers `request`.
    fn call(&self, request: RequestContext) -> BoxFuture;
}

macro_rules! handler {
    ($($arg:ident),*) => {
        impl<F, Fut, R, $($arg,)*> Handler<($($arg,)*)> for F
        where
            F: Fn($($arg),*) -> Fut + Clone + Send + Sync + 'static,
            Fut: Future<Output = R> + Send + 'static,
            R: IntoResponse,
            $($arg: FromRequest + Send + 'static,)*
        {
            #[allow(non_snake_case, unused_variables, reason = "one binding per extractor type")]
            fn call(&self, request: RequestContext) -> BoxFuture {
                $(
                    let $arg = match $arg::from_request(&request) {
                        Ok(value) => value,
                        Err(error) => {
                            let json = request.wants_json();
                            return Box::pin(async move { error.render(json) });
                        }
                    };
                )*
                let future = (self)($($arg),*);
                Box::pin(async move { future.await.into_response() })
            }
        }
    };
}

handler!();
handler!(A1);
handler!(A1, A2);
handler!(A1, A2, A3);
handler!(A1, A2, A3, A4);
handler!(A1, A2, A3, A4, A5);
handler!(A1, A2, A3, A4, A5, A6);

/// A route not yet guarded.
#[derive(Debug, Clone, Copy)]
pub struct Unguarded;

/// A route that says who may use it.
#[derive(Debug, Clone, Copy)]
pub struct Guarded;

/// A check a route runs before its handler: `Err` is the response instead.
pub(crate) type Gate =
    Arc<dyn Fn(&mut RequestContext) -> Result<(), crate::response::ServerError> + Send + Sync>;

/// A route's handlers by method.
pub struct MethodRouter<G = Unguarded> {
    pub(crate) handlers: Vec<(Method, Erased)>,
    pub(crate) gate: Option<Gate>,
    pub(crate) csrf_exempt: bool,
    pub(crate) access: &'static str,
    pub(crate) cache: Option<(&'static [&'static str], std::time::Duration)>,
    guard: PhantomData<G>,
}

impl<G> Clone for MethodRouter<G> {
    fn clone(&self) -> Self {
        Self {
            handlers: self.handlers.clone(),
            gate: self.gate.clone(),
            csrf_exempt: self.csrf_exempt,
            access: self.access,
            cache: self.cache,
            guard: PhantomData,
        }
    }
}

fn erase<H: Handler<Args>, Args>(handler: H) -> Erased {
    Arc::new(move |request| handler.call(request))
}

macro_rules! method {
    ($function:ident, $method:ident, $doc:literal) => {
        #[doc = $doc]
        pub fn $function<H: Handler<Args>, Args>(handler: H) -> MethodRouter {
            MethodRouter::default().$function(handler)
        }

        impl MethodRouter<Unguarded> {
            #[doc = $doc]
            #[must_use]
            pub fn $function<H: Handler<Args>, Args>(mut self, handler: H) -> Self {
                self.handlers.push((Method::$method, erase(handler)));
                self
            }
        }
    };
}

impl Default for MethodRouter<Unguarded> {
    fn default() -> Self {
        Self {
            handlers: Vec::new(),
            gate: None,
            csrf_exempt: false,
            access: "",
            cache: None,
            guard: PhantomData,
        }
    }
}

method!(get, GET, "Handles `GET` (and `HEAD`).");
method!(post, POST, "Handles `POST`.");
method!(put, PUT, "Handles `PUT`.");
method!(patch, PATCH, "Handles `PATCH`.");
method!(delete, DELETE, "Handles `DELETE`.");

impl MethodRouter<Unguarded> {
    /// Anyone may use this route.
    #[must_use]
    pub fn public(self) -> MethodRouter<Guarded> {
        self.guarded(None, "public")
    }

    /// Guards the route with `gate`, described as `access` in the route
    /// listing and the API schema.
    pub(crate) fn guarded(self, gate: Option<Gate>, access: &'static str) -> MethodRouter<Guarded> {
        MethodRouter {
            handlers: self.handlers,
            gate,
            csrf_exempt: self.csrf_exempt,
            access,
            cache: self.cache,
            guard: PhantomData,
        }
    }

    /// Accepts unsafe methods without a request-forgery token: for a
    /// webhook or an API authenticated by something other than a cookie.
    #[must_use]
    pub const fn csrf_exempt(mut self) -> Self {
        self.csrf_exempt = true;
        self
    }
}

impl MethodRouter<Guarded> {
    /// Caches this route's `GET` responses for `ttl` under `tags` (see
    /// [`crate::cache`]); only a public route's responses are cached.
    #[must_use]
    pub const fn cached(mut self, tags: &'static [&'static str], ttl: std::time::Duration) -> Self {
        self.cache = Some((tags, ttl));
        self
    }
}

impl<G> MethodRouter<G> {
    /// The handler for `method`; `GET` answers `HEAD`.
    pub(crate) fn handler(&self, method: &Method) -> Option<&Erased> {
        let wanted = if method == Method::HEAD { &Method::GET } else { method };
        self.handlers.iter().find(|(candidate, _)| candidate == wanted).map(|(_, handler)| handler)
    }

    /// The methods it answers, for `Allow`.
    pub(crate) fn methods(&self) -> Vec<&Method> {
        self.handlers.iter().map(|(method, _)| method).collect()
    }
}
