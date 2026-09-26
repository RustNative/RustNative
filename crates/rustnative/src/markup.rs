//! The markup syntax in the CLI: `rustnative expand` and `rustnative fmt`,
//! over both carriers — `.rsx` files and `rsx!` calls in `.rs` files.
//!
//! Both commands link `rustnative-markup`, the same parser and lowering the
//! `rsx!` macro uses, so what `expand` prints is exactly what the compiler
//! sees and what `fmt` accepts is exactly what the compiler accepts.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use proc_macro2::{TokenStream, TokenTree};
use quote::quote;
use rustnative_markup::{CompileOptions, Markup, compile, format_markup, lower};

use crate::error::{Error, Result};

/// The private wrapper the formatter lowers `.rsx` markup into, so it can
/// tell its own wrappers from `rsx!` calls a developer wrote.
const FORMAT_MARKER: &str = "__rustnative_markup__!";

/// One markup macro call found in Rust source.
#[derive(Debug, Clone)]
pub struct MacroSite {
    /// Byte offset of the macro path's first character.
    pub start: usize,
    /// Byte offset just past the closing delimiter.
    pub end: usize,
    /// The macro's body.
    pub body: TokenStream,
    /// 1-based line of the call.
    pub line: usize,
}

/// Every call of a macro whose last path segment is `name` in `source`.
///
/// # Errors
///
/// The source does not tokenize.
pub fn macro_sites(source: &str, name: &str) -> Result<Vec<MacroSite>> {
    let tokens = TokenStream::from_str(source)
        .map_err(|error| Error::Usage(format!("the file does not tokenize: {error}")))?;
    let mut sites = Vec::new();
    find_sites(tokens, name, &mut sites);
    sites.sort_by_key(|site| site.start);
    Ok(sites)
}

fn find_sites(tokens: TokenStream, name: &str, sites: &mut Vec<MacroSite>) {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    for (index, token) in tokens.iter().enumerate() {
        if let TokenTree::Group(group) = token {
            let is_call = index >= 2
                && matches!(&tokens[index - 1], TokenTree::Punct(p) if p.as_char() == '!')
                && matches!(&tokens[index - 2], TokenTree::Ident(ident) if ident == name);
            if is_call {
                // Walk back over a leading path (`::rustnative_core::rsx!`).
                let mut first = index - 2;
                while first >= 2
                    && matches!(&tokens[first - 1], TokenTree::Punct(p) if p.as_char() == ':')
                    && matches!(&tokens[first - 2], TokenTree::Punct(p) if p.as_char() == ':')
                {
                    if first >= 3 && matches!(&tokens[first - 3], TokenTree::Ident(_)) {
                        first -= 3;
                    } else {
                        first -= 2;
                        break;
                    }
                }
                let start_span = tokens[first].span();
                sites.push(MacroSite {
                    start: start_span.byte_range().start,
                    end: group.span_close().byte_range().end,
                    body: group.stream(),
                    line: start_span.start().line,
                });
                continue;
            }
            find_sites(group.stream(), name, sites);
        }
    }
}

