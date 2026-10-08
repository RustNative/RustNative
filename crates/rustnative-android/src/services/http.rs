//! HTTP over `HttpsURLConnection`: the system's trust store, the
//! application's network security config (cleartext policy, user CAs), and
//! the system proxy. With [`CertificatePins`], a pinned host's connection is
//! made first, the SHA-256 of its leaf certificate checked against the pins
//! — the same digest the Linux and Windows backends check — and only then
//! is the request written; redirects are not followed for a pinned host.

use std::sync::Arc;

use rustnative_core::{CertificatePins, HttpRequest, HttpResponse, HttpService, ServiceError};

use super::{java, off_thread};
use crate::jni_host::{Arg, Class, JavaRef, Ret};

/// The HTTP service.
#[derive(Debug, Clone)]
pub struct AndroidHttp {
    pins: Arc<CertificatePins>,
    timeout_millis: i32,
}

impl Default for AndroidHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl AndroidHttp {
    /// A service with no pins and a 30-second timeout.
    #[must_use]
    pub fn new() -> Self {
        Self { pins: Arc::new(CertificatePins::new()), timeout_millis: 30_000 }
    }

    /// The service, refusing a pinned host whose certificate matches none
    /// of its pins.
    #[must_use]
    pub fn with_pins(mut self, pins: CertificatePins) -> Self {
        self.pins = Arc::new(pins);
        self
    }
}

#[async_trait::async_trait]
impl HttpService for AndroidHttp {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ServiceError> {
        let (pins, timeout) = (Arc::clone(&self.pins), self.timeout_millis);
        off_thread(move || send(&pins, &request, timeout)).await?
    }
}

fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    if authority.starts_with('[') {
        return authority
            .split_once(']')
            .map_or(authority, |(host, _)| host.trim_start_matches('['));
    }
    authority.split(':').next().unwrap_or_default()
}

fn field(response: &JavaRef, name: &str, signature: &str) -> Result<Ret, ServiceError> {
    java(Class::Services, name, signature, &[Arg::Obj(response)])
}

const RESPONSE: &str = "Ldev/rustnative/android/RnServices$Response;";

fn send(
    pins: &CertificatePins,
    request: &HttpRequest,
    timeout: i32,
) -> Result<HttpResponse, ServiceError> {
    let host = host(request.url()).to_owned();
    let pinned = pins.is_pinned(&host);
    let headers: Vec<String> =
        request.headers().iter().flat_map(|(name, value)| [name.clone(), value.clone()]).collect();
    let response = java(
        Class::Services,
        "open",
        &format!("(Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;ZI){RESPONSE}"),
        &[
            Arg::Str(request.method().as_str()),
            Arg::Str(request.url()),
            Arg::Strs(&headers),
            Arg::Bool(pinned),
            Arg::Int(timeout),
        ],
    )?
    .obj()
    .ok_or_else(|| ServiceError::new("the request could not be made"))?;
    let error = |response: &JavaRef| {
        field(response, "error", &format!("({RESPONSE})Ljava/lang/String;")).map(Ret::string)
    };
    if let Some(message) = error(&response)? {
        return Err(ServiceError::new(message));
    }
    if pinned {
        let digest = match field(&response, "digest", &format!("({RESPONSE})[B"))? {
            Ret::Bytes(Some(digest)) => <[u8; 32]>::try_from(digest.as_slice()).ok(),
            _ => None,
        };
        if !digest.is_some_and(|digest| pins.allows(&host, &digest)) {
            field(&response, "close", &format!("({RESPONSE})V"))?;
            return Err(ServiceError::new(format!(
                "{host}'s certificate matches none of its pins"
            )));
        }
    }
    java(
        Class::Services,
        "exchange",
        &format!("({RESPONSE}[B)V"),
        &[Arg::Obj(&response), Arg::Bytes(request.body_bytes())],
    )?;
    if let Some(message) = error(&response)? {
        return Err(ServiceError::new(message));
    }
    let status = match field(&response, "status", &format!("({RESPONSE})I"))? {
        Ret::Int(status) => u16::try_from(status).unwrap_or(0),
        _ => 0,
    };
    let flat = field(&response, "headers", &format!("({RESPONSE})[Ljava/lang/String;"))?.strings();
    let headers = flat.chunks_exact(2).map(|pair| (pair[0].clone(), pair[1].clone())).collect();
    let body = match field(&response, "body", &format!("({RESPONSE})[B"))? {
        Ret::Bytes(Some(body)) => body,
        _ => Vec::new(),
    };
    Ok(HttpResponse::new(status, headers, body))
}

#[cfg(test)]
mod tests {
    use super::host;

    #[test]
    fn the_host_is_found_in_any_url() {
        assert_eq!(host("https://example.com/a?b"), "example.com");
        assert_eq!(host("https://user:pw@example.com:8443/"), "example.com");
        assert_eq!(host("http://[::1]:8080/x"), "::1");
        assert_eq!(host("example.com"), "example.com");
    }
}
