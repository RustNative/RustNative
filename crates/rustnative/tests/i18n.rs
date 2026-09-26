//! `rustnative i18n` on a copy of `examples/i18n-demo` (`PLAN.md` Milestone
//! 46): lint finds literal text in builder calls and in markup, extract
//! adds the messages the code uses, merge brings translations up to the
//! source, and show finds a message's uses.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn rustnative(directory: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rustnative"))
        .current_dir(directory)
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A copy of the example's project files (no build).
fn project() -> PathBuf {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/i18n-demo");
    let copy = std::env::temp_dir().join(format!("rustnative-i18n-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&copy);
    for folder in ["src", "locales"] {
        std::fs::create_dir_all(copy.join(folder)).unwrap();
        for entry in std::fs::read_dir(example.join(folder)).unwrap().flatten() {
            std::fs::copy(entry.path(), copy.join(folder).join(entry.file_name())).unwrap();
        }
    }
    std::fs::copy(example.join("rustnative.toml"), copy.join("rustnative.toml")).unwrap();
    copy
}

#[test]
fn the_translator_workflow_reads_both_syntaxes() {
    let root = project();
    let lint = rustnative(&root, &["i18n", "lint"]);
    assert!(lint.status.success(), "the example has no literals: {}", stdout(&lint));

    // A literal in a builder call and one in markup.
    let lib = root.join("src/lib.rs");
    let mut text = std::fs::read_to_string(&lib).unwrap();
    text.push_str("\npub fn stray() -> rustnative_core::Node {\n    rustnative_core::Node::label(\"k\", \"Forgotten text\")\n}\n");
    text.push_str("\npub fn marked() -> rustnative_core::Node {\n    rustnative_core::rsx! { <Label key=\"m\" text=\"Also forgotten\" /> }\n}\n");
    text.push_str(
        "\npub fn fresh() -> rustnative_core::i18n::Message {\n    messages::brand_new()\n}\n",
    );
    std::fs::write(&lib, text).unwrap();
    let lint = rustnative(&root, &["i18n", "lint"]);
    assert!(!lint.status.success());
    let found = stdout(&lint);
    assert!(
        found.contains("\"Forgotten text\"") && found.contains("\"Also forgotten\""),
        "{found}"
    );

    let extract = rustnative(&root, &["i18n", "extract"]);
    assert!(extract.status.success(), "{}", String::from_utf8_lossy(&extract.stderr));
    let english = std::fs::read_to_string(root.join("locales/en.ftl")).unwrap();
    assert!(english.contains("# Used in src"), "{english}");
    assert!(
        english.contains("brand-new = brand-new") || english.contains("brand_new = brand_new"),
        "{english}"
    );

    let merge = rustnative(&root, &["i18n", "merge"]);
    assert!(merge.status.success());
    let polish = std::fs::read_to_string(root.join("locales/pl.ftl")).unwrap();
    assert!(polish.contains("# needs-translation"), "{polish}");

    let show = rustnative(&root, &["i18n", "show", "inbox-count"]);
    let shown = stdout(&show);
    assert!(shown.contains("src/lib.rs:") || shown.contains("src\\lib.rs:"), "{shown}");
    assert!(shown.contains("ar: translated") && shown.contains("pl: translated"), "{shown}");
    let _ = std::fs::remove_dir_all(root);
}
