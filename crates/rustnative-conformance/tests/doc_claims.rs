//! No performance claim without a number behind it (`PLAN.md` Milestone
//! 42): wherever the project's documentation — `README.md` and `docs/` —
//! calls something fast, light, small, or quick, the same paragraph points at
//! the budget file that holds the number (`budgets/`).
//!
//! The surveys of other frameworks (`docs/ecosystem-analysis`) and the
//! working plans (`docs/superpowers`) describe other projects and intentions,
//! not this one's performance, and are not held to it. A phrase that names
//! a mechanism rather than making a claim (the "transient fast path") is
//! not one either.

use std::path::{Path, PathBuf};

/// Words that claim performance.
const CLAIMS: [&str; 14] = [
    "faster",
    "fastest",
    "blazing",
    "lightweight",
    "light-weight",
    "snappy",
    "tiny footprint",
    "small footprint",
    "low latency",
    "low-latency",
    "instant startup",
    "starts instantly",
    "efficient",
    "high-performance",
];

/// Phrases that contain a claim word but name a mechanism.
const MECHANISMS: [&str; 1] = ["fast path"];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

fn markdown_under(directory: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if name != "ecosystem-analysis" && name != "superpowers" {
                markdown_under(&path, into);
            }
        } else if path.extension().is_some_and(|extension| extension == "md") {
            into.push(path);
        }
    }
}

/// The paragraphs of `text` that make a claim without citing a budget.
fn unsupported_claims(text: &str) -> Vec<String> {
    text.split("\n\n")
        .filter(|paragraph| {
            let lower = paragraph.to_lowercase();
            let mut claimed = lower.clone();
            for mechanism in MECHANISMS {
                claimed = claimed.replace(mechanism, "");
            }
            CLAIMS.iter().any(|claim| claimed.contains(claim)) && !lower.contains("budgets/")
        })
        .map(|paragraph| paragraph.lines().next().unwrap_or_default().to_owned())
        .collect()
}

#[test]
fn every_performance_claim_cites_a_budget() {
    let root = workspace();
    let mut documents = vec![root.join("README.md")];
    markdown_under(&root.join("docs"), &mut documents);
    let mut problems = Vec::new();
    for document in documents {
        let text = std::fs::read_to_string(&document).unwrap_or_default();
        for claim in unsupported_claims(&text) {
            problems.push(format!("  {}: {claim}", document.display()));
        }
    }
    assert!(
        problems.is_empty(),
        "performance claims with no budget behind them (cite `budgets/<target>.toml`, or say it \
         with the number):\n{}",
        problems.join("\n")
    );
}

#[test]
fn the_check_finds_a_claim_and_accepts_a_cited_one() {
    assert_eq!(unsupported_claims("Intro.\n\nIt is blazing fast.\n"), ["It is blazing fast."]);
    assert!(unsupported_claims("Cold start is faster: `budgets/windows.toml`.").is_empty());
    assert!(unsupported_claims("The transient fast path renders nothing.").is_empty());
}
