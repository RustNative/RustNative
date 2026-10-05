//! Server-side push sending (`C54-1`).
//!
//! - **Web Push** (RFC 8030/8291/8292): the payload encrypted for the
//!   subscription (`aes128gcm`, ECDH P-256 and HKDF), authorized with a
//!   VAPID token (ES256 JWT), sent through the `HttpService` contract.
//! - **WNS, APNs, FCM**: the requests each service expects, built from
//!   credentials the application obtains (an OAuth token for WNS and FCM,
//!   a provider JWT for APNs). Sending them needs those credentials, so the
//!   builders are tested against the services' documented formats.

use std::time::{SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use base64::Engine;
use hkdf::Hkdf;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey, SecretKey};
use rustnative_core::{HttpRequest, HttpResponse, HttpService, Method, ServiceError};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::config::Secret;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// A browser's push subscription, as `PushSubscription.toJSON()` gives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    /// The push service endpoint.
    pub endpoint: String,
    /// The keys.
    pub keys: SubscriptionKeys,
}

/// A subscription's keys, base64url.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionKeys {
    /// The user agent's public key (65 bytes, uncompressed).
    pub p256dh: String,
    /// The authentication secret (16 bytes).
    pub auth: String,
}

/// Why a push could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushError(pub String);

impl std::fmt::Display for PushError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "push: {}", self.0)
    }
}

impl std::error::Error for PushError {}

fn error(message: impl Into<String>) -> PushError {
    PushError(message.into())
}

fn random<const N: usize>() -> Result<[u8; N], PushError> {
    let mut bytes = [0u8; N];
    getrandom::getrandom(&mut bytes).map_err(|failure| error(failure.to_string()))?;
    Ok(bytes)
}

/// Encrypts `payload` for a subscriber (RFC 8291), with the given
/// ephemeral key and salt: the `aes128gcm` body.
///
/// # Errors
///
/// The subscriber's keys are malformed.
pub fn encrypt_with(
    payload: &[u8],
    ua_public: &[u8],
    auth_secret: &[u8],
    ephemeral: &SecretKey,
    salt: [u8; 16],
) -> Result<Vec<u8>, PushError> {
    let ua_key =
        PublicKey::from_sec1_bytes(ua_public).map_err(|_| error("the subscriber's key"))?;
    let as_public = ephemeral.public_key().to_encoded_point(false);
    let shared = p256::ecdh::diffie_hellman(ephemeral.to_nonzero_scalar(), ua_key.as_affine());

    let mut info = b"WebPush: info\0".to_vec();
    info.extend_from_slice(ua_public);
    info.extend_from_slice(as_public.as_bytes());
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(auth_secret), shared.raw_secret_bytes())
        .expand(&info, &mut ikm)
        .map_err(|_| error("key derivation"))?;
    let prk = Hkdf::<Sha256>::new(Some(&salt), &ikm);
    let mut key = [0u8; 16];
    let mut nonce = [0u8; 12];
    prk.expand(b"Content-Encoding: aes128gcm\0", &mut key).map_err(|_| error("key derivation"))?;
    prk.expand(b"Content-Encoding: nonce\0", &mut nonce).map_err(|_| error("key derivation"))?;

    let mut plain = payload.to_vec();
    plain.push(0x02); // The last (and only) record's delimiter.
    let cipher = Aes128Gcm::new(&key.into());
    let sealed =
        cipher.encrypt(&Nonce::from(nonce), plain.as_slice()).map_err(|_| error("encryption"))?;

    let mut body = salt.to_vec();
    body.extend_from_slice(&4096u32.to_be_bytes());
    body.push(65);
    body.extend_from_slice(as_public.as_bytes());
    body.extend(sealed);
    Ok(body)
}

/// A VAPID identity: the application server's signing key and contact.
#[derive(Clone)]
pub struct Vapid {
    key: Secret<[u8; 32]>,
    subject: String,
}

impl Vapid {
    /// A VAPID identity from its 32-byte private key and a `mailto:` or
    /// `https:` contact.
    #[must_use]
    pub fn new(key: Secret<[u8; 32]>, subject: impl Into<String>) -> Self {
        Self { key, subject: subject.into() }
    }

    fn signing_key(&self) -> Result<SigningKey, PushError> {
        SigningKey::from_slice(self.key.expose()).map_err(|_| error("the VAPID key"))
    }

    /// The public key a browser subscribes with (`applicationServerKey`),
    /// base64url.
    ///
    /// # Errors
    ///
    /// The private key is not a P-256 scalar.
    pub fn public_key(&self) -> Result<String, PushError> {
        Ok(B64.encode(self.signing_key()?.verifying_key().to_encoded_point(false).as_bytes()))
    }

