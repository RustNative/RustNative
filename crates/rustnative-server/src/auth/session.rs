//! Sessions in an encrypted, authenticated cookie (AES-256-GCM): the
//! server keeps nothing, a client can neither read nor forge its session,
//! and the cookie is `Secure; HttpOnly; SameSite=Lax`.
//!
//! Two things a stateless cookie cannot do by itself are provided:
//!
//! - **Key rotation.** [`Sessions::with_previous_keys`] accepts cookies
//!   sealed with older keys and reseals them with the current one on the
//!   next response, so a key can be replaced without signing everyone out.
//! - **Revocation.** Every session has an id and the time it was issued
//!   ([`Session::id`], [`Session::issued`]); [`Sessions::with_revocation`]
//!   asks the application, per request, whether a session is still valid —
//!   a deleted account, "sign out everywhere", a stolen device — against
//!   whatever it records, and a revoked cookie opens as no session.

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
    /// The session's id: random, assigned when it is first sealed.
    #[serde(default)]
    id: String,
    /// When it was first sealed, seconds since the Unix epoch.
    #[serde(default)]
    issued: u64,
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

    /// The session's id, once it has been issued (empty for a session this
    /// request started) — what a revocation record names.
    #[must_use]
    pub fn id(&self) -> String {
        self.data.lock().unwrap_or_else(PoisonError::into_inner).id.clone()
    }

    /// When the session was issued, seconds since the Unix epoch (0 for a
    /// session this request started) — what "sign out everywhere before
    /// this time" compares against.
    #[must_use]
    pub fn issued(&self) -> u64 {
        self.data.lock().unwrap_or_else(PoisonError::into_inner).issued
    }

    /// Issues a new id for the session, keeping its values — what an
    /// application does when the person signs in, so an id observed before
    /// is not the one that is now signed in.
    pub fn renew(&self) {
        let mut data = self.data.lock().unwrap_or_else(PoisonError::into_inner);
        data.id.clear();
        data.issued = 0;
        *self.changed.lock().unwrap_or_else(PoisonError::into_inner) = true;
    }
}

/// What a revocation check is told about a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    /// Its id.
    pub id: String,
    /// When it was issued, seconds since the Unix epoch.
    pub issued: u64,
    /// Its values, as JSON text by key — for a check keyed by user.
    pub values: BTreeMap<String, String>,
}

/// Whether a session has been revoked.
pub type Revocation = Arc<dyn Fn(&SessionInfo) -> bool + Send + Sync>;

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
    previous: Arc<Vec<Aes256Gcm>>,
    lifetime: Duration,
    revoked: Option<Revocation>,
}

impl Sessions {
    /// Sessions encrypted with `key`, lasting `lifetime` from their last
    /// change.
    #[must_use]
    pub fn new(key: &Secret<[u8; 32]>, lifetime: Duration) -> Self {
        Self {
            cipher: Arc::new(Aes256Gcm::new(key.expose().into())),
            previous: Arc::new(Vec::new()),
            lifetime,
            revoked: None,
        }
    }

    /// Also accepts cookies sealed with `keys` — the keys this one
    /// replaced — resealing them with the current key on their next
    /// response. Drop an old key once every session it sealed has expired.
    #[must_use]
    pub fn with_previous_keys(mut self, keys: &[Secret<[u8; 32]>]) -> Self {
        self.previous =
            Arc::new(keys.iter().map(|key| Aes256Gcm::new(key.expose().into())).collect());
        self
    }

    /// Asks `revoked` whether each session opened is revoked; a revoked one
    /// opens as no session at all.
    #[must_use]
    pub fn with_revocation(mut self, revoked: Revocation) -> Self {
        self.revoked = Some(revoked);
        self
    }

    fn now() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
    }

    /// Decrypts a cookie value: the data, and whether it was sealed with a
    /// previous key (so it should be resealed). `None` for anything forged,
    /// tampered, expired, or revoked.
    fn open(&self, sealed: &str) -> Option<(Data, bool)> {
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(sealed).ok()?;
        let (nonce, ciphertext) = bytes.split_first_chunk::<12>()?;
        let nonce = Nonce::from(*nonce);
        let (plain, rotated) = match self.cipher.decrypt(&nonce, ciphertext) {
            Ok(plain) => (plain, false),
            Err(_) => (
                self.previous.iter().find_map(|cipher| cipher.decrypt(&nonce, ciphertext).ok())?,
                true,
            ),
        };
        let data: Data = serde_json::from_slice(&plain).ok()?;
        if data.expires <= Self::now() {
            return None;
        }
        if let Some(revoked) = &self.revoked {
            let info = SessionInfo {
                id: data.id.clone(),
                issued: data.issued,
                values: data.values.clone(),
            };
            if revoked(&info) {
                return None;
            }
        }
        Some((data, rotated))
    }

    fn seal(&self, data: &Data) -> Option<String> {
        let mut nonce = [0u8; 12];
        getrandom::getrandom(&mut nonce).ok()?;
        let plain = serde_json::to_vec(data).ok()?;
        let mut sealed = nonce.to_vec();
        sealed.extend(self.cipher.encrypt(&Nonce::from(nonce), plain.as_slice()).ok()?);
        Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sealed))
    }

    /// The middleware pair: attach the session before routing, write it
    /// back after the handler if it changed.
    #[must_use]
    pub fn middleware(self) -> (crate::app::Before, crate::app::After) {
        let reader = self.clone();
        let before: crate::app::Before = Arc::new(move |request: &mut RequestContext| {
            let (data, rotated) =
                request.cookie(COOKIE).and_then(|sealed| reader.open(sealed)).unwrap_or_default();
            request.insert(Session {
                data: Arc::new(Mutex::new(data)),
                changed: Arc::new(Mutex::new(rotated)),
            });
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
                    let now = Self::now();
                    data.expires = now + self.lifetime.as_secs();
                    if data.id.is_empty() {
                        data.id = crate::security::random_token(16);
                        data.issued = now;
                    }
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
