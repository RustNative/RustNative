//! Rust Native's web macros, re-exported by `rustnative-web`:
//!
//! - `#[client]` on an inline module: client logic in the client subset of
//!   Rust, compiled to JavaScript at build time (`PLAN.md` Web milestone A;
//!   `docs/web/client-subset.md`).
//! - `#[server]` on an `async fn`: a typed server function, one definition
//!   whose handler exists only on the server (Web milestone H).
#![deny(missing_docs)]

use proc_macro::TokenStream;

/// Client logic compiled to JavaScript; see `rustnative_web::client`.
#[proc_macro_attribute]
pub fn client(attribute: TokenStream, item: TokenStream) -> TokenStream {
    rustnative_webgen::client(attribute.into(), item.into()).into()
}

/// A typed server function; see `rustnative_web::server`.
#[proc_macro_attribute]
pub fn server(attribute: TokenStream, item: TokenStream) -> TokenStream {
    rustnative_webgen::server::server(attribute.into(), item.into()).into()
}
