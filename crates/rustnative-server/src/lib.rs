//! The server application model (`PLAN.md` Milestone 49).
//!
//! A server built from the same contracts as the client, not a second
//! framework with the same name:
//!
//! - routes use `rustnative_core::Route` patterns;
//! - request work runs in a [`RequestScope`] cancelled when the response
//!   is sent, as a component's tasks are cancelled when it unmounts;
//! - the application is a `tower::Service` over `http` types (`C37`), so
//!   it is served by `hyper` or mounted inside an existing service.
//!
//! ```
//! use rustnative_server::{Path, ServerApp, get};
//!
//! async fn greet(Path(name): Path<String>) -> String {
//!     format!("Hello, {name}")
//! }
//!
//! let app = ServerApp::new().route("/hello/:name", get(greet).public());
//! # let service = app.into_service();
//! # let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
//! # let response = runtime.block_on(service.handle(
//! #     http::Request::get("/hello/Ada").body(bytes::Bytes::new()).unwrap(), None));
//! # assert_eq!(response.body(), "Hello, Ada");
//! ```
//!
//! Secure by default (`docs/server/security-checklist.md`): request-forgery
//! protection, a content security policy with a nonce, secure cookies,
//! rate limiting, a body size limit, and escaping [`Html`] are on unless an
//! application turns them off.

#[cfg(feature = "db")]
pub mod admin;
pub mod app;
pub mod assets;
pub mod auth;
pub mod body;
pub mod cache;
pub mod components;
pub mod config;
#[cfg(feature = "db")]
pub mod db;
#[cfg(feature = "serve")]
pub mod deploy;
pub mod functions;
pub mod handler;
pub mod head;
#[cfg(feature = "db")]
pub mod jobs;
pub mod local;
pub mod openapi;
pub mod push;
pub mod render;
pub mod request;
pub mod response;
pub mod scope;
pub mod security;
pub mod serverless;
pub mod web;

#[cfg(feature = "serve")]
pub use app::Upgrade;
pub use app::{AppService, RouteInfo, ServerApp};
pub use body::{Body, Chunks, streamed};
pub use handler::{Guarded, Handler, MethodRouter, Unguarded, delete, get, patch, post, put};
pub use request::{Form, FromRequest, Path, Query, RequestContext, State};
pub use response::{Html, IntoResponse, Json, Redirect, Response, ServerError};
pub use scope::RequestScope;
pub use security::{Cookie, CspNonce, CsrfToken, Security, constant_time_eq, random_token};

pub use rustnative_server_macros::query;
#[doc(hidden)]
pub use serde;
