//! Authentication and authorization (`PLAN.md` Milestone 49): sessions,
//! tokens, passkeys, federation, and policies checked before handlers.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::time::Duration;

use bytes::Bytes;
use http::{Request, StatusCode};
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{DerSignature, SigningKey};
use rustnative_core::{HttpRequest, HttpResponse, HttpService, ServiceError};
use rustnative_server::auth::oauth::OAuthClient;
use rustnative_server::auth::passkey::Passkeys;
use rustnative_server::auth::session::{Session, Sessions};
use rustnative_server::auth::token::TokenSigner;
use rustnative_server::auth::{Authentication, PRINCIPAL_KEY, Policy, Principal};
use rustnative_server::config::Secret;
use rustnative_server::{AppService, RequestContext, ServerApp, get, post};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct User {
    name: String,
    admin: bool,
}

struct Admins;

impl Policy<User> for Admins {
    const NAME: &'static str = "admins";
    fn allows(user: &User, _: &RequestContext) -> bool {
        user.admin
    }
}

async fn sign_in(session: Session) -> &'static str {
    session.set(PRINCIPAL_KEY, &User { name: "Ada".into(), admin: false });
    "signed in"
}

async fn me(Principal(user): Principal<User>) -> String {
    user.name
}

async fn admin() -> &'static str {
    "the admin page"
}

fn service(signer: TokenSigner) -> AppService {
    let (before, after) =
        Sessions::new(&Secret::new([7; 32]), Duration::from_secs(3600)).middleware();
    ServerApp::new()
        .before(before)
        .after(after)
        .before(Authentication::<User>::sessions().tokens(signer).middleware())
        .route("/sign-in", post(sign_in).csrf_exempt().public())
        .route("/me", get(me).signed_in::<User>())
        .route("/admin", get(admin).authorized::<User, Admins>())
        .into_service()
}

fn signer() -> TokenSigner {
    TokenSigner::new(Secret::new(vec![9; 32]))
}

async fn get_with(
    service: &AppService,
    uri: &str,
    header: Option<(&str, String)>,
) -> http::Response<Bytes> {
    let mut request = Request::get(uri);
    if let Some((name, value)) = header {
        request = request.header(name, value);
    }
    service.handle(request.body(Bytes::new()).unwrap(), None).await
}

#[tokio::test]
async fn a_session_signs_in_and_policies_guard_routes() {
    let service = service(signer());
    assert_eq!(get_with(&service, "/me", None).await.status(), StatusCode::UNAUTHORIZED);

    let signed_in =
        service.handle(Request::post("/sign-in").body(Bytes::new()).unwrap(), None).await;
    let cookie = signed_in
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|value| value.to_str().unwrap())
        .find(|value| value.starts_with("__Host-session="))
        .unwrap()
        .to_owned();
    assert!(cookie.contains("HttpOnly") && cookie.contains("Secure"), "{cookie}");
    let session = cookie.split(';').next().unwrap().to_owned();
    assert!(!session.contains("Ada"), "the session is encrypted");

    let me = get_with(&service, "/me", Some(("cookie", session.clone()))).await;
    assert_eq!(me.body(), "Ada");
    let forbidden = get_with(&service, "/admin", Some(("cookie", session.clone()))).await;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN, "signed in, but the policy refuses");

    let mut tampered = session.into_bytes();
    let last = tampered.len() - 2;
    tampered[last] = if tampered[last] == b'A' { b'B' } else { b'A' };
    let tampered = String::from_utf8(tampered).unwrap();
    assert_eq!(
        get_with(&service, "/me", Some(("cookie", tampered))).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn bearer_tokens_carry_a_principal() {
    let signer = signer();
    let admin = serde_json::to_string(&User { name: "Grace".into(), admin: true }).unwrap();
    let token = signer.issue(admin, Duration::from_secs(60));
    let service = service(signer.clone());
    let response =
        get_with(&service, "/admin", Some(("authorization", format!("Bearer {token}")))).await;
    assert_eq!(response.body(), "the admin page");

    let forged = format!("{token}x");
    let response =
        get_with(&service, "/admin", Some(("authorization", format!("Bearer {forged}")))).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(signer.verify(&signer.issue("x", Duration::ZERO)).is_none(), "an expired token");
}

#[test]
fn the_route_listing_names_each_routes_access() {
    let (before, _) = Sessions::new(&Secret::new([1; 32]), Duration::from_secs(60)).middleware();
    let app = ServerApp::new()
        .before(before)
        .route("/me", get(me).signed_in::<User>())
        .route("/admin", get(admin).authorized::<User, Admins>());
    let access: Vec<_> = app.routes().into_iter().map(|route| route.access).collect();
    assert_eq!(access, ["signed in", "admins"]);
}

// --- Passkeys ------------------------------------------------------------

fn cbor_head(major: u8, value: usize) -> Vec<u8> {
    let value = u16::try_from(value).unwrap();
    if value < 24 {
        vec![(major << 5) | u8::try_from(value).unwrap()]
    } else if value < 256 {
        vec![(major << 5) | 0x18, u8::try_from(value).unwrap()]
    } else {
        let [high, low] = value.to_be_bytes();
        vec![(major << 5) | 0x19, high, low]
    }
}

fn cbor_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut out = cbor_head(2, bytes.len());
    out.extend_from_slice(bytes);
    out
}

