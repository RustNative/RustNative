//! The client-subset translator (`PLAN.md` Web milestone A): client logic,
//! written as ordinary Rust in a restricted subset, compiled to JavaScript
//! when the crate is built — once per build, not per request, so the output
//! is a cacheable static asset and a strict content security policy needs no
//! nonces for it.
//!
//! [`client`] is what `#[rustnative_web::client]` runs. It reads a module
//! holding a state type and its `update`, `view`, and optionally `message`
//! and `init`, and returns:
//!
//! - the module's Rust, as written, except that integer arithmetic becomes
//!   `rustnative_web::client::rt`'s checked arithmetic and `rsx!` its
//!   builder expansion (so the same arithmetic is checked in both
//!   languages);
//! - `impl ClientLogic` for the state type, carrying the generated
//!   JavaScript, the CSS its class strings need, the server functions it
//!   calls, and a line table back to the Rust (a source map);
//! - `pub type Island = rustnative_web::Client<State>`.
//!
//! What the subset is, and why each rule is there, is
//! `docs/web/client-subset.md`. Anything outside it is a compile error at
//! the construct, naming the three ways forward.

#![deny(missing_docs)]
#![allow(
    clippy::too_many_lines,
    clippy::missing_errors_doc,
    clippy::match_same_arms,
    clippy::single_match_else,
    clippy::option_option,
    clippy::type_complexity,
    clippy::items_after_statements,
    clippy::unnecessary_wraps,
    clippy::manual_let_else,
    clippy::must_use_candidate,
    clippy::unnested_or_patterns,
    missing_docs,
    reason = "a compiler's internals: tables of one arm per construct, where arms that read alike are still separate rules"
)]

mod emit;
mod expr;
mod items;
mod methods;
mod pat;
pub mod server;
mod style;
mod ty;

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};

pub use items::Module;

/// The message every refusal ends with.
pub const WAYS_FORWARD: &str = "Ways forward: mark the function `#[server]` to run it on the server \
     and call it with `fx.call::<F>(..)`; move this logic into a WebAssembly subtree \
     (`rustnative_web::wasm_subtree!`), which runs full Rust in the browser; or call hand-written \
     JavaScript with `fx.js(..)` (docs/web/client-subset.md)";

/// A refusal: `what` is outside the subset, because `why`.
pub fn refuse(span: Span, what: &str, why: &str) -> syn::Error {
    syn::Error::new(span, format!("{what} is outside the client subset: {why}. {WAYS_FORWARD}"))
}

/// The translation of one `#[client]` module.
#[derive(Debug)]
pub struct Translation {
    /// The generated JavaScript module.
    pub js: String,
    /// For each JavaScript line, the span of the Rust it came from.
    pub lines: Vec<Option<Span>>,
    /// The `(class, rule)` pairs its class strings need.
    pub css: Vec<(String, String)>,
    /// The tokens those rules refer to.
    pub tokens: Vec<String>,
    /// The server functions it calls, by the type path written.
    pub server_fns: Vec<syn::Path>,
    /// The state type's name.
    pub state: syn::Ident,
    /// The message type, as written in `update`'s `Effects<M>`.
    pub message: syn::Type,
    /// Which optional methods the state type has.
    pub has_message: bool,
    /// Whether it has `init`.
    pub has_init: bool,
}

/// Translates `module`, rewriting its Rust in place; errors are every
/// refusal found, each at its construct.
pub fn translate(module: &mut syn::ItemMod) -> Result<Translation, Vec<syn::Error>> {
    items::translate_module(module)
}

