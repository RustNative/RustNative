//! Sessions in an encrypted, authenticated cookie (AES-256-GCM): the
//! server keeps nothing, a client can neither read nor forge its session,
//! and the cookie is `Secure; HttpOnly; SameSite=Lax`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use http::{HeaderValue, header};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::config::Secret;
use crate::request::{FromRequest, RequestContext};
use crate::response::{Response, ServerError};
use crate::security::Cookie;

/// The session cookie's name.
pub const COOKIE: &str = "__Host-session";

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
struct Data {
    values: BTreeMap<String, String>,
    expires: u64,
}

/// The request's session; cloning shares it.
#[derive(Debug, Clone, Default)]
pub struct Session {
    data: Arc<Mutex<Data>>,
    changed: Arc<Mutex<bool>>,
}

impl Session {
    /// A value, if set and of type `T`.
    #[must_use]
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let data = self.data.lock().unwrap_or_else(PoisonError::into_inner);
        serde_json::from_str(data.values.get(key)?).ok()
    }

    /// Sets a value.
    pub fn set<T: Serialize>(&self, key: &str, value: &T) {
        if let Ok(text) = serde_json::to_string(value) {
            self.data
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .values
                .insert(key.to_owned(), text);
            *self.changed.lock().unwrap_or_else(PoisonError::into_inner) = true;
        }
    }

    /// Removes a value.
    pub fn remove(&self, key: &str) {
        self.data.lock().unwrap_or_else(PoisonError::into_inner).values.remove(key);
        *self.changed.lock().unwrap_or_else(PoisonError::into_inner) = true;
    }

    /// Ends the session (signing out).
    pub fn clear(&self) {
        self.data.lock().unwrap_or_else(PoisonError::into_inner).values.clear();
        *self.changed.lock().unwrap_or_else(PoisonError::into_inner) = true;
    }
}

impl FromRequest for Session {
    fn from_request(request: &RequestContext) -> Result<Self, ServerError> {
        request
            .value::<Self>()
            .cloned()
            .ok_or_else(|| ServerError::internal("sessions are not set up"))
    }
}

/// The session middleware, keyed by a 32-byte secret.
#[derive(Clone)]
pub struct Sessions {
    cipher: Arc<Aes256Gcm>,
    lifetime: Duration,
}

impl Sessions {
    /// Sessions encrypted with `key`, lasting `lifetime` from their last
    /// change.
    #[must_use]
    pub fn new(key: &Secret<[u8; 32]>, lifetime: Duration) -> Self {
        Self { cipher: Arc::new(Aes256Gcm::new(key.expose().into())), lifetime }
    }

    fn now() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
    }

    /// Decrypts a cookie value; `None` for anything forged, tampered, or
    /// expired.
    fn open(&self, sealed: &str) -> Option<Data> {
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(sealed).ok()?;
        if bytes.len() < 12 {
            return None;
        }
        let (nonce, ciphertext) = bytes.split_at(12);
        let plain = self.cipher.decrypt(Nonce::from_slice(nonce), ciphertext).ok()?;
        let data: Data = serde_json::from_slice(&plain).ok()?;
        (data.expires > Self::now()).then_some(data)
    }

    fn seal(&self, data: &Data) -> Option<String> {
        let mut nonce = [0u8; 12];
        getrandom::getrandom(&mut nonce).ok()?;
        let plain = serde_json::to_vec(data).ok()?;
        let mut sealed = nonce.to_vec();
        sealed.extend(self.cipher.encrypt(Nonce::from_slice(&nonce), plain.as_slice()).ok()?);
        Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sealed))
    }

    /// The middleware pair: attach the session before routing, write it
    /// back after the handler if it changed.
    #[must_use]
    pub fn middleware(self) -> (crate::app::Before, crate::app::After) {
        let reader = self.clone();
        let before: crate::app::Before = Arc::new(move |request: &mut RequestContext| {
            let data =
                request.cookie(COOKIE).and_then(|sealed| reader.open(sealed)).unwrap_or_default();
            request.insert(Session { data: Arc::new(Mutex::new(data)), changed: Arc::default() });
            Ok(())
        });
        let after: crate::app::After =
            Arc::new(move |request: &RequestContext, response: &mut Response| {
                let Some(session) = request.value::<Session>() else { return };
                if !*session.changed.lock().unwrap_or_else(PoisonError::into_inner) {
                    return;
                }
                let mut data = session.data.lock().unwrap_or_else(PoisonError::into_inner).clone();
                let cookie = if data.values.is_empty() {
                    Cookie::removal(COOKIE)
                } else {
                    data.expires = Self::now() + self.lifetime.as_secs();
                    let Some(sealed) = self.seal(&data) else { return };
                    Cookie::new(COOKIE, sealed).max_age(self.lifetime)
                };
                if let Ok(value) = HeaderValue::from_str(&cookie.to_string()) {
                    response.headers_mut().append(header::SET_COOKIE, value);
                }
            });
        (before, after)
    }
}
