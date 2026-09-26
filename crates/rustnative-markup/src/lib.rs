//! The markup syntax of Rust Native (`PLAN.md` 2.9, Milestone 53).
//!
//! One grammar, owned here, with two carriers:
//!
//! - **the `rsx!` macro** (`rustnative-macros`, re-exported by
//!   `rustnative-core`), which is [`parse_markup`] + [`lower()`];
//! - **`.rsx` files**, which [`rsx_file::compile`] lowers by wrapping each
//!   markup expression in `rsx!` and changing nothing else.
//!
//! The CLI links the same crate for `rustnative expand`, `fmt`, and `lsp`,
//! so there is exactly one parser, one lowering, and one element table
//! ([`table::element_table`]).
//!
//! ```
//! use rustnative_markup::{lower, parse_markup};
//!
//! let markup = parse_markup(quote::quote! {
//!     <Column key="root">
//!         <Label key="greeting" text="Hello" />
//!     </Column>
//! })?;
//! let builder = lower(&markup)?.to_string();
//! assert!(builder.contains("column_with_layout"));
//! assert!(builder.contains("label_with_layout"));
//! # Ok::<(), syn::Error>(())
//! ```
#![deny(missing_docs)]

pub mod ast;
pub mod format;
pub mod lower;
pub mod parse;
pub mod rsx_file;
mod rsx_scan;
pub mod table;

pub use ast::{Attr, AttrValue, Child, Element, Markup};
pub use format::{format_element_at, format_markup};
pub use lower::lower;
pub use parse::{parse_leading_element, parse_markup};
pub use rsx_file::{CompileOptions, RsxError, RsxOutput, SourceMap, compile};
pub use table::{AttrKind, AttrSpec, Constructor, ElementSpec, element_spec, element_table};

/// Parses and lowers `tokens` in one step — what `rsx!` does.
///
/// # Errors
///
/// A parse or lowering error, spanned to the offending markup.
pub fn expand(tokens: proc_macro2::TokenStream) -> syn::Result<proc_macro2::TokenStream> {
    lower(&parse_markup(tokens)?)
}