fn cbor_text(text: &str) -> Vec<u8> {
    let mut out = cbor_head(3, text.len());
    out.extend_from_slice(text.as_bytes());
    out
}

fn cbor_int(value: i64) -> Vec<u8> {
    if value >= 0 {
        cbor_head(0, usize::try_from(value).unwrap())
    } else {
        cbor_head(1, usize::try_from(-1 - value).unwrap())
    }
}

fn client_data(kind: &str, challenge: &str) -> Vec<u8> {
    format!(r#"{{"type":"{kind}","challenge":"{challenge}","origin":"https://example.com"}}"#)
        .into_bytes()
}

fn auth_data(flags: u8, count: u32) -> Vec<u8> {
    let mut data = Sha256::digest(b"example.com").to_vec();
    data.push(flags);
    data.extend_from_slice(&count.to_be_bytes());
    data
}

#[test]
fn a_passkey_registers_then_signs_in_once_per_counter() {
    let key = SigningKey::from_slice(&[3; 32]).unwrap();
    let point = key.verifying_key().to_encoded_point(false);
    let passkeys = Passkeys::new("example.com", "https://example.com");

    // Registration: authData with the attested credential and its COSE key.
    let challenge = Passkeys::challenge();
    let mut registration = auth_data(0x41, 0);
    registration.extend_from_slice(&[0; 16]); // AAGUID
    registration.extend_from_slice(&[0, 4]);
    registration.extend_from_slice(b"cred");
    let mut cose = cbor_head(5, 5);
    for (label, value) in [(1, cbor_int(2)), (3, cbor_int(-7)), (-1, cbor_int(1))] {
        cose.extend(cbor_int(label));
        cose.extend(value);
    }
    cose.extend(cbor_int(-2));
    cose.extend(cbor_bytes(point.x().unwrap()));
    cose.extend(cbor_int(-3));
    cose.extend(cbor_bytes(point.y().unwrap()));
    registration.extend(cose);
    let mut attestation = cbor_head(5, 3);
    attestation.extend(cbor_text("fmt"));
    attestation.extend(cbor_text("none"));
    attestation.extend(cbor_text("attStmt"));
    attestation.extend(cbor_head(5, 0));
    attestation.extend(cbor_text("authData"));
    attestation.extend(cbor_bytes(&registration));

    let mut credential = passkeys
        .register(&client_data("webauthn.create", &challenge), &attestation, &challenge)
        .unwrap();
    assert_eq!(credential.id, b"cred");
    assert!(
        passkeys
            .register(&client_data("webauthn.create", "other"), &attestation, &challenge)
            .is_err()
    );

    // Sign-in.
    let sign = |count: u32, challenge: &str| {
        let data = auth_data(0x01, count);
        let client = client_data("webauthn.get", challenge);
        let mut signed = data.clone();
        signed.extend_from_slice(&Sha256::digest(&client));
        let signature: DerSignature = key.sign(&signed);
        (client, data, signature.as_bytes().to_vec())
    };
    let challenge = Passkeys::challenge();
    let (client, data, signature) = sign(1, &challenge);
    passkeys.verify(&mut credential, &client, &data, &signature, &challenge).unwrap();
    assert_eq!(credential.sign_count, 1);
    assert!(
        passkeys.verify(&mut credential, &client, &data, &signature, &challenge).is_err(),
        "a replayed assertion (the counter did not move)"
    );
    let (client, data, mut signature) = sign(2, &challenge);
    let last = signature.len() - 1;
    signature[last] ^= 1;
    assert!(passkeys.verify(&mut credential, &client, &data, &signature, &challenge).is_err());
}

// --- Federation -------------------------------------------------------------

struct Provider;

#[async_trait::async_trait]
impl HttpService for Provider {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ServiceError> {
        let body = String::from_utf8(request.body_bytes().to_vec()).unwrap();
        assert!(body.contains("code=the-code") && body.contains("code_verifier="), "{body}");
        Ok(HttpResponse::new(
            200,
            Vec::new(),
            br#"{"access_token":"at","token_type":"Bearer","id_token":"it"}"#.to_vec(),
        ))
    }
}

#[tokio::test]
async fn federation_uses_pkce_and_checks_the_state() {
    let client = OAuthClient {
        authorize_url: "https://id.example.com/authorize".into(),
        token_url: "https://id.example.com/token".into(),
        client_id: "app".into(),
        redirect_uri: "https://example.com/callback".into(),
        scopes: vec!["openid".into(), "email".into()],
    };
    let started = client.start();
    assert!(
        started.url.contains("code_challenge_method=S256")
            && started.url.contains("scope=openid%20email")
    );
    assert!(client.finish(&Provider, "the-code", "forged", &started).await.is_err());
    let tokens =
        client.finish(&Provider, "the-code", &started.state.clone(), &started).await.unwrap();
    assert_eq!(tokens.id_token.as_deref(), Some("it"));
}
