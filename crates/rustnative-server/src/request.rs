//! Requests and typed extractors: a handler names what it needs in its
//! parameters (`Path<T>`, `Query<T>`, `Json<T>`, `Form<T>`, `State<T>`,
//! `Scope`, …) and receives it already parsed, or the request is refused
//! with a `400` before the handler runs.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http::{HeaderMap, Method};
use rustnative_core::RouteParams;
use serde::de::DeserializeOwned;

use crate::response::{Json, ServerError};
use crate::scope::RequestScope;

/// Values of any type, one per type.
#[derive(Clone, Default)]
pub(crate) struct TypeMap(HashMap<TypeId, Arc<dyn Any + Send + Sync>>);

impl TypeMap {
    pub(crate) fn insert<T: Send + Sync + 'static>(&mut self, value: T) {
        self.0.insert(TypeId::of::<T>(), Arc::new(value));
    }

    pub(crate) fn get<T: 'static>(&self) -> Option<&T> {
        self.0.get(&TypeId::of::<T>()).and_then(|value| value.downcast_ref())
    }
}

/// Everything a handler can know about the request it is answering.
pub struct RequestContext {
    pub(crate) method: Method,
    pub(crate) path: String,
    pub(crate) query: String,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Bytes,
    pub(crate) params: RouteParams,
    pub(crate) param_order: Vec<String>,
    pub(crate) state: Arc<TypeMap>,
    pub(crate) values: TypeMap,
    pub(crate) scope: RequestScope,
    pub(crate) client: Option<SocketAddr>,
}

impl RequestContext {
    /// The method.
    #[must_use]
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// The path, below any mount prefix.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The headers.
    #[must_use]
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// The body.
    #[must_use]
    pub fn body(&self) -> &Bytes {
        &self.body
    }

    /// A header's value, if present and text.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }

    /// A cookie's value.
    #[must_use]
    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.headers.get_all(http::header::COOKIE).iter().find_map(|value| {
            value.to_str().ok()?.split(';').find_map(|pair| {
                let (key, value) = pair.trim().split_once('=')?;
                (key == name).then_some(value)
            })
        })
    }

    /// A value a middleware attached to this request (the session, the
    /// principal, …).
    #[must_use]
    pub fn value<T: 'static>(&self) -> Option<&T> {
        self.values.get()
    }

    /// Attaches a value for the handler and later middleware.
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) {
        self.values.insert(value);
    }

    /// The application state of type `T`.
    #[must_use]
    pub fn state<T: 'static>(&self) -> Option<&T> {
        self.state.get()
    }

    /// The request's task scope.
    #[must_use]
    pub const fn scope(&self) -> &RequestScope {
        &self.scope
    }

    /// The client's address, when served over a socket.
    #[must_use]
    pub const fn client(&self) -> Option<SocketAddr> {
        self.client
    }

    /// Whether the client prefers JSON to HTML.
    #[must_use]
    pub fn wants_json(&self) -> bool {
        self.header("accept").is_some_and(|accept| {
            accept.contains("application/json") && !accept.contains("text/html")
        })
    }
}

/// Something a handler parameter can be built from.
pub trait FromRequest: Sized {
    /// Builds it, or refuses the request.
    ///
    /// # Errors
    ///
    /// The response the request gets instead (usually `400`).
    fn from_request(request: &RequestContext) -> Result<Self, ServerError>;
}

/// The route's parameters, parsed: a single value, a tuple in pattern
/// order, or a struct by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path<T>(pub T);

