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
use rustnative_server::deploy::{DeploymentAdapter, LocalAdapter, Revision, container};
use rustnative_server::{Security, ServerApp, get};

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

#[tokio::test]
async fn the_control_api_drives_the_same_adapter() {
    let adapter = LocalAdapter::default();
    let control = adapter.control().into_service();
    let r1 = revision("r1").await;
    let post = |path: &str, body: String| {
        Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .body(Bytes::from(body))
            .unwrap()
    };
    let added = control.handle(post("/revisions", serde_json::to_string(&r1).unwrap()), None).await;
    assert!(added.status().is_success(), "{:?}", added.body());
    let refused =
        control.handle(post("/promote", r#"{"revision":"nope","percent":10}"#.into()), None).await;
    assert_eq!(refused.status(), http::StatusCode::BAD_REQUEST);
    assert_eq!(adapter.status().revisions, vec![r1]);
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
        port: 8080,
        environment: vec!["RUSTNATIVE_RESOURCE_DATA".into()],
        resources: vec!["data".into()],
    };
    let dockerfile = container::dockerfile(&service);
    assert!(dockerfile.contains("USER nonroot") && dockerfile.contains("EXPOSE 8080"));
    let kubernetes = container::kubernetes(&service);
    assert!(
        kubernetes.contains("readOnlyRootFilesystem: true") && kubernetes.contains("path: /readyz")
    );
    assert!(kubernetes.contains("key: RUSTNATIVE_RESOURCE_DATA"));
    assert!(container::systemd(&service).contains("DynamicUser=yes"));
}
