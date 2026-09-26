//! Compiler diagnostics reported where the developer wrote the code.
//!
//! `rustnative build`, `check`, and `test` run Cargo with structured
//! diagnostics and rewrite every position that falls in a lowered `.rsx`
//! file — markup errors and ordinary Rust errors alike — back through the
//! source map written beside it, so the location, the quoted line, and the
//! caret all point into the `.rsx` file (`PLAN.md` Milestone 53). Plain
//! `cargo build` still works and reports the lowered file, whose map names
//! its source; the CLI is the documented way to build a `.rsx` project.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rustnative_markup::SourceMap;

use crate::error::{Error, Result};

/// A lowered file's source map, found beside it (`<file>.map`).
fn map_for(lowered: &Path) -> Option<SourceMap> {
    let map_path = PathBuf::from(format!("{}.map", lowered.display()));
    SourceMap::from_json(&std::fs::read_to_string(map_path).ok()?)
}

/// Rewrites one rendered diagnostic so every location in a lowered `.rsx`
/// file points into the `.rsx` file instead. `root` resolves relative paths.
#[must_use]
pub fn remap_rendered(rendered: &str, root: &Path) -> String {
    let mut out = Vec::new();
    // The map and source lines of the location block being rewritten.
    let mut current: Option<(SourceMap, Vec<String>)> = None;
    // The snippet line just rewritten, whose annotations follow.
    let mut last_line_number: Option<usize> = None;
    for line in rendered.lines() {
        if let Some((prefix, path, line_number, column)) = parse_location(line) {
            let lowered = resolve(root, &path);
            if let Some(map) = map_for(&lowered) {
                let (source_line, source_column) = map.to_source(line_number, column);
                let source_text = std::fs::read_to_string(&map.source)
                    .unwrap_or_default()
                    .replace("\r\n", "\n")
                    .lines()
                    .map(str::to_owned)
                    .collect();
                out.push(format!(
                    "{prefix}{}:{source_line}:{source_column}",
                    display_path(&map.source, root)
                ));
                current = Some((map, source_text));
                continue;
            }
            current = None;
            out.push(line.to_owned());
            continue;
        }
        let Some((map, source_lines)) = &current else {
            out.push(line.to_owned());
            continue;
        };
        if let Some((gutter, number, _lowered_text)) = parse_code_line(line) {
            if let Some(source) = source_lines.get(number.saturating_sub(1)) {
                last_line_number = Some(number);
                out.push(format!("{gutter}{number} | {source}"));
                continue;
            }
        }
        if let (Some(number), Some((gutter, markers))) = (last_line_number, parse_annotation(line))
        {
            // Carets are in lowered columns; map the first one and move the
            // whole annotation by the same amount.
            let caret = markers.chars().take_while(|c| *c == ' ').count() + 1;
            let (_, source_caret) = map.to_source(number, caret);
            let rest = markers.trim_start();
            out.push(format!("{gutter}| {}{rest}", " ".repeat(source_caret.saturating_sub(1))));
            continue;
        }
        out.push(line.to_owned());
    }
    let mut text = out.join("\n");
    if rendered.ends_with('\n') {
        text.push('\n');
    }
    text
}

fn resolve(root: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() { path } else { root.join(path) }
}

fn display_path(source: &str, root: &Path) -> String {
    let source = PathBuf::from(source);
    source
        .strip_prefix(root)
        .map_or_else(|_| source.display().to_string(), |relative| relative.display().to_string())
}

/// ` --> path:line:col` or `  ::: path:line:col`.
fn parse_location(line: &str) -> Option<(String, String, usize, usize)> {
    let marker =
        line.find("--> ").map(|at| at + 4).or_else(|| line.find("::: ").map(|at| at + 4))?;
    let (prefix, location) = line.split_at(marker);
    let mut parts = location.rsplitn(3, ':');
    let column = parts.next()?.trim().parse().ok()?;
    let line_number = parts.next()?.parse().ok()?;
    let path = parts.next()?.to_owned();
    Some((prefix.to_owned(), path, line_number, column))
}

/// `12 |     code` → (gutter before the number, 12, code).
fn parse_code_line(line: &str) -> Option<(String, usize, String)> {
    let bar = line.find(" | ").or_else(|| line.strip_suffix(" |").map(str::len))?;
    let (head, tail) = line.split_at(bar);
    let number: usize = head.trim().parse().ok()?;
    let gutter = head[..head.len() - head.trim_start().len()].to_owned();
    Some((gutter, number, tail.get(3..).unwrap_or("").to_owned()))
}

