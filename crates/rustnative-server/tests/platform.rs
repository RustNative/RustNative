//! Typed server functions, the API schema and its contract check,
//! generated clients, web metadata, push, the admin surface, server-only
//! components, and server inspection (`PLAN.md` Milestone 49).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::time::Duration;

use aes_gcm::aead::{Aead, KeyInit};
use base64::Engine;
use bytes::Bytes;
use hkdf::Hkdf;
use http::StatusCode;
use p256::ecdsa::signature::Verifier;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use rustnative_core::api_schema::{ApiSchema, object};
use rustnative_core::server_fn::{
    ServerComponentDef, ServerFn, ServerFnError, call, fetch_component,
};
use rustnative_core::{HttpService, Node};
use rustnative_server::admin::Admin;
use rustnative_server::auth::session::Sessions;
use rustnative_server::auth::token::TokenSigner;
use rustnative_server::auth::{Authentication, Policy};
use rustnative_server::components::server_component;
use rustnative_server::config::Secret;
use rustnative_server::db::Db;
use rustnative_server::db::schema::Schema;
use rustnative_server::functions::server_fn;
use rustnative_server::head::{Head, Sitemap};
use rustnative_server::jobs::Jobs;
use rustnative_server::local::InProcess;
use rustnative_server::openapi::{breaking_changes, typescript_client};
use rustnative_server::push::{
    Subscription, SubscriptionKeys, Vapid, apns_request, encrypt_with, fcm_request, wns_request,
};
use rustnative_server::{RequestContext, ServerApp, ServerError};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

#[derive(Debug, Serialize, Deserialize)]
struct Add {
    a: i64,
    b: i64,
}

impl ApiSchema for Add {
    fn schema() -> serde_json::Value {
        object("Add", [("a", i64::schema(), true), ("b", i64::schema(), true)])
    }
}

struct Sum;
impl ServerFn for Sum {
    const PATH: &'static str = "math/sum";
    type Input = Add;
    type Output = i64;
}

struct Greeting;
impl ServerComponentDef for Greeting {
    const NAME: &'static str = "greeting";
    type Props = String;
}

fn app() -> ServerApp {
    ServerApp::new()
        .openapi("Math", "1.0.0")
        .function::<Sum>(
            server_fn::<Sum, _, _>(|add: Add| async move {
                if add.b == 0 && add.a == 0 {
                    Err(ServerError::bad_request("nothing to add"))
                } else {
                    Ok(add.a + add.b)
                }
            })
            .csrf_exempt()
            .public(),
        )
        .component::<Greeting>(
            server_component::<Greeting, _, _>(|name: String| async move {
                Ok(Node::column("greeting", [Node::label("hello", format!("Hello, {name}"))]))
            })
            .csrf_exempt()
            .public(),
        )
        .resource("/sitemap.xml", || {
            let xml = Sitemap::new("https://example.com").page("/", None).xml();
            let mut response = http::Response::new(Bytes::from(xml));
            response
                .headers_mut()
                .insert("content-type", http::HeaderValue::from_static("application/xml"));
            response
        })
}

#[tokio::test]
async fn one_definition_serves_and_calls() {
    let http = InProcess(app().into_service());
    let sum = call::<Sum>(&http, "https://example.com", &Add { a: 2, b: 3 }, &[]).await.unwrap();
    assert_eq!(sum, 5);
    let refused = call::<Sum>(&http, "https://example.com", &Add { a: 0, b: 0 }, &[]).await;
    assert_eq!(
        refused,
        Err(ServerFnError::Server { status: 400, message: "nothing to add".into() })
    );

    let tree = fetch_component::<Greeting>(&http, "https://example.com", &"Ada".to_owned(), &[])
        .await
        .unwrap();
    assert_eq!(
        tree,
        Node::column("greeting", [Node::label("hello", "Hello, Ada")]),
        "merged as an ordinary tree"
    );

    let sitemap = http
        .execute(rustnative_core::HttpRequest::get("https://example.com/sitemap.xml"))
        .await
        .unwrap();
    assert!(
        String::from_utf8_lossy(sitemap.body_bytes()).contains("<loc>https://example.com/</loc>")
    );
}

