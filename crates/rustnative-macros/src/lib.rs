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
use rustnative_style::{ANDROID, LINUX, StyleSupport, StyleValue, Vocabulary, WINDOWS};

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

/// The `[style]` table of the crate's `rustnative.toml`, if it has one.
fn style_table() -> Option<toml::Table> {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?);
    let text = std::fs::read_to_string(root.join("rustnative.toml")).ok()?;
    match text.parse::<toml::Table>().ok()?.remove("style")? {
        toml::Value::Table(table) => Some(table),
        _ => None,
    }
}

/// The project's style file: `[style] file` in `rustnative.toml`, else
/// `app.css`, beside the crate's manifest.
fn style_file() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?);
    let configured = style_table()
        .and_then(|table| table.get("file").and_then(toml::Value::as_str).map(str::to_owned));
    let path = root.join(configured.as_deref().unwrap_or("app.css"));
    path.is_file().then_some(path)
}

/// The backends the project declares it is realized on: `[style] targets`
/// in `rustnative.toml` (`targets = ["windows", "web"]`), or `None` when it
/// declares none.
fn declared_targets() -> Option<Vec<String>> {
    let table = style_table()?;
    let targets = table.get("targets")?.as_array()?;
    Some(
        targets
            .iter()
            .filter_map(toml::Value::as_str)
            .map(str::to_ascii_lowercase)
            .filter(|item| !item.is_empty())
            .collect(),
    )
}

/// Targets with a shipped backend and capability table.
const SHIPPED_TARGETS: &[&str] = &["windows", "linux", "android", "web", "headless", "tui"];

/// Targets on the roadmap with no backend yet: accepted in `[style]
/// targets` and checked once their tables exist.
const PLANNED_TARGETS: &[&str] = &["macos", "ios", "ipados", "embedded"];

