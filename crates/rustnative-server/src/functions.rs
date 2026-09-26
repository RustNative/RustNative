//! Serving typed server functions (`rustnative_core::server_fn`): the
//! handler's input and output are the definition's, so the server and its
//! callers cannot disagree.
//!
//! ```
//! # use rustnative_core::api_schema::{ApiSchema, object};
//! # use serde::{Deserialize, Serialize};
//! # #[derive(Serialize, Deserialize)] pub struct Add { a: i64, b: i64 }
//! # impl ApiSchema for Add { fn schema() -> serde_json::Value { object("Add", [("a", i64::schema(), true), ("b", i64::schema(), true)]) } }
//! use rustnative_core::server_fn::ServerFn;
//! use rustnative_server::{ServerApp, ServerError, functions::server_fn};
//!
//! struct Sum;
//! impl ServerFn for Sum {
//!     const PATH: &'static str = "sum";
//!     type Input = Add;
//!     type Output = i64;
//! }
//!
//! let app = ServerApp::new()
//!     .function::<Sum>(server_fn::<Sum, _, _>(|add: Add| async move { Ok::<_, ServerError>(add.a + add.b) }).public());
//! assert_eq!(app.routes()[0].pattern, "/_fn/sum");
//! ```

use std::future::Future;

use rustnative_core::server_fn::ServerFn;

use crate::handler::{Guarded, MethodRouter, post};
use crate::request::FromRequest;
use crate::response::{Json, ServerError};

/// A route answering `F` with `handler`.
pub fn server_fn<F, H, Fut>(handler: H) -> MethodRouter
where
    F: ServerFn,
    H: Fn(F::Input) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<F::Output, ServerError>> + Send + 'static,
{
    post(move |Json(input): Json<F::Input>| {
        let handler = handler.clone();
        async move { handler(input).await.map(Json) }
    })
}

/// A route answering `F` with `handler`, which also takes an extractor —
/// the principal, the database, the scope.
pub fn server_fn_with<F, E, H, Fut>(handler: H) -> MethodRouter
where
    F: ServerFn,
    E: FromRequest + Send + 'static,
    H: Fn(F::Input, E) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<F::Output, ServerError>> + Send + 'static,
{
    post(move |Json(input): Json<F::Input>, extra: E| {
        let handler = handler.clone();
        async move { handler(input, extra).await.map(Json) }
    })
}

impl crate::ServerApp {
    /// Serves `F` at its path, and describes it in the API schema.
    #[must_use]
    pub fn function<F: ServerFn>(self, router: MethodRouter<Guarded>) -> Self {
        let path = F::url("");
        self.describe(crate::openapi::Operation::function::<F>(&path)).route(&path, router)
    }
}
