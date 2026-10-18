//! Deployment (`PLAN.md` Milestone 50): immutable revisions behind the
//! traffic splitter (preview, staged promotion, rollback), tag-invalidated
//! response caching, and embedded assets.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bytes::Bytes;
use http::Request;
use rustnative_server::assets::{self, Asset};
use rustnative_server::deploy::{
    DeploymentAdapter, LocalAdapter, Revision, TrafficSplitter, container,
};
use rustnative_server::{Json, Security, ServerApp, get};

async fn revision(name: &'static str) -> Revision {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    // Behind the splitter every request comes from the proxy's address, so
    // the per-client rate limit belongs at the proxy, not the revision.
    let app = ServerApp::new()
        .security(Security { rate_limit: None, ..Security::default() })
        .route("/", get(move || async move { name }).public());
    tokio::spawn(app.into_service().serve(listener, std::future::pending()));
    Revision { name: name.into(), address }
}

fn client(n: u8) -> SocketAddr {
    SocketAddr::from(([10, 0, 0, n], 5000))
}

async fn served_by(adapter: &LocalAdapter, from: SocketAddr, preview: Option<&str>) -> String {
    let mut request = Request::builder().uri("/");
    if let Some(name) = preview {
        request = request.header("x-revision", name);
    }
    let response = adapter.splitter().forward(request.body(Bytes::new()).unwrap(), from).await;
    assert!(response.status().is_success(), "{:?}", response.status());
    let body = String::from_utf8(response.body().to_vec()).unwrap();
    assert_eq!(response.headers()["x-served-by"], body.as_str());
    body
}

#[tokio::test]
async fn revisions_are_previewed_promoted_by_percentage_and_rolled_back() {
    let adapter = LocalAdapter::default();
    adapter.deploy(revision("r1").await).await.unwrap();
    adapter.deploy(revision("r2").await).await.unwrap();
    assert!(adapter.deploy(revision("r1").await).await.is_err(), "revisions are immutable");

    // r2 is a preview: only a request that names it reaches it.
    for n in 0..20 {
        assert_eq!(served_by(&adapter, client(n), None).await, "r1");
    }
    assert_eq!(served_by(&adapter, client(1), Some("r2")).await, "r2");

    // 30% of clients move, and each client stays where it is sent.
    adapter.promote("r2", 30).await.unwrap();
    let mut on_r2 = 0;
    for n in 0..200 {
        let first = served_by(&adapter, client(n), None).await;
        assert_eq!(
            served_by(&adapter, client(n), None).await,
            first,
            "a client keeps its revision"
        );
        on_r2 += usize::from(first == "r2");
    }
    assert!((30..=90).contains(&on_r2), "about 30% of 200 clients: {on_r2}");

    adapter.promote("r2", 100).await.unwrap();
    assert_eq!(served_by(&adapter, client(7), None).await, "r2");
    adapter.rollback().await.unwrap();
    for n in 0..20 {
        assert_eq!(served_by(&adapter, client(n), None).await, "r1", "rolled back");
    }
    assert_eq!(adapter.status().weights.get("r1"), Some(&100));
}

const CONTROL_PORT: u16 = 8081;
const TOKEN: &str = "this-run-s-control-token";

/// A JSON request to the control API naming `host`, with `token` if any.
fn to_control(method: &str, path: &str, host: &str, token: Option<&str>) -> http::request::Builder {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", host)
        .header("content-type", "application/json");
    match token {
        Some(token) => request.header("authorization", format!("Bearer {token}")),
        None => request,
    }
}

/// A request to the control API as `rustnative deploy local` sends it.
fn from_the_cli(method: &str, path: &str) -> http::request::Builder {
    to_control(method, path, &format!("127.0.0.1:{CONTROL_PORT}"), Some(TOKEN))
}

