//! Signed bearer tokens: `payload.signature`, the payload a subject and an
//! expiry, the signature HMAC-SHA256 under the server's key. Stateless;
//! revoke by rotating the key or keeping expiries short.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::config::Secret;

/// What a token says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    /// Who it is for (the principal, serialized).
    pub subject: String,
    /// When it expires, in seconds since the Unix epoch.
    pub expires: u64,
}

/// Issues and verifies tokens.
#[derive(Clone)]
pub struct TokenSigner {
    key: Secret<Vec<u8>>,
}

type HmacSha256 = Hmac<Sha256>;

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

impl TokenSigner {
    /// A signer with `key` (at least 32 bytes of randomness).
    #[must_use]
    pub const fn new(key: Secret<Vec<u8>>) -> Self {
        Self { key }
    }

    fn sign(&self, payload: &str) -> Option<Vec<u8>> {
        let mut mac = HmacSha256::new_from_slice(self.key.expose()).ok()?;
        mac.update(payload.as_bytes());
        Some(mac.finalize().into_bytes().to_vec())
    }

    /// A token for `subject`, valid for `lifetime`.
    #[must_use]
    pub fn issue(&self, subject: impl Into<String>, lifetime: Duration) -> String {
        let engine = &base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let claims = Claims { subject: subject.into(), expires: now() + lifetime.as_secs() };
        let payload = engine.encode(serde_json::to_vec(&claims).unwrap_or_default());
        let signature = engine.encode(self.sign(&payload).unwrap_or_default());
        format!("{payload}.{signature}")
    }

    /// The claims of a genuine, unexpired token.
    #[must_use]
    pub fn verify(&self, token: &str) -> Option<Claims> {
        let engine = &base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let (payload, signature) = token.split_once('.')?;
        let expected = self.sign(payload)?;
        let given = engine.decode(signature).ok()?;
        if !crate::security::constant_time_eq(&expected, &given) {
            return None;
        }
        let claims: Claims = serde_json::from_slice(&engine.decode(payload).ok()?).ok()?;
        (claims.expires > now()).then_some(claims)
    }
}
