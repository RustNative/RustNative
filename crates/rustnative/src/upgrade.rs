//! `rustnative upgrade` (`PLAN.md` Milestone 52, `C57-2`): the codemods
//! that carry an application across a breaking change, so the stability
//! policy (`docs/policy/stability.md`) is kept by tooling rather than by
//! the application's authors.
//!
//! A codemod is a syntax-aware rewrite: it parses each `.rs` file, finds
//! exactly the constructs a change affected, and edits those spans in
//! place — the rest of the file, its formatting and comments, is left
//! byte for byte. What a codemod cannot rewrite with certainty it reports
//! with its position instead of guessing.

use std::path::{Path, PathBuf};

use syn::visit::Visit;

use crate::error::{Error, Result};

/// One breaking change and its rewrite.
pub struct Codemod {
    /// The release that made the change.
    pub release: &'static str,
    /// What changed.
    pub summary: &'static str,
    /// The rewrite.
    pub rewrite: fn(&str) -> Rewrite,
}

/// What a codemod did to one file.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Rewrite {
    /// The new text (unchanged when nothing applied).
    pub text: String,
    /// Edits made.
    pub edits: usize,
    /// Places it could not rewrite with certainty: line, column, why.
    pub manual: Vec<(usize, usize, String)>,
}

/// Every codemod, oldest first.
pub const CODEMODS: &[Codemod] = &[Codemod {
    release: "0.1.0",
    summary: "EdgeInsets' physical `left`/`right` became logical `start`/`end` (Milestone 39)",
    rewrite: logical_insets,
}];

/// Byte offset of a line/column position (proc-macro2 columns count
/// characters).
fn offset(text: &str, line: usize, column: usize) -> usize {
    let start: usize = text.split_inclusive('\n').take(line.saturating_sub(1)).map(str::len).sum();
    let rest = &text[start..];
    start + rest.char_indices().nth(column).map_or(rest.len(), |(at, _)| at)
}

struct Insets {
    /// (line, column, old, new) of each field name to rename.
    renames: Vec<(usize, usize, &'static str, &'static str)>,
    manual: Vec<(usize, usize, String)>,
}

fn names_edge_insets(path: &syn::Path) -> bool {
    path.segments.last().is_some_and(|segment| segment.ident == "EdgeInsets")
}

impl<'ast> Visit<'ast> for Insets {
    fn visit_expr_struct(&mut self, node: &'ast syn::ExprStruct) {
        if names_edge_insets(&node.path) {
            for field in &node.fields {
                if let syn::Member::Named(name) = &field.member {
                    let renamed = match name.to_string().as_str() {
                        "left" => Some(("left", "start")),
                        "right" => Some(("right", "end")),
                        _ => None,
                    };
                    if let Some((old, new)) = renamed {
                        let start = name.span().start();
                        self.renames.push((start.line, start.column, old, new));
                        if field.colon_token.is_none() {
                            // `EdgeInsets { left, .. }` shorthand: the
                            // variable keeps its name, so write it out.
                            self.renames.pop();
                            self.renames.push((
                                start.line,
                                start.column,
                                old,
                                if new == "start" { "start: left" } else { "end: right" },
                            ));
                        }
                    }
                }
            }
        }
        syn::visit::visit_expr_struct(self, node);
    }

    fn visit_pat_struct(&mut self, node: &'ast syn::PatStruct) {
        if names_edge_insets(&node.path) {
            for field in &node.fields {
                if let syn::Member::Named(name) = &field.member {
                    if name == "left" || name == "right" {
                        let start = name.span().start();
                        self.manual.push((
                            start.line,
                            start.column,
                            format!(
                                "a pattern binds EdgeInsets' `{name}`; bind `{}`",
                                if name == "left" { "start" } else { "end" }
                            ),
                        ));
                    }
                }
            }
        }
        syn::visit::visit_pat_struct(self, node);
    }

    fn visit_expr_field(&mut self, node: &'ast syn::ExprField) {
        // `.left` on an unknown type may not be EdgeInsets: reported, not
        // rewritten, unless the receiver is plainly an `EdgeInsets` value.
        if let syn::Member::Named(name) = &node.member {
            if name == "left" || name == "right" {
                let plainly_insets = matches!(&*node.base, syn::Expr::Call(call)
                    if matches!(&*call.func, syn::Expr::Path(path) if path.path.segments.iter().any(|segment| segment.ident == "EdgeInsets")))
                    || matches!(&*node.base, syn::Expr::Struct(value) if names_edge_insets(&value.path));
                let start = name.span().start();
                if plainly_insets {
                    self.renames.push((
                        start.line,
                        start.column,
                        if name == "left" { "left" } else { "right" },
                        if name == "left" { "start" } else { "end" },
                    ));
                } else {
                    self.manual.push((
                        start.line,
                        start.column,
                        format!(
                            "`.{name}`: if this is EdgeInsets, it is now `.{}`",
                            if name == "left" { "start" } else { "end" }
                        ),
                    ));
                }
            }
        }
        syn::visit::visit_expr_field(self, node);
    }
}

