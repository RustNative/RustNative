//! Passkeys (`WebAuthn`, `C52-2`): registration stores a credential's
//! public key; sign-in verifies an assertion signed by it.
//!
//! The server side of the ceremony, for ES256 credentials (the algorithm
//! every platform authenticator supports) with `none` attestation (what
//! passkeys send):
//!
//! 1. [`Passkeys::challenge`] makes a single-use challenge; keep it in the
//!    session and send it to the browser or app.
//! 2. Registration: [`Passkeys::register`] checks the client data (type,
//!    challenge, origin), the relying party, and user presence, and returns
//!    the [`Credential`] to store.
//! 3. Sign-in: [`Passkeys::verify`] checks the same, then the signature
//!    over `authenticatorData || SHA-256(clientDataJSON)` with the stored
//!    key, and that the signature counter moved forward (a cloned
//!    authenticator is refused).

use base64::Engine;
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A registered credential, to store with its user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credential {
    /// The credential's id, as the authenticator reports it.
    pub id: Vec<u8>,
    /// Its public key, an uncompressed SEC1 point.
    pub public_key: Vec<u8>,
    /// The last signature counter seen.
    pub sign_count: u32,
}

/// Why a ceremony was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasskeyError(pub &'static str);

impl std::fmt::Display for PasskeyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "passkey refused: {}", self.0)
    }
}

impl std::error::Error for PasskeyError {}

/// The relying party: its id (a domain) and the origin pages come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passkeys {
    rp_id: String,
    origin: String,
}

#[derive(Deserialize)]
struct ClientData {
    #[serde(rename = "type")]
    kind: String,
    challenge: String,
    origin: String,
}

const USER_PRESENT: u8 = 0x01;
const ATTESTED: u8 = 0x40;

fn fail<T>(reason: &'static str) -> Result<T, PasskeyError> {
    Err(PasskeyError(reason))
}

impl Passkeys {
    /// The relying party `rp_id` (`example.com`) serving `origin`
    /// (`https://example.com`).
    #[must_use]
    pub fn new(rp_id: impl Into<String>, origin: impl Into<String>) -> Self {
        Self { rp_id: rp_id.into(), origin: origin.into() }
    }

    /// A fresh challenge, base64url, to keep in the session and send.
    #[must_use]
    pub fn challenge() -> String {
        crate::security::random_token(32)
    }

    fn check_client(
        &self,
        client_data: &[u8],
        kind: &str,
        challenge: &str,
    ) -> Result<(), PasskeyError> {
        let data: ClientData =
            serde_json::from_slice(client_data).map_err(|_| PasskeyError("client data"))?;
        if data.kind != kind {
            return fail("wrong ceremony");
        }
        if !crate::security::constant_time_eq(data.challenge.as_bytes(), challenge.as_bytes()) {
            return fail("challenge");
        }
        if data.origin != self.origin {
            return fail("origin");
        }
        Ok(())
    }

    fn check_authenticator(&self, auth_data: &[u8]) -> Result<(u8, u32), PasskeyError> {
        if auth_data.len() < 37 {
            return fail("authenticator data");
        }
        let rp_hash = Sha256::digest(self.rp_id.as_bytes());
        if auth_data[..32] != rp_hash[..] {
            return fail("relying party");
        }
        let flags = auth_data[32];
        if flags & USER_PRESENT == 0 {
            return fail("user not present");
        }
        let count =
            u32::from_be_bytes([auth_data[33], auth_data[34], auth_data[35], auth_data[36]]);
        Ok((flags, count))
    }

    /// Checks a registration (`clientDataJSON`, `attestationObject`)
    /// against `challenge`, and returns the credential to store.
    ///
    /// # Errors
    ///
    /// Anything that does not check out.
    pub fn register(
        &self,
        client_data: &[u8],
        attestation_object: &[u8],
        challenge: &str,
    ) -> Result<Credential, PasskeyError> {
        self.check_client(client_data, "webauthn.create", challenge)?;
        let (object, _) = cbor::read(attestation_object).ok_or(PasskeyError("attestation"))?;
        let auth_data = object
            .get_text("authData")
            .and_then(cbor::Value::bytes)
            .ok_or(PasskeyError("authData"))?;
        let (flags, sign_count) = self.check_authenticator(auth_data)?;
        if flags & ATTESTED == 0 || auth_data.len() < 55 {
            return fail("no credential");
        }
        let id_length = usize::from(u16::from_be_bytes([auth_data[53], auth_data[54]]));
        let id_end = 55 + id_length;
        let id = auth_data.get(55..id_end).ok_or(PasskeyError("credential id"))?.to_vec();
        let (key, _) = cbor::read(&auth_data[id_end..]).ok_or(PasskeyError("public key"))?;
        // COSE: kty 2 (EC2), alg -7 (ES256), crv 1 (P-256), x -2, y -3.
        if key.get_int(1).and_then(cbor::Value::int) != Some(2)
            || key.get_int(3).and_then(cbor::Value::int) != Some(-7)
        {
            return fail("only ES256 credentials");
        }
        let x = key.get_int(-2).and_then(cbor::Value::bytes).ok_or(PasskeyError("x"))?;
        let y = key.get_int(-3).and_then(cbor::Value::bytes).ok_or(PasskeyError("y"))?;
        let mut public_key = vec![0x04];
        public_key.extend_from_slice(x);
        public_key.extend_from_slice(y);
        VerifyingKey::from_sec1_bytes(&public_key).map_err(|_| PasskeyError("public key"))?;
        Ok(Credential { id, public_key, sign_count })
    }

