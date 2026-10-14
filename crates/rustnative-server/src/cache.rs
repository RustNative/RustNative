//! Response caching with tag-based invalidation (incremental
//! regeneration): a public `GET` route declared `cached` keeps its
//! rendered response until it expires or a tag it carries is invalidated
//! — `cache.invalidate("notes")` when notes change regenerates every page
//! that shows them on its next request. The cache's state is queryable
//! (`ServerApp::cache_inspection`, behind a policy).
//!
//! Only public routes are cached, and a cached response never carries a
//! cookie: what is stored is the handler's output, before the pipeline
//! adds anything specific to one visitor. A response that says it must not
//! be shared (`Cache-Control: no-store` or `private`) is never stored —
//! which is every rendered page, because a page carries its request's
//! content-security-policy nonce and request-forgery token. A page's
//! shareable part is cached through its prerendered shell instead
//! (`Strategy::PartiallyPrerendered`).
//!
//! The key is the path and query and the request headers a response may
//! vary by — the languages and the formats the client accepts — so one
//! visitor's language is never served to another. The cache holds at most
//! [`ResponseCache::DEFAULT_CAPACITY`] entries; past that the entry nearest
//! its expiry is dropped first.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::response::Response;

#[derive(Clone)]
struct Entry {
    response: Response,
    tags: Vec<&'static str>,
    expires: Instant,
    hits: u64,
}

/// The cache; cloning shares it.
#[derive(Clone)]
pub struct ResponseCache {
    entries: Arc<Mutex<HashMap<String, Entry>>>,
    capacity: usize,
}

impl Default for ResponseCache {
    fn default() -> Self {
        Self { entries: Arc::default(), capacity: Self::DEFAULT_CAPACITY }
    }
}

/// The cache key for a request: its path and query and the headers a
/// response may vary by (see the module documentation).
#[must_use]
pub(crate) fn key(path: &str, query: &str, headers: &http::HeaderMap) -> String {
    let header = |name: http::header::HeaderName| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .trim()
            .to_owned()
    };
    format!(
        "{path}?{query}\u{0}{}\u{0}{}",
        header(http::header::ACCEPT_LANGUAGE),
        header(http::header::ACCEPT)
    )
}

/// One cached response, as inspection lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CachedEntry {
    /// The request path and query.
    pub key: String,
    /// The varying request headers' values it was stored for: the accepted
    /// languages and formats (empty where the request had none).
    pub vary: Vec<String>,
    /// Its tags.
    pub tags: Vec<String>,
    /// Seconds until it expires.
    pub expires_in: u64,
    /// How often it was served from the cache.
    pub hits: u64,
}

impl ResponseCache {
    /// How many responses a cache holds unless told otherwise.
    pub const DEFAULT_CAPACITY: usize = 1024;

    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty cache holding at most `capacity` responses.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self { entries: Arc::default(), capacity: capacity.max(1) }
    }

    /// A fresh cached response for `key`.
    pub(crate) fn get(&self, key: &str) -> Option<Response> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = entries.get_mut(key)?;
        if entry.expires <= Instant::now() {
            entries.remove(key);
            return None;
        }
        entry.hits += 1;
        Some(entry.response.clone())
    }

    /// Stores a response.
    pub(crate) fn put(
        &self,
        key: String,
        response: &Response,
        tags: &[&'static str],
        ttl: Duration,
    ) {
        if !response.status().is_success()
            || response.headers().contains_key(http::header::SET_COOKIE)
            || !shareable(response)
        {
            return;
        }
        let now = Instant::now();
        let entry =
            Entry { response: response.clone(), tags: tags.to_vec(), expires: now + ttl, hits: 0 };
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if entries.len() >= self.capacity && !entries.contains_key(&key) {
            entries.retain(|_, entry| entry.expires > now);
            if entries.len() >= self.capacity {
                let soonest = entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.expires)
                    .map(|(key, _)| key.clone());
                if let Some(soonest) = soonest {
                    entries.remove(&soonest);
                }
            }
        }
        entries.insert(key, entry);
    }

    /// How many responses are cached.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    /// Whether nothing is cached.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drops every response carrying `tag`; returns how many.
    pub fn invalidate(&self, tag: &str) -> usize {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let before = entries.len();
        entries.retain(|_, entry| !entry.tags.contains(&tag));
        before - entries.len()
    }

    /// What is cached.
    #[must_use]
    pub fn entries(&self) -> Vec<CachedEntry> {
        let now = Instant::now();
        let mut listed: Vec<CachedEntry> = self
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(key, entry)| CachedEntry {
                key: key.split('\u{0}').next().unwrap_or_default().to_owned(),
                vary: key.split('\u{0}').skip(1).map(str::to_owned).collect(),
                tags: entry.tags.iter().map(|tag| (*tag).to_owned()).collect(),
                expires_in: entry.expires.saturating_duration_since(now).as_secs(),
                hits: entry.hits,
            })
            .collect();
        listed.sort_by(|a, b| a.key.cmp(&b.key));
        listed
    }
}

/// Whether `response` may be served to someone other than the request it
/// answered: not `no-store`, not `private`.
fn shareable(response: &Response) -> bool {
    response.headers().get_all(http::header::CACHE_CONTROL).iter().all(|value| {
        value.to_str().map_or(true, |value| {
            !value
                .split(',')
                .map(|part| part.trim().to_ascii_lowercase())
                .any(|directive| directive == "no-store" || directive == "private")
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(cache_control: Option<&'static str>) -> Response {
        let mut response = http::Response::new(bytes::Bytes::from_static(b"body"));
        if let Some(value) = cache_control {
            response
                .headers_mut()
                .insert(http::header::CACHE_CONTROL, http::HeaderValue::from_static(value));
        }
        response
    }

    #[test]
    fn a_response_that_must_not_be_shared_is_not_stored() {
        let cache = ResponseCache::new();
        cache.put("page".into(), &response(Some("no-store")), &[], Duration::from_secs(60));
        cache.put(
            "mine".into(),
            &response(Some("max-age=0, private")),
            &[],
            Duration::from_secs(60),
        );
        cache.put(
            "public".into(),
            &response(Some("public, max-age=60")),
            &[],
            Duration::from_secs(60),
        );
        assert_eq!(cache.len(), 1);
        assert!(cache.get("public").is_some());
    }

    #[test]
    fn the_key_varies_by_language_and_format() {
        let mut english = http::HeaderMap::new();
        english.insert(http::header::ACCEPT_LANGUAGE, http::HeaderValue::from_static("en"));
        let mut german = http::HeaderMap::new();
        german.insert(http::header::ACCEPT_LANGUAGE, http::HeaderValue::from_static("de"));
        assert_ne!(key("/notes", "", &english), key("/notes", "", &german));
        assert_eq!(key("/notes", "", &english), key("/notes", "", &english.clone()));
    }

    #[test]
    fn the_cache_is_bounded() {
        let cache = ResponseCache::with_capacity(3);
        for index in 0..10_u64 {
            cache.put(index.to_string(), &response(None), &[], Duration::from_secs(60 + index));
        }
        assert_eq!(cache.len(), 3);
        assert!(cache.get("9").is_some(), "the newest stays");
        assert!(cache.get("0").is_none(), "the one nearest expiry went first");
    }
}
