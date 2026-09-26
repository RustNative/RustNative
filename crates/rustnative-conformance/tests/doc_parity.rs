//! The documentation obligation (`PLAN.md` 2.9 and 2.14): wherever the
//! project's prose shows a tree in one syntax, the same section shows it in
//! the other. Checked per section (a run of text under one heading) of
//! `README.md`, `PLAN.md`, and every guide under `docs/guides/`.
//!
//! A builder example is a `rust` block that constructs nodes (`Node::…`);
//! a markup example is one that writes an element (`rsx!`, or a line that
//! starts with `<Name`). A section may show a tree in only one syntax when
//! it says why, with `<!-- single-syntax: reason -->`. Likewise a section
//! styling a node shows the typed spelling (`VisualStyle`) and the class or
//! declaration spelling (`classes!`, `class="…"`), or says
//! `<!-- single-spelling: reason -->` (Milestone 58).

use std::path::{Path, PathBuf};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

#[derive(Default)]
struct Section {
    heading: String,
    builder: usize,
    markup: usize,
    exempt: bool,
    typed_style: usize,
    utility_style: usize,
    style_exempt: bool,
}

fn sections(text: &str) -> Vec<Section> {
    let mut sections = vec![Section::default()];
    let mut in_block: Option<String> = None;
    let mut block = String::new();
    for line in text.lines() {
        if let Some(language) = &in_block {
            if line.trim_start().starts_with("```") {
                if let (true, Some(current)) = (language == "rust", sections.last_mut()) {
                    if block.contains("Node::") {
                        current.builder += 1;
                    }
                    let markup = block.contains("rsx!")
                        || block.lines().any(|code| {
                            let code = code.trim_start();
                            code.starts_with('<')
                                && code.chars().nth(1).is_some_and(|c| c.is_ascii_uppercase())
                        });
                    if markup {
                        current.markup += 1;
                    }
                    current.typed_style += usize::from(typed_style(&block));
                    current.utility_style += usize::from(utility_style(&block));
                }
                in_block = None;
                block.clear();
            } else {
                block.push_str(line);
                block.push('\n');
            }
            continue;
        }
        if let Some(language) = line.trim_start().strip_prefix("```") {
            in_block = Some(language.trim().to_owned());
            continue;
        }
        if line.starts_with('#') {
            sections.push(Section { heading: line.to_owned(), ..Section::default() });
        }
        if line.contains("<!-- single-syntax:") {
            if let Some(current) = sections.last_mut() {
                current.exempt = true;
            }
        }
        if line.contains("<!-- single-spelling:") {
            if let Some(current) = sections.last_mut() {
                current.style_exempt = true;
            }
        }
    }
    sections
}

fn documents() -> Vec<PathBuf> {
    let root = workspace();
    let mut documents = vec![root.join("README.md"), root.join("PLAN.md")];
    if let Ok(entries) = std::fs::read_dir(root.join("docs/guides")) {
        documents.extend(
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|e| e == "md")),
        );
    }
    documents
}

/// Whether runnable code styles a node in the typed spelling.
fn typed_style(code: &str) -> bool {
    code.lines().any(|line| {
        !line.starts_with('#')
            && (line.contains("VisualStyle::new()") || line.contains(".with_state_style("))
    })
}

/// Whether runnable code styles a node in the utility or declaration
/// spelling.
fn utility_style(code: &str) -> bool {
    code.contains("classes!")
        || code.contains("styles!")
        || code.contains(" class=\"")
        || code.contains(" style=\"")
}

/// Every doc comment block (a run of `///` or `//!` lines) in `crate`'s
/// sources whose runnable example builds nodes, as `(file, first line)`,
/// when the block has no `rsx!` spelling beside it.
fn single_syntax_doc_examples(crate_dir: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let mut pending = vec![crate_dir.join("src")];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let mut block = String::new();
            let mut start = 0;
            for (number, line) in text.lines().enumerate().chain(std::iter::once((usize::MAX, "")))
            {
                let trimmed = line.trim_start();
                if trimmed.starts_with("///") || trimmed.starts_with("//!") {
                    if block.is_empty() {
                        start = number + 1;
                    }
                    block.push_str(trimmed);
                    block.push('\n');
                    continue;
                }
                // Only what is inside runnable code fences counts.
                let mut code = String::new();
                let mut open = false;
                let mut runnable = false;
                for doc_line in block.lines() {
                    let content =
                        doc_line.trim_start_matches("///").trim_start_matches("//!").trim_start();
                    if let Some(language) = content.strip_prefix("```") {
                        if open {
                            open = false;
                            runnable = false;
                        } else {
                            open = true;
                            runnable = !matches!(
                                language.trim(),
                                "text" | "ignore" | "sh" | "toml" | "json" | "css"
                            );
                        }
                        continue;
                    }
                    if open && runnable {
                        code.push_str(content);
                        code.push('\n');
                    }
                }
                let builds =
                    code.lines().any(|line| !line.starts_with('#') && line.contains("Node::"));
                if builds && !code.contains("rsx!") && !block.contains("single-syntax:") {
                    problems.push(format!("{}:{start}", path.display()));
                }
                // Milestone 58: a style example shows both spellings.
                if typed_style(&code) != utility_style(&code) && !block.contains("single-spelling:")
                {
                    problems.push(format!("{}:{start} (style spellings)", path.display()));
                }
                block.clear();
            }
        }
    }
    problems.sort();
    problems
}

#[test]
fn every_runnable_doc_example_that_builds_a_tree_has_both_syntaxes() {
    let root = workspace();
    let mut problems = Vec::new();
    for krate in ["rustnative-core", "rustnative-headless", "rustnative-components"] {
        problems.extend(single_syntax_doc_examples(&root.join("crates").join(krate)));
    }
    assert!(
        problems.is_empty(),
        "doc examples that build a tree in the builder syntax only (add an `rsx!` twin, or say \
         `single-syntax: reason` when the example is about the builder API itself; a style example shows \
         the typed spelling and the class/declaration spelling, or says `single-spelling: reason`):\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn every_documented_tree_is_shown_in_both_syntaxes() {
    let mut problems = Vec::new();
    for document in documents() {
        let Ok(text) = std::fs::read_to_string(&document) else { continue };
        for section in sections(&text) {
            if !section.style_exempt && (section.typed_style > 0) != (section.utility_style > 0) {
                problems.push(format!(
                    "{} — {}: {} typed-style, {} class/declaration-style (style spellings)",
                    document.display(),
                    section.heading,
                    section.typed_style,
                    section.utility_style
                ));
            }
            if section.exempt {
                continue;
            }
            if (section.builder > 0) != (section.markup > 0) {
                problems.push(format!(
                    "{} — {}: {} builder, {} markup",
                    document.display(),
                    section.heading,
                    section.builder,
                    section.markup
                ));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "sections showing a tree in one syntax only:\n  {}",
        problems.join("\n  ")
    );
}
