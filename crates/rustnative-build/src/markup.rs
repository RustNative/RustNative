//! `compile_rsx()`: lowering a crate's `.rsx` files before `rustc` sees
//! them (`PLAN.md` Milestone 53).
//!
//! Every `.rsx` file under `src/` is compiled by `rustnative-markup`'s
//! `.rsx` compiler into `OUT_DIR/rsx/<same relative path>.rs`, with its
//! source map beside it (`….rs.map`). A module is then declared with
//! `rustnative_core::rsx_mod!(name);`. Each file is recompiled only when its
//! content changes, and Cargo is told to rerun the build script per file.

use std::path::{Path, PathBuf};

use rustnative_markup::{CompileOptions, RsxError, compile};

/// One `.rsx` file that failed to compile.
#[derive(Debug)]
pub struct RsxFailure {
    /// The file.
    pub path: PathBuf,
    /// What was wrong, by position.
    pub errors: Vec<RsxError>,
}

impl std::fmt::Display for RsxFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for error in &self.errors {
            writeln!(
                f,
                "{}:{}:{}: {}",
                self.path.display(),
                error.line,
                error.column,
                error.message
            )?;
        }
        Ok(())
    }
}

/// Compiles every `.rsx` file under the crate's `src/` directory.
///
/// # Panics
///
/// When a `.rsx` file does not compile — a mistake in the project, which
/// must stop the build — with every error, by file, line, and column.
pub fn compile_rsx() {
    match try_compile_rsx() {
        Ok(_) => {}
        Err(failures) => {
            let mut message = String::from("`.rsx` files did not compile:\n");
            for failure in failures {
                message.push_str(&failure.to_string());
            }
            panic!("{message}");
        }
    }
}

/// Compiles every `.rsx` file under `src/`, returning the lowered files.
///
/// # Errors
///
/// Every file that did not compile.
pub fn try_compile_rsx() -> Result<Vec<PathBuf>, Vec<RsxFailure>> {
    let project = std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from).unwrap_or_default();
    let out = std::env::var_os("OUT_DIR").map(PathBuf::from).unwrap_or_default();
    let src = project.join("src");
    println!("cargo:rerun-if-changed=src");
    compile_tree(&src, &out.join("rsx"))
}

/// Compiles every `.rsx` file under `src` into `out`, mirroring paths.
/// Separated from the environment so it can be tested.
///
/// # Errors
///
/// Every file that did not compile.
pub fn compile_tree(src: &Path, out: &Path) -> Result<Vec<PathBuf>, Vec<RsxFailure>> {
    let mut sources = Vec::new();
    collect(src, &mut sources);
    sources.sort();
    let mut written = Vec::new();
    let mut failures = Vec::new();
    for source_path in sources {
        println!("cargo:rerun-if-changed={}", source_path.display());
        let Ok(source) = std::fs::read_to_string(&source_path) else {
            failures.push(RsxFailure {
                path: source_path.clone(),
                errors: vec![RsxError { line: 1, column: 1, message: "cannot be read".to_owned() }],
            });
            continue;
        };
        let options = CompileOptions {
            source_path: source_path.clone(),
            src_root: src.to_path_buf(),
            wrapper: None,
        };
        match compile(&source, &options) {
            Ok(output) => {
                let relative =
                    source_path.strip_prefix(src).unwrap_or(&source_path).with_extension("rs");
                let target = out.join(relative);
                if let Some(parent) = target.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                // Unchanged output is not rewritten, so `rustc` does not
                // see a newer file and rebuild for nothing.
                if std::fs::read_to_string(&target).ok().as_deref() != Some(output.code.as_str()) {
                    let _ = std::fs::write(&target, &output.code);
                }
                let map_path = PathBuf::from(format!("{}.map", target.display()));
                let _ = std::fs::write(map_path, output.map.to_json());
                written.push(target);
            }
            Err(errors) => failures.push(RsxFailure { path: source_path, errors }),
        }
    }
    if failures.is_empty() { Ok(written) } else { Err(failures) }
}

fn collect(directory: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, into);
        } else if path.extension().is_some_and(|extension| extension == "rsx") {
            into.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_of_rsx_files_is_mirrored_with_source_maps() {
        let root = std::env::temp_dir().join(format!("rustnative-rsx-{}", std::process::id()));
        let src = root.join("src");
        std::fs::create_dir_all(src.join("screens")).expect("dirs");
        std::fs::write(src.join("app.rsx"), "fn f() -> Node { <Label key=\"a\" text=\"b\" /> }\n")
            .expect("write");
        std::fs::write(src.join("screens/home.rsx"), "fn g() -> u8 { 1 < 2; 3 }\n").expect("write");
        std::fs::write(src.join("plain.rs"), "fn h() {}\n").expect("write");
        let out = root.join("out");
        let written = compile_tree(&src, &out).expect("compiles");
        assert_eq!(written.len(), 2);
        let app = std::fs::read_to_string(out.join("app.rs")).expect("lowered");
        assert!(app.contains("::rustnative_core::rsx!(<Label"));
        assert!(out.join("screens/home.rs.map").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_broken_file_is_reported_by_position() {
        let root = std::env::temp_dir().join(format!("rustnative-rsx-bad-{}", std::process::id()));
        let src = root.join("src");
        std::fs::create_dir_all(&src).expect("dirs");
        std::fs::write(src.join("bad.rsx"), "fn f() -> Node {\n    <Screen key=\"s\" />\n}\n")
            .expect("write");
        let failures = compile_tree(&src, &root.join("out")).expect_err("no context");
        let text = failures[0].to_string();
        assert!(text.contains("bad.rsx:2:5:"), "{text}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
