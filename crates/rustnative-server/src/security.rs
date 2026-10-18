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
    /// The capabilities the application's pages use: each opens its
    /// powerful features to the application's own origin in the
    /// `Permissions-Policy`; every other powerful feature stays closed
    /// (`C69-1`).
    pub capabilities: Vec<rustnative_core::Capability>,
    /// The proxies whose `X-Forwarded-For` is believed (default: loopback,
    /// so a proxy on the same machine — `rustnative deploy local`, a local
    /// nginx — is). A request arriving from one of these is keyed by the
    /// nearest address in its `X-Forwarded-For` that is not itself a
    /// trusted proxy; from anywhere else, by its peer address, so a client
    /// cannot choose its own rate-limit bucket. Add the load balancer's
    /// addresses when the server runs behind one.
    pub trusted_proxies: Vec<std::net::IpAddr>,
    /// The most connections the application's own listener serves at once
    /// (default 1024); one more is closed as it arrives. A connection
    /// upgraded to a WebSocket leaves the count: its handler bounds those.
    pub max_connections: usize,
    /// How long a client has to send a request's headers, on the
    /// application's own listener (default 30 s); an idle kept-alive
    /// connection is closed after the same time.
    pub header_timeout: Duration,
    /// How long a client has to send a request's body, on the
    /// application's own listener (default 30 s); slower is `408`.
    pub body_timeout: Duration,
    /// How long the application's own listener waits, once told to stop,
    /// for the requests in flight to finish (default 10 s).
    pub shutdown_grace: Duration,
}

impl Default for Security {
    fn default() -> Self {
        Self {
            body_limit: 1024 * 1024,
            rate_limit: Some((120, Duration::from_secs(60))),
            csrf: true,
            hsts: true,
            cross_origin_isolation: false,
            capabilities: Vec::new(),
            trusted_proxies: vec![
                std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
                std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
            ],
            max_connections: 1024,
            header_timeout: Duration::from_secs(30),
            body_timeout: Duration::from_secs(30),
            shutdown_grace: Duration::from_secs(10),
        }
    }
}

