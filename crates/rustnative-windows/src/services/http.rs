//! `WinHttp`: the framework's [`HttpService`] on Windows, over `WinHTTP`,
//! with certificate pinning as declared policy (`PLAN.md` Milestone 47,
//! `C34`).
//!
//! Each request runs on the blocking pool with `WinHTTP`'s synchronous API.
//! For a host with pins ([`CertificatePins`]), the request is sent in two
//! steps: the headers first — which completes the TLS handshake — then,
//! only once the server's certificate matches a pin, the body. A mismatch
//! ends the request with an error before a byte of the body leaves the
//! machine. Redirects are not followed for a pinned host, so a redirect
//! cannot lead the request somewhere the pins do not cover.

use std::ffi::c_void;
use std::sync::Arc;

use rustnative_core::{CertificatePins, HttpRequest, HttpResponse, HttpService, ServiceError};
use windows_sys::Win32::Networking::WinHttp::{
    URL_COMPONENTS, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ADDREQ_FLAG_ADD,
    WINHTTP_DISABLE_REDIRECTS, WINHTTP_FLAG_SECURE, WINHTTP_INTERNET_SCHEME_HTTPS,
    WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_OPTION_SERVER_CERT_CONTEXT, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_RAW_HEADERS_CRLF, WINHTTP_QUERY_STATUS_CODE, WinHttpAddRequestHeaders,
    WinHttpCloseHandle, WinHttpConnect, WinHttpCrackUrl, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpQueryOption, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption, WinHttpWriteData,
};
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_SHA256_ALG_HANDLE, BCryptHash, CERT_CONTEXT, CertFreeCertificateContext,
};

use super::run_blocking;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error(what: &str) -> ServiceError {
    ServiceError::new(format!("{what} failed: {}", std::io::Error::last_os_error()))
}

/// A `WinHTTP` handle, closed when dropped.
struct Handle(*mut c_void);

impl Handle {
    fn new(raw: *mut c_void, what: &str) -> Result<Self, ServiceError> {
        if raw.is_null() { Err(last_error(what)) } else { Ok(Self(raw)) }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: a handle WinHTTP returned, closed exactly once.
        unsafe { WinHttpCloseHandle(self.0) };
    }
}

/// The SHA-256 digest of `bytes`.
#[must_use]
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut digest = [0_u8; 32];
    let length = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
    // SAFETY: the pseudo-handle is a documented constant (Windows 10+); the
    // input and output pointers and lengths describe live buffers.
    unsafe {
        BCryptHash(
            BCRYPT_SHA256_ALG_HANDLE,
            std::ptr::null(),
            0,
            bytes.as_ptr(),
            length,
            digest.as_mut_ptr(),
            32,
        );
    }
    digest
}

/// The framework's HTTP service on Windows, over `WinHTTP`. For a host
/// with [`CertificatePins`], the headers are sent first and the body only
/// once the server's certificate matches a pin; plain HTTP and redirects are
/// refused for that host.
#[derive(Debug, Clone, Default)]
pub struct WinHttp {
    pins: Arc<CertificatePins>,
    agent: String,
}

impl WinHttp {
    /// A service with no pins.
    #[must_use]
    pub fn new() -> Self {
        Self { pins: Arc::new(CertificatePins::new()), agent: "RustNative".to_owned() }
    }

    /// Enforces `pins`.
    #[must_use]
    pub fn with_pins(mut self, pins: CertificatePins) -> Self {
        self.pins = Arc::new(pins);
        self
    }
}

#[async_trait::async_trait]
impl HttpService for WinHttp {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ServiceError> {
        let pins = Arc::clone(&self.pins);
        let agent = self.agent.clone();
        run_blocking(move || send(&agent, &pins, &request)).await
    }
}

struct Target {
    secure: bool,
    host: String,
    port: u16,
    path: String,
}

fn crack(url: &str) -> Result<Target, ServiceError> {
    let url_wide = wide(url);
    let mut host = vec![0_u16; 256];
    let mut path = vec![0_u16; 2048];
    let mut extra = vec![0_u16; 2048];
    let mut parts = URL_COMPONENTS {
        dwStructSize: u32::try_from(size_of::<URL_COMPONENTS>()).unwrap_or(0),
        lpszHostName: host.as_mut_ptr(),
        dwHostNameLength: 256,
        lpszUrlPath: path.as_mut_ptr(),
        dwUrlPathLength: 2048,
        lpszExtraInfo: extra.as_mut_ptr(),
        dwExtraInfoLength: 2048,
        ..URL_COMPONENTS::default()
    };
    // SAFETY: a nul-terminated URL (length 0 means "terminated"); each
    // component buffer is live and its length is given in characters.
    if unsafe { WinHttpCrackUrl(url_wide.as_ptr(), 0, 0, &raw mut parts) } == 0 {
        return Err(last_error("WinHttpCrackUrl"));
    }
    let text = |buffer: &[u16], length: u32| {
        String::from_utf16_lossy(&buffer[..usize::try_from(length).unwrap_or(0).min(buffer.len())])
    };
    Ok(Target {
        secure: parts.nScheme == WINHTTP_INTERNET_SCHEME_HTTPS,
        host: text(&host, parts.dwHostNameLength),
        port: parts.nPort,
        path: text(&path, parts.dwUrlPathLength) + &text(&extra, parts.dwExtraInfoLength),
    })
}

