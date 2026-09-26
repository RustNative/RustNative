//! The feature kits `rustnative generate kit` writes (`PLAN.md` Milestone
//! 52, `C57-3`), compiled and exercised as generated: an account signs up,
//! signs in, and is seen; the first account administers and the second
//! does not; a purchase is validated before its entitlement is recorded.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use bytes::Bytes;
use kits::{admin, auth, commerce};
use rustnative_core::product::Product;
use rustnative_server::db::Db;
use rustnative_server::{AppService, ServerApp};

struct Browser {
    service: AppService,
    cookies: Vec<(String, String)>,
}

impl Browser {
    /// A browser that has loaded a page, so it holds the request-forgery
    /// cookie a form would send back.
    async fn new(service: AppService) -> Self {
        let mut browser = Self { service, cookies: Vec::new() };
        browser.send("GET", "/store/products", None).await;
        browser
    }

    fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn csrf(&self) -> String {
        self.cookies
            .iter()
            .find(|(name, _)| name == "__Host-csrf")
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    }

    async fn send(
        &mut self,
        method: &str,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> (u16, String) {
        let mut request = http::Request::builder()
            .method(method)
            .uri(path)
            .header("cookie", self.cookie_header());
        if body.is_some() {
            request = request
                .header("content-type", "application/json")
                .header("x-csrf-token", self.csrf());
        }
        let body = body.map_or_else(Bytes::new, |body| Bytes::from(body.to_string()));
        let response = self.service.handle(request.body(body).unwrap(), None).await;
        for value in response.headers().get_all("set-cookie") {
            let pair = value.to_str().unwrap().split(';').next().unwrap();
            let (name, value) = pair.split_once('=').unwrap();
            self.cookies.retain(|(existing, _)| existing != name);
            if !value.is_empty() {
                self.cookies.push((name.to_owned(), value.to_owned()));
            }
        }
        (response.status().as_u16(), String::from_utf8(response.body().to_vec()).unwrap())
    }
}

fn server(name: &str) -> (AppService, Db) {
    let db = Db::memory(name, 2).unwrap();
    auth::migrate(&db).unwrap();
    commerce::migrate(&db).unwrap();
    let store = commerce::Store::new(vec![Product {
        id: "pro".into(),
        title: "Pro".into(),
        price: "$5".into(),
        subscription: true,
    }]);
    let app = auth::routes(ServerApp::new(), &db, [7; 32]);
    let app = admin::routes(app, &db);
    let app = commerce::routes(app, &db, store);
    (app.into_service(), db)
}

#[tokio::test]
async fn an_account_signs_up_signs_in_and_signs_out() {
    let (service, _) = server("kit-auth");
    let mut browser = Browser::new(service).await;
    assert_eq!(browser.send("GET", "/auth/me", None).await.0, 401, "no one is signed in");

    let credentials = serde_json::json!({ "name": "ada", "password": "analytical engine" });
    assert_eq!(
        browser
            .send(
                "POST",
                "/auth/sign-up",
                Some(serde_json::json!({ "name": "ada", "password": "short" }))
            )
            .await
            .0,
        400
    );
    let (status, body) = browser.send("POST", "/auth/sign-up", Some(credentials.clone())).await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("\"admin\":true"), "the first account administers: {body}");
    assert_eq!(
        browser.send("POST", "/auth/sign-up", Some(credentials.clone())).await.0,
        400,
        "a name is taken once"
    );
    assert!(browser.send("GET", "/auth/me", None).await.1.contains("\"name\":\"ada\""));

    browser.send("POST", "/auth/sign-out", Some(serde_json::json!(null))).await;
    assert_eq!(browser.send("GET", "/auth/me", None).await.0, 401, "signed out");
    let wrong = serde_json::json!({ "name": "ada", "password": "difference engine" });
    assert_eq!(browser.send("POST", "/auth/sign-in", Some(wrong)).await.0, 401);
    assert_eq!(browser.send("POST", "/auth/sign-in", Some(credentials)).await.0, 200);
    assert_eq!(browser.send("GET", "/auth/me", None).await.0, 200);
}

#[tokio::test]
async fn only_administrators_reach_the_admin_surface() {
    let (service, _) = server("kit-admin");
    let mut ada = Browser::new(service.clone()).await;
    ada.send(
        "POST",
        "/auth/sign-up",
        Some(serde_json::json!({ "name": "ada", "password": "analytical engine" })),
    )
    .await;
    let mut grace = Browser::new(service).await;
    grace
        .send(
            "POST",
            "/auth/sign-up",
            Some(serde_json::json!({ "name": "grace", "password": "compilers!" })),
        )
        .await;

    let (status, page) = ada.send("GET", "/admin", None).await;
    assert_eq!(status, 200);
    assert!(page.contains("accounts"), "the admin surface lists the tables");
    assert_eq!(grace.send("GET", "/admin", None).await.0, 403);
}

#[tokio::test]
async fn a_purchase_is_validated_before_it_is_owned() {
    let (service, _) = server("kit-commerce");
    let mut buyer = Browser::new(service).await;
    let (_, products) = buyer.send("GET", "/store/products", None).await;
    assert!(products.contains("\"pro\""), "{products}");
    let (status, receipt) =
        buyer.send("POST", "/store/ada/checkout", Some(serde_json::json!("pro"))).await;
    assert_eq!(status, 200, "{receipt}");
    assert_eq!(
        buyer.send("POST", "/store/ada/checkout", Some(serde_json::json!("free-lunch"))).await.0,
        400
    );
    assert_eq!(buyer.send("GET", "/store/ada/owned", None).await.1, "[\"pro\"]");
    assert_eq!(buyer.send("GET", "/store/grace/owned", None).await.1, "[]");
}