impl<T: DeserializeOwned> FromRequest for Path<T> {
    fn from_request(request: &RequestContext) -> Result<Self, ServerError> {
        // In the order the pattern names them, so a tuple reads left to right.
        let values: Vec<(&str, &str)> = request
            .param_order
            .iter()
            .filter_map(|name| Some((name.as_str(), request.params.get(name)?)))
            .collect();
        let as_map = || {
            serde_json::Value::Object(
                values.iter().map(|(key, value)| ((*key).to_owned(), scalar(value))).collect(),
            )
        };
        let text = |value: &&str| serde_json::Value::String((*value).to_owned());
        // Numbers read as numbers first; a type that wants text gets text.
        let attempts = [
            values.first().map(|(_, value)| scalar(value)),
            values.first().map(|(_, value)| text(value)),
            Some(serde_json::Value::Array(values.iter().map(|(_, value)| scalar(value)).collect())),
            Some(serde_json::Value::Array(values.iter().map(|(_, value)| text(value)).collect())),
            Some(as_map()),
            Some(serde_json::Value::Object(
                values.iter().map(|(key, value)| ((*key).to_owned(), text(value))).collect(),
            )),
        ];
        attempts
            .into_iter()
            .flatten()
            .find_map(|value| serde_json::from_value(value).ok())
            .map(Path)
            .ok_or_else(|| ServerError::bad_request("The address has an invalid parameter"))
    }
}

/// A route parameter as JSON: a number or boolean where it reads as one,
/// a string otherwise.
fn scalar(value: &str) -> serde_json::Value {
    serde_json::from_str::<serde_json::Value>(value)
        .ok()
        .filter(|parsed| parsed.is_number() || parsed.is_boolean())
        .unwrap_or_else(|| serde_json::Value::String(value.to_owned()))
}

/// Decodes `application/x-www-form-urlencoded` pairs.
pub(crate) fn form_pairs(text: &str) -> Vec<(String, String)> {
    text.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(key), decode(value))
        })
        .collect()
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => out.push(b' '),
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
                match hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        index += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            byte => out.push(byte),
        }
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn pairs_to<T: DeserializeOwned>(pairs: Vec<(String, String)>) -> Result<T, ServerError> {
    // Each value is read as its field's type asks: a number, a boolean,
    // text — so a numeric-looking value for a text field stays text.
    rustnative_web::request::from_pairs(pairs)
        .map_err(|error| ServerError::bad_request(format!("Invalid input: {error}")))
}

/// The query string, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query<T>(pub T);

impl<T: DeserializeOwned> FromRequest for Query<T> {
    fn from_request(request: &RequestContext) -> Result<Self, ServerError> {
        pairs_to(form_pairs(&request.query)).map(Query)
    }
}

/// A URL-encoded form body, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form<T>(pub T);

impl<T: DeserializeOwned> FromRequest for Form<T> {
    fn from_request(request: &RequestContext) -> Result<Self, ServerError> {
        let text = std::str::from_utf8(&request.body)
            .map_err(|_| ServerError::bad_request("The form is not UTF-8"))?;
        let pairs = form_pairs(text).into_iter().filter(|(key, _)| key != "_csrf").collect();
        pairs_to(pairs).map(Form)
    }
}

impl<T: DeserializeOwned> FromRequest for Json<T> {
    fn from_request(request: &RequestContext) -> Result<Self, ServerError> {
        serde_json::from_slice(&request.body)
            .map(Json)
            .map_err(|error| ServerError::bad_request(format!("Invalid JSON: {error}")))
    }
}

/// Application state registered with `ServerApp::state`.
#[derive(Debug, Clone)]
pub struct State<T>(pub T);

impl<T: Clone + 'static> FromRequest for State<T> {
    fn from_request(request: &RequestContext) -> Result<Self, ServerError> {
        request.state::<T>().cloned().map(State).ok_or_else(|| {
            ServerError::internal(format!("no state of type {}", std::any::type_name::<T>()))
        })
    }
}

/// The request's task scope: work spawned in it is cancelled when the
/// response is sent.
impl FromRequest for RequestScope {
    fn from_request(request: &RequestContext) -> Result<Self, ServerError> {
        Ok(request.scope.clone())
    }
}

/// The request's headers.
impl FromRequest for HeaderMap {
    fn from_request(request: &RequestContext) -> Result<Self, ServerError> {
        Ok(request.headers.clone())
    }
}

/// The raw body.
impl FromRequest for Bytes {
    fn from_request(request: &RequestContext) -> Result<Self, ServerError> {
        Ok(request.body.clone())
    }
}

impl<T: FromRequest> FromRequest for Option<T> {
    fn from_request(request: &RequestContext) -> Result<Self, ServerError> {
        Ok(T::from_request(request).ok())
    }
}
