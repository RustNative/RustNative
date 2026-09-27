//! Secure-by-default request handling (`docs/server/security-checklist.md`):
//! request-forgery protection, a strict content security policy with a
//! per-response nonce, secure cookie defaults, rate limiting, a request
//! size limit, and security headers. Each is on unless an application
//! turns it off, and each has a test.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use http::{HeaderMap, HeaderValue, Method};

use crate::request::RequestContext;
use crate::response::ServerError;

/// `bytes` random bytes, URL-safe base64.
///
/// # Panics
///
/// If the operating system has no randomness to give, which no request
/// should be answered without.
#[must_use]
#[allow(clippy::expect_used, reason = "a server without randomness must not answer")]
pub fn random_token(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    getrandom::getrandom(&mut buffer).expect("the operating system's random source");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buffer)
}

/// The security settings; [`Security::default`] is the strict set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Security {
    /// The largest request body accepted (default 1 MiB); larger is `413`.
    pub body_limit: usize,
    /// Requests per client per window before `429` (default 120 a minute).
    pub rate_limit: Option<(u32, Duration)>,
    /// Unsafe methods need the request-forgery token (default on).
    pub csrf: bool,
    /// `Strict-Transport-Security` (default one year, with subdomains).
    pub hsts: bool,
    /// Cross-origin isolation (`COOP`/`COEP`), off by default: it breaks
    /// embedding third-party resources, so an application opts in.
    pub cross_origin_isolation: bool,
}

impl Default for Security {
    fn default() -> Self {
        Self {
            body_limit: 1024 * 1024,
            rate_limit: Some((120, Duration::from_secs(60))),
            csrf: true,
            hsts: true,
            cross_origin_isolation: false,
        }
    }
}

/// The per-response content-security-policy nonce: a `<script>` or
/// `<style>` the page writes itself carries it (`nonce="…"`), and nothing
/// else runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CspNonce(pub String);

impl crate::request::FromRequest for CspNonce {
    fn from_request(request: &RequestContext) -> Result<Self, ServerError> {
        request.value::<Self>().cloned().ok_or_else(|| ServerError::internal("no CSP nonce"))
    }
}

/// The request-forgery token to put in a form (`<input type="hidden"
/// name="_csrf" value="…">`) or send as `X-CSRF-Token`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsrfToken(pub String);

impl crate::request::FromRequest for CsrfToken {
    fn from_request(request: &RequestContext) -> Result<Self, ServerError> {
        request.value::<Self>().cloned().ok_or_else(|| ServerError::internal("no CSRF token"))
    }
}

/// The cookie that carries the request-forgery token.
pub(crate) const CSRF_COOKIE: &str = "__Host-csrf";

/// Whether a method changes something (and so needs the token).
pub(crate) fn is_unsafe(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE)
}

/// Double-submit check: an unsafe request must carry the token its cookie
/// holds, in `X-CSRF-Token` or the form's `_csrf` field. A request
/// authenticated by a bearer token carries no ambient credential, so it is
/// not a forgery risk.
pub(crate) fn check_csrf(request: &RequestContext) -> Result<(), ServerError> {
    if !is_unsafe(request.method()) {
        return Ok(());
    }
    if request.header("authorization").is_some_and(|value| value.starts_with("Bearer ")) {
        return Ok(());
    }
    let Some(expected) = request.cookie(CSRF_COOKIE) else {
        return Err(ServerError::forbidden());
    };
    let submitted = request.header("x-csrf-token").map(str::to_owned).or_else(|| {
        let text = std::str::from_utf8(request.body()).ok()?;
        crate::request::form_pairs(text)
            .into_iter()
            .find_map(|(key, value)| (key == "_csrf").then_some(value))
    });
    match submitted {
        Some(submitted) if constant_time_eq(submitted.as_bytes(), expected.as_bytes()) => Ok(()),
        _ => Err(ServerError::forbidden()),
    }
}

/// Compares without leaking where the first difference is.
#[must_use]
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).fold(0u8, |difference, (a, b)| difference | (a ^ b)) == 0
}

