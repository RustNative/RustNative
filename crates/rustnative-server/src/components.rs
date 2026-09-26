//! Server-only components (`C05`): rendered on the server from props that
//! crossed the boundary, their output sent as a tree payload
//! (`rustnative_core::wire`) that the client merges with its ordinary
//! reconciler. The rendering — and whatever it reaches (the database, a
//! secret) — exists only in the server build.

use std::future::Future;

use rustnative_core::Node;
use rustnative_core::server_fn::ServerComponentDef;
use rustnative_core::wire::WireNode;

use crate::handler::{Guarded, MethodRouter, post};
use crate::response::{Json, ServerError};

/// A route rendering `C` with `render`.
pub fn server_component<C, H, Fut>(render: H) -> MethodRouter
where
    C: ServerComponentDef,
    H: Fn(C::Props) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<Node, ServerError>> + Send + 'static,
{
    post(move |Json(props): Json<C::Props>| {
        let render = render.clone();
        async move { render(props).await.map(|node| Json(WireNode::from_node(&node))) }
    })
}

impl crate::ServerApp {
    /// Serves `C` at its path.
    #[must_use]
    pub fn component<C: ServerComponentDef>(self, router: MethodRouter<Guarded>) -> Self {
        self.route(&C::url(""), router)
    }
}