/// `   |     ^^^ label` → (gutter up to the bar, markers after `| `).
fn parse_annotation(line: &str) -> Option<(String, String)> {
    let bar = line.find('|')?;
    let (head, tail) = line.split_at(bar);
    if !head.trim().is_empty() {
        return None;
    }
    let markers = tail.get(2..).unwrap_or("");
    markers.trim_start().starts_with(['^', '-']).then(|| (head.to_owned(), markers.to_owned()))
}

/// Runs `cargo <arguments>` in `directory` with structured diagnostics,
/// printing each one remapped, and passing through everything else (test
/// output, Cargo's own progress on stderr).
///
/// # Errors
///
/// [`Error::ToolMissing`] if Cargo is missing, [`Error::ToolFailed`] if it
/// fails.
pub fn run_cargo(directory: &Path, arguments: &[String]) -> Result<()> {
    // `--message-format` goes before any `--` separating test arguments.
    let mut with_format = Vec::new();
    let mut inserted = false;
    for argument in arguments {
        if argument == "--" && !inserted {
            with_format.push("--message-format=json".to_owned());
            inserted = true;
        }
        with_format.push(argument.clone());
    }
    if !inserted {
        with_format.push("--message-format=json".to_owned());
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut child = Command::new(cargo)
        .current_dir(directory)
        .args(&with_format)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|cause| Error::ToolMissing {
            tool: "cargo",
            hint: "install Rust from https://rustup.rs".to_owned(),
            cause: Some(cause.to_string()),
        })?;
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(std::result::Result::ok) {
            handle_line(&line, directory);
        }
    }
    let status =
        child.wait().map_err(|cause| Error::Io { what: "wait for cargo".to_owned(), cause })?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::ToolFailed { tool: "cargo", code: status.code() })
    }
}

fn handle_line(line: &str, directory: &Path) {
    let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
        // Not Cargo's JSON: a test's own output.
        println!("{line}");
        return;
    };
    if message.get("reason").and_then(serde_json::Value::as_str) != Some("compiler-message") {
        return;
    }
    if let Some(rendered) = message.pointer("/message/rendered").and_then(serde_json::Value::as_str)
    {
        eprint!("{}", remap_rendered(rendered, directory));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diagnostic_in_a_lowered_file_is_reported_in_the_rsx_file() {
        let root = std::env::temp_dir().join(format!("rustnative-diag-{}", std::process::id()));
        let src = root.join("src");
        let out = root.join("target/out/rsx");
        std::fs::create_dir_all(&src).expect("dirs");
        std::fs::create_dir_all(&out).expect("dirs");
        let source = "fn f() -> Node {\n    <Column key=\"a\" gap={\"x\"}></Column>\n}\n";
        std::fs::write(src.join("app.rsx"), source).expect("write");
        let output = rustnative_markup::compile(
            source,
            &rustnative_markup::CompileOptions {
                source_path: src.join("app.rsx"),
                src_root: src.clone(),
                wrapper: None,
            },
        )
        .expect("compiles");
        std::fs::write(out.join("app.rs"), &output.code).expect("write");
        std::fs::write(out.join("app.rs.map"), output.map.to_json()).expect("write");

        // The `"x"` sits 24 characters further right in the lowered line.
        let lowered_line = output.code.lines().nth(1).expect("line 2");
        let lowered_column = lowered_line.find("\"x\"").expect("value") + 1;
        let source_column =
            source.lines().nth(1).expect("line 2").find("\"x\"").expect("value") + 1;
        let rendered = format!(
            "error[E0308]: mismatched types\n --> target/out/rsx/app.rs:2:{lowered_column}\n  |\n2 | {lowered_line}\n  | {}^^^ expected `i32`\n",
            " ".repeat(lowered_column - 1)
        );
        let remapped = remap_rendered(&rendered, &root);
        assert!(
            remapped
                .contains(&format!("src{}app.rsx:2:{source_column}", std::path::MAIN_SEPARATOR)),
            "{remapped}"
        );
        assert!(remapped.contains("2 |     <Column key=\"a\" gap={\"x\"}></Column>"), "{remapped}");
        assert!(
            remapped.contains(&format!("  | {}^^^ expected `i32`", " ".repeat(source_column - 1))),
            "{remapped}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_diagnostic_elsewhere_is_left_alone() {
        let rendered = "error: oops\n --> src/main.rs:3:5\n  |\n3 | let x = y;\n  |     ^\n";
        assert_eq!(remap_rendered(rendered, Path::new(".")), rendered);
    }
}