/// The 0.1.0 codemod.
fn logical_insets(text: &str) -> Rewrite {
    let Ok(file) = syn::parse_file(text) else {
        return Rewrite {
            text: text.to_owned(),
            edits: 0,
            manual: vec![(0, 0, "the file does not parse; nothing was changed".into())],
        };
    };
    if !text.contains("EdgeInsets") {
        return Rewrite { text: text.to_owned(), ..Rewrite::default() };
    }
    let mut visitor = Insets { renames: Vec::new(), manual: Vec::new() };
    visitor.visit_file(&file);
    let mut edited = text.to_owned();
    let mut renames = visitor.renames;
    // From the end, so earlier offsets stay valid.
    renames.sort_by_key(|rename| std::cmp::Reverse((rename.0, rename.1)));
    for (line, column, old, new) in &renames {
        let at = offset(text, *line, *column);
        if text[at..].starts_with(old) {
            edited.replace_range(at..at + old.len(), new);
        }
    }
    Rewrite { text: edited, edits: renames.len(), manual: visitor.manual }
}

fn rust_files(directory: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else { return };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name != "target" && name != ".git") {
                rust_files(&path, out);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

/// Runs every codemod newer than `from` over the project's `.rs` files.
///
/// # Errors
///
/// A file cannot be read or written.
pub fn run(root: &Path, from: &str, dry_run: bool) -> Result<()> {
    let due: Vec<&Codemod> = CODEMODS
        .iter()
        .filter(|codemod| {
            rustnative_core::package::version_matches(&format!(">{from}"), codemod.release)
        })
        .collect();
    if due.is_empty() {
        println!("upgrade: nothing to do from {from}");
        return Ok(());
    }
    let mut files = Vec::new();
    rust_files(root, &mut files);
    for codemod in due {
        println!("upgrade: {} — {}", codemod.release, codemod.summary);
        for path in &files {
            let text = std::fs::read_to_string(path)
                .map_err(|cause| Error::Io { what: format!("read {}", path.display()), cause })?;
            let rewrite = (codemod.rewrite)(&text);
            let shown = path.strip_prefix(root).unwrap_or(path).display();
            for (line, column, why) in &rewrite.manual {
                println!("  {shown}:{line}:{}: by hand: {why}", column + 1);
            }
            if rewrite.edits > 0 {
                println!(
                    "  {shown}: {} edit(s){}",
                    rewrite.edits,
                    if dry_run { " (dry run)" } else { "" }
                );
                if !dry_run {
                    std::fs::write(path, &rewrite.text).map_err(|cause| Error::Io {
                        what: format!("write {}", path.display()),
                        cause,
                    })?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The corpus: before and after for every construct the codemod
    /// knows, and the ones it must leave alone.
    #[test]
    fn the_corpus_upgrades_as_expected() {
        let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/codemod-corpus");
        let mut checked = 0;
        for entry in std::fs::read_dir(&corpus).expect("the corpus exists").flatten() {
            let case = entry.path();
            let input = std::fs::read_to_string(case.join("input.rs")).expect("input.rs");
            let expected = std::fs::read_to_string(case.join("expected.rs")).expect("expected.rs");
            let rewrite = logical_insets(&input);
            assert_eq!(rewrite.text, expected, "{}", case.display());
            let manual = std::fs::read_to_string(case.join("manual.txt")).unwrap_or_default();
            assert_eq!(
                rewrite.manual.len(),
                manual.lines().filter(|line| !line.trim().is_empty()).count(),
                "{}",
                case.display()
            );
            assert!(syn::parse_file(&rewrite.text).is_ok(), "the result still parses");
            checked += 1;
        }
        assert!(checked >= 2);
    }

    #[test]
    fn upgrading_twice_changes_nothing_more() {
        let once = logical_insets(
            "fn f() -> EdgeInsets { EdgeInsets { top: 1, left: 2, bottom: 3, right: 4 } }",
        )
        .text;
        assert_eq!(logical_insets(&once).text, once);
    }
}
