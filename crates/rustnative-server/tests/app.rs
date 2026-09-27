//! The server application model (`PLAN.md` Milestone 49): typed handling,
//! the security defaults (`docs/server/security-checklist.md`, one test per
//! line), request scopes, error pages, and mounting.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bytes::Bytes;
use http::{Request, StatusCode};
use rustnative_server::{
    AppService, CsrfToken, Form, Html, Json, Path, Query, RequestScope, Security, ServerApp,
    ServerError, State, get, post,
};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct Counter(Arc<AtomicU32>);

#[derive(Deserialize)]
struct Search {
    q: String,
    page: u32,
}

#[derive(Serialize, Deserialize)]
struct Note {
    title: String,
}

async fn user(Path(id): Path<u32>) -> String {
    format!("user {id}")
}

async fn search(Query(search): Query<Search>) -> String {
    format!("{} page {}", search.q, search.page)
}

async fn create(Json(note): Json<Note>) -> (StatusCode, Json<Note>) {
    (StatusCode::CREATED, Json(note))
}

async fn submit(Form(note): Form<Note>) -> Html {
    Html::trusted("<p>").text(&note.title).and_trusted("</p>")
}

async fn form(token: CsrfToken) -> String {
    token.0
}

async fn count(State(counter): State<Counter>) -> String {
    counter.0.fetch_add(1, Ordering::SeqCst).to_string()
}

async fn fails() -> Result<String, ServerError> {
    Err(ServerError::internal("the database password is hunter2"))
}

async fn background(scope: RequestScope, State(counter): State<Counter>) -> &'static str {
    scope.spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        counter.0.fetch_add(1, Ordering::SeqCst);
    });
    "answered"
}

fn app(counter: &Counter) -> AppService {
    ServerApp::new()
        .state(counter.clone())
        .route("/users/:id", get(user).public())
        .route("/search", get(search).public())
        .route("/notes", post(create).public())
        .route("/submit", get(form).post(submit).public())
        .route("/count", get(count).public())
        .route("/fails", get(fails).public())
        .route("/background", get(background).public())
        .route("/hook", post(create).csrf_exempt().public())
        .into_service()
}

fn request(method: &str, uri: &str) -> http::request::Builder {
    Request::builder().method(method).uri(uri)
}

async fn send(service: &AppService, request: Request<Bytes>) -> http::Response<Bytes> {
    service.handle(request, None).await
}

fn text(response: &http::Response<Bytes>) -> String {
    String::from_utf8(response.body().to_vec()).unwrap()
}

fn counter() -> Counter {
    Counter(Arc::new(AtomicU32::new(0)))
}

#[tokio::test]
async fn handlers_receive_typed_values() {
    let service = app(&counter());
    let response = send(&service, request("GET", "/users/42").body(Bytes::new()).unwrap()).await;
    assert_eq!(text(&response), "user 42");
    let response =
        send(&service, request("GET", "/search?q=rust+native&page=2").body(Bytes::new()).unwrap())
            .await;
    assert_eq!(text(&response), "rust native page 2");
    let response = send(&service, request("GET", "/users/ada").body(Bytes::new()).unwrap()).await;
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "a bad parameter never reaches the handler"
    );
    let response = send(&service, request("GET", "/count").body(Bytes::new()).unwrap()).await;
    assert_eq!(text(&response), "0");
}

#[tokio::test]
async fn unknown_paths_and_methods_are_refused() {
    let service = app(&counter());
    let response = send(&service, request("GET", "/nowhere").body(Bytes::new()).unwrap()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = send(&service, request("DELETE", "/users/1").body(Bytes::new()).unwrap()).await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()["allow"], "GET");
    let response = send(&service, request("HEAD", "/users/1").body(Bytes::new()).unwrap()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.body().is_empty(), "HEAD has no body");
}

#[tokio::test]
async fn unsafe_requests_need_the_forgery_token() {
    let service = app(&counter());
    let body = r#"{"title":"hi"}"#;
    let response = send(&service, request("POST", "/notes").body(Bytes::from(body)).unwrap()).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN, "no token, no write");

    // A page hands out the token and its cookie.
    let page = send(&service, request("GET", "/submit").body(Bytes::new()).unwrap()).await;
    let token = text(&page);
    let cookie = page.headers()["set-cookie"].to_str().unwrap().to_owned();
    assert!(cookie.starts_with(&format!("__Host-csrf={token}")), "{cookie}");
    assert!(cookie.contains("Secure") && cookie.contains("SameSite=Strict"));

    let with_header = request("POST", "/notes")
        .header("cookie", format!("__Host-csrf={token}"))
        .header("x-csrf-token", &token)
        .body(Bytes::from(body))
        .unwrap();
    assert_eq!(send(&service, with_header).await.status(), StatusCode::CREATED);

    let form = format!("title=%3Cb%3Ehi%3C%2Fb%3E&_csrf={token}");
    let with_field = request("POST", "/submit")
        .header("cookie", format!("__Host-csrf={token}"))
        .body(Bytes::from(form))
        .unwrap();
    let response = send(&service, with_field).await;
    assert_eq!(text(&response), "<p>&lt;b&gt;hi&lt;/b&gt;</p>", "output is escaped");

    let wrong = request("POST", "/notes")
        .header("cookie", format!("__Host-csrf={token}"))
        .header("x-csrf-token", "forged")
        .body(Bytes::from(body))
        .unwrap();
    assert_eq!(send(&service, wrong).await.status(), StatusCode::FORBIDDEN);

    let bearer = request("POST", "/notes")
        .header("authorization", "Bearer abc")
        .body(Bytes::from(body))
        .unwrap();
    assert_eq!(send(&service, bearer).await.status(), StatusCode::CREATED, "no ambient credential");
    let hook = request("POST", "/hook").body(Bytes::from(body)).unwrap();
    assert_eq!(send(&service, hook).await.status(), StatusCode::CREATED, "declared exempt");
}

