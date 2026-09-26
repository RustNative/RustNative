//! Provider federation: OAuth 2.0 authorization code with PKCE (RFC 7636),
//! the flow every identity provider supports for a server or a native app.
//! The token exchange goes through the core `HttpService` contract, so the
//! same client works on the server and in the Windows app.

use std::fmt::Write as _;

use base64::Engine;
use rustnative_core::{HttpRequest, HttpService, Method};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// A provider's endpoints and this application's registration with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthClient {
    /// The provider's authorization endpoint.
    pub authorize_url: String,
    /// The provider's token endpoint.
    pub token_url: String,
    /// This application's client id.
    pub client_id: String,
    /// Where the provider sends the person back.
    pub redirect_uri: String,
    /// The scopes asked for.
    pub scopes: Vec<String>,
}

/// A started sign-in: send the person to `url`; keep `state` and
/// `verifier` in the session for the callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    /// Where to send the person.
    pub url: String,
    /// The anti-forgery state, checked on return.
    pub state: String,
    /// The PKCE verifier, sent with the code.
    pub verifier: String,
}

/// What the provider returned.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Tokens {
    /// The access token.
    pub access_token: String,
    /// Its type (`Bearer`).
    pub token_type: String,
    /// The `OpenID Connect` identity token, when asked for.
    #[serde(default)]
    pub id_token: Option<String>,
    /// A refresh token, when granted.
    #[serde(default)]
    pub refresh_token: Option<String>,
}

fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

impl OAuthClient {
    /// Starts a sign-in.
    #[must_use]
    pub fn start(&self) -> Started {
        let verifier = crate::security::random_token(32);
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.as_bytes()));
        let state = crate::security::random_token(16);
        let url = format!(
            "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
            self.authorize_url,
            encode(&self.client_id),
            encode(&self.redirect_uri),
            encode(&self.scopes.join(" ")),
            state,
            challenge
        );
        Started { url, state, verifier }
    }

    /// Exchanges the callback's `code` for tokens, after checking its
    /// `returned_state` against the one kept.
    ///
    /// # Errors
    ///
    /// The state does not match (a forged callback), or the provider
    /// refused.
    pub async fn finish(
        &self,
        http: &dyn HttpService,
        code: &str,
        returned_state: &str,
        started: &Started,
    ) -> Result<Tokens, String> {
        if !crate::security::constant_time_eq(returned_state.as_bytes(), started.state.as_bytes()) {
            return Err("the sign-in state does not match".into());
        }
        let body = format!(
            "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
            encode(code),
            encode(&self.redirect_uri),
            encode(&self.client_id),
            encode(&started.verifier)
        );
        let request = HttpRequest::new(Method::Post, self.token_url.clone())
            .header("content-type", "application/x-www-form-urlencoded")
            .header("accept", "application/json")
            .body(body.into_bytes());
        let response = http.execute(request).await.map_err(|error| error.to_string())?;
        if !(200..300).contains(&response.status()) {
            return Err(format!("the provider refused: {}", response.status()));
        }
        serde_json::from_slice(response.body_bytes()).map_err(|error| error.to_string())
    }
}