/// The digest of the certificate the server of `request` presented.
fn server_certificate(request: &Handle) -> Result<[u8; 32], ServiceError> {
    let mut context: *const CERT_CONTEXT = std::ptr::null();
    let mut size = u32::try_from(size_of::<*const CERT_CONTEXT>()).unwrap_or(0);
    // SAFETY: the option writes one `PCCERT_CONTEXT` into the buffer given,
    // whose size is passed.
    let ok = unsafe {
        WinHttpQueryOption(
            request.0,
            WINHTTP_OPTION_SERVER_CERT_CONTEXT,
            (&raw mut context).cast(),
            &raw mut size,
        )
    };
    if ok == 0 || context.is_null() {
        return Err(last_error("reading the server certificate"));
    }
    // SAFETY: WinHTTP returned a valid certificate context, which we own
    // and free below; its encoded bytes live as long as it does.
    let digest = unsafe {
        let certificate = &*context;
        let encoded = std::slice::from_raw_parts(
            certificate.pbCertEncoded,
            usize::try_from(certificate.cbCertEncoded).unwrap_or(0),
        );
        let digest = sha256(encoded);
        CertFreeCertificateContext(context);
        digest
    };
    Ok(digest)
}

fn query_status(request: &Handle) -> Result<u16, ServiceError> {
    let mut status = 0_u32;
    let mut size = 4_u32;
    // SAFETY: the number flag makes WinHTTP write one `u32` into `status`.
    let ok = unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            std::ptr::null(),
            (&raw mut status).cast(),
            &raw mut size,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(last_error("reading the status"));
    }
    u16::try_from(status).map_err(|_| ServiceError::new("an out-of-range status code"))
}

fn query_headers(request: &Handle) -> Vec<(String, String)> {
    let mut size = 0_u32;
    // SAFETY: a null buffer asks for the size needed, in bytes.
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_RAW_HEADERS_CRLF,
            std::ptr::null(),
            std::ptr::null_mut(),
            &raw mut size,
            std::ptr::null_mut(),
        );
    }
    let mut buffer = vec![0_u16; usize::try_from(size).unwrap_or(0) / 2 + 1];
    // SAFETY: the buffer holds the `size` bytes WinHTTP asked for.
    let ok = unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_RAW_HEADERS_CRLF,
            std::ptr::null(),
            buffer.as_mut_ptr().cast(),
            &raw mut size,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Vec::new();
    }
    let text = String::from_utf16_lossy(&buffer[..usize::try_from(size).unwrap_or(0) / 2]);
    text.split("\r\n")
        .skip(1) // the status line
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
        .collect()
}

fn read_body(request: &Handle) -> Result<Vec<u8>, ServiceError> {
    let mut body = Vec::new();
    loop {
        let mut available = 0_u32;
        // SAFETY: WinHTTP writes the count into `available`.
        if unsafe { WinHttpQueryDataAvailable(request.0, &raw mut available) } == 0 {
            return Err(last_error("WinHttpQueryDataAvailable"));
        }
        if available == 0 {
            return Ok(body);
        }
        let start = body.len();
        body.resize(start + usize::try_from(available).unwrap_or(0), 0);
        let mut read = 0_u32;
        // SAFETY: the buffer has `available` bytes free from `start`.
        if unsafe {
            WinHttpReadData(request.0, body[start..].as_mut_ptr().cast(), available, &raw mut read)
        } == 0
        {
            return Err(last_error("WinHttpReadData"));
        }
        body.truncate(start + usize::try_from(read).unwrap_or(0));
        if read == 0 {
            return Ok(body);
        }
    }
}

