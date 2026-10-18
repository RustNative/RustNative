//! No control characters in the repository's text: a broken escape
//! sequence (`\0`, `\b`, `\a` written where a backtick or letter was meant)
//! leaves a raw byte in the file that renders as nothing, or as garbage, in
//! the documentation built from it.
//!
//! Every file git tracks is read; one that is not UTF-8 is binary (the
//! golden bitmaps) and is not text to hold to this. Tab and newline are the
//! only control characters text has; a carriage return before a newline is
//! allowed too, for a checkout that converts line endings.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

/// The 1-based line and column of each stray control character in `text`.
fn control_characters(text: &str) -> Vec<(usize, usize, char)> {
    let mut found = Vec::new();
    for (line_index, line) in text.split('\n').enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        for (column, character) in line.chars().enumerate() {
            if character.is_control() && character != '\t' {
                found.push((line_index + 1, column + 1, character));
            }
        }
    }
    found
}

#[test]
fn tracked_text_has_no_control_characters() {
    let root = workspace();
    if !root.join(".git").exists() {
        // A published copy of this crate has no repository to read.
        return;
    }
    let output =
        Command::new("git").args(["ls-files", "-z"]).current_dir(&root).output().expect("git runs");
    assert!(output.status.success(), "git ls-files failed");
    let mut offenders = Vec::new();
    for name in output.stdout.split(|&byte| byte == 0).filter(|name| !name.is_empty()) {
        let name = String::from_utf8_lossy(name);
        let Ok(bytes) = std::fs::read(root.join(name.as_ref())) else { continue };
        let Ok(text) = String::from_utf8(bytes) else { continue };
        for (line, column, character) in control_characters(&text) {
            offenders.push(format!("{name}:{line}:{column}: U+{:04X}", u32::from(character)));
        }
    }
    assert!(offenders.is_empty(), "control characters in tracked text:\n{}", offenders.join("\n"));
}

#[test]
fn control_characters_are_found_and_line_endings_are_not() {
    assert_eq!(control_characters("a\tb\r\nc\n"), Vec::new());
    assert_eq!(control_characters("ok\n/// its \u{8}reak"), vec![(2, 9, '\u{8}')]);
    assert_eq!(control_characters("lone\rreturn"), vec![(1, 5, '\r')]);
}