/// One backend a style is checked against: its table, the operating system
/// whose `cfg` the error is emitted under (`None`: unconditionally, for a
/// declared target), and the table's constant name for the message.
type CheckedTarget = (rustnative_style::StyleCapabilities, Option<&'static str>, String);

/// The backends to check a style against, given the project's declared
/// targets: exactly those (the planned ones have no table yet), or — when
/// none are declared — the native backend of the OS compiled for.
fn checked_targets(declared: Option<&[String]>) -> Result<Vec<CheckedTarget>, String> {
    let Some(declared) = declared else {
        return Ok(vec![
            (WINDOWS, Some("windows"), "WINDOWS".to_owned()),
            (LINUX, Some("linux"), "LINUX".to_owned()),
            (ANDROID, Some("android"), "ANDROID".to_owned()),
        ]);
    };
    if let Some(unknown) = declared
        .iter()
        .find(|target| table_for(target).is_none() && !PLANNED_TARGETS.contains(&target.as_str()))
    {
        return Err(format!(
            "`[style] targets` in rustnative.toml names `{unknown}`, which is not a target; \
             the targets are {}",
            SHIPPED_TARGETS.iter().chain(PLANNED_TARGETS).copied().collect::<Vec<_>>().join(", ")
        ));
    }
    Ok(declared
        .iter()
        .filter_map(|target| table_for(target).map(|table| (table, target)))
        // The constant is named for the target (`TUI` for `tui`), not for
        // the backend's display name ("terminal").
        .map(|(table, target)| (table, None, target.to_ascii_uppercase()))
        .collect())
}

/// The capability table of the backend named `target`, if one is shipped.
fn table_for(target: &str) -> Option<rustnative_style::StyleCapabilities> {
    match target {
        "windows" => Some(WINDOWS),
        "linux" => Some(LINUX),
        "android" => Some(ANDROID),
        "web" => Some(rustnative_style::WEB),
        "headless" => Some(rustnative_style::HEADLESS),
        "tui" => Some(rustnative_style::TUI),
        _ => None,
    }
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
    // Which backends to check against. A project that declares its targets
    // (`[style] targets` in `rustnative.toml`) is checked against exactly
    // those, whatever machine compiles it — a server-rendered web project
    // built on Windows is a web project, not a Windows one. A project that
    // declares none is checked against the native backend of the operating
    // system it is compiled for: the error is emitted under that OS's
    // `cfg`, which the *application's* build evaluates.
    let targets = match checked_targets(declared_targets().as_deref()) {
        Ok(targets) => targets,
        Err(message) => return error(span, &message),
    };
    let unavailable = pieces.iter().flat_map(|piece| {
        let declarations = resolve(piece).unwrap_or_default();
        let targets = &targets;
        declarations.into_iter().flat_map(move |declaration| {
            let property = declaration.declaration.property;
            // "No shadow" is realized by every backend.
            let nothing = matches!(&declaration.declaration.value, StyleValue::Shadow(layers) if layers.is_empty());
            targets.iter().filter_map(move |(table, os, name)| match table.support(property) {
                StyleSupport::Unavailable(reason) if !nothing => {
                    let message = format!(
                        "`{piece}` sets `{property}`, which the {} backend cannot realize: {reason} \
                         (its capability table, `rustnative_style::{name}`)",
                        table.backend
                    );
                    Some(if let Some(os) = os {
                        quote_spanned!(span=> #[cfg(target_os = #os)] ::core::compile_error!(#message);)
                    } else {
                        quote_spanned!(span=> ::core::compile_error!(#message);)
                    })
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
    // The declared targets decide what fails to build, so a change to them
    // must rebuild what uses the macro.
    let manifest_tracking = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(|root| PathBuf::from(root).join("rustnative.toml"))
        .filter(|path| path.is_file())
        .map(|path| {
            let path = path.display().to_string();
            quote!(
                const _: &str = ::core::include_str!(#path);
            )
        });
    let prelude = quote_spanned!(span=> #tracking #manifest_tracking #(#unavailable)*);
    rustnative_style::tokens::declaration_set(
        &declarations,
        &sources,
        matches!(kind, Kind::Declarations),
        &quote!(::rustnative_core),
        &prelude,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(targets: &[CheckedTarget]) -> Vec<(&'static str, Option<&'static str>)> {
        targets.iter().map(|(table, os, _)| (table.backend, *os)).collect()
    }

    #[test]
    fn a_declared_web_project_is_checked_against_the_web_alone_on_any_machine() {
        let declared = vec!["web".to_owned()];
        let targets = checked_targets(Some(&declared)).unwrap();
        assert_eq!(names(&targets), [("Web", None)]);
        // The Web realizes shadows, so nothing fails — even compiled on
        // Windows, whose table would refuse them.
        assert!(matches!(
            targets[0].0.support(rustnative_style::StyleProperty::Shadow),
            StyleSupport::Realized
        ));
    }

    #[test]
    fn declared_native_targets_fail_wherever_the_project_compiles() {
        let declared = vec!["windows".to_owned(), "android".to_owned(), "ipados".to_owned()];
        let targets = checked_targets(Some(&declared)).unwrap();
        assert_eq!(
            names(&targets),
            [("Windows", None), ("Android", None)],
            "ipados has no table yet"
        );
    }

    #[test]
    fn without_declared_targets_the_compiled_for_os_is_checked() {
        let targets = checked_targets(None).unwrap();
        assert_eq!(
            names(&targets),
            [("Windows", Some("windows")), ("Linux", Some("linux")), ("Android", Some("android"))]
        );
    }

    #[test]
    fn a_declared_terminal_target_refuses_what_a_cell_cannot_show() {
        let declared = vec!["tui".to_owned()];
        let targets = checked_targets(Some(&declared)).unwrap();
        assert_eq!(names(&targets), [("terminal", None)]);
        assert_eq!(targets[0].2, "TUI", "the message names `rustnative_style::TUI`");
        for property in [
            rustnative_style::StyleProperty::Shadow,
            rustnative_style::StyleProperty::BorderRadius,
            rustnative_style::StyleProperty::FontSize,
        ] {
            assert!(matches!(targets[0].0.support(property), StyleSupport::Unavailable(_)));
        }
    }

    #[test]
    fn an_unknown_target_is_refused_by_name() {
        let declared = vec!["mars".to_owned()];
        let message = checked_targets(Some(&declared)).unwrap_err();
        assert!(message.contains("`mars`") && message.contains("windows"), "{message}");
    }
}
