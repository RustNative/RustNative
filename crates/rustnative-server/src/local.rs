//! The application as an `HttpService`, in process: a client written
//! against the core contract — server functions, server components — calls
//! it without a socket. For tests, and for a desktop application that
//! embeds its own server.

use bytes::Bytes;
use rustnative_core::{HttpRequest, HttpResponse, HttpService, Method, ServiceError};

use crate::AppService;

/// `service`, answering `HttpService` requests directly.
#[derive(Clone)]
pub struct InProcess(pub AppService);

#[async_trait::async_trait]
impl HttpService for InProcess {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ServiceError> {
        let method = match request.method() {
            Method::Get => http::Method::GET,
            Method::Post => http::Method::POST,
            Method::Put => http::Method::PUT,
            Method::Delete => http::Method::DELETE,
            Method::Patch => http::Method::PATCH,
            Method::Head => http::Method::HEAD,
            _ => http::Method::OPTIONS,
        };
        // Only the path and query reach the application.
        let url = request.url();
        let path = url
            .find("://")
            .and_then(|scheme| url[scheme + 3..].find('/').map(|slash| &url[scheme + 3 + slash..]))
            .unwrap_or(url);
        let mut builder = http::Request::builder().method(method).uri(path);
        for (name, value) in request.headers() {
            builder = builder.header(name.as_str(), value.as_str());
        }
        let request = builder
            .body(Bytes::copy_from_slice(request.body_bytes()))
            .map_err(|error| ServiceError::new(error.to_string()))?;
        let response = self.0.handle(request, None).await;
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
            })
            .collect();
        Ok(HttpResponse::new(response.status().as_u16(), headers, response.body().to_vec()))
    }
}