#[tokio::test]
async fn the_api_schema_comes_from_the_types() {
    let http = InProcess(app().into_service());
    let response = http
        .execute(rustnative_core::HttpRequest::get("https://example.com/openapi.json"))
        .await
        .unwrap();
    let document: serde_json::Value = serde_json::from_slice(response.body_bytes()).unwrap();
    let operation = &document["paths"]["/_fn/math/sum"]["post"];
    assert_eq!(operation["operationId"], "math.sum");
    assert_eq!(
        operation["requestBody"]["content"]["application/json"]["schema"]["required"],
        serde_json::json!(["a", "b"])
    );
    assert_eq!(
        operation["responses"]["200"]["content"]["application/json"]["schema"]["type"],
        "integer"
    );

    // The contract check: compatible with itself, broken by a new required field.
    assert_eq!(breaking_changes(&document, &document), [] as [&str; 0]);
    let mut changed = document.clone();
    changed["paths"]["/_fn/math/sum"]["post"]["requestBody"]["content"]["application/json"]["schema"]
        ["required"] = serde_json::json!(["a", "b", "c"]);
    let breaks = breaking_changes(&document, &changed);
    assert_eq!(breaks, ["POST /_fn/math/sum: request field `c` is now required"]);
    let mut removed = document.clone();
    removed["paths"].as_object_mut().unwrap().remove("/_fn/math/sum");
    assert_eq!(breaking_changes(&document, &removed).len(), 1);

    let client = typescript_client(&document);
    assert!(client.contains("export interface Add {"), "{client}");
    assert!(client.contains("export async function mathSum(base: string, input: Add"), "{client}");
    assert!(client.contains("Promise<number>"));
}

#[test]
fn heads_are_escaped_and_validated() {
    let head = Head::new("A \"quoted\" <title>", "short").canonical("/relative").structured_data(
        serde_json::json!({ "@type": "Article", "name": "</script><script>alert(1)" }),
    );
    let problems = head.validate();
    assert_eq!(problems.len(), 2, "{problems:?}");
    let html = head.render("abc");
    assert!(html.as_str().contains("<title>A &quot;quoted&quot; &lt;title&gt;</title>"));
    assert!(
        !html.as_str().contains("</script><script>"),
        "structured data cannot close its script"
    );
    assert!(html.as_str().contains("nonce=\"abc\""));
}

#[test]
fn web_push_encrypts_for_the_subscriber_and_signs_with_vapid() {
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    // The browser's side.
    let ua_secret = p256::SecretKey::from_slice(&[5; 32]).unwrap();
    let ua_public = ua_secret.public_key().to_encoded_point(false);
    let auth = [9u8; 16];
    // The server's side, with a fixed ephemeral key for the test.
    let ephemeral = p256::SecretKey::from_slice(&[7; 32]).unwrap();
    let body = encrypt_with(b"hello", ua_public.as_bytes(), &auth, &ephemeral, [1; 16]).unwrap();

    // Decrypt as the browser would (RFC 8291 section 3.4).
    let salt = &body[..16];
    assert_eq!(u32::from_be_bytes(body[16..20].try_into().unwrap()), 4096);
    assert_eq!(body[20], 65);
    let as_public = p256::PublicKey::from_sec1_bytes(&body[21..86]).unwrap();
    let shared = p256::ecdh::diffie_hellman(ua_secret.to_nonzero_scalar(), as_public.as_affine());
    let mut info = b"WebPush: info\0".to_vec();
    info.extend_from_slice(ua_public.as_bytes());
    info.extend_from_slice(&body[21..86]);
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&auth), shared.raw_secret_bytes()).expand(&info, &mut ikm).unwrap();
    let prk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let (mut key, mut nonce) = ([0u8; 16], [0u8; 12]);
    prk.expand(b"Content-Encoding: aes128gcm\0", &mut key).unwrap();
    prk.expand(b"Content-Encoding: nonce\0", &mut nonce).unwrap();
    let plain = aes_gcm::Aes128Gcm::new(&key.into())
        .decrypt(aes_gcm::Nonce::from_slice(&nonce), &body[86..])
        .unwrap();
    assert_eq!(plain, b"hello\x02");

    let vapid = Vapid::new(Secret::new([3; 32]), "mailto:ops@example.com");
    let subscription = Subscription {
        endpoint: "https://push.example.net/send/abc".into(),
        keys: SubscriptionKeys {
            p256dh: engine.encode(ua_public.as_bytes()),
            auth: engine.encode(auth),
        },
    };
    let request = vapid.request(&subscription, b"hi", 60).unwrap();
    let authorization =
        request.headers().iter().find(|(name, _)| name == "authorization").unwrap().1.clone();
    let jwt = authorization.strip_prefix("vapid t=").unwrap().split(',').next().unwrap();
    let parts = jwt.split('.').collect::<Vec<_>>();
    let claims: serde_json::Value =
        serde_json::from_slice(&engine.decode(parts[1]).unwrap()).unwrap();
    assert_eq!(claims["aud"], "https://push.example.net");
    let key = p256::ecdsa::VerifyingKey::from_sec1_bytes(
        &engine.decode(vapid.public_key().unwrap()).unwrap(),
    )
    .unwrap();
    let signature = p256::ecdsa::Signature::from_slice(&engine.decode(parts[2]).unwrap()).unwrap();
    key.verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature).unwrap();
    assert!(
        request
            .headers()
            .iter()
            .any(|(name, value)| name == "content-encoding" && value == "aes128gcm")
    );
}