#[tokio::test]
async fn the_control_api_drives_the_same_adapter() {
    let adapter = LocalAdapter::default();
    let control = adapter.control(CONTROL_PORT, TOKEN).into_service();
    let r1 = revision("r1").await;
    let post =
        |path: &str, body: String| from_the_cli("POST", path).body(Bytes::from(body)).unwrap();
    let added = control.handle(post("/revisions", serde_json::to_string(&r1).unwrap()), None).await;
    assert!(added.status().is_success(), "{:?}", added.body());
    let refused =
        control.handle(post("/promote", r#"{"revision":"nope","percent":10}"#.into()), None).await;
    assert_eq!(refused.status(), http::StatusCode::BAD_REQUEST);
    let status =
        control.handle(from_the_cli("GET", "/status").body(Bytes::new()).unwrap(), None).await;
    assert!(status.status().is_success(), "{:?}", status.body());
    let localhost = control
        .handle(
            to_control("GET", "/status", &format!("localhost:{CONTROL_PORT}"), Some(TOKEN))
                .body(Bytes::new())
                .unwrap(),
            None,
        )
        .await;
    assert!(localhost.status().is_success(), "localhost names the same listener");
    assert_eq!(adapter.status().revisions, vec![r1]);
}

/// The attack: a page on any origin posts `text/plain` (no preflight) to
/// the loopback control port, adding its own server as a revision and
/// sending every client of the public proxy to it.
#[tokio::test]
async fn a_cross_site_page_cannot_redirect_the_deployment() {
    let adapter = LocalAdapter::default();
    let control = adapter.control(CONTROL_PORT, TOKEN).into_service();
    let simple = |path: &str, body: &'static str| {
        Request::builder()
            .method("POST")
            .uri(path)
            .header("host", format!("127.0.0.1:{CONTROL_PORT}"))
            .header("content-type", "text/plain;charset=UTF-8")
            .header("origin", "https://attacker.example")
            .header("sec-fetch-site", "cross-site")
            .body(Bytes::from_static(body.as_bytes()))
            .unwrap()
    };
    for (path, body) in [
        ("/revisions", r#"{"name":"evil","address":"203.0.113.7:80"}"#),
        ("/promote", r#"{"revision":"evil","percent":100}"#),
    ] {
        let response = control.handle(simple(path, body), None).await;
        assert!(response.status().is_client_error(), "{path}: {:?}", response.status());
    }
    // Even holding the token, a page is refused: it sent `Origin`.
    let with_token = control
        .handle(
            from_the_cli("POST", "/rollback")
                .header("origin", "https://attacker.example")
                .body(Bytes::new())
                .unwrap(),
            None,
        )
        .await;
    assert_eq!(with_token.status(), http::StatusCode::FORBIDDEN);
    let fetched = control
        .handle(
            from_the_cli("GET", "/status")
                .header("sec-fetch-site", "cross-site")
                .body(Bytes::new())
                .unwrap(),
            None,
        )
        .await;
    assert_eq!(fetched.status(), http::StatusCode::FORBIDDEN);
    assert_eq!(adapter.status(), rustnative_server::deploy::DeploymentStatus::default());
}

#[tokio::test]
async fn the_control_api_needs_this_run_s_token() {
    let adapter = LocalAdapter::default();
    let control = adapter.control(CONTROL_PORT, TOKEN).into_service();
    let body = r#"{"name":"r1","address":"127.0.0.1:9001"}"#;
    let host = format!("127.0.0.1:{CONTROL_PORT}");
    let without = to_control("POST", "/revisions", &host, None)
        .body(Bytes::from_static(body.as_bytes()))
        .unwrap();
    assert_eq!(control.handle(without, None).await.status(), http::StatusCode::UNAUTHORIZED);
    let wrong = to_control("POST", "/revisions", &host, Some("another-run-s-token"))
        .body(Bytes::from_static(body.as_bytes()))
        .unwrap();
    assert_eq!(control.handle(wrong, None).await.status(), http::StatusCode::UNAUTHORIZED);
    let status = to_control("GET", "/status", &host, None).body(Bytes::new()).unwrap();
    assert_eq!(control.handle(status, None).await.status(), http::StatusCode::UNAUTHORIZED);
    assert!(adapter.status().revisions.is_empty());
}

/// DNS rebinding: a page whose name now resolves to 127.0.0.1 still sends
/// its own name as `Host`.
#[tokio::test]
async fn the_control_api_answers_only_its_own_loopback_name() {
    let adapter = LocalAdapter::default();
    let control = adapter.control(CONTROL_PORT, TOKEN).into_service();
    for host in [
        "attacker.example".to_owned(),
        format!("attacker.example:{CONTROL_PORT}"),
        "127.0.0.1".to_owned(),
        "127.0.0.1:9999".to_owned(),
        "0.0.0.0:8081".to_owned(),
    ] {
        let request = to_control("GET", "/status", &host, Some(TOKEN)).body(Bytes::new()).unwrap();
        assert_eq!(
            control.handle(request, None).await.status(),
            http::StatusCode::FORBIDDEN,
            "{host}"
        );
    }
    let missing = Request::builder()
        .uri("/status")
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Bytes::new())
        .unwrap();
    assert_eq!(control.handle(missing, None).await.status(), http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_json_body_must_say_it_is_json() {
    let app = ServerApp::new().security(Security { csrf: false, ..Security::default() }).route(
        "/echo",
        rustnative_server::post(|Json(value): Json<serde_json::Value>| async move { Json(value) })
            .public(),
    );
    let service = app.into_service();
    let send = |content_type: Option<&'static str>| {
        let service = service.clone();
        async move {
            let mut request = Request::builder().method("POST").uri("/echo");
            if let Some(content_type) = content_type {
                request = request.header("content-type", content_type);
            }
            service.handle(request.body(Bytes::from_static(b"{}")).unwrap(), None).await.status()
        }
    };
    for refused in [
        None,
        Some("text/plain;charset=UTF-8"),
        Some("application/x-www-form-urlencoded"),
        Some("multipart/form-data; boundary=x"),
        Some("text/json"),
    ] {
        assert_eq!(send(refused).await, http::StatusCode::UNSUPPORTED_MEDIA_TYPE, "{refused:?}");
    }
    for accepted in
        ["application/json", "Application/JSON; charset=utf-8", "application/merge-patch+json"]
    {
        assert_eq!(send(Some(accepted)).await, http::StatusCode::OK, "{accepted}");
    }
}

#[tokio::test]
async fn cached_pages_are_reused_until_their_tag_is_invalidated() {
    let renders = Arc::new(AtomicU32::new(0));
    let counted = renders.clone();
    let app = ServerApp::new().route(
        "/notes",
        get(move || {
            let renders = counted.clone();
            async move { format!("render {}", renders.fetch_add(1, Ordering::SeqCst) + 1) }
        })
        .public()
        .cached(&["notes"], Duration::from_secs(60)),
    );
    let cache = app.response_cache();
    let service = app.into_service();
    let fetch = || async {
        let response = service
            .handle(Request::builder().uri("/notes").body(Bytes::new()).unwrap(), None)
            .await;
        (
            String::from_utf8(response.body().to_vec()).unwrap(),
            response.headers()["x-cache"].to_str().unwrap().to_owned(),
        )
    };

    assert_eq!(fetch().await, ("render 1".into(), "miss".into()));
    assert_eq!(fetch().await, ("render 1".into(), "hit".into()));
    let listed = cache.entries();
    assert_eq!(
        (listed[0].key.as_str(), listed[0].hits, listed[0].tags.clone()),
        ("/notes?", 1, vec!["notes".to_owned()])
    );

    assert_eq!(cache.invalidate("notes"), 1);
    assert_eq!(fetch().await, ("render 2".into(), "miss".into()), "regenerated after invalidation");
    assert_eq!(renders.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_response_carrying_one_visitor_s_secrets_is_never_replayed_to_another() {
    // A rendered page is `no-store`: it carries its request's CSP nonce and
    // request-forgery token. Declaring its route cached must not replay it.
    let renders = Arc::new(AtomicU32::new(0));
    let counted = renders.clone();
    let app = ServerApp::new().route(
        "/page",
        get(move || {
            let renders = counted.clone();
            async move {
                let mut response = rustnative_server::Response::new(Bytes::from(format!(
                    "nonce {}",
                    renders.fetch_add(1, Ordering::SeqCst)
                )));
                response.headers_mut().insert(
                    http::header::CACHE_CONTROL,
                    http::HeaderValue::from_static("no-store"),
                );
                response
            }
        })
        .public()
        .cached(&["pages"], Duration::from_secs(60)),
    );
    let cache = app.response_cache();
    let service = app.into_service();
    for _ in 0..2 {
        let _ =
            service.handle(Request::builder().uri("/page").body(Bytes::new()).unwrap(), None).await;
    }
    assert_eq!(renders.load(Ordering::SeqCst), 2, "rendered for each visitor");
    assert_eq!(cache.entries(), []);
}

#[tokio::test]
async fn cached_responses_vary_by_language() {
    let app = ServerApp::new().route(
        "/hello",
        get(|headers: http::HeaderMap| async move {
            headers
                .get("accept-language")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("en")
                .to_owned()
        })
        .public()
        .cached(&["hello"], Duration::from_secs(60)),
    );
    let service = app.into_service();
    let fetch = |language: &'static str| {
        let service = service.clone();
        async move {
            let response = service
                .handle(
                    Request::builder()
                        .uri("/hello")
                        .header("accept-language", language)
                        .body(Bytes::new())
                        .unwrap(),
                    None,
                )
                .await;
            String::from_utf8(response.body().to_vec()).unwrap()
        }
    };
    assert_eq!(fetch("de").await, "de");
    assert_eq!(fetch("fr").await, "fr", "not the German visitor's page");
    assert_eq!(fetch("de").await, "de");
}

static ASSETS: &[Asset] = &[Asset {
    path: "app.css",
    hashed: "app.2c26b46b.css",
    content_type: "text/css; charset=utf-8",
    integrity: "sha256-LCa0a2j/xo/5m0U8HTBBNBNCLXBkg7+g+YpeiGJm564=",
    bytes: b"foo",
}];

#[tokio::test]
async fn embedded_assets_are_served_immutable_and_linked_with_integrity() {
    let service = ServerApp::new().assets(ASSETS).into_service();
    let response = service
        .handle(
            Request::builder().uri("/assets/app.2c26b46b.css").body(Bytes::new()).unwrap(),
            None,
        )
        .await;
    assert_eq!(response.body().as_ref(), b"foo");
    assert_eq!(response.headers()["content-type"], "text/css; charset=utf-8");
    assert_eq!(response.headers()["cache-control"], "public, max-age=31536000, immutable");
    assert_eq!(assets::url(ASSETS, "app.css").as_deref(), Some("/assets/app.2c26b46b.css"));
    let link = assets::stylesheet(ASSETS, "app.css").unwrap();
    let link = link.as_str();
    assert!(
        link.contains("integrity=\"sha256-LCa0a2j/xo/5m0U8HTBBNBNCLXBkg7+g+YpeiGJm564=\""),
        "{link}"
    );
}

#[test]
fn container_descriptions_ask_only_for_what_is_declared() {
    let service = container::Service {
        name: "notes".into(),
        binary: "notes".into(),
        version: "1.4.2".into(),
        port: 8080,
        environment: vec!["RUSTNATIVE_RESOURCE_DATA".into()],
        resources: vec!["data".into()],
    };
    let dockerfile = container::dockerfile(&service);
    assert!(dockerfile.contains("USER nonroot") && dockerfile.contains("EXPOSE 8080"));
    assert!(dockerfile.contains("WORKDIR /data") && dockerfile.contains("--chown=nonroot:nonroot"));
    let kubernetes = container::kubernetes(&service);
    assert!(
        kubernetes.contains("readOnlyRootFilesystem: true") && kubernetes.contains("path: /readyz")
    );
    assert!(kubernetes.contains("key: RUSTNATIVE_RESOURCE_DATA"));
    // The state has somewhere to live, it belongs to one instance, and the
    // image is the revision it names.
    assert!(
        kubernetes.contains("mountPath: /data")
            && kubernetes.contains("kind: PersistentVolumeClaim")
    );
    assert!(kubernetes.contains("replicas: 1") && kubernetes.contains("image: notes:1.4.2"));
    assert!(!kubernetes.contains(":latest"));
    let compose = container::compose(&service);
    assert!(compose.contains("read_only: true") && compose.contains("notes-data:/data"));
    let systemd = container::systemd(&service);
    assert!(systemd.contains("DynamicUser=yes") && systemd.contains("StateDirectory=notes"));
}

#[test]
fn serverless_descriptions_name_the_artifact_and_its_secrets() {
    use rustnative_server::deploy::serverless::{EdgeModule, Function, sam, spin};
    let template = sam(&Function {
        name: "notes".into(),
        code: "../target/web/lambda".into(),
        memory_mb: 256,
        timeout_s: 15,
        environment: vec!["NOTES_DATA_KEY".into()],
    });
    for line in [
        "Runtime: provided.al2023",
        "Handler: bootstrap",
        "CodeUri: ../target/web/lambda",
        "MemorySize: 256",
        "Timeout: 15",
        "AutoPublishAlias: live",
        "Type: Canary10Percent5Minutes",
        "NOTES_DATA_KEY: !Ref NotesDataKey",
        "NotesDataKey:",
        "NoEcho: true",
        "Type: HttpApi",
    ] {
        assert!(template.contains(line), "{line}:\n{template}");
    }
    let manifest = spin(&EdgeModule {
        name: "notes".into(),
        source: "../target/web/wagi/notes.wasm".into(),
        outbound: vec!["https://data.example.com".into()],
        environment: vec!["NOTES_DATA_KEY".into()],
    });
    let parsed: toml::Value = toml::from_str(&manifest).unwrap();
    assert_eq!(parsed["spin_manifest_version"].as_integer(), Some(2));
    let trigger = &parsed["trigger"]["http"][0];
    assert_eq!(trigger["executor"]["type"].as_str(), Some("wagi"));
    let component = &parsed["component"]["notes"];
    assert_eq!(component["source"].as_str(), Some("../target/web/wagi/notes.wasm"));
    assert_eq!(component["allowed_outbound_hosts"][0].as_str(), Some("https://data.example.com"));
    assert_eq!(component["environment"]["NOTES_DATA_KEY"].as_str(), Some("{{ notes_data_key }}"));
    assert_eq!(parsed["variables"]["notes_data_key"]["secret"].as_bool(), Some(true));
}

#[test]
fn each_target_states_what_its_host_allows() {
    use rustnative_server::deploy::Target;
    let function = LocalAdapter::for_target(Target::Function);
    assert_eq!(function.name(), "function runtime");
    assert_eq!(function.limits().payload_bytes, Some(6 * 1024 * 1024));
    assert_eq!(LocalAdapter::for_target(Target::Edge).limits().filesystem, "none");
    assert_eq!(LocalAdapter::for_target(Target::Static).limits().payload_bytes, Some(0));
    assert_eq!(LocalAdapter::default().limits().deadline, None);
}

#[tokio::test]
async fn the_splitter_refuses_oversized_and_slow_bodies_and_keeps_serving() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let limits = Security {
        body_limit: 16,
        body_timeout: Duration::from_millis(200),
        ..Security::default()
    };
    let adapter = LocalAdapter::new(TrafficSplitter::new().with_limits(&limits));
    adapter.deploy(revision("r1").await).await.unwrap();
    adapter.promote("r1", 100).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(adapter.splitter().serve(listener));
    let send = |request: Vec<u8>| async move {
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream.write_all(&request).await.unwrap();
        let mut response = String::new();
        tokio::time::timeout(Duration::from_secs(5), stream.read_to_string(&mut response))
            .await
            .expect("the splitter answers")
            .unwrap();
        response
    };

    let mut oversized =
        b"POST / HTTP/1.1\r\nHost: test\r\nContent-Length: 4096\r\nConnection: close\r\n\r\n"
            .to_vec();
    oversized.extend(std::iter::repeat_n(b'x', 4096));
    let response = send(oversized).await;
    assert!(response.starts_with("HTTP/1.1 413"), "{response}");

    let slow =
        b"POST / HTTP/1.1\r\nHost: test\r\nContent-Length: 10\r\nConnection: close\r\n\r\nabc"
            .to_vec();
    let response = send(slow).await;
    assert!(response.starts_with("HTTP/1.1 408"), "{response}");

    let response =
        send(b"GET / HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n".to_vec()).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("x-served-by: r1"), "{response}");
}