    /// Checks a sign-in assertion against `challenge` and the stored
    /// `credential`, whose counter it advances.
    ///
    /// # Errors
    ///
    /// Anything that does not check out, including a counter that did not
    /// move forward.
    pub fn verify(
        &self,
        credential: &mut Credential,
        client_data: &[u8],
        auth_data: &[u8],
        signature: &[u8],
        challenge: &str,
    ) -> Result<(), PasskeyError> {
        self.check_client(client_data, "webauthn.get", challenge)?;
        let (_, sign_count) = self.check_authenticator(auth_data)?;
        let key = VerifyingKey::from_sec1_bytes(&credential.public_key)
            .map_err(|_| PasskeyError("stored key"))?;
        let signature = Signature::from_der(signature).map_err(|_| PasskeyError("signature"))?;
        let mut signed = auth_data.to_vec();
        signed.extend_from_slice(&Sha256::digest(client_data));
        key.verify(&signed, &signature).map_err(|_| PasskeyError("signature"))?;
        if (sign_count != 0 || credential.sign_count != 0) && sign_count <= credential.sign_count {
            return fail("counter went backwards: a cloned authenticator");
        }
        credential.sign_count = sign_count;
        Ok(())
    }

    /// base64url-decodes what a browser sends.
    ///
    /// # Errors
    ///
    /// It is not base64url.
    pub fn decode(text: &str) -> Result<Vec<u8>, PasskeyError> {
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(text.trim_end_matches('='))
            .map_err(|_| PasskeyError("base64"))
    }
}

/// The subset of CBOR `WebAuthn` uses.
pub(crate) mod cbor {
    /// A decoded item.
    #[derive(Debug, Clone, PartialEq)]
    pub(crate) enum Value<'a> {
        Int(i64),
        Bytes(&'a [u8]),
        Text(&'a str),
        Array(Vec<Value<'a>>),
        Map(Vec<(Value<'a>, Value<'a>)>),
        Other,
    }

    impl<'a> Value<'a> {
        pub(crate) const fn int(&self) -> Option<i64> {
            if let Self::Int(value) = self { Some(*value) } else { None }
        }

        pub(crate) const fn bytes(&self) -> Option<&'a [u8]> {
            if let Self::Bytes(value) = self { Some(value) } else { None }
        }

        fn entry(&self, key: &Value<'_>) -> Option<&Value<'a>> {
            let Self::Map(entries) = self else { return None };
            entries.iter().find(|(candidate, _)| candidate == key).map(|(_, value)| value)
        }

        pub(crate) fn get_text(&self, key: &str) -> Option<&Value<'a>> {
            self.entry(&Value::Text(key))
        }

        pub(crate) fn get_int(&self, key: i64) -> Option<&Value<'a>> {
            self.entry(&Value::Int(key))
        }
    }

    fn argument(input: &[u8], info: u8) -> Option<(u64, usize)> {
        Some(match info {
            0..=23 => (u64::from(info), 0),
            24 => (u64::from(*input.first()?), 1),
            25 => (u64::from(u16::from_be_bytes(input.get(..2)?.try_into().ok()?)), 2),
            26 => (u64::from(u32::from_be_bytes(input.get(..4)?.try_into().ok()?)), 4),
            27 => (u64::from_be_bytes(input.get(..8)?.try_into().ok()?), 8),
            _ => return None,
        })
    }

    /// Reads one item; returns it and how many bytes it took.
    pub(crate) fn read(input: &[u8]) -> Option<(Value<'_>, usize)> {
        read_depth(input, 0)
    }

    fn read_depth(input: &[u8], depth: u8) -> Option<(Value<'_>, usize)> {
        if depth > 16 {
            return None;
        }
        let head = *input.first()?;
        let (major, info) = (head >> 5, head & 0x1f);
        let (argument, extra) = argument(&input[1..], info)?;
        let mut at = 1 + extra;
        let length = usize::try_from(argument).ok()?;
        let value = match major {
            0 => Value::Int(i64::try_from(argument).ok()?),
            1 => Value::Int(-1 - i64::try_from(argument).ok()?),
            2 => {
                let bytes = input.get(at..at.checked_add(length)?)?;
                at += length;
                Value::Bytes(bytes)
            }
            3 => {
                let bytes = input.get(at..at.checked_add(length)?)?;
                at += length;
                Value::Text(std::str::from_utf8(bytes).ok()?)
            }
            4 => {
                let mut items = Vec::new();
                for _ in 0..length.min(1024) {
                    let (item, used) = read_depth(&input[at..], depth + 1)?;
                    at += used;
                    items.push(item);
                }
                Value::Array(items)
            }
            5 => {
                let mut entries = Vec::new();
                for _ in 0..length.min(1024) {
                    let (key, used) = read_depth(&input[at..], depth + 1)?;
                    at += used;
                    let (value, used) = read_depth(&input[at..], depth + 1)?;
                    at += used;
                    entries.push((key, value));
                }
                Value::Map(entries)
            }
            _ => Value::Other,
        };
        Some((value, at))
    }
}
