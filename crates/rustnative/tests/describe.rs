//! The machine-readable description (`PLAN.md` Milestone 52): the
//! committed `docs/api/framework.json` is what the tool generates, and its
//! event list is the `Event` enum's.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::path::Path;
use std::process::Command;

fn workspace() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).unwrap()
}

#[test]
fn the_committed_description_is_current() {
    let output = Command::new(env!("CARGO_BIN_EXE_rustnative"))
        .args(["describe", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let generated = String::from_utf8(output.stdout).unwrap();
    let path = workspace().join("docs/api/framework.json");
    let committed = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert!(
        committed == generated,
        "docs/api/framework.json is stale: `rustnative describe --json > docs/api/framework.json`"
    );
    let description: serde_json::Value = serde_json::from_str(&generated).unwrap();
    let column = description["elements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|element| element["name"] == "Column")
        .unwrap();
    assert!(
        column["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|attribute| attribute["builder"] == "ColumnStyle::gap"),
        "each attribute names the builder method it calls"
    );
    assert!(
        description["utilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|utility| utility["class"] == "p-4"),
        "the utility vocabulary is listed"
    );
}

#[test]
fn the_event_list_is_the_enums() {
    let source =
        std::fs::read_to_string(workspace().join("crates/rustnative-core/src/event.rs")).unwrap();
    let file = syn::parse_file(&source).unwrap();
    let variants: Vec<String> = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Enum(item) if item.ident == "Event" => {
                Some(item.variants.iter().map(|variant| variant.ident.to_string()).collect())
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(variants, rustnative_core::event::EVENT_NAMES);
}
