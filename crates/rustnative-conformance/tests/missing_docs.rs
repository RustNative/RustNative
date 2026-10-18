//! Every library crate the workspace publishes documents its whole public
//! API: each `crates/*/src/lib.rs` carries `#![deny(missing_docs)]`, so an
//! undocumented public item is a compile error rather than a gap someone
//! notices later.
//!
//! The attribute lives in each crate rather than in `[workspace.lints]`,
//! which would also reach the examples, benchmarks, integration tests and
//! build-script output that are not API.

use std::path::{Path, PathBuf};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

#[test]
fn every_library_crate_denies_missing_docs() {
    let crates = workspace().join("crates");
    let Ok(entries) = std::fs::read_dir(&crates) else {
        // A published copy of this crate has no workspace to read.
        return;
    };
    let mut lacking = Vec::new();
    for entry in entries.flatten() {
        let lib = entry.path().join("src").join("lib.rs");
        let Ok(source) = std::fs::read_to_string(&lib) else { continue };
        if !source.lines().any(|line| line.trim() == "#![deny(missing_docs)]") {
            lacking.push(lib.display().to_string());
        }
    }
    lacking.sort();
    assert!(
        lacking.is_empty(),
        "library crates without #![deny(missing_docs)]:\n{}",
        lacking.join("\n")
    );
}
