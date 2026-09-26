//! Capability grants enforced at each call (`PLAN.md` Milestone 51,
//! `C68`): a scope reaches only the origins it was granted, whatever URL it
//! builds at run time.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::sync::{Arc, Mutex};

use rustnative_core::grant::{Grant, GrantSet};
use rustnative_core::{HttpRequest, HttpResponse, HttpService, ServiceError, Services};

#[derive(Default)]
struct Recorder(Mutex<Vec<String>>);

#[async_trait::async_trait]
impl HttpService for Recorder {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ServiceError> {
        self.0.lock().unwrap().push(request.url().to_owned());
        Ok(HttpResponse::new(200, Vec::new(), Vec::new()))
    }
}

// req: C68-1
#[tokio::test]
async fn a_scope_reaches_only_its_granted_origins() {
    let recorder = Arc::new(Recorder::default());
    let services = Services::default().with_http(recorder.clone());

    let package = services
        .scoped(GrantSet::none().with(Grant::Origins(vec!["https://api.example.com".into()])));
    let http = package.http().expect("an origin is granted");
    assert!(http.get().execute(HttpRequest::get("https://api.example.com/v1/items")).await.is_ok());
    let refused = http
        .get()
        .execute(HttpRequest::get("https://tracker.example.net/collect"))
        .await
        .unwrap_err();
    assert!(refused.to_string().contains("not granted"), "{refused}");
    assert!(
        http.get().execute(HttpRequest::get("https://api.example.com.evil.test/")).await.is_err()
    );
    assert_eq!(
        *recorder.0.lock().unwrap(),
        ["https://api.example.com/v1/items"],
        "refused requests never left"
    );

    assert!(services.scoped(GrantSet::none()).http().is_none(), "no origin, no HTTP at all");
    let own = services.scoped(GrantSet::unrestricted()).http().unwrap();
    assert!(own.get().execute(HttpRequest::get("https://anywhere.example.org/")).await.is_ok());
}
