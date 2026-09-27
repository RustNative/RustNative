//! What a page render knows about the request it answers.
//!
//! A server render puts a [`RequestInfo`] in the render's services; a
//! component reads it through [`request`]. Reading it makes the component
//! *dynamic*: a partially prerendered page (`C06-1`) renders its static
//! shell once, with no request, and each component that asked for the
//! request there renders its per-request part in a hole streamed later.

use std::sync::Arc;
use std::time::Duration;

use rustnative_core::ComponentContext;

use crate::client::ServerRender;

/// The request a page answers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestInfo {
    /// The path (`/notes/7`).
    pub path: String,
    /// The query string, without `?`.
    pub query: String,
    /// The request headers a page may read, lower-case names.
    pub headers: Vec<(String, String)>,
    /// The route's parameters.
    pub params: Vec<(String, String)>,
    /// The request-forgery token forms and server calls carry.
    pub csrf: String,
    /// The nonce the response's content security policy allows.
    pub nonce: String,
    /// What the host allows this request (`W-SL-2`).
    pub limits: HostLimits,
}

impl RequestInfo {
    /// A request for `path` (`/notes?sort=new`).
    #[must_use]
    pub fn get(path: &str) -> Self {
        let (path, query) = path.split_once('?').unwrap_or((path, ""));
        Self { path: path.to_owned(), query: query.to_owned(), ..Self::default() }
    }

    /// Header `name` (any case), if sent.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Route parameter `name`, if the route has it.
    #[must_use]
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    /// Query parameter `name`, percent-decoded, if present.
    #[must_use]
    pub fn query_param(&self, name: &str) -> Option<String> {
        self.query
            .split('&')
            .filter_map(|pair| pair.split_once('=').or(Some((pair, ""))))
            .find_map(|(key, value)| (decode(key) == name).then(|| decode(value)))
    }

    /// What the host allows this request.
    #[must_use]
    pub const fn limits(&self) -> &HostLimits {
        &self.limits
    }
}

/// `application/x-www-form-urlencoded` decoding: `+` is a space, `%XX` a
/// byte; malformed escapes stay as written.
#[must_use]
pub fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => out.push(b' '),
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3])
                    .ok()
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok());
                if let Some(byte) = hex {
                    out.push(byte);
                    index += 3;
                    continue;
                }
                out.push(b'%');
            }
            byte => out.push(byte),
        }
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The host's limits for one request, as capabilities an application can
/// ask about (`W-SL-2`): a function runtime's deadline, an edge sandbox's
/// memory and response ceilings. `None` is "no stated limit".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostLimits {
    /// How long the request may take, from when it arrived.
    pub deadline: Option<Duration>,
    /// Memory available to the request's work, in bytes.
    pub memory: Option<u64>,
    /// Whether a writable file system is available.
    pub filesystem: bool,
    /// The largest response body, in bytes.
    pub response_bytes: Option<u64>,
    /// The largest request body, in bytes.
    pub payload_bytes: Option<u64>,
}

/// The request the render answers, or `None` when there is none — a static
/// export, or the shell of a partially prerendered page. Asking marks the
/// component dynamic.
#[must_use]
pub fn request<M: Send + 'static>(context: &ComponentContext<'_, M>) -> Option<Arc<RequestInfo>> {
    if let Some(render) = context.services().extension::<ServerRender>() {
        render.note_dynamic(context.id());
    }
    context.services().extension::<RequestInfo>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parameters_are_decoded() {
        let request = RequestInfo::get("/search?q=caf%C3%A9+au+lait&empty&bad=%zz");
        assert_eq!(request.path, "/search");
        assert_eq!(request.query_param("q").as_deref(), Some("café au lait"));
        assert_eq!(request.query_param("empty").as_deref(), Some(""));
        assert_eq!(request.query_param("bad").as_deref(), Some("%zz"));
        assert_eq!(request.query_param("missing"), None);
    }
}