/// What `#[rustnative_web::client]` expands to.
#[must_use]
#[allow(
    clippy::needless_pass_by_value,
    reason = "the signature of the proc-macro attribute it serves"
)]
pub fn client(attribute: TokenStream, item: TokenStream) -> TokenStream {
    if !attribute.is_empty() {
        return syn::Error::new(Span::call_site(), "`#[client]` takes no arguments")
            .to_compile_error();
    }
    let mut module: syn::ItemMod = match syn::parse2(item) {
        Ok(module) => module,
        Err(error) => {
            return syn::Error::new(
                error.span(),
                "`#[client]` goes on an inline module holding a state type and its `update` and `view`: \
                 `#[client] mod counter { .. }`",
            )
            .to_compile_error();
        }
    };
    let translation = match translate(&mut module) {
        Ok(translation) => translation,
        Err(errors) => {
            let errors = errors.iter().map(syn::Error::to_compile_error);
            // The module is still emitted, so the refusals are the only
            // errors the developer sees.
            return quote!(#(#errors)* #module);
        }
    };
    let Translation { js, lines, css, tokens, server_fns, state, message, has_message, has_init } =
        translation;
    drop_rsx_imports(&mut module);
    let name = module.ident.to_string();
    let lines = lines.iter().map(|span| match span {
        Some(span) => quote_spanned!(*span=> ::core::line!()),
        None => quote!(0),
    });
    let css = css.iter().map(|(class, rule)| quote!((#class, #rule)));
    let fn_names = server_fns.iter().map(|path| {
        let name = ty::last_segment(path);
        quote!((#name, <#path as ::rustnative_core::server_fn::ServerFn>::PATH))
    });
    let message_impl = has_message.then(|| {
        quote! {
            fn message(&mut self, message: Self::Message, fx: &mut ::rustnative_web::Effects<Self::Message>) {
                #state::message(self, message, fx);
            }
        }
    });
    let init_impl = has_init.then(|| {
        quote! {
            fn init(&mut self, fx: &mut ::rustnative_web::Effects<Self::Message>) {
                #state::init(self, fx);
            }
        }
    });
    let module_const = format_ident!("__RUSTNATIVE_CLIENT_MODULE");
    let generated = quote! {
        #[doc(hidden)]
        pub static #module_const: ::rustnative_web::ClientModule = ::rustnative_web::ClientModule {
            name: #name,
            js: #js,
            css: &[#(#css),*],
            tokens: &[#(#tokens),*],
            server_fns: &[#(#fn_names),*],
            file: ::core::file!(),
            lines: &[#(#lines),*],
        };

        impl ::rustnative_web::ClientLogic for #state {
            type Message = #message;
            const MODULE: &'static ::rustnative_web::ClientModule = &#module_const;

            fn update(&mut self, event: ::rustnative_core::Event, fx: &mut ::rustnative_web::Effects<Self::Message>) {
                #state::update(self, event, fx);
            }
            #message_impl
            #init_impl
            fn view(&self) -> ::rustnative_core::Node {
                #state::view(self)
            }
        }

        /// The client component: a `Component` whose props are its initial
        /// state.
        pub type Island = ::rustnative_web::Client<#state>;
    };
    if let Some((_, content)) = &mut module.content {
        match syn::parse2::<syn::File>(generated) {
            Ok(file) => content.extend(file.items),
            Err(error) => return error.to_compile_error(),
        }
    }
    quote!(#module)
}

/// Drops `rsx` from the module's imports once every `rsx!` in it has been
/// replaced by its expansion, so the import is not reported unused.
fn drop_rsx_imports(module: &mut syn::ItemMod) {
    use quote::ToTokens;
    fn keep(tree: &mut syn::UseTree) -> bool {
        match tree {
            syn::UseTree::Name(name) => name.ident != "rsx",
            syn::UseTree::Path(path) => keep(&mut path.tree),
            syn::UseTree::Group(group) => {
                group.items = std::mem::take(&mut group.items)
                    .into_pairs()
                    .filter_map(|mut pair| keep(pair.value_mut()).then_some(pair))
                    .collect();
                !group.items.is_empty()
            }
            syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => true,
        }
    }
    let Some((_, items)) = &mut module.content else { return };
    if items.iter().any(|item| {
        !matches!(item, syn::Item::Use(_)) && item.to_token_stream().to_string().contains("rsx !")
    }) {
        return;
    }
    items.retain_mut(
        |item| if let syn::Item::Use(item) = item { keep(&mut item.tree) } else { true },
    );
}