fn send(
    agent: &str,
    pins: &CertificatePins,
    request: &HttpRequest,
) -> Result<HttpResponse, ServiceError> {
    let target = crack(request.url())?;
    let pinned = pins.is_pinned(&target.host);
    if pinned && !target.secure {
        return Err(ServiceError::new(format!("{} is pinned; plain HTTP is refused", target.host)));
    }
    let agent = wide(agent);
    // SAFETY: nul-terminated agent; null proxy strings with the automatic
    // proxy access type, as documented.
    let session = Handle::new(
        unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        },
        "WinHttpOpen",
    )?;
    let host = wide(&target.host);
    // SAFETY: a live session and a nul-terminated host name.
    let connection = Handle::new(
        unsafe { WinHttpConnect(session.0, host.as_ptr(), target.port, 0) },
        "WinHttpConnect",
    )?;
    let verb = wide(request.method().as_str());
    let path = wide(&target.path);
    let flags = if target.secure { WINHTTP_FLAG_SECURE } else { 0 };
    // SAFETY: a live connection; nul-terminated verb and path; null version,
    // referrer, and accept types mean the defaults.
    let handle = Handle::new(
        unsafe {
            WinHttpOpenRequest(
                connection.0,
                verb.as_ptr(),
                path.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                flags,
            )
        },
        "WinHttpOpenRequest",
    )?;
    if pinned {
        let disable = WINHTTP_DISABLE_REDIRECTS;
        // SAFETY: the option takes one `u32`, whose size is passed.
        unsafe {
            WinHttpSetOption(
                handle.0,
                WINHTTP_OPTION_DISABLE_FEATURE,
                (&raw const disable).cast(),
                4,
            )
        };
    }
    if !request.headers().is_empty() {
        let headers = request.headers().iter().fold(String::new(), |mut all, (name, value)| {
            use std::fmt::Write as _;
            let _ = write!(all, "{name}: {value}\r\n");
            all
        });
        let headers = wide(&headers);
        // SAFETY: nul-terminated headers; `u32::MAX` means "terminated".
        if unsafe {
            WinHttpAddRequestHeaders(handle.0, headers.as_ptr(), u32::MAX, WINHTTP_ADDREQ_FLAG_ADD)
        } == 0
        {
            return Err(last_error("WinHttpAddRequestHeaders"));
        }
    }
    let body = request.body_bytes();
    let length =
        u32::try_from(body.len()).map_err(|_| ServiceError::new("the body is too large"))?;
    // Headers only: the body follows once the certificate is checked.
    // SAFETY: a live request; no additional headers or optional data; the
    // total length announces the body written below.
    if unsafe { WinHttpSendRequest(handle.0, std::ptr::null(), 0, std::ptr::null(), 0, length, 0) }
        == 0
    {
        return Err(last_error("WinHttpSendRequest"));
    }
    if pinned {
        let digest = server_certificate(&handle)?;
        if !pins.allows(&target.host, &digest) {
            return Err(ServiceError::new(format!(
                "{} presented a certificate that matches none of its pins",
                target.host
            )));
        }
    }
    if !body.is_empty() {
        let mut written = 0_u32;
        // SAFETY: the body buffer and its length.
        if unsafe { WinHttpWriteData(handle.0, body.as_ptr().cast(), length, &raw mut written) }
            == 0
        {
            return Err(last_error("WinHttpWriteData"));
        }
    }
    // SAFETY: a live request whose body has been written.
    if unsafe { WinHttpReceiveResponse(handle.0, std::ptr::null_mut()) } == 0 {
        return Err(last_error("WinHttpReceiveResponse"));
    }
    let status = query_status(&handle)?;
    let headers = query_headers(&handle);
    let body = read_body(&handle)?;
    drop(handle);
    drop(connection);
    drop(session);
    Ok(HttpResponse::new(status, headers, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_the_standard_vector() {
        let digest = sha256(b"abc");
        assert_eq!(digest[..4], [0xba, 0x78, 0x16, 0xbf]);
        assert_eq!(digest[28..], [0xf2, 0x00, 0x15, 0xad]);
    }

    #[test]
    fn urls_crack_into_host_port_and_path() {
        let target =
            crack("https://example.com:8443/a/b?c=d").unwrap_or_else(|error| panic!("{error}"));
        assert!(target.secure);
        assert_eq!(
            (target.host.as_str(), target.port, target.path.as_str()),
            ("example.com", 8443, "/a/b?c=d")
        );
    }

    #[test]
    fn a_request_round_trips_through_a_local_server() {
        use std::io::{Read, Write};
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("{error}"));
        let port = listener.local_addr().map_or(0, |address| address.port());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap_or_else(|error| panic!("{error}"));
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            while !String::from_utf8_lossy(&request).contains("hello") {
                let read = stream.read(&mut buffer).unwrap_or(0);
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            let _ = stream.write_all(
                b"HTTP/1.1 201 Created
X-Test: yes
Content-Length: 2

ok",
            );
            String::from_utf8_lossy(&request).into_owned()
        });
        let request = HttpRequest::new(
            rustnative_core::Method::Post,
            format!("http://127.0.0.1:{port}/echo"),
        )
        .header("X-Client", "rustnative")
        .body(b"hello".to_vec());
        let response = send("test", &CertificatePins::new(), &request)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(response.status(), 201);
        assert_eq!(response.body_bytes(), b"ok");
        assert!(response.headers().iter().any(|(name, value)| name == "X-Test" && value == "yes"));
        let seen = server.join().unwrap_or_default();
        assert!(seen.starts_with("POST /echo") && seen.contains("X-Client: rustnative"), "{seen}");
    }

    #[test]
    fn a_pinned_host_is_refused_over_plain_http() {
        let pins = CertificatePins::new().pin("example.com", [0; 32]);
        let outcome = send("test", &pins, &HttpRequest::get("http://example.com/"));
        assert!(outcome.is_err_and(|error| error.to_string().contains("plain HTTP")));
    }
}
