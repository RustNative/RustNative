//! The notes server end to end (`PLAN.md` Milestone 49's "done when"):
//! authenticated, database-backed, job-processing traffic, through the
//! same typed functions the Windows client calls, and the same view
//! served as a page. The published API contract must hold.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use bytes::Bytes;
use notes_shared::{CreateNote, Credentials, ListNotes, NewNote, NoteSummary, SignIn};
use rustnative_core::server_fn::{ServerFnError, call, fetch_component};
use rustnative_core::{HttpRequest, HttpService, Node};
use rustnative_server::db::Db;
use rustnative_server::jobs::Jobs;
use rustnative_server::local::InProcess;
use rustnative_server::openapi::breaking_changes;
use server_demo::{IndexNote, app, database};

const BASE: &str = "https://notes.example.com";

fn server(name: &str) -> (InProcess, Jobs) {
    let db = Db::memory(name, 2).unwrap();
    database(&db).unwrap();
    let jobs = Jobs::new(db.clone()).unwrap().register::<IndexNote>();
    (InProcess(app(&db, &jobs, [4; 32]).into_service()), jobs)
}

async fn token(http: &InProcess, name: &str, password: &str) -> Result<String, ServerFnError> {
    call::<SignIn>(http, BASE, &Credentials { name: name.into(), password: password.into() }, &[])
        .await
}

#[tokio::test]
async fn a_signed_in_client_writes_notes_that_jobs_index() {
    let (http, jobs) = server("notes-flow");
    assert!(matches!(
        token(&http, "ada", "wrong").await,
        Err(ServerFnError::Server { status: 401, .. })
    ));
    assert!(matches!(
        call::<ListNotes>(&http, BASE, &(), &[]).await,
        Err(ServerFnError::Server { status: 401, .. })
    ));

    let bearer = format!("Bearer {}", token(&http, "grace", "compiler").await.unwrap());
    let auth = [("authorization", bearer.as_str())];
    let note = call::<CreateNote>(&http, BASE, &NewNote { title: "Buy milk".into() }, &auth)
        .await
        .unwrap();
    assert!(!note.indexed);
    assert!(call::<CreateNote>(&http, BASE, &NewNote { title: " ".into() }, &auth).await.is_err());

    jobs.work_until_idle().await;
    let notes = call::<ListNotes>(&http, BASE, &(), &auth).await.unwrap();
    assert_eq!(notes.len(), 1);
    assert!(notes[0].indexed, "the job ran");

    // Another person sees none of them.
    let ada = format!("Bearer {}", token(&http, "ada", "analytical engine").await.unwrap());
    assert_eq!(
        call::<ListNotes>(&http, BASE, &(), &[("authorization", ada.as_str())]).await.unwrap(),
        []
    );

    // The server-only component, merged as an ordinary tree.
    let summary =
        fetch_component::<NoteSummary>(&http, BASE, &"grace".to_owned(), &auth).await.unwrap();
    assert_eq!(summary, Node::label("summary", "1 notes, 1 indexed"));
}

#[tokio::test]
async fn the_browser_gets_the_same_view_as_a_page() {
    let (http, _) = server("notes-page");
    let service = http.0.clone();
    let sign_in_form =
        service.handle(http::Request::get("/").body(Bytes::new()).unwrap(), None).await;
    let cookie = sign_in_form.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let csrf = cookie.trim_start_matches("__Host-csrf=").to_owned();

    let signed_in = service
        .handle(
            http::Request::post("/sign-in")
                .header("cookie", &cookie)
                .body(Bytes::from(format!("name=ada&password=analytical+engine&_csrf={csrf}")))
                .unwrap(),
            None,
        )
        .await;
    assert_eq!(signed_in.status(), http::StatusCode::SEE_OTHER);
    let session = signed_in
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|value| value.to_str().unwrap())
        .find(|value| value.starts_with("__Host-session="))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let page = service
        .handle(
            http::Request::get("/")
                .header("cookie", format!("{cookie}; {session}"))
                .body(Bytes::new())
                .unwrap(),
            None,
        )
        .await;
    let html = String::from_utf8(page.body().to_vec()).unwrap();
    assert!(html.contains("ada&#39;s notes"), "{html}");
    assert!(html.contains("<meta property=\"og:title\""), "the head is rendered");
    let csp = page.headers()["content-security-policy"].to_str().unwrap();
    let nonce = csp.split("'nonce-").nth(1).unwrap().split('\'').next().unwrap();
    assert!(
        html.contains(&format!("<style nonce=\"{nonce}\">")),
        "the page's style passes its own policy"
    );

    // The admin surface is Ada's, not Grace's.
    let admin = service
        .handle(
            http::Request::get("/admin")
                .header("cookie", format!("{cookie}; {session}"))
                .body(Bytes::new())
                .unwrap(),
            None,
        )
        .await;
    assert_eq!(admin.status(), http::StatusCode::OK);
}

#[tokio::test]
async fn the_published_api_contract_holds() {
    let (http, _) = server("notes-contract");
    let response = http.execute(HttpRequest::get(format!("{BASE}/openapi.json"))).await.unwrap();
    let current: serde_json::Value = serde_json::from_slice(response.body_bytes()).unwrap();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/api/v1.json");
    if std::env::var("RUSTNATIVE_BLESS").is_ok_and(|value| value == "1") {
        std::fs::write(path, serde_json::to_string_pretty(&current).unwrap()).unwrap();
    }
    let published: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let breaks = breaking_changes(&published, &current);
    assert!(breaks.is_empty(), "this change breaks clients of v1: {breaks:#?}");

    for path in ["/healthz", "/readyz", "/metrics", "/sitemap.xml"] {
        let response = http.execute(HttpRequest::get(format!("{BASE}{path}"))).await.unwrap();
        assert_eq!(response.status(), 200, "{path}");
    }
}