impl Security {
    /// The client a request came from: the peer, or — when the peer is a
    /// trusted proxy — the nearest untrusted address it forwarded. `None`
    /// when the host told the application nothing about the peer.
    #[must_use]
    pub fn client_address(
        &self,
        peer: Option<std::net::IpAddr>,
        headers: &HeaderMap,
    ) -> Option<std::net::IpAddr> {
        let peer = peer?;
        if !self.trusted_proxies.contains(&peer) {
            return Some(peer);
        }
        let forwarded = headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .filter_map(|part| part.trim().parse::<std::net::IpAddr>().ok())
            .collect::<Vec<_>>();
        Some(
            forwarded
                .into_iter()
                .rev()
                .find(|address| !self.trusted_proxies.contains(address))
                .unwrap_or(peer),
        )
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
/// holds, in `X-CSRF-Token` or the form's `_csrf` field. A request that
/// presents a bearer token *and no cookies at all* carries no ambient
/// credential, so it is not a forgery risk and is exempt; one that also
/// carries cookies is checked like any other, because a page on another
/// site can add an `Authorization` header to a credentialed request
/// wherever cross-origin requests are allowed.
pub(crate) fn check_csrf(request: &RequestContext) -> Result<(), ServerError> {
    if !is_unsafe(request.method()) {
        return Ok(());
    }
    if request.header("authorization").is_some_and(|value| value.starts_with("Bearer "))
        && request.header("cookie").is_none()
    {
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
/// The powerful features a page could reach, and the capability that
/// opens each; a feature no capability opens is always closed.
const POLICY: &[(&str, Option<rustnative_core::Capability>)] = {
    use rustnative_core::Capability as C;
    &[
        ("accelerometer", Some(C::Sensors)),
        ("ambient-light-sensor", Some(C::Sensors)),
        ("bluetooth", Some(C::Bluetooth)),
        ("camera", Some(C::Camera)),
        ("clipboard-read", Some(C::Clipboard)),
        ("display-capture", None),
        ("geolocation", Some(C::Location)),
        ("gyroscope", Some(C::Sensors)),
        ("hid", None),
        ("magnetometer", Some(C::Sensors)),
        ("microphone", Some(C::Microphone)),
        ("midi", None),
        ("payment", None),
        ("serial", Some(C::SerialPorts)),
        ("usb", None),
        ("web-share", Some(C::SystemShare)),
    ]
};

/// The `Permissions-Policy` for pages using `capabilities`: their features
/// open to this origin (`(self)`), every other powerful feature closed.
#[must_use]
pub fn permissions_policy(capabilities: &[rustnative_core::Capability]) -> String {
    POLICY
        .iter()
        .map(|(feature, capability)| {
            let open = capability.is_some_and(|capability| capabilities.contains(&capability));
            format!("{feature}={}", if open { "(self)" } else { "()" })
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Marks a response whose page has a WebAssembly subtree: its policy
/// allows compiling WebAssembly (`'wasm-unsafe-eval'`) and nothing more.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WasmPage;

pub(crate) fn secure_headers(
    headers: &mut HeaderMap,
    security: &Security,
    nonce: &str,
    wasm: bool,
) {
    let wasm = if wasm { " 'wasm-unsafe-eval'" } else { "" };
    let mut set = |name: &'static str, value: String| {
        if let Ok(value) = HeaderValue::from_str(&value) {
            headers.entry(name).or_insert(value);
        }
    };
    set(
        "content-security-policy",
        format!(
            "default-src 'self'; script-src 'self' 'nonce-{nonce}'{wasm}; style-src 'self' 'nonce-{nonce}'; \
             img-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; \
             form-action 'self'"
        ),
    );
    set("x-content-type-options", "nosniff".into());
    set("x-frame-options", "DENY".into());
    set("referrer-policy", "strict-origin-when-cross-origin".into());
    set("permissions-policy", permissions_policy(&security.capabilities));
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
            // A bucket that has refilled says nothing a fresh one would not:
            // drop those, and keep every client still being limited.
            buckets.retain(|_, (tokens, last)| {
                *tokens + now.duration_since(*last).as_secs_f64() * rate < capacity
            });
            if buckets.len() > 100_000 {
                // ponytail: past this many clients all mid-limit, a brief
                // amnesty; a sharded table if that ever matters.
                buckets.clear();
            }
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

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::*;

    fn forwarded(value: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static(value));
        headers
    }

    #[test]
    fn a_trusted_proxy_s_forwarded_client_keys_the_request() {
        let security = Security::default();
        let local: IpAddr = "127.0.0.1".parse().unwrap();
        let client: IpAddr = "203.0.113.7".parse().unwrap();
        assert_eq!(security.client_address(Some(local), &forwarded("203.0.113.7")), Some(client));
        // A spoofed entry on the left is not believed: the nearest
        // untrusted address is the one the trusted proxy saw.
        assert_eq!(
            security.client_address(Some(local), &forwarded("198.51.100.1, 203.0.113.7")),
            Some(client)
        );
    }

    #[test]
    fn an_untrusted_peer_cannot_choose_its_bucket() {
        let security = Security::default();
        let peer: IpAddr = "203.0.113.9".parse().unwrap();
        assert_eq!(security.client_address(Some(peer), &forwarded("10.0.0.1")), Some(peer));
        assert_eq!(security.client_address(None, &forwarded("10.0.0.1")), None);
    }

    #[test]
    fn refilled_buckets_are_dropped_before_any_amnesty() {
        let limiter = RateLimiter::default();
        let limit = (1, Duration::from_secs(3600));
        assert!(limiter.take("busy", limit).is_ok());
        assert!(limiter.take("busy", limit).is_err());
        {
            let mut buckets = limiter.buckets.lock().unwrap();
            for index in 0..100_001 {
                buckets.insert(format!("idle-{index}"), (1.0, Instant::now()));
            }
        }
        assert!(limiter.take("someone", limit).is_ok());
        assert!(limiter.take("busy", limit).is_err(), "still limited: its bucket was kept");
    }
}
