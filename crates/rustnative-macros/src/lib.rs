//! Procedural macros for Rust Native.
//!
//! A thin shell over `rustnative-markup` and `rustnative-style`: the macros
//! here parse and lower with those crates and report their errors with
//! native spans, so a diagnostic points at the attribute, element, or class
//! string in the `.rs` file with no remapping. Use them through
//! `rustnative-core`, which re-exports them.
#![deny(missing_docs)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::SystemTime;

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{quote, quote_spanned};
use rustnative_style::{LINUX, StyleSupport, StyleValue, Vocabulary, WINDOWS};

/// The markup syntax, delimited, inside any `.rs` file (`PLAN.md` 2.9).
///
/// Evaluates to a `rustnative_core::Node`. A tree containing component
/// elements names its context first: `rsx! { in context, <Screen key="s" /> }`.
/// See `rustnative-core`'s documentation for the grammar, and a `.rsx` file
/// for the same grammar with no wrapper.
#[proc_macro]
pub fn rsx(input: TokenStream) -> TokenStream {
    match rustnative_markup::expand(input.into()) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Utility classes (Tailwind CSS v4's vocabulary), compiled: evaluates to a
/// `rustnative_core::style::DeclarationSet`. See `rustnative-core`'s
/// documentation.
#[proc_macro]
pub fn classes(input: TokenStream) -> TokenStream {
    style_macro(input.into(), Kind::Classes).into()
}

/// A declaration block (`padding: 1rem; color: var(--color-red-500)`),
/// compiled: evaluates to a `rustnative_core::style::DeclarationSet`.
#[proc_macro]
pub fn styles(input: TokenStream) -> TokenStream {
    style_macro(input.into(), Kind::Declarations).into()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Classes,
    Declarations,
}

fn error(span: Span, message: &str) -> proc_macro2::TokenStream {
    quote_spanned!(span=> ::core::compile_error!(#message))
}

/// The project's style file: `[style] file` in `rustnative.toml`, else
/// `app.css`, beside the crate's manifest.
fn style_file() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?);
    let configured = std::fs::read_to_string(root.join("rustnative.toml")).ok().and_then(|text| {
        let mut in_style = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                in_style = line == "[style]";
            } else if in_style {
                if let Some(value) =
                    line.strip_prefix("file").map(str::trim).and_then(|rest| rest.strip_prefix('='))
                {
                    return Some(value.trim().trim_matches('"').to_owned());
                }
            }
        }
        None
    });
    let path = root.join(configured.as_deref().unwrap_or("app.css"));
    path.is_file().then_some(path)
}

type Cache = HashMap<PathBuf, (Option<SystemTime>, Result<Vocabulary, String>)>;

/// The vocabulary for this crate, cached per style file and modification
/// time (an editor's long-lived macro server sees edits).
fn vocabulary(path: Option<&Path>) -> Result<Vocabulary, String> {
    static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| Mutex::new(HashMap::new()));
    let Some(path) = path else { return Ok(Vocabulary::defaults()) };
    let modified = std::fs::metadata(path).and_then(|metadata| metadata.modified()).ok();
    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((stamp, vocabulary)) = cache.get(path) {
        if *stamp == modified {
            return vocabulary.clone();
        }
    }
    let loaded = std::fs::read_to_string(path)
        .map_err(|cause| format!("reading {}: {cause}", path.display()))
        .and_then(|source| {
            Vocabulary::with_style_file(&source).map_err(|errors| {
                let problems: Vec<String> = errors
                    .iter()
                    .map(|problem| {
                        let (line, column) = problem.line_column(&source);
                        format!("{}:{line}:{column}: {}", path.display(), problem.message)
                    })
                    .collect();
                problems.join("\n")
            })
        });
    cache.insert(path.to_path_buf(), (modified, loaded.clone()));
    loaded
}

fn style_macro(input: proc_macro2::TokenStream, kind: Kind) -> proc_macro2::TokenStream {
    let name = if kind == Kind::Classes { "classes" } else { "styles" };
    let Ok(literal) = syn::parse2::<syn::LitStr>(input.clone()) else {
        let span = input.into_iter().next().map_or_else(Span::call_site, |token| token.span());
        return error(
            span,
            &format!(
                "`{name}!` takes one string literal: a class name is resolved where it is written, so a \
                 computed one would style nothing silently — choose between literal strings with `if`"
            ),
        );
    };
    let span = literal.span();
    let file = style_file();
    let vocabulary = match vocabulary(file.as_deref()) {
        Ok(vocabulary) => vocabulary,
        Err(message) => {
            return error(span, &format!("the project's style file does not compile:\n{message}"));
        }
    };
    let text = literal.value();
    let resolve = |text: &str| match kind {
        Kind::Classes => vocabulary.resolve_classes(text),
        Kind::Declarations => vocabulary.resolve_declarations(text),
    };
    let declarations = match resolve(&text) {
        Ok(declarations) => declarations,
        Err(problems) => {
            // `Literal::subspan` is unstable, so every problem points at the
            // string; each names the class (or declaration) it is about.
            return problems.iter().map(|problem| error(span, &problem.message)).collect();
        }
    };
    // An unavailable property fails the build for the target whose
    // backend cannot realize it, at the class that set it (2.14).
    let pieces: Vec<String> = match kind {
        Kind::Classes => text.split_whitespace().map(str::to_owned).collect(),
        Kind::Declarations => text
            .split(';')
            .map(str::trim)
            .filter(|piece| !piece.is_empty())
            .map(str::to_owned)
            .collect(),
    };
    // Each declaration's provenance: the class (or declaration) it came
    // from, for the inspector (`PLAN.md` Milestone 44, `C18-2`).
    let sources: Vec<String> = pieces
        .iter()
        .flat_map(|piece| {
            let count = resolve(piece).map_or(0, |declarations| declarations.len());
            std::iter::repeat_n(piece.clone(), count)
        })
        .collect();
    // One native backend per operating system today, so the target OS
    // selects the table; the error is emitted under that OS's `cfg`, which
    // the *application's* build evaluates.
    let targets = [(WINDOWS, "windows", "WINDOWS"), (LINUX, "linux", "LINUX")];
    let unavailable = pieces.iter().flat_map(|piece| {
        let declarations = resolve(piece).unwrap_or_default();
        declarations.into_iter().flat_map(move |declaration| {
            let property = declaration.declaration.property;
            // "No shadow" is realized by every backend.
            let nothing = matches!(&declaration.declaration.value, StyleValue::Shadow(layers) if layers.is_empty());
            targets.into_iter().filter_map(move |(table, os, name)| match table.support(property) {
                StyleSupport::Unavailable(reason) if !nothing => {
                    let message = format!(
                        "`{piece}` sets `{property}`, which the {} backend cannot realize: {reason} \
                         (its capability table, `rustnative_style::{name}`)",
                        table.backend
                    );
                    Some(quote_spanned!(span=> #[cfg(target_os = #os)] ::core::compile_error!(#message);))
                }
                _ => None,
            })
        })
    });
    let unavailable: Vec<_> = unavailable.collect();
    let tracking = file.map(|path| {
        let path = path.display().to_string();
        quote!(
            const _: &str = ::core::include_str!(#path);
        )
    });
    let prelude = quote_spanned!(span=> #tracking #(#unavailable)*);
    rustnative_style::tokens::declaration_set(
        &declarations,
        &sources,
        matches!(kind, Kind::Declarations),
        &quote!(::rustnative_core),
        &prelude,
    )
}