/// Pretty-prints an expression.
fn pretty(expression: &TokenStream) -> String {
    let file: std::result::Result<syn::File, _> =
        syn::parse2(quote!(fn __expansion() { #expression }));
    let Ok(file) = file else { return expression.to_string() };
    let text = prettyplease::unparse(&file);
    // Drop the wrapper function's first and last lines and one level of
    // indentation.
    let lines: Vec<&str> = text.lines().collect();
    let inner = lines.get(1..lines.len().saturating_sub(1)).unwrap_or(&[]);
    inner
        .iter()
        .map(|line| line.strip_prefix("    ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// What `rustnative expand <file>` prints: the builder form of every markup
/// tree in `path`, in source order.
///
/// # Errors
///
/// The file cannot be read, does not compile as `.rsx`, or holds markup
/// that does not lower.
pub fn expand_file(path: &Path) -> Result<String> {
    let source = read(path)?;
    let lowered_source = if is_rsx(path) { compile_rsx(path, &source, None)? } else { source };
    let mut out = String::new();
    for site in macro_sites(&lowered_source, "rsx")? {
        let markup = syn::parse2::<Markup>(site.body)
            .map_err(|error| Error::Usage(format!("{}:{}: {error}", path.display(), site.line)))?;
        let lowered = lower(&markup)
            .map_err(|error| Error::Usage(format!("{}:{}: {error}", path.display(), site.line)))?;
        let _ = write!(out, "// {}:{}\n{}\n\n", path.display(), site.line, pretty(&lowered));
    }
    if out.is_empty() {
        let _ = writeln!(out, "// {}: no markup", path.display());
    }
    Ok(out)
}

/// What `rustnative expand --classes`/`--styles` prints: each declaration
/// the input lowers to, and the typed value it resolves to against the
/// default theme with the project's style file over it.
///
/// # Errors
///
/// The style file or the input does not compile.
pub fn expand_style(here: &Path, input: &str, classes: bool) -> Result<String> {
    let project = here.ancestors().find(|dir| dir.join("Cargo.toml").is_file()).unwrap_or(here);
    let vocabulary = match rustnative_build::styles::style_file(project) {
        Some(path) => {
            let source = read(&path)?;
            rustnative_style::Vocabulary::with_style_file(&source).map_err(|errors| {
                Error::Usage(
                    errors
                        .iter()
                        .map(|error| {
                            let (line, column) = error.line_column(&source);
                            format!("{}:{line}:{column}: {}", path.display(), error.message)
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            })?
        }
        None => rustnative_style::Vocabulary::defaults(),
    };
    let lowered = if classes {
        vocabulary.resolve_classes(input)
    } else {
        vocabulary.resolve_declarations(input)
    };
    let declarations = lowered.map_err(|errors| {
        Error::Usage(
            errors.iter().map(|error| error.message.clone()).collect::<Vec<_>>().join("\n"),
        )
    })?;
    let tokens = vocabulary.token_table();
    let mut out = String::new();
    for declaration in declarations {
        let resolved = tokens.resolve(&declaration.declaration.value);
        match resolved {
            Some(value) if value != declaration.declaration.value => {
                let _ = writeln!(out, "{declaration}  /* = {value} */");
            }
            _ => {
                let _ = writeln!(out, "{declaration}");
            }
        }
    }
    Ok(out)
}

/// Every `.rsx` file, and every `.rs` file containing `rsx!`, under `src`.
#[must_use]
pub fn project_files(src: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![src.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if is_rsx(&path)
                || (path.extension().is_some_and(|extension| extension == "rs")
                    && std::fs::read_to_string(&path).is_ok_and(|text| text.contains("rsx!")))
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn is_rsx(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "rsx")
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path)
        .map(|text| text.replace("\r\n", "\n"))
        .map_err(|cause| Error::Io { what: format!("read {}", path.display()), cause })
}

fn src_root(path: &Path) -> PathBuf {
    path.ancestors()
        .find(|ancestor| ancestor.file_name().is_some_and(|name| name == "src"))
        .map_or_else(
            || path.parent().map_or_else(PathBuf::new, Path::to_path_buf),
            Path::to_path_buf,
        )
}

fn compile_rsx(path: &Path, source: &str, wrapper: Option<&str>) -> Result<String> {
    let options = CompileOptions {
        source_path: path.to_path_buf(),
        src_root: src_root(path),
        wrapper: wrapper.map(str::to_owned),
    };
    compile(source, &options).map(|output| output.code).map_err(|errors| {
        Error::Usage(
            errors
                .iter()
                .map(|error| {
                    format!("{}:{}:{}: {}", path.display(), error.line, error.column, error.message)
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })
}

/// Formats one file's markup — and, for a `.rsx` file, its Rust through
/// `rustfmt` — returning the formatted text.
///
/// # Errors
///
/// The file does not read, compile, or format.
pub fn format_file(path: &Path) -> Result<String> {
    let source = read(path)?;
    if is_rsx(path) { format_rsx(path, &source) } else { format_macro_calls(&source, "rsx", false) }
}

/// Formats every `name!` call's body in `source`. With `unwrap`, a call is
/// replaced by its formatted markup alone (the `.rsx` formatter's private
/// wrapper, and any context it inserted, disappear).
fn format_macro_calls(source: &str, name: &str, unwrap: bool) -> Result<String> {
    let sites = macro_sites(source, name)?;
    let mut out = String::with_capacity(source.len());
    let mut cursor = 0;
    for site in sites {
        let markup = syn::parse2::<Markup>(site.body.clone())
            .map_err(|error| Error::Usage(format!("line {}: {error}", site.line)))?;
        let line_start = source[..site.start].rfind('\n').map_or(0, |index| index + 1);
        let indent = source[line_start..site.start].chars().take_while(|c| *c == ' ').count();
        out.push_str(&source[cursor..site.start]);
        if unwrap {
            let column = source[line_start..site.start].chars().count();
            let markup = Markup { context: None, root: markup.root };
            out.push_str(&format_markup(&markup, source, column.max(indent)));
        } else {
            let prefix = &source[site.start
                ..source[site.start..].find('!').map_or(site.start, |bang| site.start + bang + 1)];
            let body = format_markup(&markup, source, indent + 4);
            let _ = write!(
                out,
                "{prefix} {{\n{}{body}\n{}}}",
                " ".repeat(indent + 4),
                " ".repeat(indent)
            );
        }
        cursor = site.end;
    }
    out.push_str(&source[cursor..]);
    Ok(out)
}

fn format_rsx(path: &Path, source: &str) -> Result<String> {
    let lowered = compile_rsx(path, source, Some(FORMAT_MARKER))?;
    let formatted_rust = rustfmt(&lowered)?;
    let marker_name = FORMAT_MARKER.trim_end_matches('!');
    let with_markup = format_macro_calls(&formatted_rust, marker_name, true)?;
    // `rsx!` calls the developer wrote in the `.rsx` file are formatted too.
    format_macro_calls(&with_markup, "rsx", false)
}

fn rustfmt(source: &str) -> Result<String> {
    use std::io::Write as _;
    let mut child = std::process::Command::new("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|cause| Error::ToolMissing {
            tool: "rustfmt",
            hint: "rustup component add rustfmt".to_owned(),
            cause: Some(cause.to_string()),
        })?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(source.as_bytes())
            .map_err(|cause| Error::Io { what: "feed rustfmt".to_owned(), cause })?;
    }
    let output = child
        .wait_with_output()
        .map_err(|cause| Error::Io { what: "run rustfmt".to_owned(), cause })?;
    if !output.status.success() {
        return Err(Error::Usage(format!(
            "rustfmt could not format the file:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str, contents: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("rustnative-markup-{name}-{}", std::process::id()));
        let src = directory.join("src");
        std::fs::create_dir_all(&src).expect("dirs");
        let path = src.join(name);
        std::fs::write(&path, contents).expect("write");
        path
    }

    #[test]
    fn expand_prints_the_builder_form_of_both_carriers() {
        let rs =
            scratch("view.rs", "fn v() -> Node { rsx! { <Label key=\"a\" text=\"Hi\" /> } }\n");
        let expanded = expand_file(&rs).expect("expands");
        assert!(
            expanded
                .contains("::rustnative_core::Node::label_with_layout(\"a\", \"Hi\", __layout)"),
            "{expanded}"
        );

        let rsx =
            scratch("screen.rsx", "fn v() -> Node {\n    <Button key=\"b\" text=\"Go\" />\n}\n");
        let expanded = expand_file(&rsx).expect("expands");
        assert!(expanded.contains("button_with_layout(\"b\", \"Go\", __layout)"), "{expanded}");
        assert!(expanded.contains("screen.rsx:2"), "{expanded}");
    }

    #[test]
    fn fmt_lays_out_markup_in_an_rs_file() {
        let rs = scratch(
            "long.rs",
            "fn v() -> Node {\n    rsx! { <Column key=\"root\"><Label key=\"a\" text=\"A\" /><Label key=\"b\" text=\"B\" /></Column> }\n}\n",
        );
        let formatted = format_file(&rs).expect("formats");
        assert_eq!(
            formatted,
            "fn v() -> Node {\n    rsx! {\n        <Column key=\"root\">\n            <Label key=\"a\" text=\"A\" />\n            <Label key=\"b\" text=\"B\" />\n        </Column>\n    }\n}\n"
        );
    }

    #[test]
    fn fmt_formats_an_rsx_files_rust_and_markup_together() {
        let rsx = scratch(
            "tidy.rsx",
            "fn v( ) -> Node {\n  let x=1;\n    <Column key=\"root\"><Label key=\"a\" text={x.to_string()} /></Column>\n}\n",
        );
        let formatted = format_file(&rsx).expect("formats");
        assert!(
            formatted.contains("fn v() -> Node {\n    let x = 1;\n"),
            "rustfmt ran: {formatted}"
        );
        assert!(formatted.contains("<Column key=\"root\">\n        <Label key=\"a\" text={x.to_string()} />\n    </Column>"), "{formatted}");
        assert!(!formatted.contains(FORMAT_MARKER), "the private wrapper is gone: {formatted}");
        // Formatting is idempotent.
        std::fs::write(&rsx, &formatted).expect("write");
        assert_eq!(format_file(&rsx).expect("formats again"), formatted);
    }
}
