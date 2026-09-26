//! Capability packages, feature kits, and codemods from the command line
//! (`PLAN.md` Milestone 52): `add` checks a package before adding it,
//! `search` reads the index, `generate kit` writes the kits, and `upgrade`
//! carries a project across a breaking change.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn project(name: &str, cargo: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("rustnative-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("rustnative.toml"),
        "[app]\nname = \"demo\"\nid = \"dev.rustnative.demo\"\ndisplay-name = \"Demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(root.join("Cargo.toml"), cargo).unwrap();
    std::fs::write(root.join("src/lib.rs"), "//! Demo.\n").unwrap();
    root
}

fn run(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rustnative"))
        .current_dir(root)
        .args(arguments)
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

const CARGO: &str = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[dependencies]\nrustnative-core = \"0.1\"\n";

#[test]
fn a_package_is_checked_before_it_is_added() {
    let root = project("add", CARGO);
    let battery = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/package-battery");
    let added = run(&root, &["add", battery.to_str().unwrap()]);
    assert!(added.status.success(), "{}", text(&added));
    let cargo = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(cargo.contains("package-battery = { path ="), "{cargo}");
    assert!(
        text(&run(&root, &["add", battery.to_str().unwrap()])).contains("already a dependency")
    );

    // A package with no Windows code is refused, with the reason.
    let foreign = root.join("foreign");
    std::fs::create_dir_all(&foreign).unwrap();
    std::fs::write(
        foreign.join("Cargo.toml"),
        "[package]\nname = \"android-only\"\nversion = \"1.0.0\"\n\n[package.metadata.rustnative]\ncontract = \"X\"\nbackends = [\"android\"]\nframework = \"0.1\"\n",
    )
    .unwrap();
    let refused = run(&root, &["add", foreign.to_str().unwrap()]);
    assert!(
        !refused.status.success() && text(&refused).contains("no code for the windows backend"),
        "{}",
        text(&refused)
    );

    let found = run(&root, &["search", "battery"]);
    assert!(text(&found).contains("package-battery 0.1.0"), "{}", text(&found));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn kits_are_written_in_order_on_the_server_model() {
    let plain = project("kit-plain", CARGO);
    assert!(
        text(&run(&plain, &["generate", "kit", "auth"])).contains("rustnative-server"),
        "needs the server model"
    );
    let root = project("kits", &format!("{CARGO}rustnative-server = \"0.1\"\n"));
    assert!(
        text(&run(&root, &["generate", "kit", "admin"])).contains("kit auth"),
        "admin builds on auth"
    );
    assert!(run(&root, &["generate", "kit", "auth"]).status.success());
    assert!(run(&root, &["generate", "kit", "admin"]).status.success());
    assert!(run(&root, &["generate", "kit", "commerce"]).status.success());
    let listing = std::fs::read_to_string(root.join("src/kits/mod.rs")).unwrap();
    assert!(listing.contains("pub mod auth;\npub mod admin;\npub mod commerce;"), "{listing}");
    assert!(std::fs::read_to_string(root.join("src/lib.rs")).unwrap().contains("pub mod kits;"));
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(plain);
}

#[test]
fn upgrade_rewrites_in_place_and_reports_the_rest() {
    let root = project("upgrade", CARGO);
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/codemod-corpus");
    std::fs::copy(corpus.join("struct-literals/input.rs"), root.join("src/insets.rs")).unwrap();
    std::fs::copy(corpus.join("leave-alone/input.rs"), root.join("src/other.rs")).unwrap();

    let dry = run(&root, &["upgrade", "--from", "0.0", "--dry-run"]);
    assert!(text(&dry).contains("(dry run)"), "{}", text(&dry));
    assert_eq!(
        std::fs::read_to_string(root.join("src/insets.rs")).unwrap(),
        std::fs::read_to_string(corpus.join("struct-literals/input.rs")).unwrap(),
        "a dry run writes nothing"
    );
    let upgraded = run(&root, &["upgrade", "--from", "0.0"]);
    assert!(upgraded.status.success(), "{}", text(&upgraded));
    assert_eq!(
        std::fs::read_to_string(root.join("src/insets.rs")).unwrap(),
        std::fs::read_to_string(corpus.join("struct-literals/expected.rs")).unwrap()
    );
    assert!(text(&upgraded).contains("by hand"), "uncertain places are reported");
    assert!(text(&run(&root, &["upgrade", "--from", "0.1"])).contains("nothing to do"));
    let _ = std::fs::remove_dir_all(root);
}