#[test]
fn native_push_services_get_their_documented_requests() {
    let token = Secret::new("t0k".to_owned());
    let wns = wns_request("https://wns2-par02p.notify.windows.com/w/?token=x", &token, b"raw");
    assert!(wns.headers().iter().any(|(name, value)| name == "x-wns-type" && value == "wns/raw"));
    let apns = apns_request("abc123", "com.example.notes", &token, "Hi");
    assert_eq!(apns.url(), "https://api.push.apple.com/3/device/abc123");
    assert!(
        apns.headers()
            .iter()
            .any(|(name, value)| name == "apns-topic" && value == "com.example.notes")
    );
    let fcm = fcm_request("notes-app", "device", &token, "Title", "Body");
    assert_eq!(fcm.url(), "https://fcm.googleapis.com/v1/projects/notes-app/messages:send");
    let message: serde_json::Value = serde_json::from_slice(fcm.body_bytes()).unwrap();
    assert_eq!(message["message"]["token"], "device");
}

#[derive(Clone, Serialize, Deserialize)]
struct Staff {
    admin: bool,
}

struct Admins;
impl Policy<Staff> for Admins {
    const NAME: &'static str = "admins";
    fn allows(staff: &Staff, _: &RequestContext) -> bool {
        staff.admin
    }
}

#[tokio::test]
async fn the_admin_surface_is_generated_and_guarded() {
    let db = Db::memory("admin", 1).unwrap();
    db.get()
        .execute_batch(
            "CREATE TABLE notes (id INTEGER PRIMARY KEY, title TEXT NOT NULL, stars INTEGER);",
        )
        .unwrap();
    let schema = Schema::of(&db.get()).unwrap();
    let signer = TokenSigner::new(Secret::new(vec![1; 32]));
    let (before, after) =
        Sessions::new(&Secret::new([2; 32]), Duration::from_secs(60)).middleware();
    let jobs = Jobs::new(db.clone()).unwrap();
    let service = ServerApp::new()
        .before(before)
        .after(after)
        .before(Authentication::<Staff>::sessions().tokens(signer.clone()).middleware())
        .admin::<Staff, Admins>(Admin::new(db.clone(), schema))
        .inspection::<Staff, Admins>(jobs)
        .into_service();
    let bearer = |admin: bool| {
        format!(
            "Bearer {}",
            signer.issue(serde_json::to_string(&Staff { admin }).unwrap(), Duration::from_secs(60))
        )
    };
    let send = |method: &str, uri: &str, auth: String, body: &str| {
        http::Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", auth)
            .body(Bytes::from(body.to_owned()))
            .unwrap()
    };

    assert_eq!(
        service.handle(send("GET", "/admin", bearer(false), ""), None).await.status(),
        StatusCode::FORBIDDEN
    );
    let created = service
        .handle(
            send("POST", "/admin/notes/new", bearer(true), "title=%3Cb%3Ebold%3C%2Fb%3E&stars=3"),
            None,
        )
        .await;
    assert_eq!(created.status(), StatusCode::SEE_OTHER);
    let list = service.handle(send("GET", "/admin/notes", bearer(true), ""), None).await;
    let page = String::from_utf8(list.body().to_vec()).unwrap();
    assert!(page.contains("&lt;b&gt;bold&lt;/b&gt;"), "escaped: {page}");
    assert_eq!(
        service.handle(send("GET", "/admin/secrets", bearer(true), ""), None).await.status(),
        StatusCode::NOT_FOUND
    );
    let edit = service.handle(send("GET", "/admin/notes/1", bearer(true), ""), None).await;
    let edit_page = String::from_utf8(edit.body().to_vec()).unwrap();
    assert!(edit_page.contains("name=\"_csrf\""), "{} {edit_page}", edit.status());
    service
        .handle(send("POST", "/admin/notes/1", bearer(true), "title=renamed&stars="), None)
        .await;
    let title: String =
        db.get().query_row("SELECT title FROM notes WHERE id = 1", [], |row| row.get(0)).unwrap();
    assert_eq!(title, "renamed");
    service.handle(send("POST", "/admin/notes/1/delete", bearer(true), ""), None).await;
    let count: i64 =
        db.get().query_row("SELECT COUNT(*) FROM notes", [], |row| row.get(0)).unwrap();
    assert_eq!(count, 0);

    let inspected = service
        .handle(send("POST", "/__inspect", bearer(true), r#"{"request":"jobs"}"#), None)
        .await;
    let reply: rustnative_core::inspect::Reply = serde_json::from_slice(inspected.body()).unwrap();
    assert!(
        matches!(reply, rustnative_core::inspect::Reply::Ok(serde_json::Value::Array(_))),
        "{reply:?}"
    );
}