#[tokio::test]
async fn every_response_carries_the_security_headers() {
    let service = app(&counter());
    let first = send(&service, request("GET", "/users/1").body(Bytes::new()).unwrap()).await;
    let second = send(&service, request("GET", "/users/1").body(Bytes::new()).unwrap()).await;
    let csp = first.headers()["content-security-policy"].to_str().unwrap();
    assert!(csp.contains("default-src 'self'") && csp.contains("'nonce-"), "{csp}");
    assert_ne!(csp, second.headers()["content-security-policy"], "a fresh nonce per response");
    assert_eq!(first.headers()["x-content-type-options"], "nosniff");
    assert_eq!(first.headers()["x-frame-options"], "DENY");
    assert!(first.headers().contains_key("strict-transport-security"));
    assert!(first.headers().contains_key("referrer-policy"));
    assert!(first.headers().contains_key("permissions-policy"));
    assert!(!first.headers().contains_key("cross-origin-opener-policy"), "opt-in");
}

#[tokio::test]
async fn clients_are_rate_limited_and_bodies_bounded() {
    let service = ServerApp::new()
        .security(Security {
            rate_limit: Some((3, Duration::from_secs(60))),
            body_limit: 16,
            ..Security::default()
        })
        .route("/users/:id", get(user).public())
        .route("/hook", post(create).csrf_exempt().public())
        .into_service();
    let client = Some("10.0.0.1:5000".parse().unwrap());
    for _ in 0..3 {
        let response =
            service.handle(request("GET", "/users/1").body(Bytes::new()).unwrap(), client).await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    let limited =
        service.handle(request("GET", "/users/1").body(Bytes::new()).unwrap(), client).await;
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(limited.headers().contains_key("retry-after"));
    let other = Some("10.0.0.2:5000".parse().unwrap());
    let large = request("POST", "/hook").body(Bytes::from(vec![b'x'; 17])).unwrap();
    assert_eq!(service.handle(large, other).await.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn work_does_not_outlive_its_request() {
    let counter = counter();
    let service = app(&counter);
    let response = send(&service, request("GET", "/background").body(Bytes::new()).unwrap()).await;
    assert_eq!(text(&response), "answered");
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(counter.0.load(Ordering::SeqCst), 0, "the scope was cancelled with the response");
}

#[tokio::test]
async fn error_pages_follow_accept_and_hide_internals() {
    let service = app(&counter());
    let html = send(&service, request("GET", "/fails").body(Bytes::new()).unwrap()).await;
    assert_eq!(html.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(text(&html).starts_with("<!doctype html>"));
    assert!(!text(&html).contains("hunter2"), "the detail is logged, not shown");
    let json = send(
        &service,
        request("GET", "/fails").header("accept", "application/json").body(Bytes::new()).unwrap(),
    )
    .await;
    assert_eq!(text(&json), r#"{"error":"Something went wrong"}"#);
}

#[tokio::test]
async fn health_metrics_and_the_report() {
    let ready = Arc::new(AtomicU32::new(0));
    let flag = Arc::clone(&ready);
    let app = ServerApp::new()
        .route("/users/:id", get(user).public())
        .readiness("database", move || flag.load(Ordering::SeqCst) == 1);
    assert!(app.report().iter().any(|line| line.starts_with("rate limit: 120 per 60s")));
    let service = app.into_service();
    let get = |uri: &'static str| request("GET", uri).body(Bytes::new()).unwrap();
    assert_eq!(send(&service, get("/healthz")).await.status(), StatusCode::OK);
    let not_ready = send(&service, get("/readyz")).await;
    assert_eq!(not_ready.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(text(&not_ready).contains("database"));
    ready.store(1, Ordering::SeqCst);
    assert_eq!(send(&service, get("/readyz")).await.status(), StatusCode::OK);
    let _ = send(&service, get("/users/1")).await;
    assert!(text(&send(&service, get("/metrics")).await).contains("http_requests_total"));
}

/// Mounted inside an existing hyper service, sharing its listener: the
/// host answers its own paths and hands `/app/*` to the application.
#[tokio::test]
async fn it_mounts_inside_an_existing_service() {
    use http_body_util::{BodyExt, Full};
    use tower_service::Service;

    let mounted =
        ServerApp::new().prefix("/app").route("/users/:id", get(user).public()).into_service();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let mounted = mounted.clone();
            tokio::spawn(async move {
                let host =
                    hyper::service::service_fn(move |request: Request<hyper::body::Incoming>| {
                        let mut mounted = mounted.clone();
                        async move {
                            if request.uri().path().starts_with("/app/") {
                                mounted.call(request).await
                            } else {
                                Ok(http::Response::new(rustnative_server::Body::from("host")))
                            }
                        }
                    });
                let io = hyper_util::rt::TokioIo::new(stream);
                let _ = hyper::server::conn::http1::Builder::new().serve_connection(io, host).await;
            });
        }
    });

    let fetch = |path: &'static str| async move {
        let stream = tokio::net::TcpStream::connect(address).await.unwrap();
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(stream))
                .await
                .unwrap();
        tokio::spawn(connection);
        let request =
            Request::get(path).header("host", "localhost").body(Full::new(Bytes::new())).unwrap();
        let response = sender.send_request(request).await.unwrap();
        String::from_utf8(response.into_body().collect().await.unwrap().to_bytes().to_vec())
            .unwrap()
    };
    assert_eq!(fetch("/app/users/7").await, "user 7");
    assert_eq!(fetch("/elsewhere").await, "host");
}
