//! Typed server functions (`PLAN.md` Milestone 49): one definition, checked
//! at both call sites.
//!
//! A [`ServerFn`] names a path, an input type, and an output type. The
//! server registers a handler for it (`rustnative_server::server_fn`); a
//! client — the Windows application, a test, another service — calls it
//! with [`call`] over the `HttpService` contract. Both sides use the same
//! types, so a change to either breaks the build on both, not a request in
//! production. The definition lives in a crate both depend on, and needs
//! nothing of the server.
//!
//! ```
//! use rustnative_core::api_schema::{ApiSchema, object};
//! use rustnative_core::server_fn::ServerFn;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! pub struct AddNote { pub title: String }
//! impl ApiSchema for AddNote {
//!     fn schema() -> serde_json::Value { object("AddNote", [("title", String::schema(), true)]) }
//! }
//! #[derive(Serialize, Deserialize)]
//! pub struct NoteId(pub i64);
//! impl ApiSchema for NoteId {
//!     fn schema() -> serde_json::Value { i64::schema() }
//! }
//!
//! pub struct CreateNote;
//! impl ServerFn for CreateNote {
//!     const PATH: &'static str = "notes/create";
//!     type Input = AddNote;
//!     type Output = NoteId;
//! }
//! assert_eq!(CreateNote::url("https://example.com"), "https://example.com/_fn/notes/create");
//! ```

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::api_schema::ApiSchema;
use crate::services::{HttpRequest, HttpService, Method};

/// A server function's definition.
pub trait ServerFn: 'static {
    /// Its path below `/_fn/`.
    const PATH: &'static str;
    /// What the caller sends.
    type Input: Serialize + DeserializeOwned + ApiSchema + Send + 'static;
    /// What the server returns.
    type Output: Serialize + DeserializeOwned + ApiSchema + Send + 'static;

    /// Its URL on the server at `base`.
    #[must_use]
    fn url(base: &str) -> String {
        format!("{}/_fn/{}", base.trim_end_matches('/'), Self::PATH)
    }
}

/// Why a call failed. Serializable, so a browser's generated client code
/// and a native client see a failed call as the same value.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ServerFnError {
    /// The request did not complete.
    Transport(String),
    /// The server answered with an error status and message.
    Server {
        /// The status.
        status: u16,
        /// The message the server showed.
        message: String,
    },
    /// The answer was not the declared output.
    Decode(String),
}

impl std::fmt::Display for ServerFnError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(error) => write!(formatter, "the call did not complete: {error}"),
            Self::Server { status, message } => write!(formatter, "{status}: {message}"),
            Self::Decode(error) => write!(formatter, "unexpected answer: {error}"),
        }
    }
}

impl std::error::Error for ServerFnError {}

/// Calls `F` on the server at `base` with `input`. `headers` carry what
/// the caller authenticates with (`Authorization: Bearer …`).
///
/// # Errors
///
/// The call did not complete, the server refused, or its answer was not
/// `F::Output`.
pub async fn call<F: ServerFn>(
    http: &dyn HttpService,
    base: &str,
    input: &F::Input,
    headers: &[(&str, &str)],
) -> Result<F::Output, ServerFnError> {
    let body =
        serde_json::to_vec(input).map_err(|error| ServerFnError::Decode(error.to_string()))?;
    let mut request = HttpRequest::new(Method::Post, F::url(base))
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .body(body);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response =
        http.execute(request).await.map_err(|error| ServerFnError::Transport(error.to_string()))?;
    if !(200..300).contains(&response.status()) {
        let message = serde_json::from_slice::<serde_json::Value>(response.body_bytes())
            .ok()
            .and_then(|value| value.get("error")?.as_str().map(str::to_owned))
            .unwrap_or_default();
        return Err(ServerFnError::Server { status: response.status(), message });
    }
    serde_json::from_slice(response.body_bytes())
        .map_err(|error| ServerFnError::Decode(error.to_string()))
}

/// A server-only component's definition (`C05`): its name and the props
/// that cross the boundary — `Serialize` and `DeserializeOwned`, checked at
/// compile time where the definition is written. Its code lives only in
/// the server (`rustnative_server::components`); a client holds this
/// definition and [`fetch_component`], never the rendering.
pub trait ServerComponentDef: 'static {
    /// Its name below `/_component/`.
    const NAME: &'static str;
    /// Its props.
    type Props: Serialize + DeserializeOwned + Send + 'static;

    /// Its URL on the server at `base`.
    #[must_use]
    fn url(base: &str) -> String {
        format!("{}/_component/{}", base.trim_end_matches('/'), Self::NAME)
    }
}

/// Renders `C` on the server at `base` with `props`, and returns its tree
/// for the ordinary reconciler (see [`crate::wire`]).
///
/// # Errors
///
/// The call did not complete, the server refused, or the answer was not a
/// tree.
pub async fn fetch_component<C: ServerComponentDef>(
    http: &dyn HttpService,
    base: &str,
    props: &C::Props,
    headers: &[(&str, &str)],
) -> Result<crate::Node, ServerFnError> {
    let body =
        serde_json::to_vec(props).map_err(|error| ServerFnError::Decode(error.to_string()))?;
    let mut request = HttpRequest::new(Method::Post, C::url(base))
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .body(body);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response =
        http.execute(request).await.map_err(|error| ServerFnError::Transport(error.to_string()))?;
    if !(200..300).contains(&response.status()) {
        return Err(ServerFnError::Server { status: response.status(), message: String::new() });
    }
    serde_json::from_slice::<crate::wire::WireNode>(response.body_bytes())
        .map(crate::wire::WireNode::into_node)
        .map_err(|error| ServerFnError::Decode(error.to_string()))
}
