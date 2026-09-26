//! Responses: what a handler returns, HTML that escapes by construction,
//! and typed error pages.

use std::fmt;

use bytes::Bytes;
use http::{HeaderValue, StatusCode, header};
use serde::Serialize;

/// A response, with its whole body.
pub type Response = http::Response<Bytes>;

/// What a handler may return.
pub trait IntoResponse {
    /// The response.
    fn into_response(self) -> Response;
}

fn with_type(status: StatusCode, content_type: &'static str, body: impl Into<Bytes>) -> Response {
    let mut response = http::Response::new(body.into());
    *response.status_mut() = status;
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
}

impl IntoResponse for Response {
    fn into_response(self) -> Response {
        self
    }
}

impl IntoResponse for &'static str {
    fn into_response(self) -> Response {
        with_type(StatusCode::OK, "text/plain; charset=utf-8", self)
    }
}

impl IntoResponse for String {
    fn into_response(self) -> Response {
        with_type(StatusCode::OK, "text/plain; charset=utf-8", self)
    }
}

impl IntoResponse for StatusCode {
    fn into_response(self) -> Response {
        let mut response = http::Response::new(Bytes::new());
        *response.status_mut() = self;
        response
    }
}

impl<T: IntoResponse> IntoResponse for (StatusCode, T) {
    fn into_response(self) -> Response {
        let mut response = self.1.into_response();
        *response.status_mut() = self.0;
        response
    }
}

impl<T: IntoResponse, E: IntoResponse> IntoResponse for Result<T, E> {
    fn into_response(self) -> Response {
        match self {
            Ok(value) => value.into_response(),
            Err(error) => error.into_response(),
        }
    }
}

/// A JSON body (and, as an extractor, a JSON request body).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Json<T>(pub T);

impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        match serde_json::to_vec(&self.0) {
            Ok(body) => with_type(StatusCode::OK, "application/json", body),
            Err(error) => ServerError::internal(error.to_string()).into_response(),
        }
    }
}

/// HTML that is safe by construction: text is escaped as it goes in, and
/// the only way to add markup unescaped is [`Html::trusted`], which takes a
/// `&'static str` — markup written in the source, never a runtime string.
/// Output escaping cannot be bypassed by accident.
///
/// ```
/// use rustnative_server::Html;
///
/// let name = "<script>alert(1)</script>";
/// let page = Html::trusted("<p>Hello, ").text(name).and_trusted("</p>");
/// assert_eq!(page.as_str(), "<p>Hello, &lt;script&gt;alert(1)&lt;/script&gt;</p>");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Html(String);

impl Html {
    /// Escaped text.
    #[must_use]
    pub fn escaped(text: &str) -> Self {
        Self::default().text(text)
    }

    /// Markup written in the source.
    #[must_use]
    pub fn trusted(markup: &'static str) -> Self {
        Self(markup.to_owned())
    }

    /// Appends escaped `text`.
    #[must_use]
    pub fn text(mut self, text: &str) -> Self {
        escape_into(&mut self.0, text);
        self
    }

    /// Appends markup written in the source.
    #[must_use]
    pub fn and_trusted(mut self, markup: &'static str) -> Self {
        self.0.push_str(markup);
        self
    }

    /// Appends other safe HTML.
    #[must_use]
    pub fn html(mut self, other: &Self) -> Self {
        self.0.push_str(&other.0);
        self
    }

    /// Markup this crate built by escaping every value in it.
    pub(crate) const fn from_escaped(markup: String) -> Self {
        Self(markup)
    }

    /// The markup.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Escapes `&`, `<`, `>`, `"`, and `'`.
pub(crate) fn escape_into(out: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
}

impl IntoResponse for Html {
    fn into_response(self) -> Response {
        with_type(StatusCode::OK, "text/html; charset=utf-8", self.0)
    }
}

/// A redirect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    status: StatusCode,
    location: String,
}

impl Redirect {
    /// `303 See Other`: after a form post, to a page to `GET`.
    #[must_use]
    pub fn see_other(location: impl Into<String>) -> Self {
        Self { status: StatusCode::SEE_OTHER, location: location.into() }
    }

    /// `307 Temporary Redirect`: the same method, somewhere else.
    #[must_use]
    pub fn temporary(location: impl Into<String>) -> Self {
        Self { status: StatusCode::TEMPORARY_REDIRECT, location: location.into() }
    }
}

impl IntoResponse for Redirect {
    fn into_response(self) -> Response {
        let mut response = self.status.into_response();
        if let Ok(value) = HeaderValue::from_str(&self.location) {
            response.headers_mut().insert(header::LOCATION, value);
        }
        response
    }
}

/// A failed request, rendered as an error page: JSON for a client that
/// accepts it, HTML otherwise (see `ServerApp`). A server error's message
/// is logged, never shown: the page says only what went wrong in general.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerError {
    status: StatusCode,
    message: String,
}

impl ServerError {
    /// An error with `status` and a message the client may see.
    #[must_use]
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self { status, message: message.into() }
    }

    /// `400 Bad Request`.
    #[must_use]
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    /// `401 Unauthorized`.
    #[must_use]
    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "Sign in to continue")
    }

    /// `403 Forbidden`.
    #[must_use]
    pub fn forbidden() -> Self {
        Self::new(StatusCode::FORBIDDEN, "You do not have access to this")
    }

    /// `404 Not Found`.
    #[must_use]
    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "Not found")
    }

    /// `500 Internal Server Error`; `detail` is logged, not shown.
    #[must_use]
    pub fn internal(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, detail)
    }

    /// The status.
    #[must_use]
    pub const fn status(&self) -> StatusCode {
        self.status
    }

    /// What the client is shown.
    #[must_use]
    pub fn public_message(&self) -> &str {
        if self.status.is_server_error() { "Something went wrong" } else { &self.message }
    }

    /// The page for a client that prefers JSON or HTML.
    #[must_use]
    pub fn render(&self, json: bool) -> Response {
        if self.status.is_server_error() {
            eprintln!("rustnative-server: {}: {}", self.status, self.message);
        }
        if json {
            let body = serde_json::json!({ "error": self.public_message() }).to_string();
            with_type(self.status, "application/json", body)
        } else {
            let page = Html::trusted("<!doctype html><title>")
                .text(self.status.canonical_reason().unwrap_or("Error"))
                .and_trusted("</title><h1>")
                .text(self.status.canonical_reason().unwrap_or("Error"))
                .and_trusted("</h1><p>")
                .text(self.public_message())
                .and_trusted("</p>");
            with_type(self.status, "text/html; charset=utf-8", page.0)
        }
    }
}

impl fmt::Display for ServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.status, self.message)
    }
}

impl std::error::Error for ServerError {}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        // Rendered as HTML here; the application re-renders it by the
        // request's `Accept` (see `app`), where the request is known.
        let mut response = self.render(false);
        response.extensions_mut().insert(self);
        response
    }
}
