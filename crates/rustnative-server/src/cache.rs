//! Response caching with tag-based invalidation (incremental
//! regeneration): a public `GET` route declared `cached` keeps its
//! rendered response until it expires or a tag it carries is invalidated
//! — `cache.invalidate("notes")` when notes change regenerates every page
//! that shows them on its next request. The cache's state is queryable
//! (`ServerApp::cache_inspection`, behind a policy).
//!
//! Only public routes are cached, and a cached response never carries a
//! cookie: what is stored is the handler's output, before the pipeline
//! adds anything specific to one visitor.

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
#[derive(Clone, Default)]
pub struct ResponseCache {
    entries: Arc<Mutex<HashMap<String, Entry>>>,
}

/// One cached response, as inspection lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CachedEntry {
    /// The request path and query.
    pub key: String,
    /// Its tags.
    pub tags: Vec<String>,
    /// Seconds until it expires.
    pub expires_in: u64,
    /// How often it was served from the cache.
    pub hits: u64,
}

impl ResponseCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
        {
            return;
        }
        let entry = Entry {
            response: response.clone(),
            tags: tags.to_vec(),
            expires: Instant::now() + ttl,
            hits: 0,
        };
        self.entries.lock().unwrap_or_else(PoisonError::into_inner).insert(key, entry);
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
                key: key.clone(),
                tags: entry.tags.iter().map(|tag| (*tag).to_owned()).collect(),
                expires_in: entry.expires.saturating_duration_since(now).as_secs(),
                hits: entry.hits,
            })
            .collect();
        listed.sort_by(|a, b| a.key.cmp(&b.key));
        listed
    }
}