/// A `Set-Cookie` value with secure defaults: `Secure`, `HttpOnly`,
/// `SameSite=Lax`, `Path=/`.
///
/// ```
/// use rustnative_server::Cookie;
///
/// let cookie = Cookie::new("theme", "dark").to_string();
/// assert_eq!(cookie, "theme=dark; Path=/; Secure; HttpOnly; SameSite=Lax");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cookie {
    name: String,
    value: String,
    http_only: bool,
    same_site: &'static str,
    max_age: Option<Duration>,
}

impl Cookie {
    /// A cookie with the secure defaults.
    #[must_use]
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            http_only: true,
            same_site: "Lax",
            max_age: None,
        }
    }

    /// Readable by the page's script (only for values a script must send
    /// back, like the request-forgery token).
    #[must_use]
    pub const fn readable_by_script(mut self) -> Self {
        self.http_only = false;
        self
    }

    /// `SameSite=Strict`.
    #[must_use]
    pub const fn strict(mut self) -> Self {
        self.same_site = "Strict";
        self
    }

    /// Expires after `age`.
    #[must_use]
    pub const fn max_age(mut self, age: Duration) -> Self {
        self.max_age = Some(age);
        self
    }

    /// A cookie that removes `name`.
    #[must_use]
    pub fn removal(name: impl Into<String>) -> Self {
        Self::new(name, "").max_age(Duration::ZERO)
    }
}

impl std::fmt::Display for Cookie {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}={}; Path=/; Secure", self.name, self.value)?;
        if self.http_only {
            formatter.write_str("; HttpOnly")?;
        }
        write!(formatter, "; SameSite={}", self.same_site)?;
        if let Some(age) = self.max_age {
            write!(formatter, "; Max-Age={}", age.as_secs())?;
        }
        Ok(())
    }
}

/// Adds the security headers to a response.
pub(crate) fn secure_headers(headers: &mut HeaderMap, security: &Security, nonce: &str) {
    let mut set = |name: &'static str, value: String| {
        if let Ok(value) = HeaderValue::from_str(&value) {
            headers.entry(name).or_insert(value);
        }
    };
    set(
        "content-security-policy",
        format!(
            "default-src 'self'; script-src 'self' 'nonce-{nonce}'; style-src 'self' 'nonce-{nonce}'; \
             img-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; \
             form-action 'self'"
        ),
    );
    set("x-content-type-options", "nosniff".into());
    set("x-frame-options", "DENY".into());
    set("referrer-policy", "strict-origin-when-cross-origin".into());
    set("permissions-policy", "camera=(), microphone=(), geolocation=()".into());
    if security.hsts {
        set("strict-transport-security", "max-age=31536000; includeSubDomains".into());
    }
    if security.cross_origin_isolation {
        set("cross-origin-opener-policy", "same-origin".into());
        set("cross-origin-embedder-policy", "require-corp".into());
    }
}

/// A token bucket per client.
#[derive(Debug, Default)]
pub(crate) struct RateLimiter {
    buckets: Mutex<HashMap<String, (f64, Instant)>>,
}

impl RateLimiter {
    /// Takes one token for `client`; `Err` holds how long until one frees.
    pub(crate) fn take(&self, client: &str, limit: (u32, Duration)) -> Result<(), Duration> {
        let (capacity, window) = limit;
        let capacity = f64::from(capacity.max(1));
        let rate = capacity / window.as_secs_f64().max(f64::EPSILON);
        let now = Instant::now();
        let mut buckets = self.buckets.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if buckets.len() > 100_000 {
            // ponytail: drops every bucket when the table is huge, a brief
            // amnesty; an LRU keeps the busy ones if that ever matters.
            buckets.clear();
        }
        let (tokens, last) = buckets.entry(client.to_owned()).or_insert((capacity, now));
        *tokens = (*tokens + now.duration_since(*last).as_secs_f64() * rate).min(capacity);
        *last = now;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            Ok(())
        } else {
            Err(Duration::from_secs_f64((1.0 - *tokens) / rate))
        }
    }
}
