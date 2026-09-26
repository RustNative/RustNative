//! The tree on the wire (`rustnative_core::wire`): every syntax-equivalence
//! case survives the trip to JSON and back, except what the wire format
//! documents it does not carry — compiled style, input interest,
//! transitions, and a canvas's drawing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use rustnative_conformance::builder_cases;
use rustnative_core::wire::WireNode;

/// Cases whose point is something the wire does not carry.
const NOT_CARRIED: &[&str] =
    &["canvas", "class", "style", "style_declarations", "input", "transition", "spread"];

#[test]
fn every_case_round_trips() {
    let mut checked = 0;
    for (name, node) in builder_cases::cases() {
        if NOT_CARRIED.contains(&name) {
            continue;
        }
        let json = serde_json::to_string(&WireNode::from_node(&node)).unwrap();
        let back = serde_json::from_str::<WireNode>(&json).unwrap().into_node();
        assert_eq!(back, node, "`{name}` changed on the wire: {json}");
        checked += 1;
    }
    assert!(checked > 30, "the suite covers the grammar ({checked} cases)");
}
