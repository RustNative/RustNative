//! `#[server]`: a typed server function from one `async fn` (`PLAN.md` Web
//! milestone H, `W-MF-1`).
//!
//! ```ignore
//! #[rustnative_web::server]
//! pub async fn create_note(input: NewNote, session: Session) -> Result<Note, ServerError> {
//!     // server-only code: the database, the session
//! }
//! ```
//!
//! defines, in the crate both sides depend on:
//!
//! - `pub struct CreateNote;` implementing `ServerFn` — its path
//!   (`create_note`, or `#[server(path = "…")]`), its input (the first
//!   parameter's type, `()` when there is none), and its output (the `Ok`
//!   type) — which client logic calls with `fx.call::<CreateNote>(..)` and a
//!   native client with `server_fn::call::<CreateNote>(..)`;
//! - with the crate's `server` feature only (or the feature
//!   `#[server(feature = "…")]` names): the function itself and
//!   `CreateNote::route()`, the route serving it, taking the remaining
//!   parameter (at most one) as an extractor.
//!
//! Because the body exists only under the feature, code it reaches — a
//! database crate, the server crate — is a compile error, not a runtime
//! one, if client code reaches for it (`C05-2`).

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::spanned::Spanned;

fn pascal(name: &str) -> String {
    name.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect()
}

fn ok_type(output: &syn::ReturnType) -> Option<syn::Type> {
    let syn::ReturnType::Type(_, ty) = output else { return None };
    let syn::Type::Path(path) = &**ty else { return None };
    let segment = path.path.segments.last()?;
    if segment.ident != "Result" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else { return None };
    match args.args.first()? {
        syn::GenericArgument::Type(ty) => Some(ty.clone()),
        _ => None,
    }
}

/// What `#[server]` expands to.
#[must_use]
pub fn server(attribute: TokenStream, item: TokenStream) -> TokenStream {
    let mut path_override: Option<String> = None;
    let mut feature = "server".to_owned();
    let parser = syn::meta::parser(|meta| {
        let value: syn::LitStr = meta.value()?.parse()?;
        if meta.path.is_ident("path") {
            path_override = Some(value.value());
        } else if meta.path.is_ident("feature") {
            feature = value.value();
        } else {
            return Err(meta.error("`#[server]` takes `path = \"…\"` and `feature = \"…\"`"));
        }
        Ok(())
    });
    if let Err(error) = syn::parse::Parser::parse2(parser, attribute) {
        return error.to_compile_error();
    }
    let function: syn::ItemFn = match syn::parse2(item) {
        Ok(function) => function,
        Err(error) => {
            return syn::Error::new(error.span(), "`#[server]` goes on an `async fn`")
                .to_compile_error();
        }
    };
    if function.sig.asyncness.is_none() {
        return syn::Error::new(function.sig.fn_token.span(), "a server function is an `async fn`")
            .to_compile_error();
    }
    let Some(output) = ok_type(&function.sig.output) else {
        return syn::Error::new(
            function.sig.output.span(),
            "a server function returns `Result<Output, ServerError>`: its `Ok` type is what callers receive",
        )
        .to_compile_error();
    };
    let params: Vec<&syn::PatType> = function
        .sig
        .inputs
        .iter()
        .filter_map(|input| match input {
            syn::FnArg::Typed(typed) => Some(typed),
            syn::FnArg::Receiver(_) => None,
        })
        .collect();
    if params.len() > 2 {
        return syn::Error::new(
            params[2].span(),
            "a server function takes its input and at most one extractor (a tuple of extractors is one)",
        )
        .to_compile_error();
    }
    let input: syn::Type =
        params.first().map_or_else(|| syn::parse_quote!(()), |param| (*param.ty).clone());
    let name = function.sig.ident.clone();
    let definition = format_ident!("{}", pascal(&name.to_string()), span = name.span());
    let path = path_override.unwrap_or_else(|| name.to_string());
    let vis = &function.vis;
    let doc = format!(
        "The server function `{name}` (`/_fn/{path}`): call it with `fx.call::<{definition}>(..)` from client logic."
    );
    let route = match params.len() {
        0 => quote! { ::rustnative_server::functions::server_fn::<Self, _, _>(|(): ()| #name()) },
        1 => quote! { ::rustnative_server::functions::server_fn::<Self, _, _>(#name) },
        _ => quote! { ::rustnative_server::functions::server_fn_with::<Self, _, _, _>(#name) },
    };
    let _ = Span::call_site();
    quote! {
        #[doc = #doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
        #vis struct #definition;

        impl ::rustnative_core::server_fn::ServerFn for #definition {
            const PATH: &'static str = #path;
            type Input = #input;
            type Output = #output;
        }

        #[cfg(feature = #feature)]
        impl #definition {
            /// The route serving this function; give it its access
            /// (`.public()`, `.signed_in()`, …) and pass it to
            /// `ServerApp::function`.
            #[must_use]
            pub fn route() -> ::rustnative_server::MethodRouter {
                #route
            }
        }

        #[cfg(feature = #feature)]
        #function
    }
}