    /// The `Authorization` header for a push to `endpoint` (RFC 8292).
    ///
    /// # Errors
    ///
    /// The key is unusable or the endpoint is not a URL.
    pub fn authorization(&self, endpoint: &str) -> Result<String, PushError> {
        let origin = endpoint.split('/').take(3).collect::<Vec<_>>().join("/");
        if !origin.starts_with("https://") {
            return Err(error("the endpoint is not an https URL"));
        }
        let expires =
            SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
                + 12 * 3600;
        let header = B64.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims = B64.encode(
            serde_json::json!({ "aud": origin, "exp": expires, "sub": self.subject }).to_string(),
        );
        let signing_input = format!("{header}.{claims}");
        let signature: Signature = self.signing_key()?.sign(signing_input.as_bytes());
        let jwt = format!("{signing_input}.{}", B64.encode(signature.to_bytes()));
        Ok(format!("vapid t={jwt}, k={}", self.public_key()?))
    }

    /// The request that delivers `payload` to `subscription`, valid for
    /// `ttl` seconds.
    ///
    /// # Errors
    ///
    /// The subscription or key is malformed.
    pub fn request(
        &self,
        subscription: &Subscription,
        payload: &[u8],
        ttl: u32,
    ) -> Result<HttpRequest, PushError> {
        let ua_public = B64
            .decode(subscription.keys.p256dh.trim_end_matches('='))
            .map_err(|_| error("p256dh"))?;
        let auth =
            B64.decode(subscription.keys.auth.trim_end_matches('=')).map_err(|_| error("auth"))?;
        let ephemeral =
            SecretKey::from_slice(&random::<32>()?).map_err(|_| error("an ephemeral key"))?;
        let body = encrypt_with(payload, &ua_public, &auth, &ephemeral, random::<16>()?)?;
        Ok(HttpRequest::new(Method::Post, subscription.endpoint.clone())
            .header("authorization", self.authorization(&subscription.endpoint)?)
            .header("content-encoding", "aes128gcm")
            .header("content-type", "application/octet-stream")
            .header("ttl", ttl.to_string())
            .body(body))
    }

    /// Sends `payload` to `subscription`. A `404` or `410` means the
    /// subscription is gone: delete it.
    ///
    /// # Errors
    ///
    /// The request could not be built or sent.
    pub async fn send(
        &self,
        http: &dyn HttpService,
        subscription: &Subscription,
        payload: &[u8],
    ) -> Result<HttpResponse, PushError> {
        let request = self.request(subscription, payload, 24 * 3600)?;
        http.execute(request).await.map_err(|failure: ServiceError| error(failure.to_string()))
    }
}

/// A Windows Push Notification Services request: a raw notification to
/// `channel_uri`, authorized by the access token WNS issued the app.
#[must_use]
pub fn wns_request(
    channel_uri: &str,
    access_token: &Secret<String>,
    payload: &[u8],
) -> HttpRequest {
    HttpRequest::new(Method::Post, channel_uri)
        .header("authorization", format!("Bearer {}", access_token.expose()))
        .header("x-wns-type", "wns/raw")
        .header("content-type", "application/octet-stream")
        .body(payload.to_vec())
}

/// An Apple Push Notification service request (HTTP/2) to `device_token`
/// for the app `topic`, authorized by a provider token (ES256 JWT).
#[must_use]
pub fn apns_request(
    device_token: &str,
    topic: &str,
    provider_token: &Secret<String>,
    alert: &str,
) -> HttpRequest {
    let body = serde_json::json!({ "aps": { "alert": alert } }).to_string();
    HttpRequest::new(Method::Post, format!("https://api.push.apple.com/3/device/{device_token}"))
        .header("authorization", format!("bearer {}", provider_token.expose()))
        .header("apns-topic", topic)
        .header("apns-push-type", "alert")
        .header("content-type", "application/json")
        .body(body.into_bytes())
}

/// A Firebase Cloud Messaging (HTTP v1) request for `project` to a
/// registration `token`.
#[must_use]
pub fn fcm_request(
    project: &str,
    token: &str,
    access_token: &Secret<String>,
    title: &str,
    body: &str,
) -> HttpRequest {
    let message = serde_json::json!({ "message": { "token": token, "notification": { "title": title, "body": body } } });
    HttpRequest::new(
        Method::Post,
        format!("https://fcm.googleapis.com/v1/projects/{project}/messages:send"),
    )
    .header("authorization", format!("Bearer {}", access_token.expose()))
    .header("content-type", "application/json")
    .body(message.to_string().into_bytes())
}
